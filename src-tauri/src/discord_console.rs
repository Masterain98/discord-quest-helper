//! A console owns one verified renderer connection. User expressions and results
//! never pass through the application's diagnostic logger.
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type Reply = oneshot::Sender<Result<Value, String>>;
type EventSink = Arc<dyn Fn(ConsoleEvent) -> bool + Send + Sync>;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const INVALIDATED: &str = "console_session_invalidated";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConsoleEvent {
    session_id: String,
    sequence: u64,
    timestamp: f64,
    kind: &'static str,
    level: String,
    args: Vec<Value>,
    exception: Option<Value>,
    message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConsoleSessionInfo {
    pub session_id: String,
    pub target_title: String,
    pub target_url: String,
    pub port: u16,
}

enum Operation {
    Evaluate(String),
    Properties(String),
    Release(Vec<String>),
    Close,
}
struct Request {
    operation: Operation,
    reply: Reply,
}
struct Pending {
    reply: Reply,
    deadline: tokio::time::Instant,
    capture_objects: bool,
    evaluation: bool,
}

struct Session {
    info: ConsoleSessionInfo,
    open: Arc<AtomicBool>,
    requests: mpsc::Sender<Request>,
}

#[derive(Default)]
pub(crate) struct ConsoleManager {
    session: Mutex<Option<Arc<Session>>>,
    open_gate: Mutex<()>,
}

impl ConsoleManager {
    pub async fn open(&self, port: u16, sink: EventSink) -> Result<ConsoleSessionInfo, String> {
        let _gate = self.open_gate.lock().await;
        self.close_all().await;
        let verified = crate::cdp_client::verify_primary_discord_target(port)
            .await
            .map_err(|_| "console_target_unavailable".to_string())?;
        let ws_url = verified
            .target
            .web_socket_debugger_url
            .as_deref()
            .ok_or("console_target_unavailable")?;
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), connect_async(ws_url))
            .await
            .map_err(|_| "console_connect_timeout")?
            .map_err(|_| "console_connect_failed")?;
        let (context, buffered) = tokio::time::timeout(
            Duration::from_secs(10),
            initialize(&mut socket, &verified.generation),
        )
        .await
        .map_err(|_| "console_connect_timeout")??;
        let info = ConsoleSessionInfo {
            session_id: uuid::Uuid::new_v4().to_string(),
            target_title: verified.target.title,
            target_url: verified.target.url,
            port,
        };
        let (requests, receiver) = mpsc::channel(64);
        let open = Arc::new(AtomicBool::new(true));
        let session = Arc::new(Session {
            info: info.clone(),
            open: open.clone(),
            requests,
        });
        *self.session.lock().await = Some(session);
        tokio::spawn(run_session(
            socket,
            receiver,
            SessionRuntime {
                session: info.session_id.clone(),
                context,
                buffered,
                sink,
                open,
                request_timeout: REQUEST_TIMEOUT,
            },
        ));
        Ok(info)
    }

    async fn request(&self, id: &str, operation: Operation) -> Result<Value, String> {
        let session = self
            .session
            .lock()
            .await
            .clone()
            .filter(|s| s.info.session_id == id && s.open.load(Ordering::SeqCst))
            .ok_or(INVALIDATED)?;
        let (reply, result) = oneshot::channel();
        tokio::time::timeout(
            Duration::from_secs(2),
            session.requests.send(Request { operation, reply }),
        )
        .await
        .map_err(|_| "console_busy")?
        .map_err(|_| INVALIDATED)?;
        tokio::time::timeout(REQUEST_TIMEOUT + Duration::from_secs(2), result)
            .await
            .map_err(|_| "console_request_timeout")?
            .map_err(|_| INVALIDATED)?
    }

    pub async fn evaluate(&self, id: &str, expression: String) -> Result<Value, String> {
        if expression.trim().is_empty() {
            return Err("console_empty_expression".into());
        }
        self.request(id, Operation::Evaluate(expression)).await
    }
    pub async fn properties(&self, id: &str, object: String) -> Result<Value, String> {
        self.request(id, Operation::Properties(object)).await
    }
    pub async fn release(&self, id: &str, objects: Vec<String>) -> Result<(), String> {
        self.request(id, Operation::Release(objects))
            .await
            .map(|_| ())
    }
    pub async fn close(&self, id: &str) {
        let session = {
            let mut slot = self.session.lock().await;
            if slot.as_ref().is_some_and(|s| s.info.session_id == id) {
                slot.take()
            } else {
                None
            }
        };
        if let Some(session) = session {
            stop(session).await;
        }
    }
    pub async fn close_all(&self) {
        let session = self.session.lock().await.take();
        if let Some(session) = session {
            stop(session).await;
        }
    }
}

async fn stop(session: Arc<Session>) {
    session.open.store(false, Ordering::SeqCst);
    let (reply, result) = oneshot::channel();
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        let _ = session
            .requests
            .send(Request {
                operation: Operation::Close,
                reply,
            })
            .await;
        let _ = result.await;
    })
    .await;
}

struct Context {
    id: i64,
    unique_id: String,
}

async fn exchange(
    socket: &mut Socket,
    id: u64,
    method: &str,
    params: Value,
    events: &mut Vec<Value>,
) -> Result<Value, String> {
    socket
        .send(Message::Text(
            json!({"id":id,"method":method,"params":params})
                .to_string()
                .into(),
        ))
        .await
        .map_err(|_| "console_connect_failed")?;
    while let Some(message) = socket.next().await {
        match message {
            Ok(Message::Text(text)) => {
                let value: Value =
                    serde_json::from_str(&text).map_err(|_| "console_protocol_error")?;
                if value["id"].as_u64() == Some(id) {
                    if value.get("error").is_some() {
                        return Err("console_protocol_unsupported".into());
                    }
                    return Ok(value["result"].clone());
                }
                if value.get("method").is_some() {
                    events.push(value);
                }
            }
            Ok(Message::Ping(payload)) => {
                socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|_| INVALIDATED)?;
            }
            Ok(Message::Close(_)) | Err(_) => return Err(INVALIDATED.into()),
            _ => {}
        }
    }
    Err(INVALIDATED.into())
}

async fn initialize(
    socket: &mut Socket,
    generation: &str,
) -> Result<(Context, Vec<Value>), String> {
    let mut events = Vec::new();
    let tree = exchange(socket, 1, "Page.getFrameTree", json!({}), &mut events).await?;
    let frame = tree
        .pointer("/frameTree/frame/id")
        .and_then(Value::as_str)
        .ok_or("console_context_unavailable")?
        .to_owned();
    exchange(socket, 2, "Runtime.enable", json!({}), &mut events).await?;
    let context = events
        .iter()
        .rev()
        .filter(|e| e["method"] == "Runtime.executionContextCreated")
        .map(|e| &e["params"]["context"])
        .find(|c| c["auxData"]["isDefault"] == true && c["auxData"]["frameId"] == frame)
        .ok_or("console_context_unavailable")?;
    let context = Context {
        id: context["id"]
            .as_i64()
            .ok_or("console_context_unavailable")?,
        unique_id: context["uniqueId"]
            .as_str()
            .ok_or("console_protocol_unsupported")?
            .into(),
    };
    let result = exchange(
        socket,
        3,
        "Runtime.evaluate",
        json!({
            "expression":"String(performance.timeOrigin)", "uniqueContextId":context.unique_id,
            "returnByValue":true, "timeout":2000
        }),
        &mut events,
    )
    .await?;
    if result.pointer("/result/value").and_then(Value::as_str) != Some(generation) {
        return Err(INVALIDATED.into());
    }
    Ok((context, events))
}

fn collect_objects(value: &Value, objects: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(id) = map.get("objectId").and_then(Value::as_str) {
                objects.insert(id.into());
            }
            for (key, child) in map {
                if key != "value" || !map.contains_key("type") {
                    collect_objects(child, objects);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_objects(child, objects);
            }
        }
        _ => {}
    }
}

fn invalidates(value: &Value, context: &Context) -> bool {
    match value["method"].as_str() {
        Some("Runtime.executionContextsCleared" | "Inspector.detached") => true,
        Some("Runtime.executionContextDestroyed") => {
            value["params"]["executionContextId"].as_i64() == Some(context.id)
        }
        _ => false,
    }
}

fn console_event(
    value: &Value,
    session: &str,
    sequence: u64,
    context: &Context,
) -> Option<ConsoleEvent> {
    let params = &value["params"];
    let (kind, level, args, exception) = match value["method"].as_str()? {
        "Runtime.consoleAPICalled" if params["executionContextId"].as_i64() == Some(context.id) => {
            let level = match params["type"].as_str().unwrap_or("log") {
                "warning" => "warn",
                "error" | "assert" => "error",
                "debug" => "debug",
                "info" => "info",
                _ => "log",
            };
            (
                "console",
                level,
                params["args"].as_array().cloned().unwrap_or_default(),
                None,
            )
        }
        "Runtime.exceptionThrown"
            if params["exceptionDetails"]["executionContextId"].as_i64() == Some(context.id) =>
        {
            (
                "exception",
                "error",
                Vec::new(),
                Some(params["exceptionDetails"].clone()),
            )
        }
        _ => return None,
    };
    Some(ConsoleEvent {
        session_id: session.into(),
        sequence,
        timestamp: params["timestamp"]
            .as_f64()
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as f64),
        kind,
        level: level.into(),
        args,
        exception,
        message: None,
    })
}

struct SessionRuntime {
    session: String,
    context: Context,
    buffered: Vec<Value>,
    sink: EventSink,
    open: Arc<AtomicBool>,
    request_timeout: Duration,
}

async fn run_session(
    mut socket: Socket,
    mut requests: mpsc::Receiver<Request>,
    runtime: SessionRuntime,
) {
    let SessionRuntime {
        session,
        context,
        buffered,
        sink,
        open,
        request_timeout,
    } = runtime;
    let mut objects = HashSet::new();
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut next_id = 4u64;
    let mut sequence = 0u64;
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    let group = format!("discord-console-{session}");
    let mut reason = INVALIDATED;
    for value in buffered {
        if invalidates(&value, &context) {
            open.store(false, Ordering::SeqCst);
            break;
        }
        if let Some(event) = console_event(&value, &session, sequence + 1, &context) {
            sequence += 1;
            collect_objects(
                &json!({"args":event.args,"exception":event.exception}),
                &mut objects,
            );
            if !sink(event) {
                open.store(false, Ordering::SeqCst);
                break;
            }
        }
    }
    while open.load(Ordering::SeqCst) {
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { break; };
                let (method, params, capture_objects) = match request.operation {
                    Operation::Close => { let _ = request.reply.send(Ok(Value::Null)); break; }
                    Operation::Evaluate(expression) => {
                        if pending.values().any(|p| p.evaluation) {
                            let _ = request.reply.send(Err("console_busy".into()));
                            continue;
                        }
                        ("Runtime.evaluate", json!({"expression":expression,"uniqueContextId":context.unique_id,
                            "objectGroup":group,"returnByValue":false,"generatePreview":true,
                            "awaitPromise":true,"replMode":true,"includeCommandLineAPI":true,"silent":true,
                            "timeout":30000}), true)
                    }
                    Operation::Properties(id) => {
                        if !objects.contains(&id) { let _ = request.reply.send(Err("console_object_expired".into())); continue; }
                        ("Runtime.getProperties", json!({"objectId":id,"ownProperties":true,"generatePreview":true}), true)
                    }
                    Operation::Release(ids) => {
                        let mut failed = false;
                        for id in ids {
                            if objects.remove(&id) {
                                let wire = json!({"id":next_id,"method":"Runtime.releaseObject","params":{"objectId":id}});
                                next_id += 1;
                                if !send_wire(&mut socket, wire).await { failed = true; break; }
                            }
                        }
                        let _ = request.reply.send(if failed { Err(INVALIDATED.into()) } else { Ok(Value::Null) });
                        if failed { break; } else { continue; }
                    }
                };
                let id = next_id;
                next_id += 1;
                pending.insert(id, Pending { reply:request.reply, deadline:tokio::time::Instant::now() + request_timeout, capture_objects, evaluation: method == "Runtime.evaluate" });
                if !send_wire(&mut socket, json!({"id":id,"method":method,"params":params})).await { break; }
            }
            message = socket.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        let Ok(value) = serde_json::from_str::<Value>(&text) else { reason = "console_protocol_error"; break; };
                        if let Some(id) = value["id"].as_u64() {
                            if let Some(request) = pending.remove(&id) {
                                if request.capture_objects { collect_objects(&value["result"], &mut objects); }
                                let result = if value.get("error").is_some() { Err("console_protocol_error".into()) } else { Ok(value["result"].clone()) };
                                let _ = request.reply.send(result);
                            } else {
                                // An evaluation can resolve after the UI deadline.
                                // Its result is never replayed; release its handles.
                                let mut late = HashSet::new();
                                collect_objects(&value["result"], &mut late);
                                for object in late {
                                    let wire = json!({"id":next_id,"method":"Runtime.releaseObject","params":{"objectId":object}});
                                    next_id += 1;
                                    if !send_wire(&mut socket, wire).await { open.store(false, Ordering::SeqCst); break; }
                                }
                            }
                        } else {
                            if invalidates(&value, &context) { break; }
                            if let Some(event) = console_event(&value, &session, sequence + 1, &context) {
                                sequence += 1;
                                collect_objects(&json!({"args":event.args,"exception":event.exception}), &mut objects);
                                if !sink(event) { break; }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if !matches!(tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Pong(payload))).await, Ok(Ok(()))) { break; }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            _ = ticker.tick() => {
                let expired: Vec<_> = pending.iter().filter(|(_,p)| p.deadline <= tokio::time::Instant::now()).map(|(id,_)| *id).collect();
                for id in expired {
                    if let Some(request) = pending.remove(&id) { let _ = request.reply.send(Err("console_request_timeout".into())); }
                }
            }
        }
    }
    open.store(false, Ordering::SeqCst);
    for (_, request) in pending {
        let _ = request.reply.send(Err(INVALIDATED.into()));
    }
    requests.close();
    while let Some(request) = requests.recv().await {
        let _ = request.reply.send(Err(INVALIDATED.into()));
    }
    let _ = sink(ConsoleEvent {
        session_id: session,
        sequence: sequence + 1,
        timestamp: chrono::Utc::now().timestamp_millis() as f64,
        kind: "disconnected",
        level: "warn".into(),
        args: Vec::new(),
        exception: None,
        message: Some(reason.into()),
    });
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        socket.send(Message::Text(json!({"id":next_id,"method":"Runtime.releaseObjectGroup","params":{"objectGroup":group}}).to_string().into())).await?;
        for object in objects {
            next_id += 1;
            socket.send(Message::Text(json!({"id":next_id,"method":"Runtime.releaseObject","params":{"objectId":object}}).to_string().into())).await?;
        }
        socket.close(None).await
    }).await;
}

async fn send_wire(socket: &mut Socket, value: Value) -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            socket.send(Message::Text(value.to_string().into()))
        )
        .await,
        Ok(Ok(()))
    )
}

#[tauri::command]
pub(crate) async fn open_discord_console(
    port: u16,
    on_event: tauri::ipc::Channel<ConsoleEvent>,
    manager: tauri::State<'_, Arc<ConsoleManager>>,
) -> Result<ConsoleSessionInfo, String> {
    manager
        .open(port, Arc::new(move |event| on_event.send(event).is_ok()))
        .await
}
#[tauri::command]
pub(crate) async fn evaluate_discord_console(
    session_id: String,
    expression: String,
    manager: tauri::State<'_, Arc<ConsoleManager>>,
) -> Result<Value, String> {
    manager.evaluate(&session_id, expression).await
}
#[tauri::command]
pub(crate) async fn get_discord_console_properties(
    session_id: String,
    object_id: String,
    manager: tauri::State<'_, Arc<ConsoleManager>>,
) -> Result<Value, String> {
    manager.properties(&session_id, object_id).await
}
#[tauri::command]
pub(crate) async fn release_discord_console_objects(
    session_id: String,
    object_ids: Vec<String>,
    manager: tauri::State<'_, Arc<ConsoleManager>>,
) -> Result<(), String> {
    manager.release(&session_id, object_ids).await
}
#[tauri::command]
pub(crate) async fn close_discord_console(
    session_id: String,
    manager: tauri::State<'_, Arc<ConsoleManager>>,
) -> Result<(), String> {
    manager.close(&session_id).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};
    type Peer = WebSocketStream<TcpStream>;

    async fn sockets() -> (Socket, Peer) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            tokio_tungstenite::accept_async(listener.accept().await.unwrap().0)
                .await
                .unwrap()
        });
        let (client, _) = connect_async(format!("ws://{address}")).await.unwrap();
        (client, server.await.unwrap())
    }
    async fn wire(peer: &mut Peer) -> Value {
        loop {
            match tokio::time::timeout(Duration::from_secs(3), peer.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
            {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Ping(p) => peer.send(Message::Pong(p)).await.unwrap(),
                _ => {}
            }
        }
    }
    async fn send(peer: &mut Peer, value: Value) {
        peer.send(Message::Text(value.to_string().into()))
            .await
            .unwrap();
    }
    async fn reply(peer: &mut Peer, request: &Value, result: Value) {
        send(peer, json!({"id":request["id"],"result":result})).await;
    }
    fn context() -> Context {
        Context {
            id: 7,
            unique_id: "unique-main".into(),
        }
    }
    async fn harness(
        timeout: Duration,
    ) -> (
        Arc<ConsoleManager>,
        Peer,
        mpsc::UnboundedReceiver<ConsoleEvent>,
    ) {
        let (client, peer) = sockets().await;
        let manager = Arc::new(ConsoleManager::default());
        let info = ConsoleSessionInfo {
            session_id: "test".into(),
            port: 9223,
            target_title: "Discord".into(),
            target_url: "https://discord.com".into(),
        };
        let open = Arc::new(AtomicBool::new(true));
        let (requests, receiver) = mpsc::channel(64);
        let (events, sink) = mpsc::unbounded_channel();
        *manager.session.lock().await = Some(Arc::new(Session {
            info,
            open: open.clone(),
            requests,
        }));
        tokio::spawn(run_session(
            client,
            receiver,
            SessionRuntime {
                session: "test".into(),
                context: context(),
                buffered: Vec::new(),
                sink: Arc::new(move |event| events.send(event).is_ok()),
                open,
                request_timeout: timeout,
            },
        ));
        (manager, peer, sink)
    }

    #[tokio::test]
    async fn initializes_only_the_verified_main_default_context() {
        let (mut client, mut peer) = sockets().await;
        let init = tokio::spawn(async move { initialize(&mut client, "100").await });
        let request = wire(&mut peer).await;
        assert_eq!(request["method"], "Page.getFrameTree");
        reply(
            &mut peer,
            &request,
            json!({"frameTree":{"frame":{"id":"main"}}}),
        )
        .await;
        let request = wire(&mut peer).await;
        assert_eq!(request["method"], "Runtime.enable");
        for (id, frame) in [(7, "main"), (8, "iframe")] {
            send(&mut peer, json!({"method":"Runtime.executionContextCreated","params":{"context":{"id":id,"uniqueId":format!("unique-{frame}"),"auxData":{"isDefault":true,"frameId":frame}}}})).await;
        }
        reply(&mut peer, &request, json!({})).await;
        let request = wire(&mut peer).await;
        assert_eq!(request["params"]["uniqueContextId"], "unique-main");
        reply(
            &mut peer,
            &request,
            json!({"result":{"type":"string","value":"100"}}),
        )
        .await;
        assert_eq!(init.await.unwrap().unwrap().0.id, 7);
    }

    #[tokio::test]
    async fn multiplexes_live_logs_and_evaluation_without_wrapping_expressions() {
        let (manager, mut peer, mut events) = harness(REQUEST_TIMEOUT).await;
        let worker = manager.clone();
        let expression = "let example = 1;\nawait Promise.resolve(example + 1)";
        let execution =
            tokio::spawn(async move { worker.evaluate("test", expression.into()).await });
        let request = wire(&mut peer).await;
        assert_eq!(request["params"]["expression"], expression);
        assert_eq!(request["params"]["uniqueContextId"], "unique-main");
        assert_eq!(request["params"]["replMode"], true);
        assert_eq!(request["params"]["awaitPromise"], true);
        assert_eq!(request["params"]["returnByValue"], false);
        send(&mut peer, json!({"method":"Runtime.consoleAPICalled","params":{"executionContextId":7,"type":"warning","timestamp":123,"args":[{"type":"string","value":"during"}]}})).await;
        let event = events.recv().await.unwrap();
        assert_eq!(event.level, "warn");
        assert_eq!(event.args[0]["value"], "during");
        assert!(!execution.is_finished());
        assert_eq!(
            manager
                .evaluate("test", "second()".into())
                .await
                .unwrap_err(),
            "console_busy"
        );
        reply(
            &mut peer,
            &request,
            json!({"result":{"type":"number","value":2}}),
        )
        .await;
        assert_eq!(execution.await.unwrap().unwrap()["result"]["value"], 2);
        manager.close("test").await;
        manager.close("test").await;
    }

    #[tokio::test]
    async fn preserves_special_values_and_exception_details() {
        let (manager, mut peer, _) = harness(REQUEST_TIMEOUT).await;
        let responses = vec![
            json!({"result":{"type":"undefined"}}),
            json!({"result":{"type":"object","subtype":"null","value":null}}),
            json!({"result":{"type":"number","unserializableValue":"NaN"}}),
            json!({"result":{"type":"bigint","unserializableValue":"42n"}}),
            json!({"result":{"type":"object","objectId":"error"},"exceptionDetails":{"text":"Uncaught","exception":{"type":"object","description":"Error: example","objectId":"error"}}}),
        ];
        for expected in responses {
            let worker = manager.clone();
            let execution =
                tokio::spawn(async move { worker.evaluate("test", "test".into()).await });
            let request = wire(&mut peer).await;
            reply(&mut peer, &request, expected.clone()).await;
            assert_eq!(execution.await.unwrap().unwrap(), expected);
        }
        manager.close_all().await;
    }

    #[tokio::test]
    async fn expands_descriptors_and_releases_owned_handles() {
        let (manager, mut peer, _) = harness(REQUEST_TIMEOUT).await;
        let worker = manager.clone();
        let execution = tokio::spawn(async move { worker.evaluate("test", "object".into()).await });
        let request = wire(&mut peer).await;
        reply(
            &mut peer,
            &request,
            json!({"result":{"type":"object","objectId":"root"}}),
        )
        .await;
        execution.await.unwrap().unwrap();
        let worker = manager.clone();
        let expansion = tokio::spawn(async move { worker.properties("test", "root".into()).await });
        let request = wire(&mut peer).await;
        assert_eq!(request["method"], "Runtime.getProperties");
        assert_eq!(request["params"]["ownProperties"], true);
        reply(&mut peer, &request, json!({"result":[{"name":"child","value":{"type":"object","objectId":"child"}},{"name":"accessor","get":{"type":"function","objectId":"getter"}}]})).await;
        assert_eq!(
            expansion.await.unwrap().unwrap()["result"][1]["get"]["objectId"],
            "getter"
        );
        manager
            .release(
                "test",
                vec![
                    "root".into(),
                    "child".into(),
                    "getter".into(),
                    "foreign".into(),
                ],
            )
            .await
            .unwrap();
        let mut released = HashSet::new();
        for _ in 0..3 {
            let request = wire(&mut peer).await;
            assert_eq!(request["method"], "Runtime.releaseObject");
            released.insert(request["params"]["objectId"].as_str().unwrap().to_owned());
        }
        assert_eq!(
            released,
            HashSet::from(["root".into(), "child".into(), "getter".into()])
        );
        assert_eq!(
            manager.properties("test", "root".into()).await.unwrap_err(),
            "console_object_expired"
        );
        manager.close_all().await;
    }

    #[tokio::test]
    async fn invalidates_pending_commands_on_reload_and_reports_uncaught_exceptions() {
        let (manager, mut peer, mut events) = harness(REQUEST_TIMEOUT).await;
        send(&mut peer, json!({"method":"Runtime.exceptionThrown","params":{"timestamp":123,"exceptionDetails":{"executionContextId":7,"text":"Uncaught","exception":{"description":"Error: async"}}}})).await;
        assert_eq!(events.recv().await.unwrap().kind, "exception");
        let worker = manager.clone();
        let execution = tokio::spawn(async move { worker.evaluate("test", "slow()".into()).await });
        wire(&mut peer).await;
        send(
            &mut peer,
            json!({"method":"Runtime.executionContextsCleared","params":{}}),
        )
        .await;
        assert_eq!(execution.await.unwrap().unwrap_err(), INVALIDATED);
        assert_eq!(events.recv().await.unwrap().kind, "disconnected");
        assert_eq!(
            manager
                .evaluate("test", "again()".into())
                .await
                .unwrap_err(),
            INVALIDATED
        );
        manager.close_all().await;
    }

    #[tokio::test]
    async fn times_out_without_replaying_and_releases_late_results() {
        let (manager, mut peer, mut events) = harness(Duration::from_millis(120)).await;
        let worker = manager.clone();
        let execution =
            tokio::spawn(async move { worker.evaluate("test", "never()".into()).await });
        let request = wire(&mut peer).await;
        assert_eq!(
            execution.await.unwrap().unwrap_err(),
            "console_request_timeout"
        );
        reply(
            &mut peer,
            &request,
            json!({"result":{"type":"object","objectId":"late"}}),
        )
        .await;
        let release = wire(&mut peer).await;
        assert_eq!(release["method"], "Runtime.releaseObject");
        assert_eq!(release["params"]["objectId"], "late");
        send(&mut peer, json!({"method":"Runtime.consoleAPICalled","params":{"executionContextId":7,"type":"log","args":[{"type":"number","value":1}]}})).await;
        assert_eq!(events.recv().await.unwrap().args[0]["value"], 1);
        manager.close_all().await;
    }

    #[test]
    fn collects_protocol_objects_without_treating_user_values_as_handles() {
        let mut objects = HashSet::new();
        collect_objects(
            &json!({"result":[{"name":"child","value":{"type":"object","objectId":"child"}}],"other":{"type":"object","value":{"objectId":"user-data"}}}),
            &mut objects,
        );
        assert_eq!(objects, HashSet::from(["child".into()]));
    }

    /// Explicit opt-in: uses only disposable expressions in the local Discord
    /// renderer. No credentials, client stores, or existing variables are read.
    #[tokio::test]
    #[ignore = "requires an authorized local Discord CDP session on port 9223"]
    async fn live_console_expressions_and_objects() {
        let manager = ConsoleManager::default();
        let (events, mut observed) = mpsc::unbounded_channel();
        let info = manager
            .open(
                9223,
                Arc::new(move |event| {
                    // Keep only our own test output. Do not expose Discord background logs.
                    if event
                        .args
                        .iter()
                        .any(|arg| arg["value"] == "dqhc-console-test")
                    {
                        let _ = events.send(event);
                    }
                    true
                }),
            )
            .await
            .unwrap();
        let id = &info.session_id;
        assert_eq!(
            manager.evaluate(id, "1 + 1".into()).await.unwrap()["result"]["value"],
            2
        );
        let variable = format!("dqhc_{}", uuid::Uuid::new_v4().simple());
        manager
            .evaluate(id, format!("let {variable} = 40"))
            .await
            .unwrap();
        assert_eq!(
            manager
                .evaluate(id, format!("{variable} + 2"))
                .await
                .unwrap()["result"]["value"],
            42
        );
        assert_eq!(
            manager
                .evaluate(
                    id,
                    format!("let {variable} = 41;\nawait Promise.resolve({variable} + 1)")
                )
                .await
                .unwrap()["result"]["value"],
            42
        );
        manager.evaluate(id, "await new Promise(resolve => setTimeout(() => { console.log('dqhc-console-test'); resolve(42); }, 20))".into()).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), observed.recv())
                .await
                .unwrap()
                .is_some()
        );
        let result = manager.evaluate(id, "({ nested: { value: 42 }, get untouched() { throw new Error('getter must not run') } })".into()).await.unwrap();
        let object = result["result"]["objectId"].as_str().unwrap();
        let properties = manager.properties(id, object.into()).await.unwrap();
        assert!(properties["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "untouched" && p.get("get").is_some()));
        let child = properties["result"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "nested")
            .unwrap()["value"]["objectId"]
            .as_str()
            .unwrap();
        assert!(
            manager.properties(id, child.into()).await.unwrap()["result"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "value" && p["value"]["value"] == 42)
        );
        assert!(manager
            .evaluate(id, "throw new Error('dqhc test exception')".into())
            .await
            .unwrap()
            .get("exceptionDetails")
            .is_some());
        manager
            .evaluate(id, format!("{variable} = undefined"))
            .await
            .unwrap();
        manager.close_all().await;
        assert!(manager.evaluate(id, "1".into()).await.is_err());
    }
}

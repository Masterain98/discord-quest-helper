//! Shared read-only CDP connection; concurrent request preparations share one read.
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

#[derive(Clone, Deserialize)]
pub struct Snapshot {
    pub base64: String,
    pub decoded: Value,
    pub fallback: Value,
    pub focused: Option<bool>,
    pub heartbeat_known: bool,
    pub heartbeat_session_id: Option<String>,
    pub account_id: String,
    pub renderer_key: String,
    #[serde(default)]
    pub mod_bit_indices: Vec<u8>,
    #[serde(skip)]
    pub mod_bits: u128,
    pub module_ids: Value,
    pub native_desktop: bool,
}

#[derive(Default)]
struct Reader {
    port: u16,
    socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    generation: String,
    target: String,
    module_ids: Value,
    next_id: u64,
    completed: Option<Instant>,
    cached: Option<Snapshot>,
    failed: Option<Instant>,
}

static READER: once_cell::sync::Lazy<tokio::sync::Mutex<Reader>> =
    once_cell::sync::Lazy::new(|| tokio::sync::Mutex::new(Reader::default()));

pub async fn refresh(port: u16) -> Result<Snapshot> {
    refresh_reader(&READER, port).await
}

async fn refresh_reader(lock: &tokio::sync::Mutex<Reader>, port: u16) -> Result<Snapshot> {
    let requested = Instant::now();
    let mut reader = lock.lock().await;
    if reader.port != port {
        *reader = Reader {
            port,
            ..Default::default()
        };
    }
    if reader
        .failed
        .is_some_and(|failed| failed.elapsed() < Duration::from_secs(5))
    {
        anyhow::bail!("cdp_retry_backoff");
    }
    if reader
        .completed
        .is_some_and(|completed| completed >= requested)
    {
        if let Some(cached) = &reader.cached {
            return Ok(cached.clone());
        }
    }
    match tokio::time::timeout(Duration::from_secs(1), reader.read()).await {
        Ok(Ok(snapshot)) => {
            reader.completed = Some(Instant::now());
            reader.cached = Some(snapshot.clone());
            reader.failed = None;
            Ok(snapshot)
        }
        outcome => {
            reader.socket = None;
            reader.module_ids = Value::Null;
            reader.cached = None;
            reader.failed = Some(Instant::now());
            match outcome {
                Ok(Err(error)) => Err(error),
                _ => anyhow::bail!("cdp_read_timeout"),
            }
        }
    }
}

impl Reader {
    async fn read(&mut self) -> Result<Snapshot> {
        if self.socket.is_none() {
            let verified = crate::cdp_client::verify_primary_discord_target(self.port).await?;
            let url = verified
                .target
                .web_socket_debugger_url
                .context("missing_cdp_websocket")?;
            self.target = verified.target.id;
            self.generation = verified.generation;
            self.socket = Some(connect_async(&url).await?.0);
            self.module_ids = serde_json::json!({});
        }
        let script = include_str!("cdp_snapshot.js")
            .replace("__GENERATION__", &serde_json::to_string(&self.generation)?)
            .replace("__MODULE_IDS__", &self.module_ids.to_string());
        self.next_id += 1;
        let id = self.next_id;
        let socket = self.socket.as_mut().unwrap();
        socket
            .send(Message::Text(
                serde_json::json!({"id":id,"method":"Runtime.evaluate",
            "params":{"expression":script,"returnByValue":true,"timeout":800}})
                .to_string()
                .into(),
            ))
            .await?;
        while let Some(message) = socket.next().await {
            match message? {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text)?;
                    if value["id"].as_u64() != Some(id) {
                        continue;
                    }
                    if value.get("error").is_some()
                        || value["result"].get("exceptionDetails").is_some()
                    {
                        // Do not include renderer values or exception payloads in errors.
                        anyhow::bail!("cdp_identity_read_failed");
                    }
                    let mut snapshot: Snapshot =
                        serde_json::from_value(value["result"]["result"]["value"].clone())?;
                    if !snapshot.native_desktop {
                        anyhow::bail!("cdp_native_desktop_unavailable");
                    }
                    snapshot.renderer_key = format!("{}:{}", self.target, snapshot.renderer_key);
                    snapshot.mod_bits = snapshot
                        .mod_bit_indices
                        .iter()
                        .filter(|bit| **bit < 128)
                        .fold(0u128, |mask, bit| mask | (1u128 << (127 - *bit)));
                    self.module_ids = snapshot.module_ids.clone();
                    return Ok(snapshot);
                }
                Message::Ping(data) => socket.send(Message::Pong(data)).await?,
                Message::Close(_) => break,
                _ => {}
            }
        }
        anyhow::bail!("cdp_connection_closed")
    }
}

/// Apply network results only if the identity context did not change while waiting.
pub async fn prepare(
    manager: &std::sync::Arc<std::sync::Mutex<super::XSuperPropertiesManager>>,
) -> super::RequestIdentity {
    let key = manager
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .session_key
        .clone();
    prepare_for(manager, &key).await.unwrap_or_else(|| {
        manager
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .request_identity()
    })
}

pub async fn prepare_for(
    manager: &std::sync::Arc<std::sync::Mutex<super::XSuperPropertiesManager>>,
    expected_key: &str,
) -> Option<super::RequestIdentity> {
    prepare_with(manager, expected_key, refresh).await
}

async fn prepare_with<F, Fut>(
    manager: &std::sync::Arc<std::sync::Mutex<super::XSuperPropertiesManager>>,
    expected_key: &str,
    read: F,
) -> Option<super::RequestIdentity>
where
    F: FnOnce(u16) -> Fut,
    Fut: std::future::Future<Output = Result<Snapshot>>,
{
    let (port, session_key) = {
        let guard = manager.lock().unwrap_or_else(|error| error.into_inner());
        if guard.session_key != expected_key {
            return None;
        }
        (
            guard.cdp_port.filter(|_| guard.account_id.is_some()),
            guard.session_key.clone(),
        )
    };
    if let Some(port) = port {
        let result = read(port).await;
        let mut guard = manager.lock().unwrap_or_else(|error| error.into_inner());
        if guard.session_key == session_key {
            match result {
                Ok(snapshot) => {
                    let _ = guard.accept_cdp(&snapshot);
                }
                Err(_) => guard.use_local(Some("cdp_unavailable".into())),
            }
        }
    }
    let mut guard = manager.lock().unwrap_or_else(|error| error.into_inner());
    (guard.session_key == expected_key).then(|| guard.request_identity())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    async fn mock_reader(fail: bool) -> (tokio::sync::Mutex<Reader>, Arc<AtomicUsize>) {
        mock_reader_delayed(fail, Duration::from_millis(20)).await
    }

    async fn mock_reader_delayed(
        fail: bool,
        delay: Duration,
    ) -> (tokio::sync::Mutex<Reader>, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let command: Value = serde_json::from_str(&text).unwrap();
                observed.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(delay).await;
                let response = if fail {
                    serde_json::json!({"id":command["id"],"result":{"exceptionDetails":{"text":"synthetic"}}})
                } else {
                    serde_json::json!({"id":command["id"],"result":{"result":{"value":{
                        "base64":"e30=","decoded":{},"fallback":{},"focused":false,"heartbeat_known":true,
                        "heartbeat_session_id":null,"account_id":"synthetic-account","renderer_key":"1",
                        "mod_bit_indices":[43],"module_ids":{"properties":"a"},"native_desktop":true
                    }}}})
                };
                if socket
                    .send(Message::Text(response.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        let socket = connect_async(format!("ws://{address}")).await.unwrap().0;
        (
            tokio::sync::Mutex::new(Reader {
                port: 1,
                socket: Some(socket),
                generation: "1".into(),
                target: "synthetic-target".into(),
                module_ids: serde_json::json!({}),
                ..Default::default()
            }),
            count,
        )
    }

    #[tokio::test]
    async fn concurrent_reads_share_refresh_and_later_requests_refresh_again() {
        let (reader, count) = mock_reader(false).await;
        let (first, second, third) = tokio::join!(
            refresh_reader(&reader, 1),
            refresh_reader(&reader, 1),
            refresh_reader(&reader, 1)
        );
        assert_eq!(first.unwrap().renderer_key, "synthetic-target:1");
        assert!(second.is_ok() && third.is_ok());
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let next = refresh_reader(&reader, 1).await.unwrap();
        assert_eq!(next.mod_bits, 1u128 << (127 - 43));
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn renderer_failure_invalidates_socket_and_backs_off() {
        let (reader, count) = mock_reader(true).await;
        assert!(refresh_reader(&reader, 1).await.is_err());
        assert!(reader.lock().await.socket.is_none());
        assert_eq!(
            refresh_reader(&reader, 1).await.err().unwrap().to_string(),
            "cdp_retry_backoff"
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let (replacement, _) = mock_reader(false).await;
        {
            let replacement = replacement.into_inner();
            let mut state = reader.lock().await;
            state.socket = replacement.socket;
            state.failed = Some(Instant::now() - Duration::from_secs(5));
        }
        assert!(refresh_reader(&reader, 1).await.is_ok());
    }

    #[tokio::test]
    async fn unresponsive_renderer_is_bounded_to_one_second() {
        let (reader, _) = mock_reader_delayed(false, Duration::from_secs(10)).await;
        let started = Instant::now();
        assert_eq!(
            refresh_reader(&reader, 1).await.err().unwrap().to_string(),
            "cdp_read_timeout"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(reader.lock().await.socket.is_none());
    }

    #[tokio::test]
    async fn account_switch_during_refresh_discards_the_inflight_result() {
        let mut identity = super::super::XSuperPropertiesManager::new();
        identity.account_id = Some("synthetic-account".into());
        identity.cdp_port = Some(1);
        let old_key = identity.session_key.clone();
        let manager = Arc::new(std::sync::Mutex::new(identity));
        let (started, waiting) = tokio::sync::oneshot::channel();
        let (send_result, result) = tokio::sync::oneshot::channel();
        let task_manager = manager.clone();
        let task = tokio::spawn(async move {
            prepare_with(&task_manager, &old_key, |_| async move {
                started.send(()).unwrap();
                result.await.unwrap()
            })
            .await
        });
        waiting.await.unwrap();
        let before = {
            let mut current = manager.lock().unwrap();
            current.logout().unwrap();
            current.authenticate("synthetic-account".into());
            current.request_identity().base64
        };
        let stale: Snapshot = serde_json::from_value(serde_json::json!({
            "base64":"e30=","decoded":{"future_field":"stale-renderer"},"fallback":{},
            "focused":true,"heartbeat_known":true,"heartbeat_session_id":"stale-session",
            "account_id":"synthetic-account","renderer_key":"stale-renderer", "module_ids":{},
            "native_desktop":true
        }))
        .unwrap();
        assert!(send_result.send(Ok(stale)).is_ok());
        assert!(task.await.unwrap().is_none());
        let mut current = manager.lock().unwrap();
        assert_eq!(current.request_identity().base64, before);
        assert!(current
            .request_identity()
            .properties
            .get("future_field")
            .is_none());
    }
}

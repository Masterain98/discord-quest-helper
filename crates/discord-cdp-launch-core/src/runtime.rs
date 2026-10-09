//! Bounded, read-only runtime verification shared by the GUI and launcher.
use crate::{CdpRuntime, CdpRuntimeStatus, CdpTarget};
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};
use tungstenite::{client::client_with_config, protocol::WebSocketConfig, Message, WebSocket};
use url::Url;

struct DeadlineStream {
    stream: TcpStream,
    deadline: Instant,
}
impl Read for DeadlineStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        set_budget(&self.stream, self.deadline)
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?;
        self.stream.read(buffer)
    }
}
impl Write for DeadlineStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        set_budget(&self.stream, self.deadline)
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?;
        self.stream.write(buffer)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

const EXPRESSION: &str = r#"(() => ({
 appRootPresent: !!document.getElementById('app-mount'),
 moduleLoaderPresent: Array.isArray(window.webpackChunkdiscord_app) &&
   typeof window.webpackChunkdiscord_app.push === 'function' &&
   window.webpackChunkdiscord_app.push !== Array.prototype.push,
 nativeBridgePresent: !!window.DiscordNative || !!window.VesktopNative,
 focused: document.hasFocus(),
 documentGeneration: String(performance.timeOrigin),
 loading: document.readyState !== 'complete'
}))()"#;

fn failed(stage: &str, reason: &str, reachable: bool) -> CdpRuntime {
    CdpRuntime {
        runtime_status: CdpRuntimeStatus::ProbeFailed,
        web_socket_reachable: reachable,
        failure_stage: Some(stage.into()),
        reason_code: Some(reason.into()),
        ..Default::default()
    }
}

fn budget(deadline: Instant) -> Result<Duration, ()> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or(())
}

fn set_budget(stream: &TcpStream, deadline: Instant) -> Result<(), ()> {
    let remaining = budget(deadline)?;
    stream.set_read_timeout(Some(remaining)).map_err(|_| ())?;
    stream.set_write_timeout(Some(remaining)).map_err(|_| ())
}

pub(crate) fn verify(port: u16, target: &CdpTarget, round_deadline: Instant) -> CdpRuntime {
    let flags = match evaluate(port, target, EXPRESSION, round_deadline) {
        Ok(flags) => flags,
        Err(failure) => return failure,
    };
    runtime_from_flags(&flags).unwrap_or_else(|| failed("evaluate", "invalid_response", true))
}

fn runtime_from_flags(flags: &Value) -> Option<CdpRuntime> {
    let flag = |key| flags.get(key).and_then(Value::as_bool);
    let app_root_present = flag("appRootPresent")?;
    let module_loader_present = flag("moduleLoaderPresent")?;
    let native_bridge_present = flag("nativeBridgePresent")?;
    let focused = flag("focused")?;
    let loading = flag("loading")?;
    let runtime_status = if app_root_present && module_loader_present {
        CdpRuntimeStatus::Ready
    } else if loading || app_root_present {
        CdpRuntimeStatus::Loading
    } else {
        CdpRuntimeStatus::Unsupported
    };
    Some(CdpRuntime {
        runtime_status,
        web_socket_reachable: true,
        app_root_present,
        module_loader_present,
        native_bridge_present,
        focused,
        failure_stage: None,
        document_generation: flags
            .get("documentGeneration")
            .and_then(Value::as_str)
            .map(str::to_owned),
        reason_code: (runtime_status != CdpRuntimeStatus::Ready)
            .then(|| "runtime_not_loaded".into()),
    })
}

/// Evaluate a bounded expression on an exact loopback discovery target.
fn evaluate(
    port: u16,
    target: &CdpTarget,
    expression: &str,
    round_deadline: Instant,
) -> Result<Value, CdpRuntime> {
    let request = json!({"id": 1, "method": "Runtime.evaluate", "params": {
        "expression": expression, "returnByValue": true, "awaitPromise": false,
        "timeout": budget(round_deadline).map(|d| d.as_millis().min(750) as u64).unwrap_or(1)
    }});
    let result = exchange(port, target, request, round_deadline)?;
    result
        .pointer("/result/value")
        .cloned()
        .ok_or_else(|| failed("evaluate", "invalid_response", true))
}

#[cfg(target_os = "windows")]
pub(crate) fn close_browser(port: u16, target: &CdpTarget, deadline: Instant) -> bool {
    // Electron's DevToolsManagerDelegate handles this with Browser::Quit(),
    // including applications which expose no renderer-side app.quit bridge.
    // Electron intentionally does not send an acknowledgment before exiting.
    browser_close_submitted(exchange(
        port,
        target,
        json!({"id": 1, "method": "Browser.close", "params": {}}),
        deadline,
    ))
}

#[cfg(target_os = "windows")]
fn browser_close_submitted(outcome: Result<Value, CdpRuntime>) -> bool {
    match outcome {
        Ok(_) => true,
        Err(failure) => {
            failure.failure_stage.as_deref() == Some("evaluate")
                && matches!(
                    failure.reason_code.as_deref(),
                    Some("read_failed" | "target_closed" | "deadline")
                )
        }
    }
}

fn exchange(
    port: u16,
    target: &CdpTarget,
    request: Value,
    round_deadline: Instant,
) -> Result<Value, CdpRuntime> {
    let deadline = round_deadline.min(Instant::now() + Duration::from_millis(750));
    let Some(ws_url) = target.web_socket_debugger_url.as_deref() else {
        return Err(failed("discovery", "missing_websocket", false));
    };
    if Url::parse(ws_url).is_err() {
        return Err(failed("discovery", "invalid_websocket", false));
    }
    if !is_loopback_websocket(port, ws_url) {
        return Err(failed("discovery", "non_loopback_websocket", false));
    }
    // No DNS, proxies, TLS or Origin overrides. Dial the exact discovery port.
    let Ok(remaining) = budget(deadline) else {
        return Err(failed("connect", "deadline", false));
    };
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let Ok(stream) = TcpStream::connect_timeout(&address, remaining) else {
        return Err(failed("connect", "connection_failed", false));
    };
    if set_budget(&stream, deadline).is_err() {
        return Err(failed("connect", "deadline", false));
    }
    // The socket owns its stream; dropping it closes TCP even on handshake errors.
    let stream = DeadlineStream { stream, deadline };
    let Ok((socket, _)) = client_with_config(
        ws_url,
        stream,
        Some(
            WebSocketConfig::default()
                .max_message_size(Some(1024 * 1024))
                .max_frame_size(Some(1024 * 1024)),
        ),
    ) else {
        return Err(failed("handshake", "handshake_failed", false));
    };
    exchange_request(socket, request, deadline)
}

fn exchange_request(
    mut socket: WebSocket<DeadlineStream>,
    request: Value,
    deadline: Instant,
) -> Result<Value, CdpRuntime> {
    let outcome = (|| {
        set_budget(&socket.get_ref().stream, deadline)
            .map_err(|_| ("evaluate", "deadline_before_send"))?;
        socket
            .send(Message::Text(request.to_string().into()))
            .map_err(|_| ("evaluate", "send_failed"))?;
        loop {
            set_budget(&socket.get_ref().stream, deadline).map_err(|_| ("evaluate", "deadline"))?;
            let message = socket.read().map_err(|_| ("evaluate", "read_failed"))?;
            match message {
                Message::Text(text) => {
                    if text.len() > 1024 * 1024 {
                        return Err(("evaluate", "response_too_large"));
                    }
                    let value: Value = serde_json::from_str(&text)
                        .map_err(|_| ("evaluate", "invalid_response"))?;
                    if value.get("id") != Some(&json!(1)) {
                        continue;
                    }
                    if value.get("error").is_some() {
                        return Err(("evaluate", "protocol_error"));
                    }
                    if value.pointer("/result/exceptionDetails").is_some() {
                        return Err(("evaluate", "javascript_exception"));
                    }
                    let result = value
                        .get("result")
                        .ok_or(("evaluate", "invalid_response"))?;
                    return Ok(result.clone());
                }
                Message::Ping(_) => {
                    socket.flush().map_err(|_| ("evaluate", "ping_failed"))?;
                }
                Message::Close(_) => return Err(("evaluate", "target_closed")),
                _ => {}
            }
        }
    })();
    // Do not wait for a close handshake after the deadline.
    if set_budget(&socket.get_ref().stream, deadline).is_ok() {
        let _ = socket.close(None);
        let _ = socket.flush();
    }
    outcome.map_err(|(stage, reason)| failed(stage, reason, true))
}

pub(crate) fn is_loopback_websocket(port: u16, ws_url: &str) -> bool {
    Url::parse(ws_url).ok().is_some_and(|url| {
        url.scheme() == "ws"
            && url.port_or_known_default() == Some(port)
            && matches!(
                url.host_str(),
                Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
            )
            && url.username().is_empty()
            && url.password().is_none()
    })
}

pub(crate) fn preference<'a>(runtime: &CdpRuntime, id: &'a str) -> (bool, bool, &'a str) {
    (!runtime.native_bridge_present, !runtime.focused, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    fn fixture(
        reply: Value,
        event_and_ping: bool,
        close: bool,
        delay: Duration,
    ) -> (CdpTarget, u16, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let closed = Arc::new(AtomicBool::new(false));
        let observed = closed.clone();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let request = socket.read().unwrap().into_text().unwrap();
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["method"], "Runtime.evaluate");
            assert!(!request["params"]["expression"]
                .as_str()
                .unwrap()
                .contains("getToken"));
            std::thread::sleep(delay);
            if close {
                let _ = socket.close(None);
                return;
            }
            if event_and_ping {
                let _ = socket.send(Message::Text(
                    json!({"method":"Runtime.consoleAPICalled"})
                        .to_string()
                        .into(),
                ));
                let _ = socket.send(Message::Ping(vec![1, 2].into()));
                let _ = socket.send(Message::Text(
                    json!({"id":999,"result":{}}).to_string().into(),
                ));
            }
            if socket
                .send(Message::Text(reply.to_string().into()))
                .is_err()
            {
                return;
            }
            loop {
                match socket.read() {
                    Ok(Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => {
                        observed.store(true, Ordering::SeqCst);
                        break;
                    }
                    Err(tungstenite::Error::Io(error))
                        if matches!(
                            error.kind(),
                            io::ErrorKind::UnexpectedEof
                                | io::ErrorKind::ConnectionReset
                                | io::ErrorKind::ConnectionAborted
                        ) =>
                    {
                        observed.store(true, Ordering::SeqCst);
                        break;
                    }
                    Err(_) => break,
                    _ => {}
                }
            }
        });
        let target = CdpTarget {
            id: "a".into(),
            target_type: "page".into(),
            title: "".into(),
            url: "https://discord.com/new-route-never-seen-before".into(),
            web_socket_debugger_url: Some(format!("ws://127.0.0.1:{port}/devtools/page/a")),
        };
        (target, port, closed, handle)
    }

    fn flags(root: bool, loader: bool, native: bool, loading: bool) -> Value {
        json!({"id":1,"result":{"result":{"value": {
            "appRootPresent":root,"moduleLoaderPresent":loader,"nativeBridgePresent":native,
            "focused":false,"loading":loading,"documentGeneration":"100"
        }}}})
    }

    #[test]
    fn runtime_not_route_or_login_decides_readiness_and_socket_is_closed() {
        for route in ["/login", "/store", "/shop", "/future-route", "/popout"] {
            let (mut target, port, closed, handle) =
                fixture(flags(true, true, false, false), true, false, Duration::ZERO);
            target.url = format!("https://discord.com{route}");
            let runtime = verify(port, &target, Instant::now() + Duration::from_secs(3));
            assert_eq!(runtime.runtime_status, CdpRuntimeStatus::Ready);
            assert!(!runtime.native_bridge_present); // Vesktop needs no official native bridge.
            assert_eq!(runtime.document_generation.as_deref(), Some("100"));
            handle.join().unwrap();
            assert!(closed.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn absent_environment_and_loading_are_distinct() {
        for (root, loader, loading, expected) in [
            (false, false, false, CdpRuntimeStatus::Unsupported),
            (false, false, true, CdpRuntimeStatus::Loading),
            (true, false, false, CdpRuntimeStatus::Loading),
        ] {
            let (target, port, _, handle) = fixture(
                flags(root, loader, false, loading),
                false,
                false,
                Duration::ZERO,
            );
            assert_eq!(
                verify(port, &target, Instant::now() + Duration::from_secs(3)).runtime_status,
                expected
            );
            handle.join().unwrap();
        }
    }

    #[test]
    fn protocol_errors_and_exceptions_are_sanitized() {
        for (reply, reason) in [
            (
                json!({"id":1,"error":{"message":"private secret"}}),
                "protocol_error",
            ),
            (
                json!({"id":1,"result":{"exceptionDetails":{"text":"private secret"}}}),
                "javascript_exception",
            ),
            (
                json!({"id":1,"result":{"result":{"value":{}}}}),
                "invalid_response",
            ),
        ] {
            let (target, port, closed, handle) = fixture(reply, false, false, Duration::ZERO);
            let runtime = verify(port, &target, Instant::now() + Duration::from_secs(3));
            assert_eq!(runtime.runtime_status, CdpRuntimeStatus::ProbeFailed);
            assert_eq!(runtime.reason_code.as_deref(), Some(reason));
            handle.join().unwrap();
            assert!(closed.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn closing_or_stalled_target_is_not_ready() {
        for (close, delay) in [(true, Duration::ZERO), (false, Duration::from_millis(900))] {
            let (target, port, _, handle) =
                fixture(flags(true, true, true, false), false, close, delay);
            let start = Instant::now();
            let runtime = verify(port, &target, start + Duration::from_secs(3));
            assert_eq!(runtime.runtime_status, CdpRuntimeStatus::ProbeFailed);
            assert!(start.elapsed() < Duration::from_millis(850));
            handle.join().unwrap();
        }
    }

    #[test]
    fn websocket_must_use_the_discovered_loopback_port() {
        let mut target = CdpTarget {
            id: "a".into(),
            target_type: "page".into(),
            title: "".into(),
            url: "https://discord.com/anything".into(),
            web_socket_debugger_url: None,
        };
        for url in [
            "ws://example.com:9223/a",
            "ws://127.0.0.1:9999/a",
            "wss://127.0.0.1:9223/a",
            "ws://user:pass@127.0.0.1:9223/a",
        ] {
            target.web_socket_debugger_url = Some(url.into());
            assert_eq!(
                verify(9223, &target, Instant::now() + Duration::from_secs(3))
                    .reason_code
                    .as_deref(),
                Some("non_loopback_websocket")
            );
        }
    }

    #[test]
    fn selection_ranks_capabilities_then_focus_then_stable_id() {
        let native = CdpRuntime {
            native_bridge_present: true,
            ..Default::default()
        };
        let focused = CdpRuntime {
            focused: true,
            ..Default::default()
        };
        let other = CdpRuntime::default();
        assert!(preference(&native, "z") < preference(&focused, "a"));
        assert!(preference(&focused, "z") < preference(&other, "a"));
        assert!(preference(&other, "a") < preference(&other, "z"));
    }

    #[test]
    fn failed_and_trickling_handshakes_obey_the_target_deadline() {
        for trickle in [false, true] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                if trickle {
                    for byte in b"HTTP/1.1 101 Switching Protocols\r\n" {
                        if stream.write_all(&[*byte]).is_err() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(60));
                    }
                } else {
                    let _ =
                        stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
                }
            });
            let target = CdpTarget {
                id: "x".into(),
                target_type: "page".into(),
                title: "".into(),
                url: "https://discord.com/new".into(),
                web_socket_debugger_url: Some(format!("ws://127.0.0.1:{port}/devtools/page/x")),
            };
            let start = Instant::now();
            let result = verify(port, &target, start + Duration::from_secs(3));
            assert_eq!(result.runtime_status, CdpRuntimeStatus::ProbeFailed);
            assert_eq!(result.failure_stage.as_deref(), Some("handshake"));
            assert!(start.elapsed() < Duration::from_millis(850));
            server.join().unwrap();
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn browser_close_rejects_a_deadline_reached_before_sending() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            // The client finishes the handshake but must not send Browser.close.
            assert!(!matches!(socket.read(), Ok(Message::Text(_))));
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (socket, _) = client_with_config(
            format!("ws://127.0.0.1:{port}/devtools/page/close"),
            DeadlineStream {
                stream,
                deadline: Instant::now() + Duration::from_secs(2),
            },
            None,
        )
        .unwrap();
        // Expire the budget at the exact boundary after the completed handshake.
        let outcome = exchange_request(
            socket,
            json!({"id": 1, "method": "Browser.close", "params": {}}),
            Instant::now(),
        );
        server.join().unwrap();
        assert!(!browser_close_submitted(outcome));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn browser_close_accepts_electron_disconnect_but_rejects_protocol_errors() {
        for disconnect in [true, false] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                let request: Value =
                    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
                assert_eq!(request["method"], "Browser.close");
                assert_eq!(request["params"], json!({}));
                if disconnect {
                    let _ = socket.close(None);
                } else {
                    let _ = socket.send(Message::Text(
                        json!({"id":1,"error":{
                            "code":-32601,"message":"unsupported"
                        }})
                        .to_string()
                        .into(),
                    ));
                }
            });
            let target = CdpTarget {
                id: "close".into(),
                target_type: "page".into(),
                title: "".into(),
                url: "https://discord.com/app".into(),
                web_socket_debugger_url: Some(format!("ws://127.0.0.1:{port}/devtools/page/close")),
            };
            assert_eq!(
                close_browser(port, &target, Instant::now() + Duration::from_secs(2)),
                disconnect
            );
            server.join().unwrap();
        }
    }
}

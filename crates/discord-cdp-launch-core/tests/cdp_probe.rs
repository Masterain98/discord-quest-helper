use discord_cdp_launch_core::{
    list_cdp_targets_with_timeouts, parse_cdp_targets_http_response, CdpListError, CdpProbe,
    CdpProbeStatus, StdCdpProbe,
};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

static SOCKET_TEST_LOCK: Mutex<()> = Mutex::new(());

fn serialize_socket_test() -> MutexGuard<'static, ()> {
    SOCKET_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn serve_once(response: Option<&'static str>, delay: Duration) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(1500);
        let mut delayed = false;
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_nodelay(true);
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
                    let mut request = [0_u8; 1024];
                    let _ = stream.read(&mut request);
                    if !delay.is_zero() && !delayed {
                        std::thread::sleep(delay);
                        delayed = true;
                    }
                    if let Some(response) = response {
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                        let _ = stream.shutdown(Shutdown::Write);
                    }
                    // Keep the socket open long enough for the client to finish
                    // reading the Content-Length body. Dropping it immediately
                    // after a write can make Windows report a reset socket.
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(400));
                        drop(stream);
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => break,
            }
        }
    });
    port
}

fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn leaked_response(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn fast_probe() -> StdCdpProbe {
    // Keep the test fast while leaving enough scheduling headroom for the
    // in-process server when the whole workspace test suite runs in parallel.
    StdCdpProbe::with_timeouts(Duration::from_millis(500), Duration::from_secs(1))
}

#[test]
fn recognizes_a_discord_page_target() {
    let _guard = serialize_socket_test();
    let body = r#"[{"id":"1","type":"page","title":"Quests","url":"https://discord.com/quest-home","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/1"}]"#;
    let port = serve_once(
        Some(leaked_response(response("200 OK", body))),
        Duration::ZERO,
    );
    assert_eq!(
        fast_probe().probe(port),
        CdpProbeStatus::DiscordReady {
            target_title: Some("Quests".to_string())
        }
    );
}

#[test]
fn list_cdp_targets_returns_full_fixture_including_workers() {
    let _guard = serialize_socket_test();
    let body = r#"[{"id":"1","type":"page","title":"Quests","url":"https://discord.com/quest-home","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/1"},{"id":"2","type":"worker","title":"","url":""}]"#;
    let raw = response("200 OK", body);
    let parsed = parse_cdp_targets_http_response(9223, raw.as_bytes()).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].title, "Quests");
    assert_eq!(parsed[1].target_type, "worker");

    let port = serve_once(Some(leaked_response(raw)), Duration::ZERO);
    let listed = list_cdp_targets_with_timeouts(
        port,
        Duration::from_millis(500),
        Duration::from_millis(300),
    )
    .unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, "1");
    assert_eq!(listed[1].id, "2");
}

#[test]
fn list_cdp_targets_rejects_http_400() {
    let _guard = serialize_socket_test();
    let port = serve_once(
        Some(leaked_response(response("400 Bad Request", "[]"))),
        Duration::ZERO,
    );
    let error = list_cdp_targets_with_timeouts(
        port,
        Duration::from_millis(500),
        Duration::from_millis(300),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        CdpListError::HttpStatus { status: 400, .. }
    ));
    assert!(!error.is_transient());
}

#[test]
fn overlay_before_main_renderer_still_reports_main_window() {
    let _guard = serialize_socket_test();
    let body = r#"[{"id":"1","type":"page","title":"Discord Overlay","url":"https://discord.com/popout","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/1"},{"id":"2","type":"page","title":"Friends","url":"https://discord.com/channels/@me","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/2"}]"#;
    let port = serve_once(
        Some(leaked_response(response("200 OK", body))),
        Duration::ZERO,
    );
    assert_eq!(
        fast_probe().probe(port),
        CdpProbeStatus::DiscordReady {
            target_title: Some("Friends".to_string())
        }
    );
}

#[test]
fn overlay_only_is_not_discord_ready() {
    let _guard = serialize_socket_test();
    let body = r#"[{"id":"1","type":"page","title":"Discord Overlay","url":"https://discord.com/popout","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/1"}]"#;
    let port = serve_once(
        Some(leaked_response(response("200 OK", body))),
        Duration::ZERO,
    );
    assert_eq!(
        fast_probe().probe(port),
        CdpProbeStatus::CdpWithoutDiscordTarget
    );
}

#[test]
fn distinguishes_valid_chromium_without_discord() {
    let _guard = serialize_socket_test();
    let body = r#"[{"type":"page","title":"Chromium","url":"https://example.com","webSocketDebuggerUrl":"ws://example"}]"#;
    let port = serve_once(
        Some(leaked_response(response("200 OK", body))),
        Duration::ZERO,
    );
    assert_eq!(
        fast_probe().probe(port),
        CdpProbeStatus::CdpWithoutDiscordTarget
    );
}

#[test]
fn discord_target_without_websocket_is_not_ready() {
    let _guard = serialize_socket_test();
    let body = r#"[{"type":"page","title":"Discord","url":"https://discord.com/app"}]"#;
    let port = serve_once(
        Some(leaked_response(response("200 OK", body))),
        Duration::ZERO,
    );
    assert_eq!(
        fast_probe().probe(port),
        CdpProbeStatus::CdpWithoutDiscordTarget
    );
}

#[test]
fn malformed_json_and_http_500_are_port_occupied() {
    let _guard = serialize_socket_test();
    for response in [
        response("200 OK", "not json"),
        response("500 Internal Server Error", "[]"),
    ] {
        let port = serve_once(Some(leaked_response(response)), Duration::ZERO);
        assert_eq!(fast_probe().probe(port), CdpProbeStatus::PortOccupied);
    }
}

#[test]
fn response_timeout_is_port_occupied() {
    let _guard = serialize_socket_test();
    let port = serve_once(None, Duration::from_millis(800));
    assert_eq!(fast_probe().probe(port), CdpProbeStatus::PortOccupied);
}

#[test]
fn closed_port_is_unreachable() {
    let _guard = serialize_socket_test();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    assert_eq!(fast_probe().probe(port), CdpProbeStatus::Unreachable);
    let error = list_cdp_targets_with_timeouts(
        port,
        Duration::from_millis(200),
        Duration::from_millis(200),
    )
    .unwrap_err();
    assert!(matches!(error, CdpListError::Unreachable { .. }));
    assert!(error.is_transient());
}

#[test]
fn unrelated_tcp_listener_is_not_misclassified_as_discord() {
    let _guard = serialize_socket_test();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_nodelay(true);
                    let _ = stream.write_all(b"HELLO");
                    let _ = stream.flush();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => break,
            }
        }
    });
    assert_eq!(fast_probe().probe(port), CdpProbeStatus::PortOccupied);
}

#[test]
fn complete_content_length_does_not_wait_for_connection_close() {
    let _guard = serialize_socket_test();
    let body = r#"[{"type":"page","title":"Discord","url":"https://discord.com/app","webSocketDebuggerUrl":"ws://example"}]"#;
    let response = response("200 OK", body);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(2500);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_nodelay(true);
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(2));
                        drop(stream);
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => break,
            }
        }
    });
    let started = Instant::now();
    assert!(matches!(
        StdCdpProbe::with_timeouts(Duration::from_millis(500), Duration::from_secs(3)).probe(port),
        CdpProbeStatus::DiscordReady { .. }
    ));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "probe waited for the server to close the connection"
    );
}

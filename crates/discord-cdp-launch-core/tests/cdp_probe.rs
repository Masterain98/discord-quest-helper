use discord_cdp_launch_core::{
    detailed_probe_cdp, list_cdp_targets_with_timeouts, parse_cdp_targets_http_response,
    CdpListError, CdpProbe, CdpProbeStatus, CdpTargetClassification, StdCdpProbe,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

static SOCKET_TEST_LOCK: Mutex<()> = Mutex::new(());

fn serialize_socket_test() -> MutexGuard<'static, ()> {
    SOCKET_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// tungstenite's handshake callback requires an unboxed HTTP ErrorResponse.
#[allow(clippy::result_large_err)]
fn serve_once(response: Option<&'static str>, delay: Duration) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut capabilities = std::collections::HashMap::new();
    let response = response.map(|raw| {
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        if let Ok(mut targets) = serde_json::from_str::<serde_json::Value>(body) {
            if let Some(items) = targets.as_array_mut() {
                for target in items {
                    let id = target["id"].as_str().unwrap_or("unknown").to_string();
                    let ready = target["title"].as_str() != Some("Discord Overlay");
                    capabilities.insert(id.clone(), target.get("testRuntime").cloned().unwrap_or_else(|| serde_json::json!({
                        "appRootPresent":ready,"moduleLoaderPresent":ready,"nativeBridgePresent":ready,"focused":false,"loading":false,"documentGeneration":"100"
                    })));
                    if target.get("webSocketDebuggerUrl").is_some() {
                        target["webSocketDebuggerUrl"] =
                            serde_json::json!(format!("ws://127.0.0.1:{port}/devtools/page/{id}"));
                    }
                }
                let body = targets.to_string();
                let status = head
                    .lines()
                    .next()
                    .unwrap()
                    .strip_prefix("HTTP/1.1 ")
                    .unwrap();
                return response_owned(status, &body);
            }
        }
        raw.to_string()
    });
    std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = [0_u8; 1024];
            let size = stream.peek(&mut request).unwrap_or(0);
            if String::from_utf8_lossy(&request[..size]).starts_with("GET /json") {
                let _ = stream.read(&mut request);
                std::thread::sleep(delay);
                if let Some(raw) = &response {
                    let _ = stream.write_all(raw.as_bytes());
                    let _ = stream.flush();
                }
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(400));
                    drop(stream);
                });
            } else {
                let capabilities = capabilities.clone();
                std::thread::spawn(move || {
                    let mut target_id = String::new();
                    let socket = tungstenite::accept_hdr(
                        stream,
                        |req: &tungstenite::handshake::server::Request, response| {
                            target_id = req
                                .uri()
                                .path()
                                .rsplit('/')
                                .next()
                                .unwrap_or_default()
                                .into();
                            Ok(response)
                        },
                    );
                    if let Ok(mut socket) = socket {
                        let _ = socket.read();
                        let runtime = capabilities.get(&target_id);
                        if let Some(delay) = runtime.and_then(|value| value["delayMs"].as_u64()) {
                            std::thread::sleep(Duration::from_millis(delay));
                        }
                        let reply =
                            serde_json::json!({"id":1,"result":{"result":{"value":runtime}}});
                        let _ = socket.send(tungstenite::Message::Text(reply.to_string().into()));
                        let _ = socket.read();
                    }
                });
            }
        }
    });
    port
}

fn response_owned(status: &str, body: &str) -> String {
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
        Some(leaked_response(response_owned("200 OK", body))),
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
fn nitro_and_shop_pages_report_ready_and_a_main_renderer() {
    let _guard = serialize_socket_test();
    for route in ["store", "shop"] {
        let body = format!(
            r#"[{{"id":"1","type":"page","title":"Discord","url":"https://discord.com/{route}?source=test","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/1"}}]"#
        );
        let port = serve_once(
            Some(leaked_response(response_owned("200 OK", &body))),
            Duration::ZERO,
        );
        let detailed = detailed_probe_cdp(port);
        assert!(matches!(
            detailed.status,
            CdpProbeStatus::DiscordReady { .. }
        ));
        assert!(detailed.targets[0].is_main_renderer);
        assert_eq!(
            detailed.targets[0].classification,
            CdpTargetClassification::DiscordMainRenderer
        );
    }
}

#[test]
fn list_cdp_targets_returns_full_fixture_including_workers() {
    let _guard = serialize_socket_test();
    let body = r#"[{"id":"1","type":"page","title":"Quests","url":"https://discord.com/quest-home","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/1"},{"id":"2","type":"worker","title":"","url":""}]"#;
    let raw = response_owned("200 OK", body);
    let parsed = parse_cdp_targets_http_response(9223, raw.as_bytes()).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].title, "Quests");
    assert_eq!(parsed[1].target_type, "worker");

    let port = serve_once(Some(leaked_response(raw)), Duration::ZERO);
    let listed =
        list_cdp_targets_with_timeouts(port, Duration::from_secs(1), Duration::from_secs(2))
            .unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, "1");
    assert_eq!(listed[1].id, "2");
}

#[test]
fn list_cdp_targets_rejects_http_400() {
    let _guard = serialize_socket_test();
    let port = serve_once(
        Some(leaked_response(response_owned("400 Bad Request", "[]"))),
        Duration::ZERO,
    );
    let error =
        list_cdp_targets_with_timeouts(port, Duration::from_secs(1), Duration::from_secs(2))
            .unwrap_err();
    assert!(matches!(
        error,
        CdpListError::HttpStatus { status: 400, .. }
    ));
    assert!(!error.is_transient());
}

#[test]
fn detailed_probe_reports_http_and_sanitized_target_classification() {
    let _guard = serialize_socket_test();
    let body = r#"[{"id":"1","type":"page","title":"Friends","url":"https://discord.com/channels/@me?secret=value#fragment","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/private"},{"id":"2","type":"page","title":"Discord Overlay","url":"https://discord.com/popout","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/overlay"}]"#;
    let port = serve_once(
        Some(leaked_response(response_owned("200 OK", body))),
        Duration::ZERO,
    );
    let detailed = detailed_probe_cdp(port);
    assert!(matches!(
        detailed.status,
        CdpProbeStatus::DiscordReady { .. }
    ));
    assert!(detailed.port_listening);
    assert!(detailed.http_reachable);
    assert_eq!(detailed.http_status, Some(200));
    assert!(detailed.response_parseable);
    assert_eq!(detailed.targets.len(), 2);
    assert_eq!(detailed.targets[0].url, "https://discord.com/channels/@me");
    assert_eq!(
        detailed.targets[0].classification,
        CdpTargetClassification::DiscordMainRenderer
    );
    assert_eq!(
        detailed.targets[1].classification,
        CdpTargetClassification::DiscordAuxiliary
    );
}

#[test]
fn diagnostics_mark_only_the_preferred_ready_target_as_main() {
    let _guard = serialize_socket_test();
    let body = serde_json::json!([
        {"id":"a-popout", "type":"page", "title":"Popout", "url":"https://discord.com/new-popout-route",
            "webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/a-popout",
            "testRuntime":{"appRootPresent":true,"moduleLoaderPresent":true,"nativeBridgePresent":false,"focused":true,"loading":false,"documentGeneration":"100"}},
        {"id":"z-main", "type":"page", "title":"Discord", "url":"https://discord.com/new-main-route",
            "webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/z-main",
            "testRuntime":{"appRootPresent":true,"moduleLoaderPresent":true,"nativeBridgePresent":true,"focused":false,"loading":false,"documentGeneration":"200"}}
    ]).to_string();
    let port = serve_once(
        Some(leaked_response(response_owned("200 OK", &body))),
        Duration::ZERO,
    );
    let detailed = detailed_probe_cdp(port);
    assert_eq!(detailed.selected_target.as_ref().unwrap().id, "z-main");
    assert!(detailed.targets.iter().all(|target| {
        target.runtime.runtime_status == discord_cdp_launch_core::CdpRuntimeStatus::Ready
    }));
    assert_eq!(
        detailed
            .targets
            .iter()
            .filter(|target| target.is_main_renderer)
            .count(),
        1
    );
    assert_eq!(
        detailed.targets[0].classification,
        CdpTargetClassification::DiscordOtherRenderer
    );
    assert_eq!(
        detailed.targets[1].classification,
        CdpTargetClassification::DiscordMainRenderer
    );
}

#[test]
fn detailed_probe_distinguishes_non_cdp_http_service() {
    let _guard = serialize_socket_test();
    let port = serve_once(
        Some(leaked_response(response_owned("404 Not Found", "not cdp"))),
        Duration::ZERO,
    );
    let detailed = detailed_probe_cdp(port);
    assert_eq!(detailed.status, CdpProbeStatus::PortOccupied);
    assert!(detailed.port_listening);
    assert!(detailed.http_reachable);
    assert_eq!(detailed.http_status, Some(404));
    assert!(!detailed.response_parseable);
}

#[test]
fn overlay_before_main_renderer_still_reports_main_window() {
    let _guard = serialize_socket_test();
    let body = r#"[{"id":"1","type":"page","title":"Discord Overlay","url":"https://discord.com/popout","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/1"},{"id":"2","type":"page","title":"Friends","url":"https://discord.com/channels/@me","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/2"}]"#;
    let port = serve_once(
        Some(leaked_response(response_owned("200 OK", body))),
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
        Some(leaked_response(response_owned("200 OK", body))),
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
        Some(leaked_response(response_owned("200 OK", body))),
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
        Some(leaked_response(response_owned("200 OK", body))),
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
        response_owned("200 OK", "not json"),
        response_owned("500 Internal Server Error", "[]"),
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
    let response = response_owned("200 OK", body);
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
    assert_eq!(
        list_cdp_targets_with_timeouts(port, Duration::from_millis(500), Duration::from_secs(3))
            .unwrap()
            .len(),
        1
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "probe waited for the server to close the connection"
    );
}

#[test]
fn stalled_candidates_cannot_starve_a_later_ready_renderer() {
    let _guard = serialize_socket_test();
    let targets: Vec<_> = (0..6)
        .map(|id| {
            serde_json::json!({
                "id":id.to_string(), "type":"page", "title":"Discord",
                "url":"https://discord.com/channels/@me", "webSocketDebuggerUrl":"ws://placeholder",
                "testRuntime":{"appRootPresent":true,"moduleLoaderPresent":true,
                    "nativeBridgePresent":false,"focused":false,"loading":false,
                    "documentGeneration":"100","delayMs":if id < 5 { 900 } else { 0 }}
            })
        })
        .collect();
    let body = serde_json::to_string(&targets).unwrap();
    let port = serve_once(
        Some(leaked_response(response_owned("200 OK", &body))),
        Duration::ZERO,
    );
    let started = Instant::now();
    let probe = detailed_probe_cdp(port);
    assert_eq!(probe.selected_target.unwrap().id, "5");
    assert_eq!(
        probe
            .targets
            .iter()
            .filter(|target| target.is_main_renderer)
            .count(),
        1
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn oversized_candidate_lists_fail_before_opening_any_probe_socket() {
    let _guard = serialize_socket_test();
    let targets: Vec<_> = (0..33)
        .map(|id| {
            serde_json::json!({
                "id":id.to_string(), "type":"page", "title":"Discord",
                "url":"https://discord.com/channels/@me", "webSocketDebuggerUrl":"ws://placeholder"
            })
        })
        .collect();
    let body = serde_json::to_string(&targets).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("{error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        stream
            .write_all(response_owned("200 OK", &body).as_bytes())
            .unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "an oversized list must not create probe connections"
        );
    });
    let probe = detailed_probe_cdp(port);
    assert!(probe.selected_target.is_none());
    assert_eq!(probe.status, CdpProbeStatus::CdpWithoutDiscordTarget);
    assert_eq!(
        probe.runtime.reason_code.as_deref(),
        Some("candidate_limit_exceeded")
    );
    assert_eq!(probe.targets.len(), 33);
    assert!(probe
        .targets
        .iter()
        .all(|target| target.runtime.reason_code.as_deref() == Some("candidate_limit_exceeded")));
    server.join().unwrap();
}

#[test]
fn same_url_is_selected_by_runtime_not_route_or_login() {
    let _guard = serialize_socket_test();
    let body = r#"[{"id":"overlay","type":"page","title":"Discord","url":"https://discord.com/future-route","webSocketDebuggerUrl":"ws://placeholder","testRuntime":{"appRootPresent":false,"moduleLoaderPresent":false,"nativeBridgePresent":true,"focused":true,"loading":false}}, {"id":"vesktop","type":"page","title":"Sign in","url":"https://discord.com/future-route","webSocketDebuggerUrl":"ws://placeholder","testRuntime":{"appRootPresent":true,"moduleLoaderPresent":true,"nativeBridgePresent":false,"focused":false,"loading":false,"documentGeneration":"100"}}]"#;
    let body = Box::leak(response_owned("200 OK", body).into_boxed_str());
    let port = serve_once(Some(body), Duration::ZERO);
    let probe = detailed_probe_cdp(port);
    assert_eq!(probe.selected_target.unwrap().id, "vesktop");
    assert!(!probe.runtime.native_bridge_present);
    assert_eq!(
        probe
            .targets
            .iter()
            .filter(|target| target.is_main_renderer)
            .count(),
        1
    );
}

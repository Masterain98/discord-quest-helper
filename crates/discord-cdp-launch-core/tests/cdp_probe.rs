use discord_cdp_launch_core::{CdpProbe, CdpProbeStatus, StdCdpProbe};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

fn serve_once(response: Option<&'static str>, delay: Duration) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        if let Some(response) = response {
            let _ = stream.write_all(response.as_bytes());
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
    StdCdpProbe::with_timeouts(Duration::from_millis(500), Duration::from_millis(300))
}

#[test]
fn recognizes_a_discord_page_target() {
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
fn distinguishes_valid_chromium_without_discord() {
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
    let port = serve_once(None, Duration::from_millis(800));
    assert_eq!(fast_probe().probe(port), CdpProbeStatus::PortOccupied);
}

#[test]
fn closed_port_is_unreachable() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    assert_eq!(fast_probe().probe(port), CdpProbeStatus::Unreachable);
}

#[test]
fn unrelated_tcp_listener_is_not_misclassified_as_discord() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = stream.write_all(b"HELLO");
    });
    assert_eq!(fast_probe().probe(port), CdpProbeStatus::PortOccupied);
}

#[test]
fn complete_content_length_does_not_wait_for_connection_close() {
    let body = r#"[{"type":"page","title":"Discord","url":"https://discord.com/app","webSocketDebuggerUrl":"ws://example"}]"#;
    let response = response("200 OK", body);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        stream.write_all(response.as_bytes()).unwrap();
        std::thread::sleep(Duration::from_secs(2));
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

use crate::{
    CdpDiagnosticTarget, CdpProbeObservation, CdpProbeStatus, CdpTarget, CdpTargetClassification,
    DetailedCdpProbeResult,
};
use serde_json::Value;
use std::error::Error;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

const MAX_HTTP_RESPONSE_BYTES: usize = 1024 * 1024;

pub(crate) fn port_is_listening(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        Duration::from_millis(500),
    )
    .is_ok()
}

pub trait CdpProbe {
    fn probe(&self, port: u16) -> CdpProbeStatus;

    fn observe(&self, port: u16) -> CdpProbeObservation {
        self.probe(port).into()
    }
}

#[derive(Debug, Clone)]
pub struct StdCdpProbe {
    connect_timeout: Duration,
    io_timeout: Duration,
}

impl Default for StdCdpProbe {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_millis(500),
            io_timeout: Duration::from_secs(2),
        }
    }
}

impl StdCdpProbe {
    #[doc(hidden)]
    pub const fn with_timeouts(connect_timeout: Duration, io_timeout: Duration) -> Self {
        Self {
            connect_timeout,
            io_timeout,
        }
    }
}

impl CdpProbe for StdCdpProbe {
    fn probe(&self, port: u16) -> CdpProbeStatus {
        probe_with_timeouts(port, self.connect_timeout, self.io_timeout)
    }

    fn observe(&self, port: u16) -> CdpProbeObservation {
        detailed_probe_with_timeouts(port, self.connect_timeout, self.io_timeout).observation()
    }
}

pub fn probe_cdp(port: u16) -> CdpProbeStatus {
    StdCdpProbe::default().probe(port)
}

pub fn is_discord_target(target: &CdpTarget) -> bool {
    if target.target_type != "page" {
        return false;
    }
    url::Url::parse(&target.url).ok().is_some_and(|url| {
        matches!(url.scheme(), "https" | "http")
            && url.host_str().is_some_and(|host| {
                ["discord.com", "discordapp.com"]
                    .iter()
                    .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
            })
    })
}

// Auxiliary labels are diagnostic hints only, never target selection gates.
pub fn is_discord_auxiliary_window(target: &CdpTarget) -> bool {
    is_discord_auxiliary_page(&target.title, &target.url)
}
pub fn is_discord_auxiliary_page(title: &str, _url: &str) -> bool {
    title.eq_ignore_ascii_case("discord overlay")
}

pub fn classify_cdp_target(target: &CdpTarget) -> CdpTargetClassification {
    if target.target_type != "page" {
        return CdpTargetClassification::NotPage;
    }
    if target.url.eq_ignore_ascii_case("about:blank") {
        return CdpTargetClassification::AboutBlank;
    }
    if target.web_socket_debugger_url.is_none() {
        return CdpTargetClassification::MissingWebSocketDebuggerUrl;
    }
    if !is_discord_target(target) {
        return CdpTargetClassification::NonDiscordPage;
    }
    CdpTargetClassification::DiscordOtherRenderer
}

fn sanitized_target_url(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_string()
}

fn diagnostic_target(target: &CdpTarget, runtime: crate::CdpRuntime) -> CdpDiagnosticTarget {
    let classification = if runtime.runtime_status == crate::CdpRuntimeStatus::Ready {
        CdpTargetClassification::DiscordMainRenderer
    } else if is_discord_auxiliary_window(target) {
        CdpTargetClassification::DiscordAuxiliary
    } else {
        classify_cdp_target(target)
    };
    CdpDiagnosticTarget {
        id: target.id.clone(),
        target_type: target.target_type.clone(),
        title: target.title.clone(),
        url: sanitized_target_url(&target.url),
        has_web_socket_debugger_url: target.web_socket_debugger_url.is_some(),
        is_discord_target: is_discord_target(target),
        is_auxiliary_window: is_discord_auxiliary_window(target),
        is_main_renderer: classification == CdpTargetClassification::DiscordMainRenderer,
        classification,
        runtime,
    }
}

/// Loopback `/json` discovery error. This path never uses an HTTP proxy.
#[derive(Debug)]
pub enum CdpListError {
    Unreachable { port: u16 },
    ConnectionFailed { port: u16, source: io::Error },
    IncompleteResponse { port: u16 },
    HttpStatus { port: u16, status: u16 },
    InvalidResponse { port: u16, details: String },
}

impl CdpListError {
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Unreachable { .. } | Self::IncompleteResponse { .. } => true,
            Self::ConnectionFailed { source, .. } => is_transient_cdp_io_error(source),
            Self::HttpStatus { status, .. } => (500..600).contains(status),
            Self::InvalidResponse { .. } => false,
        }
    }
}

impl fmt::Display for CdpListError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable { port } => {
                write!(formatter, "CDP endpoint unreachable at 127.0.0.1:{port}")
            }
            Self::ConnectionFailed { port, source } => write!(
                formatter,
                "CDP endpoint reset / unreachable at 127.0.0.1:{port}: {source}"
            ),
            Self::IncompleteResponse { port } => write!(
                formatter,
                "CDP endpoint reset / unreachable at 127.0.0.1:{port}: incomplete /json response"
            ),
            Self::HttpStatus { port, status } => {
                write!(
                    formatter,
                    "CDP endpoint at 127.0.0.1:{port} returned HTTP {status}"
                )
            }
            Self::InvalidResponse { port, details } => {
                write!(
                    formatter,
                    "CDP endpoint at 127.0.0.1:{port} returned an invalid /json body: {details}"
                )
            }
        }
    }
}

impl Error for CdpListError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ConnectionFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn is_transient_cdp_io_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::TimedOut
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::Interrupted
    ) || matches!(error.raw_os_error(), Some(10053 | 10054 | 10060 | 10061))
}

/// List every CDP target on the loopback DevTools HTTP server.
///
/// Uses raw HTTP/1.1 to `127.0.0.1` and never consults a system or environment proxy.
pub fn list_cdp_targets(port: u16) -> Result<Vec<CdpTarget>, CdpListError> {
    list_cdp_targets_with_timeouts(port, Duration::from_secs(2), Duration::from_secs(3))
}

pub fn list_cdp_targets_with_timeouts(
    port: u16,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> Result<Vec<CdpTarget>, CdpListError> {
    let response = fetch_cdp_http_response(port, connect_timeout, io_timeout)?;
    parse_cdp_targets_http_response(port, &response)
}

fn fetch_cdp_http_response(
    port: u16,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> Result<Vec<u8>, CdpListError> {
    if port == 0 {
        return Err(CdpListError::Unreachable { port });
    }
    let deadline = Instant::now() + (connect_timeout + io_timeout).min(Duration::from_secs(3));
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream = TcpStream::connect_timeout(
        &address,
        connect_timeout.min(deadline.saturating_duration_since(Instant::now())),
    )
    .map_err(|_| CdpListError::Unreachable { port })?;
    let _ = stream.set_read_timeout(Some(io_timeout));
    let _ = stream.set_write_timeout(Some(
        io_timeout.min(deadline.saturating_duration_since(Instant::now())),
    ));
    let request =
        format!("GET /json HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|source| CdpListError::ConnectionFailed { port, source })?;
    read_http_response(
        &mut stream,
        port,
        deadline.saturating_duration_since(Instant::now()),
    )
}

pub fn detailed_probe_cdp(port: u16) -> DetailedCdpProbeResult {
    let probe = StdCdpProbe::default();
    detailed_probe_with_timeouts(port, probe.connect_timeout, probe.io_timeout)
}

fn detailed_probe_with_timeouts(
    port: u16,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> DetailedCdpProbeResult {
    let deadline = Instant::now() + Duration::from_secs(3);
    let response = match fetch_cdp_http_response(port, connect_timeout, io_timeout) {
        Ok(response) => response,
        Err(CdpListError::Unreachable { .. }) => {
            return DetailedCdpProbeResult {
                status: CdpProbeStatus::Unreachable,
                port_listening: false,
                http_reachable: false,
                http_status: None,
                response_parseable: false,
                targets: Vec::new(),
                runtime: crate::CdpRuntime {
                    runtime_status: crate::CdpRuntimeStatus::ProbeFailed,
                    failure_stage: Some("discovery".into()),
                    reason_code: Some("endpoint_unreachable".into()),
                    ..Default::default()
                },
                selected_target: None,
            };
        }
        Err(_) => {
            return DetailedCdpProbeResult {
                status: CdpProbeStatus::PortOccupied,
                port_listening: true,
                http_reachable: false,
                http_status: None,
                response_parseable: false,
                targets: Vec::new(),
                runtime: crate::CdpRuntime {
                    runtime_status: crate::CdpRuntimeStatus::ProbeFailed,
                    failure_stage: Some("discovery".into()),
                    reason_code: Some("http_failed".into()),
                    ..Default::default()
                },
                selected_target: None,
            };
        }
    };
    let http_status = http_status(&response);
    match parse_cdp_targets_http_response(port, &response) {
        Ok(raw_targets) => {
            let mut candidates: Vec<_> = raw_targets
                .iter()
                .filter(|t| is_discord_target(t))
                .collect();
            candidates.sort_by(|a, b| a.id.cmp(&b.id));
            let runtimes: std::collections::HashMap<_, _> = candidates
                .iter()
                .map(|target| {
                    (
                        target.id.clone(),
                        crate::runtime::verify(port, target, deadline),
                    )
                })
                .collect();
            let selected = candidates
                .iter()
                .filter(|target| {
                    runtimes[&target.id].runtime_status == crate::CdpRuntimeStatus::Ready
                })
                .min_by_key(|target| crate::runtime::preference(&runtimes[&target.id], &target.id));
            let selected_target = selected.map(|target| (*target).clone());
            let runtime = selected
                .map(|target| runtimes[&target.id].clone())
                .unwrap_or_else(|| {
                    candidates
                        .iter()
                        .filter_map(|target| runtimes.get(&target.id))
                        .min_by_key(|runtime| match runtime.runtime_status {
                            crate::CdpRuntimeStatus::Loading => 0,
                            crate::CdpRuntimeStatus::ProbeFailed => 1,
                            _ => 2,
                        })
                        .cloned()
                        .unwrap_or_default()
                });
            let status = selected_target.as_ref().map_or(
                CdpProbeStatus::CdpWithoutDiscordTarget,
                |target| CdpProbeStatus::DiscordReady {
                    target_title: Some(target.title.clone()),
                },
            );
            DetailedCdpProbeResult {
                status,
                port_listening: true,
                http_reachable: true,
                http_status,
                response_parseable: true,
                targets: raw_targets
                    .iter()
                    .map(|target| {
                        diagnostic_target(
                            target,
                            runtimes.get(&target.id).cloned().unwrap_or_default(),
                        )
                    })
                    .collect(),
                runtime,
                selected_target,
            }
        }
        Err(_) => DetailedCdpProbeResult {
            status: CdpProbeStatus::PortOccupied,
            port_listening: true,
            http_reachable: http_status.is_some(),
            http_status,
            response_parseable: false,
            targets: Vec::new(),
            runtime: crate::CdpRuntime {
                runtime_status: crate::CdpRuntimeStatus::ProbeFailed,
                failure_stage: Some("discovery".into()),
                reason_code: Some("invalid_target_list".into()),
                ..Default::default()
            },
            selected_target: None,
        },
    }
}

fn http_status(response: &[u8]) -> Option<u16> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&response[..header_end]).ok()?;
    head.lines().next()?.split_whitespace().nth(1)?.parse().ok()
}

fn probe_with_timeouts(
    port: u16,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> CdpProbeStatus {
    detailed_probe_with_timeouts(port, connect_timeout, io_timeout).status
}

fn read_http_response(
    stream: &mut TcpStream,
    port: u16,
    timeout: Duration,
) -> Result<Vec<u8>, CdpListError> {
    let mut response = Vec::new();
    let mut buffer = [0_u8; 4096];
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            break;
        }
        let _ = stream.set_read_timeout(Some(
            deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1)),
        ));
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if response.len().saturating_add(read) > MAX_HTTP_RESPONSE_BYTES {
                    return Err(CdpListError::InvalidResponse {
                        port,
                        details: "response exceeded 1 MiB".to_string(),
                    });
                }
                response.extend_from_slice(&buffer[..read]);
                if http_response_is_complete(&response) {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(source) => {
                // Chromium and Windows test sockets often RST after a complete
                // /json body. Keep the bytes we already parsed.
                if http_response_is_complete(&response) {
                    break;
                }
                return Err(CdpListError::ConnectionFailed { port, source });
            }
        }
    }
    if response.is_empty() {
        return Err(CdpListError::IncompleteResponse { port });
    }
    Ok(response)
}

fn http_content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    })
}

fn http_response_is_complete(response: &[u8]) -> bool {
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let Ok(head) = std::str::from_utf8(&response[..header_end]) else {
        return false;
    };
    let content_length = http_content_length(head);
    content_length.is_some_and(|length| response.len() >= header_end + 4 + length)
}

pub fn parse_cdp_targets_http_response(
    port: u16,
    response: &[u8],
) -> Result<Vec<CdpTarget>, CdpListError> {
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Err(CdpListError::InvalidResponse {
            port,
            details: "missing HTTP header terminator".to_string(),
        });
    };
    let (head, body_with_separator) = response.split_at(header_end);
    let body = &body_with_separator[4..];
    let Ok(head) = std::str::from_utf8(head) else {
        return Err(CdpListError::InvalidResponse {
            port,
            details: "HTTP headers were not valid UTF-8".to_string(),
        });
    };
    if let Some(content_length) = http_content_length(head) {
        if body.len() < content_length {
            return Err(CdpListError::IncompleteResponse { port });
        }
    }
    let Some(status_line) = head.lines().next() else {
        return Err(CdpListError::InvalidResponse {
            port,
            details: "missing HTTP status line".to_string(),
        });
    };
    let mut status_parts = status_line.split_whitespace();
    if status_parts.next().is_none() {
        return Err(CdpListError::InvalidResponse {
            port,
            details: "missing HTTP version".to_string(),
        });
    }
    let status = status_parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| CdpListError::InvalidResponse {
            port,
            details: "missing HTTP status code".to_string(),
        })?;
    if !(200..=299).contains(&status) {
        return Err(CdpListError::HttpStatus { port, status });
    }

    let value =
        serde_json::from_slice::<Value>(body).map_err(|error| CdpListError::InvalidResponse {
            port,
            details: error.to_string(),
        })?;
    let items = value
        .as_array()
        .ok_or_else(|| CdpListError::InvalidResponse {
            port,
            details: "JSON root was not an array".to_string(),
        })?;
    Ok(items.iter().filter_map(target_from_value).collect())
}

fn target_from_value(value: &Value) -> Option<CdpTarget> {
    let object = value.as_object()?;
    Some(CdpTarget {
        id: object
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        target_type: object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        title: object
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        url: object
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        web_socket_debugger_url: object
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CdpTarget;

    fn target(id: &str, title: &str, url: &str) -> CdpTarget {
        CdpTarget {
            id: id.to_string(),
            target_type: "page".to_string(),
            title: title.to_string(),
            url: url.to_string(),
            web_socket_debugger_url: Some("ws://example".to_string()),
        }
    }

    #[test]
    fn candidates_use_exact_domains_and_not_routes_or_titles() {
        assert!(is_discord_target(&target(
            "a",
            "",
            "https://discord.com/future/unknown"
        )));
        assert!(is_discord_target(&target(
            "a",
            "",
            "https://canary.discord.com/anything"
        )));
        assert!(!is_discord_target(&target(
            "a",
            "Discord",
            "https://discord.com.evil.test/app"
        )));
        assert!(!is_discord_target(&target(
            "a",
            "Discord",
            "https://example.com/discord.com"
        )));
        assert!(!is_discord_target(&target("a", "Discord", "about:blank")));
    }

    fn http_json(status: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn parse_lists_pages_and_workers_from_json_fixture() {
        let body = r#"[{"id":"1","type":"page","title":"Friends","url":"https://discord.com/channels/@me","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/1"},{"id":"2","type":"worker","title":"","url":""}]"#;
        let targets = parse_cdp_targets_http_response(9223, &http_json("200 OK", body)).unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].id, "1");
        assert_eq!(targets[0].title, "Friends");
        assert_eq!(targets[1].target_type, "worker");
    }

    #[test]
    fn parse_rejects_http_400_and_malformed_json() {
        let bad_status = parse_cdp_targets_http_response(9223, &http_json("400 Bad Request", "[]"));
        assert!(matches!(
            bad_status,
            Err(CdpListError::HttpStatus {
                port: 9223,
                status: 400
            })
        ));
        assert!(!bad_status.unwrap_err().is_transient());

        let bad_json = parse_cdp_targets_http_response(9223, &http_json("200 OK", "not json"));
        assert!(matches!(
            bad_json,
            Err(CdpListError::InvalidResponse { .. })
        ));
        assert!(!bad_json.unwrap_err().is_transient());
    }

    #[test]
    fn windows_connection_reset_and_refused_are_transient() {
        let reset = CdpListError::ConnectionFailed {
            port: 9223,
            source: io::Error::from_raw_os_error(10054),
        };
        let refused = CdpListError::ConnectionFailed {
            port: 9223,
            source: io::Error::from_raw_os_error(10061),
        };
        assert!(reset.is_transient());
        assert!(refused.is_transient());
        assert!(is_transient_cdp_io_error(&io::Error::from_raw_os_error(
            10054
        )));
        assert!(is_transient_cdp_io_error(&io::Error::from_raw_os_error(
            10061
        )));
    }
}

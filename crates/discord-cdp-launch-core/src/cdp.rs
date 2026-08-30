use crate::{CdpProbeStatus, CdpTarget};
use serde_json::Value;
use std::error::Error;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

const MAX_HTTP_RESPONSE_BYTES: usize = 1024 * 1024;

pub trait CdpProbe {
    fn probe(&self, port: u16) -> CdpProbeStatus;
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
}

pub fn probe_cdp(port: u16) -> CdpProbeStatus {
    StdCdpProbe::default().probe(port)
}

pub fn is_discord_target(target: &CdpTarget) -> bool {
    if target.target_type != "page" {
        return false;
    }

    let title = target.title.to_ascii_lowercase();
    let url = target.url.to_ascii_lowercase();
    if title.contains("updater") || url == "about:blank" {
        return false;
    }

    title.contains("discord") || url.contains("discord.com") || url.contains("discordapp.com")
}

/// Overlay and popout windows are Discord pages, but they do not load
/// `webpackChunkdiscord_app` or `DiscordNative`. Live Discord CDP lists
/// `Discord Overlay` (`https://discord.com/popout`) before the main renderer.
pub fn is_discord_auxiliary_window(target: &CdpTarget) -> bool {
    is_discord_auxiliary_page(&target.title, &target.url)
}

/// Title/URL form used when CDP execution results no longer carry a full target.
pub fn is_discord_auxiliary_page(title: &str, url: &str) -> bool {
    if title.eq_ignore_ascii_case("discord overlay") {
        return true;
    }

    let path = discord_target_path(url);
    path == "popout"
        || path.starts_with("popout/")
        || path == "overlay"
        || path.starts_with("overlay/")
}

fn discord_target_path(url: &str) -> String {
    let lowered = url.to_ascii_lowercase();
    let without_scheme = lowered
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&lowered);
    let path = without_scheme
        .split_once('/')
        .map(|(_, path)| path)
        .unwrap_or("");
    path.split(['?', '#']).next().unwrap_or(path).to_string()
}

fn is_discord_main_renderer(target: &CdpTarget) -> bool {
    let path = discord_target_path(&target.url);
    path == "app"
        || path.starts_with("app/")
        || path == "login"
        || path.starts_with("login/")
        || path.starts_with("channels/")
        || path == "quest-home"
        || path.starts_with("quest-home/")
}

pub fn pick_discord_target(targets: &[CdpTarget]) -> Option<&CdpTarget> {
    let is_candidate = |target: &&CdpTarget| {
        is_discord_target(target)
            && target.web_socket_debugger_url.is_some()
            && !is_discord_auxiliary_window(target)
    };

    targets
        .iter()
        .filter(is_candidate)
        .find(|target| is_discord_main_renderer(target))
        .or_else(|| targets.iter().find(is_candidate))
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
    if port == 0 {
        return Err(CdpListError::Unreachable { port });
    }

    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream = TcpStream::connect_timeout(&address, connect_timeout)
        .map_err(|_| CdpListError::Unreachable { port })?;

    let _ = stream.set_read_timeout(Some(io_timeout));
    let _ = stream.set_write_timeout(Some(io_timeout));
    let request =
        format!("GET /json HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");

    stream
        .write_all(request.as_bytes())
        .map_err(|source| CdpListError::ConnectionFailed { port, source })?;

    let response = read_http_response(&mut stream, port, io_timeout)?;
    parse_cdp_targets_http_response(port, &response)
}

fn probe_with_timeouts(
    port: u16,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> CdpProbeStatus {
    match list_cdp_targets_with_timeouts(port, connect_timeout, io_timeout) {
        Ok(targets) => {
            if let Some(target) = pick_discord_target(&targets) {
                CdpProbeStatus::DiscordReady {
                    target_title: (!target.title.is_empty()).then(|| target.title.clone()),
                }
            } else {
                CdpProbeStatus::CdpWithoutDiscordTarget
            }
        }
        Err(CdpListError::Unreachable { .. }) => CdpProbeStatus::Unreachable,
        Err(_) => CdpProbeStatus::PortOccupied,
    }
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
    fn pick_skips_overlay_in_favor_of_main_renderer() {
        let targets = vec![
            target("overlay", "Discord Overlay", "https://discord.com/popout"),
            target("friends", "Friends", "https://discord.com/channels/@me"),
        ];
        let picked = pick_discord_target(&targets).unwrap();
        assert_eq!(picked.id, "friends");
    }

    #[test]
    fn overlay_only_is_not_a_primary_target() {
        let targets = vec![target(
            "overlay",
            "Discord Overlay",
            "https://discord.com/popout",
        )];
        assert!(pick_discord_target(&targets).is_none());
    }

    #[test]
    fn pick_prefers_channels_over_other_discord_pages() {
        let targets = vec![
            target("store", "Discord Store", "https://discord.com/store"),
            target("me", "Friends", "https://canary.discord.com/channels/@me"),
        ];
        let picked = pick_discord_target(&targets).unwrap();
        assert_eq!(picked.id, "me");
    }

    #[test]
    fn application_directory_is_not_treated_as_app_renderer() {
        let targets = vec![
            target(
                "directory",
                "App Directory",
                "https://discord.com/application-directory",
            ),
            target("app", "Discord", "https://discord.com/app"),
        ];
        let picked = pick_discord_target(&targets).unwrap();
        assert_eq!(picked.id, "app");
    }

    #[test]
    fn overlay_is_still_a_discord_page_target() {
        let overlay = target("overlay", "Discord Overlay", "https://discord.com/popout");
        assert!(is_discord_target(&overlay));
        assert!(is_discord_auxiliary_window(&overlay));
        assert!(is_discord_auxiliary_page(
            "Discord Overlay",
            "https://discord.com/popout"
        ));
        assert!(!is_discord_auxiliary_page(
            "Friends",
            "https://discord.com/channels/@me"
        ));
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

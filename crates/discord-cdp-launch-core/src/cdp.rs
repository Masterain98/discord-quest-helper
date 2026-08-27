use crate::{CdpProbeStatus, CdpTarget};
use serde_json::Value;
use std::io::{Read, Write};
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

fn probe_with_timeouts(
    port: u16,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> CdpProbeStatus {
    if port == 0 {
        return CdpProbeStatus::Unreachable;
    }

    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let Ok(mut stream) = TcpStream::connect_timeout(&address, connect_timeout) else {
        return CdpProbeStatus::Unreachable;
    };

    let _ = stream.set_read_timeout(Some(io_timeout));
    let _ = stream.set_write_timeout(Some(io_timeout));
    let request =
        format!("GET /json HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");

    if stream.write_all(request.as_bytes()).is_err() {
        return CdpProbeStatus::PortOccupied;
    }

    let Some(response) = read_http_response(&mut stream, io_timeout) else {
        return CdpProbeStatus::PortOccupied;
    };
    parse_http_response(&response)
}

fn read_http_response(stream: &mut TcpStream, timeout: Duration) -> Option<Vec<u8>> {
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
                    return None;
                }
                response.extend_from_slice(&buffer[..read]);
                if http_response_is_complete(&response) {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(_) => return None,
        }
    }
    (!response.is_empty()).then_some(response)
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

fn parse_http_response(response: &[u8]) -> CdpProbeStatus {
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return CdpProbeStatus::PortOccupied;
    };
    let (head, body_with_separator) = response.split_at(header_end);
    let body = &body_with_separator[4..];
    let Ok(head) = std::str::from_utf8(head) else {
        return CdpProbeStatus::PortOccupied;
    };
    if let Some(content_length) = http_content_length(head) {
        if body.len() < content_length {
            return CdpProbeStatus::PortOccupied;
        }
    }
    let Some(status_line) = head.lines().next() else {
        return CdpProbeStatus::PortOccupied;
    };
    let mut status_parts = status_line.split_whitespace();
    if status_parts.next().is_none()
        || !matches!(
            status_parts
                .next()
                .and_then(|value| value.parse::<u16>().ok()),
            Some(200..=299)
        )
    {
        return CdpProbeStatus::PortOccupied;
    }

    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return CdpProbeStatus::PortOccupied;
    };
    let Some(items) = value.as_array() else {
        return CdpProbeStatus::PortOccupied;
    };
    let targets: Vec<CdpTarget> = items.iter().filter_map(target_from_value).collect();

    if let Some(target) = pick_discord_target(&targets) {
        CdpProbeStatus::DiscordReady {
            target_title: (!target.title.is_empty()).then(|| target.title.clone()),
        }
    } else {
        CdpProbeStatus::CdpWithoutDiscordTarget
    }
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
    fn vesktop_friends_page_is_a_main_discord_target() {
        let vesktop = CdpTarget {
            id: "vesktop-main".to_string(),
            target_type: "page".to_string(),
            title: "\u{0007} Discord | Friends".to_string(),
            url: "https://discord.com/channels/@me".to_string(),
            web_socket_debugger_url: Some("ws://127.0.0.1:9223/devtools/page/1".to_string()),
        };
        assert!(is_discord_target(&vesktop));
        assert!(!is_discord_auxiliary_window(&vesktop));
        assert_eq!(pick_discord_target(&[vesktop]).unwrap().id, "vesktop-main");
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
}

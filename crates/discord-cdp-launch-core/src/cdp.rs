use crate::{CdpProbeStatus, CdpTarget};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

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

pub fn pick_discord_target(targets: &[CdpTarget]) -> Option<&CdpTarget> {
    targets
        .iter()
        .find(|target| is_discord_target(target) && target.web_socket_debugger_url.is_some())
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

    let Some(response) = read_http_response(&mut stream) else {
        return CdpProbeStatus::PortOccupied;
    };
    parse_http_response(&response)
}

fn read_http_response(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut response = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
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

fn http_response_is_complete(response: &[u8]) -> bool {
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let Ok(head) = std::str::from_utf8(&response[..header_end]) else {
        return false;
    };
    let content_length = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    });
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
    if let Some(content_length) = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    }) {
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

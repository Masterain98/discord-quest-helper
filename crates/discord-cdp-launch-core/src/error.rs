use crate::DiscordChannel;
use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug)]
pub enum LaunchError {
    InvalidPort(u16),
    UnsupportedChannel(String),
    UnsupportedPlatform,
    InstallNotFound {
        channel: Option<DiscordChannel>,
    },
    DiscordAlreadyRunning {
        channel: Option<DiscordChannel>,
    },
    ProcessInspection {
        operation: &'static str,
        source: io::Error,
    },
    ProcessTermination {
        process: String,
        details: String,
    },
    ShutdownTimeout {
        timeout: Duration,
    },
    PortOccupied {
        port: u16,
    },
    NonDiscordCdpTarget {
        port: u16,
    },
    SpawnFailed {
        path: PathBuf,
        source: io::Error,
    },
    ReadinessTimeout {
        port: u16,
        timeout: Duration,
    },
    CdpProtocol {
        details: String,
    },
}

impl fmt::Display for LaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPort(_) => write!(formatter, "CDP port must be between 1 and 65535."),
            Self::UnsupportedChannel(value) => {
                write!(formatter, "Unsupported Discord channel: {value}")
            }
            Self::UnsupportedPlatform => write!(
                formatter,
                "Discord CDP launcher is only supported on Windows, macOS, and Linux."
            ),
            Self::InstallNotFound {
                channel: Some(channel),
            } => write!(
                formatter,
                "Could not find Discord {} installation.",
                channel.display_name()
            ),
            Self::InstallNotFound { channel: None } => {
                write!(formatter, "Could not find Discord installation.")
            }
            Self::DiscordAlreadyRunning { channel } => {
                let channel = channel
                    .map(DiscordChannel::display_name)
                    .unwrap_or("Discord");
                write!(
                    formatter,
                    "{channel} is already running without CDP. Restart it to close it and relaunch with CDP."
                )
            }
            Self::ProcessInspection { operation, source } => {
                write!(formatter, "Could not execute {operation}: {source}")
            }
            Self::ProcessTermination { process, details } => {
                write!(formatter, "Failed to terminate {process}: {details}")
            }
            Self::ShutdownTimeout { timeout } => write!(
                formatter,
                "Discord did not exit within {} seconds. Please close Discord manually and try again.",
                timeout.as_secs()
            ),
            Self::PortOccupied { port } => {
                write!(formatter, "CDP port {port} is already used by another process.")
            }
            Self::NonDiscordCdpTarget { port } => write!(
                formatter,
                "CDP port {port} is already used by a non-Discord CDP target."
            ),
            Self::SpawnFailed { path, source } => write!(
                formatter,
                "Failed to launch Discord with CDP from '{}': {source}",
                path.display()
            ),
            Self::ReadinessTimeout { port, timeout } => write!(
                formatter,
                "Discord was launched, but CDP did not become available on port {port} within {} seconds.",
                timeout.as_secs()
            ),
            Self::CdpProtocol { details } => {
                write!(formatter, "CDP protocol error: {details}")
            }
        }
    }
}

impl Error for LaunchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ProcessInspection { source, .. } | Self::SpawnFailed { source, .. } => {
                Some(source)
            }
            _ => None,
        }
    }
}

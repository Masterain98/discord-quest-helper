use crate::launcher::build_launch_args;
use crate::{DiscordChannel, DiscordInstall, LaunchError};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

pub(crate) fn find_installs() -> Result<Vec<DiscordInstall>, LaunchError> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }
    Ok(discover_macos_installs_in(&roots))
}

#[doc(hidden)]
pub fn discover_macos_installs_in(roots: &[PathBuf]) -> Vec<DiscordInstall> {
    let specs = [
        (DiscordChannel::Stable, "Discord.app", ["Discord", ""]),
        (
            DiscordChannel::Ptb,
            "Discord PTB.app",
            ["Discord PTB", "Discord"],
        ),
        (
            DiscordChannel::Canary,
            "Discord Canary.app",
            ["Discord Canary", "Discord"],
        ),
    ];
    let mut installs = Vec::new();
    for (channel, app_name, executable_names) in specs {
        'roots: for root in roots {
            let macos_dir = root.join(app_name).join("Contents").join("MacOS");
            for executable_name in executable_names {
                if executable_name.is_empty() {
                    continue;
                }
                let executable_path = macos_dir.join(executable_name);
                if executable_path.is_file() {
                    installs.push(DiscordInstall {
                        channel,
                        executable_path,
                        working_dir: macos_dir,
                    });
                    break 'roots;
                }
            }
        }
    }
    installs
}

pub(crate) fn is_running(channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
    for name in process_names_for(channel) {
        let status = Command::new("pgrep")
            .args(["-x", name])
            .status()
            .map_err(|source| LaunchError::ProcessInspection {
                operation: "pgrep",
                source,
            })?;
        if status.success() {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn terminate(channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
    for name in process_names_for(channel) {
        let script = format!("tell application \"{}\" to quit", name.replace('"', "\\\""));
        let _ = Command::new("osascript").args(["-e", &script]).output();
    }
    std::thread::sleep(Duration::from_secs(3));
    let mut first_error = None;
    for name in process_names_for(channel) {
        let output = Command::new("pkill")
            .args(["-x", name])
            .output()
            .map_err(|source| LaunchError::ProcessInspection {
                operation: "pkill",
                source,
            })?;
        if !output.status.success() && output.status.code() != Some(1) && first_error.is_none() {
            first_error = Some(LaunchError::ProcessTermination {
                process: name.to_string(),
                details: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
    }
    first_error.map_or(Ok(()), Err)
}

pub(crate) fn spawn(
    install: &DiscordInstall,
    port: u16,
    allow_origins: bool,
) -> Result<u32, LaunchError> {
    let mut command = Command::new(&install.executable_path);
    command
        .current_dir(&install.working_dir)
        .args(build_launch_args(port, allow_origins));
    command
        .spawn()
        .map(|mut child| {
            let pid = child.id();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            pid
        })
        .map_err(|source| LaunchError::SpawnFailed {
            path: install.executable_path.clone(),
            source,
        })
}

fn process_names_for(channel: Option<DiscordChannel>) -> Vec<&'static str> {
    match channel {
        Some(channel) => vec![process_name(channel)],
        None => DiscordChannel::ALL
            .iter()
            .copied()
            .map(process_name)
            .collect(),
    }
}

fn process_name(channel: DiscordChannel) -> &'static str {
    match channel {
        DiscordChannel::Stable => "Discord",
        DiscordChannel::Ptb => "Discord PTB",
        DiscordChannel::Canary => "Discord Canary",
    }
}

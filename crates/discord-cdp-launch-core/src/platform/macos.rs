use crate::launcher::build_launch_args;
use crate::vesktop::VesktopInstall;
use crate::{DiscordChannel, DiscordInstall, DiscordLaunchMode, LaunchError};
use std::path::PathBuf;
use std::process::{Command, Stdio};
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
        let status = Command::new("/usr/bin/pgrep")
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
        let _ = Command::new("/usr/bin/osascript")
            .args(["-e", &script])
            .output();
    }
    std::thread::sleep(Duration::from_secs(3));
    let mut first_error = None;
    for name in process_names_for(channel) {
        let output = Command::new("/usr/bin/pkill")
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

pub(crate) fn spawn(install: &DiscordInstall, mode: DiscordLaunchMode) -> Result<u32, LaunchError> {
    let bundle_path = macos_app_bundle_path(&install.executable_path);
    if let Some(bundle_path) = bundle_path {
        let mut command = Command::new("/usr/bin/open");
        command
            .args(macos_bundle_launch_args(bundle_path, mode))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        return command
            .spawn()
            .map(|mut child| {
                let pid = child.id();
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                pid
            })
            .map_err(|source| LaunchError::SpawnFailed {
                path: bundle_path.to_path_buf(),
                source,
            });
    }

    let mut command = Command::new(&install.executable_path);
    command
        .current_dir(&install.working_dir)
        .args(build_launch_args(mode))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
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

fn macos_app_bundle_path(executable_path: &std::path::Path) -> Option<&std::path::Path> {
    executable_path
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
}

fn macos_bundle_launch_args(
    bundle_path: &std::path::Path,
    mode: DiscordLaunchMode,
) -> Vec<std::ffi::OsString> {
    let mut args = vec![
        "-n".into(),
        "-a".into(),
        bundle_path.as_os_str().to_owned(),
        "--args".into(),
    ];
    args.extend(build_launch_args(mode));
    args
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

pub(crate) fn is_vesktop_running() -> Result<bool, LaunchError> {
    for name in ["Vesktop", "vesktop"] {
        let status = Command::new("/usr/bin/pgrep")
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

pub(crate) fn spawn_vesktop(
    install: &VesktopInstall,
    mode: DiscordLaunchMode,
) -> Result<u32, LaunchError> {
    let mut command = Command::new(&install.executable_path);
    command
        .current_dir(&install.working_dir)
        .args(build_launch_args(mode))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
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

#[cfg(test)]
mod tests {
    use super::{macos_app_bundle_path, macos_bundle_launch_args};
    use crate::DiscordLaunchMode;
    use std::path::Path;

    #[test]
    fn finds_app_bundle_for_inner_discord_executable() {
        assert_eq!(
            macos_app_bundle_path(Path::new(
                "/Applications/Discord.app/Contents/MacOS/Discord"
            )),
            Some(Path::new("/Applications/Discord.app"))
        );
        assert_eq!(macos_app_bundle_path(Path::new("/opt/discord")), None);
    }

    #[test]
    fn passes_the_requested_debugging_port_through_launch_services() {
        assert_eq!(
            macos_bundle_launch_args(
                Path::new("/Applications/Discord.app"),
                DiscordLaunchMode::Cdp { port: 9223 }
            ),
            vec![
                "-n",
                "-a",
                "/Applications/Discord.app",
                "--args",
                "--remote-debugging-port=9223"
            ]
        );
    }
}

use crate::launcher::{build_launch_args, PlatformBackend};
use crate::{DiscordChannel, DiscordInstall, LaunchError};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) fn find_installs() -> Result<Vec<DiscordInstall>, LaunchError> {
    let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") else {
        return Ok(Vec::new());
    };
    Ok(discover_windows_installs_in(Path::new(&local_appdata)))
}

#[doc(hidden)]
pub fn discover_windows_installs_in(base: &Path) -> Vec<DiscordInstall> {
    channel_specs()
        .into_iter()
        .filter_map(|(channel, folder, executable)| {
            let channel_path = base.join(folder);
            find_channel_executable(&channel_path, executable).map(|executable_path| {
                let working_dir = executable_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| channel_path.clone());
                DiscordInstall {
                    channel,
                    executable_path,
                    working_dir,
                }
            })
        })
        .collect()
}

pub(crate) fn is_running(channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
    let output = no_window_cmd("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .map_err(|source| LaunchError::ProcessInspection {
            operation: "tasklist",
            source,
        })?;
    if !output.status.success() {
        return Err(LaunchError::ProcessTermination {
            process: "tasklist".to_string(),
            details: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    Ok(process_names_for(channel)
        .iter()
        .any(|name| stdout.contains(&format!("\"{}\"", name.to_ascii_lowercase()))))
}

pub(crate) fn terminate(channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
    for name in process_names_for(channel) {
        let output = no_window_cmd("taskkill")
            .args(["/IM", name, "/T", "/F"])
            .output()
            .map_err(|source| LaunchError::ProcessInspection {
                operation: "taskkill",
                source,
            })?;
        if !output.status.success() {
            let details = String::from_utf8_lossy(&output.stderr);
            eprintln!("taskkill for {name} returned non-zero: {}", details.trim());
        }
    }
    Ok(())
}

pub(crate) fn spawn(
    install: &DiscordInstall,
    port: u16,
    allow_origins: bool,
) -> Result<u32, LaunchError> {
    let mut command = Command::new(&install.executable_path);
    command
        .current_dir(&install.working_dir)
        .args(build_launch_args(port, allow_origins))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
        .spawn()
        .map(|child| child.id())
        .map_err(|source| LaunchError::SpawnFailed {
            path: install.executable_path.clone(),
            source,
        })
}

fn channel_specs() -> [(DiscordChannel, &'static str, &'static str); 3] {
    [
        (DiscordChannel::Stable, "Discord", "Discord.exe"),
        (DiscordChannel::Ptb, "DiscordPTB", "DiscordPTB.exe"),
        (DiscordChannel::Canary, "DiscordCanary", "DiscordCanary.exe"),
    ]
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
        DiscordChannel::Stable => "Discord.exe",
        DiscordChannel::Ptb => "DiscordPTB.exe",
        DiscordChannel::Canary => "DiscordCanary.exe",
    }
}

fn find_channel_executable(channel_path: &Path, executable: &str) -> Option<PathBuf> {
    let mut app_dirs = std::fs::read_dir(channel_path)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter(|entry| {
            entry.file_type().is_ok_and(|file_type| file_type.is_dir())
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("app-")
        })
        .collect::<Vec<_>>();
    app_dirs.sort_by(|left, right| {
        parse_app_version(&right.file_name())
            .cmp(&parse_app_version(&left.file_name()))
            .then_with(|| right.file_name().cmp(&left.file_name()))
    });

    app_dirs
        .into_iter()
        .map(|entry| entry.path().join(executable))
        .find(|path| path.is_file())
        .or_else(|| {
            let direct = channel_path.join(executable);
            direct.is_file().then_some(direct)
        })
}

fn parse_app_version(name: &OsStr) -> Vec<u32> {
    let name = name.to_string_lossy();
    name.get(..4)
        .filter(|prefix| prefix.eq_ignore_ascii_case("app-"))
        .map(|_| &name[4..])
        .unwrap_or_default()
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn no_window_cmd(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[allow(dead_code)]
fn _assert_backend_object_safe(_backend: &dyn PlatformBackend) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions_numerically() {
        assert!(
            parse_app_version(OsStr::new("app-1.0.10000"))
                > parse_app_version(OsStr::new("app-1.0.9999"))
        );
        assert!(
            parse_app_version(OsStr::new("app-1.1.0"))
                > parse_app_version(OsStr::new("app-1.0.99999"))
        );
    }

    #[test]
    fn parses_case_insensitive_app_directory_prefix() {
        assert_eq!(parse_app_version(OsStr::new("App-1.2.3")), vec![1, 2, 3]);
    }
}

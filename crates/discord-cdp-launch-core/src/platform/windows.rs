use crate::launcher::build_launch_args;
use crate::vesktop::VesktopInstall;
use crate::{DiscordChannel, DiscordInstall, DiscordLaunchMode, LaunchError};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) mod shutdown;

pub(crate) fn find_installs() -> Result<Vec<DiscordInstall>, LaunchError> {
    Ok(find_installs_with_issues().0)
}

pub(crate) fn find_installs_with_issues() -> (Vec<DiscordInstall>, Vec<String>) {
    let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") else {
        return (Vec::new(), Vec::new());
    };
    discover_windows_installs_with_issues_in(Path::new(&local_appdata))
}

#[doc(hidden)]
pub fn discover_windows_installs_in(base: &Path) -> Vec<DiscordInstall> {
    discover_windows_installs_with_issues_in(base).0
}

fn discover_windows_installs_with_issues_in(base: &Path) -> (Vec<DiscordInstall>, Vec<String>) {
    let mut installs = Vec::new();
    let mut issues = Vec::new();
    for (channel, folder, executable) in channel_specs() {
        let channel_path = base.join(folder);
        match find_channel_executable(&channel_path, executable) {
            Ok(Some(executable_path)) => {
                let working_dir = executable_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| channel_path.clone());
                installs.push(DiscordInstall {
                    channel,
                    executable_path,
                    working_dir,
                });
            }
            Ok(None) => {}
            Err(error) => issues.push(format!(
                "Discord {} discovery failed ({:?}).",
                channel.display_name(),
                error.kind()
            )),
        }
    }
    (installs, issues)
}

/// Identity for one Windows installation, independent of its Squirrel app version.
/// Use lexical normalization: the old executable may already have been deleted.
pub fn windows_official_installation_id(
    path: &Path,
    channel: DiscordChannel,
) -> Option<crate::InstallationId> {
    let root = official_installation_root(path, channel)?;
    let mut normalized = PathBuf::new();
    for component in root.components() {
        if component == std::path::Component::ParentDir {
            normalized.pop();
        } else {
            normalized.push(component.as_os_str());
        }
    }
    let root = normalized;
    let key = root
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let key = key
        .strip_prefix(r"\\?\unc\")
        .map(|suffix| format!(r"\\{suffix}"))
        .unwrap_or_else(|| key.strip_prefix(r"\\?\").unwrap_or(&key).to_string());
    Some(crate::provider::installation_id_for_key(
        &crate::ProviderId::official_discord(),
        &format!("windows-installation\0{}\0{key}", channel.as_str()),
    ))
}

/// Old IDs hashed canonical paths, which on Windows include the extended-path
/// prefix. Reconstruct both spellings even after an old version was removed.
pub fn windows_legacy_installation_id_matches(path: &Path, id: &crate::InstallationId) -> bool {
    legacy_path_spellings(path).iter().any(|key| {
        crate::provider::installation_id_for_key(&crate::ProviderId::official_discord(), key) == *id
    })
}

fn legacy_path_spellings(path: &Path) -> Vec<String> {
    let resolved = crate::provider::canonicalize_existing_ancestor(path);
    let mut spellings = Vec::new();
    for path in [path, resolved.as_path()] {
        let key = path
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        let plain = key
            .strip_prefix(r"\\?\unc\")
            .map(|suffix| format!(r"\\{suffix}"))
            .unwrap_or_else(|| key.strip_prefix(r"\\?\").unwrap_or(&key).to_string());
        let extended = if let Some(unc) = plain.strip_prefix(r"\\") {
            format!(r"\\?\unc\{unc}")
        } else {
            format!(r"\\?\{plain}")
        };
        spellings.extend([plain, extended]);
    }
    spellings
}

fn official_installation_root(path: &Path, channel: DiscordChannel) -> Option<PathBuf> {
    if !path.is_absolute()
        || !path
            .file_name()?
            .eq_ignore_ascii_case(process_name(channel))
    {
        return None;
    }
    let parent = path.parent()?;
    let name = parent.file_name()?.to_string_lossy();
    if name
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("app-"))
    {
        let version = &name[4..];
        if version
            .split('.')
            .any(|part| part.is_empty() || part.parse::<u32>().is_err())
        {
            return None;
        }
        Some(parent.parent()?.to_path_buf())
    } else {
        Some(parent.to_path_buf())
    }
}

/// Refresh only automatically discovered official installations. An I/O error
/// means unknown, not uninstalled; callers must preserve the saved record.
pub fn refresh_windows_official_installation(
    install: &crate::ClientInstallation,
) -> std::io::Result<Option<crate::ClientInstallation>> {
    let Some((path, channel)) = automatic_official_target(install) else {
        return Ok(Some(crate::refresh_installation_validation(
            install.clone(),
        )));
    };
    let Some(root) = official_installation_root(path, channel) else {
        return Ok(Some(crate::refresh_installation_validation(
            install.clone(),
        )));
    };
    Ok(
        find_channel_executable(&root, process_name(channel))?.map(|executable_path| {
            let working_dir = executable_path.parent().unwrap().to_path_buf();
            crate::provider::official_installation(DiscordInstall {
                channel,
                executable_path,
                working_dir,
            })
        }),
    )
}

fn automatic_official_target(
    install: &crate::ClientInstallation,
) -> Option<(&Path, DiscordChannel)> {
    if install.provider_id != crate::ProviderId::official_discord()
        || install.source != crate::DiscoverySource::StandardPath
    {
        return None;
    }
    let crate::LaunchTarget::Executable { path, .. } = &install.launch_target else {
        return None;
    };
    let channel = crate::parse_discord_channel(Some(&install.variant_id.as_ref()?.0)).ok()??;
    Some((path, channel))
}

pub(crate) fn is_running(channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
    let stdout = tasklist_output()?;
    Ok(process_names_for(channel)
        .iter()
        .any(|name| stdout.contains(&format!("\"{}\"", name.to_ascii_lowercase()))))
}

pub(crate) fn terminate(channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
    let names = process_names_for(channel);
    crate::processes::terminate_matching_process_trees(|process| {
        names
            .iter()
            .any(|name| process.name().eq_ignore_ascii_case(name))
    })
}

pub(crate) fn spawn(install: &DiscordInstall, mode: DiscordLaunchMode) -> Result<u32, LaunchError> {
    spawn_path(&install.executable_path, &install.working_dir, mode)
}

pub(crate) fn is_vesktop_running() -> Result<bool, LaunchError> {
    Ok(tasklist_output()?.contains("\"vesktop.exe\""))
}

fn tasklist_output() -> Result<String, LaunchError> {
    let output = no_window_cmd("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .map_err(|source| LaunchError::ProcessInspection {
            operation: "tasklist",
            source,
        })?;
    if !output.status.success() {
        return Err(LaunchError::ProcessInspection {
            operation: "tasklist",
            source: std::io::Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_ascii_lowercase())
}

pub(crate) fn spawn_vesktop(
    install: &VesktopInstall,
    mode: DiscordLaunchMode,
) -> Result<u32, LaunchError> {
    spawn_path(&install.executable_path, &install.working_dir, mode)
}

fn spawn_path(
    executable_path: &Path,
    working_dir: &Path,
    mode: DiscordLaunchMode,
) -> Result<u32, LaunchError> {
    let mut command = Command::new(executable_path);
    command
        .current_dir(working_dir)
        .args(build_launch_args(mode))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
        .spawn()
        .map(|child| child.id())
        .map_err(|source| LaunchError::SpawnFailed {
            path: executable_path.to_path_buf(),
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

fn find_channel_executable(
    channel_path: &Path,
    executable: &str,
) -> std::io::Result<Option<PathBuf>> {
    let entries = match std::fs::read_dir(channel_path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut app_dirs = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with("app-")
        {
            app_dirs.push(entry);
        }
    }
    app_dirs.sort_by(|left, right| {
        parse_app_version(&right.file_name())
            .cmp(&parse_app_version(&left.file_name()))
            .then_with(|| right.file_name().cmp(&left.file_name()))
    });

    for path in app_dirs
        .into_iter()
        .map(|entry| entry.path().join(executable))
        .chain(std::iter::once(channel_path.join(executable)))
    {
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => return Ok(Some(path)),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_ids_survive_version_updates_but_distinguish_roots_and_channels() {
        let id =
            |path, channel| windows_official_installation_id(Path::new(path), channel).unwrap();
        let stable = id(
            r"C:\Users\Test\Discord\app-1.0.9256\Discord.exe",
            DiscordChannel::Stable,
        );
        assert_eq!(
            stable,
            id(
                r"c:\users\test\discord\app-1.0.9264\discord.exe",
                DiscordChannel::Stable
            )
        );
        assert_eq!(
            stable,
            id(
                r"\\?\C:\Users\Test\Discord\Discord.exe",
                DiscordChannel::Stable
            )
        );
        assert_ne!(
            stable,
            id(
                r"D:\Discord\app-1.0.9264\Discord.exe",
                DiscordChannel::Stable
            )
        );
        assert_eq!(
            id(
                r"\\server\share\Discord\app-1.0.1\Discord.exe",
                DiscordChannel::Stable
            ),
            id(
                r"\\?\UNC\server\share\Discord\app-1.0.2\Discord.exe",
                DiscordChannel::Stable
            ),
        );
        assert_ne!(
            stable,
            id(
                r"C:\Users\Test\Discord\app-1.0.9264\DiscordPTB.exe",
                DiscordChannel::Ptb
            )
        );
    }

    #[test]
    fn refresh_updates_only_automatic_installations_and_preserves_io_errors() {
        let root = std::env::temp_dir().join(format!(
            "dqh-refresh-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old = root.join("app-1.0.9999/Discord.exe");
        let new = root.join("app-1.0.10000/Discord.exe");
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::write(&new, []).unwrap();
        let legacy_id = crate::custom_executable_installation(
            &crate::ProviderId::official_discord(),
            new.clone(),
        )
        .unwrap()
        .id;
        let mut install = crate::provider::official_installation(DiscordInstall {
            channel: DiscordChannel::Stable,
            working_dir: old.parent().unwrap().into(),
            executable_path: old.clone(),
        });
        let current = refresh_windows_official_installation(&install)
            .unwrap()
            .unwrap();
        assert_eq!(current.id, install.id);
        assert!(
            matches!(current.launch_target, crate::LaunchTarget::Executable { path, .. } if path == new)
        );
        install.source = crate::DiscoverySource::User;
        let manual = refresh_windows_official_installation(&install)
            .unwrap()
            .unwrap();
        assert_eq!(manual.launch_target, install.launch_target);
        assert_eq!(manual.validation, crate::ValidationState::Missing);
        std::fs::remove_file(&new).unwrap();
        assert!(windows_legacy_installation_id_matches(&new, &legacy_id));
        install.source = crate::DiscoverySource::StandardPath;
        assert!(refresh_windows_official_installation(&install)
            .unwrap()
            .is_none());
        // A path that cannot be scanned is an error, never evidence of uninstall.
        let blocked = root.join("blocked");
        std::fs::write(&blocked, []).unwrap();
        install.launch_target = crate::LaunchTarget::Executable {
            path: blocked.join("Discord.exe"),
            working_dir: blocked,
            prefix_args: Vec::new(),
        };
        assert!(refresh_windows_official_installation(&install).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

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

    #[test]
    fn discovery_keeps_readable_channels_when_another_channel_fails() {
        let root = std::env::temp_dir().join(format!(
            "dqh-partial-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("Discord")).unwrap();
        std::fs::write(root.join("Discord/Discord.exe"), []).unwrap();
        std::fs::write(root.join("DiscordPTB"), []).unwrap();
        let (installs, issues) = discover_windows_installs_with_issues_in(&root);
        assert_eq!(installs.len(), 1);
        assert_eq!(installs[0].channel, DiscordChannel::Stable);
        assert_eq!(issues.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}

use crate::launcher::PlatformBackend;
use crate::{
    CdpProbe, CdpProbeStatus, DiscordChannel, DiscordInstall, DiscordLaunchMode, LaunchError,
    RestoreFailure, RestoreResult, RunningCdpSession, StdCdpProbe, SystemPlatform,
};
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessSnapshot {
    pub name: OsString,
    pub executable_path: Option<PathBuf>,
    pub command_line: Vec<OsString>,
}

pub fn list_running_discord_cdp_sessions() -> Result<Vec<RunningCdpSession>, LaunchError> {
    let installs = SystemPlatform.find_installs()?;
    let snapshots = process_snapshots();
    Ok(sessions_from_processes(
        &snapshots,
        &installs,
        &StdCdpProbe::default(),
    ))
}

pub fn list_running_desktop_cdp_sessions() -> Result<Vec<crate::DesktopCdpSession>, LaunchError> {
    let official_installs = SystemPlatform.find_installs()?;
    let (installations, _) = crate::discover_client_installations();
    let snapshots = process_snapshots();
    let probe = StdCdpProbe::default();
    let mut sessions = Vec::new();
    let mut seen = HashSet::new();
    let mut port_readiness = HashMap::new();
    for process in &snapshots {
        let provider_id = if crate::is_vesktop_process_name(&process.name.to_string_lossy()) {
            crate::ProviderId::vesktop()
        } else if classify_known_discord_process(process, &official_installs).is_some() {
            crate::ProviderId::official_discord()
        } else {
            continue;
        };
        let variant_id = classify_known_discord_process(process, &official_installs)
            .map(|channel| crate::VariantId(channel.as_str().to_string()));
        let matching_install = process
            .executable_path
            .as_deref()
            .and_then(|path| {
                installations.iter().find(|install| {
                    install.provider_id == provider_id
                        && installation_executable_path(install).is_some_and(|candidate| {
                            paths_refer_to_same_executable(path, candidate)
                        })
                })
            })
            .or_else(|| {
                let flatpak_path = process
                    .executable_path
                    .as_deref()
                    .is_some_and(|path| path.starts_with("/app"));
                flatpak_path
                    .then(|| {
                        installations.iter().find(|install| {
                            install.provider_id == provider_id
                                && matches!(
                                    &install.launch_target,
                                    crate::LaunchTarget::Flatpak { .. }
                                )
                        })
                    })
                    .flatten()
            });
        for port in process
            .command_line
            .iter()
            .filter_map(|argument| parse_cdp_port(argument))
        {
            let ready = *port_readiness.entry(port).or_insert_with(|| {
                matches!(probe.probe(port), CdpProbeStatus::DiscordReady { .. })
            });
            if !ready {
                continue;
            }
            let key = (
                provider_id.clone(),
                matching_install.map(|install| install.id.clone()),
                port,
            );
            if !seen.insert(key) {
                continue;
            }
            sessions.push(crate::DesktopCdpSession {
                provider_id: provider_id.clone(),
                installation_id: matching_install.map(|install| install.id.clone()),
                variant_id: matching_install
                    .and_then(|install| install.variant_id.clone())
                    .or_else(|| variant_id.clone()),
                port,
                ownership: crate::SessionOwnership::ExternalAttached,
                executable_path: process.executable_path.clone(),
            });
        }
    }
    sessions.sort_by(|left, right| {
        left.provider_id
            .as_str()
            .cmp(right.provider_id.as_str())
            .then(left.port.cmp(&right.port))
    });
    Ok(sessions)
}

pub fn restore_desktop_client_to_normal(
    installation: &crate::ClientInstallation,
    port: u16,
) -> Result<(), LaunchError> {
    if let crate::LaunchTarget::Flatpak { app_id, command } = &installation.launch_target {
        if crate::launcher::flatpak_is_running(app_id, command.as_deref())? {
            crate::launcher::flatpak_kill(app_id, command.as_deref())?;
        }
        let started = Instant::now();
        while started.elapsed() < SHUTDOWN_TIMEOUT {
            if !crate::launcher::flatpak_is_running(app_id, command.as_deref())? {
                break;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        if crate::launcher::flatpak_is_running(app_id, command.as_deref())? {
            return Err(LaunchError::ShutdownTimeout {
                timeout: SHUTDOWN_TIMEOUT,
            });
        }
        if matches!(
            StdCdpProbe::default().probe(port),
            CdpProbeStatus::DiscordReady { .. }
        ) {
            return Err(LaunchError::ProcessTermination {
                process: installation.display_name.clone(),
                details: format!(
                    "CDP endpoint on port {port} remained active after Flatpak shutdown"
                ),
            });
        }
        crate::launcher::flatpak_spawn(app_id, command.as_deref(), None)?;
        return Ok(());
    }
    let executable = installation_executable_path(installation).ok_or_else(|| {
        LaunchError::InvalidInstallation {
            details: "This launch target cannot yet be restored by executable path.".into(),
        }
    })?;
    terminate_installation_process_tree(executable)?;
    let mut system = System::new();
    let started = Instant::now();
    while started.elapsed() < SHUTDOWN_TIMEOUT {
        refresh_processes(&mut system);
        if !is_installation_running_in_system(&system, executable) {
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    refresh_processes(&mut system);
    if is_installation_running_in_system(&system, executable) {
        return Err(LaunchError::ShutdownTimeout {
            timeout: SHUTDOWN_TIMEOUT,
        });
    }
    if matches!(
        StdCdpProbe::default().probe(port),
        CdpProbeStatus::DiscordReady { .. }
    ) {
        return Err(LaunchError::ProcessTermination {
            process: installation.display_name.clone(),
            details: format!("CDP endpoint on port {port} remained active after shutdown"),
        });
    }
    if let Some(official) = crate::installation_as_official(installation) {
        SystemPlatform.spawn(&official, DiscordLaunchMode::Normal)?;
    } else if let Some(vesktop) = crate::installation_as_vesktop(installation) {
        crate::vesktop::spawn_vesktop(&vesktop, DiscordLaunchMode::Normal)?;
    } else {
        return Err(LaunchError::InvalidInstallation {
            details: "The provider does not expose a normal-mode launch target.".into(),
        });
    }
    Ok(())
}

pub fn restore_all_discord_to_normal() -> Result<RestoreResult, LaunchError> {
    let sessions = list_running_discord_cdp_sessions()?;
    Ok(restore_sessions_with_backends(
        &sessions,
        &SystemPlatform,
        &StdCdpProbe::default(),
    ))
}

pub(crate) fn sessions_from_processes<C: CdpProbe>(
    processes: &[ProcessSnapshot],
    installs: &[DiscordInstall],
    probe: &C,
) -> Vec<RunningCdpSession> {
    let mut candidates = HashSet::new();
    for process in processes {
        let Some(channel) = classify_known_discord_process(process, installs) else {
            continue;
        };
        for argument in &process.command_line {
            if let Some(port) = parse_cdp_port(argument) {
                candidates.insert(RunningCdpSession { channel, port });
            }
        }
    }

    let mut sessions: Vec<_> = candidates
        .into_iter()
        .filter(|session| {
            matches!(
                probe.probe(session.port),
                CdpProbeStatus::DiscordReady { .. }
            )
        })
        .collect();
    sessions.sort_by_key(|session| (channel_order(session.channel), session.port));
    sessions
}

pub(crate) fn restore_sessions_with_backends<P: PlatformBackend, C: CdpProbe>(
    sessions: &[RunningCdpSession],
    platform: &P,
    probe: &C,
) -> RestoreResult {
    let mut result = RestoreResult::default();
    let mut channels = Vec::new();
    for channel in DiscordChannel::ALL {
        if sessions.iter().any(|session| session.channel == channel) {
            channels.push(channel);
        }
    }
    if channels.is_empty() {
        return result;
    }

    let installs = match platform.find_installs() {
        Ok(installs) => installs,
        Err(error) => {
            for channel in channels {
                result.failures.push(RestoreFailure {
                    channel,
                    error: error.to_string(),
                });
            }
            return result;
        }
    };

    for channel in channels {
        let Some(install) = installs
            .iter()
            .find(|install| install.channel == channel)
            .cloned()
        else {
            result.failures.push(RestoreFailure {
                channel,
                error: LaunchError::InstallNotFound {
                    channel: Some(channel),
                }
                .to_string(),
            });
            continue;
        };

        let restored = restore_channel(channel, &install, sessions, platform, probe);
        match restored {
            Ok(()) => result.restored.push(channel),
            Err(error) => result.failures.push(RestoreFailure { channel, error }),
        }
    }
    result
}

fn restore_channel<P: PlatformBackend, C: CdpProbe>(
    channel: DiscordChannel,
    install: &DiscordInstall,
    sessions: &[RunningCdpSession],
    platform: &P,
    probe: &C,
) -> Result<(), String> {
    platform
        .terminate(Some(channel))
        .map_err(|error| error.to_string())?;
    wait_for_running_state(platform, channel, false, SHUTDOWN_TIMEOUT)?;

    for port in sessions
        .iter()
        .filter(|session| session.channel == channel)
        .map(|session| session.port)
    {
        if matches!(probe.probe(port), CdpProbeStatus::DiscordReady { .. }) {
            return Err(format!(
                "Discord {} CDP endpoint on port {port} remained active after shutdown.",
                channel.display_name()
            ));
        }
    }

    platform
        .spawn(install, DiscordLaunchMode::Normal)
        .map_err(|error| error.to_string())?;
    wait_for_running_state(platform, channel, true, STARTUP_TIMEOUT).map_err(|error| {
        format!(
            "Discord {} was relaunched in normal mode, but startup verification failed: {error}",
            channel.display_name()
        )
    })?;

    for port in sessions
        .iter()
        .filter(|session| session.channel == channel)
        .map(|session| session.port)
    {
        if matches!(probe.probe(port), CdpProbeStatus::DiscordReady { .. }) {
            return Err(format!(
                "Discord {} restarted but CDP is still active on port {port}.",
                channel.display_name()
            ));
        }
    }
    Ok(())
}

fn wait_for_running_state<P: PlatformBackend>(
    platform: &P,
    channel: DiscordChannel,
    expected: bool,
    timeout: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        let running = platform
            .is_running(Some(channel))
            .map_err(|error| error.to_string())?;
        if running == expected {
            return Ok(());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    Err(if expected {
        format!(
            "Discord {} did not start within {} seconds.",
            channel.display_name(),
            timeout.as_secs()
        )
    } else {
        format!(
            "Discord {} did not exit within {} seconds.",
            channel.display_name(),
            timeout.as_secs()
        )
    })
}

fn process_snapshots() -> Vec<ProcessSnapshot> {
    let mut system = System::new();
    refresh_processes(&mut system);
    system
        .processes()
        .values()
        .map(|process| ProcessSnapshot {
            name: process.name().to_os_string(),
            executable_path: process.exe().map(Path::to_path_buf),
            command_line: process.cmd().to_vec(),
        })
        .collect()
}

fn refresh_processes(system: &mut System) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_exe(UpdateKind::Always)
            .without_tasks(),
    );
}

fn is_installation_running_in_system(system: &System, executable_path: &Path) -> bool {
    system.processes().values().any(|process| {
        process
            .exe()
            .is_some_and(|path| paths_refer_to_same_executable(path, executable_path))
    })
}

pub fn running_vesktop_installs() -> Vec<crate::VesktopInstall> {
    let mut seen = HashSet::new();
    process_snapshots()
        .into_iter()
        .filter(|process| crate::is_vesktop_process_name(&process.name.to_string_lossy()))
        .filter_map(|process| process.executable_path)
        .filter_map(crate::vesktop::vesktop_install_from_executable)
        .filter(|install| seen.insert(install.executable_path.clone()))
        .collect()
}

pub fn is_installation_running(executable_path: &Path) -> bool {
    process_snapshots().iter().any(|process| {
        process
            .executable_path
            .as_deref()
            .is_some_and(|path| paths_refer_to_same_executable(path, executable_path))
    })
}

pub fn is_client_installation_running(
    installation: &crate::ClientInstallation,
) -> Result<bool, LaunchError> {
    match &installation.launch_target {
        crate::LaunchTarget::Executable { path, .. }
        | crate::LaunchTarget::MacBundle {
            executable_path: path,
            ..
        } => Ok(is_installation_running(path)),
        crate::LaunchTarget::Flatpak { app_id, command } => {
            crate::launcher::flatpak_is_running(app_id, command.as_deref())
        }
    }
}

/// Terminates only the process tree whose root executable matches the exact
/// selected installation. Every target is revalidated by PID, start time, and
/// executable immediately before termination. `sysinfo` does not expose stable
/// cross-platform process handles, so this narrows PID-reuse risk without
/// claiming an OS-level race-free guarantee.
pub fn terminate_installation_process_tree(executable_path: &Path) -> Result<(), LaunchError> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .without_tasks(),
    );
    let roots: HashSet<_> = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            process
                .exe()
                .is_some_and(|path| paths_refer_to_same_executable(path, executable_path))
                .then_some((*pid, process.start_time()))
        })
        .collect();
    if roots.is_empty() {
        return Ok(());
    }

    let mut targets: HashSet<_> = roots.iter().map(|(pid, _)| *pid).collect();
    loop {
        let previous_len = targets.len();
        for (pid, process) in system.processes() {
            if process
                .parent()
                .is_some_and(|parent| targets.contains(&parent))
            {
                targets.insert(*pid);
            }
        }
        if targets.len() == previous_len {
            break;
        }
    }

    let target_identities: HashMap<_, _> = targets
        .iter()
        .filter_map(|pid| {
            system.process(*pid).map(|process| {
                (
                    *pid,
                    (process.start_time(), process.exe().map(Path::to_path_buf)),
                )
            })
        })
        .collect();

    // Descendants first. Refreshing the process table before every kill keeps
    // the identity check as close as possible to the operation itself.
    let root_pids: HashSet<_> = roots.iter().map(|(pid, _)| *pid).collect();
    let mut ordered: Vec<_> = targets.into_iter().collect();
    ordered.sort_by_key(|pid| root_pids.contains(pid));
    let mut current = System::new();
    for pid in ordered {
        let Some((expected_start, expected_executable)) = target_identities.get(&pid) else {
            continue;
        };
        refresh_processes(&mut current);
        let Some(process) = current.process(pid) else {
            continue;
        };
        if process.start_time() != *expected_start
            || process.exe().map(Path::to_path_buf) != *expected_executable
        {
            continue;
        }
        if !process.kill() {
            return Err(LaunchError::ProcessTermination {
                process: pid.to_string(),
                details: format!(
                    "the process for '{}' refused termination",
                    executable_path.display()
                ),
            });
        }
    }
    Ok(())
}

fn classify_known_discord_process(
    process: &ProcessSnapshot,
    installs: &[DiscordInstall],
) -> Option<DiscordChannel> {
    let name_channel = channel_from_process_name(&process.name)?;
    let executable_path = process.executable_path.as_deref()?;
    installs.iter().find_map(|install| {
        (install.channel == name_channel
            && paths_refer_to_same_executable(executable_path, &install.executable_path))
        .then_some(name_channel)
    })
}

fn channel_from_process_name(name: &OsStr) -> Option<DiscordChannel> {
    match name.to_string_lossy().to_ascii_lowercase().as_str() {
        "discord" | "discord.exe" => Some(DiscordChannel::Stable),
        "discordptb" | "discordptb.exe" | "discord-ptb" | "discord ptb" => {
            Some(DiscordChannel::Ptb)
        }
        "discordcanary" | "discordcanary.exe" | "discord-canary" | "discord canary" => {
            Some(DiscordChannel::Canary)
        }
        _ => None,
    }
}

fn paths_refer_to_same_executable(left: &Path, right: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let paths_match = |left: &Path, right: &Path| {
            left.to_string_lossy()
                .eq_ignore_ascii_case(&right.to_string_lossy())
        };
        match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
            (Ok(left), Ok(right)) => paths_match(&left, &right),
            _ => paths_match(left, right),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        left == right
    }
}

fn installation_executable_path(install: &crate::ClientInstallation) -> Option<&Path> {
    match &install.launch_target {
        crate::LaunchTarget::Executable { path, .. } => Some(path),
        crate::LaunchTarget::MacBundle {
            executable_path, ..
        } => Some(executable_path),
        crate::LaunchTarget::Flatpak { .. } => None,
    }
}

pub fn inspect_cdp_port_owner(port: u16) -> crate::CdpPortOwner {
    classify_cdp_port_owner(&process_snapshots(), port)
}

pub(crate) fn cdp_port_matches_installation(
    port: u16,
    installation: &crate::ClientInstallation,
) -> bool {
    cdp_port_matches_installation_in(&process_snapshots(), port, installation)
}

fn cdp_port_matches_installation_in(
    processes: &[ProcessSnapshot],
    port: u16,
    installation: &crate::ClientInstallation,
) -> bool {
    processes.iter().any(|process| {
        let uses_port = process
            .command_line
            .iter()
            .any(|argument| parse_cdp_port(argument) == Some(port));
        if !uses_port {
            return false;
        }
        match &installation.launch_target {
            crate::LaunchTarget::Executable { path, .. } => process
                .executable_path
                .as_deref()
                .is_some_and(|running| paths_refer_to_same_executable(running, path)),
            crate::LaunchTarget::MacBundle {
                executable_path, ..
            } => process
                .executable_path
                .as_deref()
                .is_some_and(|running| paths_refer_to_same_executable(running, executable_path)),
            crate::LaunchTarget::Flatpak { .. } => {
                installation.provider_id == crate::ProviderId::vesktop()
                    && process
                        .executable_path
                        .as_deref()
                        .is_some_and(|path| path.starts_with("/app"))
                    && crate::is_vesktop_process_name(&process.name.to_string_lossy())
            }
        }
    })
}

pub(crate) fn classify_cdp_port_owner(
    processes: &[ProcessSnapshot],
    port: u16,
) -> crate::CdpPortOwner {
    let mut official = false;
    let mut vesktop = false;
    let mut other = false;
    for process in processes {
        if !process
            .command_line
            .iter()
            .any(|argument| parse_cdp_port(argument) == Some(port))
        {
            continue;
        }
        let name = process.name.to_string_lossy();
        if crate::is_vesktop_process_name(&name) {
            vesktop = true;
        } else if channel_from_process_name(&process.name).is_some() {
            official = true;
        } else {
            other = true;
        }
    }
    match (official, vesktop, other) {
        (false, false, false) => crate::CdpPortOwner::None,
        (true, false, _) => crate::CdpPortOwner::Official,
        (false, true, _) => crate::CdpPortOwner::Vesktop,
        _ => crate::CdpPortOwner::Other,
    }
}

fn parse_cdp_port(argument: &OsStr) -> Option<u16> {
    let value = argument.to_str()?;
    value
        .strip_prefix("--remote-debugging-port=")?
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
}

const fn channel_order(channel: DiscordChannel) -> u8 {
    match channel {
        DiscordChannel::Stable => 0,
        DiscordChannel::Ptb => 1,
        DiscordChannel::Canary => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct Probe(HashSet<u16>);

    impl CdpProbe for Probe {
        fn probe(&self, port: u16) -> CdpProbeStatus {
            if self.0.contains(&port) {
                CdpProbeStatus::DiscordReady { target_title: None }
            } else {
                CdpProbeStatus::Unreachable
            }
        }
    }

    fn install(channel: DiscordChannel, path: &str) -> DiscordInstall {
        DiscordInstall {
            channel,
            executable_path: PathBuf::from(path),
            working_dir: PathBuf::from("C:\\Discord"),
        }
    }

    #[test]
    fn discovers_only_confirmed_known_discord_sessions_and_deduplicates_children() {
        let installs = vec![install(DiscordChannel::Stable, "C:\\Discord\\Discord.exe")];
        let discord = ProcessSnapshot {
            name: "Discord.exe".into(),
            executable_path: Some("C:\\Discord\\Discord.exe".into()),
            command_line: vec!["Discord.exe".into(), "--remote-debugging-port=9223".into()],
        };
        let unrelated = ProcessSnapshot {
            name: "chrome.exe".into(),
            executable_path: Some("C:\\Chrome\\chrome.exe".into()),
            command_line: vec!["chrome.exe".into(), "--remote-debugging-port=9223".into()],
        };
        let invalid = ProcessSnapshot {
            command_line: vec!["Discord.exe".into(), "--remote-debugging-port=0".into()],
            ..discord.clone()
        };
        let sessions = sessions_from_processes(
            &[discord.clone(), discord, unrelated, invalid],
            &installs,
            &Probe(HashSet::from([9223])),
        );
        assert_eq!(
            sessions,
            vec![RunningCdpSession {
                channel: DiscordChannel::Stable,
                port: 9223
            }]
        );
    }

    #[test]
    fn vesktop_is_not_classified_as_a_discord_release_channel() {
        assert_eq!(channel_from_process_name(OsStr::new("vesktop.exe")), None);
        assert_eq!(channel_from_process_name(OsStr::new("Vesktop")), None);
        let installs = vec![install(DiscordChannel::Stable, "C:\\Discord\\Discord.exe")];
        let vesktop = ProcessSnapshot {
            name: "vesktop.exe".into(),
            executable_path: Some("C:\\Users\\user\\AppData\\Local\\vesktop\\vesktop.exe".into()),
            command_line: vec!["vesktop.exe".into(), "--remote-debugging-port=9223".into()],
        };
        assert!(classify_known_discord_process(&vesktop, &installs).is_none());
        assert!(
            sessions_from_processes(&[vesktop], &installs, &Probe(HashSet::from([9223])))
                .is_empty()
        );
    }

    #[test]
    fn classifies_which_desktop_client_owns_a_cdp_port() {
        let official = ProcessSnapshot {
            name: "Discord.exe".into(),
            executable_path: Some("C:\\Discord\\Discord.exe".into()),
            command_line: vec!["Discord.exe".into(), "--remote-debugging-port=9223".into()],
        };
        let vesktop = ProcessSnapshot {
            name: "vesktop.exe".into(),
            executable_path: Some("C:\\vesktop\\vesktop.exe".into()),
            command_line: vec!["vesktop.exe".into(), "--remote-debugging-port=9223".into()],
        };
        assert_eq!(
            classify_cdp_port_owner(std::slice::from_ref(&official), 9223),
            crate::CdpPortOwner::Official
        );
        assert_eq!(
            classify_cdp_port_owner(std::slice::from_ref(&vesktop), 9223),
            crate::CdpPortOwner::Vesktop
        );
        assert_eq!(
            classify_cdp_port_owner(&[official, vesktop], 9223),
            crate::CdpPortOwner::Other
        );
        assert_eq!(
            classify_cdp_port_owner(&[], 9223),
            crate::CdpPortOwner::None
        );
    }

    #[test]
    fn explicit_installation_owner_requires_the_exact_executable() {
        let selected = crate::custom_executable_installation(
            &crate::ProviderId::vesktop(),
            PathBuf::from("C:\\Portable A\\vesktop.exe"),
        )
        .expect("named Vesktop executable is a valid locator");
        let processes = vec![ProcessSnapshot {
            name: OsString::from("vesktop.exe"),
            executable_path: Some(PathBuf::from("C:\\Portable B\\vesktop.exe")),
            command_line: vec![OsString::from("--remote-debugging-port=9223")],
        }];

        assert!(!cdp_port_matches_installation_in(
            &processes, 9223, &selected
        ));
    }

    #[test]
    fn recognizes_space_separated_macos_channel_names() {
        assert_eq!(
            channel_from_process_name(OsStr::new("Discord PTB")),
            Some(DiscordChannel::Ptb)
        );
        assert_eq!(
            channel_from_process_name(OsStr::new("Discord Canary")),
            Some(DiscordChannel::Canary)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn resolves_equivalent_windows_path_representations() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "discord-cdp-path-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("Discord.exe");
        std::fs::File::create(&executable).unwrap();
        let verbatim = PathBuf::from(format!(r"\\?\{}", executable.display()));

        assert!(paths_refer_to_same_executable(&executable, &verbatim));

        std::fs::remove_file(executable).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    struct RestorePlatform {
        installs: Vec<DiscordInstall>,
        running: Mutex<HashMap<DiscordChannel, bool>>,
        spawned: Mutex<Vec<(DiscordChannel, DiscordLaunchMode)>>,
    }

    impl PlatformBackend for RestorePlatform {
        fn find_installs(&self) -> Result<Vec<DiscordInstall>, LaunchError> {
            Ok(self.installs.clone())
        }
        fn is_running(&self, channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
            Ok(channel
                .and_then(|channel| self.running.lock().unwrap().get(&channel).copied())
                .unwrap_or(false))
        }
        fn terminate(&self, channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
            if let Some(channel) = channel {
                self.running.lock().unwrap().insert(channel, false);
            }
            Ok(())
        }
        fn spawn(
            &self,
            install: &DiscordInstall,
            mode: DiscordLaunchMode,
        ) -> Result<u32, LaunchError> {
            self.spawned.lock().unwrap().push((install.channel, mode));
            self.running.lock().unwrap().insert(install.channel, true);
            Ok(1)
        }
    }

    #[test]
    fn restores_each_channel_once_in_normal_mode() {
        let installs = vec![
            install(DiscordChannel::Stable, "C:\\Discord\\Discord.exe"),
            install(DiscordChannel::Ptb, "C:\\DiscordPTB\\DiscordPTB.exe"),
        ];
        let platform = RestorePlatform {
            installs,
            running: Mutex::new(HashMap::from([
                (DiscordChannel::Stable, true),
                (DiscordChannel::Ptb, true),
            ])),
            spawned: Mutex::new(Vec::new()),
        };
        let sessions = vec![
            RunningCdpSession {
                channel: DiscordChannel::Stable,
                port: 9223,
            },
            RunningCdpSession {
                channel: DiscordChannel::Stable,
                port: 9224,
            },
            RunningCdpSession {
                channel: DiscordChannel::Ptb,
                port: 9333,
            },
        ];
        let result = restore_sessions_with_backends(&sessions, &platform, &Probe(HashSet::new()));
        assert!(result.failures.is_empty());
        assert_eq!(
            *platform.spawned.lock().unwrap(),
            vec![
                (DiscordChannel::Stable, DiscordLaunchMode::Normal),
                (DiscordChannel::Ptb, DiscordLaunchMode::Normal),
            ]
        );
    }

    #[test]
    fn restore_is_a_noop_without_sessions() {
        let platform = RestorePlatform {
            installs: Vec::new(),
            running: Mutex::new(HashMap::new()),
            spawned: Mutex::new(Vec::new()),
        };
        let result = restore_sessions_with_backends(&[], &platform, &Probe(HashSet::new()));
        assert_eq!(result, RestoreResult::default());
        assert!(platform.spawned.lock().unwrap().is_empty());
    }

    #[test]
    fn missing_channel_install_does_not_prevent_other_channels_from_restoring() {
        let platform = RestorePlatform {
            installs: vec![install(DiscordChannel::Stable, "C:\\Discord\\Discord.exe")],
            running: Mutex::new(HashMap::from([
                (DiscordChannel::Stable, true),
                (DiscordChannel::Ptb, true),
            ])),
            spawned: Mutex::new(Vec::new()),
        };
        let sessions = vec![
            RunningCdpSession {
                channel: DiscordChannel::Stable,
                port: 9223,
            },
            RunningCdpSession {
                channel: DiscordChannel::Ptb,
                port: 9333,
            },
        ];
        let result = restore_sessions_with_backends(&sessions, &platform, &Probe(HashSet::new()));
        assert_eq!(result.restored, vec![DiscordChannel::Stable]);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].channel, DiscordChannel::Ptb);
    }

    #[test]
    fn reports_a_cdp_endpoint_that_remains_active_after_shutdown() {
        let platform = RestorePlatform {
            installs: vec![install(DiscordChannel::Stable, "C:\\Discord\\Discord.exe")],
            running: Mutex::new(HashMap::from([(DiscordChannel::Stable, true)])),
            spawned: Mutex::new(Vec::new()),
        };
        let sessions = vec![RunningCdpSession {
            channel: DiscordChannel::Stable,
            port: 9223,
        }];

        let result =
            restore_sessions_with_backends(&sessions, &platform, &Probe(HashSet::from([9223])));

        assert!(result.restored.is_empty());
        assert_eq!(result.failures.len(), 1);
        assert!(result.failures[0]
            .error
            .contains("CDP endpoint on port 9223 remained active after shutdown"));
        assert!(platform.spawned.lock().unwrap().is_empty());
    }

    struct VerificationFailurePlatform {
        install: DiscordInstall,
        spawned: Mutex<bool>,
    }

    impl PlatformBackend for VerificationFailurePlatform {
        fn find_installs(&self) -> Result<Vec<DiscordInstall>, LaunchError> {
            Ok(vec![self.install.clone()])
        }

        fn is_running(&self, _channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
            if *self.spawned.lock().unwrap() {
                Err(LaunchError::UnsupportedPlatform)
            } else {
                Ok(false)
            }
        }

        fn terminate(&self, _channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
            Ok(())
        }

        fn spawn(
            &self,
            _install: &DiscordInstall,
            _mode: DiscordLaunchMode,
        ) -> Result<u32, LaunchError> {
            *self.spawned.lock().unwrap() = true;
            Ok(1)
        }
    }

    #[test]
    fn distinguishes_post_relaunch_verification_failure() {
        let platform = VerificationFailurePlatform {
            install: install(DiscordChannel::Stable, "C:\\Discord\\Discord.exe"),
            spawned: Mutex::new(false),
        };
        let sessions = vec![RunningCdpSession {
            channel: DiscordChannel::Stable,
            port: 9223,
        }];

        let result = restore_sessions_with_backends(&sessions, &platform, &Probe(HashSet::new()));

        assert_eq!(result.failures.len(), 1);
        assert!(result.failures[0]
            .error
            .contains("was relaunched in normal mode, but startup verification failed"));
    }

    struct FailingDiscoveryPlatform;

    impl PlatformBackend for FailingDiscoveryPlatform {
        fn find_installs(&self) -> Result<Vec<DiscordInstall>, LaunchError> {
            Err(LaunchError::UnsupportedPlatform)
        }

        fn is_running(&self, _channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
            unreachable!()
        }

        fn terminate(&self, _channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
            unreachable!()
        }

        fn spawn(
            &self,
            _install: &DiscordInstall,
            _mode: DiscordLaunchMode,
        ) -> Result<u32, LaunchError> {
            unreachable!()
        }
    }

    #[test]
    fn reports_discovery_failure_for_every_affected_channel() {
        let sessions = vec![
            RunningCdpSession {
                channel: DiscordChannel::Stable,
                port: 9223,
            },
            RunningCdpSession {
                channel: DiscordChannel::Ptb,
                port: 9333,
            },
        ];

        let result = restore_sessions_with_backends(
            &sessions,
            &FailingDiscoveryPlatform,
            &Probe(HashSet::new()),
        );

        assert!(result.restored.is_empty());
        assert_eq!(result.failures.len(), 2);
        assert_eq!(result.failures[0].channel, DiscordChannel::Stable);
        assert_eq!(result.failures[1].channel, DiscordChannel::Ptb);
        assert!(result.failures.iter().all(|failure| failure.error
            == "Discord CDP launcher is only supported on Windows, macOS, and Linux."));
    }
}

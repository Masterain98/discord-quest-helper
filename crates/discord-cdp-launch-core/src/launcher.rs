use crate::platform::SystemPlatform;
use crate::vesktop::{
    cdp_ready_matches_preference, find_vesktop_install, is_vesktop_running, spawn_vesktop,
    vesktop_launch_plan, VesktopInstall, VesktopLaunchPlan,
};
use crate::{
    CdpProbe, CdpProbeStatus, DiscordChannel, DiscordInstall, DiscordLaunchMode, LaunchError,
    LaunchOptions, LaunchOutcome, LaunchResult, ProviderId, SessionOwnership, StdCdpProbe,
    VariantId,
};
use std::ffi::OsString;
use std::time::Instant;

pub trait PlatformBackend {
    fn find_installs(&self) -> Result<Vec<DiscordInstall>, LaunchError>;
    fn is_running(&self, channel: Option<DiscordChannel>) -> Result<bool, LaunchError>;
    fn terminate(&self, channel: Option<DiscordChannel>) -> Result<(), LaunchError>;
    fn spawn(&self, install: &DiscordInstall, mode: DiscordLaunchMode) -> Result<u32, LaunchError>;
}

pub fn find_discord_installs() -> Result<Vec<DiscordInstall>, LaunchError> {
    SystemPlatform.find_installs()
}

pub fn select_preferred_install(
    installs: &[DiscordInstall],
    channel: Option<DiscordChannel>,
) -> Result<DiscordInstall, LaunchError> {
    let selected = if let Some(channel) = channel {
        installs.iter().find(|install| install.channel == channel)
    } else {
        DiscordChannel::ALL
            .iter()
            .find_map(|channel| installs.iter().find(|install| install.channel == *channel))
    };

    selected
        .cloned()
        .ok_or(LaunchError::InstallNotFound { channel })
}

pub fn is_cdp_available(port: u16) -> bool {
    matches!(
        StdCdpProbe::default().probe(port),
        CdpProbeStatus::DiscordReady { .. }
    )
}

pub fn is_discord_running(channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
    SystemPlatform.is_running(channel)
}

pub fn terminate_discord_processes(channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
    SystemPlatform.terminate(channel)
}

pub fn launch_discord_with_cdp(options: LaunchOptions) -> Result<LaunchResult, LaunchError> {
    if is_cdp_available(options.port) {
        let owner = crate::inspect_cdp_port_owner(options.port);
        if !cdp_ready_matches_preference(options.client, owner) {
            return Err(LaunchError::CdpOwnedByOtherClient {
                port: options.port,
                owner: owner.as_str(),
            });
        }
        if options.installation.as_ref().is_some_and(|installation| {
            !crate::processes::cdp_port_matches_installation(options.port, installation)
        }) {
            return Err(LaunchError::CdpOwnedByOtherClient {
                port: options.port,
                owner: "another installation",
            });
        }
    }
    if should_launch_vesktop(&options)? {
        let selected_installation = options.installation.clone();
        if selected_installation.as_ref().is_some_and(|installation| {
            matches!(
                &installation.launch_target,
                crate::LaunchTarget::Flatpak { .. }
            )
        }) {
            return launch_flatpak_with_cdp(
                options,
                selected_installation
                    .as_ref()
                    .expect("checked selected installation"),
                &StdCdpProbe::default(),
            );
        }
        let install = selected_installation
            .as_ref()
            .and_then(crate::installation_as_vesktop)
            .or_else(find_vesktop_install)
            .ok_or(LaunchError::InstallNotFound { channel: None })?;
        return launch_vesktop_with_cdp(
            options,
            install,
            selected_installation.as_ref(),
            &StdCdpProbe::default(),
        );
    }
    launch_with_backends(options, &SystemPlatform, &StdCdpProbe::default())
}

fn launch_flatpak_with_cdp<C: CdpProbe>(
    options: LaunchOptions,
    installation: &crate::ClientInstallation,
    cdp: &C,
) -> Result<LaunchResult, LaunchError> {
    let crate::LaunchTarget::Flatpak { app_id, command } = &installation.launch_target else {
        return Err(LaunchError::InvalidInstallation {
            details: "The selected target is not a Flatpak application.".into(),
        });
    };
    if options.port == 0 {
        return Err(LaunchError::InvalidPort(options.port));
    }
    match cdp.probe(options.port) {
        CdpProbeStatus::DiscordReady { .. } => {
            return Ok(flatpak_result(
                installation,
                &options,
                LaunchOutcome::AlreadyAvailable,
                None,
                true,
            ));
        }
        CdpProbeStatus::PortOccupied => {
            return Err(LaunchError::PortOccupied { port: options.port })
        }
        CdpProbeStatus::Unreachable | CdpProbeStatus::CdpWithoutDiscordTarget => {}
    }
    if flatpak_is_running(app_id, command.as_deref())? {
        if !options.restart_existing {
            return Err(LaunchError::DesktopClientAlreadyRunning { client: "Vesktop" });
        }
        flatpak_kill(app_id, command.as_deref())?;
        let started = Instant::now();
        while started.elapsed() < options.shutdown_timeout {
            if !flatpak_is_running(app_id, command.as_deref())? {
                break;
            }
            std::thread::sleep(options.poll_interval);
        }
        if flatpak_is_running(app_id, command.as_deref())? {
            return Err(LaunchError::ShutdownTimeout {
                timeout: options.shutdown_timeout,
            });
        }
    }
    match cdp.probe(options.port) {
        CdpProbeStatus::Unreachable => {}
        CdpProbeStatus::DiscordReady { .. } => {
            return Ok(flatpak_result(
                installation,
                &options,
                LaunchOutcome::AlreadyAvailable,
                None,
                true,
            ));
        }
        CdpProbeStatus::PortOccupied => {
            return Err(LaunchError::PortOccupied { port: options.port })
        }
        CdpProbeStatus::CdpWithoutDiscordTarget => {
            return Err(LaunchError::NonDiscordCdpTarget { port: options.port })
        }
    }
    let pid = flatpak_spawn(app_id, command.as_deref(), Some(options.port))?;
    if !options.wait_for_cdp {
        return Ok(flatpak_result(
            installation,
            &options,
            LaunchOutcome::Spawned,
            Some(pid),
            false,
        ));
    }
    let started = Instant::now();
    while started.elapsed() < options.readiness_timeout {
        if matches!(cdp.probe(options.port), CdpProbeStatus::DiscordReady { .. }) {
            return Ok(flatpak_result(
                installation,
                &options,
                LaunchOutcome::Spawned,
                Some(pid),
                true,
            ));
        }
        std::thread::sleep(options.poll_interval);
    }
    Err(LaunchError::ReadinessTimeout {
        port: options.port,
        timeout: options.readiness_timeout,
    })
}

fn flatpak_result(
    installation: &crate::ClientInstallation,
    options: &LaunchOptions,
    outcome: LaunchOutcome,
    pid: Option<u32>,
    cdp_connected: bool,
) -> LaunchResult {
    LaunchResult {
        outcome,
        launched_path: std::path::PathBuf::new(),
        channel: DiscordChannel::Stable,
        port: options.port,
        pid,
        cdp_connected,
        provider_id: ProviderId::vesktop(),
        installation_id: Some(installation.id.clone()),
        variant_id: installation.variant_id.clone(),
        ownership: if outcome == LaunchOutcome::Spawned {
            SessionOwnership::Managed
        } else {
            SessionOwnership::ExternalAttached
        },
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn flatpak_is_running(app_id: &str, command: Option<&str>) -> Result<bool, LaunchError> {
    let command = command.unwrap_or("flatpak");
    let output = std::process::Command::new(command)
        .args(["ps", "--columns=application"])
        .output()
        .map_err(|source| LaunchError::ProcessInspection {
            operation: "flatpak ps",
            source,
        })?;
    Ok(output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.trim() == app_id))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn flatpak_is_running(
    _app_id: &str,
    _command: Option<&str>,
) -> Result<bool, LaunchError> {
    Err(LaunchError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
pub(crate) fn flatpak_kill(app_id: &str, command: Option<&str>) -> Result<(), LaunchError> {
    let command = command.unwrap_or("flatpak");
    let output = std::process::Command::new(command)
        .args(["kill", app_id])
        .output()
        .map_err(|source| LaunchError::ProcessInspection {
            operation: "flatpak kill",
            source,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(LaunchError::ProcessTermination {
            process: app_id.into(),
            details: String::from_utf8_lossy(&output.stderr).trim().into(),
        })
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn flatpak_kill(_app_id: &str, _command: Option<&str>) -> Result<(), LaunchError> {
    Err(LaunchError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
pub(crate) fn flatpak_spawn(
    app_id: &str,
    command: Option<&str>,
    port: Option<u16>,
) -> Result<u32, LaunchError> {
    let command = command.unwrap_or("flatpak");
    let mut process = std::process::Command::new(command);
    process.args(["run", app_id]);
    if let Some(port) = port {
        process.arg(format!("--remote-debugging-port={port}"));
    }
    process
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|child| child.id())
        .map_err(|source| LaunchError::SpawnFailed {
            path: std::path::PathBuf::from(command),
            source,
        })
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn flatpak_spawn(
    _app_id: &str,
    _command: Option<&str>,
    _port: Option<u16>,
) -> Result<u32, LaunchError> {
    Err(LaunchError::UnsupportedPlatform)
}

fn should_launch_vesktop(options: &LaunchOptions) -> Result<bool, LaunchError> {
    if let Some(installation) = &options.installation {
        return Ok(installation.provider_id == ProviderId::vesktop());
    }
    let official_running = SystemPlatform.is_running(options.channel)?;
    let vesktop_running = is_vesktop_running()?;
    let official_install_found =
        select_preferred_install(&SystemPlatform.find_installs()?, options.channel).is_ok();
    Ok(vesktop_launch_plan(
        options.channel,
        options.client,
        official_running,
        vesktop_running,
        official_install_found,
        find_vesktop_install().is_some(),
    ) == VesktopLaunchPlan::LaunchVesktop)
}

fn launch_vesktop_with_cdp<C: CdpProbe>(
    options: LaunchOptions,
    install: VesktopInstall,
    selected_installation: Option<&crate::ClientInstallation>,
    cdp: &C,
) -> Result<LaunchResult, LaunchError> {
    if options.port == 0 {
        return Err(LaunchError::InvalidPort(options.port));
    }

    match cdp.probe(options.port) {
        CdpProbeStatus::DiscordReady { .. } => {
            return Ok(vesktop_result(
                &install,
                &options,
                selected_installation,
                LaunchOutcome::AlreadyAvailable,
                None,
                true,
            ));
        }
        CdpProbeStatus::PortOccupied => {
            return Err(LaunchError::PortOccupied { port: options.port });
        }
        CdpProbeStatus::Unreachable | CdpProbeStatus::CdpWithoutDiscordTarget => {}
    }

    let supervised_installation = selected_installation.cloned().unwrap_or_else(|| {
        crate::provider::vesktop_installation(install.clone(), crate::DiscoverySource::StandardPath)
    });
    let running = crate::ProcessSupervisor::installation_running(&supervised_installation);
    if running && !options.restart_existing {
        return Err(LaunchError::DesktopClientAlreadyRunning { client: "Vesktop" });
    }
    if running {
        crate::ProcessSupervisor::terminate_exact(&supervised_installation)?;
        crate::ProcessSupervisor::wait_for_exit(
            &supervised_installation,
            options.shutdown_timeout,
            options.poll_interval,
        )?;
    }

    match cdp.probe(options.port) {
        CdpProbeStatus::Unreachable => {}
        CdpProbeStatus::DiscordReady { .. } => {
            return Ok(vesktop_result(
                &install,
                &options,
                selected_installation,
                LaunchOutcome::AlreadyAvailable,
                None,
                true,
            ));
        }
        CdpProbeStatus::PortOccupied => {
            return Err(LaunchError::PortOccupied { port: options.port });
        }
        CdpProbeStatus::CdpWithoutDiscordTarget => {
            return Err(LaunchError::NonDiscordCdpTarget { port: options.port });
        }
    }

    let pid = spawn_vesktop(&install, DiscordLaunchMode::Cdp { port: options.port })?;
    if !options.wait_for_cdp {
        return Ok(vesktop_result(
            &install,
            &options,
            selected_installation,
            LaunchOutcome::Spawned,
            Some(pid),
            false,
        ));
    }

    if crate::ProcessSupervisor::wait_for_discord_ready(
        cdp,
        options.port,
        options.readiness_timeout,
        options.poll_interval,
    ) {
        return Ok(vesktop_result(
            &install,
            &options,
            selected_installation,
            LaunchOutcome::Spawned,
            Some(pid),
            true,
        ));
    }

    Err(LaunchError::ReadinessTimeout {
        port: options.port,
        timeout: options.readiness_timeout,
    })
}

fn vesktop_result(
    install: &VesktopInstall,
    options: &LaunchOptions,
    selected_installation: Option<&crate::ClientInstallation>,
    outcome: LaunchOutcome,
    pid: Option<u32>,
    cdp_connected: bool,
) -> LaunchResult {
    let installation = selected_installation.cloned().unwrap_or_else(|| {
        crate::provider::vesktop_installation(install.clone(), crate::DiscoverySource::StandardPath)
    });
    LaunchResult {
        outcome,
        launched_path: install.executable_path.clone(),
        channel: DiscordChannel::Stable,
        port: options.port,
        pid,
        cdp_connected,
        provider_id: ProviderId::vesktop(),
        installation_id: Some(installation.id),
        variant_id: None,
        ownership: if outcome == LaunchOutcome::Spawned {
            SessionOwnership::Managed
        } else {
            SessionOwnership::ExternalAttached
        },
    }
}

pub fn restart_discord_with_cdp(mut options: LaunchOptions) -> Result<LaunchResult, LaunchError> {
    options.restart_existing = true;
    launch_discord_with_cdp(options)
}

#[doc(hidden)]
pub fn launch_with_backends<P, C>(
    options: LaunchOptions,
    platform: &P,
    cdp: &C,
) -> Result<LaunchResult, LaunchError>
where
    P: PlatformBackend,
    C: CdpProbe,
{
    if options.port == 0 {
        return Err(LaunchError::InvalidPort(options.port));
    }

    match cdp.probe(options.port) {
        CdpProbeStatus::DiscordReady { .. } => {
            return already_available_result(&options, platform.find_installs()?);
        }
        CdpProbeStatus::PortOccupied => {
            return Err(LaunchError::PortOccupied { port: options.port });
        }
        CdpProbeStatus::Unreachable | CdpProbeStatus::CdpWithoutDiscordTarget => {}
    }

    let installs = platform.find_installs()?;
    let install = if let Some(selected) = &options.installation {
        crate::installation_as_official(selected).ok_or_else(|| {
            LaunchError::InvalidInstallation {
                details: "The selected installation is not a valid official Discord target.".into(),
            }
        })?
    } else {
        select_preferred_install(&installs, options.channel)?
    };
    let selected_channel = Some(install.channel);
    let running = platform.is_running(selected_channel)?;
    if running && !options.restart_existing {
        return Err(LaunchError::DiscordAlreadyRunning {
            channel: options.channel,
        });
    }
    if running {
        platform.terminate(selected_channel)?;
        wait_until_discord_exits(platform, selected_channel, &options)?;
    }

    match cdp.probe(options.port) {
        CdpProbeStatus::Unreachable => {}
        CdpProbeStatus::DiscordReady { .. } => {
            return already_available_result(&options, platform.find_installs()?);
        }
        CdpProbeStatus::PortOccupied => {
            return Err(LaunchError::PortOccupied { port: options.port });
        }
        CdpProbeStatus::CdpWithoutDiscordTarget => {
            return Err(LaunchError::NonDiscordCdpTarget { port: options.port });
        }
    }

    let pid = platform.spawn(&install, DiscordLaunchMode::Cdp { port: options.port })?;
    if !options.wait_for_cdp {
        return Ok(result_for(
            &install,
            &options,
            LaunchOutcome::Spawned,
            Some(pid),
            false,
        ));
    }

    let started = Instant::now();
    while started.elapsed() < options.readiness_timeout {
        if matches!(cdp.probe(options.port), CdpProbeStatus::DiscordReady { .. }) {
            return Ok(result_for(
                &install,
                &options,
                LaunchOutcome::Spawned,
                Some(pid),
                true,
            ));
        }
        std::thread::sleep(options.poll_interval);
    }

    Err(LaunchError::ReadinessTimeout {
        port: options.port,
        timeout: options.readiness_timeout,
    })
}

#[allow(dead_code)]
fn _assert_backend_object_safe(_backend: &dyn PlatformBackend) {}

fn already_available_result(
    options: &LaunchOptions,
    installs: Vec<DiscordInstall>,
) -> Result<LaunchResult, LaunchError> {
    if let Ok(install) = select_preferred_install(&installs, options.channel) {
        return Ok(result_for(
            &install,
            options,
            LaunchOutcome::AlreadyAvailable,
            None,
            true,
        ));
    }

    // CDP is authoritative: a working Discord CDP target may come from a
    // portable, Flatpak, or otherwise undiscoverable install. Do not reject an
    // already-usable session merely because no local executable path is known.
    Ok(LaunchResult {
        outcome: LaunchOutcome::AlreadyAvailable,
        launched_path: std::path::PathBuf::new(),
        channel: options.channel.unwrap_or(DiscordChannel::Stable),
        port: options.port,
        pid: None,
        cdp_connected: true,
        provider_id: match options.client {
            crate::DesktopClientPreference::Vesktop => ProviderId::vesktop(),
            _ => ProviderId::official_discord(),
        },
        installation_id: options
            .installation
            .as_ref()
            .map(|install| install.id.clone()),
        variant_id: options
            .installation
            .as_ref()
            .and_then(|install| install.variant_id.clone()),
        ownership: SessionOwnership::ExternalAttached,
    })
}

pub fn build_launch_args(mode: DiscordLaunchMode) -> Vec<OsString> {
    match mode {
        DiscordLaunchMode::Normal => Vec::new(),
        DiscordLaunchMode::Cdp { port } => {
            vec![OsString::from(format!("--remote-debugging-port={port}"))]
        }
    }
}

fn wait_until_discord_exits<P: PlatformBackend>(
    platform: &P,
    channel: Option<DiscordChannel>,
    options: &LaunchOptions,
) -> Result<(), LaunchError> {
    let started = Instant::now();
    while started.elapsed() < options.shutdown_timeout {
        if !platform.is_running(channel)? {
            return Ok(());
        }
        std::thread::sleep(options.poll_interval);
    }
    Err(LaunchError::ShutdownTimeout {
        timeout: options.shutdown_timeout,
    })
}

fn result_for(
    install: &DiscordInstall,
    options: &LaunchOptions,
    outcome: LaunchOutcome,
    pid: Option<u32>,
    cdp_connected: bool,
) -> LaunchResult {
    let installation = options
        .installation
        .clone()
        .unwrap_or_else(|| crate::provider::official_installation(install.clone()));
    LaunchResult {
        outcome,
        launched_path: install.executable_path.clone(),
        channel: install.channel,
        port: options.port,
        pid,
        cdp_connected,
        provider_id: ProviderId::official_discord(),
        installation_id: Some(installation.id),
        variant_id: installation
            .variant_id
            .or_else(|| Some(VariantId(install.channel.as_str().to_string()))),
        ownership: if outcome == LaunchOutcome::Spawned {
            SessionOwnership::Managed
        } else {
            SessionOwnership::ExternalAttached
        },
    }
}

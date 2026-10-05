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
use std::time::{Duration, Instant};

fn readiness_timeout_for_observation(
    status: &CdpProbeStatus,
    saw_cdp_server: &mut bool,
    base_timeout: Duration,
    extended_timeout: Duration,
) -> Duration {
    if matches!(status, CdpProbeStatus::CdpWithoutDiscordTarget) {
        *saw_cdp_server = true;
    }
    if *saw_cdp_server {
        extended_timeout
    } else {
        base_timeout
    }
}

fn wait_for_cdp_readiness<C: CdpProbe>(
    cdp: &C,
    options: &LaunchOptions,
) -> Result<(), LaunchError> {
    let started = Instant::now();
    let base_timeout = options.readiness_timeout;
    let extended_timeout = base_timeout.saturating_add(base_timeout);
    // Once a CDP server has appeared, keep the extended renderer grace period.
    // Discord's updater-to-app handoff can make the endpoint briefly disappear.
    let mut saw_cdp_server = false;
    let mut last = cdp.observe(options.port);
    let mut logged_status: Option<CdpProbeStatus> = None;

    loop {
        if logged_status.as_ref() != Some(&last.status) {
            eprintln!(
                "[cdp-launch] endpoint state={} targets={} elapsed={:.1}s",
                last.status.as_str(),
                last.target_count
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                started.elapsed().as_secs_f64()
            );
            logged_status = Some(last.status.clone());
        }
        match &last.status {
            CdpProbeStatus::DiscordReady { .. } => return Ok(()),
            CdpProbeStatus::PortOccupied => {
                return Err(LaunchError::PortOccupied { port: options.port });
            }
            CdpProbeStatus::CdpWithoutDiscordTarget => {}
            CdpProbeStatus::Unreachable => {}
        }
        let timeout = readiness_timeout_for_observation(
            &last.status,
            &mut saw_cdp_server,
            base_timeout,
            extended_timeout,
        );
        if started.elapsed() >= timeout {
            return Err(LaunchError::ReadinessTimeout {
                port: options.port,
                timeout,
                last_status: last.status,
                target_count: last.target_count,
                discord_target_count: last.discord_target_count,
                main_renderer_found: last.main_renderer_found,
            });
        }
        std::thread::sleep(options.poll_interval);
        last = cdp.observe(options.port);
    }
}

pub trait PlatformBackend {
    fn find_installs(&self) -> Result<Vec<DiscordInstall>, LaunchError>;
    fn is_running(&self, channel: Option<DiscordChannel>) -> Result<bool, LaunchError>;
    fn terminate(&self, channel: Option<DiscordChannel>) -> Result<(), LaunchError>;
    /// Launch the client and return its PID when the platform exposes it.
    /// Bundle launch services may only expose their own short-lived PID.
    fn spawn(
        &self,
        install: &DiscordInstall,
        mode: DiscordLaunchMode,
    ) -> Result<Option<u32>, LaunchError>;
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
        let selected_installation = options
            .installation
            .clone()
            .or_else(discovered_vesktop_installation);
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
    validated_flatpak_command(command.as_deref())?;
    if options.port == 0 {
        return Err(LaunchError::InvalidPort(options.port));
    }
    match cdp.probe(options.port) {
        CdpProbeStatus::DiscordReady { .. } => {
            if !options.restart_existing {
                return Ok(flatpak_result(
                    installation,
                    &options,
                    LaunchOutcome::AlreadyAvailable,
                    None,
                    true,
                ));
            }
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
    eprintln!("[cdp-launch] process spawned pid={pid}");
    if !options.wait_for_cdp {
        return Ok(flatpak_result(
            installation,
            &options,
            LaunchOutcome::Spawned,
            Some(pid),
            false,
        ));
    }
    wait_for_cdp_readiness(cdp, &options)?;
    Ok(flatpak_result(
        installation,
        &options,
        LaunchOutcome::Spawned,
        Some(pid),
        true,
    ))
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
    let command = validated_flatpak_command(command)?;
    let output = crate::linux_host_command(command)
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
    let command = validated_flatpak_command(command)?;
    let output = crate::linux_host_command(command)
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
    let command = validated_flatpak_command(command)?;
    let mut process = crate::linux_host_command(command);
    process.args(["run", app_id]);
    if let Some(port) = port {
        process.arg(format!("--remote-debugging-port={port}"));
    }
    process
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(reap_spawned_child)
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
    let vesktop_install_found = discovered_vesktop_installation().is_some();
    Ok(vesktop_launch_plan(
        options.channel,
        options.client,
        official_running,
        vesktop_running,
        official_install_found,
        vesktop_install_found,
    ) == VesktopLaunchPlan::LaunchVesktop)
}

fn discovered_vesktop_installation() -> Option<crate::ClientInstallation> {
    crate::discover_client_installations()
        .0
        .into_iter()
        .find(|installation| {
            installation.provider_id == ProviderId::vesktop()
                && installation.validation == crate::ValidationState::Valid
        })
}

fn validated_flatpak_command(command: Option<&str>) -> Result<&str, LaunchError> {
    let command = command.unwrap_or("flatpak");
    if command == "flatpak" {
        Ok(command)
    } else {
        Err(LaunchError::InvalidInstallation {
            details: "Flatpak launch targets must use the flatpak command.".into(),
        })
    }
}

#[cfg(target_os = "linux")]
fn reap_spawned_child(mut child: std::process::Child) -> u32 {
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    pid
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
            if !options.restart_existing {
                return Ok(vesktop_result(
                    &install,
                    &options,
                    selected_installation,
                    LaunchOutcome::AlreadyAvailable,
                    None,
                    true,
                ));
            }
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
    eprintln!("[cdp-launch] process spawned pid={pid}");
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

    wait_for_cdp_readiness(cdp, &options)?;
    Ok(vesktop_result(
        &install,
        &options,
        selected_installation,
        LaunchOutcome::Spawned,
        Some(pid),
        true,
    ))
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
            if !options.restart_existing {
                return already_available_result(
                    &options,
                    platform.find_installs()?,
                    Some(crate::inspect_cdp_port_owner(options.port)),
                );
            }
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
            channel: selected_channel,
        });
    }
    if running {
        platform.terminate(selected_channel)?;
        wait_until_discord_exits(platform, selected_channel, &options)?;
    }

    match cdp.probe(options.port) {
        CdpProbeStatus::Unreachable => {}
        CdpProbeStatus::DiscordReady { .. } => {
            return already_available_result(
                &options,
                platform.find_installs()?,
                Some(crate::inspect_cdp_port_owner(options.port)),
            );
        }
        CdpProbeStatus::PortOccupied => {
            return Err(LaunchError::PortOccupied { port: options.port });
        }
        CdpProbeStatus::CdpWithoutDiscordTarget => {
            return Err(LaunchError::NonDiscordCdpTarget { port: options.port });
        }
    }

    let pid = platform.spawn(&install, DiscordLaunchMode::Cdp { port: options.port })?;
    eprintln!("[cdp-launch] process spawned pid={pid:?}");
    if !options.wait_for_cdp {
        return Ok(result_for(
            &install,
            &options,
            LaunchOutcome::Spawned,
            pid,
            false,
        ));
    }

    wait_for_cdp_readiness(cdp, &options)?;
    Ok(result_for(
        &install,
        &options,
        LaunchOutcome::Spawned,
        pid,
        true,
    ))
}

#[allow(dead_code)]
fn _assert_backend_object_safe(_backend: &dyn PlatformBackend) {}

fn already_available_result(
    options: &LaunchOptions,
    installs: Vec<DiscordInstall>,
    owner: Option<crate::CdpPortOwner>,
) -> Result<LaunchResult, LaunchError> {
    let provider_id = match owner.unwrap_or(crate::CdpPortOwner::None) {
        crate::CdpPortOwner::Vesktop => ProviderId::vesktop(),
        crate::CdpPortOwner::Official => ProviderId::official_discord(),
        crate::CdpPortOwner::None | crate::CdpPortOwner::Other => match options.client {
            crate::DesktopClientPreference::Vesktop => ProviderId::vesktop(),
            _ => ProviderId::official_discord(),
        },
    };
    if provider_id == ProviderId::official_discord() {
        if let Ok(install) = select_preferred_install(&installs, options.channel) {
            return Ok(result_for(
                &install,
                options,
                LaunchOutcome::AlreadyAvailable,
                None,
                true,
            ));
        }
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
        provider_id,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_custom_flatpak_commands() {
        assert!(validated_flatpak_command(Some("flatpak")).is_ok());
        assert!(validated_flatpak_command(None).is_ok());
        assert!(matches!(
            validated_flatpak_command(Some("/tmp/launcher")),
            Err(LaunchError::InvalidInstallation { .. })
        ));
    }

    #[test]
    fn already_available_result_reports_the_observed_vesktop_owner() {
        let options = LaunchOptions {
            port: 9223,
            ..Default::default()
        };
        let result =
            already_available_result(&options, Vec::new(), Some(crate::CdpPortOwner::Vesktop))
                .expect("a ready CDP endpoint should be attachable");
        assert_eq!(result.provider_id, ProviderId::vesktop());
        assert_eq!(result.ownership, SessionOwnership::ExternalAttached);
    }

    #[test]
    fn renderer_grace_remains_enabled_during_endpoint_handoff() {
        let base = Duration::from_secs(15);
        let extended = Duration::from_secs(30);
        let mut saw_cdp_server = false;

        assert_eq!(
            readiness_timeout_for_observation(
                &CdpProbeStatus::CdpWithoutDiscordTarget,
                &mut saw_cdp_server,
                base,
                extended,
            ),
            extended
        );
        assert_eq!(
            readiness_timeout_for_observation(
                &CdpProbeStatus::Unreachable,
                &mut saw_cdp_server,
                base,
                extended,
            ),
            extended
        );
    }
}

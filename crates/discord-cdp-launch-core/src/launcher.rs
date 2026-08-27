use crate::platform::SystemPlatform;
use crate::vesktop::{
    cdp_ready_matches_preference, find_vesktop_install, is_vesktop_running, spawn_vesktop,
    terminate_vesktop, vesktop_launch_plan, VesktopInstall, VesktopLaunchPlan,
};
use crate::{
    CdpProbe, CdpProbeStatus, DiscordChannel, DiscordInstall, DiscordLaunchMode, LaunchError,
    LaunchOptions, LaunchOutcome, LaunchResult, StdCdpProbe,
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
    }
    if should_launch_vesktop(&options)? {
        let install =
            find_vesktop_install().ok_or(LaunchError::InstallNotFound { channel: None })?;
        return launch_vesktop_with_cdp(options, install, &StdCdpProbe::default());
    }
    launch_with_backends(options, &SystemPlatform, &StdCdpProbe::default())
}

fn should_launch_vesktop(options: &LaunchOptions) -> Result<bool, LaunchError> {
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

    let running = is_vesktop_running()?;
    if running && !options.restart_existing {
        return Err(LaunchError::DesktopClientAlreadyRunning { client: "Vesktop" });
    }
    if running {
        terminate_vesktop()?;
        wait_until_vesktop_exits(&options)?;
    }

    match cdp.probe(options.port) {
        CdpProbeStatus::Unreachable => {}
        CdpProbeStatus::DiscordReady { .. } => {
            return Ok(vesktop_result(
                &install,
                &options,
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
            LaunchOutcome::Spawned,
            Some(pid),
            false,
        ));
    }

    let started = Instant::now();
    while started.elapsed() < options.readiness_timeout {
        if matches!(cdp.probe(options.port), CdpProbeStatus::DiscordReady { .. }) {
            return Ok(vesktop_result(
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

fn wait_until_vesktop_exits(options: &LaunchOptions) -> Result<(), LaunchError> {
    let started = Instant::now();
    while started.elapsed() < options.shutdown_timeout {
        if !is_vesktop_running()? {
            return Ok(());
        }
        std::thread::sleep(options.poll_interval);
    }
    Err(LaunchError::ShutdownTimeout {
        timeout: options.shutdown_timeout,
    })
}

fn vesktop_result(
    install: &VesktopInstall,
    options: &LaunchOptions,
    outcome: LaunchOutcome,
    pid: Option<u32>,
    cdp_connected: bool,
) -> LaunchResult {
    // Vesktop is not a Discord release channel. LaunchResult still carries
    // DiscordChannel for the existing DTO; Vesktop hosts discord.com stable.
    LaunchResult {
        outcome,
        launched_path: install.executable_path.clone(),
        channel: DiscordChannel::Stable,
        port: options.port,
        pid,
        cdp_connected,
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

    let running = platform.is_running(options.channel)?;
    if running && !options.restart_existing {
        return Err(LaunchError::DiscordAlreadyRunning {
            channel: options.channel,
        });
    }
    if running {
        platform.terminate(options.channel)?;
        wait_until_discord_exits(platform, options.channel, &options)?;
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

    let install = select_preferred_install(&platform.find_installs()?, options.channel)?;
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
    LaunchResult {
        outcome,
        launched_path: install.executable_path.clone(),
        channel: install.channel,
        port: options.port,
        pid,
        cdp_connected,
    }
}

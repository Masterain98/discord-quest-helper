use crate::{CdpProbe, CdpProbeStatus, ClientInstallation, LaunchError};
use std::path::Path;
use std::time::{Duration, Instant};

/// Shared process/CDP lifecycle operations used by provider adapters.
pub struct ProcessSupervisor;

impl ProcessSupervisor {
    pub fn installation_running(installation: &ClientInstallation) -> bool {
        executable_path(installation).is_some_and(crate::is_installation_running)
    }

    pub fn terminate_exact(installation: &ClientInstallation) -> Result<(), LaunchError> {
        let path =
            executable_path(installation).ok_or_else(|| LaunchError::InvalidInstallation {
                details: "The launch target has no exact executable path.".into(),
            })?;
        crate::terminate_installation_process_tree(path)
    }

    pub fn wait_for_exit(
        installation: &ClientInstallation,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<(), LaunchError> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if !Self::installation_running(installation) {
                return Ok(());
            }
            std::thread::sleep(poll_interval);
        }
        Err(LaunchError::ShutdownTimeout { timeout })
    }

    pub fn wait_for_discord_ready<C: CdpProbe>(
        probe: &C,
        port: u16,
        timeout: Duration,
        poll_interval: Duration,
    ) -> bool {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if matches!(probe.probe(port), CdpProbeStatus::DiscordReady { .. }) {
                return true;
            }
            std::thread::sleep(poll_interval);
        }
        false
    }
}

fn executable_path(installation: &ClientInstallation) -> Option<&Path> {
    match &installation.launch_target {
        crate::LaunchTarget::Executable { path, .. } => Some(path),
        crate::LaunchTarget::MacBundle {
            executable_path, ..
        } => Some(executable_path),
        crate::LaunchTarget::Flatpak { .. } => None,
    }
}

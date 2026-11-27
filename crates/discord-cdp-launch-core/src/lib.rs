mod cdp;
mod channel;
mod error;
mod launcher;
#[cfg(any(target_os = "linux", test))]
mod linux_environment;
#[cfg(target_os = "linux")]
pub use linux_environment::linux_host_command;
mod model;
mod platform;
mod processes;
mod provider;
mod runtime;
mod supervisor;
mod vesktop;

pub use cdp::{
    classify_cdp_target, detailed_probe_cdp, is_discord_auxiliary_page,
    is_discord_auxiliary_window, is_discord_target, is_transient_cdp_io_error, list_cdp_targets,
    list_cdp_targets_with_timeouts, parse_cdp_targets_http_response, probe_cdp, CdpListError,
    CdpProbe, StdCdpProbe,
};
pub use channel::{parse_discord_channel, DiscordChannel};
pub use error::LaunchError;
pub use launcher::{
    build_launch_args, find_discord_installs, is_cdp_available, is_discord_running,
    launch_discord_with_cdp, launch_with_backends, restart_discord_with_cdp,
    select_preferred_install, terminate_discord_processes, PlatformBackend,
};
pub use model::{
    CdpDiagnosticProcess, CdpDiagnosticTarget, CdpPortOwner, CdpProbeObservation, CdpProbeStatus,
    CdpRuntime, CdpRuntimeStatus, CdpTarget, CdpTargetClassification, ClientCapabilities,
    ClientInstallation, DesktopCdpSession, DesktopClientPreference, DetailedCdpProbeResult,
    DiscordInstall, DiscordLaunchMode, DiscoverySource, InstallationId, LaunchOptions,
    LaunchOutcome, LaunchResult, LaunchSelector, LaunchTarget, LinuxDesktopProxySettings,
    ProviderId, RestoreFailure, RestoreResult, RunningCdpSession, SessionOwnership,
    ValidationState, VariantId, DEFAULT_CDP_PORT,
};
pub use platform::SystemPlatform;
pub use processes::{
    cdp_diagnostic_processes, inspect_cdp_port_owner, is_client_installation_running,
    is_installation_running, list_running_desktop_cdp_sessions, list_running_discord_cdp_sessions,
    restore_all_discord_to_normal, restore_desktop_client_to_normal, running_vesktop_installs,
    terminate_installation_process_tree,
};
#[cfg(target_os = "linux")]
pub use provider::parse_desktop_exec_arguments;
pub use provider::{
    custom_executable_installation, discover_client_installations, installation_as_official,
    installation_as_vesktop, provider_registry, refresh_automatic_installation,
    refresh_installation_validation, same_executable_path, DesktopClientProvider,
};
pub use supervisor::ProcessSupervisor;
/// Validate discovery-provided debugger URLs without DNS, proxies, or credentials.
pub fn is_loopback_cdp_websocket(port: u16, ws_url: &str) -> bool {
    runtime::is_loopback_websocket(port, ws_url)
}
pub fn verify_cdp_target(port: u16, target: &CdpTarget) -> CdpRuntime {
    runtime::verify(
        port,
        target,
        std::time::Instant::now() + std::time::Duration::from_millis(750),
    )
}
pub use vesktop::{
    cdp_ready_matches_preference, discover_linux_vesktop_install_in,
    discover_macos_vesktop_install_in, discover_windows_vesktop_install_in, find_vesktop_install,
    is_vesktop_process_name, is_vesktop_running, parse_desktop_client_preference, vesktop_cdp_args,
    vesktop_launch_plan, VesktopInstall, VesktopLaunchPlan,
};

#[cfg(target_os = "windows")]
#[doc(hidden)]
pub use platform::windows::{
    discover_windows_installs_in, refresh_windows_official_installation,
    windows_legacy_installation_id_matches, windows_official_installation_id,
};

#[cfg(target_os = "macos")]
#[doc(hidden)]
pub use platform::macos::discover_macos_installs_in;

#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use platform::linux::{
    classify_linux_process, discover_linux_installs_in, linux_desktop_proxy_config,
    linux_desktop_proxy_settings, LinuxProcessInfo,
};

/// Run a platform settings query with bounded output and execution time.
#[cfg(unix)]
#[doc(hidden)]
pub fn proxy_command_output(
    command: &mut std::process::Command,
) -> std::io::Result<Option<Vec<u8>>> {
    use std::{
        io::Read,
        process::Stdio,
        sync::mpsc,
        time::{Duration, Instant},
    };

    let deadline = Instant::now() + Duration::from_millis(750);
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let stdout = child.stdout.take().expect("piped stdout");
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let output = stdout
            .take(64 * 1024)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = send.send(output);
    });
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return receive
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|_| std::io::ErrorKind::TimedOut)?
                    .map(Some);
            }
            Ok(Some(_)) => return Ok(None),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Err(std::io::ErrorKind::TimedOut.into());
            }
        }
    }
}

#[cfg(all(test, unix))]
mod proxy_command_tests {
    use super::proxy_command_output;
    use std::{
        io::ErrorKind,
        process::Command,
        time::{Duration, Instant},
    };

    #[test]
    fn reads_the_output_of_a_successful_proxy_query() {
        let output = proxy_command_output(Command::new("/bin/echo").arg("manual")).unwrap();
        assert_eq!(output.as_deref(), Some(&b"manual\n"[..]));
    }

    #[test]
    fn times_out_a_hung_proxy_query() {
        let start = Instant::now();
        let error = proxy_command_output(Command::new("/bin/sleep").arg("30")).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}

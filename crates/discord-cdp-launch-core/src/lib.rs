mod cdp;
mod channel;
mod error;
mod launcher;
mod model;
mod platform;
mod processes;
mod provider;
mod supervisor;
mod vesktop;

pub use cdp::{
    is_discord_auxiliary_page, is_discord_auxiliary_window, is_discord_target, pick_discord_target,
    probe_cdp, CdpProbe, StdCdpProbe,
};
pub use channel::{parse_discord_channel, DiscordChannel};
pub use error::LaunchError;
pub use launcher::{
    build_launch_args, find_discord_installs, is_cdp_available, is_discord_running,
    launch_discord_with_cdp, launch_with_backends, restart_discord_with_cdp,
    select_preferred_install, terminate_discord_processes, PlatformBackend,
};
pub use model::{
    CdpPortOwner, CdpProbeStatus, CdpTarget, ClientCapabilities, ClientInstallation,
    DesktopCdpSession, DesktopClientPreference, DiscordInstall, DiscordLaunchMode, DiscoverySource,
    InstallationId, LaunchOptions, LaunchOutcome, LaunchResult, LaunchSelector, LaunchTarget,
    LinuxDesktopProxySettings, ProviderId, RestoreFailure, RestoreResult, RunningCdpSession,
    SessionOwnership, ValidationState, VariantId, DEFAULT_CDP_PORT,
};
pub use platform::SystemPlatform;
pub use processes::{
    inspect_cdp_port_owner, is_installation_running, list_running_desktop_cdp_sessions,
    list_running_discord_cdp_sessions, restore_all_discord_to_normal,
    restore_desktop_client_to_normal, running_vesktop_installs,
    terminate_installation_process_tree,
};
pub use provider::{
    custom_executable_installation, discover_client_installations, installation_as_official,
    installation_as_vesktop, provider_registry, refresh_installation_validation,
    DesktopClientProvider,
};
pub use supervisor::ProcessSupervisor;
pub use vesktop::{
    cdp_ready_matches_preference, discover_linux_vesktop_install_in,
    discover_macos_vesktop_install_in, discover_windows_vesktop_install_in, find_vesktop_install,
    is_vesktop_process_name, is_vesktop_running, parse_desktop_client_preference, vesktop_cdp_args,
    vesktop_launch_plan, VesktopInstall, VesktopLaunchPlan,
};

#[cfg(target_os = "windows")]
#[doc(hidden)]
pub use platform::windows::discover_windows_installs_in;

#[cfg(target_os = "macos")]
#[doc(hidden)]
pub use platform::macos::discover_macos_installs_in;

#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use platform::linux::{
    classify_linux_process, discover_linux_installs_in, linux_desktop_proxy_settings,
    LinuxProcessInfo,
};

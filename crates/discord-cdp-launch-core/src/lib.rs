mod cdp;
mod channel;
mod error;
mod launcher;
mod model;
mod platform;

pub use cdp::{is_discord_target, pick_discord_target, probe_cdp, CdpProbe, StdCdpProbe};
pub use channel::{parse_discord_channel, DiscordChannel};
pub use error::LaunchError;
pub use launcher::{
    build_launch_args, find_discord_installs, is_cdp_available, is_discord_running,
    launch_discord_with_cdp, launch_with_backends, restart_discord_with_cdp,
    select_preferred_install, terminate_discord_processes, PlatformBackend,
};
pub use model::{
    CdpProbeStatus, CdpTarget, DiscordInstall, LaunchOptions, LaunchOutcome, LaunchResult,
    LinuxDesktopProxySettings, DEFAULT_CDP_PORT,
};
pub use platform::SystemPlatform;

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

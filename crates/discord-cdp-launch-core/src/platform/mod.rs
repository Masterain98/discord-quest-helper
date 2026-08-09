#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod unsupported;
#[cfg(target_os = "windows")]
pub mod windows;

use crate::launcher::PlatformBackend;
use crate::{DiscordChannel, DiscordInstall, DiscordLaunchMode, LaunchError};

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemPlatform;

#[cfg(target_os = "windows")]
impl PlatformBackend for SystemPlatform {
    fn find_installs(&self) -> Result<Vec<DiscordInstall>, LaunchError> {
        windows::find_installs()
    }

    fn is_running(&self, channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
        windows::is_running(channel)
    }

    fn terminate(&self, channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
        windows::terminate(channel)
    }

    fn spawn(&self, install: &DiscordInstall, mode: DiscordLaunchMode) -> Result<u32, LaunchError> {
        windows::spawn(install, mode)
    }
}

#[cfg(target_os = "macos")]
impl PlatformBackend for SystemPlatform {
    fn find_installs(&self) -> Result<Vec<DiscordInstall>, LaunchError> {
        macos::find_installs()
    }

    fn is_running(&self, channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
        macos::is_running(channel)
    }

    fn terminate(&self, channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
        macos::terminate(channel)
    }

    fn spawn(&self, install: &DiscordInstall, mode: DiscordLaunchMode) -> Result<u32, LaunchError> {
        macos::spawn(install, mode)
    }
}

#[cfg(target_os = "linux")]
impl PlatformBackend for SystemPlatform {
    fn find_installs(&self) -> Result<Vec<DiscordInstall>, LaunchError> {
        linux::find_installs()
    }

    fn is_running(&self, channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
        linux::is_running(channel)
    }

    fn terminate(&self, channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
        linux::terminate(channel)
    }

    fn spawn(&self, install: &DiscordInstall, mode: DiscordLaunchMode) -> Result<u32, LaunchError> {
        linux::spawn(install, mode)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
impl PlatformBackend for SystemPlatform {
    fn find_installs(&self) -> Result<Vec<DiscordInstall>, LaunchError> {
        unsupported::unsupported()
    }

    fn is_running(&self, _channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
        unsupported::unsupported()
    }

    fn terminate(&self, _channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
        unsupported::unsupported()
    }

    fn spawn(
        &self,
        _install: &DiscordInstall,
        _mode: DiscordLaunchMode,
    ) -> Result<u32, LaunchError> {
        unsupported::unsupported()
    }
}

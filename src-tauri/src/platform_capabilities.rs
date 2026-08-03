//! Read-only description of what the current platform build supports.
//!
//! The frontend consumes this to drive platform-aware behaviour (executable
//! selection, default quest mode, launcher-entry availability) instead of
//! hardcoding `win32`. This is a new, additive command: it does not change any
//! existing IPC contract, and Windows/macOS keep their current behaviour.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    /// Host operating system: `windows`, `macos`, `linux`, or `unknown`.
    pub os: &'static str,
    /// Host architecture: `x86_64`, `aarch64`, or `unknown`.
    pub arch: &'static str,
    /// Whether the bundled CDP launcher sidecar is supported.
    pub cdp_launcher: bool,
    /// Whether a desktop launcher entry (shortcut / `.desktop`) can be created.
    pub launcher_entry: bool,
    /// Whether native game-process simulation is available.
    pub game_simulation: bool,
    /// Token auto-detection level: `full`, `manual_only`, or `unavailable`.
    pub token_auto_detection: &'static str,
    /// Executable `os` values to try, in priority order, when resolving a
    /// detectable game's simulation executable.
    pub executable_os_priority: Vec<&'static str>,
    /// Default game quest mode when the user has no saved preference.
    pub default_game_quest_mode: &'static str,
}

impl PlatformCapabilities {
    fn detect() -> Self {
        Self::for_os(std::env::consts::OS, detect_arch())
    }

    /// Build the descriptor for a named OS. Split out from [`Self::detect`] so
    /// every branch is reachable from a single-host test run — `cfg` blocks
    /// would only ever compile the branch matching the build target.
    fn for_os(os: &str, arch: &'static str) -> Self {
        match os {
            "windows" => PlatformCapabilities {
                os: "windows",
                arch,
                cdp_launcher: true,
                launcher_entry: true,
                game_simulation: true,
                token_auto_detection: "full",
                executable_os_priority: vec!["win32"],
                default_game_quest_mode: "simulate",
            },
            "macos" => PlatformCapabilities {
                os: "macos",
                arch,
                cdp_launcher: true,
                launcher_entry: true,
                game_simulation: true,
                token_auto_detection: "full",
                executable_os_priority: vec!["win32"],
                default_game_quest_mode: "simulate",
            },
            "linux" => PlatformCapabilities {
                os: "linux",
                arch,
                cdp_launcher: true,
                launcher_entry: true,
                game_simulation: true,
                token_auto_detection: "full",
                executable_os_priority: vec!["linux", "win32"],
                default_game_quest_mode: "cdp",
            },
            _ => PlatformCapabilities {
                os: "unknown",
                arch,
                cdp_launcher: false,
                launcher_entry: false,
                game_simulation: false,
                token_auto_detection: "unavailable",
                executable_os_priority: vec!["win32"],
                default_game_quest_mode: "heartbeat",
            },
        }
    }
}

fn detect_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    }
}

/// Return the current platform's capability descriptor. Additive command; does
/// not alter any existing IPC.
#[tauri::command]
pub fn get_platform_capabilities() -> PlatformCapabilities {
    PlatformCapabilities::detect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_and_macos_stay_win32_simulate() {
        for os in ["windows", "macos"] {
            let caps = PlatformCapabilities::for_os(os, "x86_64");
            assert_eq!(caps.os, os);
            assert_eq!(caps.executable_os_priority, vec!["win32"]);
            assert_eq!(caps.default_game_quest_mode, "simulate");
            assert_eq!(caps.token_auto_detection, "full");
            assert!(caps.cdp_launcher && caps.launcher_entry && caps.game_simulation);
        }
    }

    #[test]
    fn linux_prefers_native_executables_and_defaults_to_cdp() {
        let caps = PlatformCapabilities::for_os("linux", "x86_64");
        assert_eq!(caps.os, "linux");
        assert_eq!(caps.executable_os_priority, vec!["linux", "win32"]);
        assert_eq!(caps.default_game_quest_mode, "cdp");
        assert_eq!(caps.token_auto_detection, "full");
        assert!(caps.cdp_launcher && caps.launcher_entry && caps.game_simulation);
    }

    #[test]
    fn unrecognized_os_degrades_to_heartbeat_only() {
        let caps = PlatformCapabilities::for_os("freebsd", "aarch64");
        assert_eq!(caps.os, "unknown");
        assert_eq!(caps.arch, "aarch64");
        assert_eq!(caps.default_game_quest_mode, "heartbeat");
        assert_eq!(caps.token_auto_detection, "unavailable");
        assert!(!caps.cdp_launcher && !caps.launcher_entry && !caps.game_simulation);
    }

    #[test]
    fn detect_matches_the_host_target() {
        let caps = PlatformCapabilities::detect();
        assert_eq!(
            caps.os,
            PlatformCapabilities::for_os(std::env::consts::OS, caps.arch).os
        );
        assert_ne!(caps.arch, "");
    }
}

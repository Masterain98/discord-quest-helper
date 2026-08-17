//! CDP play-quest spoof fidelity helpers.
//!
//! Live Discord CDP on Windows :9223 (2026-08-13) confirmed the scanner object
//! keys below, including four own-properties that were `undefined` on a Chrome
//! non-game. Official PLAY_ON_DESKTOP heartbeats in
//! `discord-heartbeat-game-quest.har` are exactly 60s apart. Path and
//! executable selection are OS-specific:
//!
//! - Windows: `win32` + `C:\Program Files\...`
//! - macOS: Unix-family — prefer `darwin`, emit `/Applications/*.app/Contents/MacOS/...`
//! - Linux: Unix-family — prefer `linux`, emit `/opt/...`
//!
//! Simulation mode on macOS still uses win32 executable names; these helpers
//! are for CDP store spoofing only.

use once_cell::sync::Lazy;
use rand::RngExt;
use serde::Serialize;

/// Discord detectable-game OS tags used by `/applications/public`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectableOs {
    Win32,
    Darwin,
    Linux,
}

impl DetectableOs {
    pub fn from_host() -> Self {
        match std::env::consts::OS {
            "macos" => Self::Darwin,
            "linux" => Self::Linux,
            _ => Self::Win32,
        }
    }

    pub fn as_api_tag(self) -> &'static str {
        match self {
            Self::Win32 => "win32",
            Self::Darwin => "darwin",
            Self::Linux => "linux",
        }
    }

    /// macOS and Linux share Unix process-name / path conventions for CDP spoof.
    pub fn is_unix(self) -> bool {
        matches!(self, Self::Darwin | Self::Linux)
    }

    /// Native tag first, then win32 so a Unix host still has a name to spoof.
    /// Paths are always rendered for `self`, never as `C:\Program Files` on Unix.
    pub fn cdp_executable_os_priority(self) -> &'static [&'static str] {
        match self {
            Self::Win32 => &["win32"],
            Self::Darwin => &["darwin", "win32"],
            Self::Linux => &["linux", "win32"],
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationExecutable {
    pub os: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeGamePathTemplates {
    pub cmd_line: &'static str,
    pub exe_path: &'static str,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeGamePaths {
    pub cmd_line: String,
    pub exe_path: String,
    pub exe_name: String,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunningGameIdentity<'a> {
    pub id: &'a str,
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulatedProcessHint {
    pub pid: u32,
    pub exe_name: String,
    pub exe_path: String,
    pub cmd_line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CdpVideoTiming {
    pub speed: u32,
    pub interval: u32,
    pub max_future: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupVerify {
    pub dqh_present: bool,
    pub spoof_active: bool,
    pub fake_game_present: bool,
    pub has_dispatch_hook: bool,
    pub broad_patch_count: u64,
    pub observer_hook: bool,
    pub fake_in_running_games: bool,
    pub debug_game_present: bool,
}

/// Official Discord desktop game-quest heartbeat cadence, from
/// `discord-heartbeat-game-quest.har` (60.000s ± 10ms between POSTs).
pub const OFFICIAL_GAME_HEARTBEAT_SECS: u64 = 60;

/// RunningGameStore object keys observed on a live Windows Discord client.
/// Chrome non-game also owns `distributor`, `sku`, `gameMetadata`, and
/// `executableFingerprint` as enumerable keys with `undefined` values.
#[cfg(test)]
pub const RUNNING_GAME_SCHEMA_KEYS: &[&str] = &[
    "cmdLine",
    "distributor",
    "elevated",
    "exeName",
    "exePath",
    "executableFingerprint",
    "fullscreenType",
    "gameMetadata",
    "hidden",
    "id",
    "isLauncher",
    "lastFocused",
    "name",
    "nativeProcessObserverId",
    "origGameName",
    "pid",
    "pidPath",
    "processName",
    "sandboxed",
    "sku",
    "start",
    "windowHandle",
];

pub fn cdp_bridge_name() -> &'static str {
    static NAME: Lazy<String> = Lazy::new(|| {
        let mut rng = rand::rng();
        let suffix: String = (0..10)
            .map(|_| format!("{:x}", rng.random::<u8>() % 16))
            .collect();
        format!("__n{suffix}")
    });
    NAME.as_str()
}

/// Rewrite the historical `__dqh_cdp` identifier to the process-scoped bridge.
pub fn with_bridge(js: &str) -> String {
    js.replace("__dqh_cdp", cdp_bridge_name())
}

pub fn path_templates(os: DetectableOs) -> FakeGamePathTemplates {
    match os {
        DetectableOs::Win32 => FakeGamePathTemplates {
            cmd_line: r#""C:\Program Files\{app}\{exe}""#,
            exe_path: r"c:/program files/{app_lower}/{exe}",
        },
        DetectableOs::Darwin => FakeGamePathTemplates {
            cmd_line: "/Applications/{app}.app/Contents/MacOS/{exe}",
            exe_path: "/Applications/{app}.app/Contents/MacOS/{exe}",
        },
        DetectableOs::Linux => FakeGamePathTemplates {
            cmd_line: "/opt/{app_slug}/{exe}",
            exe_path: "/opt/{app_slug}/{exe}",
        },
    }
}

/// Test-only mirrors of the injected JavaScript helpers `sanitizeApp`,
/// `normalizeExe`, `appSlug`, and `render` in `cdp_quest.rs`. Keep both
/// implementations in lockstep.
#[cfg(test)]
fn sanitize_app_name(app_name: &str) -> String {
    app_name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            _ => ch,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
fn normalize_exe_name(os: DetectableOs, raw: &str) -> String {
    let trimmed = raw.trim().trim_start_matches('>');
    let file = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed).trim();
    if os.is_unix() {
        let cut = file.len().saturating_sub(4);
        if file
            .get(cut..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".exe"))
        {
            file[..cut].to_string()
        } else {
            file.to_string()
        }
    } else {
        file.to_string()
    }
}

#[cfg(test)]
fn app_slug(app: &str) -> String {
    app.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
fn render_template(template: &str, app: &str, exe: &str) -> String {
    template
        .replace("{app}", app)
        .replace("{app_lower}", &app.to_lowercase())
        .replace("{app_slug}", &app_slug(app))
        .replace("{exe}", exe)
}

#[cfg(test)]
fn build_fake_game_paths(os: DetectableOs, app_name: &str, exe_name: &str) -> FakeGamePaths {
    let app = sanitize_app_name(app_name);
    let exe = normalize_exe_name(os, exe_name);
    let templates = path_templates(os);
    FakeGamePaths {
        cmd_line: render_template(templates.cmd_line, &app, &exe),
        exe_path: render_template(templates.exe_path, &app, &exe),
        exe_name: exe,
    }
}

#[cfg(test)]
fn select_executable<'a>(
    executables: &'a [ApplicationExecutable],
    priority: &[&str],
) -> Option<&'a ApplicationExecutable> {
    for os in priority {
        if let Some(found) = executables.iter().find(|item| item.os == *os) {
            return Some(found);
        }
    }
    executables.first()
}

#[cfg(test)]
fn merge_running_games<'a>(
    real: &[RunningGameIdentity<'a>],
    fake: RunningGameIdentity<'a>,
) -> Vec<RunningGameIdentity<'a>> {
    if real
        .iter()
        .any(|game| game.id == fake.id || game.pid == fake.pid)
    {
        return real.to_vec();
    }
    let mut merged = real.to_vec();
    merged.push(fake);
    merged
}

#[cfg(test)]
fn match_process_hint<'a>(
    hints: &'a [SimulatedProcessHint],
    exe_name: &str,
) -> Option<&'a SimulatedProcessHint> {
    hints.iter().find(|hint| {
        hint.exe_name.eq_ignore_ascii_case(exe_name)
            || hint
                .exe_path
                .rsplit(['/', '\\'])
                .next()
                .is_some_and(|file| file.eq_ignore_ascii_case(exe_name))
    })
}

pub fn cdp_video_timing() -> CdpVideoTiming {
    CdpVideoTiming {
        speed: 10,
        interval: 10,
        max_future: 10,
    }
}

pub fn cdp_video_timeout_secs(seconds_needed: u32, initial_progress: f64) -> u64 {
    let remaining = (seconds_needed as f64 - initial_progress).max(0.0);
    let timing = cdp_video_timing();
    let wall = remaining * timing.interval.max(1) as f64 / timing.speed.max(1) as f64;
    (wall * 2.0).ceil() as u64 + 300
}

pub fn align_play_activity_heartbeat_secs(requested: u64) -> u64 {
    requested.max(OFFICIAL_GAME_HEARTBEAT_SECS)
}

pub fn cleanup_verify_is_clean(verify: &CleanupVerify) -> bool {
    !verify.dqh_present
        && !verify.spoof_active
        && !verify.fake_game_present
        && !verify.has_dispatch_hook
        && verify.broad_patch_count == 0
        && !verify.observer_hook
        && !verify.fake_in_running_games
        && !verify.debug_game_present
}

pub fn cleanup_verify_from_json(parsed: &serde_json::Value) -> Option<CleanupVerify> {
    if parsed.get("success") != Some(&serde_json::json!(true)) {
        return None;
    }
    Some(CleanupVerify {
        dqh_present: parsed
            .get("dqhPresent")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        spoof_active: parsed
            .get("spoofActive")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        fake_game_present: parsed
            .get("fakeGamePresent")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        has_dispatch_hook: parsed
            .get("hasDispatchHook")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        broad_patch_count: parsed
            .get("broadPatchCount")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        observer_hook: parsed
            .get("observerHook")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        fake_in_running_games: parsed
            .get("fakeInRunningGames")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        debug_game_present: parsed
            .get("debugGamePresent")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_hosts_prefer_native_executables_then_win32() {
        assert_eq!(
            DetectableOs::Linux.cdp_executable_os_priority(),
            &["linux", "win32"]
        );
        assert_eq!(
            DetectableOs::Darwin.cdp_executable_os_priority(),
            &["darwin", "win32"]
        );
        assert_eq!(DetectableOs::Win32.cdp_executable_os_priority(), &["win32"]);
        assert!(DetectableOs::Linux.is_unix());
        assert!(DetectableOs::Darwin.is_unix());
        assert!(!DetectableOs::Win32.is_unix());
    }

    #[test]
    fn macos_and_linux_are_both_unix_family_for_cdp_spoof() {
        for os in [DetectableOs::Darwin, DetectableOs::Linux] {
            let paths = build_fake_game_paths(os, "Cool Game", ">Game.exe");
            assert!(
                !paths.cmd_line.contains("Program Files"),
                "{os:?} must not emit Windows paths: {}",
                paths.cmd_line
            );
            assert!(
                !paths.exe_path.contains("program files"),
                "{os:?} must not emit Windows paths: {}",
                paths.exe_path
            );
            assert_eq!(paths.exe_name, "Game");
            assert_eq!(
                build_fake_game_paths(os, "Cool Game", "Game.Exe").exe_name,
                "Game"
            );
        }
    }

    #[test]
    fn macos_uses_application_bundle_paths() {
        let paths = build_fake_game_paths(DetectableOs::Darwin, "Cool Game", "Cool Game");
        assert_eq!(
            paths.cmd_line,
            "/Applications/Cool Game.app/Contents/MacOS/Cool Game"
        );
        assert_eq!(
            paths.exe_path,
            "/Applications/Cool Game.app/Contents/MacOS/Cool Game"
        );
    }

    #[test]
    fn linux_uses_opt_prefix_paths() {
        let paths = build_fake_game_paths(DetectableOs::Linux, "Cool Game", "cool-game");
        assert_eq!(paths.cmd_line, "/opt/cool-game/cool-game");
        assert_eq!(paths.exe_path, "/opt/cool-game/cool-game");
    }

    #[test]
    fn windows_keeps_program_files_shape() {
        let paths = build_fake_game_paths(DetectableOs::Win32, "Cool Game", ">Game.exe");
        assert_eq!(paths.cmd_line, r#""C:\Program Files\Cool Game\Game.exe""#);
        assert_eq!(paths.exe_path, "c:/program files/cool game/Game.exe");
        assert_eq!(paths.exe_name, "Game.exe");
    }

    #[test]
    fn select_executable_prefers_first_matching_os() {
        let executables = [
            ApplicationExecutable {
                os: "win32".into(),
                name: "game.exe".into(),
            },
            ApplicationExecutable {
                os: "darwin".into(),
                name: "Game".into(),
            },
            ApplicationExecutable {
                os: "linux".into(),
                name: "game".into(),
            },
        ];
        assert_eq!(
            select_executable(&executables, &["darwin", "win32"])
                .unwrap()
                .name,
            "Game"
        );
        assert_eq!(
            select_executable(&executables, &["linux", "win32"])
                .unwrap()
                .name,
            "game"
        );
        assert_eq!(
            select_executable(&executables, &["win32"]).unwrap().name,
            "game.exe"
        );
    }

    #[test]
    fn unix_falls_back_to_win32_name_but_keeps_unix_paths() {
        let executables = [ApplicationExecutable {
            os: "win32".into(),
            name: ">Title.exe".into(),
        }];
        let selected = select_executable(
            &executables,
            DetectableOs::Darwin.cdp_executable_os_priority(),
        )
        .unwrap();
        let paths = build_fake_game_paths(DetectableOs::Darwin, "Title", &selected.name);
        assert_eq!(paths.exe_name, "Title");
        assert!(paths.cmd_line.starts_with("/Applications/"));
        assert!(!paths.cmd_line.contains(".exe"));
    }

    #[test]
    fn merge_keeps_native_entry_with_the_same_application_id() {
        let real = [
            RunningGameIdentity {
                id: "other",
                pid: 11,
            },
            RunningGameIdentity {
                id: "stale",
                pid: 22,
            },
        ];
        let fake = RunningGameIdentity {
            id: "stale",
            pid: 99,
        };
        let merged = merge_running_games(&real, fake);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "other");
        assert_eq!(merged[1].id, "stale");
        assert_eq!(merged[1].pid, 22);
    }

    #[test]
    fn merge_appends_fake_only_when_id_is_absent() {
        let real = [RunningGameIdentity {
            id: "other",
            pid: 11,
        }];
        let fake = RunningGameIdentity {
            id: "fresh",
            pid: 99,
        };
        let merged = merge_running_games(&real, fake);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1].id, "fresh");
        assert_eq!(merged[1].pid, 99);
    }

    #[test]
    fn process_hint_matches_exe_name_or_path_basename() {
        let hints = [SimulatedProcessHint {
            pid: 4242,
            exe_name: "Game".into(),
            exe_path: "/Applications/Game.app/Contents/MacOS/Game".into(),
            cmd_line: "/Applications/Game.app/Contents/MacOS/Game".into(),
        }];
        assert_eq!(match_process_hint(&hints, "Game").unwrap().pid, 4242);
        assert_eq!(
            match_process_hint(&hints, "game").unwrap().exe_path,
            "/Applications/Game.app/Contents/MacOS/Game"
        );
        assert!(match_process_hint(&hints, "other").is_none());
    }

    #[test]
    fn running_game_schema_lists_client_scanner_fields() {
        for key in [
            "exePath",
            "cmdLine",
            "pid",
            "pidPath",
            "exeName",
            "nativeProcessObserverId",
            "elevated",
            "sandboxed",
            "lastFocused",
            "windowHandle",
            "fullscreenType",
            "origGameName",
            "distributor",
            "sku",
            "gameMetadata",
            "executableFingerprint",
            "start",
        ] {
            assert!(RUNNING_GAME_SCHEMA_KEYS.contains(&key));
        }
    }

    #[test]
    fn official_heartbeat_is_sixty_seconds() {
        assert_eq!(align_play_activity_heartbeat_secs(15), 60);
        assert_eq!(align_play_activity_heartbeat_secs(60), 60);
        assert_eq!(align_play_activity_heartbeat_secs(90), 90);
    }

    #[test]
    fn video_timing_is_realtime_not_seven_x() {
        let timing = cdp_video_timing();
        assert_eq!(timing.speed, timing.interval);
        assert_eq!(timing.max_future, 10);
        assert_ne!(timing.speed, 7);
        assert_eq!(cdp_video_timeout_secs(700, 0.0), 1700);
    }

    #[test]
    fn cleanup_verify_requires_every_hook_gone() {
        let clean = CleanupVerify {
            dqh_present: false,
            spoof_active: false,
            fake_game_present: false,
            has_dispatch_hook: false,
            broad_patch_count: 0,
            observer_hook: false,
            fake_in_running_games: false,
            debug_game_present: false,
        };
        assert!(cleanup_verify_is_clean(&clean));
        assert!(!cleanup_verify_is_clean(&CleanupVerify {
            has_dispatch_hook: true,
            ..clean.clone()
        }));
        assert!(!cleanup_verify_is_clean(&CleanupVerify {
            observer_hook: true,
            ..clean.clone()
        }));
        assert!(!cleanup_verify_is_clean(&CleanupVerify {
            debug_game_present: true,
            ..clean.clone()
        }));
        assert!(cleanup_verify_from_json(&serde_json::json!({
            "success": true,
            "dqhPresent": false,
            "spoofActive": false,
            "fakeGamePresent": false,
            "hasDispatchHook": false,
            "broadPatchCount": 0,
            "observerHook": false,
            "fakeInRunningGames": false,
            "debugGamePresent": false
        }))
        .is_some_and(|verify| cleanup_verify_is_clean(&verify)));
    }

    #[test]
    fn with_bridge_strips_stable_dqh_identifier() {
        let rewritten = with_bridge("window.__dqh_cdp = 1; delete window.__dqh_cdp;");
        assert!(!rewritten.contains("__dqh_cdp"));
        assert!(rewritten.contains(cdp_bridge_name()));
        assert!(cdp_bridge_name().starts_with("__n"));
    }
}

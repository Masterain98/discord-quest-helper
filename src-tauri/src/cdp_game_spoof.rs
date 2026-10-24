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

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use once_cell::sync::Lazy;
use rand::RngExt;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Discord `discord_utils.node` (`sub_a3990` -> `sub_a4120` -> `sub_1350`)
/// binary fingerprint size (146 bytes = `0x92`) and Base64 length (196 chars).
pub const EXECUTABLE_FINGERPRINT_BYTE_LEN: usize = 0x92;
pub const EXECUTABLE_FINGERPRINT_BASE64_LEN: usize = 196;
pub const EXECUTABLE_FINGERPRINT_SALT: &str = "https://discord.com/careers";

/// Format `exe_path` identically to Discord webpack `Module 973522` (`o.Ic`):
/// lowercase, normalize `\` to `/`, strip empty segments, and join the last 2 segments.
pub fn format_quest_executable_path(exe_path: &str) -> String {
    let normalized = exe_path.trim().to_lowercase().replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        parts.first().copied().unwrap_or_default().to_string()
    }
}

/// Compute a 32-byte SHA-256 digest of `data`.
fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Read the local Windows `MachineGuid` from `HKLM\SOFTWARE\Microsoft\Cryptography`
/// (`sub_a3f60` at `0x1800a3f60` in `discord_utils.node`).
pub fn local_machine_guid() -> Option<String> {
    #[cfg(windows)]
    {
        let guid = windows_registry::LOCAL_MACHINE
            .open("SOFTWARE\\Microsoft\\Cryptography")
            .and_then(|key| key.get_string("MachineGuid"))
            .unwrap_or_default();
        let trimmed = guid.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Derive the 32-byte hardware + user fingerprint (`sub_a3f60`:
/// `SHA-256(MachineGuid || user_id)` on Windows; `SHA-256("hw:OS:ARCH:cores:user_id")` fallback).
pub fn local_machine_hw_sha256(user_id: &str) -> [u8; 32] {
    if let Some(guid) = local_machine_guid() {
        return sha256_bytes(format!("{guid}{user_id}").as_bytes());
    }
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    sha256_bytes(
        format!(
            "hw:{}:{}:{}:{user_id}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            cores
        )
        .as_bytes(),
    )
}

/// Synthesize a valid 196-character Base64 `executableFingerprint` matching
/// `discord_utils.node` (`sub_a3990` -> `sub_a4120` -> `sub_1350`).
///
/// Binary layout (`0x92` = 146 bytes before XOR):
/// - `[0x00]`: `0x01` (struct version, un-XORed in `sub_1350`)
/// - `[0x01]`: `0x03` (`is_signed (0x01) | is_pe64 (0x02)`) or `0x01` (`is_signed` only)
/// - `[0x02..0x09]`: `file_size` (`u64 LE`, realistic 48..140 MB signed game PE size)
/// - `[0x0A..0x0D]`: `time_date_stamp` (`u32 LE`, PE COFF compile timestamp)
/// - `[0x0E..0x11]`: `size_of_image` (`u32 LE`, page-aligned PE `SizeOfImage`)
/// - `[0x12..0x31]`: `pe_headers_sha256` (32 bytes)
/// - `[0x32..0x51]`: `hw_sha256` (32 bytes, `SHA-256(MachineGuid || userId)`)
/// - `[0x52..0x71]`: `sections_sha256` (32 bytes)
/// - `[0x72..0x91]`: `cert_sha256` (32 bytes, `SHA-256(Publisher_CN || SPKI_SHA256)`)
///
/// Stream cipher (`sub_1350`): byte `[0x00]` is kept plaintext (`0x01`), while bytes `[0x01..0x91]`
/// are XORed with `xor_key[(i - 1) & 31]` where `xor_key = SHA256(user_id + "https://discord.com/careers")`.
pub fn synthesize_signed_executable_fingerprint(
    user_id: &str,
    app_id: &str,
    exe_path: &str,
    harvested_hw: Option<&[u8; 32]>,
    is_pe64: bool,
) -> String {
    let norm_path = format_quest_executable_path(exe_path);
    let seed = format!("{app_id}:{norm_path}");
    let seed_hash = sha256_bytes(format!("seed:{seed}").as_bytes());

    let base_mb =
        48u64 + (u32::from_le_bytes(seed_hash[0..4].try_into().unwrap_or([0; 4])) as u64 % 92);
    let extra_bytes =
        (u32::from_le_bytes(seed_hash[4..8].try_into().unwrap_or([0; 4])) as u64 % 1024) * 512;
    let file_size: u64 = base_mb * 1024 * 1024 + extra_bytes;

    let time_date_stamp: u32 = 1_704_067_200u32.wrapping_add(
        u32::from_le_bytes(seed_hash[8..12].try_into().unwrap_or([0; 4])) % 55_000_000,
    );
    let pages: u32 = 0x3800u32
        + (u16::from_le_bytes(seed_hash[12..14].try_into().unwrap_or([0; 2])) as u32 % 0x4000);
    let size_of_image: u32 = pages.wrapping_mul(0x1000);

    let h_headers =
        sha256_bytes(format!("PE_HEADERS_V1:{seed}:{time_date_stamp}:{size_of_image}").as_bytes());
    let h_hw = local_machine_hw_sha256(user_id);
    let h_sections = sha256_bytes(format!("PE_SECTIONS_V1:{seed}:{size_of_image}").as_bytes());
    let h_cert = sha256_bytes(format!("AUTHENTICODE_CERT_V1:{seed}").as_bytes());

    let mut plain = [0u8; EXECUTABLE_FINGERPRINT_BYTE_LEN];
    plain[0x00] = 0x01; // version = 1 (un-XORed in sub_1350)
    plain[0x01] = if is_pe64 { 0x03 } else { 0x01 };
    plain[0x02..0x0A].copy_from_slice(&file_size.to_le_bytes());
    plain[0x0A..0x0E].copy_from_slice(&time_date_stamp.to_le_bytes());
    plain[0x0E..0x12].copy_from_slice(&size_of_image.to_le_bytes());
    plain[0x12..0x32].copy_from_slice(&h_headers);
    if let Some(hw) = harvested_hw {
        plain[0x32..0x52].copy_from_slice(hw);
    } else {
        plain[0x32..0x52].copy_from_slice(&h_hw);
    }
    plain[0x52..0x72].copy_from_slice(&h_sections);
    plain[0x72..0x92].copy_from_slice(&h_cert);

    let xor_key = sha256_bytes(format!("{user_id}{EXECUTABLE_FINGERPRINT_SALT}").as_bytes());
    let mut cipher = [0u8; EXECUTABLE_FINGERPRINT_BYTE_LEN];
    cipher[0] = plain[0];
    for i in 1..EXECUTABLE_FINGERPRINT_BYTE_LEN {
        cipher[i] = plain[i] ^ xor_key[(i - 1) & 31];
    }
    BASE64.encode(cipher)
}

/// Decrypt a 196-character Base64 `executableFingerprint` back into its raw 146-byte (`0x92`) struct.
#[cfg_attr(not(test), allow(dead_code))]
pub fn decrypt_executable_fingerprint(
    fingerprint_b64: &str,
    user_id: &str,
) -> Option<[u8; EXECUTABLE_FINGERPRINT_BYTE_LEN]> {
    let trimmed = fingerprint_b64.trim();
    if trimmed.len() != EXECUTABLE_FINGERPRINT_BASE64_LEN {
        return None;
    }
    let raw = BASE64.decode(trimmed).ok()?;
    if raw.len() != EXECUTABLE_FINGERPRINT_BYTE_LEN || raw[0] != 0x01 {
        return None;
    }
    let xor_key = sha256_bytes(format!("{user_id}{EXECUTABLE_FINGERPRINT_SALT}").as_bytes());
    let mut plain = [0u8; EXECUTABLE_FINGERPRINT_BYTE_LEN];
    plain[0] = raw[0];
    for i in 1..EXECUTABLE_FINGERPRINT_BYTE_LEN {
        plain[i] = raw[i] ^ xor_key[(i - 1) & 31];
    }
    Some(plain)
}

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

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct ApplicationExecutable {
    pub os: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeGamePathTemplates {
    pub cmd_line: &'static str,
    pub exe_path: &'static str,
}

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

/// Sanitize an application display name for filesystem path construction,
/// mirroring `sanitizeApp` in `cdp_quest.rs`.
pub fn sanitize_app_name(app_name: &str) -> String {
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

/// Normalize a raw executable name from Discord's detectable catalog for `os`,
/// mirroring `normalizeExe` in `cdp_quest.rs`.
pub fn normalize_exe_name(os: DetectableOs, raw: &str) -> String {
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

/// Convert a sanitized application name into a hyphenated lowercase slug,
/// mirroring `appSlug` in `cdp_quest.rs`.
pub fn app_slug(app: &str) -> String {
    app.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// Render a path/command template with `{app}`, `{app_lower}`, `{app_slug}`, and `{exe}`,
/// mirroring `render` in `cdp_quest.rs`.
pub fn render_template(template: &str, app: &str, exe: &str) -> String {
    template
        .replace("{app}", app)
        .replace("{app_lower}", &app.to_lowercase())
        .replace("{app_slug}", &app_slug(app))
        .replace("{exe}", exe)
}

/// Build realistic OS-specific `cmd_line`, `exe_path`, and `exe_name` values for a spoofed game.
pub fn build_fake_game_paths(os: DetectableOs, app_name: &str, exe_name: &str) -> FakeGamePaths {
    let app = sanitize_app_name(app_name);
    let exe = normalize_exe_name(os, exe_name);
    let templates = path_templates(os);
    FakeGamePaths {
        cmd_line: render_template(templates.cmd_line, &app, &exe),
        exe_path: render_template(templates.exe_path, &app, &exe),
        exe_name: exe,
    }
}

/// Select the highest-priority executable entry matching `priority`, falling back to the first entry.
pub fn select_executable<'a>(
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

/// Find a simulated process hint matching `exe_name` by name or path basename.
pub fn match_process_hint<'a>(
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

/// Resolve the realistic host-OS executable path for an application from its
/// `/applications/public` metadata (`name` + `executables`) and optional
/// simulated process hints.
pub fn resolve_application_exe_path(
    host_os: DetectableOs,
    app_name: &str,
    executables: &[ApplicationExecutable],
    hints: &[SimulatedProcessHint],
) -> String {
    let sanitized = sanitize_app_name(app_name);
    let default_exe = if host_os.is_unix() {
        sanitized
    } else {
        format!("{sanitized}.exe")
    };
    let selected_name = select_executable(executables, host_os.cdp_executable_os_priority())
        .map(|exe| exe.name.as_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&default_exe);
    let paths = build_fake_game_paths(host_os, app_name, selected_name);
    if let Some(hint) = match_process_hint(hints, &paths.exe_name) {
        if !hint.exe_path.trim().is_empty() {
            return hint.exe_path.clone();
        }
    }
    paths.exe_path
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
    // Require every verification key: a missing or mistyped key must fail
    // parsing (and therefore count as dirty) instead of silently defaulting
    // to a clean value, otherwise schema drift could report a verified clean
    // state while the spoof is still active in Discord.
    let flag = |key: &str| parsed.get(key).and_then(|value| value.as_bool());
    Some(CleanupVerify {
        dqh_present: flag("dqhPresent")?,
        spoof_active: flag("spoofActive")?,
        fake_game_present: flag("fakeGamePresent")?,
        has_dispatch_hook: flag("hasDispatchHook")?,
        broad_patch_count: parsed.get("broadPatchCount").and_then(|v| v.as_u64())?,
        observer_hook: flag("observerHook")?,
        fake_in_running_games: flag("fakeInRunningGames")?,
        debug_game_present: flag("debugGamePresent")?,
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
    fn cleanup_verify_from_json_rejects_missing_keys() {
        let complete = serde_json::json!({
            "success": true,
            "dqhPresent": false,
            "spoofActive": false,
            "fakeGamePresent": false,
            "hasDispatchHook": false,
            "broadPatchCount": 0,
            "observerHook": false,
            "fakeInRunningGames": false,
            "debugGamePresent": false
        });
        assert!(cleanup_verify_from_json(&complete).is_some());
        for key in [
            "dqhPresent",
            "spoofActive",
            "fakeGamePresent",
            "hasDispatchHook",
            "broadPatchCount",
            "observerHook",
            "fakeInRunningGames",
            "debugGamePresent",
        ] {
            let mut missing = complete.clone();
            assert!(missing.as_object_mut().unwrap().remove(key).is_some());
            assert!(
                cleanup_verify_from_json(&missing).is_none(),
                "missing '{}' must fail parsing so schema drift cannot read as clean",
                key
            );
        }
        assert!(cleanup_verify_from_json(&serde_json::json!({ "success": false })).is_none());
    }

    #[test]
    fn with_bridge_strips_stable_dqh_identifier() {
        let rewritten = with_bridge("window.__dqh_cdp = 1; delete window.__dqh_cdp;");
        assert!(!rewritten.contains("__dqh_cdp"));
        assert!(rewritten.contains(cdp_bridge_name()));
        assert!(cdp_bridge_name().starts_with("__n"));
    }

    #[test]
    fn format_quest_executable_path_matches_module_973522() {
        assert_eq!(
            format_quest_executable_path(r"C:\Program Files\Cool Game\Win64\Game.exe"),
            "win64/game.exe"
        );
        assert_eq!(
            format_quest_executable_path("c:/program files/cool game/Game.exe"),
            "cool game/game.exe"
        );
    }

    #[test]
    fn synthesize_and_decrypt_executable_fingerprint_roundtrip() {
        let user_id = "257900031351193600";
        let app_id = "1158877933042143272";
        let exe_path = r"C:\Program Files\Counter-Strike 2\win64\cs2.exe";
        let fp = synthesize_signed_executable_fingerprint(user_id, app_id, exe_path, None, true);
        assert_eq!(fp.len(), EXECUTABLE_FINGERPRINT_BASE64_LEN);

        let raw = BASE64.decode(&fp).expect("base64 decode");
        assert_eq!(raw[0], 0x01, "byte 0 must remain plaintext version 0x01");

        let plain = decrypt_executable_fingerprint(&fp, user_id).expect("decrypt");
        assert_eq!(plain[0x00], 0x01, "version must be 1");
        assert_eq!(plain[0x01], 0x03, "flags must set is_signed | is_pe64");
        let file_size = u64::from_le_bytes(plain[0x02..0x0A].try_into().unwrap());
        assert!(
            (48 * 1024 * 1024..=141 * 1024 * 1024).contains(&file_size),
            "unexpected file_size: {file_size}"
        );
        let time_date_stamp = u32::from_le_bytes(plain[0x0A..0x0E].try_into().unwrap());
        assert!(time_date_stamp >= 1_704_067_200);
        let size_of_image = u32::from_le_bytes(plain[0x0E..0x12].try_into().unwrap());
        assert_eq!(
            size_of_image & 0xFFF,
            0,
            "size_of_image must be page-aligned"
        );
        assert!(plain[0x12..0x32].iter().any(|&b| b != 0));
        assert!(plain[0x32..0x52].iter().any(|&b| b != 0));
        assert!(plain[0x52..0x72].iter().any(|&b| b != 0));
        assert!(plain[0x72..0x92].iter().any(|&b| b != 0));

        let fp_pe32 =
            synthesize_signed_executable_fingerprint(user_id, app_id, exe_path, None, false);
        let plain_pe32 = decrypt_executable_fingerprint(&fp_pe32, user_id).expect("decrypt pe32");
        assert_eq!(plain_pe32[0x01], 0x01, "flags must set is_signed only");
    }

    #[test]
    fn synthesize_uses_harvested_hw_tail() {
        let hw = [0xABu8; 32];
        let fp = synthesize_signed_executable_fingerprint("1", "2", "a/b.exe", Some(&hw), true);
        let plain = decrypt_executable_fingerprint(&fp, "1").expect("decrypt");
        assert_eq!(&plain[0x32..0x52], &hw[..]);
    }

    #[test]
    fn decrypt_rejects_malformed_input() {
        assert!(decrypt_executable_fingerprint("not base64!", "1").is_none());
        assert!(decrypt_executable_fingerprint("AAAA", "1").is_none());
        let bad_b64 = "!".repeat(EXECUTABLE_FINGERPRINT_BASE64_LEN);
        assert!(decrypt_executable_fingerprint(&bad_b64, "1").is_none());
    }
}

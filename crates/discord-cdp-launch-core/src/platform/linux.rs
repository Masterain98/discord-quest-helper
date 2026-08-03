//! Linux platform backend for the Discord CDP launcher.
//!
//! Linux support is intentionally limited to native installs (official DEB /
//! tar.gz) in the first release. Flatpak and Snap wrappers are a later phase and
//! must not change the shared `DiscordInstall` model here.

use crate::launcher::build_launch_args;
use crate::{DiscordChannel, DiscordInstall, LaunchError, LinuxDesktopProxySettings};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Native install layout for a single Discord channel.
struct NativeInstallSpec {
    channel: DiscordChannel,
    /// Executable file names probed directly inside a bin-style directory or a
    /// `$PATH` entry (e.g. `/usr/bin/discord`).
    bin_names: &'static [&'static str],
    /// Executable locations relative to an "opt"-style root (e.g.
    /// `/opt/Discord/Discord`).
    opt_subpaths: &'static [&'static str],
    /// Names used to match a running process by `argv[0]` basename or `comm`.
    process_names: &'static [&'static str],
}

fn channel_specs() -> [NativeInstallSpec; 3] {
    [
        NativeInstallSpec {
            channel: DiscordChannel::Stable,
            bin_names: &["discord", "Discord"],
            opt_subpaths: &["Discord/Discord", "discord/Discord"],
            process_names: &["Discord", "discord"],
        },
        NativeInstallSpec {
            channel: DiscordChannel::Ptb,
            bin_names: &["discord-ptb", "DiscordPTB"],
            opt_subpaths: &["DiscordPTB/DiscordPTB", "discord-ptb/DiscordPTB"],
            process_names: &["DiscordPTB", "discord-ptb"],
        },
        NativeInstallSpec {
            channel: DiscordChannel::Canary,
            bin_names: &["discord-canary", "DiscordCanary"],
            opt_subpaths: &[
                "DiscordCanary/DiscordCanary",
                "discord-canary/DiscordCanary",
            ],
            process_names: &["DiscordCanary", "discord-canary"],
        },
    ]
}

pub(crate) fn find_installs() -> Result<Vec<DiscordInstall>, LaunchError> {
    let system_roots = [
        PathBuf::from("/usr/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt"),
        PathBuf::from("/usr/share"),
    ];
    let mut user_roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        user_roots.push(home.join(".local").join("bin"));
        user_roots.push(home.join(".local").join("opt"));
        user_roots.push(home.join(".local").join("share"));
    }
    let path_dirs = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();

    Ok(discover_linux_installs_in(
        &system_roots,
        &user_roots,
        &path_dirs,
    ))
}

/// Discover native Discord installs under the given roots.
///
/// Exposed (hidden) so unit tests can inject temporary directories instead of
/// depending on a real Discord install on the CI runner. `bin_names` are probed
/// directly under every root and every `$PATH` entry; `opt_subpaths` are probed
/// under every root. Results are canonicalized, de-duplicated across channels,
/// and returned in Stable, PTB, Canary order.
#[doc(hidden)]
pub fn discover_linux_installs_in(
    system_roots: &[PathBuf],
    user_roots: &[PathBuf],
    path_dirs: &[PathBuf],
) -> Vec<DiscordInstall> {
    let mut installs = Vec::new();
    let mut used: Vec<PathBuf> = Vec::new();

    for spec in channel_specs() {
        let mut candidates: Vec<PathBuf> = Vec::new();
        for root in system_roots.iter().chain(user_roots.iter()) {
            for name in spec.bin_names {
                candidates.push(root.join(name));
            }
            for sub in spec.opt_subpaths {
                candidates.push(root.join(sub));
            }
        }
        for dir in path_dirs {
            for name in spec.bin_names {
                candidates.push(dir.join(name));
            }
        }

        if let Some(executable_path) = candidates.into_iter().find_map(resolve_executable) {
            if used.contains(&executable_path) {
                continue;
            }
            let working_dir = executable_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| executable_path.clone());
            used.push(executable_path.clone());
            installs.push(DiscordInstall {
                channel: spec.channel,
                executable_path,
                working_dir,
            });
        }
    }

    installs
}

/// Return the canonicalized path if `candidate` resolves to an executable
/// regular file (following symlinks), otherwise `None`.
fn resolve_executable(candidate: PathBuf) -> Option<PathBuf> {
    let metadata = std::fs::metadata(&candidate).ok()?; // follows symlinks
    if !metadata.is_file() {
        return None;
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return None;
    }
    Some(std::fs::canonicalize(&candidate).unwrap_or(candidate))
}

/// Snapshot of a `/proc/<pid>` entry used for Discord process matching.
#[derive(Debug, Clone, Default)]
pub struct LinuxProcessInfo {
    pub pid: u32,
    pub executable: Option<PathBuf>,
    pub comm: Option<String>,
    pub cmdline: Vec<OsString>,
}

fn read_process_info(pid: u32) -> LinuxProcessInfo {
    let base = PathBuf::from("/proc").join(pid.to_string());
    let executable = std::fs::read_link(base.join("exe")).ok();
    let comm = std::fs::read_to_string(base.join("comm"))
        .ok()
        .map(|value| value.trim_end_matches('\n').to_string());
    let cmdline = std::fs::read(base.join("cmdline"))
        .ok()
        .map(|bytes| parse_cmdline(&bytes))
        .unwrap_or_default();
    LinuxProcessInfo {
        pid,
        executable,
        comm,
        cmdline,
    }
}

fn parse_cmdline(bytes: &[u8]) -> Vec<OsString> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| OsStr::from_bytes(part).to_os_string())
        .collect()
}

fn enumerate_processes() -> Result<Vec<LinuxProcessInfo>, LaunchError> {
    let entries = std::fs::read_dir("/proc").map_err(|source| LaunchError::ProcessInspection {
        operation: "read /proc",
        source,
    })?;
    let mut processes = Vec::new();
    for entry in entries.flatten() {
        if let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        {
            processes.push(read_process_info(pid));
        }
    }
    Ok(processes)
}

/// Classify a process as a Discord channel, or `None` if it is not Discord.
///
/// Matching priority mirrors the design: canonical executable path > known
/// install path > `argv[0]` basename > `comm`. Processes belonging to DQH or the
/// CDP launcher themselves are never matched, and `comm` is treated as the
/// weakest signal because Linux truncates it to 15 bytes.
pub fn classify_linux_process(
    process: &LinuxProcessInfo,
    installs: &[DiscordInstall],
) -> Option<DiscordChannel> {
    // Never match our own binaries (main app, runner sidecar, CDP launcher).
    if let Some(name) = process
        .executable
        .as_deref()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
    {
        if is_self_binary(name) {
            return None;
        }
    }
    if let Some(name) = process
        .cmdline
        .first()
        .map(Path::new)
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
    {
        if is_self_binary(name) {
            return None;
        }
    }

    // 1 & 2: executable path match against discovered installs.
    if let Some(exe) = process.executable.as_deref() {
        let canonical = std::fs::canonicalize(exe).ok();
        for install in installs {
            if exe == install.executable_path
                || canonical.as_deref() == Some(install.executable_path.as_path())
            {
                return Some(install.channel);
            }
        }
    }

    // 3: argv[0] basename exact match.
    if let Some(name) = process
        .cmdline
        .first()
        .map(Path::new)
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
    {
        if let Some(channel) = match_channel_by_name(name) {
            return Some(channel);
        }
    }

    // 4: comm exact match (weakest; may be truncated to 15 bytes).
    if let Some(comm) = process.comm.as_deref() {
        if let Some(channel) = match_channel_by_name(comm) {
            return Some(channel);
        }
    }

    None
}

fn is_self_binary(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("discord-quest-helper")
        || lower.contains("discord-quest-runner")
        || lower.contains("discord-cdp-launcher")
}

/// Exact (case-insensitive) match of a process name against a channel's known
/// names. Exact comparison avoids the generic `discord` name capturing PTB or
/// Canary processes.
fn match_channel_by_name(name: &str) -> Option<DiscordChannel> {
    channel_specs().into_iter().find_map(|spec| {
        spec.process_names
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(name))
            .then_some(spec.channel)
    })
}

fn channel_matches(found: DiscordChannel, wanted: Option<DiscordChannel>) -> bool {
    match wanted {
        Some(wanted) => wanted == found,
        None => true,
    }
}

fn running_discord_pids(channel: Option<DiscordChannel>) -> Result<Vec<u32>, LaunchError> {
    let installs = find_installs()?;
    let self_pid = std::process::id();
    Ok(enumerate_processes()?
        .into_iter()
        .filter(|process| process.pid != self_pid)
        .filter(|process| {
            classify_linux_process(process, &installs)
                .is_some_and(|found| channel_matches(found, channel))
        })
        .map(|process| process.pid)
        .collect())
}

pub(crate) fn is_running(channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
    Ok(!running_discord_pids(channel)?.is_empty())
}

/// How long to wait for a graceful exit, and again after escalating to SIGKILL.
const TERMINATE_GRACE: Duration = Duration::from_secs(3);

/// True when `pid` *currently* still looks like the Discord channel we targeted.
///
/// PIDs are reused, and there is a multi-second gap between enumeration and the
/// SIGKILL escalation below, so re-reading `/proc/<pid>` before escalating stops
/// a recycled PID from being killed. Re-classification is a couple of small
/// `/proc` reads, so it is cheap enough to do per signal.
fn still_matches_discord(
    pid: u32,
    channel: Option<DiscordChannel>,
    installs: &[DiscordInstall],
) -> bool {
    classify_linux_process(&read_process_info(pid), installs)
        .is_some_and(|found| channel_matches(found, channel))
}

pub(crate) fn terminate(channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
    let installs = find_installs()?;
    let targets = running_discord_pids(channel)?;

    let mut first_error = None;
    for pid in &targets {
        match signal::kill(Pid::from_raw(*pid as i32), Signal::SIGTERM) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(errno) => {
                first_error.get_or_insert(LaunchError::ProcessTermination {
                    process: pid.to_string(),
                    details: errno.to_string(),
                });
            }
        }
    }
    let deadline = Instant::now() + TERMINATE_GRACE;
    while Instant::now() < deadline {
        if targets.iter().all(|pid| !process_is_alive(*pid)) {
            return first_error.map_or(Ok(()), Err);
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    for pid in &targets {
        if process_is_alive(*pid) && still_matches_discord(*pid, channel, &installs) {
            let _ = signal::kill(Pid::from_raw(*pid as i32), Signal::SIGKILL);
        }
    }

    // SIGKILL only queues the signal — a process in uninterruptible I/O can
    // outlive it. Confirm before reporting success, or the caller relaunches
    // Discord while the old instance still holds the CDP port.
    let deadline = Instant::now() + TERMINATE_GRACE;
    while Instant::now() < deadline {
        if targets
            .iter()
            .all(|pid| !process_is_alive(*pid) || !still_matches_discord(*pid, channel, &installs))
        {
            return first_error.map_or(Ok(()), Err);
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    first_error.map_or_else(
        || {
            Err(LaunchError::ShutdownTimeout {
                timeout: TERMINATE_GRACE,
            })
        },
        Err,
    )
}

/// Liveness check via `kill(pid, 0)`, which performs error checking without
/// delivering a signal.
fn process_is_alive(pid: u32) -> bool {
    signal::kill(Pid::from_raw(pid as i32), None).is_ok()
}

pub(crate) fn spawn(
    install: &DiscordInstall,
    port: u16,
    allow_origins: bool,
) -> Result<u32, LaunchError> {
    let mut command = Command::new(&install.executable_path);
    apply_desktop_proxy_if_missing(&mut command);
    command
        .current_dir(&install.working_dir)
        .args(build_launch_args(port, allow_origins))
        // Discord/Electron is a GUI child process. Inheriting the launcher's
        // terminal floods Tauri dev output with Chromium GPU/shared-surface
        // and preload diagnostics that are unrelated to quest simulation.
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
        .spawn()
        .map(|mut child| {
            let pid = child.id();
            // Keep a reaper alive after this short-lived launch operation
            // returns. Dropping Child leaks a zombie into the long-lived Tauri
            // parent, and kill(pid, 0) would then mistake it for Discord.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            pid
        })
        .map_err(|source| LaunchError::SpawnFailed {
            path: install.executable_path.clone(),
            source,
        })
}

/// Applications launched from GNOME's app grid do not necessarily inherit the
/// proxy variables present in a terminal. Discord's native updater uses those
/// variables but does not read GNOME GSettings itself, which can leave its
/// splash screen retrying forever even though the desktop proxy is configured.
/// Preserve each explicit environment value and only synthesize missing
/// variables from GNOME's manual proxy settings.
fn apply_desktop_proxy_if_missing(command: &mut Command) {
    let Some(settings) = linux_desktop_proxy_settings() else {
        return;
    };

    if proxy_env_missing("HTTP_PROXY", "http_proxy") {
        if let Some(proxy) = settings.http {
            command.env("HTTP_PROXY", &proxy).env("http_proxy", proxy);
        }
    }
    if proxy_env_missing("HTTPS_PROXY", "https_proxy") {
        if let Some(proxy) = settings.https {
            command.env("HTTPS_PROXY", &proxy).env("https_proxy", proxy);
        }
    }
    if proxy_env_missing("ALL_PROXY", "all_proxy") {
        if let Some(proxy) = settings.all {
            command.env("ALL_PROXY", &proxy).env("all_proxy", proxy);
        }
    }

    if proxy_env_missing("NO_PROXY", "no_proxy") {
        if let Some(ignore_hosts) = settings.no_proxy {
            command
                .env("NO_PROXY", &ignore_hosts)
                .env("no_proxy", ignore_hosts);
        }
    }
}

fn proxy_env_missing(upper: &str, lower: &str) -> bool {
    std::env::var_os(upper).is_none() && std::env::var_os(lower).is_none()
}

/// Read the current GNOME-compatible manual proxy configuration.
///
/// This intentionally returns `None` for `none` and `auto` modes. PAC support
/// requires a resolver rather than a single proxy URL and must not be treated
/// as if it were a manual endpoint.
pub fn linux_desktop_proxy_settings() -> Option<LinuxDesktopProxySettings> {
    if gsettings_string("org.gnome.system.proxy", "mode").as_deref() != Some("manual") {
        return None;
    }

    let settings = LinuxDesktopProxySettings {
        http: gsettings_proxy_url("http", "http"),
        https: gsettings_proxy_url("https", "http"),
        all: gsettings_proxy_url("socks", "socks5"),
        no_proxy: gsettings_string_list("org.gnome.system.proxy", "ignore-hosts"),
    };

    settings.has_proxy().then_some(settings)
}

fn gsettings_proxy_url(section: &str, scheme: &str) -> Option<String> {
    let schema = format!("org.gnome.system.proxy.{section}");
    let host = gsettings_string(&schema, "host")?;
    if host.is_empty() {
        return None;
    }
    let port = gsettings_raw(&schema, "port")?.parse::<u16>().ok()?;
    if port == 0 {
        return None;
    }
    Some(format!("{scheme}://{host}:{port}"))
}

fn gsettings_string(schema: &str, key: &str) -> Option<String> {
    let value = gsettings_raw(schema, key)?;
    parse_gvariant_string(&value)
}

fn gsettings_string_list(schema: &str, key: &str) -> Option<String> {
    let value = gsettings_raw(schema, key)?;
    parse_gvariant_string_list(&value)
}

fn parse_gvariant_string_list(value: &str) -> Option<String> {
    let inner = value.trim().strip_prefix('[')?.strip_suffix(']')?;
    let values: Vec<String> = inner
        .split(',')
        .filter_map(|item| parse_gvariant_string(item.trim()))
        .filter(|item| !item.is_empty())
        .collect();
    (!values.is_empty()).then(|| values.join(","))
}

fn gsettings_raw(schema: &str, key: &str) -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_gvariant_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() < 2 {
        return None;
    }
    let quote = value.as_bytes()[0];
    if !matches!(quote, b'\'' | b'"') || value.as_bytes()[value.len() - 1] != quote {
        return None;
    }
    Some(value[1..value.len() - 1].to_string())
}

#[cfg(test)]
mod proxy_setting_tests {
    use super::{parse_gvariant_string, parse_gvariant_string_list, LinuxDesktopProxySettings};

    #[test]
    fn parses_gsettings_strings() {
        assert_eq!(parse_gvariant_string("'manual'"), Some("manual".into()));
        assert_eq!(
            parse_gvariant_string("\"localhost\""),
            Some("localhost".into())
        );
        assert_eq!(parse_gvariant_string("manual"), None);
    }

    #[test]
    fn parses_gsettings_ignore_host_list_for_no_proxy() {
        assert_eq!(
            parse_gvariant_string_list("['localhost', '127.0.0.0/8', '::1']"),
            Some("localhost,127.0.0.0/8,::1".into())
        );
        assert_eq!(parse_gvariant_string_list("[]"), None);
    }

    #[test]
    fn desktop_proxy_requires_at_least_one_endpoint() {
        assert!(!LinuxDesktopProxySettings::default().has_proxy());
        assert!(LinuxDesktopProxySettings {
            https: Some("http://127.0.0.1:10808".into()),
            ..LinuxDesktopProxySettings::default()
        }
        .has_proxy());
    }
}

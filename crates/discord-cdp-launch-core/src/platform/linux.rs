//! Linux platform backend for the Discord CDP launcher.
//!
//! Linux support is intentionally limited to native installs (official DEB /
//! tar.gz) in the first release. Flatpak and Snap wrappers are a later phase and
//! must not change the shared `DiscordInstall` model here.

use crate::launcher::build_launch_args;
use crate::{DiscordChannel, DiscordInstall, LaunchError};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
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

pub(crate) fn terminate(channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
    let targets = running_discord_pids(channel)?;

    for pid in &targets {
        match signal::kill(Pid::from_raw(*pid as i32), Signal::SIGTERM) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(errno) => {
                return Err(LaunchError::ProcessTermination {
                    process: pid.to_string(),
                    details: errno.to_string(),
                });
            }
        }
    }

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if targets.iter().all(|pid| !process_is_alive(*pid)) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    for pid in &targets {
        if process_is_alive(*pid) {
            let _ = signal::kill(Pid::from_raw(*pid as i32), Signal::SIGKILL);
        }
    }

    Ok(())
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
    command
        .current_dir(&install.working_dir)
        .args(build_launch_args(port, allow_origins));
    command
        .spawn()
        .map(|child| child.id())
        .map_err(|source| LaunchError::SpawnFailed {
            path: install.executable_path.clone(),
            source,
        })
}

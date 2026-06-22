use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::cdp_client::{self, DEFAULT_CDP_PORT};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiscordChannel {
    Stable,
    Ptb,
    Canary,
}

impl DiscordChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            DiscordChannel::Stable => "stable",
            DiscordChannel::Ptb => "ptb",
            DiscordChannel::Canary => "canary",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            DiscordChannel::Stable => "Stable",
            DiscordChannel::Ptb => "PTB",
            DiscordChannel::Canary => "Canary",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiscordInstall {
    pub channel: DiscordChannel,
    pub executable_path: PathBuf,
    pub working_dir: PathBuf,
    pub process_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub port: u16,
    pub allow_origins: bool,
    pub channel: Option<DiscordChannel>,
    pub restart_existing: bool,
    pub wait_for_cdp: bool,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            port: DEFAULT_CDP_PORT,
            allow_origins: true,
            channel: None,
            restart_existing: false,
            wait_for_cdp: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchResult {
    pub launched_path: String,
    pub channel: DiscordChannel,
    pub port: u16,
    pub cdp_connected: bool,
}

pub fn parse_discord_channel(value: Option<&str>) -> Result<Option<DiscordChannel>, String> {
    let Some(value) = value else {
        return Ok(None);
    };

    match value.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => Ok(None),
        "stable" | "discord" => Ok(Some(DiscordChannel::Stable)),
        "ptb" | "discordptb" | "discord-ptb" => Ok(Some(DiscordChannel::Ptb)),
        "canary" | "discordcanary" | "discord-canary" => Ok(Some(DiscordChannel::Canary)),
        other => Err(format!("Unsupported Discord channel: {}", other)),
    }
}

pub fn find_discord_installs() -> Vec<DiscordInstall> {
    find_discord_installs_platform()
}

pub fn select_preferred_install(channel: Option<DiscordChannel>) -> Result<DiscordInstall, String> {
    let installs = find_discord_installs();
    if let Some(channel) = channel {
        installs
            .into_iter()
            .find(|install| install.channel == channel)
            .ok_or_else(|| {
                format!(
                    "Could not find Discord {} installation.",
                    channel.display_name()
                )
            })
    } else {
        installs
            .into_iter()
            .next()
            .ok_or_else(|| "Could not find Discord installation.".to_string())
    }
}

pub async fn is_cdp_available(port: u16) -> bool {
    cdp_client::check_cdp_available(port).await.connected
}

pub fn is_discord_running(channel: Option<DiscordChannel>) -> Result<bool, String> {
    is_discord_running_platform(channel)
}

pub fn terminate_discord_processes(channel: Option<DiscordChannel>) -> Result<(), String> {
    terminate_discord_processes_platform(channel)
}

pub async fn launch_discord_with_cdp(options: LaunchOptions) -> Result<LaunchResult, String> {
    let status = cdp_client::check_cdp_available(options.port).await;
    if status.connected {
        let install = select_preferred_install(options.channel)?;
        return Ok(launch_result(&install, options.port, true));
    }

    if options.restart_existing {
        terminate_discord_processes(options.channel)?;
        wait_until_discord_exits(options.channel, Duration::from_secs(8)).await?;
    } else if is_discord_running(options.channel)? {
        return Err(
            "Discord is already running without CDP. Use Restart Discord with CDP to close it and relaunch."
                .to_string(),
        );
    }

    ensure_port_available_for_cdp(options.port, &status).await?;

    let install = select_preferred_install(options.channel)?;
    let mut command = Command::new(&install.executable_path);
    command
        .current_dir(&install.working_dir)
        .arg(format!("--remote-debugging-port={}", options.port));

    if options.allow_origins {
        command.arg("--remote-allow-origins=*");
    }

    let child = command
        .spawn()
        .map_err(|e| format!("Failed to launch Discord with CDP: {}", e))?;

    println!(
        "Launched Discord {} with CDP: path='{}', pid={}, port={}",
        install.channel.display_name(),
        install.executable_path.display(),
        child.id(),
        options.port
    );

    let cdp_connected = if options.wait_for_cdp {
        poll_cdp_connected(options.port, Duration::from_secs(15)).await
    } else {
        false
    };

    if options.wait_for_cdp && !cdp_connected {
        return Err(format!(
            "Discord was launched, but CDP did not become available on port {} within 15 seconds.",
            options.port
        ));
    }

    Ok(launch_result(&install, options.port, cdp_connected))
}

pub async fn restart_discord_with_cdp(mut options: LaunchOptions) -> Result<LaunchResult, String> {
    options.restart_existing = true;
    launch_discord_with_cdp(options).await
}

async fn ensure_port_available_for_cdp(
    port: u16,
    status: &cdp_client::CdpStatus,
) -> Result<(), String> {
    if status.available && !status.connected {
        return Err(format!(
            "CDP port {} is already used by another process or non-Discord CDP target.",
            port
        ));
    }

    if is_tcp_port_open(port) {
        return Err(format!(
            "CDP port {} is already used by another process.",
            port
        ));
    }

    Ok(())
}

fn is_tcp_port_open(port: u16) -> bool {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok()
}

async fn poll_cdp_connected(port: u16, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if cdp_client::check_cdp_available(port).await.connected {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

async fn wait_until_discord_exits(
    channel: Option<DiscordChannel>,
    timeout: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !is_discord_running(channel)? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Err("Discord is running but could not be terminated. Please close Discord manually and try again."
        .to_string())
}

fn launch_result(install: &DiscordInstall, port: u16, cdp_connected: bool) -> LaunchResult {
    LaunchResult {
        launched_path: install.executable_path.to_string_lossy().to_string(),
        channel: install.channel,
        port,
        cdp_connected,
    }
}

fn process_names_for(channel: Option<DiscordChannel>) -> Vec<String> {
    match channel {
        Some(DiscordChannel::Stable) => vec![platform_process_name(DiscordChannel::Stable)],
        Some(DiscordChannel::Ptb) => vec![platform_process_name(DiscordChannel::Ptb)],
        Some(DiscordChannel::Canary) => vec![platform_process_name(DiscordChannel::Canary)],
        None => vec![
            platform_process_name(DiscordChannel::Stable),
            platform_process_name(DiscordChannel::Ptb),
            platform_process_name(DiscordChannel::Canary),
        ],
    }
}

#[cfg(target_os = "windows")]
fn platform_process_name(channel: DiscordChannel) -> String {
    match channel {
        DiscordChannel::Stable => "Discord.exe",
        DiscordChannel::Ptb => "DiscordPTB.exe",
        DiscordChannel::Canary => "DiscordCanary.exe",
    }
    .to_string()
}

#[cfg(target_os = "macos")]
fn platform_process_name(channel: DiscordChannel) -> String {
    match channel {
        DiscordChannel::Stable => "Discord",
        DiscordChannel::Ptb => "Discord PTB",
        DiscordChannel::Canary => "Discord Canary",
    }
    .to_string()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_process_name(_channel: DiscordChannel) -> String {
    String::new()
}

#[cfg(target_os = "windows")]
fn find_discord_installs_platform() -> Vec<DiscordInstall> {
    let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") else {
        return Vec::new();
    };
    let base = PathBuf::from(local_appdata);
    let channels = [
        (
            DiscordChannel::Stable,
            "Discord",
            "Discord.exe",
            vec!["Discord.exe".to_string()],
        ),
        (
            DiscordChannel::Ptb,
            "DiscordPTB",
            "DiscordPTB.exe",
            vec!["DiscordPTB.exe".to_string()],
        ),
        (
            DiscordChannel::Canary,
            "DiscordCanary",
            "DiscordCanary.exe",
            vec!["DiscordCanary.exe".to_string()],
        ),
    ];

    channels
        .into_iter()
        .filter_map(|(channel, folder, exe_name, process_names)| {
            let channel_path = base.join(folder);
            find_windows_channel_executable(&channel_path, exe_name).map(|executable_path| {
                let working_dir = executable_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| channel_path.clone());

                DiscordInstall {
                    channel,
                    executable_path,
                    working_dir,
                    process_names,
                }
            })
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn find_windows_channel_executable(channel_path: &Path, exe_name: &str) -> Option<PathBuf> {
    let mut app_dirs = std::fs::read_dir(channel_path)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(|entry| entry.ok()))
        .filter(|entry| {
            entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("app-")
        })
        .collect::<Vec<_>>();

    app_dirs.sort_by(|a, b| {
        parse_app_version(&b.file_name())
            .cmp(&parse_app_version(&a.file_name()))
            .then_with(|| b.file_name().cmp(&a.file_name()))
    });

    for app_dir in app_dirs {
        let exe_path = app_dir.path().join(exe_name);
        if exe_path.exists() {
            return Some(exe_path);
        }
    }

    let direct_exe = channel_path.join(exe_name);
    direct_exe.exists().then_some(direct_exe)
}

#[cfg(target_os = "windows")]
fn parse_app_version(name: &std::ffi::OsStr) -> Vec<u32> {
    name.to_string_lossy()
        .strip_prefix("app-")
        .unwrap_or("")
        .split('.')
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

#[cfg(target_os = "windows")]
fn no_window_cmd(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[cfg(target_os = "windows")]
fn is_discord_running_platform(channel: Option<DiscordChannel>) -> Result<bool, String> {
    let output = no_window_cmd("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .map_err(|e| format!("Could not execute tasklist: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    let names = process_names_for(channel);
    Ok(names.iter().any(|name| {
        let quoted_name = format!("\"{}\"", name.to_ascii_lowercase());
        stdout.contains(&quoted_name)
    }))
}

#[cfg(target_os = "windows")]
fn terminate_discord_processes_platform(channel: Option<DiscordChannel>) -> Result<(), String> {
    for name in process_names_for(channel) {
        let output = no_window_cmd("taskkill")
            .args(["/IM", &name, "/T", "/F"])
            .output()
            .map_err(|e| format!("Could not execute taskkill for {}: {}", name, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!("taskkill for {} returned non-zero: {}", name, stderr.trim());
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn find_discord_installs_platform() -> Vec<DiscordInstall> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }

    let channels = [
        (
            DiscordChannel::Stable,
            "Discord.app",
            vec!["Discord"],
            vec!["Discord".to_string()],
        ),
        (
            DiscordChannel::Ptb,
            "Discord PTB.app",
            vec!["Discord PTB", "Discord"],
            vec!["Discord PTB".to_string()],
        ),
        (
            DiscordChannel::Canary,
            "Discord Canary.app",
            vec!["Discord Canary", "Discord"],
            vec!["Discord Canary".to_string()],
        ),
    ];

    let mut installs = Vec::new();
    for (channel, app_name, executable_names, process_names) in channels {
        for root in &roots {
            let app_path = root.join(app_name);
            let macos_dir = app_path.join("Contents").join("MacOS");
            for executable_name in &executable_names {
                let executable_path = macos_dir.join(executable_name);
                if executable_path.exists() {
                    installs.push(DiscordInstall {
                        channel,
                        executable_path,
                        working_dir: macos_dir.clone(),
                        process_names: process_names.clone(),
                    });
                    break;
                }
            }
            if installs.iter().any(|install| install.channel == channel) {
                break;
            }
        }
    }

    installs
}

#[cfg(target_os = "macos")]
fn is_discord_running_platform(channel: Option<DiscordChannel>) -> Result<bool, String> {
    for name in process_names_for(channel) {
        let status = Command::new("pgrep")
            .args(["-x", &name])
            .status()
            .map_err(|e| format!("Could not execute pgrep for {}: {}", name, e))?;
        if status.success() {
            return Ok(true);
        }
    }

    Ok(false)
}

#[cfg(target_os = "macos")]
fn terminate_discord_processes_platform(channel: Option<DiscordChannel>) -> Result<(), String> {
    for name in process_names_for(channel) {
        let script = format!("tell application \"{}\" to quit", name.replace('"', "\\\""));
        let _ = Command::new("osascript").args(["-e", &script]).output();
    }

    std::thread::sleep(Duration::from_secs(3));

    for name in process_names_for(channel) {
        if is_discord_running(Some(channel_from_process_name(&name)))? {
            let output = Command::new("pkill")
                .args(["-x", &name])
                .output()
                .map_err(|e| format!("Could not execute pkill for {}: {}", name, e))?;
            if !output.status.success() && output.status.code() != Some(1) {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to terminate {}: {}", name, stderr.trim()));
            }
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn channel_from_process_name(name: &str) -> DiscordChannel {
    match name {
        "Discord PTB" => DiscordChannel::Ptb,
        "Discord Canary" => DiscordChannel::Canary,
        _ => DiscordChannel::Stable,
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn find_discord_installs_platform() -> Vec<DiscordInstall> {
    Vec::new()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn is_discord_running_platform(_channel: Option<DiscordChannel>) -> Result<bool, String> {
    Err("Discord CDP launcher is only supported on Windows and macOS.".to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn terminate_discord_processes_platform(_channel: Option<DiscordChannel>) -> Result<(), String> {
    Err("Discord CDP launcher is only supported on Windows and macOS.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channel_names() {
        assert_eq!(
            parse_discord_channel(Some("stable")).unwrap(),
            Some(DiscordChannel::Stable)
        );
        assert_eq!(
            parse_discord_channel(Some("discord-ptb")).unwrap(),
            Some(DiscordChannel::Ptb)
        );
        assert_eq!(parse_discord_channel(Some("auto")).unwrap(), None);
        assert!(parse_discord_channel(Some("nightly")).is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parses_app_versions_numerically() {
        assert!(
            parse_app_version(std::ffi::OsStr::new("app-1.0.10000"))
                > parse_app_version(std::ffi::OsStr::new("app-1.0.9999"))
        );
    }
}

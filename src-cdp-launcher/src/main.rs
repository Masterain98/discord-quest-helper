#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_CDP_PORT: u16 = 9223;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscordChannel {
    Stable,
    Ptb,
    Canary,
}

impl DiscordChannel {
    fn display_name(self) -> &'static str {
        match self {
            DiscordChannel::Stable => "Stable",
            DiscordChannel::Ptb => "PTB",
            DiscordChannel::Canary => "Canary",
        }
    }
}

#[derive(Debug, Clone)]
struct DiscordInstall {
    channel: DiscordChannel,
    executable_path: PathBuf,
    working_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct LaunchOptions {
    port: u16,
    allow_origins: bool,
    channel: Option<DiscordChannel>,
    restart_existing: bool,
    wait_for_cdp: bool,
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

#[derive(Debug)]
struct LaunchResult {
    launched_path: String,
    channel: DiscordChannel,
    port: u16,
    cdp_connected: bool,
}

#[derive(Debug)]
struct CliOptions {
    port: u16,
    channel: Option<DiscordChannel>,
    restart: bool,
    wait_cdp: bool,
    status: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            port: DEFAULT_CDP_PORT,
            channel: None,
            restart: false,
            wait_cdp: true,
            status: false,
        }
    }
}

struct Strings {
    title: &'static str,
    cdp_already_running: &'static str,
    restart_confirm: &'static str,
}

const EN: Strings = Strings {
    title: "Discord CDP Launcher",
    cdp_already_running: "Discord is already running with CDP mode enabled.",
    restart_confirm: "Discord is already running. Do you want to restart it with CDP mode enabled?",
};

const ZH: Strings = Strings {
    title: "Discord CDP 启动器",
    cdp_already_running: "Discord 已在 CDP 模式下运行。",
    restart_confirm: "Discord 正在运行。是否要重启并启用 CDP 模式？",
};

const ZH_TW: Strings = Strings {
    title: "Discord CDP 啟動器",
    cdp_already_running: "Discord 已在 CDP 模式下執行。",
    restart_confirm: "Discord 正在執行。是否要重新啟動並啟用 CDP 模式？",
};

const JA: Strings = Strings {
    title: "Discord CDP ランチャー",
    cdp_already_running: "Discord は既に CDP モードで実行中です。",
    restart_confirm: "Discord は実行中です。CDP モードを有効にして再起動しますか？",
};

const KO: Strings = Strings {
    title: "Discord CDP 런처",
    cdp_already_running: "Discord가 이미 CDP 모드로 실행 중입니다.",
    restart_confirm: "Discord가 실행 중입니다. CDP 모드를 활성화하여 재시작하시겠습니까?",
};

const RU: Strings = Strings {
    title: "Discord CDP Лаунчер",
    cdp_already_running: "Discord уже запущен в режиме CDP.",
    restart_confirm: "Discord уже запущен. Хотите перезапустить его с включенным CDP?",
};

const ES: Strings = Strings {
    title: "Discord CDP Lanzador",
    cdp_already_running: "Discord ya está ejecutándose con el modo CDP activado.",
    restart_confirm: "Discord ya está ejecutándose. ¿Deseas reiniciarlo con el modo CDP activado?",
};

fn main() {
    #[cfg(target_os = "windows")]
    enable_dpi_awareness();

    let strings = get_strings();

    match run(strings) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            #[cfg(target_os = "windows")]
            show_info_dialog(strings.title, &error);

            #[cfg(not(target_os = "windows"))]
            eprintln!("{}", error);

            std::process::exit(1);
        }
    }
}

fn run(strings: &Strings) -> Result<i32, String> {
    let options = parse_args(std::env::args().skip(1).collect())?;

    if options.status {
        if is_cdp_available(options.port) {
            println!("CDP is available on port {}", options.port);
            return Ok(0);
        }

        eprintln!("CDP is not available on port {}", options.port);
        return Ok(3);
    }

    if is_cdp_available(options.port) {
        #[cfg(target_os = "windows")]
        show_info_dialog(strings.title, strings.cdp_already_running);

        #[cfg(not(target_os = "windows"))]
        println!("{}", strings.cdp_already_running);

        return Ok(0);
    }

    if is_discord_running(options.channel).unwrap_or(false) && !options.restart {
        let want_restart = {
            #[cfg(target_os = "windows")]
            {
                show_confirm_dialog(strings.title, strings.restart_confirm)
            }

            #[cfg(not(target_os = "windows"))]
            {
                eprintln!(
                    "Discord is already running without CDP. Re-run with --restart to close it and relaunch with CDP."
                );
                false
            }
        };

        if !want_restart {
            return Ok(0);
        }

        let result = restart_discord_with_cdp(LaunchOptions {
            port: options.port,
            channel: options.channel,
            restart_existing: true,
            wait_for_cdp: options.wait_cdp,
            ..Default::default()
        })?;
        print_launch_result(&result);
        return Ok(0);
    }

    let launch_options = LaunchOptions {
        port: options.port,
        channel: options.channel,
        restart_existing: options.restart,
        wait_for_cdp: options.wait_cdp,
        ..Default::default()
    };

    let result = if options.restart {
        restart_discord_with_cdp(launch_options)?
    } else {
        launch_discord_with_cdp(launch_options)?
    };

    print_launch_result(&result);
    Ok(0)
}

fn print_launch_result(result: &LaunchResult) {
    println!(
        "Launched Discord {} with CDP on port {}: {}",
        result.channel.display_name(),
        result.port,
        result.launched_path
    );
    if result.cdp_connected {
        println!("CDP is available on port {}", result.port);
    }
}

fn parse_args(args: Vec<String>) -> Result<CliOptions, String> {
    let mut options = CliOptions::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--port" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--port requires a value".to_string())?;
                options.port = value
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid --port value: {}", value))?;
            }
            "--channel" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--channel requires a value".to_string())?;
                options.channel = parse_discord_channel(Some(value))?;
            }
            "--restart" => {
                options.restart = true;
            }
            "--wait-cdp" => {
                options.wait_cdp = true;
            }
            "--no-wait-cdp" => {
                options.wait_cdp = false;
            }
            "--status" => {
                options.status = true;
            }
            unknown => {
                return Err(format!("Unknown argument: {}\n\n{}", unknown, help_text()));
            }
        }

        i += 1;
    }

    Ok(options)
}

fn parse_discord_channel(value: Option<&str>) -> Result<Option<DiscordChannel>, String> {
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

fn launch_discord_with_cdp(options: LaunchOptions) -> Result<LaunchResult, String> {
    if is_cdp_available(options.port) {
        let install = select_preferred_install(options.channel)?;
        return Ok(launch_result(&install, options.port, true));
    }

    if options.restart_existing {
        terminate_discord_processes(options.channel)?;
        wait_until_discord_exits(options.channel, Duration::from_secs(8))?;
    } else if is_discord_running(options.channel)? {
        return Err(
            "Discord is already running without CDP. Use --restart to close it and relaunch with CDP."
                .to_string(),
        );
    }

    ensure_port_available_for_cdp(options.port)?;

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
        .map_err(|error| format!("Failed to launch Discord with CDP: {}", error))?;

    println!(
        "Launched Discord {} with CDP: path='{}', pid={}, port={}",
        install.channel.display_name(),
        install.executable_path.display(),
        child.id(),
        options.port
    );

    let cdp_connected = if options.wait_for_cdp {
        poll_cdp_connected(options.port, Duration::from_secs(15))
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

fn restart_discord_with_cdp(mut options: LaunchOptions) -> Result<LaunchResult, String> {
    options.restart_existing = true;
    launch_discord_with_cdp(options)
}

fn ensure_port_available_for_cdp(port: u16) -> Result<(), String> {
    if is_tcp_port_open(port) {
        return Err(format!(
            "CDP port {} is already used by another process or non-Discord CDP target.",
            port
        ));
    }

    Ok(())
}

fn is_cdp_available(port: u16) -> bool {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
        return false;
    };

    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let request = format!(
        "GET /json HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        port
    );

    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut response = String::new();
    if stream.read_to_string(&mut response).is_err() {
        return false;
    }

    let lower = response.to_ascii_lowercase();

    response.contains("webSocketDebuggerUrl")
        && (lower.contains("discord")
            || lower.contains("discord.com")
            || lower.contains("discordapp"))
}

fn is_tcp_port_open(port: u16) -> bool {
    use std::net::{SocketAddr, TcpStream};

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok()
}

fn poll_cdp_connected(port: u16, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if is_cdp_available(port) {
            return true;
        }

        thread::sleep(Duration::from_millis(500));
    }

    false
}

fn wait_until_discord_exits(
    channel: Option<DiscordChannel>,
    timeout: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !is_discord_running(channel)? {
            return Ok(());
        }

        thread::sleep(Duration::from_millis(500));
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

fn select_preferred_install(channel: Option<DiscordChannel>) -> Result<DiscordInstall, String> {
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

fn find_discord_installs() -> Vec<DiscordInstall> {
    find_discord_installs_platform()
}

fn is_discord_running(channel: Option<DiscordChannel>) -> Result<bool, String> {
    is_discord_running_platform(channel)
}

fn terminate_discord_processes(channel: Option<DiscordChannel>) -> Result<(), String> {
    terminate_discord_processes_platform(channel)
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
        (DiscordChannel::Stable, "Discord", "Discord.exe"),
        (DiscordChannel::Ptb, "DiscordPTB", "DiscordPTB.exe"),
        (DiscordChannel::Canary, "DiscordCanary", "DiscordCanary.exe"),
    ];

    channels
        .into_iter()
        .filter_map(|(channel, folder, exe_name)| {
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
        .map_err(|error| format!("Could not execute tasklist: {}", error))?;

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
            .map_err(|error| format!("Could not execute taskkill for {}: {}", name, error))?;

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
        (DiscordChannel::Stable, "Discord.app", vec!["Discord"]),
        (
            DiscordChannel::Ptb,
            "Discord PTB.app",
            vec!["Discord PTB", "Discord"],
        ),
        (
            DiscordChannel::Canary,
            "Discord Canary.app",
            vec!["Discord Canary", "Discord"],
        ),
    ];

    let mut installs = Vec::new();
    for (channel, app_name, executable_names) in channels {
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
            .map_err(|error| format!("Could not execute pgrep for {}: {}", name, error))?;
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

    thread::sleep(Duration::from_secs(3));

    for name in process_names_for(channel) {
        let output = Command::new("pkill")
            .args(["-x", &name])
            .output()
            .map_err(|error| format!("Could not execute pkill for {}: {}", name, error))?;

        if !output.status.success() && output.status.code() != Some(1) {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to terminate {}: {}", name, stderr.trim()));
        }
    }

    Ok(())
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

#[cfg(target_os = "windows")]
mod win32 {
    //! Hand-written Win32 FFI bindings — avoids pulling in the `windows` crate
    //! for just three simple API calls.

    // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
    pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;

    pub const MB_OK: u32 = 0x0000_0000;
    pub const MB_YESNO: u32 = 0x0000_0004;
    pub const MB_ICONQUESTION: u32 = 0x0000_0020;
    pub const MB_ICONINFORMATION: u32 = 0x0000_0040;
    pub const IDYES: i32 = 6;

    #[link(name = "user32")]
    extern "system" {
        pub fn SetProcessDpiAwarenessContext(value: isize) -> i32;
        pub fn GetUserDefaultUILanguage() -> u16;
        pub fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            r#type: u32,
        ) -> i32;
    }
}

#[cfg(target_os = "windows")]
fn enable_dpi_awareness() {
    unsafe {
        let _ = win32::SetProcessDpiAwarenessContext(
            win32::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
}

#[cfg(target_os = "windows")]
fn get_system_lang_id() -> u16 {
    unsafe { win32::GetUserDefaultUILanguage() & 0x3FF }
}

fn get_strings() -> &'static Strings {
    #[cfg(target_os = "windows")]
    {
        let lang_id = get_system_lang_id();
        let full_id = unsafe { win32::GetUserDefaultUILanguage() };

        match lang_id {
            0x04 => {
                if full_id == 0x0404 {
                    &ZH_TW
                } else {
                    &ZH
                }
            }
            0x11 => &JA,
            0x12 => &KO,
            0x19 => &RU,
            0x0A => &ES,
            _ => &EN,
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        &EN
    }
}

#[cfg(target_os = "windows")]
fn to_wide(value: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(target_os = "windows")]
fn show_info_dialog(title: &str, message: &str) {
    let title_w = to_wide(title);
    let message_w = to_wide(message);

    unsafe {
        win32::MessageBoxW(
            core::ptr::null_mut(),
            message_w.as_ptr(),
            title_w.as_ptr(),
            win32::MB_OK | win32::MB_ICONINFORMATION,
        );
    }
}

#[cfg(target_os = "windows")]
fn show_confirm_dialog(title: &str, message: &str) -> bool {
    let title_w = to_wide(title);
    let message_w = to_wide(message);

    unsafe {
        win32::MessageBoxW(
            core::ptr::null_mut(),
            message_w.as_ptr(),
            title_w.as_ptr(),
            win32::MB_YESNO | win32::MB_ICONQUESTION,
        ) == win32::IDYES
    }
}

fn print_help() {
    println!("{}", help_text());
}

fn help_text() -> &'static str {
    "Usage:
  discord-cdp-launcher --port 9223 --channel auto
  discord-cdp-launcher --port 9223 --channel stable
  discord-cdp-launcher --port 9223 --restart --wait-cdp
  discord-cdp-launcher --status --port 9223

Options:
  --port <port>                 CDP debugging port. Defaults to 9223.
  --channel <auto|stable|ptb|canary>
                                Discord channel to launch. Defaults to auto.
  --restart                     Close the selected Discord client before launching.
  --wait-cdp                    Wait until CDP becomes available. Enabled by default.
  --no-wait-cdp                 Do not wait for CDP readiness.
  --status                      Check whether CDP is already available.
  --help, -h                    Show this help."
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

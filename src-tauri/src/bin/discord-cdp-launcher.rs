#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use discord_quest_helper_lib::discord_cdp_launcher::{self, parse_discord_channel, LaunchOptions};

// ── i18n ────────────────────────────────────────────────────────────────────

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

#[cfg(target_os = "windows")]
fn get_system_lang_id() -> u16 {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    unsafe { GetUserDefaultUILanguage() & 0x3FF }
}

fn get_strings() -> &'static Strings {
    #[cfg(target_os = "windows")]
    {
        // Primary language ID is the low 10 bits of the lang ID
        // 0x04 = Chinese, 0x11 = Japanese, 0x12 = Korean, 0x19 = Russian, 0x0A = Spanish
        let lang_id = get_system_lang_id();
        let full_id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };

        match lang_id {
            0x04 => {
                // Chinese — distinguish Simplified (0x0804) vs Traditional (0x0404)
                if full_id == 0x0404 { &ZH_TW } else { &ZH }
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

// ── Windows helpers ─────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn enable_dpi_awareness() {
    use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;
    use windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

#[cfg(target_os = "windows")]
fn show_info_dialog(title: &str, message: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    use windows::core::PCWSTR;

    let title_w = to_wide(title);
    let message_w = to_wide(message);

    unsafe {
        MessageBoxW(None, PCWSTR(message_w.as_ptr()), PCWSTR(title_w.as_ptr()), MB_OK | MB_ICONINFORMATION);
    }
}

#[cfg(target_os = "windows")]
fn show_confirm_dialog(title: &str, message: &str) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONQUESTION, MB_YESNO, IDYES};
    use windows::core::PCWSTR;

    let title_w = to_wide(title);
    let message_w = to_wide(message);

    unsafe { MessageBoxW(None, PCWSTR(message_w.as_ptr()), PCWSTR(title_w.as_ptr()), MB_YESNO | MB_ICONQUESTION) == IDYES }
}

// ── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct CliOptions {
    port: u16,
    channel: Option<discord_cdp_launcher::DiscordChannel>,
    restart: bool,
    wait_cdp: bool,
    status: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            port: LaunchOptions::default().port,
            channel: None,
            restart: false,
            wait_cdp: true,
            status: false,
        }
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    #[cfg(target_os = "windows")]
    enable_dpi_awareness();

    let strings = get_strings();

    match run(strings).await {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            #[cfg(target_os = "windows")]
            show_info_dialog(strings.title, &err);
            #[cfg(not(target_os = "windows"))]
            eprintln!("{}", err);
            std::process::exit(1);
        }
    }
}

async fn run(strings: &Strings) -> Result<i32, String> {
    let options = parse_args(std::env::args().skip(1).collect())?;

    if options.status {
        let connected = discord_cdp_launcher::is_cdp_available(options.port).await;
        if connected {
            println!("CDP is available on port {}", options.port);
            return Ok(0);
        }
        eprintln!("CDP is not available on port {}", options.port);
        return Ok(3);
    }

    // Check if CDP is already available
    let cdp_available = discord_cdp_launcher::is_cdp_available(options.port).await;
    if cdp_available {
        #[cfg(target_os = "windows")]
        show_info_dialog(strings.title, strings.cdp_already_running);
        return Ok(0);
    }

    // Check if Discord is running
    let discord_running = discord_cdp_launcher::is_discord_running(options.channel)
        .unwrap_or(false);

    // If Discord is running, decide whether to restart
    if discord_running && !options.restart {
        let want_restart = {
            #[cfg(target_os = "windows")]
            { show_confirm_dialog(strings.title, strings.restart_confirm) }
            #[cfg(not(target_os = "windows"))]
            { false }
        };

        if !want_restart {
            return Ok(0);
        }

        // User confirmed restart — use the same path as the in-app restart
        let launch_options = LaunchOptions {
            port: options.port,
            channel: options.channel,
            restart_existing: true,
            wait_for_cdp: options.wait_cdp,
            ..Default::default()
        };
        let result = discord_cdp_launcher::restart_discord_with_cdp(launch_options).await?;
        println!(
            "Launched Discord {} with CDP on port {}: {}",
            result.channel.display_name(),
            result.port,
            result.launched_path
        );
        return Ok(0);
    }

    // Discord not running or --restart flag passed
    let launch_options = LaunchOptions {
        port: options.port,
        channel: options.channel,
        restart_existing: options.restart,
        wait_for_cdp: options.wait_cdp,
        ..Default::default()
    };

    let result = if options.restart {
        discord_cdp_launcher::restart_discord_with_cdp(launch_options).await?
    } else {
        discord_cdp_launcher::launch_discord_with_cdp(launch_options).await?
    };

    println!(
        "Launched Discord {} with CDP on port {}: {}",
        result.channel.display_name(),
        result.port,
        result.launched_path
    );
    Ok(0)
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
  --help                        Show this help."
}

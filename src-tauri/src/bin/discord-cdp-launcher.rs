#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use discord_quest_helper_lib::discord_cdp_launcher::{self, parse_discord_channel, LaunchOptions};

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

#[tokio::main]
async fn main() {
    match run().await {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("{}", err);
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<i32, String> {
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

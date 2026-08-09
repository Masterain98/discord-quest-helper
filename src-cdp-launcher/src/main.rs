#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod cli;
mod dialogs;
mod locale;

use cli::{help_text, parse_args};
use discord_cdp_launch_core as cdp_launch;
use locale::Strings;

fn main() {
    dialogs::enable_dpi_awareness();
    let strings = locale::get_strings();

    match run(strings) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            dialogs::show_error_dialog(strings.title, &error);

            std::process::exit(1);
        }
    }
}

fn run(strings: &Strings) -> Result<i32, String> {
    let mut options = parse_args(std::env::args().skip(1).collect())?;
    if options.help {
        println!("{}", help_text());
        return Ok(0);
    }

    if options.restore_normal_all {
        let result =
            cdp_launch::restore_all_discord_to_normal().map_err(|error| error.to_string())?;
        if result.failures.is_empty() {
            return Ok(0);
        }
        let details = result
            .failures
            .iter()
            .map(|failure| format!("{}: {}", failure.channel.display_name(), failure.error))
            .collect::<Vec<_>>()
            .join("\n");
        dialogs::show_error_dialog(
            strings.title,
            &format!("{}\n\n{details}", strings.restore_failure),
        );
        return Ok(4);
    }

    if options.status {
        if cdp_launch::is_cdp_available(options.port) {
            println!("CDP is available on port {}", options.port);
            return Ok(0);
        }
        eprintln!("CDP is not available on port {}", options.port);
        return Ok(3);
    }

    if cdp_launch::is_cdp_available(options.port) {
        dialogs::show_info_dialog(strings.title, strings.cdp_already_running);

        return Ok(0);
    }

    let running =
        cdp_launch::is_discord_running(options.channel).map_err(|error| error.to_string())?;
    if running && !options.restart {
        let want_restart = {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            {
                dialogs::show_confirm_dialog(strings.title, strings.restart_confirm)?
            }

            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
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
        options.restart = true;
    }

    let request = cdp_launch::LaunchOptions {
        port: options.port,
        channel: options.channel,
        restart_existing: options.restart,
        ..Default::default()
    };
    let result = if options.restart {
        cdp_launch::restart_discord_with_cdp(request)
    } else {
        cdp_launch::launch_discord_with_cdp(request)
    }
    .map_err(|error| error.to_string())?;

    println!(
        "Launched Discord {} with CDP on port {}: {}",
        result.channel.display_name(),
        result.port,
        result.launched_path.display()
    );
    Ok(0)
}

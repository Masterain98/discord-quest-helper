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

#[cfg(any(target_os = "linux", test))]
const RUNTIME_PROCESS_NAME: &str = "waybridge";

#[cfg(any(target_os = "linux", test))]
fn valid_runtime_process_name(name: &str) -> bool {
    (6..=14).contains(&name.len()) && name.bytes().all(|byte| byte.is_ascii_lowercase())
}

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
    apply_runtime_process_name()?;
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

#[cfg(target_os = "linux")]
fn apply_runtime_process_name() -> Result<(), String> {
    use std::ffi::CString;

    if std::env::var_os("RUNTIME_IDENTITY_MODE").as_deref() == Some(std::ffi::OsStr::new("off")) {
        return Ok(());
    }
    if !valid_runtime_process_name(RUNTIME_PROCESS_NAME) {
        return Err("Launcher runtime identity is invalid".into());
    }
    let name =
        CString::new(RUNTIME_PROCESS_NAME).map_err(|_| "Launcher runtime identity is invalid")?;
    // SAFETY: PR_SET_NAME reads the valid NUL-terminated string while it is
    // alive and ignores the remaining variadic arguments.
    let result = unsafe { libc::prctl(libc::PR_SET_NAME, name.as_ptr(), 0, 0, 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "Launcher runtime identity could not be prepared: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_runtime_process_name() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod runtime_identity_tests {
    use super::*;

    #[test]
    fn launcher_process_name_fits_linux_comm_without_truncation() {
        assert!(valid_runtime_process_name(RUNTIME_PROCESS_NAME));
        assert!(RUNTIME_PROCESS_NAME.len() <= 15);
    }
}

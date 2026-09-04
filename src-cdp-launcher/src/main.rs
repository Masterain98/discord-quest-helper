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
    if let Err(error) = apply_runtime_process_name() {
        eprintln!("Runtime process identity warning: {error}");
    }
    let mut options = parse_args(std::env::args().skip(1).collect())?;
    if options.help {
        println!("{}", help_text());
        return Ok(0);
    }

    if options.restore_normal_all {
        let result = cdp_launch::restore_all_discord_to_normal()
            .map_err(|error| runtime_error(strings.restore_action, &error))?;
        if result.failures.is_empty() {
            return Ok(0);
        }
        let details = if cfg!(debug_assertions) {
            result
                .failures
                .iter()
                .map(|failure| format!("{}: {}", failure.channel.display_name(), failure.error))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            strings.restore_retry.to_string()
        };
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

    let installation = options
        .installation
        .clone()
        .map(|path| {
            let provider = match options.client {
                cdp_launch::DesktopClientPreference::Vesktop => cdp_launch::ProviderId::vesktop(),
                cdp_launch::DesktopClientPreference::Official => {
                    cdp_launch::ProviderId::official_discord()
                }
                cdp_launch::DesktopClientPreference::Auto => {
                    return Err(
                        "--installation requires --client official or --client vesktop".to_string(),
                    );
                }
            };
            cdp_launch::custom_executable_installation(&provider, path)
                .map_err(|error| error.to_string())
        })
        .transpose()?;
    let running = if let Some(installation) = &installation {
        match &installation.launch_target {
            cdp_launch::LaunchTarget::Executable { path, .. }
            | cdp_launch::LaunchTarget::MacBundle {
                executable_path: path,
                ..
            } => cdp_launch::is_installation_running(path),
            cdp_launch::LaunchTarget::Flatpak { .. } => false,
        }
    } else if options.client == cdp_launch::DesktopClientPreference::Vesktop {
        cdp_launch::is_vesktop_running()
            .map_err(|error| runtime_error(strings.status_action, &error))?
    } else {
        cdp_launch::is_discord_running(options.channel)
            .map_err(|error| runtime_error(strings.status_action, &error))?
    };
    if running && !options.restart {
        let want_restart = {
            #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
            {
                dialogs::show_confirm_dialog(strings.title, strings.restart_confirm)?
            }

            #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
            {
                eprintln!("{}", strings.restart_instruction);
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
        client: options.client,
        installation,
        restart_existing: options.restart,
        ..Default::default()
    };
    let result = if options.restart {
        cdp_launch::restart_discord_with_cdp(request)
    } else {
        cdp_launch::launch_discord_with_cdp(request)
    }
    .map_err(|error| runtime_error(strings.launch_action, &error))?;

    println!(
        "{}",
        strings
            .launch_success
            .replace("{channel}", result.channel.display_name())
            .replace("{port}", &result.port.to_string())
    );
    Ok(0)
}

fn runtime_error(action: &str, error: &impl std::fmt::Display) -> String {
    if cfg!(debug_assertions) {
        format!("{action} failed: {error}")
    } else {
        format!("{action} failed. Please fully quit Discord and try again.")
    }
}

#[cfg(target_os = "linux")]
fn apply_runtime_process_name() -> Result<(), String> {
    use std::ffi::CString;

    if runtime_identity_override_enabled(
        cfg!(debug_assertions),
        std::env::var_os("RUNTIME_IDENTITY_MODE").as_deref(),
    ) {
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

#[cfg(any(target_os = "linux", test))]
fn runtime_identity_override_enabled(debug_build: bool, mode: Option<&std::ffi::OsStr>) -> bool {
    debug_build && mode == Some(std::ffi::OsStr::new("off"))
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

    #[test]
    fn release_build_ignores_runtime_identity_off_environment() {
        let off = std::ffi::OsStr::new("off");
        assert!(runtime_identity_override_enabled(true, Some(off)));
        assert!(!runtime_identity_override_enabled(false, Some(off)));
    }
}

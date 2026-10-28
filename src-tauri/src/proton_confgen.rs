#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
use std::fs;
#[cfg(windows)]
use std::io::Write;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::process::{Command, Stdio};
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use sha2::{Digest, Sha256};
#[cfg(windows)]
use zeroize::Zeroizing;

use crate::live_tunnel::LiveTunnelStatus;

const DEFAULT_COUNTRIES: &str = "US,NL,JP";
const MAX_COUNTRIES: usize = 10;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const PROTON_CONFGEN: &[u8] = include_bytes!("../binaries/proton-confgen.exe");
#[cfg(windows)]
const PROTON_CONFGEN_SHA256: &str =
    "75f81e24a9e4ef4f3bc390f00084b037a575027f6e1b834c767546c3a83fe581";

fn normalize_countries(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(DEFAULT_COUNTRIES.to_string());
    }

    let mut codes: Vec<String> = Vec::new();
    for part in trimmed.split(',') {
        let code = part.trim().to_ascii_uppercase();
        if code.len() != 2 || !code.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return Err("Use two-letter country codes separated by commas.".to_string());
        }
        if !codes.iter().any(|existing| existing == &code) {
            codes.push(code);
        }
        if codes.len() > MAX_COUNTRIES {
            return Err("Choose at most 10 countries.".to_string());
        }
    }

    if codes.is_empty() {
        return Ok(DEFAULT_COUNTRIES.to_string());
    }
    Ok(codes.join(","))
}

fn sanitize_generator_error(stderr: &[u8], password: &str) -> String {
    let mut text = String::from_utf8_lossy(stderr).into_owned();
    if !password.is_empty() {
        text = text.replace(password, "***");
    }
    let mut chars: Vec<char> = text.trim().chars().collect();
    if chars.len() > 2000 {
        chars = chars.split_off(chars.len() - 2000);
    }
    chars.into_iter().collect()
}

#[cfg(windows)]
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(windows)]
fn ensure_generator(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;

    let directory = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Unable to resolve the app data directory: {error}"))?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Unable to create the app data directory: {error}"))?;
    let path = directory.join("proton-confgen.exe");

    if path.is_file() {
        if let Ok(metadata) = fs::metadata(&path) {
            if metadata.len() == PROTON_CONFGEN.len() as u64 {
                if let Ok(bytes) = fs::read(&path) {
                    if sha256_hex(&bytes) == PROTON_CONFGEN_SHA256 {
                        return Ok(path);
                    }
                }
            }
        }
    }

    if sha256_hex(PROTON_CONFGEN) != PROTON_CONFGEN_SHA256 {
        return Err("The bundled proton-confgen binary failed its integrity check.".to_string());
    }

    let temporary = directory.join("proton-confgen.exe.tmp");
    fs::write(&temporary, PROTON_CONFGEN)
        .map_err(|error| format!("Unable to write proton-confgen.exe: {error}"))?;
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|error| format!("Unable to replace proton-confgen.exe: {error}"))?;
    }
    fs::rename(&temporary, &path)
        .map_err(|error| format!("Unable to install proton-confgen.exe: {error}"))?;
    Ok(path)
}

#[cfg(windows)]
fn start_proton_tunnel_blocking(
    generator: &Path,
    username: String,
    password: String,
    countries: String,
) -> Result<LiveTunnelStatus, String> {
    use std::os::windows::process::CommandExt;

    let username = username.trim().to_string();
    if username.is_empty() {
        return Err("Enter your ProtonVPN username.".to_string());
    }
    let countries = normalize_countries(&countries)?;
    let password = Zeroizing::new(password);

    let status = crate::live_tunnel::live_tunnel_status_blocking()?;
    if status.running {
        return Err("Stop the current tunnel before generating a Proton profile.".to_string());
    }
    crate::live_tunnel::ensure_wiresock_installed()?;

    let directory = std::env::temp_dir().join("discord-quest-helper-live");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Unable to create the tunnel temp directory: {error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let output_path = directory.join(format!("proton-{}-{}.conf", std::process::id(), stamp));
    let output_argument = output_path.to_string_lossy().into_owned();

    let mut command = Command::new(generator);
    command
        .args([
            "--username",
            username.as_str(),
            "--password-stdin",
            "--free-only",
            "--countries",
            countries.as_str(),
            "--output",
            output_argument.as_str(),
            // Session-only certificate: nothing is registered on the Proton
            // account, so repeated starts never pile up persistent devices.
            "--no-save",
            "--session-duration",
            "0",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|error| format!("Unable to start proton-confgen: {error}"))?;
    let mut write_error: Option<String> = None;
    if let Some(mut stdin) = child.stdin.take() {
        if !password.is_empty() {
            if let Err(error) = stdin
                .write_all(password.as_bytes())
                .and_then(|_| stdin.write_all(b"\n"))
            {
                write_error = Some(error.to_string());
            }
        }
    }
    let child_output = child
        .wait_with_output()
        .map_err(|error| format!("Unable to read proton-confgen output: {error}"))?;

    if !child_output.status.success() {
        let _ = fs::remove_file(&output_path);
        let detail = sanitize_generator_error(&child_output.stderr, password.as_str());
        let detail = if detail.is_empty() {
            format!("exit code {:?}", child_output.status.code())
        } else {
            detail
        };
        let prefix = write_error
            .map(|error| format!("Unable to send the password: {error}; "))
            .unwrap_or_default();
        let lower = detail.to_ascii_lowercase();
        if lower.contains("2fa") || lower.contains("controlling terminal") {
            return Err(format!(
                "{prefix}2FA required. Run proton-confgen once in a terminal to save a session, then retry without a password. {detail}"
            ));
        }
        return Err(format!("{prefix}proton-confgen failed: {detail}"));
    }

    let valid_profile = fs::metadata(&output_path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false);
    if !valid_profile {
        let _ = fs::remove_file(&output_path);
        return Err("proton-confgen finished without creating a WireGuard profile.".to_string());
    }

    let result = crate::live_tunnel::start_live_tunnel_blocking(output_argument);
    let _ = fs::remove_file(&output_path);
    result
}

#[cfg(not(windows))]
fn start_proton_tunnel_blocking(
    _generator: &Path,
    _username: String,
    _password: String,
    _countries: String,
) -> Result<LiveTunnelStatus, String> {
    Err("Proton tunnel is only supported on Windows.".to_string())
}

#[tauri::command]
pub(crate) async fn start_proton_tunnel(
    app: tauri::AppHandle,
    username: String,
    password: String,
    countries: String,
) -> Result<LiveTunnelStatus, String> {
    #[cfg(not(windows))]
    {
        let _ = (app, username, password, countries);
        Err("Proton tunnel is only supported on Windows.".to_string())
    }

    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(move || {
            let generator = ensure_generator(&app)?;
            start_proton_tunnel_blocking(&generator, username, password, countries)
        })
        .await
        .map_err(|error| format!("Proton tunnel task failed: {error}"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_deduplicates_countries() {
        assert_eq!(normalize_countries(" us, nl,US ").unwrap(), "US,NL");
        assert_eq!(normalize_countries("").unwrap(), DEFAULT_COUNTRIES);
        assert!(normalize_countries("USA").is_err());
        assert!(normalize_countries("U1").is_err());
    }

    #[test]
    fn limits_country_count() {
        assert!(normalize_countries("US,NL,JP,DE,FR,GB,CA,AU,CH,SE,PL").is_err());
    }

    #[test]
    fn redacts_passwords_and_truncates_errors() {
        let error = sanitize_generator_error(b"login failed for hunter2\n", "hunter2");
        assert!(!error.contains("hunter2"));
        assert!(error.contains("***"));

        let long = "x".repeat(3000);
        assert_eq!(
            sanitize_generator_error(long.as_bytes(), "")
                .chars()
                .count(),
            2000
        );
    }

    #[cfg(windows)]
    #[test]
    fn embedded_generator_matches_the_notice_hash() {
        assert_eq!(&PROTON_CONFGEN[..2], b"MZ");
        assert_eq!(sha256_hex(PROTON_CONFGEN), PROTON_CONFGEN_SHA256);
    }
}

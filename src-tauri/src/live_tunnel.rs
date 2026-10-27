#![cfg_attr(not(windows), allow(dead_code))]

use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;

use once_cell::sync::Lazy;

const ALLOWED_APPS: &str = "Discord, Update, DiscordCanary, DiscordPTB, Vesktop";
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const STARTUP_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveTunnelStatus {
    pub(crate) supported: bool,
    pub(crate) wiresock_installed: bool,
    pub(crate) wiresock_path: Option<String>,
    pub(crate) running: bool,
    pub(crate) external_wiresock_running: bool,
    pub(crate) config_path: Option<String>,
    pub(crate) error: Option<String>,
}

#[derive(Default)]
struct TunnelState {
    child: Option<Child>,
    temp_config: Option<PathBuf>,
    config_path: Option<PathBuf>,
    last_error: Option<String>,
}

static TUNNEL: Lazy<Mutex<TunnelState>> = Lazy::new(|| Mutex::new(TunnelState::default()));

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfigSection {
    None,
    Interface,
    Peer,
    Other,
}

fn prepare_wireguard_config(input: &str) -> Result<String, String> {
    if input.trim().is_empty() {
        return Err("The WireGuard configuration is empty.".to_string());
    }

    let mut output_lines: Vec<String> = Vec::new();
    let mut section = ConfigSection::None;
    let mut has_interface = false;
    let mut peer_count = 0usize;
    let mut peer_line: Option<usize> = None;
    let mut interface_keys = HashSet::new();
    let mut peer_keys = HashSet::new();

    for (index, raw) in input.lines().enumerate() {
        let line = raw.trim();
        let wire_sock_directive = line.starts_with("#@ws:");
        if line.is_empty()
            || (!wire_sock_directive && (line.starts_with('#') || line.starts_with(';')))
        {
            output_lines.push(raw.to_string());
            continue;
        }

        if line.starts_with('[') {
            let end = line
                .find(']')
                .ok_or_else(|| format!("Invalid section header on line {}.", index + 1))?;
            let name = line[1..end].trim().to_ascii_lowercase();
            section = match name.as_str() {
                "interface" => {
                    has_interface = true;
                    ConfigSection::Interface
                }
                "peer" => {
                    peer_count += 1;
                    if peer_line.is_none() {
                        peer_line = Some(output_lines.len());
                    }
                    ConfigSection::Peer
                }
                _ => ConfigSection::Other,
            };
            output_lines.push(raw.to_string());
            continue;
        }

        let Some((raw_key, _)) = line.split_once('=') else {
            return Err(format!("Invalid configuration line {}.", index + 1));
        };
        let key = raw_key.trim().to_ascii_lowercase();
        if is_forbidden_directive(&key) {
            return Err(format!(
                "WireGuard directive '{}' is not allowed.",
                raw_key.trim()
            ));
        }
        if key == "#@ws:allowedapps" || key == "allowedapps" {
            continue;
        }

        match section {
            ConfigSection::Interface => {
                interface_keys.insert(key);
            }
            ConfigSection::Peer => {
                peer_keys.insert(key);
            }
            ConfigSection::None | ConfigSection::Other => {}
        }
        output_lines.push(raw.to_string());
    }

    if !has_interface {
        return Err("The WireGuard configuration is missing an [Interface] section.".to_string());
    }
    if peer_count == 0 {
        return Err("The WireGuard configuration is missing a [Peer] section.".to_string());
    }
    if peer_count > 1 {
        return Err(
            "The WireGuard configuration contains more than one [Peer] section.".to_string(),
        );
    }
    let peer_line = peer_line
        .ok_or_else(|| "The WireGuard configuration is missing a [Peer] section.".to_string())?;

    for (key, label) in [("privatekey", "PrivateKey"), ("address", "Address")] {
        if !interface_keys.contains(key) {
            return Err(format!(
                "The WireGuard configuration is missing {label} in [Interface]."
            ));
        }
    }
    for (key, label) in [
        ("publickey", "PublicKey"),
        ("allowedips", "AllowedIPs"),
        ("endpoint", "Endpoint"),
    ] {
        if !peer_keys.contains(key) {
            return Err(format!(
                "The WireGuard configuration is missing {label} in [Peer]."
            ));
        }
    }

    let mut result_lines = Vec::with_capacity(output_lines.len() + 1);
    for (index, line) in output_lines.into_iter().enumerate() {
        result_lines.push(line);
        if peer_line == index {
            result_lines.push(format!("#@ws:AllowedApps = {ALLOWED_APPS}"));
        }
    }

    let mut result = result_lines.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

fn is_forbidden_directive(key: &str) -> bool {
    matches!(
        key,
        "postup" | "preup" | "postdown" | "predown" | "saveconfig"
    )
}

fn read_wireguard_config(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Unable to read the WireGuard configuration: {error}"))?;
    if !metadata.is_file() {
        return Err("The selected WireGuard configuration is not a file.".to_string());
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err("The WireGuard configuration is too large.".to_string());
    }
    fs::read_to_string(path)
        .map_err(|error| format!("Unable to read the WireGuard configuration: {error}"))
}

fn write_temp_config(contents: &str) -> Result<PathBuf, String> {
    let directory = std::env::temp_dir().join("discord-quest-helper-live");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Unable to create the tunnel temp directory: {error}"))?;
    let path = directory.join(format!("tunnel-{}.conf", std::process::id()));
    fs::write(&path, contents)
        .map_err(|error| format!("Unable to write the tunnel temp configuration: {error}"))?;
    Ok(path)
}

#[cfg(windows)]
fn find_wiresock() -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let mut candidates = Vec::new();
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(value) = std::env::var(variable) {
            let base = PathBuf::from(value);
            candidates.push(
                base.join("WireSock Secure Connect")
                    .join("wiresock-client.exe"),
            );
            candidates.push(
                base.join("WireSock Secure Connect")
                    .join("sdk")
                    .join("wiresock-client.exe"),
            );
            candidates.push(
                base.join("WireSock Secure Connect CLI")
                    .join("wiresock-client.exe"),
            );
            candidates.push(
                base.join("WireSock Secure Connect CLI")
                    .join("sdk")
                    .join("wiresock-client.exe"),
            );
            candidates.push(
                base.join("WireSock Secure Connect SDK")
                    .join("wiresock-client.exe"),
            );
            candidates.push(
                base.join("WireSock Secure Connect SDK")
                    .join("sdk")
                    .join("wiresock-client.exe"),
            );
            candidates.push(base.join("WireSock").join("wiresock-client.exe"));
        }
    }
    if let Ok(value) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(value);
        candidates.push(
            base.join("Programs")
                .join("WireSock Secure Connect")
                .join("wiresock-client.exe"),
        );
        candidates.push(
            base.join("WireSock Secure Connect")
                .join("wiresock-client.exe"),
        );
        candidates.push(
            base.join("Microsoft")
                .join("WinGet")
                .join("Links")
                .join("wiresock-client.exe"),
        );
    }
    if let Some(path) = candidates.into_iter().find(|candidate| candidate.is_file()) {
        return Some(path);
    }

    let output = Command::new("where")
        .arg("wiresock-client.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

#[cfg(not(windows))]
fn find_wiresock() -> Option<PathBuf> {
    None
}

#[cfg(windows)]
pub(crate) fn ensure_wiresock_installed() -> Result<PathBuf, String> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    if let Some(path) = find_wiresock() {
        return Ok(path);
    }

    let script = "$p = Start-Process -FilePath winget -ArgumentList @('install','--id','NTKERNEL.WireSockVPNClientCLI','-e','--silent','--accept-source-agreements','--accept-package-agreements') -Verb RunAs -Wait -PassThru; exit $p.ExitCode";
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            script,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("Unable to start the WireSock installer: {error}"))?;

    for _ in 0..30 {
        if let Some(path) = find_wiresock() {
            return Ok(path);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        if detail.is_empty() {
            return Err(
                "WireSock installation was cancelled or failed (UAC/exit code).".to_string(),
            );
        }
        return Err(format!("WireSock installation failed: {detail}"));
    }

    Err(
        "WireSock was installed but wiresock-client.exe was not found. Restart the app and try again."
            .to_string(),
    )
}

#[cfg(windows)]
fn is_wiresock_process_running() -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    for image in [
        "wiresock-client.exe",
        "wiresock-client-service.exe",
        "wiresock-pro-client-service.exe",
    ] {
        let filter = format!("IMAGENAME eq {image}");
        let output = Command::new("tasklist")
            .args(["/FI", filter.as_str(), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        if let Ok(output) = output {
            let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
            if text.contains(image) {
                return true;
            }
        }
    }
    false
}

#[cfg(not(windows))]
fn is_wiresock_process_running() -> bool {
    false
}

fn cleanup_temp(state: &mut TunnelState) {
    if let Some(path) = state.temp_config.take() {
        let _ = fs::remove_file(path);
    }
}

fn build_status(state: &mut TunnelState) -> LiveTunnelStatus {
    let mut running = false;
    if let Some(child) = state.child.as_mut() {
        match child.try_wait() {
            Ok(None) => running = true,
            Ok(Some(_)) | Err(_) => {
                state.child = None;
                cleanup_temp(state);
                if state.last_error.is_none() {
                    state.last_error = Some("WireSock stopped unexpectedly.".to_string());
                }
            }
        }
    }

    let wiresock = find_wiresock();
    let external_wiresock_running = !running && is_wiresock_process_running();
    LiveTunnelStatus {
        supported: cfg!(windows),
        wiresock_installed: wiresock.is_some(),
        wiresock_path: wiresock.map(|path| path.to_string_lossy().into_owned()),
        running,
        external_wiresock_running,
        config_path: state
            .config_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        error: state.last_error.clone(),
    }
}

fn lock_state() -> Result<std::sync::MutexGuard<'static, TunnelState>, String> {
    TUNNEL
        .lock()
        .map_err(|_| "Live tunnel state is unavailable.".to_string())
}

pub(crate) fn live_tunnel_status_blocking() -> Result<LiveTunnelStatus, String> {
    let mut state = lock_state()?;
    Ok(build_status(&mut state))
}

pub(crate) fn start_live_tunnel_blocking(config_path: String) -> Result<LiveTunnelStatus, String> {
    #[cfg(not(windows))]
    {
        let _ = config_path;
        return Err("Live tunnel is only supported on Windows.".to_string());
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        let mut state = lock_state()?;
        if state.child.is_some() {
            let status = build_status(&mut state);
            if status.running {
                return Ok(status);
            }
        }
        if is_wiresock_process_running() {
            let error = "WireSock is already running outside this app. Stop it before starting the helper tunnel.".to_string();
            state.last_error = Some(error.clone());
            return Err(error);
        }

        let wiresock = match find_wiresock() {
            Some(path) => path,
            None => {
                let error = "WireSock is not installed. Install it and try again.".to_string();
                state.last_error = Some(error.clone());
                return Err(error);
            }
        };
        let input = match read_wireguard_config(Path::new(&config_path)) {
            Ok(value) => value,
            Err(error) => {
                state.last_error = Some(error.clone());
                return Err(error);
            }
        };
        let prepared = match prepare_wireguard_config(&input) {
            Ok(value) => value,
            Err(error) => {
                state.last_error = Some(error.clone());
                return Err(error);
            }
        };
        let temp_config = match write_temp_config(&prepared) {
            Ok(path) => path,
            Err(error) => {
                state.last_error = Some(error.clone());
                return Err(error);
            }
        };
        let temp_argument = temp_config.to_string_lossy().into_owned();
        let child = Command::new(&wiresock)
            .args([
                "run",
                "-config",
                temp_argument.as_str(),
                "-log-level",
                "error",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(&temp_config);
                let error = format!("Unable to start WireSock: {error}");
                state.last_error = Some(error.clone());
                return Err(error);
            }
        };

        state.child = Some(child);
        state.temp_config = Some(temp_config);
        state.config_path = Some(PathBuf::from(&config_path));
        state.last_error = None;

        std::thread::sleep(STARTUP_GRACE);
        let startup_failure = match state.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(exit_status)) => {
                    Some(format!("WireSock exited during startup ({exit_status})."))
                }
                Ok(None) => None,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    Some(format!("Unable to verify the WireSock process: {error}"))
                }
            },
            None => None,
        };
        if let Some(error) = startup_failure {
            state.child = None;
            cleanup_temp(&mut state);
            state.last_error = Some(error.clone());
            return Err(error);
        }

        Ok(build_status(&mut state))
    }
}

fn stop_live_tunnel_blocking() -> Result<LiveTunnelStatus, String> {
    #[cfg(not(windows))]
    {
        return Err("Live tunnel is only supported on Windows.".to_string());
    }

    #[cfg(windows)]
    {
        let mut state = lock_state()?;
        if let Some(mut child) = state.child.take() {
            let stop_error = match child.try_wait() {
                Ok(Some(_)) => None,
                Ok(None) => match child.kill() {
                    Ok(()) => {
                        let _ = child.wait();
                        None
                    }
                    Err(error) => Some(format!("Unable to stop WireSock: {error}")),
                },
                Err(error) => match child.kill() {
                    Ok(()) => {
                        let _ = child.wait();
                        None
                    }
                    Err(kill_error) => {
                        Some(format!("Unable to stop WireSock: {error}; {kill_error}"))
                    }
                },
            };
            if let Some(error) = stop_error {
                state.child = Some(child);
                state.last_error = Some(error.clone());
                return Err(error);
            }
        }

        cleanup_temp(&mut state);
        state.last_error = None;
        Ok(build_status(&mut state))
    }
}

#[tauri::command]
pub(crate) async fn live_tunnel_status() -> Result<LiveTunnelStatus, String> {
    tauri::async_runtime::spawn_blocking(live_tunnel_status_blocking)
        .await
        .map_err(|error| format!("Live tunnel status task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn start_live_tunnel(config_path: String) -> Result<LiveTunnelStatus, String> {
    tauri::async_runtime::spawn_blocking(move || start_live_tunnel_blocking(config_path))
        .await
        .map_err(|error| format!("Live tunnel start task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn stop_live_tunnel() -> Result<LiveTunnelStatus, String> {
    tauri::async_runtime::spawn_blocking(stop_live_tunnel_blocking)
        .await
        .map_err(|error| format!("Live tunnel stop task failed: {error}"))?
}

pub(crate) fn stop_for_exit() {
    if let Ok(mut state) = TUNNEL.lock() {
        if let Some(mut child) = state.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        cleanup_temp(&mut state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_CONFIG: &str = "[Interface]\nPrivateKey = secret\nAddress = 10.0.0.2/32\nDNS = 1.1.1.1\n\n[Peer]\nPublicKey = pub\nAllowedIPs = 0.0.0.0/0\nEndpoint = 1.2.3.4:51820\n";

    fn prepared(input: &str) -> String {
        prepare_wireguard_config(input).expect("config should be valid")
    }

    #[test]
    fn inserts_the_allowlist_after_the_peer_header() {
        let output = prepared(VALID_CONFIG);
        let peer = output.find("[Peer]").expect("peer header");
        let allowlist = output
            .find("#@ws:AllowedApps")
            .expect("allowlist directive");
        assert!(allowlist > peer);
        assert_eq!(output.matches("#@ws:AllowedApps").count(), 1);
        assert!(output.contains(ALLOWED_APPS));
    }

    #[test]
    fn replaces_an_existing_allowlist_instead_of_duplicating_it() {
        let input = VALID_CONFIG.replace(
            "[Peer]\n",
            "[Peer]\n#@ws:AllowedApps = old-client\nAllowedApps = old-client\n",
        );
        let output = prepared(&input);
        assert_eq!(output.matches("#@ws:AllowedApps").count(), 1);
        assert!(!output.contains("old-client"));
        assert!(output.contains(ALLOWED_APPS));
    }

    #[test]
    fn accepts_crlf_configs() {
        let input = VALID_CONFIG.replace('\n', "\r\n");
        let output = prepared(&input);
        assert!(output.contains("#@ws:AllowedApps"));
        assert!(!output.contains('\r'));
    }

    #[test]
    fn rejects_missing_peer_section() {
        let input = "[Interface]\nPrivateKey = secret\nAddress = 10.0.0.2/32\n";
        let error = prepare_wireguard_config(input).expect_err("missing peer");
        assert!(error.contains("[Peer]"));
    }

    #[test]
    fn rejects_multiple_peer_sections() {
        let input = format!(
            "{VALID_CONFIG}\n[Peer]\nPublicKey = other\nAllowedIPs = 0.0.0.0/0\nEndpoint = 1.2.3.5:51820\n"
        );
        let error = prepare_wireguard_config(&input).expect_err("multiple peers");
        assert!(error.contains("more than one"));
    }

    #[test]
    fn rejects_missing_required_keys_without_echoing_values() {
        let input = "[Interface]\nPrivateKey = secret\n\n[Peer]\nPublicKey = pub\n";
        let error = prepare_wireguard_config(input).expect_err("missing address");
        assert!(error.contains("Address"));
        assert!(!error.contains("secret"));
        assert!(!error.contains("pub"));
    }

    #[test]
    fn rejects_command_directives() {
        let input = VALID_CONFIG.replace("[Peer]\n", "[Peer]\nPostUp = calc\n");
        let error = prepare_wireguard_config(&input).expect_err("postup");
        assert!(error.contains("PostUp"));
        assert!(!error.contains("secret"));
    }

    #[test]
    fn rejects_empty_configs() {
        assert!(prepare_wireguard_config(" \n").is_err());
    }
}

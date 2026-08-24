mod audit;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod model;
#[cfg(target_os = "windows")]
mod windows;

use model::{configured_internal_names_are_valid, RuntimeIdentityStatus};
pub(crate) use model::{contains_product_token, RUNTIME_BRIDGE_NAME, RUNTIME_NAMESPACE};

use once_cell::sync::Lazy;
#[cfg(any(target_os = "windows", test))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::RwLock;

#[cfg(any(target_os = "windows", test))]
pub(crate) const DIR_HEX_LEN: usize = 16;
#[cfg(any(target_os = "windows", test))]
pub(crate) const FILE_HEX_LEN: usize = 12;

static STATUS: Lazy<RwLock<RuntimeIdentityStatus>> = Lazy::new(|| {
    RwLock::new(RuntimeIdentityStatus::disabled(
        std::env::consts::OS,
        "runtime identity has not been initialized",
    ))
});

pub fn initialize() {
    #[cfg(target_os = "windows")]
    let status = {
        windows::ensure_stealth_mode();
        windows::apply_process_identity();
        if cfg!(debug_assertions) {
            RuntimeIdentityStatus::disabled(
                "windows",
                "runtime identity minimization is disabled for this development process",
            )
        } else if windows::is_stealth_mode() {
            RuntimeIdentityStatus {
                platform: "windows".into(),
                level: model::RuntimeIdentityLevel::Full,
                main_executable_ok: true,
                helper_identity_ok: None,
                package_signature_ok: None,
                desktop_integration_ok: None,
                reasons: Vec::new(),
            }
        } else {
            RuntimeIdentityStatus {
                platform: "windows".into(),
                level: model::RuntimeIdentityLevel::Degraded,
                main_executable_ok: false,
                helper_identity_ok: None,
                package_signature_ok: None,
                desktop_integration_ok: None,
                reasons: vec!["temporary runtime identity could not be prepared".into()],
            }
        }
    };

    #[cfg(target_os = "linux")]
    let status = linux::initial_status();
    #[cfg(target_os = "macos")]
    let status = macos::initial_status();
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    let status = RuntimeIdentityStatus {
        platform: std::env::consts::OS.into(),
        level: model::RuntimeIdentityLevel::NotApplicable,
        main_executable_ok: false,
        helper_identity_ok: None,
        package_signature_ok: None,
        desktop_integration_ok: None,
        reasons: vec!["runtime identity is not implemented on this platform".into()],
    };

    let mut status = status;
    if !configured_internal_names_are_valid() {
        status.main_executable_ok = false;
        status
            .reasons
            .push("configured internal runtime names violate the identity policy".into());
        status.level = model::RuntimeIdentityLevel::Degraded;
    }

    *STATUS
        .write()
        .expect("runtime identity status lock poisoned") = status;
}

pub fn status() -> RuntimeIdentityStatus {
    STATUS
        .read()
        .expect("runtime identity status lock poisoned")
        .clone()
}

#[allow(dead_code)]
pub fn record_helper_identity(result: Result<(), String>) {
    let mut status = STATUS
        .write()
        .expect("runtime identity status lock poisoned");
    status
        .reasons
        .retain(|reason| !reason.starts_with("runtime bridge:"));
    match result {
        Ok(()) => status.helper_identity_ok = Some(true),
        Err(reason) => {
            status.helper_identity_ok = Some(false);
            status.reasons.push(format!("runtime bridge: {reason}"));
        }
    }
    status.recompute_level();
}

pub fn record_helper_degraded(reason: String) {
    let mut status = STATUS
        .write()
        .expect("runtime identity status lock poisoned");
    status.helper_identity_ok = Some(true);
    status
        .reasons
        .retain(|existing| !existing.starts_with("runtime bridge:"));
    status.reasons.push(format!("runtime bridge: {reason}"));
    status.recompute_level();
}

pub fn cleanup_on_exit() {
    #[cfg(target_os = "windows")]
    windows::cleanup_on_exit();
}

pub fn uses_temporary_runtime() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::is_stealth_mode()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub fn runtime_window_title() -> String {
    #[cfg(target_os = "windows")]
    {
        windows::generate_stealth_window_title()
    }
    #[cfg(not(target_os = "windows"))]
    {
        "Discord Quest Helper".into()
    }
}

pub fn webview_user_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        windows::webview_user_data_dir()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn generate_random_suffix(length: usize) -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..length)
        .map(|_| format!("{:x}", rng.random::<u8>() % 16))
        .collect()
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn is_hex_str(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn paths_eq(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn strip_zone_identifier(path: &Path) {
    windows::strip_zone_identifier(path);
}

#[tauri::command]
pub fn get_runtime_identity_status() -> RuntimeIdentityStatus {
    status()
}

#[tauri::command]
pub fn get_runtime_identity_audit(fingerprint_raw: Option<String>) -> audit::RuntimeIdentityAudit {
    audit::collect(fingerprint_raw, status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_failure_is_observable() {
        initialize();
        record_helper_identity(Err("helper verification failed".into()));
        let status = status();
        assert_eq!(status.helper_identity_ok, Some(false));
        assert!(status
            .reasons
            .iter()
            .any(|reason| reason.contains("helper")));
    }
}

use super::model::{RuntimeIdentityLevel, RuntimeIdentityStatus, RUNTIME_MAIN_NAME};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PUBLIC_APP_BUNDLE_NAME: &str = "Discord Quest Helper.app";
const LEGACY_DIR_HEX_LEN: usize = 16;
const LEGACY_EXE_HEX_LEN: usize = 12;
const BUNDLE_IDENTIFIER: &str = "com.masterain.discord-quest-helper";

pub(super) fn initial_status() -> RuntimeIdentityStatus {
    let cleanup_reasons = cleanup_legacy_temp_runtimes();
    if cfg!(debug_assertions)
        || std::env::var_os("RUNTIME_IDENTITY_MODE").as_deref() == Some(std::ffi::OsStr::new("off"))
    {
        let mut status = RuntimeIdentityStatus::disabled(
            "macos",
            "runtime identity minimization is disabled for this development process",
        );
        status.reasons.extend(cleanup_reasons);
        return status;
    }

    let executable = std::env::current_exe().ok();
    let mut status = status_for_executable(executable.as_deref());
    match executable.as_deref().and_then(app_bundle_for_executable) {
        Some(bundle) => match verify_bundle_signature(&bundle) {
            Ok(()) => status.package_signature_ok = Some(true),
            Err(reason) => {
                status.package_signature_ok = Some(false);
                status.reasons.push(reason);
            }
        },
        None => {
            status.package_signature_ok = Some(false);
            status
                .reasons
                .push("release process is not running from the expected application bundle".into());
        }
    }
    status.reasons.extend(cleanup_reasons);
    status.recompute_level();
    status
}

fn app_bundle_for_executable(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    if macos.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    (app.file_name()?.to_str()? == PUBLIC_APP_BUNDLE_NAME).then(|| app.to_path_buf())
}

fn verify_bundle_signature(bundle: &Path) -> Result<(), String> {
    let status = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "--verbose=2"])
        .arg(bundle)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "macOS code-signing verification tool is unavailable".to_string())?;
    if !status.success() {
        return Err(
            "application bundle or nested code failed strict signature verification".into(),
        );
    }

    let display = Command::new("/usr/bin/codesign")
        .args(["-dvv"])
        .arg(bundle)
        .output()
        .map_err(|_| "macOS code-signing verification tool is unavailable".to_string())?;
    let details = String::from_utf8_lossy(&display.stderr);
    if !details.contains("Authority=Developer ID Application:") {
        return Err(
            "application bundle is not signed with a Developer ID Application identity".into(),
        );
    }
    if !details
        .lines()
        .any(|line| line.starts_with("flags=") && line.contains("runtime"))
    {
        return Err("application bundle signature is missing the hardened runtime flag".into());
    }
    Ok(())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn legacy_executable_candidate(directory: &Path) -> Option<PathBuf> {
    let directory_name = directory.file_name()?.to_str()?;
    if !is_lower_hex(directory_name, LEGACY_DIR_HEX_LEN) || !directory.join("ud").is_dir() {
        return None;
    }
    let candidates = fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let executable = path.is_file()
                && is_lower_hex(name, LEGACY_EXE_HEX_LEN)
                && fs::metadata(&path).ok()?.permissions().mode() & 0o111 != 0;
            executable.then_some(path)
        })
        .collect::<Vec<_>>();
    (candidates.len() == 1).then(|| candidates[0].clone())
}

fn has_legacy_code_identifier(executable: &Path) -> bool {
    let output = Command::new("/usr/bin/codesign")
        .args(["-d", "--verbose=2"])
        .arg(executable)
        .output();
    let Ok(output) = output else {
        return false;
    };
    let details = String::from_utf8_lossy(&output.stderr);
    details
        .lines()
        .any(|line| line.trim() == format!("Identifier={BUNDLE_IDENTIFIER}"))
}

fn legacy_executable_is_running(executable: &Path) -> bool {
    let output = Command::new("/bin/ps").args(["-axo", "comm="]).output();
    let Ok(output) = output else {
        return true;
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .any(|command| Path::new(command) == executable)
}

fn cleanup_legacy_temp_runtimes() -> Vec<String> {
    let mut reasons = Vec::new();
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return reasons;
    };
    for entry in entries.flatten() {
        let directory = entry.path();
        if !directory.is_dir() {
            continue;
        }
        let Some(executable) = legacy_executable_candidate(&directory) else {
            continue;
        };
        if !has_legacy_code_identifier(&executable) {
            reasons.push("an ambiguous legacy temporary runtime was left untouched".into());
            continue;
        }
        if legacy_executable_is_running(&executable) {
            reasons.push(
                "a verified legacy temporary runtime is still active and was left untouched".into(),
            );
            continue;
        }
        if fs::remove_dir_all(&directory).is_err() {
            reasons.push("a verified legacy temporary runtime could not be removed".into());
        }
    }
    reasons
}

pub(super) fn legacy_temp_artifact_count() -> usize {
    fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| legacy_executable_candidate(&entry.path()).is_some())
        .count()
}

fn status_for_executable(executable: Option<&Path>) -> RuntimeIdentityStatus {
    let mut reasons = Vec::new();
    let main_executable_ok = executable
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(|name| name == RUNTIME_MAIN_NAME)
        .unwrap_or(false);
    if !main_executable_ok {
        reasons.push("CFBundleExecutable does not match the configured runtime identity".into());
    }
    RuntimeIdentityStatus {
        platform: "macos".into(),
        level: if main_executable_ok {
            RuntimeIdentityLevel::Full
        } else {
            RuntimeIdentityLevel::Degraded
        },
        main_executable_ok,
        helper_identity_ok: None,
        package_signature_ok: None,
        desktop_integration_ok: None,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_executable_is_checked_independently_from_public_app_name() {
        assert_eq!(
            status_for_executable(Some(Path::new(
                "/Applications/Discord Quest Helper.app/Contents/MacOS/meridian"
            )))
            .level,
            RuntimeIdentityLevel::Full
        );
    }

    #[test]
    fn bundle_path_requires_the_complete_public_app_bundle() {
        let bundled = Path::new("/Applications/Discord Quest Helper.app/Contents/MacOS/meridian");
        assert_eq!(
            app_bundle_for_executable(bundled),
            Some(PathBuf::from("/Applications/Discord Quest Helper.app"))
        );
        assert!(app_bundle_for_executable(Path::new("/tmp/meridian")).is_none());
    }

    #[test]
    fn legacy_cleanup_layout_requires_owned_markers() {
        let root =
            std::env::temp_dir().join(format!("runtime-identity-test-{}", std::process::id()));
        let directory = root.join("0123456789abcdef");
        fs::create_dir_all(directory.join("ud")).unwrap();
        let executable = directory.join("c0ffee12beef");
        fs::write(&executable, b"fixture").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        assert_eq!(legacy_executable_candidate(&directory), Some(executable));
        fs::remove_dir_all(root).unwrap();
    }
}

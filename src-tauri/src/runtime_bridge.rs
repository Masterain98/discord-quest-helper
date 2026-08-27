//! Verified, versioned installation for the optional desktop runtime bridge.

use crate::runtime_identity::{contains_product_token, RUNTIME_BRIDGE_NAME, RUNTIME_NAMESPACE};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, PartialEq, Eq)]
pub struct InstallReport {
    pub executable: PathBuf,
    pub legacy_cleanup_warning: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActiveRuntimeManifest {
    schema_version: u8,
    version: String,
    executable: String,
    sha256: String,
}

static INSTALL_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TemporaryFile {
    path: PathBuf,
    armed: bool,
}

impl TemporaryFile {
    fn new(parent: &Path, prefix: &str, extension: &str) -> Self {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self {
            path: parent.join(format!(
                ".{prefix}-{}-{sequence}{extension}",
                std::process::id()
            )),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn versioned_executable_path(data_root: &Path) -> PathBuf {
    runtime_root(data_root)
        .join(env!("CARGO_PKG_VERSION"))
        .join(RUNTIME_BRIDGE_NAME)
}

pub fn active_manifest_path(data_root: &Path) -> PathBuf {
    runtime_root(data_root).join("active.json")
}

fn runtime_root(data_root: &Path) -> PathBuf {
    data_root.join(RUNTIME_NAMESPACE).join("runtime")
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("runtime bridge hash input is unavailable: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("runtime bridge hash could not be calculated: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_source_name(source: &Path) -> Result<(), String> {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "runtime bridge source name is unavailable".to_string())?;
    let allowed =
        name == RUNTIME_BRIDGE_NAME || name.starts_with(&format!("{RUNTIME_BRIDGE_NAME}-"));
    if !allowed || contains_product_token(name) {
        return Err("runtime bridge source has an unexpected internal identity".into());
    }
    let size = fs::metadata(source)
        .map_err(|error| format!("runtime bridge source metadata is unavailable: {error}"))?
        .len();
    if size == 0 {
        return Err("runtime bridge source is an empty build placeholder".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_platform_signature(path: &Path) -> Result<(), String> {
    if !crate::runtime_identity::MACOS_SIGNING_ENABLED {
        return Ok(());
    }
    let status = Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict", "--verbose=2"])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "runtime bridge signature verifier is unavailable".to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "runtime bridge signature verification failed".to_string())?;
    if !cfg!(debug_assertions) {
        crate::runtime_identity::verify_helper_identity_for_current_app(path)?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn verify_platform_signature(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn verify_helper_launch(path: &Path) -> Result<(), String> {
    let mut child = Command::new(path)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "runtime bridge launch verification failed".to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return status
                    .success()
                    .then_some(())
                    .ok_or_else(|| "runtime bridge launch verification failed".to_string());
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("runtime bridge launch verification timed out".into());
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("runtime bridge launch verification failed".into());
            }
        }
    }
}

fn write_active_manifest(data_root: &Path, sha256: &str) -> Result<(), String> {
    let manifest_path = active_manifest_path(data_root);
    let parent = manifest_path
        .parent()
        .ok_or_else(|| "runtime bridge manifest parent is unavailable".to_string())?;
    fs::create_dir_all(parent).map_err(|error| {
        format!("runtime bridge manifest directory could not be created: {error}")
    })?;
    let mut temporary = TemporaryFile::new(parent, "active", ".json");
    let manifest = ActiveRuntimeManifest {
        schema_version: 1,
        version: env!("CARGO_PKG_VERSION").to_string(),
        executable: format!("{}/{}", env!("CARGO_PKG_VERSION"), RUNTIME_BRIDGE_NAME),
        sha256: sha256.to_string(),
    };
    let encoded = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("runtime bridge manifest could not be encoded: {error}"))?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temporary.path)
        .map_err(|error| format!("runtime bridge manifest could not be written: {error}"))?;
    file.write_all(&encoded)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("runtime bridge manifest could not be committed: {error}"))?;
    fs::rename(&temporary.path, &manifest_path)
        .map_err(|error| format!("runtime bridge manifest switch failed: {error}"))?;
    temporary.disarm();
    Ok(())
}

fn cleanup_legacy_executable(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    if fs::remove_file(path).is_err() {
        return Some(
            "verified installation succeeded but the legacy helper could not be removed".into(),
        );
    }
    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir(parent);
    }
    None
}

fn verify_installed_for_execution(data_root: &Path, target: &Path) -> Result<(), String> {
    let manifest: ActiveRuntimeManifest = serde_json::from_slice(
        &fs::read(active_manifest_path(data_root))
            .map_err(|error| format!("runtime bridge active manifest is unavailable: {error}"))?,
    )
    .map_err(|error| format!("runtime bridge active manifest is invalid: {error}"))?;
    let expected_version = env!("CARGO_PKG_VERSION");
    let expected_executable = format!("{expected_version}/{RUNTIME_BRIDGE_NAME}");
    if manifest.schema_version != 1
        || manifest.version != expected_version
        || manifest.executable != expected_executable
    {
        return Err("runtime bridge active manifest identity is invalid".into());
    }
    if manifest.sha256 != sha256_file(target)? {
        return Err(
            "runtime bridge active manifest hash does not match installed executable".into(),
        );
    }
    Ok(())
}

fn install_with_verifier<F, G>(
    source: &Path,
    data_root: &Path,
    legacy_executable: &Path,
    verify_signature: F,
    verify_launch: G,
) -> Result<InstallReport, String>
where
    F: Fn(&Path) -> Result<(), String>,
    G: Fn(&Path) -> Result<(), String>,
{
    let _install_guard = INSTALL_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    validate_source_name(source)?;
    verify_signature(source)?;

    let target = versioned_executable_path(data_root);
    let parent = target
        .parent()
        .ok_or_else(|| "runtime bridge target parent is unavailable".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("runtime bridge directory could not be created: {error}"))?;

    let source_hash = sha256_file(source)?;
    let target_hash = if target.is_file()
        && sha256_file(&target).ok().as_deref() == Some(source_hash.as_str())
        && verify_signature(&target).is_ok()
        && verify_launch(&target).is_ok()
    {
        source_hash
    } else {
        let mut temporary = TemporaryFile::new(parent, RUNTIME_BRIDGE_NAME, ".installing");
        fs::copy(source, &temporary.path)
            .map_err(|error| format!("runtime bridge could not be copied: {error}"))?;
        fs::set_permissions(&temporary.path, fs::Permissions::from_mode(0o755)).map_err(
            |error| format!("runtime bridge executable permission could not be set: {error}"),
        )?;
        let source_hash_after_copy = sha256_file(source)?;
        let temporary_hash = sha256_file(&temporary.path)?;
        let verified = if source_hash_after_copy == temporary_hash {
            verify_signature(&temporary.path)?;
            temporary_hash
        } else {
            return Err("runtime bridge copy failed SHA-256 verification".into());
        };
        if verified != source_hash {
            return Err("runtime bridge source changed during installation".into());
        }
        verify_launch(&temporary.path)?;
        fs::rename(&temporary.path, &target)
            .map_err(|error| format!("runtime bridge activation failed: {error}"))?;
        temporary.disarm();
        let active_hash = sha256_file(&target)?;
        if active_hash != source_hash {
            return Err("activated runtime bridge failed SHA-256 verification".into());
        }
        verify_signature(&target)?;
        verified
    };

    write_active_manifest(data_root, &target_hash)?;
    verify_installed_for_execution(data_root, &target)?;
    Ok(InstallReport {
        executable: target,
        legacy_cleanup_warning: cleanup_legacy_executable(legacy_executable),
    })
}

pub fn install(
    source: &Path,
    data_root: &Path,
    legacy_executable: &Path,
) -> Result<InstallReport, String> {
    install_with_verifier(
        source,
        data_root,
        legacy_executable,
        verify_platform_signature,
        verify_helper_launch,
    )
}

pub fn verify_bundled_for_execution(path: &Path) -> Result<(), String> {
    validate_source_name(path)?;
    verify_platform_signature(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_manifest_names_are_neutral() {
        let root = Path::new("/data");
        let executable = versioned_executable_path(root);
        assert_eq!(
            executable.file_name().and_then(|name| name.to_str()),
            Some("waybridge")
        );
        assert!(executable.to_string_lossy().contains("blueorbit/runtime"));
        assert!(!contains_product_token(&executable.to_string_lossy()));
        assert_eq!(
            active_manifest_path(root)
                .file_name()
                .and_then(|name| name.to_str()),
            Some("active.json")
        );
    }

    #[test]
    fn rejects_legacy_or_empty_source_names_before_copying() {
        let root = std::env::temp_dir().join(format!("runtime-bridge-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let legacy = root.join("discord-cdp-launcher");
        fs::write(&legacy, b"binary").unwrap();
        assert!(validate_source_name(&legacy).is_err());
        let empty = root.join("waybridge");
        fs::write(&empty, b"").unwrap();
        assert!(validate_source_name(&empty).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installs_atomically_writes_manifest_and_removes_only_exact_legacy_file() {
        let root = std::env::temp_dir().join(format!(
            "runtime-bridge-install-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("thread")
        ));
        let source_dir = root.join("source");
        let data_root = root.join("data");
        let legacy_dir = root.join("Discord Quest Helper");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&legacy_dir).unwrap();
        let source = source_dir.join("waybridge-test-target");
        fs::write(&source, b"signed runtime bridge fixture").unwrap();
        let legacy = legacy_dir.join("discord-cdp-launcher");
        fs::write(&legacy, b"legacy").unwrap();
        let unrelated = legacy_dir.join("keep.txt");
        fs::write(&unrelated, b"keep").unwrap();

        let report =
            install_with_verifier(&source, &data_root, &legacy, |_| Ok(()), |_| Ok(())).unwrap();

        assert_eq!(
            fs::read(&report.executable).unwrap(),
            fs::read(&source).unwrap()
        );
        assert!(!legacy.exists());
        assert!(unrelated.exists());
        let manifest = fs::read_to_string(active_manifest_path(&data_root)).unwrap();
        assert!(manifest.contains(&format!(
            "\"executable\": \"{}/waybridge\"",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(manifest.contains("\"sha256\""));
        assert!(fs::read_dir(report.executable.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("installing")));
        assert!(root.exists(), "shared parent directory must not be removed");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_tampered_active_manifest_before_execution() {
        let root = std::env::temp_dir().join(format!(
            "runtime-bridge-manifest-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("thread")
        ));
        let source_dir = root.join("source");
        let data_root = root.join("data");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("waybridge-test-target");
        fs::write(&source, b"runtime bridge fixture").unwrap();
        let report = install_with_verifier(
            &source,
            &data_root,
            &root.join("legacy"),
            |_| Ok(()),
            |_| Ok(()),
        )
        .unwrap();

        let manifest_path = active_manifest_path(&data_root);
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["sha256"] = serde_json::Value::String("0".repeat(64));
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        assert!(verify_installed_for_execution(&data_root, &report.executable).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_launch_keeps_legacy_helper_and_previous_active_version() {
        let root = std::env::temp_dir().join(format!(
            "runtime-bridge-rollback-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("thread")
        ));
        let source_dir = root.join("source");
        let data_root = root.join("data");
        let legacy_dir = root.join("legacy");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&legacy_dir).unwrap();
        let source = source_dir.join("waybridge-test-target");
        fs::write(&source, b"new runtime bridge fixture").unwrap();
        let target = versioned_executable_path(&data_root);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"previous runtime bridge").unwrap();
        let legacy = legacy_dir.join("discord-cdp-launcher");
        fs::write(&legacy, b"legacy").unwrap();

        let error = install_with_verifier(
            &source,
            &data_root,
            &legacy,
            |_| Ok(()),
            |_| Err("launch rejected".into()),
        )
        .unwrap_err();

        assert_eq!(error, "launch rejected");
        assert_eq!(fs::read(&target).unwrap(), b"previous runtime bridge");
        assert!(legacy.exists());
        assert!(!active_manifest_path(&data_root).exists());
        fs::remove_dir_all(root).unwrap();
    }
}

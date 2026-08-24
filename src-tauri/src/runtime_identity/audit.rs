use super::model::{RuntimeIdentityStatus, RUNTIME_BRIDGE_NAME, RUNTIME_MAIN_NAME};
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};

const RELEASE_BASELINE: &str =
    include_str!("../../../docs/fixtures/runtime-identity/release-baseline.json");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableAudit {
    basename: String,
    path: String,
    path_has_product_token: bool,
    unexpected_path_product_token: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperAudit {
    installed: bool,
    basename: Option<String>,
    path: Option<String>,
    path_has_product_token: Option<bool>,
    manifest_hash_ok: Option<bool>,
    signature_ok: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintAudit {
    status: String,
    sha256: Option<String>,
    length: usize,
    field_count: usize,
    field_names: Vec<String>,
    raw_available_locally: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineComparison {
    matches: bool,
    differences: Vec<String>,
    configured_window_identity_matches: Option<bool>,
    observed_window_identity_matches: Option<bool>,
    unavailable_observations: Vec<String>,
    fingerprint_fields_added: Vec<String>,
    fingerprint_fields_removed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeIdentityAudit {
    schema_version: u8,
    captured_at_unix: u64,
    platform: String,
    build_profile: String,
    status: RuntimeIdentityStatus,
    main: ExecutableAudit,
    helper: HelperAudit,
    legacy_artifact_count: usize,
    migration_result: String,
    fingerprint: FingerprintAudit,
    platform_details: Value,
    baseline: BaselineComparison,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn redacted_path(path: &Path) -> String {
    if let Some(home) = home_dir() {
        if let Ok(relative) = path.strip_prefix(&home) {
            if relative.as_os_str().is_empty() {
                return "$HOME".to_string();
            }
            return format!("$HOME/{}", relative.to_string_lossy());
        }
    }
    path.to_string_lossy().into_owned()
}

fn main_executable_audit() -> ExecutableAudit {
    let path = std::env::current_exe().unwrap_or_default();
    let basename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let path_text = path.to_string_lossy();
    let path_has_product_token = super::contains_product_token(&path_text);
    #[cfg(target_os = "macos")]
    let unexpected_path_product_token = unexpected_macos_path_product_token(&path);
    #[cfg(not(target_os = "macos"))]
    let unexpected_path_product_token = path_has_product_token;
    ExecutableAudit {
        basename,
        path: redacted_path(&path),
        path_has_product_token,
        unexpected_path_product_token,
    }
}

#[cfg(target_os = "macos")]
fn unexpected_macos_path_product_token(path: &Path) -> bool {
    let Some(bundle) = super::macos::app_bundle_for_executable(path) else {
        return super::contains_product_token(&path.to_string_lossy());
    };
    let Some(parent) = bundle.parent() else {
        return true;
    };
    let Ok(relative) = path.strip_prefix(&bundle) else {
        return true;
    };
    let normalized = parent.join("PUBLIC_APP.app").join(relative);
    super::contains_product_token(&normalized.to_string_lossy())
}

#[cfg(unix)]
fn sha256_file(path: &Path) -> Option<String> {
    Some(format!("{:x}", Sha256::digest(fs::read(path).ok()?)))
}

#[cfg(target_os = "macos")]
fn runtime_data_root() -> Option<PathBuf> {
    Some(home_dir()?.join("Library").join("Application Support"))
}

#[cfg(target_os = "linux")]
fn runtime_data_root() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| Some(home_dir()?.join(".local").join("share")))
}

#[cfg(target_os = "macos")]
fn legacy_helper_path() -> Option<PathBuf> {
    Some(
        runtime_data_root()?
            .join("Discord Quest Helper")
            .join("discord-cdp-launcher"),
    )
}

#[cfg(target_os = "linux")]
fn legacy_helper_path() -> Option<PathBuf> {
    Some(
        runtime_data_root()?
            .join("discord-quest-helper")
            .join("bin")
            .join("discord-cdp-launcher"),
    )
}

#[cfg(unix)]
fn helper_audit() -> (HelperAudit, usize, String) {
    let Some(data_root) = runtime_data_root() else {
        return (
            HelperAudit {
                installed: false,
                basename: None,
                path: None,
                path_has_product_token: None,
                manifest_hash_ok: None,
                signature_ok: None,
            },
            0,
            "dataRootUnavailable".into(),
        );
    };
    let path = crate::runtime_bridge::versioned_executable_path(&data_root);
    let installed = path.is_file();
    let basename = installed
        .then(|| path.file_name()?.to_str().map(str::to_string))
        .flatten();
    let path_has_product_token =
        installed.then(|| super::contains_product_token(&path.to_string_lossy()));
    let manifest_hash_ok = installed.then(|| {
        let manifest = fs::read_to_string(crate::runtime_bridge::active_manifest_path(&data_root))
            .ok()
            .and_then(|encoded| serde_json::from_str::<Value>(&encoded).ok());
        let expected = manifest
            .as_ref()
            .and_then(|value| value.get("sha256"))
            .and_then(Value::as_str);
        let expected_executable = format!("{}/{}", env!("CARGO_PKG_VERSION"), RUNTIME_BRIDGE_NAME);
        let executable_ok = manifest
            .as_ref()
            .and_then(|value| value.get("executable"))
            .and_then(Value::as_str)
            == Some(expected_executable.as_str());
        let version_ok = manifest
            .as_ref()
            .and_then(|value| value.get("version"))
            .and_then(Value::as_str)
            == Some(env!("CARGO_PKG_VERSION"));
        expected.is_some()
            && sha256_file(&path).as_deref() == expected
            && executable_ok
            && version_ok
    });
    #[cfg(target_os = "macos")]
    let signature_ok =
        installed.then(|| crate::runtime_bridge::verify_bundled_for_execution(&path).is_ok());
    #[cfg(not(target_os = "macos"))]
    let signature_ok = None;
    let legacy_helper_count = legacy_helper_path().is_some_and(|path| path.is_file()) as usize;
    #[cfg(target_os = "macos")]
    let legacy_artifact_count = legacy_helper_count + super::macos::legacy_temp_artifact_count();
    #[cfg(not(target_os = "macos"))]
    let legacy_artifact_count = legacy_helper_count;
    let migration_result = match (installed, legacy_artifact_count, manifest_hash_ok) {
        (true, 0, Some(true)) => "verified",
        (true, _, Some(true)) => "legacyPendingRemoval",
        (true, _, _) => "verificationFailed",
        (false, count, _) if count > 0 => "legacyDetected",
        _ => "notRequested",
    }
    .to_string();
    (
        HelperAudit {
            installed,
            basename,
            path: installed.then(|| redacted_path(&path)),
            path_has_product_token,
            manifest_hash_ok,
            signature_ok,
        },
        legacy_artifact_count,
        migration_result,
    )
}

#[cfg(not(unix))]
fn helper_audit() -> (HelperAudit, usize, String) {
    (
        HelperAudit {
            installed: false,
            basename: None,
            path: None,
            path_has_product_token: None,
            manifest_hash_ok: None,
            signature_ok: None,
        },
        0,
        "notApplicable".into(),
    )
}

fn fingerprint_audit(raw: Option<String>) -> FingerprintAudit {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return FingerprintAudit {
            status: "unavailable".into(),
            sha256: None,
            length: 0,
            field_count: 0,
            field_names: Vec::new(),
            raw_available_locally: false,
        };
    };
    let mut field_names: Vec<String> = base64::engine::general_purpose::STANDARD
        .decode(raw.as_bytes())
        .ok()
        .and_then(|decoded| serde_json::from_slice::<Value>(&decoded).ok())
        .and_then(|value| {
            value
                .as_object()
                .map(|object| object.keys().cloned().collect())
        })
        .unwrap_or_default();
    field_names.sort();
    FingerprintAudit {
        status: if field_names.is_empty() {
            "invalid"
        } else {
            "captured"
        }
        .into(),
        sha256: Some(format!("{:x}", Sha256::digest(raw.as_bytes()))),
        length: raw.len(),
        field_count: field_names.len(),
        field_names,
        raw_available_locally: true,
    }
}

#[cfg(target_os = "linux")]
fn platform_details() -> Value {
    let argv0 = std::env::args_os().next().map(PathBuf::from);
    let proc_exe = fs::read_link("/proc/self/exe").ok();
    let desktop_id = "com.masterain.discord-quest-helper.desktop";
    let desktop_installed = runtime_data_root()
        .is_some_and(|root| root.join("applications").join(desktop_id).is_file())
        || Path::new("/usr/share/applications")
            .join(desktop_id)
            .is_file();
    let residuals = ["APPIMAGE", "APPDIR", "ARGV0"]
        .into_iter()
        .filter(|name| std::env::var_os(name).is_some())
        .collect::<Vec<_>>();
    json!({
        "comm": fs::read_to_string("/proc/self/comm").ok().map(|value| value.trim().to_string()),
        "argv0": argv0.as_deref().map(redacted_path),
        "procExe": proc_exe.as_deref().map(redacted_path),
        "desktopFileId": desktop_id,
        "desktopFileInstalled": desktop_installed,
        "windowIdentity": {
            "configured": {
                "x11WmClass": RUNTIME_MAIN_NAME,
                "waylandAppId": RUNTIME_MAIN_NAME,
            },
            "observed": {
                "x11WmClass": Value::Null,
                "waylandAppId": Value::Null,
            },
            "observationStatus": "unavailable",
            "releaseSmoke": {
                "status": "external",
                "manifests": [
                    "identity-smoke-linux-deb-x11.json",
                    "identity-smoke-linux-deb-wayland.json",
                    "identity-smoke-linux-appimage-x11.json",
                    "identity-smoke-linux-appimage-wayland.json",
                ],
            },
        },
        "packageType": if std::env::var_os("APPIMAGE").is_some() { "appimage" } else if proc_exe.as_ref().is_some_and(|path| path.starts_with("/usr/bin")) { "deb" } else { "development" },
        "appImageResidualFields": residuals,
    })
}

#[cfg(target_os = "macos")]
fn macos_bundle() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .map(Path::to_path_buf)
}

#[cfg(target_os = "macos")]
fn plist_value(info_plist: &Path, key: &str) -> Option<String> {
    let output = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", &format!("Print :{key}")])
        .arg(info_plist)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn platform_details() -> Value {
    let bundle = macos_bundle();
    let info = bundle.as_ref().map(|path| path.join("Contents/Info.plist"));
    let main_code_identity = bundle
        .as_deref()
        .and_then(|path| super::macos::read_code_identity(path).ok());
    let authority = main_code_identity.as_ref().and_then(|identity| {
        identity
            .authorities
            .first()
            .cloned()
            .or_else(|| identity.ad_hoc.then(|| "ad-hoc".into()))
    });
    let hardened_runtime = main_code_identity
        .as_ref()
        .is_some_and(|identity| identity.hardened_runtime);
    let nested_helper = bundle
        .as_ref()
        .map(|path| path.join("Contents/MacOS").join(RUNTIME_BRIDGE_NAME));
    let nested_helper_signature_ok = nested_helper.as_ref().map(|path| {
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict", "--verbose=2"])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    });
    let nested_helper_identity = nested_helper
        .as_deref()
        .and_then(|path| super::macos::read_code_identity(path).ok());
    let nested_helper_identity_matches_main = main_code_identity
        .as_ref()
        .zip(nested_helper_identity.as_ref())
        .map(|(main, helper)| {
            let policy = if main.ad_hoc {
                super::macos::SignaturePolicy::SmokeAdHoc
            } else {
                super::macos::SignaturePolicy::ReleaseStrict
            };
            super::macos::validate_related_code_identity(main, helper, policy).is_ok()
        });
    json!({
        "bundlePath": bundle.as_deref().map(redacted_path),
        "cfBundleExecutable": info.as_deref().and_then(|path| plist_value(path, "CFBundleExecutable")),
        "cfBundleDisplayName": info.as_deref().and_then(|path| plist_value(path, "CFBundleDisplayName")),
        "cfBundleIdentifier": info.as_deref().and_then(|path| plist_value(path, "CFBundleIdentifier")),
        "codeSigningAuthority": authority,
        "codeSigningTeamIdentifier": main_code_identity.as_ref().and_then(|identity| identity.team_identifier.clone()),
        "hardenedRuntime": hardened_runtime,
        // Notarization validation can perform network or trust-service work.
        // Keep this interactive command bounded by omitting that probe; release
        // workflows perform stapler validation separately.
        "notarizationStaplerOk": Value::Null,
        "notarizationObservationStatus": "external",
        "nestedHelperSignatureOk": nested_helper_signature_ok,
        "nestedHelperTeamIdentifier": nested_helper_identity.as_ref().and_then(|identity| identity.team_identifier.clone()),
        "nestedHelperHardenedRuntime": nested_helper_identity.as_ref().map(|identity| identity.hardened_runtime),
        "nestedHelperIdentityMatchesMain": nested_helper_identity_matches_main,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn platform_details() -> Value {
    json!({})
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

#[cfg(any(target_os = "linux", test))]
fn nested_string_field<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |current, field| current.get(field))
        .and_then(Value::as_str)
}

#[cfg(any(target_os = "linux", test))]
fn compare_linux_window_identity(
    details: &Value,
    linux_baseline: &Value,
) -> (bool, Option<bool>, Vec<String>, Vec<String>) {
    let fields = [
        ("x11WmClass", "X11 WM_CLASS"),
        ("waylandAppId", "Wayland app_id"),
    ];
    let mut configured_matches = true;
    let mut observed_matches = None;
    let mut unavailable = Vec::new();
    let mut differences = Vec::new();

    for (field, label) in fields {
        let expected = nested_string_field(linux_baseline, &["windowIdentity", field]);
        let configured = nested_string_field(details, &["windowIdentity", "configured", field]);
        if configured != expected {
            configured_matches = false;
            differences.push(format!(
                "configured Linux {label} differs from release baseline"
            ));
        }

        match nested_string_field(details, &["windowIdentity", "observed", field]) {
            Some(observed) => {
                let matches = configured == Some(observed);
                observed_matches = Some(observed_matches.unwrap_or(true) && matches);
                if !matches {
                    differences.push(format!("observed Linux {label} differs from configuration"));
                }
            }
            None => unavailable.push(format!("Linux {label}")),
        }
    }

    (
        configured_matches,
        observed_matches,
        unavailable,
        differences,
    )
}

fn baseline_comparison(
    main: &ExecutableAudit,
    helper: &HelperAudit,
    legacy_artifact_count: usize,
    fingerprint: &FingerprintAudit,
    details: &Value,
) -> BaselineComparison {
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = details;
    let baseline: Value = serde_json::from_str(RELEASE_BASELINE).unwrap_or_else(|_| json!({}));
    let mut differences = Vec::new();
    if main.basename != RUNTIME_MAIN_NAME {
        differences.push("main executable differs from release baseline".into());
    }
    if main.unexpected_path_product_token {
        differences.push("main executable path contains a new product token".into());
    }
    if helper.installed
        && (helper.basename.as_deref() != Some(RUNTIME_BRIDGE_NAME)
            || helper.path_has_product_token == Some(true))
    {
        differences.push("installed helper differs from release baseline".into());
    }
    if helper.installed && helper.manifest_hash_ok != Some(true) {
        differences.push("installed helper manifest failed verification".into());
    }
    if helper.signature_ok == Some(false) {
        differences.push("installed helper signature failed verification".into());
    }
    if legacy_artifact_count > 0 {
        differences.push("legacy runtime artifacts remain".into());
    }
    #[cfg(target_os = "linux")]
    let (
        configured_window_identity_matches,
        observed_window_identity_matches,
        unavailable_observations,
    ) = {
        let linux_baseline = &baseline["linux"];
        if string_field(details, "comm") != Some(RUNTIME_MAIN_NAME) {
            differences.push("Linux comm differs from release baseline".into());
        }
        for field in ["argv0", "procExe"] {
            if string_field(details, field).is_some_and(super::contains_product_token) {
                differences.push(format!("Linux {field} contains a new product token"));
            }
        }
        if string_field(details, "desktopFileId") != string_field(linux_baseline, "desktopFileId") {
            differences.push("Linux desktop file ID differs from release baseline".into());
        }
        let (configured, observed, unavailable, window_differences) =
            compare_linux_window_identity(details, linux_baseline);
        differences.extend(window_differences);
        (Some(configured), observed, unavailable)
    };
    #[cfg(not(target_os = "linux"))]
    let (
        configured_window_identity_matches,
        observed_window_identity_matches,
        unavailable_observations,
    ) = (None, None, Vec::new());
    #[cfg(target_os = "macos")]
    {
        let macos_baseline = &baseline["macos"];
        if details
            .get("cfBundleExecutable")
            .is_some_and(|value| !value.is_null())
        {
            for (actual, expected, label) in [
                (
                    "cfBundleExecutable",
                    "bundleExecutable",
                    "CFBundleExecutable",
                ),
                (
                    "cfBundleDisplayName",
                    "bundleDisplayName",
                    "CFBundleDisplayName",
                ),
                (
                    "cfBundleIdentifier",
                    "bundleIdentifier",
                    "CFBundleIdentifier",
                ),
            ] {
                if string_field(details, actual) != string_field(macos_baseline, expected) {
                    differences.push(format!("{label} differs from release baseline"));
                }
            }
            if details.get("hardenedRuntime").and_then(Value::as_bool) != Some(true) {
                differences.push("macOS hardened runtime is missing".into());
            }
            if details
                .get("nestedHelperSignatureOk")
                .and_then(Value::as_bool)
                != Some(true)
            {
                differences.push("nested helper signature is invalid".into());
            }
            if details
                .get("nestedHelperIdentityMatchesMain")
                .and_then(Value::as_bool)
                != Some(true)
            {
                differences.push("nested helper signing identity differs from the main app".into());
            }
        }
    }

    let expected_fields = baseline
        .get("fingerprintFields")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    let actual_fields = fingerprint
        .field_names
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let fingerprint_fields_added = actual_fields
        .difference(&expected_fields)
        .cloned()
        .collect::<Vec<_>>();
    let fingerprint_fields_removed = if fingerprint.status == "captured" {
        expected_fields
            .difference(&actual_fields)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if !fingerprint_fields_added.is_empty() || !fingerprint_fields_removed.is_empty() {
        differences.push("native fingerprint fields changed from release baseline".into());
    }
    BaselineComparison {
        matches: differences.is_empty(),
        differences,
        configured_window_identity_matches,
        observed_window_identity_matches,
        unavailable_observations,
        fingerprint_fields_added,
        fingerprint_fields_removed,
    }
}

pub fn collect(
    fingerprint_raw: Option<String>,
    status: RuntimeIdentityStatus,
) -> RuntimeIdentityAudit {
    let main = main_executable_audit();
    let (helper, legacy_artifact_count, mut migration_result) = helper_audit();
    if status
        .reasons
        .iter()
        .any(|reason| reason.starts_with("runtime bridge:"))
    {
        migration_result = "degraded".into();
    }
    let fingerprint = fingerprint_audit(fingerprint_raw);
    let platform_details = platform_details();
    let baseline = baseline_comparison(
        &main,
        &helper,
        legacy_artifact_count,
        &fingerprint,
        &platform_details,
    );
    RuntimeIdentityAudit {
        schema_version: 2,
        captured_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        platform: std::env::consts::OS.into(),
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .into(),
        status,
        main,
        helper,
        legacy_artifact_count,
        migration_result,
        fingerprint,
        platform_details,
        baseline,
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::RUNTIME_NAMESPACE;
    use super::*;

    #[test]
    fn release_baseline_matches_compiled_runtime_names() {
        let baseline: Value = serde_json::from_str(RELEASE_BASELINE).unwrap();
        assert_eq!(baseline["mainExecutable"], RUNTIME_MAIN_NAME);
        assert_eq!(baseline["helperExecutable"], RUNTIME_BRIDGE_NAME);
        assert_eq!(baseline["runtimeNamespace"], RUNTIME_NAMESPACE);
    }

    #[test]
    fn fingerprint_summary_never_serializes_the_raw_value() {
        let raw = base64::engine::general_purpose::STANDARD
            .encode(br#"{"os":"Mac OS X","browser":"Discord Client","token":"must-not-leak"}"#);
        let summary = fingerprint_audit(Some(raw.clone()));
        let encoded = serde_json::to_string(&summary).unwrap();
        assert!(summary.sha256.is_some());
        assert!(!encoded.contains(&raw));
        assert!(!encoded.contains("must-not-leak"));
    }

    #[test]
    fn home_paths_are_redacted() {
        if let Some(home) = home_dir() {
            assert_eq!(
                redacted_path(&home.join("private/file")),
                "$HOME/private/file"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renamed_macos_bundle_name_is_not_an_unexpected_runtime_token() {
        assert!(!unexpected_macos_path_product_token(Path::new(
            "/Applications/DQH.app/Contents/MacOS/meridian"
        )));
        assert!(unexpected_macos_path_product_token(Path::new(
            "/opt/discord-quest-helper/DQH.app/Contents/MacOS/meridian"
        )));
    }

    #[test]
    fn collected_audit_does_not_export_the_real_home_path() {
        let audit = collect(
            None,
            RuntimeIdentityStatus::disabled("test", "test fixture"),
        );
        let encoded = serde_json::to_string(&audit).unwrap();
        if let Some(home) = home_dir() {
            assert!(!encoded.contains(&home.to_string_lossy().to_string()));
        }
        assert!(!encoded.to_ascii_lowercase().contains("authorization"));
    }

    fn window_identity_details(x11: Value, wayland: Value) -> Value {
        json!({
            "windowIdentity": {
                "configured": {
                    "x11WmClass": "meridian",
                    "waylandAppId": "meridian"
                },
                "observed": {
                    "x11WmClass": x11,
                    "waylandAppId": wayland
                }
            }
        })
    }

    fn window_identity_baseline() -> Value {
        json!({
            "windowIdentity": {
                "x11WmClass": "meridian",
                "waylandAppId": "meridian"
            }
        })
    }

    #[test]
    fn configured_window_identity_is_not_reported_as_observed() {
        let (configured, observed, unavailable, differences) = compare_linux_window_identity(
            &window_identity_details(Value::Null, Value::Null),
            &window_identity_baseline(),
        );
        assert!(configured);
        assert_eq!(observed, None);
        assert_eq!(unavailable.len(), 2);
        assert!(differences.is_empty());
    }

    #[test]
    fn observed_window_identity_mismatch_fails_comparison() {
        let (_, observed, unavailable, differences) = compare_linux_window_identity(
            &window_identity_details(json!("discord-quest-helper"), json!("meridian")),
            &window_identity_baseline(),
        );
        assert_eq!(observed, Some(false));
        assert!(unavailable.is_empty());
        assert_eq!(differences.len(), 1);
    }

    #[test]
    fn matching_observed_window_identity_passes_comparison() {
        let (_, observed, unavailable, differences) = compare_linux_window_identity(
            &window_identity_details(json!("meridian"), json!("meridian")),
            &window_identity_baseline(),
        );
        assert_eq!(observed, Some(true));
        assert!(unavailable.is_empty());
        assert!(differences.is_empty());
    }
}

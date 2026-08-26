use super::model::{RuntimeIdentityLevel, RuntimeIdentityStatus, RUNTIME_MAIN_NAME};
use once_cell::sync::OnceCell;
use std::fs;
use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PUBLIC_APP_DISPLAY_NAME: &str = "Discord Quest Helper";
const LEGACY_DIR_HEX_LEN: usize = 16;
const LEGACY_EXE_HEX_LEN: usize = 12;
const BUNDLE_IDENTIFIER: &str = "com.masterain.discord-quest-helper";

static VERIFIED_CURRENT_APP_IDENTITY: OnceCell<Result<MacCodeIdentity, String>> = OnceCell::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacCodeIdentity {
    pub identifier: Option<String>,
    pub team_identifier: Option<String>,
    pub authorities: Vec<String>,
    pub hardened_runtime: bool,
    pub ad_hoc: bool,
}

fn parse_code_identity(details: &str) -> MacCodeIdentity {
    let field = |name: &str| {
        details
            .lines()
            .find_map(|line| line.strip_prefix(name).map(str::trim))
            .filter(|value| !value.is_empty() && *value != "not set")
            .map(str::to_string)
    };
    MacCodeIdentity {
        identifier: field("Identifier="),
        team_identifier: field("TeamIdentifier="),
        authorities: details
            .lines()
            .filter_map(|line| line.strip_prefix("Authority=").map(str::to_string))
            .collect(),
        hardened_runtime: details
            .lines()
            .any(|line| line.contains("flags=") && line.contains("runtime")),
        ad_hoc: details.lines().any(|line| line.trim() == "Signature=adhoc"),
    }
}

pub(crate) fn read_code_identity(path: &Path) -> Result<MacCodeIdentity, String> {
    let output = Command::new("/usr/bin/codesign")
        .args(["-dvv"])
        .arg(path)
        .output()
        .map_err(|_| "macOS code-signing identity tool is unavailable".to_string())?;
    if !output.status.success() {
        return Err("code-signing identity could not be read".into());
    }
    Ok(parse_code_identity(&String::from_utf8_lossy(
        &output.stderr,
    )))
}

pub(crate) fn validate_related_code_identity(
    main: &MacCodeIdentity,
    helper: &MacCodeIdentity,
) -> Result<(), String> {
    if !main.hardened_runtime || !helper.hardened_runtime {
        return Err("main app or runtime bridge signature is missing hardened runtime".into());
    }
    if !main.ad_hoc || !helper.ad_hoc {
        return Err("main app and runtime bridge must both use ad-hoc signatures".into());
    }
    Ok(())
}

pub(crate) fn verify_helper_identity_for_current_app(helper: &Path) -> Result<(), String> {
    let main_identity = verified_current_app_identity()?;
    let helper_identity = read_code_identity(helper)?;
    validate_related_code_identity(main_identity, &helper_identity)
}

fn verified_current_app_identity() -> Result<&'static MacCodeIdentity, String> {
    VERIFIED_CURRENT_APP_IDENTITY
        .get_or_init(|| {
            let executable = std::env::current_exe()
                .map_err(|_| "main application executable path is unavailable".to_string())?;
            let bundle = app_bundle_for_executable(&executable)
                .ok_or_else(|| "main application bundle identity is unavailable".to_string())?;
            verify_bundle_signature(&bundle)?;
            read_code_identity(&bundle)
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(super) fn initial_status() -> RuntimeIdentityStatus {
    let cleanup_reasons = cleanup_legacy_temp_runtimes();
    if cfg!(debug_assertions) {
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
        Some(bundle) => {
            if let Err(reason) = verify_bundle_metadata(&bundle) {
                status.main_executable_ok = false;
                status.reasons.push(reason);
            }
            match verified_current_app_identity() {
                Ok(_) => status.package_signature_ok = Some(true),
                Err(reason) => {
                    status.package_signature_ok = Some(false);
                    status.reasons.push(reason);
                }
            }
        }
        None => {
            status.package_signature_ok = Some(false);
            status
                .reasons
                .push("release process is not running from an application bundle".into());
        }
    }
    status.reasons.extend(cleanup_reasons);
    status.recompute_level();
    status
}

pub(crate) fn app_bundle_for_executable(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    if macos.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    (app.extension().is_some_and(|extension| extension == "app")).then(|| app.to_path_buf())
}

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

fn validate_bundle_metadata(
    executable: Option<&str>,
    identifier: Option<&str>,
    display_name: Option<&str>,
) -> Result<(), String> {
    if executable != Some(RUNTIME_MAIN_NAME) {
        return Err("CFBundleExecutable does not match the configured runtime identity".into());
    }
    if identifier != Some(BUNDLE_IDENTIFIER) {
        return Err("CFBundleIdentifier does not match the public application identity".into());
    }
    if display_name != Some(PUBLIC_APP_DISPLAY_NAME) {
        return Err("CFBundleDisplayName does not match the public application identity".into());
    }
    Ok(())
}

fn verify_bundle_metadata(bundle: &Path) -> Result<(), String> {
    let info = bundle.join("Contents/Info.plist");
    validate_bundle_metadata(
        plist_value(&info, "CFBundleExecutable").as_deref(),
        plist_value(&info, "CFBundleIdentifier").as_deref(),
        plist_value(&info, "CFBundleDisplayName").as_deref(),
    )
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

    let identity = read_code_identity(bundle)?;
    if !identity.ad_hoc {
        return Err("application bundle does not use the configured ad-hoc signature".into());
    }
    if !identity.hardened_runtime {
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
    let Some(name) = executable.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let output = Command::new("/usr/bin/pgrep").args(["-x", name]).output();
    let Ok(output) = output else {
        return true;
    };
    if output.status.code() == Some(1) {
        return false;
    }
    if !output.status.success() {
        return true;
    }

    for pid in String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
    {
        let Some(actual) = process_path(pid) else {
            // A candidate that cannot be inspected is treated as active. It is
            // safer to retain a verified legacy directory than delete live code.
            return true;
        };
        match (fs::canonicalize(&actual), fs::canonicalize(executable)) {
            (Ok(actual), Ok(expected)) if actual == expected => return true,
            _ if actual == executable => return true,
            _ => {}
        }
    }
    false
}

fn process_path(pid: u32) -> Option<PathBuf> {
    let mut buffer = Vec::<u8>::with_capacity(libc::PROC_PIDPATHINFO_MAXSIZE as usize);
    // SAFETY: the buffer exposes the requested writable capacity for the
    // duration of proc_pidpath, which returns the initialized byte count.
    let length = unsafe {
        libc::proc_pidpath(
            pid as libc::c_int,
            buffer.as_mut_ptr().cast(),
            libc::PROC_PIDPATHINFO_MAXSIZE as u32,
        )
    };
    if length <= 0 {
        return None;
    }
    // SAFETY: proc_pidpath initialized `length` bytes on success.
    unsafe { buffer.set_len(length as usize) };
    Some(PathBuf::from(std::ffi::OsString::from_vec(buffer)))
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
        reasons.push(
            "current executable filename does not match the configured runtime identity".into(),
        );
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

    fn identity(hardened: bool, ad_hoc: bool) -> MacCodeIdentity {
        MacCodeIdentity {
            identifier: Some("fixture".into()),
            team_identifier: None,
            authorities: Vec::new(),
            hardened_runtime: hardened,
            ad_hoc,
        }
    }

    #[test]
    fn parses_codesign_identity_fields() {
        let parsed = parse_code_identity(
            "Identifier=com.example.app\nTeamIdentifier=not set\nSignature=adhoc\nflags=0x10000(runtime)\n",
        );
        assert_eq!(parsed.identifier.as_deref(), Some("com.example.app"));
        assert_eq!(parsed.team_identifier, None);
        assert!(parsed.authorities.is_empty());
        assert!(parsed.hardened_runtime);
        assert!(parsed.ad_hoc);
    }

    #[test]
    fn identity_policy_accepts_only_hardened_ad_hoc_app_and_helper() {
        let ad_hoc = identity(true, true);
        assert!(validate_related_code_identity(&ad_hoc, &ad_hoc).is_ok());

        let unsigned = identity(true, false);
        assert_eq!(
            validate_related_code_identity(&ad_hoc, &unsigned).unwrap_err(),
            "main app and runtime bridge must both use ad-hoc signatures"
        );

        let unhardened = identity(false, true);
        assert_eq!(
            validate_related_code_identity(&ad_hoc, &unhardened).unwrap_err(),
            "main app or runtime bridge signature is missing hardened runtime"
        );
    }

    #[test]
    fn bundle_executable_is_checked_independently_from_public_app_name() {
        assert_eq!(
            status_for_executable(Some(Path::new(
                "/Applications/Discord Quest Helper.app/Contents/MacOS/meridian"
            )))
            .level,
            RuntimeIdentityLevel::Full
        );
        let status = status_for_executable(Some(Path::new(
            "/Applications/Discord Quest Helper.app/Contents/MacOS/wrong-name",
        )));
        assert_eq!(
            status.reasons,
            ["current executable filename does not match the configured runtime identity"]
        );
        assert_eq!(
            validate_bundle_metadata(
                Some("wrong-name"),
                Some(BUNDLE_IDENTIFIER),
                Some(PUBLIC_APP_DISPLAY_NAME),
            )
            .unwrap_err(),
            "CFBundleExecutable does not match the configured runtime identity"
        );
    }

    #[test]
    fn bundle_path_accepts_a_renamed_app_but_rejects_a_bare_executable() {
        let bundled = Path::new("/Applications/Discord Quest Helper.app/Contents/MacOS/meridian");
        assert_eq!(
            app_bundle_for_executable(bundled),
            Some(PathBuf::from("/Applications/Discord Quest Helper.app"))
        );
        let renamed = Path::new("/Applications/DQH.app/Contents/MacOS/meridian");
        assert_eq!(
            app_bundle_for_executable(renamed),
            Some(PathBuf::from("/Applications/DQH.app"))
        );
        assert!(app_bundle_for_executable(Path::new("/tmp/meridian")).is_none());
        assert!(
            app_bundle_for_executable(Path::new("/Applications/DQH/Contents/MacOS/meridian"))
                .is_none()
        );
    }

    #[test]
    fn renamed_bundle_still_requires_the_canonical_internal_metadata() {
        assert!(validate_bundle_metadata(
            Some("meridian"),
            Some("com.masterain.discord-quest-helper"),
            Some("Discord Quest Helper")
        )
        .is_ok());
        assert!(validate_bundle_metadata(
            Some("meridian"),
            Some("com.example.renamed"),
            Some("Discord Quest Helper")
        )
        .is_err());
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

    #[test]
    fn process_path_reports_the_full_current_executable() {
        let reported = process_path(std::process::id()).unwrap();
        let expected = std::env::current_exe().unwrap();
        assert_eq!(
            fs::canonicalize(reported).unwrap(),
            fs::canonicalize(expected).unwrap()
        );
    }
}

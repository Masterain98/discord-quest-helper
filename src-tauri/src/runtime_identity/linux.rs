use super::model::{RuntimeIdentityLevel, RuntimeIdentityStatus, RUNTIME_MAIN_NAME};
use std::path::Path;

pub(super) fn initial_status() -> RuntimeIdentityStatus {
    if cfg!(debug_assertions)
        || std::env::var_os("RUNTIME_IDENTITY_MODE").as_deref() == Some(std::ffi::OsStr::new("off"))
    {
        return RuntimeIdentityStatus::disabled(
            "linux",
            "runtime identity minimization is disabled for this development process",
        );
    }

    status_for_executable(std::env::current_exe().ok().as_deref())
}

fn status_for_executable(executable: Option<&Path>) -> RuntimeIdentityStatus {
    let mut reasons = Vec::new();
    let main_executable_ok = executable
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(|name| name == RUNTIME_MAIN_NAME)
        .unwrap_or(false);
    if !main_executable_ok {
        reasons
            .push("main executable basename does not match the configured runtime identity".into());
    }
    RuntimeIdentityStatus {
        platform: "linux".into(),
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
    fn product_named_release_path_is_degraded() {
        assert_eq!(
            status_for_executable(Some(Path::new("/usr/bin/discord-quest-helper"))).level,
            RuntimeIdentityLevel::Degraded
        );
    }

    #[test]
    fn neutral_release_path_is_full_before_optional_checks() {
        assert_eq!(
            status_for_executable(Some(Path::new("/usr/bin/meridian"))).level,
            RuntimeIdentityLevel::Full
        );
    }
}

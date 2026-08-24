use super::model::{
    valid_internal_name, RuntimeIdentityLevel, RuntimeIdentityStatus, RUNTIME_MAIN_NAME,
};
use std::ffi::CString;
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

    let mut status = status_for_executable(std::env::current_exe().ok().as_deref());
    if let Err(reason) = set_main_thread_name(RUNTIME_MAIN_NAME) {
        status.reasons.push(reason);
        status.level = RuntimeIdentityLevel::Degraded;
    }
    status
}

fn set_main_thread_name(name: &str) -> Result<(), String> {
    if !valid_internal_name(name) || name.len() > 15 {
        return Err("configured Linux process name violates the runtime identity policy".into());
    }
    let name = CString::new(name)
        .map_err(|_| "configured Linux process name contains an interior NUL".to_string())?;
    // SAFETY: PR_SET_NAME reads a NUL-terminated string from the second
    // argument and ignores the remaining variadic arguments. `name` remains
    // alive for the duration of the call.
    let result = unsafe { libc::prctl(libc::PR_SET_NAME, name.as_ptr(), 0, 0, 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "failed to set Linux process name: {}",
            std::io::Error::last_os_error()
        ))
    }
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

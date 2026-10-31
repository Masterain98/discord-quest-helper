//! Desktop equivalents of DiscordNative.os and navigator.languages.
use serde_json::{Map, Value};

pub fn arch(value: &str) -> &str {
    match value {
        "x86_64" | "AMD64" => "x64",
        "x86" | "i686" => "ia32",
        "aarch64" | "ARM64" => "arm64",
        value => value,
    }
}

pub fn locale() -> String {
    #[cfg(windows)]
    {
        let mut buffer = [0u16; 85];
        let count = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buffer) };
        if count > 1 {
            return String::from_utf16_lossy(&buffer[..count as usize - 1]);
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLocale"])
        .output()
    {
        if output.status.success() {
            let locale = String::from_utf8_lossy(&output.stdout)
                .trim()
                .replace('_', "-");
            if !locale.is_empty() {
                return locale;
            }
        }
    }
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .map(|value| {
            value
                .split(['.', '@'])
                .next()
                .unwrap_or("")
                .replace('_', "-")
        })
        .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
        .unwrap_or_else(|| "en-US".into())
}

pub fn release() -> String {
    #[cfg(windows)]
    if let Ok(key) =
        windows_registry::LOCAL_MACHINE.open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
    {
        if let (Ok(major), Ok(minor), Ok(build)) = (
            key.get_u32("CurrentMajorVersionNumber"),
            key.get_u32("CurrentMinorVersionNumber"),
            key.get_string("CurrentBuildNumber"),
        ) {
            return format!("{major}.{minor}.{build}");
        }
    }
    #[cfg(unix)]
    unsafe {
        let mut info: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut info) == 0 {
            return std::ffi::CStr::from_ptr(info.release.as_ptr())
                .to_string_lossy()
                .into_owned();
        }
    }
    if cfg!(windows) {
        super::DEFAULT_OS_VERSION.into()
    } else {
        String::new()
    }
}

pub fn os_arch() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{
            GetNativeSystemInfo, PROCESSOR_ARCHITECTURE_AMD64, PROCESSOR_ARCHITECTURE_ARM,
            PROCESSOR_ARCHITECTURE_ARM64, PROCESSOR_ARCHITECTURE_INTEL, SYSTEM_INFO,
        };
        let mut info = SYSTEM_INFO::default();
        unsafe {
            GetNativeSystemInfo(&mut info);
        }
        match unsafe { info.Anonymous.Anonymous.wProcessorArchitecture } {
            PROCESSOR_ARCHITECTURE_AMD64 => return "x64".into(),
            PROCESSOR_ARCHITECTURE_ARM64 => return "arm64".into(),
            PROCESSOR_ARCHITECTURE_INTEL => return "ia32".into(),
            PROCESSOR_ARCHITECTURE_ARM => return "arm".into(),
            _ => {}
        }
    }
    #[cfg(unix)]
    unsafe {
        let mut info: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut info) == 0 {
            return arch(&std::ffi::CStr::from_ptr(info.machine.as_ptr()).to_string_lossy()).into();
        }
    }
    arch(std::env::consts::ARCH).into()
}

pub fn platform_fields(properties: &mut Map<String, Value>) {
    #[cfg(target_os = "linux")]
    {
        for (key, env) in [
            ("window_manager", "XDG_CURRENT_DESKTOP"),
            ("display_server", "XDG_SESSION_TYPE"),
        ] {
            if let Ok(value) = std::env::var(env) {
                properties.insert(key.into(), value.into());
            }
        }
        if let Ok(contents) = std::fs::read_to_string("/etc/os-release") {
            if let Some(value) = contents
                .lines()
                .find_map(|line| line.strip_prefix("PRETTY_NAME="))
            {
                properties.insert("distro".into(), value.trim_matches('"').into());
            }
        }
        if std::env::var_os("FLATPAK_ID").is_some() {
            properties.insert("runtime_environment".into(), "flatpak".into());
        }
    }
    let _ = properties;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_system_snapshot_is_usable() {
        assert!(!release().is_empty());
        assert!(!locale().is_empty());
        assert!(matches!(
            os_arch().as_str(),
            "x64" | "arm64" | "ia32" | "arm"
        ));
        #[cfg(windows)]
        {
            let key = windows_registry::LOCAL_MACHINE
                .open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
                .unwrap();
            assert_eq!(
                release().split('.').nth(2),
                Some(key.get_string("CurrentBuildNumber").unwrap().as_str())
            );
        }
    }
}

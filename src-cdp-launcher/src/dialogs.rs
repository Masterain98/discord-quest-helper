#[cfg(target_os = "windows")]
mod win32 {
    pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
    pub const MB_OK: u32 = 0;
    pub const MB_YESNO: u32 = 0x0000_0004;
    pub const MB_ICONQUESTION: u32 = 0x0000_0020;
    pub const MB_ICONERROR: u32 = 0x0000_0010;
    pub const IDYES: i32 = 6;

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn SetProcessDpiAwarenessContext(value: isize) -> i32;
        pub fn GetUserDefaultUILanguage() -> u16;
        pub fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            r#type: u32,
        ) -> i32;
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn enable_dpi_awareness() {
    unsafe {
        let _ =
            win32::SetProcessDpiAwarenessContext(win32::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn enable_dpi_awareness() {}

#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
const LINUX_ICON_NAME: &str = "com.masterain.discord-quest-helper.cdp";

#[cfg(target_os = "linux")]
fn zenity_icon_flag() -> &'static str {
    let version = Command::new("zenity")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|version| {
            let mut components = version.trim().split('.');
            Some((
                components.next()?.parse::<u32>().ok()?,
                components.next()?.parse::<u32>().ok()?,
            ))
        });

    // Ubuntu 22.04's GTK3 Zenity accepts --icon-name, whereas the newer
    // libadwaita builds use --icon. Keep both supported release families from
    // rejecting the whole dialog just because its decorative icon is unknown.
    match version {
        Some((major, _)) if major >= 4 => "--icon",
        Some((3, minor)) if minor >= 90 => "--icon",
        _ => "--icon-name",
    }
}

#[cfg(target_os = "linux")]
fn zenity_dialog(kind: &str, title: &str, message: &str) -> Command {
    let mut command = Command::new("zenity");
    command.args([
        kind,
        "--title",
        title,
        "--text",
        message,
        zenity_icon_flag(),
        LINUX_ICON_NAME,
        "--no-wrap",
    ]);
    command
}

#[cfg(target_os = "windows")]
pub(crate) fn system_ui_language() -> u16 {
    unsafe { win32::GetUserDefaultUILanguage() }
}

#[cfg(target_os = "windows")]
pub(crate) fn show_error_dialog(title: &str, message: &str) {
    let title = to_wide(title);
    let message = to_wide(message);
    unsafe {
        win32::MessageBoxW(
            core::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            win32::MB_OK | win32::MB_ICONERROR,
        );
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn show_info_dialog(title: &str, message: &str) {
    let title = to_wide(title);
    let message = to_wide(message);
    unsafe {
        win32::MessageBoxW(
            core::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            win32::MB_OK,
        );
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn show_info_dialog(title: &str, message: &str) {
    let result = zenity_dialog("--info", title, message).status();
    if result.is_ok_and(|status| status.success()) {
        return;
    }
    for (program, args) in [
        ("kdialog", vec!["--title", title, "--msgbox", message]),
        ("xmessage", vec!["-title", title, "-center", message]),
    ] {
        if Command::new(program)
            .args(args)
            .status()
            .is_ok_and(|status| status.success())
        {
            return;
        }
    }
    eprintln!("{title}: {message}");
}

#[cfg(target_os = "macos")]
pub(crate) fn show_error_dialog(title: &str, message: &str) {
    show_macos_dialog(title, message, true);
}

#[cfg(target_os = "macos")]
pub(crate) fn show_info_dialog(title: &str, message: &str) {
    show_macos_dialog(title, message, false);
}

#[cfg(target_os = "linux")]
pub(crate) fn show_error_dialog(title: &str, message: &str) {
    if zenity_dialog("--error", title, message)
        .status()
        .is_ok_and(|status| status.success())
    {
        return;
    }
    show_info_dialog(title, message);
}

#[cfg(target_os = "macos")]
fn show_macos_dialog(title: &str, message: &str, critical: bool) {
    let severity = if critical { " as critical" } else { "" };
    let script = format!(
        "display alert \"{}\" message \"{}\"{} buttons {{\"OK\"}} default button \"OK\"",
        applescript_string(title),
        applescript_string(message),
        severity
    );
    if !std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .status()
        .is_ok_and(|status| status.success())
    {
        eprintln!("{title}: {message}");
    }
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub(crate) fn show_info_dialog(title: &str, message: &str) {
    eprintln!("{title}: {message}");
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub(crate) fn show_error_dialog(title: &str, message: &str) {
    show_info_dialog(title, message);
}

#[cfg(target_os = "windows")]
pub(crate) fn show_confirm_dialog(title: &str, message: &str) -> Result<bool, String> {
    let title = to_wide(title);
    let message = to_wide(message);
    Ok(unsafe {
        win32::MessageBoxW(
            core::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            win32::MB_YESNO | win32::MB_ICONQUESTION,
        ) == win32::IDYES
    })
}

#[cfg(target_os = "linux")]
pub(crate) fn show_confirm_dialog(title: &str, message: &str) -> Result<bool, String> {
    zenity_dialog("--question", title, message)
        .status()
        .map_err(|error| format!("Could not show the Zenity confirmation dialog: {error}"))
        .and_then(|status| match status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            Some(code) => Err(format!(
                "The Zenity confirmation dialog failed with exit code {code}."
            )),
            None => Err("The Zenity confirmation dialog was terminated by a signal.".to_string()),
        })
}

#[cfg(target_os = "macos")]
pub(crate) fn show_confirm_dialog(title: &str, message: &str) -> Result<bool, String> {
    let script = format!(
        "button returned of (display alert \"{}\" message \"{}\" buttons {{\"Cancel\", \"Restart\"}} default button \"Restart\")",
        applescript_string(title),
        applescript_string(message),
    );
    let output = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()
        .map_err(|error| format!("Could not show the macOS confirmation dialog: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "The macOS confirmation dialog failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "Restart")
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::applescript_string;

    #[test]
    fn escapes_applescript_string_delimiters() {
        assert_eq!(applescript_string(r#"a\"b\\c"#), r#"a\\\"b\\\\c"#);
    }
}

#[cfg(target_os = "windows")]
fn to_wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

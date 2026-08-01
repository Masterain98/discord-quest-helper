#[cfg(target_os = "windows")]
mod win32 {
    pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
    pub const MB_OK: u32 = 0;
    pub const MB_YESNO: u32 = 0x0000_0004;
    pub const MB_ICONQUESTION: u32 = 0x0000_0020;
    pub const MB_ICONINFORMATION: u32 = 0x0000_0040;
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
pub(crate) fn show_info_dialog(title: &str, message: &str) {
    let title = to_wide(title);
    let message = to_wide(message);
    unsafe {
        win32::MessageBoxW(
            core::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            win32::MB_OK | win32::MB_ICONINFORMATION,
        );
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn show_info_dialog(title: &str, message: &str) {
    let result = zenity_dialog("--info", title, message).status();
    if !result.is_ok_and(|status| status.success()) {
        eprintln!("{title}: {message}");
    }
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
        .map(|status| status.success())
        .map_err(|error| format!("Could not show the Zenity confirmation dialog: {error}"))
}

#[cfg(target_os = "windows")]
fn to_wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

//! Stealth Mode Module
//!
//! Release builds copy the main executable into a randomly named temp
//! directory and relaunch from that copy so Discord's process scanner does
//! not see the installed product name.

#![cfg_attr(any(debug_assertions, target_os = "linux"), allow(dead_code))]

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// Hex directory name length under `temp_dir`.
pub(crate) const DIR_HEX_LEN: usize = 16;
/// Hex executable stem length inside the stealth directory.
pub(crate) const FILE_HEX_LEN: usize = 12;
/// Legacy prefix from the previous `svc_<hex>.exe` layout.
const LEGACY_SVC_PREFIX: &str = "svc_";
const WEBVIEW_DATA_DIR_NAME: &str = "ud";

/// Flag indicating if current process is running in stealth mode
static IS_STEALTH_MODE: AtomicBool = AtomicBool::new(false);

/// Generate random hexadecimal string
pub(crate) fn generate_random_suffix(length: usize) -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..length)
        .map(|_| format!("{:x}", rng.random::<u8>() % 16))
        .collect()
}

/// Get executable file extension
#[cfg(target_os = "windows")]
fn get_exe_extension() -> &'static str {
    ".exe"
}

#[cfg(not(target_os = "windows"))]
fn get_exe_extension() -> &'static str {
    ""
}

pub(crate) fn is_hex_str(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Drop the NTFS MotW stream copied from a downloaded/installed exe.
/// Missing ADS is ignored.
pub(crate) fn strip_zone_identifier(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::DeleteFileW;

        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.extend(":Zone.Identifier".encode_utf16());
        wide.push(0);
        let _ = unsafe { DeleteFileW(PCWSTR(wide.as_ptr())) };
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
    }
}

pub(crate) fn paths_eq(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(left), Ok(right)) => left == right,
        _ => a == b,
    }
}

/// True when `exe` lives at `%TEMP%/<16 hex>/<12 hex>[.exe]`.
fn is_stealth_copy_path(exe: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        match exe.extension().and_then(|ext| ext.to_str()) {
            Some("exe") => {}
            _ => return false,
        }
    }

    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if !is_hex_str(stem, FILE_HEX_LEN) {
        return false;
    }

    let parent = match exe.parent() {
        Some(path) => path,
        None => return false,
    };
    let parent_name = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if !is_hex_str(parent_name, DIR_HEX_LEN) {
        return false;
    }

    let temp_parent = match parent.parent() {
        Some(path) => path,
        None => return false,
    };
    paths_eq(temp_parent, &env::temp_dir())
}

fn window_title_for_exe(exe: &Path) -> String {
    exe.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Runtime")
        .to_string()
}

fn webview_user_data_dir_for_exe(exe: &Path) -> Option<PathBuf> {
    exe.parent().map(|dir| dir.join(WEBVIEW_DATA_DIR_NAME))
}

fn is_legacy_svc_file_name(name: &str) -> bool {
    if !name.starts_with(LEGACY_SVC_PREFIX) {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        name.ends_with(".exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        !name.contains('.')
    }
}

fn is_legacy_cleanup_bat(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".bat") else {
        return false;
    };
    stem.starts_with("cleanup_")
}

fn is_removable_legacy_temp_file(name: &str) -> bool {
    is_legacy_svc_file_name(name) || is_legacy_cleanup_bat(name)
}

fn remove_legacy_temp_file_if_needed(path: &Path) {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    if !is_removable_legacy_temp_file(name) {
        return;
    }
    match fs::remove_file(path) {
        Ok(()) => println!("[Stealth] Cleaned up legacy file: {}", name),
        Err(err) => {
            if cfg!(debug_assertions) {
                eprintln!("[Stealth] Failed to clean up {}: {}", name, err);
            }
        }
    }
}

fn dir_contains_hex_stealth_exe(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_file() && is_stealth_copy_path(&path)
    })
}

/// Owned stealth trees also have the WebView2 `ud/` directory. Requiring that
/// marker avoids deleting unrelated `%TEMP%/<16 hex>/` folders that happen to
/// contain a 12-hex executable.
fn dir_looks_like_stealth_copy(dir: &Path) -> bool {
    dir.join(WEBVIEW_DATA_DIR_NAME).is_dir() && dir_contains_hex_stealth_exe(dir)
}

/// Check if currently running in stealth mode
pub fn is_stealth_mode() -> bool {
    IS_STEALTH_MODE.load(Ordering::Relaxed)
}

/// Window title for the stealth copy: the executable stem.
pub fn generate_stealth_window_title() -> String {
    env::current_exe()
        .ok()
        .map(|path| window_title_for_exe(&path))
        .unwrap_or_else(|| "Runtime".to_string())
}

/// WebView2 user-data directory beside the stealth copy (`ud/`).
pub fn webview_user_data_dir() -> Option<PathBuf> {
    if !is_stealth_mode() {
        return None;
    }
    env::current_exe()
        .ok()
        .and_then(|path| webview_user_data_dir_for_exe(&path))
}

/// Set process identity that windowing APIs read before the first window.
pub fn apply_process_identity() {
    #[cfg(target_os = "windows")]
    if is_stealth_mode() {
        let app_id = generate_stealth_window_title();
        let app_id = windows::core::HSTRING::from(app_id.as_str());
        if let Err(err) =
            unsafe { windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(&app_id) }
        {
            eprintln!("[Stealth] Failed to set AppUserModelID: {err}");
        }
    }
}

/// Ensure running in stealth mode
///
/// Returns:
/// - `true`: Continue execution (already in stealth mode or successfully launched stealth process)
/// - `false`: Cannot enter stealth mode, but can continue with original name
///
/// If stealth process launched successfully, this function calls `std::process::exit(0)`
pub fn ensure_stealth_mode() -> bool {
    // Stealth relaunches the app from a randomly named copy in the temp dir.
    // On Linux that breaks AppImage mounts, sidecar/resource resolution and the
    // Desktop Entry identity, so it is disabled for the first Linux release.
    #[cfg(target_os = "linux")]
    {
        println!("[Stealth] Disabled on Linux");
        true
    }

    // Skip stealth mode in debug builds
    #[cfg(all(debug_assertions, not(target_os = "linux")))]
    {
        println!("[Stealth] Debug mode - skipping stealth");
        true
    }

    #[cfg(all(not(debug_assertions), not(target_os = "linux")))]
    {
        ensure_stealth_mode_impl()
    }
}

#[cfg(not(debug_assertions))]
fn ensure_stealth_mode_impl() -> bool {
    let current_exe = match env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("[Stealth] Failed to get current exe path: {}", err);
            return true;
        }
    };

    if is_stealth_copy_path(&current_exe) {
        IS_STEALTH_MODE.store(true, Ordering::Relaxed);
        println!(
            "[Stealth] Running in stealth mode as: {}",
            current_exe.display()
        );
        cleanup_old_stealth_copies(&current_exe);
        return true;
    }

    println!("[Stealth] Starting stealth mode transition...");

    let dir_name = generate_random_suffix(DIR_HEX_LEN);
    let file_stem = generate_random_suffix(FILE_HEX_LEN);
    let ext = get_exe_extension();
    let stealth_dir = env::temp_dir().join(&dir_name);
    let dest_name = format!("{file_stem}{ext}");
    let dest_exe = stealth_dir.join(&dest_name);

    if let Err(err) = fs::create_dir_all(&stealth_dir) {
        eprintln!("[Stealth] Failed to create stealth directory: {}", err);
        return true;
    }

    println!("[Stealth] Copying to: {:?}", dest_exe);

    if let Err(err) = fs::copy(&current_exe, &dest_exe) {
        eprintln!("[Stealth] Failed to copy to temp: {}", err);
        let _ = fs::remove_dir_all(&stealth_dir);
        return true;
    }

    // copy -> MOTW strip -> PE rewrite -> CreateProcess -> exit. No Tauri
    // or window setup in this parent process.
    strip_zone_identifier(&dest_exe);

    #[cfg(target_os = "windows")]
    {
        if let Err(err) =
            crate::stealth_pe::rewrite_copy_identity(&dest_exe, &dest_name, &file_stem)
        {
            eprintln!("[Stealth] Failed to rewrite version info: {err}");
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(&dest_exe) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&dest_exe, perms);
        }
    }

    let args: Vec<String> = env::args().skip(1).collect();
    // Keep the original cwd so sidecar lookup still finds the install directory.
    match spawn_detached_process(&dest_exe, &args) {
        Ok(()) => {
            println!(
                "[Stealth] Successfully spawned stealth process: {}",
                dest_name
            );
            std::process::exit(0);
        }
        Err(err) => {
            eprintln!("[Stealth] Failed to spawn stealth process: {}", err);
            let _ = fs::remove_dir_all(&stealth_dir);
            true
        }
    }
}

/// Spawn process in detached mode
#[cfg(target_os = "windows")]
fn spawn_detached_process(exe_path: &PathBuf, args: &[String]) -> io::Result<()> {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    const DETACHED_PROCESS: u32 = 0x00000008;

    Command::new(exe_path)
        .args(args)
        .creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS)
        .spawn()?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn spawn_detached_process(exe_path: &PathBuf, args: &[String]) -> io::Result<()> {
    use std::process::Stdio;

    Command::new(exe_path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn spawn_detached_process(exe_path: &PathBuf, args: &[String]) -> io::Result<()> {
    Command::new(exe_path).args(args).spawn()?;

    Ok(())
}

fn cleanup_old_stealth_copies(current_exe: &Path) {
    let temp_dir = env::temp_dir();
    let current_dir = current_exe.parent();

    let Ok(entries) = fs::read_dir(&temp_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if Some(path.as_path()) == Some(current_exe) {
            continue;
        }
        if current_dir == Some(path.as_path()) {
            continue;
        }

        if path.is_file() {
            remove_legacy_temp_file_if_needed(&path);
            continue;
        }

        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_hex_str(name, DIR_HEX_LEN) {
            continue;
        }
        if !dir_looks_like_stealth_copy(&path) {
            continue;
        }
        match fs::remove_dir_all(&path) {
            Ok(()) => println!("[Stealth] Cleaned up: {}", name),
            Err(err) => {
                if cfg!(debug_assertions) {
                    eprintln!("[Stealth] Failed to clean up {}: {}", name, err);
                }
            }
        }
    }
}

/// Cleanup on application exit
///
/// Should be called before application exits
pub fn cleanup_on_exit() {
    if !is_stealth_mode() {
        return;
    }

    if let Ok(current_exe) = env::current_exe() {
        schedule_self_deletion(&current_exe);
    }
}

/// Schedule self deletion without spawning cmd/bat.
/// Locked files are marked for reboot deletion when possible; next start
/// `cleanup_old_stealth_copies` removes whatever remains.
#[cfg(target_os = "windows")]
fn schedule_self_deletion(exe_path: &Path) {
    if let Some(ud) = webview_user_data_dir_for_exe(exe_path) {
        remove_tree_best_effort(&ud);
    }
    let Some(parent) = exe_path.parent() else {
        remove_tree_best_effort(exe_path);
        return;
    };
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path == exe_path {
                continue;
            }
            remove_tree_best_effort(&path);
        }
    }
    remove_tree_best_effort(exe_path);
    remove_tree_best_effort(parent);
}

#[cfg(target_os = "windows")]
fn remove_tree_best_effort(path: &Path) {
    let removed = if path.is_dir() {
        fs::remove_dir_all(path).is_ok()
    } else {
        fs::remove_file(path).is_ok()
    };
    if !removed {
        mark_delete_on_reboot(path);
    }
}

#[cfg(target_os = "windows")]
fn mark_delete_on_reboot(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let _ = unsafe {
        MoveFileExW(
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
    };
}

#[cfg(not(target_os = "windows"))]
fn schedule_self_deletion(exe_path: &Path) {
    let parent = exe_path.parent().map(Path::to_path_buf);
    let _ = fs::remove_file(exe_path);
    if let Some(parent) = parent {
        if is_stealth_copy_path(exe_path) || dir_looks_like_stealth_copy(&parent) {
            let _ = fs::remove_dir_all(&parent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_hex_stealth_layout() {
        let temp = env::temp_dir();
        let dir = temp.join("0123456789abcdef");
        let exe = if cfg!(windows) {
            dir.join("c0ffee12beef.exe")
        } else {
            dir.join("c0ffee12beef")
        };
        assert!(is_stealth_copy_path(&exe));
        assert_eq!(webview_user_data_dir_for_exe(&exe), Some(dir.join("ud")));
    }

    #[test]
    fn rejects_product_exe_name() {
        let exe = env::temp_dir().join("discord-quest-helper.exe");
        assert!(!is_stealth_copy_path(&exe));
    }

    #[test]
    fn rejects_legacy_svc_prefix() {
        let exe = env::temp_dir().join("svc_deadbeef.exe");
        assert!(!is_stealth_copy_path(&exe));
        assert_eq!(is_legacy_svc_file_name("svc_deadbeef.exe"), cfg!(windows));
        assert_eq!(is_legacy_svc_file_name("svc_deadbeef"), !cfg!(windows));
    }

    #[test]
    fn rejects_uppercase_or_wrong_length_hex() {
        let temp = env::temp_dir();
        let exe = temp.join("0123456789ABCDEF").join("c0ffee12beef.exe");
        assert!(!is_stealth_copy_path(&exe));
        let exe = temp.join("0123456789abcde").join("c0ffee12beef.exe");
        assert!(!is_stealth_copy_path(&exe));
        let exe = temp.join("0123456789abcdef").join("c0ffee12bee.exe");
        assert!(!is_stealth_copy_path(&exe));
    }

    #[test]
    fn window_title_matches_stem() {
        assert_eq!(
            window_title_for_exe(Path::new(r"C:\Temp\abc\c0ffee12beef.exe")),
            "c0ffee12beef"
        );
        assert_eq!(generate_random_suffix(12).len(), FILE_HEX_LEN);
        let suffix = generate_random_suffix(16);
        assert!(is_hex_str(&suffix, DIR_HEX_LEN));
    }

    #[test]
    fn dir_with_hex_exe_is_stealth_copy() {
        let dir = env::temp_dir().join(generate_random_suffix(DIR_HEX_LEN));
        fs::create_dir_all(&dir).unwrap();
        let exe_name = if cfg!(windows) {
            format!("{}.exe", generate_random_suffix(FILE_HEX_LEN))
        } else {
            generate_random_suffix(FILE_HEX_LEN)
        };
        let exe = dir.join(&exe_name);
        fs::write(&exe, b"test").unwrap();
        assert!(is_stealth_copy_path(&exe));
        assert!(!dir_looks_like_stealth_copy(&dir));
        fs::create_dir_all(dir.join(WEBVIEW_DATA_DIR_NAME)).unwrap();
        assert!(dir_looks_like_stealth_copy(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recognizes_legacy_cleanup_bat() {
        assert!(is_legacy_cleanup_bat("cleanup_ab12cd34.bat"));
        assert!(!is_legacy_cleanup_bat("cleanup_ab12cd34.exe"));
        assert!(!is_legacy_cleanup_bat("notes.bat"));
    }

    #[test]
    fn strip_zone_identifier_ignores_missing_ads() {
        let path = env::temp_dir().join(format!("stealth-motw-{}.txt", generate_random_suffix(8)));
        fs::write(&path, b"ok").unwrap();
        strip_zone_identifier(&path);
        assert!(path.exists());
        let _ = fs::remove_file(&path);
    }

    #[cfg(windows)]
    #[test]
    fn strip_zone_identifier_removes_motw_when_present() {
        let path = env::temp_dir().join(format!("stealth-motw-{}.txt", generate_random_suffix(8)));
        fs::write(&path, b"ok").unwrap();
        let ads = format!("{}:Zone.Identifier", path.display());
        let wrote_ads = fs::write(&ads, "[ZoneTransfer]\r\nZoneId=3\r\n").is_ok();
        strip_zone_identifier(&path);
        if wrote_ads {
            assert!(fs::read_to_string(&ads).is_err());
        }
        let _ = fs::remove_file(&path);
    }

    #[cfg(windows)]
    #[test]
    fn exit_cleanup_does_not_write_bat() {
        let dir = env::temp_dir().join(generate_random_suffix(DIR_HEX_LEN));
        fs::create_dir_all(&dir).unwrap();
        let ud = dir.join(WEBVIEW_DATA_DIR_NAME);
        fs::create_dir_all(&ud).unwrap();
        fs::write(ud.join("cache"), b"x").unwrap();
        let exe = dir.join(format!("{}.exe", generate_random_suffix(FILE_HEX_LEN)));
        fs::write(&exe, b"test").unwrap();

        let before: Vec<_> = fs::read_dir(env::temp_dir())
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| is_legacy_cleanup_bat(n))
            .collect();

        schedule_self_deletion(&exe);

        let after: Vec<_> = fs::read_dir(env::temp_dir())
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| is_legacy_cleanup_bat(n))
            .collect();
        assert_eq!(before, after);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn removes_legacy_cleanup_bat_from_a_directory() {
        let dir = env::temp_dir().join(format!("stealth-bat-{}", generate_random_suffix(8)));
        fs::create_dir_all(&dir).unwrap();
        let bat = dir.join("cleanup_ab12cd34.bat");
        let keep = dir.join("notes.bat");
        fs::write(&bat, b"@echo off\r\n").unwrap();
        fs::write(&keep, b"keep").unwrap();
        remove_legacy_temp_file_if_needed(&bat);
        remove_legacy_temp_file_if_needed(&keep);
        assert!(!bat.exists());
        assert!(keep.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}

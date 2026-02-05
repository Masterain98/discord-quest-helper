//! Stealth Mode Module
//!
//! Implements runtime random process name to evade Discord detection.
//! Both the main program and game runner use randomly generated filenames.

use once_cell::sync::Lazy;
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// Main app random name prefix
const MAIN_APP_PREFIX: &str = "svc_";

/// Game runner random name prefix
pub const RUNNER_PREFIX: &str = "runner_";

/// Flag indicating if current process is running in stealth mode
static IS_STEALTH_MODE: AtomicBool = AtomicBool::new(false);

/// Current stealth name (for logging)
static CURRENT_STEALTH_NAME: Lazy<std::sync::Mutex<Option<String>>> =
    Lazy::new(|| std::sync::Mutex::new(None));

/// Generate random hexadecimal string
fn generate_random_suffix(length: usize) -> String {
    use rand::Rng;
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

/// Check if currently running in stealth mode
pub fn is_stealth_mode() -> bool {
    IS_STEALTH_MODE.load(Ordering::Relaxed)
}

/// Get current stealth process name (if any)
pub fn get_stealth_name() -> Option<String> {
    CURRENT_STEALTH_NAME.lock().ok()?.clone()
}

/// Generate a random window title that looks like a system process
pub fn generate_stealth_window_title() -> String {
    use rand::Rng;
    
    // Pool of system-like window title patterns
    let patterns = [
        "Windows Update",
        "Windows Defender",
        "Background Task Host",
        "Service Host",
        "Runtime Broker",
        "Settings",
        "Microsoft Edge Update",
        "Windows Security",
        "System",
        "Host Process",
    ];
    
    let mut rng = rand::rng();
    let pattern = patterns[rng.random_range(0..patterns.len())];
    
    // Optionally add a random suffix
    if rng.random_bool(0.5) {
        let suffix = generate_random_suffix(4);
        format!("{} ({})", pattern, suffix)
    } else {
        pattern.to_string()
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
    // Skip stealth mode in debug builds
    #[cfg(debug_assertions)]
    {
        println!("[Stealth] Debug mode - skipping stealth");
        return true;
    }

    #[cfg(not(debug_assertions))]
    {
        ensure_stealth_mode_impl()
    }
}

#[cfg(not(debug_assertions))]
fn ensure_stealth_mode_impl() -> bool {
    // Get current executable info
    let current_exe = match env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[Stealth] Failed to get current exe path: {}", e);
            return true; // Cannot get path, continue execution
        }
    };

    let file_name = current_exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    // If already running with random name, mark and continue
    if file_name.starts_with(MAIN_APP_PREFIX) {
        IS_STEALTH_MODE.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = CURRENT_STEALTH_NAME.lock() {
            *guard = Some(file_name.to_string());
        }

        println!("[Stealth] Running in stealth mode as: {}", file_name);

        // Clean up old temp files
        cleanup_old_temp_files(MAIN_APP_PREFIX);

        return true;
    }

    println!("[Stealth] Starting stealth mode transition...");

    // Generate random name
    let random_suffix = generate_random_suffix(8);
    let ext = get_exe_extension();
    let temp_name = format!("{}{}{}", MAIN_APP_PREFIX, random_suffix, ext);

    // Copy to temp directory
    let temp_dir = env::temp_dir();
    let temp_exe = temp_dir.join(&temp_name);

    println!("[Stealth] Copying to: {:?}", temp_exe);

    if let Err(e) = fs::copy(&current_exe, &temp_exe) {
        eprintln!("[Stealth] Failed to copy to temp: {}", e);
        return true; // Copy failed, continue with original name
    }

    // Set executable permission (Unix)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(&temp_exe) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&temp_exe, perms);
        }
    }

    // Launch new process
    let args: Vec<String> = env::args().skip(1).collect();

    let spawn_result = spawn_detached_process(&temp_exe, &args);

    match spawn_result {
        Ok(_) => {
            println!(
                "[Stealth] Successfully spawned stealth process: {}",
                temp_name
            );
            // Successfully launched new process, exit current process
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("[Stealth] Failed to spawn stealth process: {}", e);
            let _ = fs::remove_file(&temp_exe);
            true // Launch failed, continue with original name
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

/// Clean up old temp executables
fn cleanup_old_temp_files(prefix: &str) {
    let temp_dir = env::temp_dir();
    let current_exe = env::current_exe().ok();
    let ext = get_exe_extension();

    if let Ok(entries) = fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            // Skip currently running file
            if Some(&path) == current_exe.as_ref() {
                continue;
            }

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with(prefix) && name.ends_with(ext) {
                    // Try to delete old file
                    match fs::remove_file(&path) {
                        Ok(_) => println!("[Stealth] Cleaned up: {}", name),
                        Err(_) => {} // File might be in use, ignore
                    }
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

    // Clean up game runner temp files
    cleanup_old_temp_files(RUNNER_PREFIX);

    // Self-destruct current temp file
    if let Ok(current_exe) = env::current_exe() {
        schedule_self_deletion(&current_exe);
    }
}

/// Schedule self deletion (delayed delete)
#[cfg(target_os = "windows")]
fn schedule_self_deletion(exe_path: &PathBuf) {
    // Windows cannot directly delete running exe
    // Use batch script for delayed deletion
    let bat_content = format!(
        "@echo off\n\
         timeout /t 2 /nobreak >nul\n\
         del /f /q \"{}\"\n\
         del /f /q \"%~f0\"\n",
        exe_path.display()
    );

    let bat_path = env::temp_dir().join("cleanup_dqh.bat");

    if fs::write(&bat_path, bat_content).is_ok() {
        let _ = Command::new("cmd")
            .args(["/C", "start", "/min", "", bat_path.to_str().unwrap()])
            .spawn();
    }
}

#[cfg(not(target_os = "windows"))]
fn schedule_self_deletion(exe_path: &PathBuf) {
    // Unix systems can directly delete running files (just removes inode reference)
    let _ = fs::remove_file(exe_path);
}

// ============================================================================
// Game Runner Random Name Support
// ============================================================================

/// Create random-named copy of game runner
///
/// Parameters:
/// - `source_runner`: Original runner executable path
/// - `target_dir`: Target directory
///
/// Returns: Path to random-named runner
pub fn create_stealth_runner(source_runner: &PathBuf, target_dir: &PathBuf) -> io::Result<PathBuf> {
    let random_suffix = generate_random_suffix(8);
    let ext = get_exe_extension();
    let stealth_name = format!("{}{}{}", RUNNER_PREFIX, random_suffix, ext);

    let stealth_path = target_dir.join(&stealth_name);

    // Copy runner to target directory
    fs::copy(source_runner, &stealth_path)?;

    // Set executable permission (Unix)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(&stealth_path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&stealth_path, perms);
        }
    }

    println!("[Stealth] Created stealth runner: {:?}", stealth_path);

    Ok(stealth_path)
}

/// Stop and clean up random-named runners
///
/// Attempts to terminate all processes starting with RUNNER_PREFIX
#[cfg(target_os = "windows")]
pub fn stop_stealth_runners() {
    let temp_dir = env::temp_dir();
    let ext = get_exe_extension();

    if let Ok(entries) = fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with(RUNNER_PREFIX) && name.ends_with(ext) {
                    // Try to terminate process
                    let _ = Command::new("taskkill")
                        .args(["/F", "/IM", name])
                        .output();

                    // Try to delete file
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub fn stop_stealth_runners() {
    let temp_dir = env::temp_dir();

    if let Ok(entries) = fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with(RUNNER_PREFIX) {
                    // Try to terminate process
                    let _ = Command::new("pkill").args(["-f", name]).output();

                    // Try to delete file
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn stop_stealth_runners() {
    // Other platforms not supported
}

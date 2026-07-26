use anyhow::{Context, Result};
use serde::Serialize;
#[cfg(not(target_os = "linux"))]
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use once_cell::sync::Lazy;

/// Global set that tracks image names of running simulated game processes.
/// Entries are added in `run_simulated_game` and removed in `stop_simulated_game`.
/// Used by `cleanup_all_simulated_games` to kill orphaned children on app exit.
///
/// Windows/macOS match by process *name*; Linux tracks exact PIDs instead (see
/// `RUNNING_LINUX_GAMES`) so it never risks killing a real game with the same
/// executable name.
#[cfg(not(target_os = "linux"))]
static RUNNING_GAMES: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// A simulated-game child process tracked by exact PID on Linux.
///
/// The `Child` handle is kept alive so the process is actually reaped: dropping
/// it would leave a zombie for the app's lifetime, and `kill(pid, 0)` succeeds
/// against a zombie, so the termination poll below would never see it exit.
#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LinuxManagedGame {
    pid: u32,
    executable_path: PathBuf,
    /// `None` once the child has been waited on.
    child: Option<std::process::Child>,
}

/// Linux tracks simulated games by PID (keyed on executable name) so it can
/// verify `/proc/<pid>/exe` before signalling and never kill an unrelated
/// process that merely shares the game's executable name.
#[cfg(target_os = "linux")]
static RUNNING_LINUX_GAMES: Lazy<Mutex<std::collections::HashMap<String, LinuxManagedGame>>> =
    Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

// Embed the runner binary at compile time from the data/ directory.
// build.rs ensures an empty placeholder exists if the runner hasn't been built yet,
// so this never causes a hard compile-time failure on a fresh clone or `cargo check`.
#[cfg(target_os = "windows")]
const RUNNER_BYTES: &[u8] = include_bytes!("../data/discord-quest-runner.exe");

#[cfg(target_os = "macos")]
const RUNNER_BYTES: &[u8] = include_bytes!("../data/discord-quest-runner");

#[cfg(target_os = "linux")]
const RUNNER_BYTES: &[u8] = include_bytes!("../data/discord-quest-runner");

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
const RUNNER_BYTES: &[u8] = &[];

/// Embedded runner version info (commit hash + build timestamp).
/// Written by build-runner.js, placeholder created by build.rs if not built yet.
const RUNNER_VERSION_INFO: &str = include_str!("../data/runner-version.txt");

/// Runner version information exposed to the frontend
#[derive(Debug, Clone, Serialize)]
pub struct RunnerInfo {
    pub embedded: bool,
    pub commit_hash: String,
    pub build_time: String,
    pub size_bytes: usize,
}

/// Get information about the embedded runner binary
pub fn get_runner_info() -> RunnerInfo {
    let lines: Vec<&str> = RUNNER_VERSION_INFO.lines().collect();
    let commit_hash = lines.first().unwrap_or(&"unknown").to_string();
    let build_time = lines.get(1).unwrap_or(&"").to_string();
    let embedded = !RUNNER_BYTES.is_empty();

    RunnerInfo {
        embedded,
        commit_hash: if commit_hash != "not-built" {
            commit_hash
        } else {
            "unknown".to_string()
        },
        build_time: if embedded { build_time } else { String::new() },
        size_bytes: RUNNER_BYTES.len(),
    }
}

/// Write the embedded runner binary to the target path
fn ensure_runner_bytes(target_path: &Path) -> Result<()> {
    if RUNNER_BYTES.is_empty() {
        if cfg!(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux"
        )) {
            anyhow::bail!("Runner binary not embedded (run `npm run build:runner`)");
        } else {
            anyhow::bail!("Runner binary not available for this platform");
        }
    }
    fs::write(target_path, RUNNER_BYTES).context("Failed to write embedded runner binary")?;
    // On macOS/Linux, set executable permission
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(target_path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Create a simulated game executable
///
/// Writes the embedded runner executable to the specified path with the target game name.
/// Discord detects games by process name, so renaming the runner to match the
/// target game's executable name allows us to simulate running that game.
pub fn create_simulated_game(path: &str, executable_name: &str, _app_id: &str) -> Result<()> {
    println!(
        "create_simulated_game called with path: '{}', exe: '{}'",
        path, executable_name
    );

    // Create target directory
    let target_dir = PathBuf::from(path);
    println!(
        "Target directory: {:?}, exists: {}",
        target_dir,
        target_dir.exists()
    );

    if !target_dir.exists() {
        println!("Creating directory: {:?}", target_dir);
        fs::create_dir_all(&target_dir).context(format!(
            "Could not create target directory: {:?}",
            target_dir
        ))?;
    }

    // Target executable path
    let target_exe = target_dir.join(executable_name);

    // Ensure parent directory exists (for executable_name with subdirectories)
    if let Some(parent) = target_exe.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).context("Could not create target subdirectory")?;
        }
    }

    // If file exists, try to delete it first
    if target_exe.exists() {
        if let Err(e) = fs::remove_file(&target_exe) {
            println!(
                "Target file exists and remove failed ({}), trying to kill process...",
                e
            );
            // Process might be running, try to stop it
            let _ = stop_simulated_game(executable_name);
            // Wait for process to release the lock
            std::thread::sleep(std::time::Duration::from_millis(500));
            // Try to delete again
            if let Err(e) = fs::remove_file(&target_exe) {
                println!("Still cannot remove file: {}", e);
                // Continue to copy, see if it overwrites or fails
            }
        }
    }

    // Write embedded runner binary to target location with game's name
    println!("Writing embedded runner to {:?}", target_exe);
    ensure_runner_bytes(&target_exe).map_err(|e| {
        anyhow::anyhow!(
            "Could not write runner executable to {:?}: {}",
            target_exe,
            e
        )
    })?;

    println!("Simulated game created: {:?}", target_exe);
    Ok(())
}

/// Run the simulated game
#[cfg(target_os = "windows")]
pub fn run_simulated_game(
    name: &str,
    path: &str,
    executable_name: &str,
    _app_id: &str,
) -> Result<()> {
    let exe_to_run = PathBuf::from(path).join(executable_name);

    // Always try to update the runner binary from the embedded bytes
    println!("Attempting to update simulated game at {:?}", exe_to_run);
    match ensure_runner_bytes(&exe_to_run) {
        Ok(_) => println!("Successfully updated simulated game executable"),
        Err(e) => println!(
            "Could not update simulated game executable (might be running?): {}",
            e
        ),
    }

    if !exe_to_run.exists() {
        anyhow::bail!("Executable does not exist: {:?}", exe_to_run);
    }

    let _ = Command::new("cmd")
        .args(["/C", "start", "", exe_to_run.to_str().unwrap()])
        .spawn()
        .context("Could not start simulated game")?;

    // Track the running process so we can clean it up on app exit
    track_running_game(executable_name);

    println!("Simulated game {} started from {:?}", name, exe_to_run);
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn run_simulated_game(
    name: &str,
    path: &str,
    executable_name: &str,
    _app_id: &str,
) -> Result<()> {
    let exe_to_run = PathBuf::from(path).join(executable_name);

    if !exe_to_run.exists() {
        anyhow::bail!("Executable does not exist: {:?}", exe_to_run);
    }

    // Make the file executable
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&exe_to_run)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&exe_to_run, perms)?;

    // Launch the process in background
    let _ = Command::new(&exe_to_run)
        .spawn()
        .context("Could not start simulated game")?;

    // Track the running process so we can clean it up on app exit
    track_running_game(executable_name);

    println!("Simulated game {} started from {:?}", name, exe_to_run);
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn run_simulated_game(
    name: &str,
    path: &str,
    executable_name: &str,
    _app_id: &str,
) -> Result<()> {
    use std::process::Stdio;

    let exe_to_run = PathBuf::from(path).join(executable_name);

    // Refresh the runner bytes when possible; a running instance keeps the file
    // busy (ETXTBSY), which is fine — we fall back to the existing binary.
    match ensure_runner_bytes(&exe_to_run) {
        Ok(_) => println!("Successfully updated simulated game executable"),
        Err(e) => println!(
            "Could not update simulated game executable (might be running?): {}",
            e
        ),
    }

    if !exe_to_run.exists() {
        anyhow::bail!("Executable does not exist: {:?}", exe_to_run);
    }

    let child = Command::new(&exe_to_run)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Could not start simulated game")?;

    let pid = child.id();
    track_linux_game(executable_name, exe_to_run.clone(), pid, child);

    println!(
        "Simulated game {} started from {:?} with PID {}",
        name, exe_to_run, pid
    );
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn run_simulated_game(
    _name: &str,
    _path: &str,
    _executable_name: &str,
    _app_id: &str,
) -> Result<()> {
    anyhow::bail!("Game simulation is only supported on Windows, macOS, and Linux")
}

/// Stop the simulated game
#[cfg(target_os = "windows")]
pub fn stop_simulated_game(exec_name: &str) -> Result<()> {
    // taskkill /IM needs image name (filename), not path.
    // Robustly handle both / and \\ separators
    let file_name = exec_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(exec_name);

    println!(
        "Stopping simulated game: Input='{}' -> Image='{}'",
        exec_name, file_name
    );

    // Use taskkill command to terminate process
    let output = Command::new("taskkill")
        .args(["/F", "/IM", file_name])
        .output()
        .context("Could not execute taskkill command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Don't error out, process may not exist
        println!(
            "taskkill returned non-zero, process may not exist: {}",
            stderr
        );
    }

    // Remove from tracking set
    untrack_running_game(exec_name);

    println!("Simulated game {} stopped", exec_name);
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn stop_simulated_game(exec_name: &str) -> Result<()> {
    // Extract just the filename from the path
    let file_name = exec_name.split('/').next_back().unwrap_or(exec_name);

    println!(
        "Stopping simulated game: Input='{}' -> Process='{}'",
        exec_name, file_name
    );

    // Use pkill to terminate process by name
    let output = Command::new("pkill")
        .args(["-f", file_name])
        .output()
        .context("Could not execute pkill command")?;

    // pkill returns 0 if processes were killed, 1 if no processes matched
    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("pkill returned non-zero: {}", stderr);
    }

    // Remove from tracking set
    untrack_running_game(exec_name);

    println!("Simulated game {} stopped", exec_name);
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn stop_simulated_game(exec_name: &str) -> Result<()> {
    let key = file_name_key(exec_name);

    let managed = RUNNING_LINUX_GAMES
        .lock()
        .ok()
        .and_then(|mut games| games.remove(&key));

    let Some(mut managed) = managed else {
        println!("No tracked Linux simulated game for '{}'", key);
        return Ok(());
    };

    // Only signal the PID if it still points at the runner we launched, so a
    // recycled PID (or a real game with the same name) is never killed. On a
    // mismatch, collect the child if it already exited but never block on it.
    if linux_pid_is_runner(managed.pid, &managed.executable_path) {
        terminate_linux_game(&mut managed);
        println!("Simulated game '{}' (pid {}) stopped", key, managed.pid);
    } else {
        println!(
            "Tracked pid {} no longer refers to '{}'; not signalling",
            managed.pid, key
        );
        try_reap_linux_game(&mut managed);
    }

    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn stop_simulated_game(_exec_name: &str) -> Result<()> {
    anyhow::bail!("Game simulation is only supported on Windows, macOS, and Linux")
}

/// Reduce an executable name/path to its bare file-name key.
#[cfg(target_os = "linux")]
fn file_name_key(name: &str) -> String {
    name.rsplit(['/', '\\']).next().unwrap_or(name).to_string()
}

/// Record a started simulated game keyed on its executable name.
///
/// Re-launching the same name replaces the tracked entry; the superseded
/// process is stopped first so it can never be orphaned past app exit.
#[cfg(target_os = "linux")]
fn track_linux_game(
    executable_name: &str,
    executable_path: PathBuf,
    pid: u32,
    child: std::process::Child,
) {
    let key = file_name_key(executable_name);
    let previous = match RUNNING_LINUX_GAMES.lock() {
        Ok(mut games) => {
            let previous = games.insert(
                key.clone(),
                LinuxManagedGame {
                    pid,
                    executable_path,
                    child: Some(child),
                },
            );
            println!("Tracked Linux game '{}' pid {}", key, pid);
            previous
        }
        Err(_) => None,
    };

    // Terminating waits up to 2s, so do it after releasing the map lock.
    if let Some(mut previous) = previous {
        println!(
            "Replacing tracked '{}': stopping previous pid {}",
            key, previous.pid
        );
        if linux_pid_is_runner(previous.pid, &previous.executable_path) {
            terminate_linux_game(&mut previous);
        } else {
            try_reap_linux_game(&mut previous);
        }
    }
}

/// True when `/proc/<pid>/exe` still resolves to the runner we launched.
#[cfg(target_os = "linux")]
fn linux_pid_is_runner(pid: u32, executable_path: &Path) -> bool {
    let exe_link = PathBuf::from("/proc").join(pid.to_string()).join("exe");
    match (
        std::fs::canonicalize(&exe_link),
        std::fs::canonicalize(executable_path),
    ) {
        (Ok(actual), Ok(expected)) => actual == expected,
        // If the target file was replaced/removed we can still compare the raw
        // symlink target against the tracked path. The kernel appends
        // " (deleted)" to the link once the inode is unlinked, so strip it.
        _ => std::fs::read_link(&exe_link)
            .map(|target| {
                let raw = target.to_string_lossy();
                let stripped = raw.strip_suffix(" (deleted)").unwrap_or(raw.as_ref());
                Path::new(stripped) == executable_path
            })
            .unwrap_or(false),
    }
}

/// SIGTERM the tracked child, wait briefly, then SIGKILL if it is still alive.
/// The child is always reaped before returning.
#[cfg(target_os = "linux")]
fn terminate_linux_game(game: &mut LinuxManagedGame) {
    use nix::sys::signal::{self, Signal};
    use nix::unistd::Pid;

    let target = Pid::from_raw(game.pid as i32);
    if signal::kill(target, Signal::SIGTERM).is_err() {
        reap_linux_game(game);
        return;
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if linux_game_has_exited(game) {
            reap_linux_game(game);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let _ = signal::kill(target, Signal::SIGKILL);
    reap_linux_game(game);
}

/// Whether the tracked process is gone. Prefers `Child::try_wait`, which also
/// reaps the zombie; a bare `kill(pid, 0)` probe would report a zombie as alive.
#[cfg(target_os = "linux")]
fn linux_game_has_exited(game: &mut LinuxManagedGame) -> bool {
    match game.child.as_mut() {
        Some(child) => !matches!(child.try_wait(), Ok(None)),
        None => {
            use nix::sys::signal;
            use nix::unistd::Pid;
            signal::kill(Pid::from_raw(game.pid as i32), None).is_err()
        }
    }
}

/// Reap the child so an exited process doesn't linger as a zombie. Only called
/// once the process is known to be gone (or has been SIGKILLed), so the
/// underlying `wait()` returns immediately.
#[cfg(target_os = "linux")]
fn reap_linux_game(game: &mut LinuxManagedGame) {
    if let Some(mut child) = game.child.take() {
        let _ = child.wait();
    }
}

/// Non-blocking reap for identity-mismatch paths, where the child has *not*
/// been confirmed dead: an exited child (its `/proc/<pid>/exe` link breaks once
/// it is a zombie) is collected, but a live one — e.g. the runner file was
/// moved or renamed, so the link no longer matches — is left running untouched.
/// A blocking `wait()` here would hang until that unverified process exits.
#[cfg(target_os = "linux")]
fn try_reap_linux_game(game: &mut LinuxManagedGame) {
    if let Some(mut child) = game.child.take() {
        if matches!(child.try_wait(), Ok(None)) {
            // Still alive; not verified as ours to kill or wait on — put the
            // handle back untouched.
            game.child = Some(child);
        }
    }
}

/// Track a newly started simulated game process.
#[cfg(not(target_os = "linux"))]
fn track_running_game(executable_name: &str) {
    let file_name = executable_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(executable_name)
        .to_string();
    if let Ok(mut set) = RUNNING_GAMES.lock() {
        set.insert(file_name.clone());
        println!("Tracked running game: {} (total: {})", file_name, set.len());
    }
}

/// Remove a game from the tracking set (called after explicit stop).
#[cfg(not(target_os = "linux"))]
fn untrack_running_game(executable_name: &str) {
    let file_name = executable_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(executable_name)
        .to_string();
    if let Ok(mut set) = RUNNING_GAMES.lock() {
        set.remove(&file_name);
        println!(
            "Untracked running game: {} (remaining: {})",
            file_name,
            set.len()
        );
    }
}

/// Stop **all** tracked simulated game processes.
///
/// Called on application exit to ensure no orphaned child processes are left
/// running after the main app (and its RPC connection) closes.
pub fn cleanup_all_simulated_games() {
    #[cfg(target_os = "linux")]
    cleanup_all_linux_games();

    #[cfg(not(target_os = "linux"))]
    cleanup_all_tracked_games();
}

/// Windows/macOS: stop every tracked game by name.
#[cfg(not(target_os = "linux"))]
fn cleanup_all_tracked_games() {
    let games: Vec<String> = match RUNNING_GAMES.lock() {
        Ok(mut set) => set.drain().collect(),
        Err(poisoned) => poisoned.into_inner().drain().collect(),
    };

    if games.is_empty() {
        return;
    }

    println!(
        "Cleaning up {} simulated game process(es) on exit...",
        games.len()
    );
    for name in &games {
        println!("  Stopping: {}", name);
        let _ = stop_simulated_game(name);
    }
}

/// Linux: SIGTERM/SIGKILL every tracked PID whose `/proc/<pid>/exe` still
/// matches the runner we launched.
#[cfg(target_os = "linux")]
fn cleanup_all_linux_games() {
    let mut games: Vec<LinuxManagedGame> = match RUNNING_LINUX_GAMES.lock() {
        Ok(mut games) => games.drain().map(|(_, value)| value).collect(),
        Err(poisoned) => poisoned
            .into_inner()
            .drain()
            .map(|(_, value)| value)
            .collect(),
    };

    if games.is_empty() {
        return;
    }

    println!(
        "Cleaning up {} simulated game process(es) on exit...",
        games.len()
    );
    for game in &mut games {
        if linux_pid_is_runner(game.pid, &game.executable_path) {
            println!("  Stopping pid {}", game.pid);
            terminate_linux_game(game);
        } else {
            try_reap_linux_game(game);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    #[ignore] // Requires actual file system operations
    fn test_create_simulated_game() {
        let temp_dir = env::temp_dir().join("discord-quest-test");
        let result = create_simulated_game(temp_dir.to_str().unwrap(), "test-game.exe", "123456");

        match result {
            Ok(_) => {
                let exe_path = temp_dir.join("test-game.exe");
                assert!(exe_path.exists());
                // Cleanup
                let _ = fs::remove_dir_all(&temp_dir);
            }
            Err(e) => println!("Test skipped (expected): {}", e),
        }
    }
}

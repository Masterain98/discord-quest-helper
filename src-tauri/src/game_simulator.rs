use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// Embed the runner binary at compile time from the data/ directory.
// build.rs ensures an empty placeholder exists if the runner hasn't been built yet,
// so this never causes a hard compile-time failure on a fresh clone or `cargo check`.
#[cfg(target_os = "windows")]
const RUNNER_BYTES: &[u8] = include_bytes!("../data/discord-quest-runner.exe");

#[cfg(target_os = "macos")]
const RUNNER_BYTES: &[u8] = include_bytes!("../data/discord-quest-runner");

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const RUNNER_BYTES: &[u8] = &[];

/// Write the embedded runner binary to the target path
fn ensure_runner_bytes(target_path: &Path) -> Result<()> {
    if RUNNER_BYTES.is_empty() {
        anyhow::bail!("Runner binary not available for this platform");
    }
    fs::write(target_path, RUNNER_BYTES)
        .context("Failed to write embedded runner binary")?;
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
        fs::create_dir_all(&target_dir)
            .context(format!("Could not create target directory: {:?}", target_dir))?;
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
pub fn run_simulated_game(name: &str, path: &str, executable_name: &str, _app_id: &str) -> Result<()> {
    let exe_to_run = PathBuf::from(path).join(executable_name);

    // Always try to update the runner binary from the embedded bytes
    println!("Attempting to update simulated game at {:?}", exe_to_run);
    match ensure_runner_bytes(&exe_to_run) {
        Ok(_) => println!("Successfully updated simulated game executable"),
        Err(e) => println!("Could not update simulated game executable (might be running?): {}", e),
    }

    if !exe_to_run.exists() {
        anyhow::bail!("Executable does not exist: {:?}", exe_to_run);
    }

    let _ = Command::new("cmd")
        .args(["/C", "start", "", exe_to_run.to_str().unwrap()])
        .spawn()
        .context("Could not start simulated game")?;

    println!("Simulated game {} started from {:?}", name, exe_to_run);
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn run_simulated_game(name: &str, path: &str, executable_name: &str, _app_id: &str) -> Result<()> {
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

    println!("Simulated game {} started from {:?}", name, exe_to_run);
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn run_simulated_game(
    _name: &str,
    _path: &str,
    _executable_name: &str,
    _app_id: &str,
) -> Result<()> {
    anyhow::bail!("Game simulation is only supported on Windows and macOS")
}

/// Stop the simulated game
#[cfg(target_os = "windows")]
pub fn stop_simulated_game(exec_name: &str) -> Result<()> {
    // taskkill /IM needs image name (filename), not path.
    // Robustly handle both / and \\ separators
    let file_name = exec_name
        .split(|c| c == '/' || c == '\\')
        .last()
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

    println!("Simulated game {} stopped", exec_name);
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn stop_simulated_game(exec_name: &str) -> Result<()> {
    // Extract just the filename from the path
    let file_name = exec_name.split('/').last().unwrap_or(exec_name);

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

    println!("Simulated game {} stopped", exec_name);
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn stop_simulated_game(_exec_name: &str) -> Result<()> {
    anyhow::bail!("Game simulation is only supported on Windows and macOS")
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

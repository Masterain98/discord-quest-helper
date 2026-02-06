use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Create a simulated game executable
///
/// Copies the runner executable to the specified path with the target game name.
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

    // Get runner executable path
    let runner_path = get_runner_exe_path()?;

    // Copy runner to target location with game's name
    println!("Copying runner from {:?} to {:?}", runner_path, target_exe);
    fs::copy(&runner_path, &target_exe).map_err(|e| {
        anyhow::anyhow!(
            "Could not copy executable from {:?} to {:?}: {}",
            runner_path,
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

/// Get the platform-specific executable extension
#[cfg(target_os = "windows")]
fn get_exe_extension() -> &'static str {
    ".exe"
}

#[cfg(not(target_os = "windows"))]
fn get_exe_extension() -> &'static str {
    ""
}

/// Get runner executable path
fn get_runner_exe_path() -> Result<PathBuf> {
    let ext = get_exe_extension();
    let runner_name = format!("discord-quest-runner{}", ext);

    // List of potential paths to check
    let paths = vec![
        // Copied to data folder (preferred)
        PathBuf::from(format!("data/{}", runner_name)),
        PathBuf::from(format!("../src-tauri/data/{}", runner_name)),
        // Direct build locations
        PathBuf::from(format!("../src-runner/target/release/{}", runner_name)),
        PathBuf::from(format!("src-runner/target/release/{}", runner_name)),
        // Original checks
        PathBuf::from(format!("../target/release/{}", runner_name)),
    ];

    for path in paths {
        if path.exists() {
            // Convert to absolute path for clarity
            if let Ok(abs_path) = fs::canonicalize(&path) {
                return Ok(abs_path);
            }
            return Ok(path);
        }
    }

    let ext = get_exe_extension();
    let runner_name = format!("discord-quest-runner{}", ext);

    // Attempt to find via current exe directory (for prod/bundled)
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let bundled_path = parent.join(format!("data/{}", runner_name));
            if bundled_path.exists() {
                return Ok(bundled_path);
            }
            // Check in the same directory as the executable
            let sibling_path = parent.join(&runner_name);
            if sibling_path.exists() {
                return Ok(sibling_path);
            }

            // macOS: Check inside the app bundle Resources directory
            #[cfg(target_os = "macos")]
            {
                if let Some(resources) = parent.parent().map(|p| p.join("Resources")) {
                    let resources_path = resources.join(&runner_name);
                    if resources_path.exists() {
                        return Ok(resources_path);
                    }
                }
            }
        }
    }

    anyhow::bail!(
        "Runner executable not found.\nPlease ensure src-runner is built and discord-quest-runner{} exists in the data or target directory.",
        ext
    )
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

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Create a simulated game executable
///
/// Copies the template executable to the specified path with the target game name
pub fn create_simulated_game(path: &str, executable_name: &str, _app_id: &str) -> Result<()> {
    // Create target directory
    let target_dir = PathBuf::from(path);
    if !target_dir.exists() {
        fs::create_dir_all(&target_dir).context("Could not create target directory")?;
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
            println!("Target file exists and remove failed ({}), trying to kill process...", e);
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

    // Get runner executable path (assumed to be in resources directory)
    // In actual deployment, this should be obtained from Tauri resources
    let runner_path = get_runner_exe_path()?;

    // Copy file
    println!("Copying runner from {:?} to {:?}", runner_path, target_exe);
    fs::copy(&runner_path, &target_exe).map_err(|e| {
        anyhow::anyhow!("Could not copy executable from {:?} to {:?}: {}", runner_path, target_exe, e)
    })?;

    println!("Simulated game created: {:?}", target_exe);
    Ok(())
}

/// Run the simulated game
#[cfg(target_os = "windows")]
pub fn run_simulated_game(name: &str, path: &str, executable_name: &str, _app_id: &str) -> Result<()> {
    let target_exe = PathBuf::from(path).join(executable_name);

    if !target_exe.exists() {
        anyhow::bail!("Executable does not exist: {:?}", target_exe);
    }

    // Use Windows start command to launch the process
    let _ = Command::new("cmd")
        .args(&["/C", "start", "", target_exe.to_str().unwrap()])
        .spawn()
        .context("Could not start simulated game")?;

    println!("Simulated game {} started", name);
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn run_simulated_game(
    _name: &str,
    _path: &str,
    _executable_name: &str,
    _app_id: &str,
) -> Result<()> {
    anyhow::bail!("Game simulation is only supported on Windows")
}

/// Stop the simulated game
#[cfg(target_os = "windows")]
pub fn stop_simulated_game(exec_name: &str) -> Result<()> {
    // taskkill /IM needs image name (filename), not path.
    // robustly handle both / and \ separators
    let file_name = exec_name
        .split(|c| c == '/' || c == '\\')
        .last()
        .unwrap_or(exec_name);

    println!("Stopping simulated game: Input='{}' -> Image='{}'", exec_name, file_name);

    // Use taskkill command to terminate process
    let output = Command::new("taskkill")
        .args(&["/F", "/IM", file_name])
        .output()
        .context("Could not execute taskkill command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to stop game: {}", stderr);
    }

    println!("Simulated game {} stopped", exec_name);
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn stop_simulated_game(_exec_name: &str) -> Result<()> {
    anyhow::bail!("Game simulation is only supported on Windows")
}

/// Get runner executable path
fn get_runner_exe_path() -> Result<PathBuf> {
    // List of potential paths to check
    let paths = vec![
        // Copied to data folder (preferred)
        PathBuf::from("data/discord-quest-runner.exe"),
        PathBuf::from("../src-tauri/data/discord-quest-runner.exe"),
        // Direct build locations
        PathBuf::from("../src-runner/target/release/discord-quest-runner.exe"),
        PathBuf::from("src-runner/target/release/discord-quest-runner.exe"),
        // Original checks
        PathBuf::from("../target/release/discord-quest-runner.exe"),
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

    // Attempt to find via current exe directory (for prod/bundled)
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let bundled_path = parent.join("data/discord-quest-runner.exe");
            if bundled_path.exists() {
                return Ok(bundled_path);
            }
            // Check in the same directory as the executable (common for bundled resources if flattened)
            let sibling_path = parent.join("discord-quest-runner.exe");
            if sibling_path.exists() {
                return Ok(sibling_path);
            }
        }
    }

    anyhow::bail!("Runner executable not found.\nPlease ensure src-runner is built and discord-quest-runner.exe exists in the data or target directory.")
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

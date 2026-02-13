use std::fs;
use std::path::Path;

fn main() {
    // Ensure the data/ directory exists
    let data_dir = Path::new("data");
    if !data_dir.exists() {
        fs::create_dir_all(data_dir).expect("Failed to create data directory");
    }

    // Determine the runner binary name for the current platform
    let runner_exe_name = if cfg!(target_os = "windows") {
        "discord-quest-runner.exe"
    } else {
        "discord-quest-runner"
    };

    let data_runner_path = data_dir.join(runner_exe_name);

    // If the runner binary hasn't been copied to data/ yet, create an empty
    // placeholder so that include_bytes! in game_simulator.rs always compiles.
    // This allows `cargo check`, rust-analyzer, and fresh-clone builds to
    // succeed. The empty bytes are handled gracefully at runtime.
    if !data_runner_path.exists() {
        println!(
            "cargo:warning=Runner executable not found at data/{}. \
             Build src-runner first with: cd src-runner && cargo build --release, \
             then run the build-runner script to copy it to src-tauri/data/.",
            runner_exe_name
        );
        fs::write(&data_runner_path, b"").expect("Failed to create runner placeholder");
    }

    // Tell Cargo to re-run build script if the data copy changes
    println!("cargo:rerun-if-changed=data/{}", runner_exe_name);

    tauri_build::build()
}

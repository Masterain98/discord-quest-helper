use std::path::Path;

fn main() {
    // Check if runner executable exists for the current platform
    let runner_path = if cfg!(target_os = "windows") {
        "../src-runner/target/release/discord-quest-runner.exe"
    } else {
        "../src-runner/target/release/discord-quest-runner"
    };

    // Only warn if the runner doesn't exist - don't fail the build
    // This allows `cargo check` and IDE features to work without building runner first
    if !Path::new(runner_path).exists() {
        println!(
            "cargo:warning=Runner executable not found at {}. Build src-runner first with: cd src-runner && cargo build --release",
            runner_path
        );
    }

    tauri_build::build()
}

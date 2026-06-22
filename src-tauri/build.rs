use std::fs;
use std::path::Path;

#[cfg(target_os = "windows")]
fn is_png_newer(png_path: &Path, ico_path: &Path) -> bool {
    fs::metadata(png_path)
        .and_then(|p| fs::metadata(ico_path).map(|i| p.modified().ok() > i.modified().ok()))
        .unwrap_or(true)
}

#[cfg(target_os = "windows")]
fn convert_png_to_ico(png_path: &Path, ico_path: &Path) {
    use std::io::BufReader;

    let file = match fs::File::open(png_path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let reader = BufReader::new(file);
    let decoder = png::Decoder::new(reader);
    let mut reader = match decoder.read_info() {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let info = match reader.next_frame(&mut buf) {
        Ok(i) => i,
        Err(_) => return,
    };
    buf.truncate(info.buffer_size());

    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    let icon_image = ico::IconImage::from_rgba_data(info.width, info.height, buf);

    match ico::IconDirEntry::encode(&icon_image) {
        Ok(entry) => {
            icon_dir.add_entry(entry);
            let file = match fs::File::create(ico_path) {
                Ok(f) => f,
                Err(_) => return,
            };
            let _ = icon_dir.write(file);
            println!("cargo:warning=Converted launcher-logo.png to ICO");
        }
        Err(e) => {
            println!("cargo:warning=Failed to encode ICO: {}", e);
        }
    }
}

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

    // Ensure runner-version.txt exists (placeholder if not built yet)
    let version_info_path = data_dir.join("runner-version.txt");
    if !version_info_path.exists() {
        fs::write(&version_info_path, "not-built\n\n")
            .expect("Failed to create runner-version.txt placeholder");
    }

    // Tauri validates bundle.externalBin paths during the build script, so a
    // fresh checkout needs a placeholder before scripts/build-cdp-launcher.js
    // replaces it with the real launcher.
    let target_triple = std::env::var("TARGET").unwrap_or_else(|_| {
        if cfg!(target_os = "windows") {
            "x86_64-pc-windows-msvc".to_string()
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin".to_string()
        } else if cfg!(target_os = "macos") {
            "x86_64-apple-darwin".to_string()
        } else {
            "unknown".to_string()
        }
    });
    let launcher_ext = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    let binaries_dir = Path::new("binaries");
    if !binaries_dir.exists() {
        fs::create_dir_all(binaries_dir).expect("Failed to create binaries directory");
    }
    let launcher_path = binaries_dir.join(format!(
        "discord-cdp-launcher-sidecar-{}{}",
        target_triple, launcher_ext
    ));
    if !launcher_path.exists() {
        fs::write(&launcher_path, b"").expect("Failed to create CDP launcher placeholder");
    }

    // Note: Launcher icon is managed by the build-cdp-launcher.js script
    // which uses Resource Hacker or similar tool to set the icon after compilation.
    // The ICO file is pre-generated from launcher-logo.png for that purpose.
    #[cfg(target_os = "windows")]
    {
        let ico_path = Path::new("../public/icons/launcher-logo.ico");
        let png_path = Path::new("../public/icons/launcher-logo.png");

        // Convert PNG to ICO if needed (for use by external tools)
        if png_path.exists() && (!ico_path.exists() || is_png_newer(png_path, ico_path)) {
            convert_png_to_ico(png_path, ico_path);
        }
    }

    // Tell Cargo to re-run build script if the data copy changes
    println!("cargo:rerun-if-changed=data/{}", runner_exe_name);
    println!("cargo:rerun-if-changed=data/runner-version.txt");
    println!("cargo:rerun-if-changed={}", launcher_path.display());

    tauri_build::build()
}

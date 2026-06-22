use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=resources/launcher.rc");
    println!("cargo:rerun-if-changed=resources/launcher-logo.ico");

    if env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set"));
    let res_path = out_dir.join("launcher.res");
    let rc_candidates = resource_compiler_candidates();

    let mut compiled = false;
    let mut last_error = String::new();

    for rc in rc_candidates {
        let status = Command::new(&rc)
            .current_dir("resources")
            .arg("/nologo")
            .arg(format!("/fo{}", res_path.display()))
            .arg("launcher.rc")
            .status();

        match status {
            Ok(status) if status.success() => {
                compiled = true;
                break;
            }
            Ok(status) => {
                last_error = format!("{} exited with status {}", rc.display(), status);
            }
            Err(error) => {
                last_error = format!("{} failed: {}", rc.display(), error);
            }
        }
    }

    if !compiled {
        panic!(
            "failed to compile launcher Windows resources: {}",
            last_error
        );
    }

    println!(
        "cargo:rustc-link-arg-bin=discord-cdp-launcher={}",
        res_path.display()
    );
}

fn resource_compiler_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from("rc.exe"));
    candidates.push(PathBuf::from("llvm-rc"));

    if let Some(sdk_rc) = find_windows_sdk_rc() {
        candidates.insert(0, sdk_rc);
    }

    candidates
}

fn find_windows_sdk_rc() -> Option<PathBuf> {
    let sdk_root = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .or_else(|| env::var_os("ProgramFiles(x86)").map(PathBuf::from))
        .map(|path| {
            if path.ends_with("Windows Kits") || path.ends_with("Windows Kits\\10") {
                path
            } else {
                path.join("Windows Kits").join("10")
            }
        })?;

    let bin_dir = sdk_root.join("bin");
    let arch_dir = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x64",
        Ok("x86") => "x86",
        Ok("aarch64") => "arm64",
        Ok("arm") => "arm",
        _ => "x64",
    };

    let mut versions = std::fs::read_dir(&bin_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();

    versions.sort_by(|a, b| compare_sdk_paths(b, a));

    for version_dir in versions {
        let candidate = version_dir.join(arch_dir).join("rc.exe");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn compare_sdk_paths(a: &Path, b: &Path) -> std::cmp::Ordering {
    parse_sdk_version(a).cmp(&parse_sdk_version(b))
}

fn parse_sdk_version(path: &Path) -> Vec<u32> {
    path.file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default()
        .split('.')
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

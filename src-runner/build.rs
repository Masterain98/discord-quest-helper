use std::process::Command;

fn main() {
    // Capture git commit hash at compile time
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output();

    let commit_hash = match output {
        Ok(o) if o.status.success() => {
            String::from_utf8(o.stdout).unwrap_or_default().trim().to_string()
        }
        _ => "unknown".to_string(),
    };

    println!("cargo:rustc-env=RUNNER_COMMIT_HASH={}", commit_hash);
    println!("cargo:rerun-if-changed=.git/HEAD");
}

use std::path::Path;

fn main() {
    // 根据目标平台确定 runner 可执行文件的路径
    let (runner_src, runner_dest) = if cfg!(target_os = "windows") {
        (
            "../src-runner/target/release/discord-quest-runner.exe",
            "data/discord-quest-runner.exe",
        )
    } else {
        // macOS 和 Linux 没有 .exe 扩展名
        (
            "../src-runner/target/release/discord-quest-runner",
            "data/discord-quest-runner",
        )
    };

    // 检查资源文件是否存在，如果不存在则输出警告（开发模式下可能还未编译）
    let runner_path = Path::new(runner_src);
    if !runner_path.exists() {
        println!(
            "cargo:warning=Runner executable not found at '{}'. Please build src-runner first with 'cargo build --release'",
            runner_src
        );
    }

    // 配置资源打包
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .resources_by_path([(runner_src, runner_dest)])
    )
    .expect("failed to run tauri-build");
}

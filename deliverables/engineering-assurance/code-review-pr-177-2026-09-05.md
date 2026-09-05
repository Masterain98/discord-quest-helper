# PR #177 工程保障审查报告

## TL;DR

PR #177 的 6 条逐行代码反馈已逐条核验；其中 4 条真实有效并已修复，1 条真实但按维护者规则保留原状，1 条 CodeQL 告警为误报。
修复覆盖 Windows shortcut 参数 quoting、Discord 官方渠道切换、托管 CDP 会话恢复和版本元数据。
GitHub Actions 的 hash pin 反馈按仓库维护者要求不采纳，保留可读的 `@v6` 版本标签，并将该规则写入 `AGENTS.md`。
所有计划内前端、Rust、构建、格式、clippy 和锁文件校验均通过。

## Core Conclusion Card

| 项目 | 结论 |
| --- | --- |
| 总体评级 | APPROVE-WITH-FIXES |
| 真实且已修复 | 4 条功能/一致性反馈 |
| 按维护者偏好处理 | 1 条安全反馈：保留 action 版本标签 |
| 不采纳 | 1 条 CodeQL 误报；Sourcery/CodeRabbit 限流提示无代码动作 |
| 当前阻断项 | 0 |
| 下一步 | 提交并推送 `develop`，等待远端 CI |

## Action List

| # | Action | Owner | Urgency | Status |
| --- | --- | --- | --- | --- |
| 1 | 修复带空格 Windows 路径的命令行参数转义 | Code Reviewer | High | Done |
| 2 | 让 Discord Stable/PTB/Canary 变体切换比较 `variant_id` | Code Reviewer | High | Done |
| 3 | 通过 session journal 和当前 executable path 恢复移动后的托管客户端 | Code Reviewer | High | Done |
| 4 | 统一 0.10.4 发布元数据 | Release Owner | Medium | Done |
| 5 | 保留 action 版本标签规则并等待远端 CI | Maintainer | Medium | Pending remote CI |

## Findings

| # | Severity | Category | Location | Review feedback | Verdict / Fix | Source |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | High | Correctness | `src-tauri/src/lib.rs:2993` | 带空格的 Windows 路径被整体 double backslash，shortcut 启动时路径损坏。 | 有效；改为 Windows 命令行规则，仅在引号前和参数结尾转义反斜杠，并添加回归测试。 | Greptile `3939682956` |
| 2 | High | Correctness | `src-tauri/src/discord_cdp_commands.rs:585` | 同一 provider 下 Stable → Canary/PTB 时提前复用旧 CDP owner。 | 有效；冲突解析现在比较 provider、installation 和 variant，目标变体不同时先恢复旧 owner。 | Greptile `3939682957` |
| 3 | High | Reliability | `src-tauri/src/discord_cdp_commands.rs:190` | 进程无法映射到当前 installation 时，托管 session 失去身份，退出恢复不可用。 | 有效；按 provider + port 使用唯一 journal fallback 补回身份，恢复时重新扫描并使用当前运行路径；多重/缺失匹配安全失败。 | Greptile `3939682960` |
| 4 | Medium | Release consistency | `package.json:3` | canonical 版本为 0.10.4，但 package、Tauri、Cargo workspace metadata 仍为 0.10.3。 | 有效；四处 workspace/release metadata 已统一为 0.10.4。 | Greptile `3939682962` |
| 5 | Medium | Supply chain | `.github/workflows/build-release.yml:454` | write job 使用可变 `actions/checkout@v6`。 | 反馈风险真实，但按维护者要求保留可读的 `@v6`；永久规则已记录在 `AGENTS.md`，未使用 hash。 | Greptile `3939682964` |
| 6 | Medium | Security | `src-tauri/src/cdp_client.rs:2114` | CodeQL 报告 cleartext sensitive logging。 | 误报；代码只输出 `username.len()` 和 `id.len()` 两个整数，没有输出 authorization/token。保持实现不变并记录依据。 | CodeQL `3939689909` |

Sourcery 仅报告 diff 超过审查限制；CodeRabbit 仅报告 review rate limit，二者没有可执行代码反馈。

## Architecture Impact

- CDP session journal 仍是本地状态来源；扫描结果在 installation 不可发现时通过唯一 provider/port 关联恢复托管身份。
- 恢复流程以实时进程 executable path 为准，并在多个候选时拒绝操作，避免误杀或误恢复其他客户端。
- 未新增前端 IPC 字段或破坏现有 JSON contract。

## Test Coverage Assessment

| Check | Result |
| --- | --- |
| `pnpm test` | 10 files, 62 passed |
| `pnpm run build` | Passed; existing chunk-size warning only |
| `pnpm run i18n:check` | Errors 0, warnings 0 |
| `cargo test --workspace` | Passed; Tauri crate 139 passed, 6 ignored, plus all other workspace targets passed |
| `cargo check --workspace --locked` | Passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `git diff --check` | Passed |

新增回归覆盖 Windows 带空格路径、官方 variant 匹配、未知 installation journal fallback，以及移动 executable path 恢复。

## Disclaimer

本报告由工程保障团队 AI 协作生成，关键决策请由人类工程负责人复核。

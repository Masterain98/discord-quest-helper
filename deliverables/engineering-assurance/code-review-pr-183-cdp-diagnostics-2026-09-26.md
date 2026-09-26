# PR #183 工程保障审查报告

## TL;DR

PR #183 的方向正确，但机器人审查发现了 6 类需要跟进的真实问题。它们现已修复并由定向回归测试覆盖：readiness deadline 顺序、Flatpak Vesktop 进程映射、provider 多安装诊断歧义、前端 launch error 生命周期、非法端口错误覆盖，以及导出字符串脱敏。

两条建议未采纳：`CdpWithoutDiscordTarget` 后出现短暂 `Unreachable` 时不应撤销延长窗口，因为 Updater → App 切换正需要保留这一单调状态；CodeRabbit 的 docstring 覆盖率不是仓库质量门，也与本次变更无关。

## Core Conclusion Card

| 项目 | 结论 |
| --- | --- |
| 审查结论 | 修改后可重新审查 |
| 最高风险 | P1：probe 已返回 ready，却因先检查 deadline 被误报 timeout |
| 有效反馈 | 6 类（8 个 inline 线程中有 1 组重复）+ 1 条 review body 补充意见 |
| 不采纳 | sticky extension 重置、classification enum key 脱敏、批量 docstring 补齐 |
| 自动验证 | `pnpm test`、`pnpm run build`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`、CDP core 依赖边界、i18n 校验、`git diff --check` |
| CI 基线 | 跟进前 Windows / macOS / Linux 均为绿色 |

## Action List

- [x] 在判断 timeout 前接受本次 `DiscordReady` 观察结果，并增加阻塞 probe 越过 deadline 的回归测试。
- [x] 将 renderer 延长标志命名为 `saw_cdp_server`，记录 Updater → App 临时断连仍保留 grace period 的设计意图，并增加状态序列测试。
- [x] 抽取统一的 process → installation 匹配逻辑，覆盖 executable、macOS bundle 与 Flatpak Vesktop。
- [x] Provider selector 不再伪造 exact installation；精确 ID、路径和选中标记仅用于 Installation selector，并按 provider + variant 计算 running 状态。
- [x] 删除后端 snapshot 中永远为 `null` 的 `lastLaunchError` 字段，保留前端单一短生命周期状态来源。
- [x] LoginPanel 的最终 launch/restart 失败写入诊断状态；可恢复竞态不会留下伪错误；新尝试会清理旧错误。
- [x] 非法端口覆盖旧错误并记录结构化 `invalid_port`。
- [x] 导出的 executable basename 与 process name 统一经过 redactor，并增加邮箱、Discord ID、authorization 字符串测试。
- [ ] 推送后等待 GitHub Actions 重新完成，并确认没有新的人类审查阻塞项。

## Disclaimer

本报告基于 PR #183 当前分支、GitHub 上已拉取的全部 issue/review/inline 评论，以及本地可执行的自动化验证。机器人评论按代码事实逐条核验；未将第三方建议性指标视为仓库既有质量门。跨平台行为最终仍以 GitHub Actions 的 Windows、macOS、Linux 结果为准。

# PR #176 代码反馈与 CI 审查报告

## TL;DR

PR #176 的 29 条行内代码反馈均能在当前代码中找到事实依据；其中 17 条属于高优先级正确性、安全或生命周期问题，另外的反馈主要涉及路径解析、性能、可维护性和本地化。已在当前 `dev/vestop` 工作区处理全部可在本次范围内合理修复的反馈，并修复 Ubuntu CI 的真实编译失败。

Sourcery 没有产生代码反馈，只报告 diff 超过审查限制。CodeRabbit 的 macOS “越界改动”告警与 PR 相对 `develop` 的实际差异不符，因此未回退 develop 上已有的运行时身份/发布改进。docstring 覆盖率提示不是 CI 门禁，也没有为了百分比添加无意义注释。

## Core Conclusion Card

| 项目 | 结论 |
| --- | --- |
| 反馈真实性 | Greptile 3/3、CodeRabbit 26/26、PR 外 1 条均有依据；重复反馈已合并处理 |
| CI 阻断 | Ubuntu 的 Linux-only 测试引用了不存在的 `super::discord_cdp_launch_core`；已移除错误路径并放入正确的 Linux 测试导入 |
| 功能状态 | Flatpak、动态发现、CDP owner、官方渠道 variant、端口传递、客户端选择和退出恢复均已修复 |
| 安全状态 | Flatpak command 已限制为字面量 `flatpak`；DPAPI 前缀安全校验；进程树逐目标临终校验 |
| 验证状态 | 前端测试、Rust workspace 测试、clippy、build、i18n 和 diff/format 检查通过 |
| 合并建议 | 修复提交推送后等待三平台 CI；本地不再存在已知编译阻断 |

## Action List

### 已处理的高优先级反馈

1. `3935144552` / `3935371738`：Flatpak 安装不再因没有 executable path 而从运行状态中消失；自动 Vesktop 选择改用完整发现结果，能够选择 Flatpak 安装。
2. `3935371808`：进程树终止现在为每个目标保存并在 kill 前重新校验 PID、启动时间和 executable path，并明确记录 `sysinfo` 缺少跨平台稳定句柄这一残余限制，避免把 best-effort 误写成绝对安全保证。
3. `3935371832` / `3935371841` / `3935371953` / `3935371959`：添加安装透传当前 CDP 端口；安装扫描进入 `spawn_blocking`；添加/删除/选择统一使用请求序列、loading 和错误状态；迁移 marker 只在获得有效 snapshot 且迁移成功后写入。
4. `3935371776` / `3935371913`：CDP 已连接时优先使用实际 owner；接受当前 owner 后同步 legacy `questsStore.desktopClient`。
5. PR 外 `model.rs:177`：自定义官方 Discord 安装从 executable/bundle 名称推导 Stable/PTB/Canary `variant_id`，并加入 PTB 回归测试。
6. `3935371920`：退出恢复逐个尝试所有 session；任一恢复失败时继续处理后续 session、显示错误并保持窗口打开，避免留下未恢复 CDP 客户端后直接退出。

### 已处理的中优先级反馈

- `3935144568` / `3935371814` / `3935371869`：共用带引号和反斜杠转义的 Desktop Entry 参数解析器，保留带空格的 executable 与 installation path，并加入 Linux 回归测试。
- `3935371745`：Flatpak `command` 不再可以从配置注入任意可执行文件，只接受 `flatpak`。
- `3935371754`：Flatpak spawn 后转移 `Child` 到回收线程，避免长期积累 zombie。
- `3935371770`：`DiscordAlreadyRunning` 使用实际选中的渠道而不是原始自动选择值。
- `3935371823`：Unix Vesktop 发现要求普通文件同时具备 executable 权限。
- `3935371854`：Windows 注册表发现忽略空 `InstallLocation`，避免构造相对路径 `vesktop.exe`。
- `3935371790`：同一 CDP 端口在桌面进程扫描中只 probe 一次。
- `3935371803`：恢复普通 executable 客户端时复用刷新后的进程表，减少重复 liveness 扫描。
- `3935371880`：DPAPI 解码先验证 `DPAPI` 前缀，短输入返回错误而不是 panic，并加入回归测试。
- `3935371893`：移除只被测试使用且无生产调用方的旧登录辅助函数及其测试。
- `3935371905`：提取共享的 provider → legacy client 映射。
- `3935371935`：Linux 文件选择器不使用扩展名过滤，避免隐藏 `/usr/bin/vesktop` 等无扩展名可执行文件。
- `3935371947`：CDP owner 标签改用 i18n key。
- `3935371978` / `3935371983`：新增客户端选择、CDP 状态和恢复流程文案已补齐 15 个 locale；本次新增内容的 i18n 检查警告降为 0。

### 不采纳或不作为本次阻断项的反馈

- CodeRabbit 的 macOS release-signing “out of scope”告警：核对 `origin/develop...dev/vestop` 后不成立，不能据此删除 develop 的运行时身份和发布改进。
- docstring coverage 39.39%：属于全仓库质量提示，不是本 PR 的功能缺陷或 CI 失败原因；批量添加空洞文档会增加噪音，因此不处理。

## 验证记录

- `pnpm exec vitest run src/components/auth/loginFlow.test.ts src/composables/appExitGuard.test.ts`：21 passed
- `pnpm test`：8 个 test files、51 tests passed
- `pnpm run i18n:check`：Errors 0、Warnings 0
- `pnpm run build`：成功；保留既有的大 chunk warning
- `cargo test --workspace`：核心、launcher、CDP probe、Tauri 主 crate 全部通过；真实本机 CDP 测试按设计保持 ignored
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`：通过
- `cargo fmt --all -- --check`：通过
- `git diff --check`：通过

### 推送后的 CI 复核

- 首轮远端 CI 的 Windows 与 Ubuntu 前置步骤通过；macOS/Linux 的 clippy 暴露了一个本机 Windows 不可见的条件编译问题：DPAPI 前缀校验 helper 在非 Windows 目标上未被使用。
- 已将该 helper 及其测试限定为 Windows，并将 CDP socket 测试服务改为阻塞式单次 accept、延长非功能性读取超时，同时移除不必要的半关闭，避免完整 Content-Length 响应在高并发 runner 上出现误报 `IncompleteResponse`。
- 本机重复运行 `cdp_probe` 5 次（每次 12 tests）全部通过，workspace clippy 也通过；修复已作为后续提交推送，等待下一轮三平台 CI。
- 第二轮远端 run 的 macOS clippy 已通过，但暴露出两项平台测试问题：Windows 专属 Vesktop discovery 测试未加平台条件，以及官方 PTB 自定义安装测试使用了 macOS 不接受的 Linux 风格 executable 名称。已分别增加 Windows `cfg`、补齐 macOS/Linux 官方渠道名称（含 `.app` 稳定 bundle 识别），并在本机 workspace 测试与 clippy 中验证通过；该修复等待下一轮 CI。
- 第三轮远端 run 的 Windows 全部检查以及 macOS clippy/Rust tests 已通过；Ubuntu clippy 编译暴露 Linux-only desktop-entry 测试把 `String` 直接与 `&str` 匹配/传参。已改用 `as_str()`，这是条件编译分支在 Windows 本机构建中无法发现的跨平台类型问题。

## Disclaimer

进程树终止部分受到 `sysinfo` 跨平台模型限制：当前实现把身份验证延迟到每个 kill 前，并覆盖 root 与 descendant，但没有声称可以消除操作系统层面的“校验后、终止前”PID 复用竞态。若未来需要硬性保证，应为 Linux pidfd、Windows process handle 和 macOS 对应进程句柄分别增加平台级实现与集成测试。

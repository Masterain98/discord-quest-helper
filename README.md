<div align="center">

<h1>Discord Quest Helper</h1>

<p align="center">
  <img src="src-tauri/icons/icon.png" alt="Discord Quest Helper logo" width="150">
</p>

<p><strong>🎮 Automate your Discord Quests with one click</strong></p>

<p>Complete Discord video, stream, and game quests automatically while you focus on what matters.</p>

<p>⭐ <strong>If you find this helpful, please give it a star!</strong> ⭐</p>

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue.svg)](https://github.com/Masterain98/discord-quest-helper/releases)
[![Tauri](https://img.shields.io/badge/tauri-2-blue.svg)](https://tauri.app/)
[![Vue](https://img.shields.io/badge/vue-3.5-green.svg)](https://vuejs.org/)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![GitHub Release](https://img.shields.io/github/v/release/Masterain98/discord-quest-helper?label=latest%20release&color=41b883)](https://github.com/Masterain98/discord-quest-helper/releases/latest)

<br>

<img src="public/certificated-ai-sloop-tiny.png" alt="Certificated AI Slop" width="480">

</div>

## 🚀 Quick Start

> [!WARNING]
> **This tool is for educational purposes only.** Using this tool may violate Discord's Terms of Service. The authors are not responsible for any consequences resulting from the use of this software. Use at your own risk.

### Download & Run

Download the latest build from [GitHub Releases](https://github.com/Masterain98/discord-quest-helper/releases/latest).

| Platform | Release files | Instructions |
| --- | --- | --- |
| Windows x64 | `discord-quest-helper-Windows-x64-<version>-portable.zip` or `discord-quest-helper-Windows-x64-<version>-setup.msi` | Extract the portable ZIP and run `discord-quest-helper.exe`, or open the MSI installer. |
| macOS Apple Silicon | `discord-quest-helper-MacOS-arm64-<version>.dmg` | Open the DMG and drag the app to Applications. If macOS blocks it, run the quarantine-removal command below. |
| Linux x86_64 | `discord-quest-helper-Linux-x86_64-<version>.AppImage` or `discord-quest-helper-Linux-x86_64-<version>.deb` | Use the AppImage for a portable install, or install the Debian package with the command below. |

On macOS, remove the quarantine attribute if needed:

```bash
xattr -cr "/Applications/Discord Quest Helper.app"
```

On Linux, run the AppImage like this:

```bash
chmod +x discord-quest-helper-Linux-x86_64-<version>.AppImage
./discord-quest-helper-Linux-x86_64-<version>.AppImage
```

Or install the Debian package:

```bash
sudo apt install ./discord-quest-helper-Linux-x86_64-<version>.deb
```

> [!NOTE]
> Release binaries are built and published by GitHub Actions from the repository source. Linux release packages target x86_64; macOS releases currently target Apple Silicon.

### Sign in

1. **Auto Detect Token** — find accounts from supported local Discord profiles.
2. **CDP Login** — connect to the official Discord desktop client or Vesktop.
3. **Manual Input** — enter a token directly when the other methods are unavailable.

> [!TIP]
> Vesktop is supported through CDP only; it is not scanned as a local token source. You can select a detected installation or add a custom/portable `vesktop.exe` in Settings.

### Complete Quests

- **Video/Stream:** Click **Start Quest** on an incomplete quest.
- **Game:** Open **Game Simulator**, select a game, then create and run a simulated game.

## ✨ Features

- ⚡ **Flexible Login** — Auto-detect supported local Discord profiles, connect through CDP, or enter a token manually.
- 🖥️ **Discord & Vesktop Support** — Select the desktop client or installation used for CDP login, including custom paths.
- 🐧 **Linux Desktop Support** — Available as an x86_64 AppImage or Debian package.
- 🎮 **Zero-Download Game Simulation** — Complete game quests without downloading or installing the actual game.
- 📺 **Video & Stream Automation** — Start once and let quest progress update in the background.
- 🔍 **Advanced Quest Filters** — Filter by reward type, completion status, and more.
- 👥 **Multi-Account Support** — Manage multiple Discord accounts in one app.
- 🌏 **Multi-language** — English, Simplified Chinese, Traditional Chinese, Japanese, Korean, Russian, Spanish, German, French, Indonesian, Polish, Portuguese, Thai, Turkish, and Vietnamese.

## 📸 Screenshots

| Login | Home |
|:-----:|:----:|
| ![Login](https://discord-quest-helper.dal.ao/images/login.png) | ![Home](https://discord-quest-helper.dal.ao/images/home1.png) |

| Multi-Account | Game Simulator |
|:-------------:|:--------------:|
| ![Multi-Account](https://discord-quest-helper.dal.ao/images/multi-account.png) | ![Game Simulator](https://discord-quest-helper.dal.ao/images/game-simulator.png) |

| Quest Progress | Settings |
|:--------------:|:--------:|
| ![Quest Progress](https://discord-quest-helper.dal.ao/images/home2.png) | ![Settings](https://discord-quest-helper.dal.ao/images/settings.png) |

## 🏗️ Architecture

```text
Discord Quest Helper
├─ Vue 3 + Vite frontend
│  ├─ Views: Home, Game Simulator, Settings, Debug
│  ├─ Pinia stores and composables for auth, quests, settings, and UI state
│  └─ src/api/tauri.ts — typed Tauri IPC client
│
├─ Tauri 2 Rust application
│  ├─ Discord API and Gateway integration
│  ├─ CDP client and quest execution for video, stream, activity, and game quests
│  ├─ Official Discord and Vesktop providers for discovery, launch, and process supervision
│  ├─ Token extraction and platform capability detection
│  ├─ Game simulation and manual CDP game sessions
│  └─ Runtime identity auditing and platform runtime bridge management
│
├─ Workspace crates
│  ├─ discord-cdp-launch-core — cross-platform client discovery and launch core
│  ├─ src-cdp-launcher — optional Discord/Vesktop CDP launcher sidecar
│  └─ src-runner — minimal game-process runner sidecar
│
└─ Discord services
   ├─ REST API — quests, accounts, rewards, and profile data
   ├─ Gateway — account and activity events
   └─ Discord/Vesktop CDP targets — browser automation and session capture
```

The frontend communicates with the Rust backend through Tauri IPC. The backend owns Discord networking, local credential extraction, CDP sessions, quest execution, process cleanup, and platform-specific integration.

Explore the codebase with [![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/Masterain98/discord-quest-helper)

## 🔒 Security

- **Tokens are kept in memory by the helper** — The app does not intentionally persist your Discord token to disk.
- **Encrypted local extraction** — Auto-detection reads supported Discord profiles through platform-native protection where available.
- **Platform-native credentials** — Windows DPAPI, macOS Keychain, and Linux Secret Service are used by the local extraction paths.
- **HTTPS for Discord API requests** — Network requests use secure HTTPS connections.
- **Sanitized diagnostics** — Logs and debug exports redact sensitive tokens and account data where applicable.

## 🤝 Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for:

- Development setup
- Project structure
- Code conventions
- Pull request guidelines

## 📄 License

MIT License — see the [LICENSE](LICENSE) file.


## 🙏 Credits

**Inspiration & Resources**
- [markterence/discord-quest-completer](https://github.com/markterence/discord-quest-completer)
- [power0matin/discord-quest-auto-completer](https://github.com/power0matin/discord-quest-auto-completer)
- [taisrisk/Discord-Quest-Helper](https://github.com/taisrisk/Discord-Quest-Helper)
- [aamiaa/CompleteDiscordQuest.md](https://gist.github.com/aamiaa/204cd9d42013ded9faf646fae7f89fbb)
- [docs.discord.food](https://docs.discord.food/)

**Technologies**
- [Tauri](https://tauri.app/) • [Vue.js](https://vuejs.org/) • [Pinia](https://pinia.vuejs.org/) • [vue-i18n](https://vue-i18n.intlify.dev/) • [shadcn-vue](https://www.shadcn-vue.com/) • [TailwindCSS](https://tailwindcss.com/) • [Lucide Icons](https://lucide.dev/)

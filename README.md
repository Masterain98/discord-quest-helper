<div align="center">

# Discord Quest Helper

<p align="center">
  <img src="src-tauri/icons/icon.png" alt="Discord Quest Helper Logo" width="150">
</p>

**🎮 Automate your Discord Quests with one click**

Complete Discord video, stream, and game quests automatically while you focus on what matters.

⭐ **If you find this helpful, please give it a star!** ⭐

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Platform](https://img.shields.io/badge/platform-Windows%20|%20macOS-blue.svg)
![Tauri](https://img.shields.io/badge/tauri-2.0-blue.svg)
![Vue](https://img.shields.io/badge/vue-3.5-green.svg)
![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)
![GitHub Release](https://img.shields.io/github/v/release/Masterain98/discord-quest-helper?label=latest%20release&color=41b883)

</div>

## 🚀 Quick Start

> [!WARNING]
> **This tool is for educational purposes only.** Using this tool may violate Discord's Terms of Service. The authors are not responsible for any consequences resulting from the use of this software. Use at your own risk.

### Download & Run

**Windows:**
1. Go to [GitHub Releases](https://github.com/Masterain98/discord-quest-helper/releases) and download the latest `.zip` file
2. Extract the archive to any folder
3. Run `Discord Quest Helper.exe`

**macOS:**
1. Go to [GitHub Releases](https://github.com/Masterain98/discord-quest-helper/releases) and download the latest `.dmg` file
2. Open the `.dmg` file and drag the app to your Applications folder
3. Run the following command in Terminal to remove the quarantine attribute:
   ```bash
   xattr -cr /Applications/Discord\ Quest\ Helper.app
   ```
4. Run `Discord Quest Helper` from Applications

### Login

1. Click **Auto Detect Token** for automatic extraction, or
2. Click **Manual Input** to enter your token directly

> [!NOTE]
> **Auto Detect Token** requires the Discord desktop client to be running in the background.

### Complete Quests

- **Video/Stream**: Click "Start Quest" on any incomplete quest
- **Game**: Use Game Simulator tab → Select game → Create & Run simulated game

## ✨ Features

- ⚡ **One-Click Login** — Automatically detects your Discord token, no scripts or technical steps needed
- 🎮 **Zero-Download Game Simulation** — Complete game quests without downloading or installing the actual game
- 📺 **Video & Stream Automation** — Click once, progress submits automatically in the background
- 🔍 **Advanced Quest Filter** — Filter by reward type, completion status, and more
- 👥 **Multi-Account Support** — Manage multiple Discord accounts seamlessly
- 🌏 **Multi-language** — English, Chinese, Japanese, Korean, Russian, Spanish

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

```
┌─────────────────────────────────────────────────────────────────┐
│                      Discord Quest Helper                        │
├─────────────────────────────────────────────────────────────────┤
│  Vue.js Frontend (Vite dev server :1420)                         │
│  ├─ Views: Home, GameSimulator, Settings                        │
│  ├─ Stores: auth.ts, quests.ts (Pinia)                          │
│  └─ API: tauri.ts (IPC bridge)                                   │
├────────────────────────┬────────────────────────────────────────┤
│     Tauri IPC          │                                         │
├────────────────────────┴────────────────────────────────────────┤
│  Rust Backend (Tauri 2.0)                                        │
│  ├─ token_extractor.rs   - LevelDB + DPAPI + AES-GCM             │
│  ├─ cdp_client.rs        - Chrome DevTools Protocol integration  │
│  ├─ discord_api.rs       - HTTP client & endpoints               │
│  ├─ quest_completer.rs   - Video/stream automation               │
│  └─ game_simulator.rs    - Process creation & management         │
├─────────────────────────────────────────────────────────────────┤
│  Game Runner (src-runner) - Minimal Windows exe (~140KB)         │
└─────────────────────────────────────────────────────────────────┘
                              │ HTTPS
                              ▼
                    Discord API (discord.com/api/v9)
```

## 🔒 Security

- **Tokens stored in memory only** — Never persisted to disk
- **HTTPS for all requests** — Secure API communication
- **Platform-native encryption** — Windows DPAPI / macOS Keychain

> [!CAUTION]
> Using automation tools may violate Discord ToS and result in account suspension.

## 🤝 Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for:

- Development setup
- Project structure
- Code conventions
- Pull request guidelines

## 📄 License

MIT License — see [LICENSE](LICENSE) file.


## 🙏 Credits

**Inspiration & Resources**
- [markterence/discord-quest-completer](https://github.com/markterence/discord-quest-completer)
- [power0matin/discord-quest-auto-completer](https://github.com/power0matin/discord-quest-auto-completer)
- [taisrisk/Discord-Quest-Helper](https://github.com/taisrisk/Discord-Quest-Helper)
- [docs.discord.food](https://docs.discord.food/)

**Technologies**
- [Tauri](https://tauri.app/) • [Vue.js](https://vuejs.org/) • [shadcn-vue](https://www.shadcn-vue.com/) • [TailwindCSS](https://tailwindcss.com/)

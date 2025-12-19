# Discord Quest Helper

A Windows desktop application to complete Discord Quests automatically - supports video, stream, and game quests.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Platform](https://img.shields.io/badge/platform-Windows-blue.svg)
![Tauri](https://img.shields.io/badge/tauri-2.0-blue.svg)
![Vue](https://img.shields.io/badge/vue-3.5-green.svg)
![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)

## 📸 Screenshots

| Login | Home |
|:-----:|:----:|
| ![Login](./screenshots/login.png) | ![Home](./screenshots/home1.png) |

| Multi-Account | Game Simulator |
|:-------------:|:--------------:|
| ![Multi-Account](./screenshots/multi-account.png) | ![Game Simulator](./screenshots/game-simulator.png) |

| Quest Progress | Settings |
|:--------------:|:--------:|
| ![Quest Progress](./screenshots/home2.png) | ![Settings](./screenshots/settings.png) |

---

> [!WARNING]
> **This tool is for educational purposes only.** Using this tool may violate Discord's Terms of Service. The authors are not responsible for any consequences resulting from the use of this software. Use at your own risk.

## ✨ Features

### Core Capabilities
- 🔐 **Automatic Token Extraction** - Reads and decrypts Discord tokens from local LevelDB (Stable, Canary, PTB)
- 🎮 **Game Quests** - Simulates game processes with Discord RPC integration
- 📺 **Video Quests** - Automates video watching progress
- 📡 **Stream Quests** - Sends periodic heartbeat requests
- 📊 **Real-time Progress** - Live tracking with event-driven updates
- 🌏 **Multi-language** - English, Chinese, Japanese, Korean, Russian, Spanish

### Technology Stack

| Layer | Technologies |
|-------|--------------|
| **Frontend** | Vue 3.5 • TypeScript 5.6 • TailwindCSS 3.4 • shadcn-vue |
| **Backend** | Rust • Tauri 2.0 • reqwest |
| **Build** | Vite 5.4 • pnpm |
| **Encryption** | AES-256-GCM • Windows DPAPI |

---

## 📋 Requirements

- **OS**: Windows 10/11 (x64)
- **Node.js**: 18.x+
- **Rust**: 1.70+
- **pnpm**: 8.x+
- **Visual Studio Build Tools** with C++ workload

---

## 🚀 Quick Start

### Installation

```bash
# Clone repository
git clone https://github.com/Masterain98/discord-quest-helper.git
cd discord-quest-helper

# Install dependencies
pnpm install

# Development mode
pnpm tauri:dev
```

### Production Build

```bash
# Build game runner (optional, for game quests)
cd src-runner && cargo build --release && cd ..

# Build application
pnpm tauri:build
```

Output: `src-tauri/target/release/bundle/`

---

## 📘 Usage

### Login
1. Click **Auto Detect Token** for automatic extraction, or
2. Click **Manual Input** to enter your token directly

### Complete Quests
- **Video/Stream**: Click "Start Quest" on any incomplete quest
- **Game**: Use Game Simulator tab → Select game → Create & Run simulated game

---

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

---

## 📂 Project Structure

```
discord-quest-helper/
├── src/                          # Vue.js frontend
│   ├── components/               # Reusable UI components
│   ├── views/                    # Page views (Home, Settings, GameSimulator)
│   ├── stores/                   # Pinia state management
│   ├── locales/                  # i18n translations (7 languages)
│   ├── api/tauri.ts              # Tauri IPC bridge
│   └── App.vue                   # Root component
├── src-tauri/                    # Rust backend
│   └── src/
│       ├── lib.rs                # Tauri commands
│       ├── token_extractor.rs    # Token extraction & decryption
│       ├── discord_api.rs        # Discord API client
│       ├── quest_completer.rs    # Quest completion logic
│       ├── game_simulator.rs     # Game simulation
│       └── models.rs             # Data structures
├── src-runner/                   # Game runner executable
│   └── src/
│       ├── main.rs               # Minimal Windows process
│       └── tray.rs               # System tray support
├── package.json                  # Node.js config
├── vite.config.ts                # Vite config
├── tailwind.config.js            # TailwindCSS config
└── README.md                     # This file
```

---

## 🔧 Development

### Commands

| Command | Description |
|---------|-------------|
| `pnpm install` | Install dependencies |
| `pnpm tauri:dev` | Development mode with hot reload |
| `pnpm tauri:build` | Production build |
| `pnpm dev` | Frontend dev server only |
| `cargo clippy` | Rust linting |
| `cargo fmt` | Rust formatting |

### Debugging

- **Frontend**: DevTools via `Ctrl+Shift+I` in app window
- **Backend**: Console output from `pnpm tauri:dev`
- **Verbose**: `RUST_LOG=debug pnpm tauri:dev`

---

## 📐 Code Conventions

### Rust (Backend)

```rust
// Use standard rustfmt formatting
// Run: cargo fmt

// Module structure
mod module_name;         // snake_case for modules
pub struct StructName;   // PascalCase for types
pub fn function_name();  // snake_case for functions
const CONSTANT_NAME;     // SCREAMING_SNAKE_CASE for constants

// Error handling: Use anyhow::Result with context
fn example() -> Result<T> {
    operation().context("Descriptive error message")?;
}

// Logging: Use println! for console output (English only)
println!("Starting video quest: quest_id={}, target={}s", id, seconds);

// Comments: English only
/// Documentation comments for public items
// Implementation comments for internal logic
```

### TypeScript/Vue (Frontend)

```typescript
// Use Composition API with <script setup>
<script setup lang="ts">
import { ref, computed } from 'vue'

// Reactive state
const isLoading = ref(false)

// Computed properties
const displayValue = computed(() => ...)

// Functions: camelCase
async function handleSubmit() { ... }
</script>

// Component naming: PascalCase files
// QuestCard.vue, GameSelector.vue

// Pinia stores: use composition style
export const useAuthStore = defineStore('auth', () => {
    const user = ref<DiscordUser | null>(null)
    return { user }
})
```

### Tauri IPC

```typescript
// Frontend: camelCase function names
export async function createSimulatedGame(...): Promise<void> {
    return await invoke('create_simulated_game', { ... })
}

// Backend: snake_case command names
#[tauri::command]
async fn create_simulated_game(...) -> Result<(), String> { ... }
```

### Styling (TailwindCSS)

```vue
<!-- Use utility classes with logical grouping -->
<div class="flex items-center gap-4 p-4 bg-card rounded-lg border">
    ...
</div>

<!-- Dark mode: automatic via .dark class on html -->
<!-- Use CSS variables from shadcn-vue theme -->
```

### Internationalization

```typescript
// All UI text via vue-i18n
const { t } = useI18n()

// Template usage
{{ t('settings.title') }}

// Locale files: src/locales/{lang}.ts
// Console logs/comments: English only
```

---

## 🔒 Security

### Token Safety
- Tokens stored in memory only, never persisted to disk
- All API requests use HTTPS
- DPAPI ensures only current Windows user can decrypt

### Risks
- ⚠️ May violate Discord's Terms of Service
- ⚠️ Could result in account suspension/ban
- ⚠️ Use on trusted devices only

---

## 🔨 Troubleshooting

### Common Issues

| Issue | Solution |
|-------|----------|
| `linker 'link.exe' not found` | Install Visual Studio Build Tools with C++ workload |
| `DPAPI error` | Ensure Windows SDK is installed |
| `pnpm not found` | Run `npm install -g pnpm` |
| `Rust outdated` | Run `rustup update stable` |

### Frontend-Only Development (Linux/macOS)

```bash
pnpm install
pnpm dev  # Runs Vite dev server only
```

> Note: Tauri commands won't work without Windows backend.

---

## 🤝 Contributing

1. Fork the repository
2. Create feature branch: `git checkout -b feature/amazing-feature`
3. Commit changes: `git commit -m 'Add amazing feature'`
4. Push to branch: `git push origin feature/amazing-feature`
5. Open a Pull Request

### Checklist
- [ ] Code follows conventions above
- [ ] `cargo fmt` and `cargo clippy` pass
- [ ] Console output is in English
- [ ] Comments are in English
- [ ] UI text uses i18n keys

---

## 📄 License

MIT License - see [LICENSE](LICENSE) file.

---

## 🙏 Credits

- [markterence/discord-quest-completer](https://github.com/markterence/discord-quest-completer) - Game simulation inspiration
- [power0matin/discord-quest-auto-completer](https://github.com/power0matin/discord-quest-auto-completer) - API automation inspiration

### Technology
- [Tauri](https://tauri.app/) - Application framework
- [Vue.js](https://vuejs.org/) - Frontend framework
- [shadcn-vue](https://www.shadcn-vue.com/) - UI components
- [TailwindCSS](https://tailwindcss.com/) - Styling
- [vue-i18n](https://vue-i18n.intlify.dev/) - Internationalization

---

**Remember**: This tool is for educational purposes only. Use responsibly and at your own risk.

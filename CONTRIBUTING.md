# Contributing to Discord Quest Helper

Thank you for your interest in contributing! This guide will help you get started.

## 📋 Requirements

- **OS**: Windows 10/11 (x64) or macOS (Apple Silicon)
- **Node.js**: 18.x+
- **Rust**: 1.70+
- **pnpm**: 10.x+
- **Visual Studio Build Tools** with C++ workload (Windows)
- **Xcode Command Line Tools** (macOS)

## 🚀 Getting Started

```bash
# Clone repository
git clone https://github.com/Masterain98/discord-quest-helper.git
cd discord-quest-helper

# Install dependencies
pnpm install

# Development mode
pnpm tauri:dev
```

## 🔨 Production Build

```bash
# Build application (automatically builds runner & CDP launcher sidecars first)
pnpm tauri:build
```

The `tauri:build` script automatically runs version sync, builds the game runner (`src-runner`) and CDP launcher (`src-cdp-launcher`) sidecar binaries, then builds the full Tauri application.

Output location: `src-tauri/target/release/bundle/`

## 📝 Commands

| Command | Description |
|---------|-------------|
| `pnpm install` | Install dependencies |
| `pnpm tauri:dev` | Development mode with hot reload (builds sidecars first) |
| `pnpm tauri:build` | Production build (builds sidecars first) |
| `pnpm dev` | Frontend dev server only (Vite on `:1420`) |
| `pnpm build` | Frontend type-check & production build |
| `pnpm test` | Run frontend tests (Vitest) |
| `pnpm i18n:check` | Validate i18n locale files |
| `pnpm sync-version` | Sync version from `public/version.txt` across configs |
| `pnpm build:runner` | Build game runner sidecar binary |
| `pnpm build:cdp-launcher` | Build CDP launcher sidecar binary |
| `pnpm analyze:har` | Analyze HAR files for quest data (Python) |
| `cargo fmt` | Rust formatting (run from `src-tauri/`) |
| `cargo clippy` | Rust linting (run from `src-tauri/`) |

## 🐛 Debugging

- **Frontend**: DevTools via `Ctrl+Shift+I` in app window
- **Backend**: Console output from `pnpm tauri:dev`
- **Verbose**: `RUST_LOG=debug pnpm tauri:dev`

## 🏗️ Project Structure

```
discord-quest-helper/
├── src/                              # Vue.js frontend
│   ├── api/tauri.ts                  # Tauri IPC bridge & TypeScript interfaces
│   ├── components/                   # Reusable UI components
│   │   ├── home/                     # Home view components (QuestListHeader, BatchActions, etc.)
│   │   ├── settings/                 # Settings view components (About, Account, Appearance, etc.)
│   │   └── ui/                       # shadcn-vue primitives (Button, Card, Dialog, etc.)
│   ├── composables/                  # Vue composables (useHomeQuestState, useSettingsNavigation)
│   ├── lib/utils.ts                  # cn() classname merge utility
│   ├── locales/                      # i18n translations (16 languages, JSON)
│   ├── stores/                       # Pinia state management (auth, quests, version, toast)
│   ├── utils/                        # Utility functions (navigate, questRewards, questTasks)
│   ├── views/                        # Page views (Home, GameSimulator, Settings, Debug)
│   ├── App.vue                       # Root component with tab navigation
│   ├── i18n.ts                       # vue-i18n configuration
│   └── main.ts                       # Vue app bootstrap
├── src-tauri/                        # Rust backend (Tauri 2)
│   └── src/
│       ├── lib.rs                    # Tauri commands (30+ IPC handlers) & app setup
│       ├── main.rs                   # Binary entry point
│       ├── token_extractor.rs        # Token extraction & decryption (LevelDB, DPAPI, AES-GCM)
│       ├── cdp_client.rs             # Chrome DevTools Protocol client
│       ├── cdp_quest.rs              # CDP-based quest completion
│       ├── discord_api.rs            # Discord HTTP API client
│       ├── discord_gateway.rs        # WebSocket gateway connection
│       ├── discord_cdp_launcher.rs   # CDP launcher management
│       ├── quest_completer.rs        # Quest completion logic
│       ├── game_simulator.rs         # Game simulation & process management
│       ├── super_properties.rs       # X-Super-Properties header management
│       ├── stealth.rs                # Stealth mode (random window title, cleanup)
│       ├── rpc.rs                    # Discord RPC client
│       ├── runner.rs                 # Activity runner parsing
│       ├── logger.rs                 # Structured in-memory logging
│       └── models.rs                 # Data structures & types
├── src-runner/                       # Game runner sidecar (minimal window exe)
│   └── src/main.rs                   # winit + softbuffer minimal process
├── src-cdp-launcher/                 # CDP launcher sidecar (launches Discord with CDP)
│   └── src/main.rs                   # CLI: --port, --channel, --restart, --status
├── scripts/                          # Build & utility scripts
│   ├── sync-version.js               # Version sync across package.json, Cargo.toml, tauri.conf.json
│   ├── build-runner.js               # Build game runner sidecar
│   ├── build-cdp-launcher.js         # Build CDP launcher sidecar
│   ├── i18n-validate.mjs             # i18n validation
│   └── analyze-har-quests.py         # HAR file quest analysis
├── public/                           # Static assets (version.txt, icons)
├── package.json                      # Node.js config (pnpm 10.x)
├── pnpm-workspace.yaml               # pnpm workspace config
├── vite.config.ts                    # Vite config (port 1420)
├── postcss.config.js                 # PostCSS with Tailwind CSS v4 plugin
└── tsconfig.json                     # TypeScript config
```

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

// For structured logging with sanitization, use the logger module:
// logger::log(LogCategory::Quest, LogLevel::Info, "Starting video quest");
// Note: println! is the primary logging mechanism in the codebase.

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

- All UI text must use `vue-i18n` keys.
- Source strings live in `src/locales/en.json`.
- Translations live in `src/locales/{locale}.json`.
- Do not edit generated Crowdin translation files unless fixing an urgent issue.
- Keep interpolation placeholders such as `{count}` and `{name}` unchanged.
- Console logs and code comments remain English only.

```typescript
// All UI text via vue-i18n
const { t } = useI18n()

// Template usage
{{ t('settings.title') }}
```

Run `pnpm run i18n:check` before submitting translation changes.

---

## 🔨 Troubleshooting

### Common Issues

| Issue | Solution |
|-------|----------|
| `linker 'link.exe' not found` | Install Visual Studio Build Tools with C++ workload |
| `DPAPI error` | Ensure Windows SDK is installed |
| `pnpm not found` | Run `npm install -g pnpm` |
| `Rust outdated` | Run `rustup update stable` |

### Frontend-Only Development (Linux)

```bash
pnpm install
pnpm dev  # Runs Vite dev server only on port 1420
```

> Note: Full Tauri backend builds require Windows or macOS. On Linux, only the frontend dev server is available.

---

## 🤝 Pull Request Process

1. Fork the repository
2. Create feature branch: `git checkout -b feature/amazing-feature`
3. Commit changes: `git commit -m 'Add amazing feature'`
4. Push to branch: `git push origin feature/amazing-feature`
5. Open a Pull Request

### Checklist

- [ ] Code follows conventions above
- [ ] `cargo fmt` and `cargo clippy` pass (from `src-tauri/`)
- [ ] `pnpm test` passes (frontend tests)
- [ ] `pnpm i18n:check` passes (if touching locale files)
- [ ] Console output is in English
- [ ] Comments are in English
- [ ] UI text uses i18n keys

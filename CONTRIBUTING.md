# Contributing to Discord Quest Helper

Thank you for your interest in contributing! This guide will help you get started.

## 📋 Requirements

- **OS**: Windows 10/11 (x64)
- **Node.js**: 18.x+
- **Rust**: 1.70+
- **pnpm**: 8.x+
- **Visual Studio Build Tools** with C++ workload

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
# Build game runner (optional, for game quests)
cd src-runner && cargo build --release && cd ..

# Build application
pnpm tauri:build
```

Output location: `src-tauri/target/release/bundle/`

## 📝 Commands

| Command | Description |
|---------|-------------|
| `pnpm install` | Install dependencies |
| `pnpm tauri:dev` | Development mode with hot reload |
| `pnpm tauri:build` | Production build |
| `pnpm dev` | Frontend dev server only |
| `cargo clippy` | Rust linting |
| `cargo fmt` | Rust formatting |

## 🐛 Debugging

- **Frontend**: DevTools via `Ctrl+Shift+I` in app window
- **Backend**: Console output from `pnpm tauri:dev`
- **Verbose**: `RUST_LOG=debug pnpm tauri:dev`

## 🏗️ Project Structure

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
└── README.md                     # Project readme
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

## 🤝 Pull Request Process

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

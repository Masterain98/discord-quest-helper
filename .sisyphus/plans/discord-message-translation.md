# Plan: Discord Message Translation Module

**Status:** approved for execution (momus review inline — subagent infra blocked this session; deviation documented in delivery report)
**Date:** 2026-10-29
**Reference:** E:\Github\X-Twitter---Personal-assistant (Chrome extension "X/Twitter + Discord AI Assistant"), Discord translation module.

## Goal

Port the reference Discord translation module into this Tauri app, adapted to the existing CDP injection architecture. Feature set (mirrors reference; documented cuts below):

1. Per-message **Translate** button injected into the Discord client; **auto-translate** of incoming messages with adaptive batching (8–24 msgs, 120–320 ms debounce) and code/command-skip heuristic.
2. **Markdown-aware translation**: protect code fences/inline code/links, preserve list/quote/heading prefixes per line.
3. **Two-level cache**: session LRU (200 entries) in injected JS; persistent cache in Rust (JSON file in app-data, max 120 entries, 24 h TTL) keyed `provider:targetLang:stableMsgId:hash(text)`.
4. **Providers**: `google-translate` (keyless), `microsoft-translator` (key + region), `deepl` (key). **LLM** (OpenAI-compatible: base URL + key + model) used for composer translation and channel summary.
5. **Composer Translate button**: translates the draft via LLM with channel context (recent messages, channel name/topic) and replaces the composer text.
6. **Channel summary**: fetch last 20/50 messages via Discord REST, summarize via LLM, render in a floating panel inside Discord with copy button.
7. **Settings UI** (new `translation` section) + i18n keys in all 16 locales.

**Explicit cuts (do not port):** catch-up auto-summary on channel switch, visual debug panel, per-server/channel settings overrides, guild-search button, polls/reactions features.

## Architecture

```
Vue Settings/Store ──tauri commands──► Rust
                                        ├─ translation.rs ──reqwest──► Google/MS/DeepL/LLM (no Discord token involved)
                                        ├─ discord_api.rs GET /channels/{id}/messages (summary source)
                                        └─ cdp_translate.rs ticker (~250 ms) ──Runtime.evaluate──►
                                             resident JS (window.__dqh_translate): MutationObserver,
                                             buttons, pending queue, rendered translations, toasts, panel
```

- Injected JS follows `cdp_quest.rs` conventions: JS template constants/fns, results via `JSON.stringify`, module/state cached on `window.__dqh_translate` with `_initVersion`, idempotent versioned init, behavior-based DOM detection (`[role="article"][aria-roledescription="Message"]`, `[id^="message-content-"]`, composer textbox `[role="textbox"]` inside the form) — never minified class names.
- The Rust ticker's `drain` call doubles as a health check: if the resident script is missing (Discord reloaded), the ticker re-runs init.
- Provider calls happen only in Rust. Provider errors: logged (`logger::log`), emitted as a tauri event, rendered inline in Discord (message slot / toast / panel). The ticker loop never propagates a provider failure — CDP connection is unaffected by construction.
- Provider **API keys** are stored via the `keyring` crate (Windows Credential Manager / macOS Keychain / Linux Secret Service); graceful fallback to plaintext settings file with a logged warning. Frontend only ever sees a boolean "key is set" + masked hint, never the key.
- **Consent/scope**: the whole feature is opt-in (off by default). Enabling it starts the ticker; disabling stops it and tears down the resident JS (observer disconnect + DOM cleanup).
- Rate limits: Discord REST message fetch throttled ≥1.2 s per channel, honoring 429 `Retry-After` and `X-RateLimit-Reset`-style headers; providers honor 429 `Retry-After` with a single retry; global provider concurrency capped (semaphore = 2).
- LLM base URL must be `https://` unless it points at localhost/127.0.0.1 (SSRF hygiene for a desktop tool).

## Tasks (ordered; verification per task)

### T1 — `src-tauri/src/translation.rs` (new)
Provider enum + request/response types; markdown-aware translation (token protection, line prefixes); provider HTTP calls (own reqwest client); 429 Retry-After handling; persistent cache (load/normalize/save; TTL + cap); cache-key builder (`hash` = 32-bit FNV-1a base36 — mirror reference semantics); keyring helpers (`set_provider_key`, `clear_provider_key`, `provider_key_loaded`, fallback file).
Pure-part unit tests: cache key composition, protect/restore round-trip (code fence, inline code, link, bare URL, list prefix), cache normalization (TTL eviction, cap eviction, malformed entries), DeepL language normalization, keyring fallback file write.
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml translation::` green.

### T2 — `discord_api.rs`: `get_channel_messages`
`pub async fn get_channel_messages(&self, channel_id: &str, limit: u32) -> Result<Vec<ChannelMessage>>` with per-channel throttle (last-fetch timestamps map) and 429 Retry-After respect. `ChannelMessage { id, author_username, content, timestamp }` (camelCase serde for frontend-facing paths only where needed).
Tests: throttle gate arithmetic, message JSON parsing (missing fields tolerated).
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml discord_api::` green.

### T3 — `src-tauri/src/cdp_translate.rs` (new)
- `JS_INIT_TRANSLATE` (resident observer + queues + render + buttons + panel + toasts), `js_drain_pending()`, `js_deliver_results(payload)`, `js_render_summary(payload)`, `js_teardown()`, composer translate flow, skip-code heuristic (ported logic).
- Rust: `start_message_translation(config)`, `stop_message_translation()`, `get_translation_status()`, `summarize_channel(channel_id, count)` commands; tokio ticker task with state in `AppState` (`translation: tokio::sync::Mutex<TranslationRuntime>`); error containment; `logger::log` on every failure path; tauri events `translation-status`, `translation-error`.
- Pure-part tests: drain/deliver JSON contract serde, config validation (https/localhost rule), queue item serde round-trip.
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml cdp_translate::` green.

### T4 — wiring
`lib.rs`: `mod translation; mod cdp_translate;`, AppState field init, register 5 new commands in `generate_handler!`. `logger.rs`: add `LogCategory::Translation` (+ Display arm; check exhaustive matches).
**Verify:** `cargo check --manifest-path src-tauri/Cargo.toml` clean; grep that new commands appear in handler list.

### T5 — `src/api/tauri.ts`
Types (`TranslationSettings`, `TranslationStatus`, `TranslationProvider`, `ChannelSummaryResult`, …) + wrappers: `startMessageTranslation`, `stopMessageTranslation`, `getTranslationStatus`, `setProviderKey`, `clearProviderKey`, `isProviderKeySet`, `testTranslationProvider`, `summarizeChannel`. Strict types, no `any`/`unknown`.
**Verify:** `pnpm build` (vue-tsc) clean.

### T6 — Settings UI
`src/components/settings/TranslationSettings.vue`: enable toggle (starts/stops via command; requires CDP), target language select, provider select + provider-specific fields (MS region; DeepL/LLM key set-clear flow showing only masked state; LLM base URL + model), auto-translate toggle, display mode (below/replace), accent color, summary count (20/50), **Test provider** button (acceptance A5: provider error surfaces in the UI). Register section in `useSettingsNavigation.ts`, `Settings.vue`, `SettingsNav.ts`/`SettingsOverview.vue` (follow existing section pattern). localStorage keys `dqh_translation_*` (existing pattern).
**Verify:** `pnpm build` clean; `pnpm test` green (nav section test updated if one enumerates sections).

### T7 — `src/stores/translation.ts` (new Pinia store)
State: settings, runtime status, last error; actions: enable/disable, save settings, test provider; event listeners for `translation-status`/`translation-error`. Vitest store test mirroring `gameIdle.test.ts` style (mock tauri module).
**Verify:** `pnpm test` green.

### T8 — i18n
`translation.*` namespace added to `en.json` and `pt-BR.json` (complete, human-quality) and mirrored in the other 14 locale files (de, es, fr, id, ja, ko, pl, pt-PT, ru, th, tr, vi, zh, zh-TW) with correct placeholders. No TODO markers (validator rejects them).
**Verify:** `pnpm i18n:check` passes.

### T9 — Gates + docs
`cargo fmt --manifest-path src-tauri/Cargo.toml`; `cargo clippy --workspace --all-targets --all-features` (zero new warnings — CI runs `-D warnings`); full `cargo test`; `pnpm build`; `pnpm test`; `pnpm i18n:check`. README feature bullet under existing feature list.
**Verify:** all commands exit 0 with fresh output in delivery report.

## Acceptance criteria mapping
- **A5** (provider error in UI + logger, no crash, no CDP loss): T3 error containment + T6 test-provider button + T7 error event; verified by unit tests + code inspection (no `unwrap` on provider paths, ticker catches all errors).
- **A6** (cargo test + frontend build, no new warnings): T9.
- **A7** (i18n pt-BR + en-US complete): T8 (goes further: all 16 locales per repo validator).

## Decisions / deviations
- Subagent infra (Agent tool) returned "request has been blocked due to unusual activity" on all attempts — metis/prometheus/momus roles executed inline by the conductor; plan still committed for the audit trail.
- `keyring` crate is a new dependency (OS-native secret storage). Fallback documented, no secrets in logs.
- No Rust-side settings file: settings live in localStorage and are passed per call (existing app pattern); only keys go to keyring.
- Discord token is NOT used for translation provider calls; only for the summary REST fetch (existing DiscordApiClient).

## Delivery
Branch `feature/discord-message-translation` from `main`; commit plan file with the change; PR per pre-authorized fork workflow (klaus-2). No merge without green gates.

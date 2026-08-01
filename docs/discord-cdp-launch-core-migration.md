# Discord CDP launch core migration

This document records the behavior and artifact baseline for the migration from
two copied Discord launch implementations to the shared
`discord-cdp-launch-core` crate.

## Architecture

- `crates/discord-cdp-launch-core` owns channel and install models, typed
  errors, Windows/macOS/Linux discovery and process lifecycle code, the synchronous
  localhost `/json` probe, launch argument construction, readiness polling, and
  the launch/restart state machine.
- `src-tauri/src/discord_cdp_commands.rs` is a Tauri-only adapter. It preserves
  the existing command names and four-field JSON result and runs the
  synchronous core through `tauri::async_runtime::spawn_blocking`.
- `src-tauri/src/cdp_client.rs` keeps async HTTP, WebSocket, JavaScript,
  interception, navigation, and quest behavior. It imports the shared
  `CdpTarget` and Discord target classification functions.
- `src-cdp-launcher` keeps CLI parsing, localized messages, Windows dialogs,
  DPI awareness, output, and exit-code policy.
- The root Cargo workspace owns the shared lockfile, target directory, and
  `sidecar-release` profile.

The core dependency boundary is enforced by
`pnpm run check:cdp-core-deps`; neither `tauri` nor `tauri-plugin-*` may appear
in its dependency tree.

## Migration baseline

Captured on Windows x64 on 2026-07-26 before the migration:

| Item | Baseline |
| --- | --- |
| Launcher profile | Former member-level `release` profile |
| Launcher size | 382,976 bytes |
| Launcher SHA-256 | `3FD8CE060295300AA23D052ED190C5B1B184CBDA94CBA4877689ECEEB833EC10` |
| CDP endpoint | `http://127.0.0.1:9223/json` reachable |
| Discord target | Page target titled `Quests` at a `discord.com` URL |
| Installed channels | Stable only |
| Stable executable | `%LOCALAPPDATA%\Discord\app-1.0.9249\Discord.exe` |
| PTB / Canary | Not installed on the validation host |
| macOS launcher | No local artifact; Windows host cannot establish a macOS size baseline |

Before migration, the Tauri implementation parsed JSON and waited up to 15
seconds for readiness. The standalone Launcher searched the raw HTTP response
for strings and returned immediately after spawning.

## Post-migration artifact review

Windows x64 `sidecar-release` result:

| Item | Result |
| --- | --- |
| Launcher size | 412,160 bytes |
| Launcher SHA-256 | `A56FF4AE91EC6127CB20D15B570B75A45A1B443A65B094E8B913E1F4006AEBF0` |
| Size delta | +29,184 bytes (+7.62%) |
| `--status --port 9223` | Exit 0; reports available |
| `--status --port 65534` | Exit 3; reports unavailable |

The size increase is accepted for this migration because it replaces
unstructured substring matching with typed `serde_json` parsing and makes the
standalone Launcher wait for a real Discord target before reporting launch
success. The build script prints the resulting byte size in CI so future
changes remain reviewable.

A live probe exposed a Chromium behavior not covered by the first synthetic
server tests: a complete `Content-Length` response can arrive while the TCP
connection remains open. The probe now stops reading once the declared body is
complete, and the regression suite covers that case.

## Windows packaging evidence

- Tauri produced the main executable under the root `target/release` directory.
- MSI creation succeeded at
  `target/release/bundle/msi/Discord Quest Helper_0.9.0_x64_en-US.msi`.
- The MSI `File` table contains exactly one
  `discord-cdp-launcher-sidecar.exe`, with size 412,160 bytes.
- The portable validation ZIP contains both
  `discord-quest-helper.exe` and `discord-cdp-launcher-sidecar.exe`.

## Validation coverage

The shared crate tests cover:

- all documented channel aliases and invalid input;
- numeric Windows `app-*` ordering and direct executable fallback;
- requested and automatic install selection;
- launch arguments with and without `--remote-allow-origins=*`;
- already-ready, running-without-restart, restart, shutdown timeout, occupied
  port, readiness success, readiness timeout, missing install, and automatic
  channel order state-machine paths;
- Discord, non-Discord Chromium, missing WebSocket URL, malformed JSON, HTTP
  500, stalled response, unrelated TCP service, open-connection
  `Content-Length`, and unreachable-port probe cases;
- Tauri DTO JSON compatibility and standalone CLI parsing.

## External platform acceptance

The macOS platform implementation and the standalone Launcher compile for
`aarch64-apple-darwin`, and macOS discovery uses injectable roots in its
platform-specific test. A complete macOS Tauri check or bundle cannot be
produced on the Windows validation host because Objective-C dependencies
require an Apple-target C compiler and macOS frameworks. The macOS CI runner
remains responsible for the real Stable/PTB/Canary process-name check, launcher
size, app bundle, and sidecar-content acceptance.

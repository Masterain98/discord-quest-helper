# CDP runtime verification and diagnostics

This replaces route-based CDP readiness and primary-target selection. The shared
`discord-cdp-launch-core` probe is used by the Helper and standalone launcher;
Tauri login, navigation, network capture and task operations select targets with
the same runtime verification.

## Readiness contract

Discovery accepts HTTP(S) page targets on `discord.com`, `discordapp.com`, or
their dot-delimited subdomains. Paths and page titles do not determine readiness.
A debugger URL must use an uncredentialed, plaintext loopback WebSocket on the
discovered port. The probe does not use proxies or override Origin.

`Runtime.evaluate` reads the app mount node, initialized webpack chunk loader,
optional Discord/Vesktop native bridge, focus and document generation. Readiness
requires the app root and module loader; authentication and native-bridge presence
are independent. A title/URL claiming to be Discord cannot qualify an updater,
overlay, blank page or ordinary page without that environment. An authenticated
Discord web renderer with the same environment is not distinguished by this
renderer probe alone; process/installation ownership remains a separate check.

Qualified targets rank by native bridge, then focus, then target ID. One probe's
verified target supplies its state and diagnostic classification. Each round has
a three-second deadline, and each target has at most 750 milliseconds including
connection, handshake and I/O. Events and Ping frames may interleave with replies.
Protocol failures and JavaScript exceptions return constant reason codes instead
of arbitrary remote error text. Every path releases its connection.

The probe does not read credentials or account stores, send business requests,
install persistent variables or navigate. The existing `discordReady`,
`connected` and `cdp_connected` fields indicate this basic verification only.

The added `runtime` object contains:

| Field | Meaning |
| --- | --- |
| `runtimeStatus` | `ready`, `loading`, `unsupported`, `probeFailed`, `noCandidate` |
| `webSocketReachable` | Debugger WebSocket handshake succeeded |
| `appRootPresent` | Main app mount node exists |
| `moduleLoaderPresent` | Discord webpack loader is initialized |
| `nativeBridgePresent` | Discord or Vesktop native bridge exists; optional |
| `failureStage`, `reasonCode` | Sanitized discovery/connection/handshake/evaluation failure |

Loading and failed verification show wait/retry guidance in all existing locales.
A reachable endpoint with an insufficient runtime does not open a restart dialog.
The debug view and sanitized export expose runtime capabilities; the export omits
document generation and arbitrary extra runtime properties.

## Sessions and operations

Session discovery uses exact executable identity and debug arguments independently
of renderer readiness. Loading or failed verification does not remove a known
session. Conflicting installation claims for a port remain an ambiguity error.
Restoration checks process exit and endpoint closure independently. A failed kill
is rechecked against the original PID/start-time/executable identity to tolerate
Electron children exiting concurrently; a surviving process still fails safely.
No administrator elevation or broader process termination was added.

Quest module discovery checks only methods needed by the requested operation.
Games do not require streaming methods, and video/PLAY_ACTIVITY do not require a
game store. HTTP facade discovery uses structural inspection instead of a trial
request. Discovery precedes initialization, retries at most three times within a
two-second budget, and produces `cdp_capability_missing` with operation and missing
method names. Structured quest errors remain readable by the existing frontend
error callback. Extending a module cache preserves active patches and originals.

Tasks bind the verified target ID and document generation before initialization.
Their independent monitor detects closure/reload during waits; evaluations also
guard the generation before executing. Activity iframes retain their own document
binding. Failure stops the task and triggers cleanup without selecting another
renderer. Cleanup still visits Discord page targets, including auxiliary windows,
and validates their loopback debugger URLs. Existing quest-specific warmup
navigation remains separate from connection polling and readiness checks.

## Validation record — Windows, 2026-10-01

Completed before the user's instruction to stop additional testing:

- Frontend: 19 files, 104 tests passed; TypeScript/Vite build passed. Vite retains
  its existing large-chunk warning.
- Rust workspace: 255 tests passed, 11 live/environment tests ignored. Coverage
  includes runtime/route independence, absent environment, Vesktop without a native
  bridge, events/Ping, handshake failures and slow handshakes, timeouts, exceptions,
  closure, ranking, release of connections, session discovery without readiness,
  capability isolation, structured errors and pinned-target reload/closure.
- Strict workspace Clippy passed. Formatting, all locale checks and the
  Tauri-free core dependency check passed.
- The final task WebSocket timeout/Ping/close handling and fixture formatting
  adjustment passed `cargo check`; the full suites were not repeated afterward.
- Windows x64 standalone launcher `sidecar-release`: 589,312 → 916,992 bytes
  (+327,680 bytes, +55.60%). This includes synchronous WebSocket protocol support
  and URL parsing; no Tauri dependency was introduced. No package was published.

On the authorized local Discord 1.0.9259 client, Friends (`/channels/@me`), Nitro
(`/store`), Shop (`/shop`) and Quests (`/quest-home`) all reported `discordReady`,
one dynamically qualified renderer and one exactly associated installation.
Document generation changed across full page navigation. No quest was claimed,
started or completed for this validation.

Process/permission snapshots were taken before and after the normal/CDP restart
attempts. Main process PID 44136, normal-mode PID 33764, and CDP restart PIDs
81312/56444 were Medium integrity, not elevated, and allowed termination.
Unrelated elevated DiscordSystemHelper processes were outside the selected tree.

Restoration initially returned a child termination failure for PID 54696; the
client processes and endpoint had already exited. The identity recheck described
above addresses this exit race. A subsequent restoration successfully launched
normal mode with no debug flag and a closed endpoint.

Normal → CDP restarted the exact installation and opened port 9223. The updater
then failed its manifest request to `updates.discord.com` with Windows error
10054 (`ConnectionReset`), displaying “Update failed — retrying”. The launcher
reported `ReadinessTimeout` after 30 seconds, `cdpWithoutDiscordTarget`, one target,
zero Discord candidates, and no verified main renderer. The process session
remained discoverable despite that state. The full restart-to-ready acceptance
criterion therefore remains unverified under this network failure. The last
observation found no Discord process or listening endpoint; no further restart
was performed.

macOS/Linux behavior is left to the existing CI matrix. Live Vesktop, live login
capture/server validation after this change, and actual quest execution were not
validated in this run. Fixture success does not establish those live outcomes.

## Troubleshooting

Use the debug snapshot to separate endpoint accessibility, runtime verification,
process ownership, authentication, and operation capabilities. `noCandidate` with
only an updater target is not proof that CDP is disabled. `ready` with failed login
requires investigating session capture/authentication rather than restarting.
`cdp_capability_missing` identifies the operation and missing methods; changing a
route is not a connectivity remedy. `cdp_target_invalidated` means the bound
renderer closed or reloaded and the operation was stopped.

Share the sanitized diagnostic export and exact error code/message. Do not attach
authorization headers, tokens, cookies or private account content.

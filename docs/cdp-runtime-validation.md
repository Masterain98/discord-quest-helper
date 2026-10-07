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
verified target supplies its state and diagnostic classification. Candidates are
verified concurrently so stalled earlier targets cannot starve later renderers. Each round has
a three-second deadline, and each target has at most 750 milliseconds including
connection, handshake and I/O. Events and Ping frames may interleave with replies.
Up to 32 probe workers take candidates from a shared queue; having more pages
does not reject the endpoint. Workers stop taking entries at the round deadline,
and unprocessed candidates receive `probe_round_deadline` diagnostics. Thread
creation failures and worker panics become diagnostic probe failures; ready
results from other workers remain eligible for selection.
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
Only an explicit `loading` runtime establishes that the client is loading.
`noCandidate` is shown as verification unavailable: an updater, an unrelated
Chromium endpoint and an empty target list cannot be distinguished by that status.
An explicitly loading renderer may enter the existing manual restart confirmation
when the endpoint owner matches a running, validated CDP-capable installation.
Unknown owners and other insufficient runtime states still report verification
unavailable. Non-CDP port occupation reports a port conflict before any launch or
restart attempt.
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
game store. Stream operations require both streaming and companion-game methods
before installing state, and both spoofs must succeed before polling. Startup
failure triggers cleanup rather than silently continuing.
HTTP facade discovery accepts concrete methods on the object or its prototypes,
including accessor-backed functions. Callable low-level HTTP library exports are
excluded: they accept URL strings rather than Discord's request options and may
coexist with the authenticated facade. Dynamic-method proxies and ambiguous object matches
are rejected without a trial request. Discovery precedes initialization, retries
at most three times and shares a two-second budget with initialization, producing `cdp_capability_missing` with
operation and missing method names. Structured quest errors remain readable by the
existing frontend error callback. Extending a module cache preserves active patches
and originals. The selected renderer is verified once before that two-second
budget begins; both evaluations retain their document-generation guard.

Tasks bind the verified target ID and document generation before initialization.
Initial binding retries discovery at most three times with 250ms intervals to
tolerate a loading renderer; a bound task never discovers a replacement.
Their independent monitor detects closure/reload during waits. Inconclusive probes
retry the same target up to three times; a confirmed generation change stops
immediately. Evaluations also guard the generation before executing. Activity
iframes retain their own document
binding. Failure stops the task and triggers cleanup without selecting another
renderer. Cleanup still visits Discord page targets, including auxiliary windows,
and validates their loopback debugger URLs. Quest startup and manual game
simulation discover modules on the current page without route warmup, History API
writes or full-page navigation. Missing capabilities report the existing error
rather than navigating to load them. Activity execution retains its own necessary
client operations.
JSON task execution reuses one verified renderer and its document guard per call;
the separate pinned-session monitor remains active.

Video startup first reads a valid enrollment timestamp from QuestsStore. If it
is missing or invalid, a read-only `/quests/@me` request resolves the requested
quest from either an array response or `body.quests`, accepting snake_case and
camelCase enrollment fields. The request has a ten-second timeout and reports
missing quests, unenrolled quests, invalid timestamps, malformed responses and
request failures before submitting any video progress. CDP awaits this bounded
preflight only; the video progress loop remains a globally retained Promise that
Rust polls. Stop requests are handled during this preflight, invalidate the
module bridge, and report the quest as stopped. Late enrollment responses cannot
restart the run. The progress loop checks its stop flag and bridge identity before
submitting again, including retries and the final submission.

Activity SDK capability discovery also checks at most three times within two
seconds. Its retries are spaced across that budget; the former 12-second discovery
wait is intentionally not retained. SDK readiness and actual command timeouts are
separate from capability discovery.

## Validation record — Windows, 2026-10-01 (Asia/Taipei, UTC+08:00)

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
Document generation changed across full-page navigation. No quest was claimed,
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

## PR #186 review follow-up — 2026-10-01 (Asia/Taipei)

The audit retrieved one conversation comment, three review submissions and all
eight inline threads, including their resolved/outdated state. All eight threads
were unresolved and current when inspected. The manual-spoof finding was repeated
by two reviewers. Embedded bot prompts and suggested commands were treated as
review data, not instructions.

| Review comment ID | Finding | Decision and change |
| --- | --- | --- |
| 4149081704, 4149120478 | A cancelled manual start skips its internal rollback | Valid. The command now attempts cleanup outside the cancelled pinned-session future before returning the start error. |
| 4149081715 | Activity SDK checks end after the former 750ms window | Partly valid. Three read-only checks now span about 1.8 seconds within the specified two-second limit. Restoring the old 12-second discovery wait would violate the approved timing contract. |
| 4149081727 | Initialization adds another independent two-second timeout | Valid. Discovery and initialization now use the same absolute deadline; initialization timeout reports its missing capability stage. |
| 4149120424 | Several ready targets are all diagnosed as main renderers | Valid. Only the selected target ID receives the main-renderer classification; other targets keep their runtime capability results. |
| 4149120433 | The validation date is in the future | Incorrect date premise. The PR was created on September 30 UTC, which was already October 1 in Asia/Taipei. The heading now states the timezone, and “full-page navigation” is hyphenated. |
| 4149120468 | One inconclusive probe falsely invalidates a running task | Valid. A missing generation or temporarily incomplete runtime retries the same bound target up to three times. A known changed generation still stops immediately; no replacement target is selected. |
| 4149120495 | An updater-only reachable endpoint is shown as a verification error | The initial change treated `noCandidate` as waiting. Further review found that this also labels unrelated endpoints and empty target lists as loading, so that inference was removed. Only explicit `loading` uses the waiting presentation; `noCandidate` reports verification unavailable without prompting a restart. |

The CodeRabbit docstring-coverage warning is a generic bot threshold, not an
existing repository gate or a concrete defect. No broad docstring expansion was
made. Sourcery's size-limit notice and the empty Greptile review submission add no
separate code findings; CodeRabbit's summary repeats its inline findings.

Reading the existing CI run (36773103164) also identified a strict Clippy failure
on all three platforms: `result_large_err` on the HTTP handshake callback in the
core WebSocket fixture. tungstenite fixes that callback's error type to an
unboxed HTTP `ErrorResponse`, so the fixture now has a documented, local lint
exception. The new pinned-target fixture uses the same exception; production
lint settings and GitHub Actions references remain unchanged.

Regression cases were added for preferred-target diagnostics, transient and
loading probes on a pinned target, updater startup presentation, and delayed SDK
availability without invoking commands. No additional tests, builds or live
client operations were run for this follow-up, respecting the instruction to stop
testing. The results recorded above apply to the earlier implementation, not to
these review changes.

A subsequent review of `loginFlow.ts` correctly identified that the `noCandidate`
waiting presentation hid non-Discord endpoints. The classifier and both login
attempt messages now treat that status as verification unavailable. Regression
cases cover empty target lists and unrelated Chromium targets; the explicit
`loading` case still waits. These additional cases were not run locally.

## PR #187 review follow-up — 2026-10-01 (Asia/Taipei)

Retrieved one conversation comment, three review submissions and all nine inline
threads. All nine threads were unresolved and current at the audited head
`251d1f42ca3bbc7a7fe198af8f976eb1e23e6e3b`. Every inline finding was verified
against that code; inherited/accessor facade rejection and URL-only navigation
success were also reproduced by executing the existing scripts in isolation.

| Review comment ID | Verified issue and fix |
| --- | --- |
| 4155989347 | Loading clients were blocked before recovery. Both login entry points now permit manual restart confirmation for an explicitly loading renderer with an identified, running, validated installation. Unknown endpoints remain unavailable. |
| 4155989361 | Stream startup swallowed companion-game failures. Combined capability discovery now precedes both patches; any startup failure cleans up and exits before progress polling. |
| 4155989368 | Sequential probes could exhaust the round budget before a ready later target. Scoped concurrent probes retain the shared three-second and per-target 750ms deadlines and deterministic ranking. |
| 4155989376 | Stream polling verified the same target twice consecutively. One foreground verification remains before API polling. |
| 4156017290 | Initial target binding failed after one loading probe. It now attempts discovery up to three times, 250ms apart; a bound task never discovers a replacement. |
| 4156017323 | HTTP methods on prototypes or accessors were discarded. Structural descriptor inspection now accepts these forms while rejecting dynamic-only proxies and ambiguous candidates without HTTP trial requests. |
| 4156017335 | History navigation checked only the URL changed by pushState. Independent router state is now required for every SPA success; failed History attempts restore the URL before fallbacks. |
| 4156017348 | Renderer verification consumed the module discovery/initialization budget. One verification now precedes the shared two-second deadline, and both evaluations guard the same document generation. |
| 4156017356 | A non-CDP port conflict appeared as a runtime failure. Both entry points now show a dedicated port-conflict message, translated in all 16 locales, before launch/restart. |

The generic CodeRabbit docstring threshold is not a repository gate and prompted
no broad documentation changes. Sourcery's size-limit notice, the empty Greptile
review and CodeRabbit's summary add no separate code findings. At the user's
request, no PR replies or thread-resolution mutations are part of this follow-up.

Local validation after the fixes:

- Frontend: 21 files, 136 tests passed; TypeScript/Vite production build passed
  with the existing large-chunk warning. All locales passed with zero warnings.
- Rust workspace: 262 tests passed, 11 live/environment tests ignored. New
  fixtures cover slow earlier targets, bounded initial binding, startup rollback,
  the module budget and document invalidation between discovery and installation.
- Strict workspace Clippy, formatting, diff checks, Tauri-free core dependency
  and runtime-identity checks passed; identity suites passed 14 tests with one
  platform-specific case skipped.
- Windows fixtures explicitly use blocking accepted sockets, read complete HTTP
  requests and retain written HTTP responses briefly before releasing them, to
  avoid connection-reset races during parallel tests. The reload fixture tests
  foreground verification independently of the separately tested task monitor.

No real Discord quest, client restart or account operation was performed. macOS
and Linux validation remains with the existing remote CI matrix. The SPA fallback
may use full-page navigation when a client exposes no independent router location;
fixture success does not establish live routing or heartbeat behavior.

## PR #187 second review and CI follow-up — 2026-10-01

Refreshed all surfaces at `fae722d0a48405e1b899527114a2788a6e1850e8`:
one conversation comment, five review submissions and ten inline threads, with
all pagination exhausted. The original nine threads were already resolved by
external actors. One new inline issue, one outside-diff finding in the latest
review body, and one concrete concern in the updated conversation summary were
valid:

| Source | Finding and outcome |
| --- | --- |
| Greptile 4156530649 | A successful router transition without exposed location always failed verification. Real router methods now accept their own observed URL transition when state is unavailable. Synthetic History writes still require independent state, and readable contradictory state still fails. |
| CodeRabbit review 5380708444, outside diff | JSON task execution verified its renderer twice. It now verifies once, executes on that target with its document-generation guard, and reports that same target in result metadata. |
| CodeRabbit conversation 5932532363, architecture review | Unbounded candidate counts created unbounded native threads. A 32-candidate cap rejects oversized lists before opening probe sockets, retains diagnostics, and handles worker creation failures and panics. |

The proposed cleanup-ownership redesign did not establish a regression introduced
by this PR and remains outside this targeted fix. The docstring warning is still
not a repository gate; Sourcery's size-limit notice adds no code finding.

The macOS push CI run `36875539331`, job `110413885449`, failed in
`module_budget_excludes_verification_and_both_scripts_guard_the_document` with
`ProbeFailed (read_failed)` before module discovery. Its fixture delayed runtime
verification by 600ms within the production 750ms cap. The test depended on real
network scheduling headroom. Budget policy now has a virtual-clock regression:
three seconds of verification are excluded, two 700ms scripts succeed, and two
1200ms scripts time out at their shared two-second deadline. A separate socket
regression verifies one probe per JSON execution and the document guard; the
existing reload-between-discovery-and-installation regression still exercises
both guarded module evaluations. Production timeouts and CI action refs are
unchanged.

Local validation: all 21 frontend files / 139 tests and the Rust workspace passed.
Strict workspace Clippy, Rust formatting, TypeScript/Vite build, all locales,
core dependency boundary, runtime-identity configuration and identity test suites
passed. The existing Vite chunk warning remains. Routing fixtures cover all three
router methods without state, no-op methods, History URL-only changes, aliased
Window.location and contradictory router state. Oversized-list regression asserts
no probe socket is opened. No PR reply, thread mutation or real Discord operation
was performed. Cross-platform confirmation comes from the new remote CI run.

## PR #187 candidate queue follow-up — 2026-10-01

Greptile comment `4156949973` is valid at
`a62d934ccf65ec239ef6b82b74fd7c434ba98ff4`: rejecting every list above 32
candidates can hide a ready main renderer among legitimate popout windows. The
second-round candidate cap was too restrictive. It is now a concurrency limit:
at most 32 native workers drain the entire queue until the shared three-second
deadline. Per-target verification still has a 750ms cap, and completed results
retain native-bridge/focus/ID ranking. Worker failures do not discard successful
results. A finite deadline can still leave entries unprocessed in a sufficiently
large, slow list; these entries have explicit diagnostics rather than causing
all candidates to be rejected upfront.

Regression fixtures find the last ready renderer among 40 pages, reach a ready
33rd candidate after an entire 32-worker batch stalls, verify all 96 queued
entries with no more than 32 worker threads, and preserve ready results when
another worker panics. An expired-round case confirms no late probes run.
The previous commit's Windows, macOS and Linux push/PR CI matrices passed,
including the formerly failing macOS budget test. No PR reply, thread-resolution
mutation or real Discord operation is part of this follow-up.
Local validation passed: 267 Rust workspace tests, 11 live/environment tests
ignored, strict workspace Clippy, formatting, dependency boundary and diff checks.

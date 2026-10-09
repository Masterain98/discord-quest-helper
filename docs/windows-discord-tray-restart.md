# Windows tray cleanup during client restart

Force-terminating Discord can leave its old notification icon registered with
Explorer. Hovering later can remove that stale entry after the new client has
already started. Waiting for process exit alone does not delete the tray entry.

All Windows shutdown entry points in `discord-cdp-launch-core` now share this
sequence, including channel restarts, exact-installation/Vesktop restarts and
restoring the client to normal mode:

1. Snapshot the selected process tree's identities and retain native handles to
   the original processes. Discover its `Electron_NotifyIconHostWindow` windows.
2. Capture the matching tray identities, including GUID icons and collapsed
   overflow icons, before their callback windows disappear.
3. If an original process advertises a CDP port, verify its IPv4 listening PID
   through the Windows TCP table. Discover a Discord page and recheck ownership
   immediately before sending `Browser.close`. Allow two seconds for normal exit.
4. Refresh the tray snapshot in case a slow or vetoed exit recreated an icon.
   Terminate remaining processes with the existing PID/start-time/executable
   checks. A missing CDP endpoint goes directly to this fallback.
5. Wait for the original native process handles to signal, then remove only the
   captured old icons using `Shell_NotifyIconW(NIM_DELETE)`. Complete this before
   returning to the caller which launches the replacement process.

Electron handles `Browser.close` by calling `Browser::Quit()` and deliberately
does not acknowledge the command before exiting. Therefore a CDP disconnect is
treated as a submitted request; only process exit establishes success. A vetoed
quit or an unresponsive client still falls back to termination. The current
local Discord client does not expose renderer-side `DiscordNative.app.quit()`.

Tray enumeration uses Explorer's `ITrayNotify` Windows 8+ COM callback interface,
also used by Chromium's `StatusTrayStateChangerWin`. The callback copies only
scalar identities for windows owned by the selected processes; it does not read
tooltip strings or change notification preferences. It unregisters immediately.
This interface is undocumented and may change across Windows versions. Capture
uses a dedicated COM apartment, a one-second caller budget and at most one
outstanding worker. If unavailable, a bounded documented `Shell_NotifyIconGetRect`
probe covers non-GUID IDs 1–1024. GUID cleanup cannot be guaranteed when Explorer
does not provide the enumeration interface; this fallback is logged.

Cleanup preserves live owners and reused window handles. Before removing a GUID,
it also checks that another instance of the same executable has not started.
On partial termination failure, only icons with confirmed dead owners are
eligible for cleanup. An unavailable Explorer or an already removed icon does
not prevent the restart. No pointer movement, global tray sweep, Explorer restart
or notification preference/cache deletion is performed.

## Validation

Run the ordinary core/launcher tests and Clippy on all supported platforms:

```powershell
cargo test -p discord-cdp-launch-core -p discord-cdp-launcher --all-features
cargo clippy -p discord-cdp-launch-core -p discord-cdp-launcher --all-targets --all-features -- -D warnings
```

The opt-in native tests require an interactive Windows session with Explorer.
They copy a disposable fixture into unique temporary directories and manipulate
only those fixture processes and icons. They do not restart the user's Discord:

```powershell
cargo build -p discord-cdp-launch-core --example windows_tray_fixture
cargo test -p discord-cdp-launch-core --all-features native_ -- --ignored --nocapture --test-threads=1
```

These cover graceful CDP exit without an acknowledgment, an acknowledged but
unresponsive client, forced exit without CDP, GUID icons, recreated/high icon IDs,
preservation of another installation, live owners and reused HWNDs. Ordinary
tests also cover TCP listener ownership, ownership changes during discovery and
unsupported close commands. Native shell tests were run on Windows 10 build
19045; Windows 11 and a restart of the user's actual Discord remain separate
runtime acceptance checks.

Implementation references:

- [Electron shutdown delegate](https://github.com/electron/electron/blob/main/shell/browser/ui/devtools_manager_delegate.cc)
- [Chromium notification enumeration](https://github.com/chromium/chromium/blob/main/chrome/browser/ui/views/status_icons/status_tray_state_changer_win.cc)
- [Microsoft icon identities](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-notifyiconidentifier)
- [Microsoft icon removal API](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw)

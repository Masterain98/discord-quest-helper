# Desktop client provider migration

The CDP launcher now models a desktop client independently from Discord's release channel.

## Public model

- `ProviderId`: product family (`discord.official`, `vencord.vesktop`).
- `VariantId`: provider-specific variant such as `stable`, `ptb`, `canary`, or `flatpak`.
- `InstallationId`: stable provider-scoped ID derived from an exact launch target.
- `ClientInstallation`: discovery source, validation, capabilities, and a `LaunchTarget`.
- `LaunchTarget`: executable + working directory, macOS bundle, or Flatpak app ID.
- `LaunchSelector`: Auto, provider/variant, or exact installation.
- `SessionOwnership`: Helper-managed or externally attached.

Providers are registered at compile time through `DesktopClientProvider`. A future client should implement discovery, validation, process matching, and launch-target construction. Process termination, CDP readiness, owner checks, session enumeration, and restoration belong to the shared supervisor/lifecycle layer.

## Tauri API migration

New consumers should use:

- `get_desktop_client_state`
- `set_desktop_client_selection`
- `add_desktop_client_installation`
- `remove_desktop_client_installation`
- `launch_desktop_client_cdp`
- `list_running_desktop_cdp_sessions`
- `restore_desktop_client_session`

`get_desktop_client_state` is the authoritative atomic snapshot. It returns the request port and a monotonic revision; clients must ignore stale responses. Endpoint states are `unreachable`, `occupied`, `nonDiscordCdp`, and `discordReady`. Only `discordReady` is usable for login.

The legacy `list_desktop_clients`, `launch_discord_cdp`, `restart_discord_cdp`, and channel arguments remain compatibility adapters for one release.

## Local configuration

`desktop-clients.v1.json` is stored under Tauri's application config directory. It contains only provider IDs, launch locators, validation state, and selection. It never stores Discord credentials, captured headers, Vesktop session data, or `VENCORD_USER_DATA_DIR`.

Discovery priority is saved exact installations, running-process paths, OS metadata, PATH/XDG/standard paths, then explicit browse. No recursive full-disk scan is performed. A missing saved path remains selected and visible for relocation; the launcher does not silently fall back to official Discord.

## Sidecar and shortcuts

The launcher accepts `--client` (or `--provider`) and optional `--installation <executable>`, in addition to the legacy channel and port arguments. Windows `.lnk`, macOS `.command`, and Linux `.desktop` generation preserve these arguments.

## Vesktop constraint

Vesktop is a CDP provider only. The authenticated token captured through CDP stays in Rust memory and is never returned to the WebView. Local Token extraction remains limited to official Discord profiles until a reproducible on-disk Vesktop Token source is demonstrated.

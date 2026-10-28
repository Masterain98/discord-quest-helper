# Local token extraction

Auto Detect Token reads existing local profiles when the user requests local
login. The native authentication command validates candidate tokens with
Discord's `/users/@me` endpoint before returning detected accounts. Extraction
does not change a client profile or install/start another client.

## Supported discovery

`src-tauri/src/token_extractor/profiles.rs` is the client metadata registry.
It declares profile directory aliases, Linux Flatpak IDs, and the OS credential
names for Discord Stable/Canary/PTB, Equibop, Legcord, Vesktop, ArmCord and WebCord.
Add client metadata there rather than adding a separate filesystem scanner.

| Platform | Standard profile root | Encrypted-candidate key source |
| --- | --- | --- |
| Windows | `%APPDATA%/<client>` | The storage root's `Local State`, falling back to the profile root's `Local State`; DPAPI unwraps the key |
| Linux | Absolute `$XDG_CONFIG_HOME/<client>`, otherwise `~/.config/<client>` | Legacy OSCrypt v10 compatibility key, or the client's Secret Service key for v11 |
| macOS | `~/Library/Application Support/<client>` | The client's Safe Storage Keychain item, with native API and `security` CLI fallback |

All platforms scan both `Local Storage/leveldb` and
`sessionData/Local Storage/leveldb` beneath each profile. Only `.ldb` and `.log`
files are examined. Plaintext candidates need no OS credential or `Local State`;
keys are loaded lazily only for encrypted candidates. Keys and failed key lookups
are cached for one profile scan. Windows caches each storage root's key separately.

Linux additionally checks the official Discord Snap profile and registered
Flatpak roots beneath `~/.var/app/<application-id>/config/<client>`. Equibop's
current `org.equicord.equibop` and earlier `io.github.equicord.equibop` IDs are
both included. ArmCord uses `xyz.armcord.ArmCord`.

The scanner continues past individual I/O and decryption failures. A working
layout's tokens survive a failure in another layout. If no candidate file can
be read, the retained filesystem error is returned rather than silently reporting
an empty scan. If only encrypted candidates exist and none can be decrypted,
the credential/decryption failure is retained. Diagnostics contain counts and
sanitized profile paths, never token values or key bytes.

## Compatibility boundaries

- Standard installed profiles are covered; arbitrary custom directories,
  portable `Data`/`legcord-data` folders and custom Electron session partitions
  are not searched recursively. Use CDP or manual login for these layouts.
- Legacy Windows AES-GCM and macOS/Linux AES-CBC OSCrypt formats are supported.
  New providers/encryption versions and Linux KWallet/portal-only keys may need
  another login method. Unknown encryption formats are not guessed.
- Keychain/Secret Service permissions still apply. Locked or unavailable keys
  can prevent encrypted-candidate extraction; plaintext extraction remains usable.
- Local token support does not register additional clients with the CDP launcher.
- Scanning raw LevelDB files is best effort: stale/deleted records or an actively
  changing database can produce invalid candidates or omit a token. Native
  Discord validation remains authoritative.

## Upstream evidence (checked 2026-10-28)

- [Electron app paths](https://www.electronjs.org/docs/latest/api/app#appgetpathname)
  documents platform roots and the independent `sessionData` path.
- [Electron safeStorage](https://www.electronjs.org/docs/latest/api/safe-storage)
  documents the platform-specific key providers and permission requirements.
- [Vesktop constants](https://github.com/Vencord/Vesktop/blob/3cb2a7bc1306923b60157231430c5b4e78e531da/src/main/constants.ts)
  and [Equibop constants](https://github.com/Equicord/Equibop/blob/57406f627221bc6054deb9834c18fa9cd914d96a/src/main/constants.ts)
  explicitly place Electron session data under `sessionData`; Windows portable
  installs can instead use an executable-adjacent `Data` directory.
- [Legcord portable configuration](https://github.com/Legcord/Legcord/blob/a5f0a9c3b5d6adab74e203656ed8b51bd831ccb2/src/common/config.ts)
  overrides `userData` for an executable-adjacent `legcord-data` directory.
- [Earlier Equibop Flatpak](https://github.com/flathub/io.github.equicord.equibop)
  and [ArmCord Flatpak](https://github.com/flathub/xyz.armcord.ArmCord) provide
  the published application IDs retained by discovery.

Path/scanner tests use synthetic profiles and credentials. Native Windows tests
exercise DPAPI-wrapped fixture keys plus AES-GCM across both storage layouts.
The three-platform CI matrix compiles each platform's credential implementation;
these checks do not establish live login compatibility for every client version.

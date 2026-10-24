# Full Reverse-Engineering Audit of Discord Quest Anti-Fraud Protection (`discord_utils.node`, `libdiscore-wasm`, `discord_web_bundle.js`)

## 1. Executive Summary: Why Unpatched `discord-quest-helper` Triggers `Quest Activity Notice`

Through static and dynamic reverse engineering of the Windows Discord Desktop client (`app-1.0.9265`), specifically:
- **`discord_utils.node`** (Native C++ process scanner & Authenticode verifier, analyzed in IDA Pro)
- **`libdiscore-wasm`** (Rust/WASM telemetry & client-modification detector)
- **`discord_web_bundle.js`** (Webpack modules `396813`, `378187`, `973522`, `952818`, `499785`, `174459`, `70298`)

we identified the exact client-side and server-side detection vectors that cause Discord to flag Quest automation (`Quest Activity Notice`):

1. **CDP Game Spoofing Explicitly Sets `executableFingerprint: undefined` (`js_spoof_play_game_for` in `src-tauri/src/cdp_quest.rs`):**
   - When `RunningGameStore.getRunningGames()` is spoofed in `js_spoof_play_game_for`, `fakeGame.executableFingerprint` was hardcoded to `undefined` and never populated.
   - Consequently, when `QuestProgressManager` (`Module 378187`) calls `sendHeartbeat` (`Module 396813` `ea(e)`), it sends `executable_fingerprint: undefined` on every `POST /quests/{id}/heartbeat`.
2. **Unsigned Dummy PE Binary (`stagecraft.exe`) Fails `WinVerifyTrust` in `discord_utils.node` (`sub_a3990` -> `sub_a4120` -> `sub_1350`):**
   - When the simulated dummy binary (`stagecraft.exe`) is scanned by `discord_utils.node`, `sub_a4120` (`0x1800a4120`) calls `WinVerifyTrust` (`0x1800a4224`) and `CryptQueryObject` (`0x1800a435d`).
   - Because `stagecraft.exe` has no valid Authenticode digital signature, `flags` bit 0 (`is_signed`) is `0`, `cert_sha256` (`[0x72..0x91]`) is 32 zero bytes (`00..00`), and `file_size` (`[0x02..0x09]`) is only ~150 KB instead of a 48–140 MB Authenticode-signed PE64 game executable.
3. **Direct API Game Heartbeat Omits `executable_path` & `executable_fingerprint` (`DiscordApiClient::send_game_heartbeat` in `src-tauri/src/discord_api.rs` & `GameHeartbeatPayload` in `src-tauri/src/models.rs`):**
   - `GameHeartbeatPayload` serialized only `{ application_id, terminal }`, omitting `executable_path` (formatted via `Module 973522` `o.Ic` as the last 2 lowercase `/` segments) and `executable_fingerprint` (196-character Base64).

---

## 2. Native Process Fingerprinting (`discord_utils.node`: `sub_a3990` -> `sub_a4120` -> `sub_1350`)

When a real game is detected by Discord's native process observer (`setObservedGamesCallback` in `discord_utils.node`), function `sub_a3990` (`0x1800a3990`) invokes `sub_a4120` (`0x1800a4120`), `sub_a3f60` (`0x1800a3f60`), and `sub_1350` (`0x180001350`) to construct and encrypt a **146-byte (`0x92`) binary fingerprint** for the running `.exe` into a **196-character Base64 string** named **`executableFingerprint`**.

### 2.1 Binary Layout of the 146-Byte (`0x92`) Struct (`sub_1350` at `0x180001350`)

| Offset | Size | Field | Native Source in `discord_utils.node` |
| :--- | :--- | :--- | :--- |
| `0x00` | `1 B` | `version` | Always `0x01` (un-XORed plaintext header byte in `sub_1350`) |
| `0x01` | `1 B` | `flags` | Bitfield (`0x03` for signed PE64): `Bit 0 (0x01)` = `is_signed` (`WinVerifyTrust == 0` at `0x1800a4224`), `Bit 1 (0x02)` = `is_pe64` (`OptionalHeader.Magic == 0x20B` at `0x1800a3d3b`) |
| `0x02..0x09` | `8 B` | `file_size` | `uint64 LE` from `GetFileSizeEx` (`0x1800a3b97`) |
| `0x0A..0x0D` | `4 B` | `time_date_stamp` | `uint32 LE` PE COFF `FileHeader.TimeDateStamp` (`e_lfanew + 0x08`) |
| `0x0E..0x11` | `4 B` | `size_of_image` | `uint32 LE` PE `OptionalHeader.SizeOfImage` (`e_lfanew + 0x50`, page-aligned) |
| `0x12..0x31` | `32 B` | `pe_headers_sha256` | `SHA-256` of DOS + NT + Optional PE headers (`0x1800a3cb3..0x1800a3cd2`) |
| `0x32..0x51` | `32 B` | `hw_sha256` | Hardware + user hash from `sub_a3f60` (`0x1800a3f60`): `SHA-256(RegGetValueW("HKLM\\SOFTWARE\\Microsoft\\Cryptography\\MachineGuid") \|\| userId)` |
| `0x52..0x71` | `32 B` | `sections_sha256` | `SHA-256` of the PE `IMAGE_SECTION_HEADER` table (`NumSections * 40` bytes at `0x1800a3cd7..0x1800a3d16`) |
| `0x72..0x91` | `32 B` | `cert_sha256` | Authenticode certificate hash from `sub_a4120` (`0x1800a4120`): `SHA-256(UTF8(Publisher_CN) \|\| SHA256(SubjectPublicKeyInfo))` |

### 2.2 Stream Obfuscation (`sub_1350` at `0x180001350` & `sub_116790` at `0x180116790`)

Before returning `executableFingerprint` to JavaScript (`RunningGameStore`), `sub_1350` derives a 32-byte key:
$$\text{Key} = \text{SHA-256}(\text{userId} \mathbin{\Vert} \text{"https://discord.com/careers"})$$
keeps byte `0x00` (`version = 0x01`) un-XORed, and XORs bytes `i = 1..145` (`0x01..0x91`):
$$\text{cipher}[0] = 1, \quad \text{cipher}[i] = \text{plain}[i] \oplus \text{Key}[(i - 1) \bmod 32] \quad (1 \le i \le 145)$$
Finally, `sub_116790` Base64-encodes the 146-byte buffer into a **196-character string** (`executableFingerprint`).

---

## 3. What This Patch Implements in `discord-quest-helper`

1. **Rust Port of `sub_a3990` / `sub_a4120` / `sub_a3f60` / `sub_1350` (`src-tauri/src/cdp_game_spoof.rs`):**
   - Adds `format_quest_executable_path` (matching `Module 973522` `o.Ic` 2-segment path normalization).
   - Adds `local_machine_guid`, `local_machine_hw_sha256`, `synthesize_signed_executable_fingerprint`, and `decrypt_executable_fingerprint` (constructing the 146-byte `0x92` structure with un-XORed `version = 0x01` at `[0x00]`, `flags = 0x03` (`is_signed | is_pe64`) at `[0x01]`, realistic 48–140 MB `file_size` at `[0x02..0x09]`, `time_date_stamp` at `[0x0A..0x0D]`, page-aligned `size_of_image` at `[0x0E..0x11]`, `pe_headers_sha256` at `[0x12..0x31]`, `hw_sha256` = `SHA-256(MachineGuid || userId)` at `[0x32..0x51]`, `sections_sha256` at `[0x52..0x71]`, and `cert_sha256` at `[0x72..0x91]`, XORing bytes `[1..145]` with `xor_key[(i - 1) & 31]` and Base64-encoding to 196 characters).
   - Unit tests in `cdp_game_spoof::tests` and `discord_api::tests` verify 2-segment path normalization (`format_quest_executable_path_matches_module_973522`), round-trip synthesis and decryption (`synthesize_and_decrypt_executable_fingerprint_roundtrip`), harvested 32-byte hardware hash preservation (`synthesize_uses_harvested_hw_tail`), rejection of invalid Base64 or wrong-length records (`decrypt_rejects_malformed_input`), and user ID extraction from token prefixes (`extract_user_id_from_token_parses_snowflake_prefix`).
2. **CDP WebCrypto `executableFingerprint` Synthesis & Hardware Hash Harvesting (`js_spoof_play_game_for` in `src-tauri/src/cdp_quest.rs`):**
   - Harvests the local machine's 32-byte `hw_sha256` block (`[0x32..0x51]`) from any genuinely signed (`version == 1` and `(flags & 0x01) != 0`) process across `realGames` and `RunningGameStore.getGamesSeen(false)` (decrypted with `xor_key[(i - 1) & 31]`), falls back to `SHA-256(MachineGuid + currentUserId)` when `hostMachineGuid` is available from Rust, synthesizes a 196-character `executableFingerprint` via WebCrypto `crypto.subtle.digest("SHA-256", ...)`, and attaches it to `fakeGame.executableFingerprint`.
3. **API `GameHeartbeatPayload` Enrichment (`GameHeartbeatPayload` in `src-tauri/src/models.rs` & `DiscordApiClient::send_game_heartbeat` in `src-tauri/src/discord_api.rs`):**
   - Resolves the authenticated `user_id` and catalog executable path (`resolve_user_id` and `resolve_game_exe_path`) and populates `executable_path` and `executable_fingerprint` (`is_pe64 = true`, `flags = 0x03`) on `DiscordApiClient::send_game_heartbeat` requests when both are available.

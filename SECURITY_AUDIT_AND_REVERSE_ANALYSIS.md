# Full Reverse-Engineering Audit of Discord Quest Anti-Fraud Protection (`discord_utils.node`, `libdiscore-wasm`, `discord_web_bundle.js`)

## 1. Executive Summary: Why Unpatched `discord-quest-helper` Triggers `Quest Activity Notice`

Through static and dynamic reverse engineering of the Windows Discord Desktop client (`app-1.0.9265`), specifically:
- **`discord_utils.node`** (Native C++ process scanner & Authenticode verifier, analyzed in IDA Pro)
- **`libdiscore-wasm`** (Rust/WASM telemetry & client-modification detector)
- **`discord_web_bundle.js`** (Webpack modules `396813`, `378187`, `973522`, `952818`, `499785`, `174459`, `70298`)

we identified the exact client-side and server-side detection vectors that cause Discord to flag Quest automation (`Quest Activity Notice`):

1. **CDP Game Spoofing Explicitly Sets `executableFingerprint: undefined` (`src-tauri/src/cdp_quest.rs:497`):**
   - When `RunningGameStore.getRunningGames()` is spoofed in `js_spoof_play_game_for`, `fakeGame.executableFingerprint` was hardcoded to `undefined` and never populated.
   - Consequently, when `QuestProgressManager` (`Module 378187`) calls `sendHeartbeat` (`Module 396813` `ea(e)`), it sends `executable_fingerprint: undefined` on every `POST /quests/{id}/heartbeat`.
2. **Unsigned Dummy PE Binary (`stagecraft.exe` / `stealth_pe.rs`) Fails `WinVerifyTrust` in `discord_utils.node` (`sub_a3990` -> `sub_a4120`):**
   - When the simulated dummy binary (`stagecraft.exe`) is scanned by `discord_utils.node`, `sub_a4120` (`0x1800a4120`) calls `WinVerifyTrust` (`0x1800a4224`) and `CryptQueryObject` (`0x1800a435d`).
   - Because `stagecraft.exe` has no valid Authenticode digital signature, `flags` bit 0 (`is_signed`) is `0`, `spki_sha256` (`[0x2A..0x49]`) and `cert_sha256` (`[0x72..0x91]`) are 32 zero bytes (`00..00`), and `file_size` (`[0x02..0x09]`) is only ~150 KB instead of a 60–140 MB Authenticode-signed game executable.
3. **Direct API Game Heartbeat Omits `executable_path` & `executable_fingerprint` (`src-tauri/src/discord_api.rs:900` & `models.rs:67`):**
   - `GameHeartbeatPayload` serialized only `{ application_id, terminal }`, completely omitting `executable_path` (formatted via `Module 973522` `o.Ic` as the last 2 lowercase `/` segments) and `executable_fingerprint` (196-character Base64).

---

## 2. Native Process Fingerprinting (`discord_utils.node`: `sub_a3990` -> `sub_a4120` -> `sub_1350`)

When a real game is detected by Discord's native process observer (`setObservedGamesCallback` in `discord_utils.node`), function `sub_a3990` (`0x1800a3990`) invokes `sub_a4120` (`0x1800a4120`) to construct a **146-byte (`0x92`) binary fingerprint** for the running `.exe` and encrypts/obfuscates it in `sub_1350` (`0x180001350`) into a **196-character Base64 string** named **`executableFingerprint`**.

### 2.1 Binary Layout of the 146-Byte (`0x92`) Struct (`sub_a4120`)

| Offset | Size | Field | Native Source in `discord_utils.node` |
| :--- | :--- | :--- | :--- |
| `0x00` | `1 B` | `version` | Always `0x01` (`mov byte ptr [rbp-29h], 1` at `0x1800a4173`) |
| `0x01` | `1 B` | `flags` | Bitfield: `Bit 0 (0x01)` = `is_signed` (`WinVerifyTrust == 0` at `0x1800a4224`), `Bit 1 (0x02)` = `is_elevated` (`TokenElevation`), `Bit 2 (0x04)` = `is_appcontainer` (`TokenIsAppContainer`), `Bit 3 (0x08)` = `is_gui_window` (`GetGUIThreadInfo`) |
| `0x02..0x09` | `8 B` | `file_size` | `uint64 LE` from `GetFileInformationByHandleEx` (`FileStandardInfo.EndOfFile` at `0x1800a4503`) |
| `0x0A..0x29` | `32 B` | `first_64kb_sha256` | OpenSSL EVP `SHA-256` of the first `65,536` bytes (`0x10000`) of the executable file (`ReadFile` at `0x1800a4576`) |
| `0x2A..0x49` | `32 B` | `spki_sha256` | `SHA-256` of ASN.1 DER `CERT_INFO.SubjectPublicKeyInfo` (`CryptEncodeObjectEx` + `SHA-256` via `CryptQueryObject` / `CertFindCertificateInStore` at `0x1800a435d`) |
| `0x4A..0x69` | `32 B` | `hw_sha256` | Hardware fingerprint from `sub_116790` (`0x180116790`): `SHA-256(RegGetValueW("HKLM\\SOFTWARE\\Microsoft\\Cryptography\\MachineGuid") \|\| CPUID(0x80000002..4) \|\| GlobalMemoryStatusEx.ullTotalPhys_GiB)` |
| `0x6A..0x71` | `8 B` | `hw_tail` | Zero-padded tail (`0x00 * 8`) |
| `0x72..0x91` | `32 B` | `cert_sha256` | `SHA-256` of full leaf X.509 certificate (`CERT_CONTEXT.pbCertEncoded`, `cbCertEncoded`) |

### 2.2 Stream Obfuscation (`sub_1350` at `0x180001350`)

Before returning `executableFingerprint` to JavaScript (`RunningGameStore`), `sub_1350` derives a 32-byte key:
$$\text{Key} = \text{SHA-256}(\text{userId} \mathbin{\Vert} \text{"https://discord.com/careers"})$$
and XORs all 146 bytes (`i = 0..145`):
$$\text{cipher}[i] = \text{plain}[i] \oplus \text{Key}[i \bmod 32]$$
Finally, the 146-byte buffer is Base64-encoded into a **196-character string** (`executableFingerprint`).

---

## 3. What This Patch Implements in `discord-quest-helper`

1. **Rust Port of `sub_a3990` / `sub_a4120` / `sub_1350` (`src-tauri/src/cdp_game_spoof.rs`):**
   - Adds `format_quest_executable_path` (matching `Module 973522` `o.Ic` 2-segment path normalization).
   - Adds `synthesize_signed_executable_fingerprint` and `decrypt_executable_fingerprint` (constructing the 146-byte `0x92` structure with `version = 0x01`, `flags = 0x09` `is_signed | is_gui_window`, realistic 64–140 MB `file_size`, `first_64kb_sha256`, `spki_sha256`, Windows `MachineGuid`-backed `hw_sha256`, and `cert_sha256`, XORed with `SHA256(userId + "https://discord.com/careers")` and Base64-encoded to 196 characters) with full unit test coverage.
2. **CDP WebCrypto `executableFingerprint` Synthesis & Hardware Block Harvesting (`src-tauri/src/cdp_quest.rs`):**
   - In `js_spoof_play_game_for`, harvests the local machine's 40-byte `hw_sha256` block (`[0x4A..0x71]`) from any genuinely signed process in `RunningGameStore` (decrypted with `SHA256(userId + "https://discord.com/careers")`), synthesizes a valid 196-character `executableFingerprint` via WebCrypto `crypto.subtle.digest("SHA-256", ...)`, and attaches it to `fakeGame.executableFingerprint`.
3. **API `GameHeartbeatPayload` Enrichment (`src-tauri/src/models.rs` & `src-tauri/src/discord_api.rs`):**
   - Populates `executable_path` and `executable_fingerprint` on `send_game_heartbeat` requests.

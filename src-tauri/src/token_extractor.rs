use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use regex::Regex;
use std::collections::HashSet;
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use std::path::PathBuf;

// Windows-specific imports
#[cfg(target_os = "windows")]
use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

// macOS-specific imports
#[cfg(target_os = "macos")]
use security_framework::passwords::get_generic_password;
/// Discord client type
#[derive(Debug)]
enum DiscordClient {
    Stable,
    Canary,
    Ptb,
}

impl DiscordClient {
    #[cfg(target_os = "windows")]
    fn path(&self) -> &str {
        match self {
            DiscordClient::Stable => "discord",
            DiscordClient::Canary => "discordcanary",
            DiscordClient::Ptb => "discordptb",
        }
    }

    #[cfg(target_os = "macos")]
    fn path(&self) -> &str {
        match self {
            DiscordClient::Stable => "discord",
            DiscordClient::Canary => "discordcanary",
            DiscordClient::Ptb => "discordptb",
        }
    }

    #[cfg(target_os = "macos")]
    fn safe_storage_name(&self) -> &str {
        match self {
            // Note: Discord uses lowercase in Keychain
            DiscordClient::Stable => "discord Safe Storage",
            DiscordClient::Canary => "discordcanary Safe Storage",
            DiscordClient::Ptb => "discordptb Safe Storage",
        }
    }

    #[cfg(target_os = "macos")]
    fn keychain_account(&self) -> &str {
        match self {
            DiscordClient::Stable => "discord Key",
            DiscordClient::Canary => "discordcanary Key",
            DiscordClient::Ptb => "discordptb Key",
        }
    }

    #[cfg(target_os = "linux")]
    fn path(&self) -> &str {
        match self {
            DiscordClient::Stable => "discord",
            DiscordClient::Canary => "discordcanary",
            DiscordClient::Ptb => "discordptb",
        }
    }

    #[cfg(target_os = "linux")]
    fn linux_secret_application_names(&self) -> &'static [&'static str] {
        match self {
            DiscordClient::Stable => &["discord"],
            DiscordClient::Canary => &["discordcanary"],
            DiscordClient::Ptb => &["discordptb"],
        }
    }
}

/// Auto-detect and extract Discord tokens (returns all unique tokens found)
pub fn extract_tokens() -> Result<Vec<String>> {
    use crate::logger::{log, sanitize_path, LogCategory, LogLevel};

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        "Starting token extraction",
        None,
    );
    let mut tokens = HashSet::new();
    let clients = vec![
        DiscordClient::Stable,
        DiscordClient::Canary,
        DiscordClient::Ptb,
    ];

    for client in clients {
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!("Checking Discord client: {:?}", client),
            None,
        );
        match try_extract_from_client(&client) {
            Ok(client_tokens) => {
                log(
                    LogLevel::Debug,
                    LogCategory::TokenExtraction,
                    &format!("Found {} tokens in {:?}", client_tokens.len(), client),
                    None,
                );
                for token in client_tokens {
                    tokens.insert(token);
                }
            }
            Err(e) => {
                // Sanitize error details to prevent path leakage
                let sanitized_error = sanitize_path(&e.to_string());
                log(
                    LogLevel::Debug,
                    LogCategory::TokenExtraction,
                    &format!("No tokens from {:?}", client),
                    Some(&sanitized_error),
                );
            }
        }
    }

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!("Total unique tokens found: {}", tokens.len()),
        None,
    );

    if tokens.is_empty() {
        log(
            LogLevel::Warn,
            LogCategory::TokenExtraction,
            "No tokens found in any Discord client",
            None,
        );
        anyhow::bail!("Could not find tokens in any Discord client")
    }

    Ok(sorted_tokens(tokens))
}

fn sorted_tokens(tokens: HashSet<String>) -> Vec<String> {
    let mut tokens: Vec<String> = tokens.into_iter().collect();
    tokens.sort();
    tokens
}

#[cfg(target_os = "windows")]
fn try_extract_from_client(client: &DiscordClient) -> Result<Vec<String>> {
    use crate::logger::{log, sanitize_path, LogCategory, LogLevel};

    // Get APPDATA path
    let appdata = std::env::var("APPDATA").context("Could not get APPDATA environment variable")?;

    // Build Discord path
    let discord_path = PathBuf::from(appdata).join(client.path());

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!(
            "Checking Discord path: {}",
            sanitize_path(&discord_path.to_string_lossy())
        ),
        None,
    );

    // Read Local State file to get encryption key
    let local_state_path = discord_path.join("Local State");
    let local_state_content =
        fs::read_to_string(&local_state_path).context("Could not read Local State file")?;

    // Parse JSON to get encryption key
    let local_state: serde_json::Value =
        serde_json::from_str(&local_state_content).context("Could not parse Local State JSON")?;

    let encrypted_key = local_state["os_crypt"]["encrypted_key"]
        .as_str()
        .context("Could not find encrypted_key")?;

    // Base64 decode
    let encrypted_key_bytes = BASE64
        .decode(encrypted_key)
        .context("Could not decode encrypted_key")?;

    // Remove "DPAPI" prefix (first 5 bytes)
    let encrypted_key_bytes = &encrypted_key_bytes[5..];

    // Use Windows DPAPI to decrypt master key
    let master_key = decrypt_with_dpapi(encrypted_key_bytes)?;

    // Search for tokens in LevelDB
    let leveldb_path = discord_path.join("Local Storage").join("leveldb");

    if !leveldb_path.exists() {
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!(
                "LevelDB path does not exist: {}",
                sanitize_path(&leveldb_path.to_string_lossy())
            ),
            None,
        );
        anyhow::bail!("LevelDB path does not exist");
    }

    let mut tokens = Vec::new();
    let mut file_count = 0;

    // Read all .ldb and .log files
    for entry in fs::read_dir(&leveldb_path)? {
        let entry = entry?;
        let path = entry.path();

        if let Some(ext) = path.extension() {
            if ext == "ldb" || ext == "log" {
                file_count += 1;
                if let Ok(content) = fs::read(&path) {
                    // Search for all token patterns
                    let found_tokens = find_and_decrypt_tokens(&content, &master_key);
                    tokens.extend(found_tokens);
                }
            }
        }
    }

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!(
            "Searched {} LevelDB files, found {} tokens",
            file_count,
            tokens.len()
        ),
        None,
    );

    Ok(tokens)
}

#[cfg(target_os = "windows")]
fn decrypt_with_dpapi(data: &[u8]) -> Result<Vec<u8>> {
    use std::ptr;

    unsafe {
        let input_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };

        let mut output_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

        let result = CryptUnprotectData(&input_blob, None, None, None, None, 0, &mut output_blob);

        if result.is_err() {
            anyhow::bail!("DPAPI decryption failed");
        }

        // Copy decrypted data
        let decrypted =
            std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec();

        Ok(decrypted)
    }
}

#[cfg(target_os = "macos")]
fn try_extract_from_client(client: &DiscordClient) -> Result<Vec<String>> {
    // Get Application Support path
    let home = std::env::var("HOME").context("Could not get HOME environment variable")?;

    // Build Discord path
    let discord_path = PathBuf::from(&home)
        .join("Library/Application Support")
        .join(client.path());

    if !discord_path.exists() {
        anyhow::bail!("Discord path does not exist: {:?}", discord_path);
    }

    println!(
        "Checking Discord path: {}",
        crate::logger::sanitize_path(&discord_path.to_string_lossy())
    );

    // Get the master key from macOS Keychain
    let master_key = get_master_key_from_keychain(client)?;

    // Search for tokens in LevelDB
    let leveldb_path = discord_path.join("Local Storage").join("leveldb");

    if !leveldb_path.exists() {
        anyhow::bail!("LevelDB path does not exist");
    }

    let mut tokens = Vec::new();

    // Read all .ldb and .log files
    for entry in fs::read_dir(&leveldb_path)? {
        let entry = entry?;
        let path = entry.path();

        if let Some(ext) = path.extension() {
            if ext == "ldb" || ext == "log" {
                if let Ok(content) = fs::read(&path) {
                    // Search for all token patterns
                    let found_tokens = find_and_decrypt_tokens(&content, &master_key);
                    tokens.extend(found_tokens);
                }
            }
        }
    }

    Ok(tokens)
}

#[cfg(target_os = "macos")]
fn get_master_key_from_keychain(client: &DiscordClient) -> Result<Vec<u8>> {
    use pbkdf2::pbkdf2_hmac;
    use sha1::Sha1;
    use std::process::Command;

    let service_name = client.safe_storage_name();
    let account_name = client.keychain_account();

    println!(
        "Looking for Keychain item: service='{}', account='{}'",
        service_name, account_name
    );

    let raw_password: Vec<u8>;

    // First try using the security-framework crate
    match get_generic_password(service_name, account_name) {
        Ok(password) => {
            println!(
                "Got password from Keychain using security-framework ({} bytes)",
                password.len()
            );
            raw_password = password.to_vec();
        }
        Err(e) => {
            println!(
                "security-framework failed: {:?}, trying security command",
                e
            );

            // Fallback: Use the `security` command line tool
            let output = Command::new("security")
                .args([
                    "find-generic-password",
                    "-s",
                    service_name,
                    "-a",
                    account_name,
                    "-w",
                ])
                .output()
                .context("Failed to execute security command")?;

            if output.status.success() {
                let password_str = String::from_utf8_lossy(&output.stdout);
                let password = password_str.trim();
                println!(
                    "Got password from Keychain using security CLI ({} bytes)",
                    password.len()
                );
                raw_password = password.as_bytes().to_vec();
            } else {
                anyhow::bail!(
                    "Could not get Discord Safe Storage key from Keychain.\n\
                    Make sure Discord is installed and you've logged in at least once.\n\
                    You may need to grant Keychain access when prompted."
                );
            }
        }
    }

    // Chromium on macOS uses PBKDF2-HMAC-SHA1 to derive the AES key
    // Salt: "saltysalt" (literal string)
    // Iterations: 1003
    // Key length: 16 bytes (128 bits) for AES-128, then padded to 32 bytes
    // Reference: https://source.chromium.org/chromium/chromium/src/+/main:components/os_crypt/sync/os_crypt_mac.mm

    let salt = b"saltysalt";
    let iterations: u32 = 1003;
    let mut derived_key = [0u8; 16]; // Chromium uses 128-bit key

    pbkdf2_hmac::<Sha1>(&raw_password, salt, iterations, &mut derived_key);

    println!("Derived key using PBKDF2 (16 bytes)");

    // For AES-256-GCM we need 32 bytes, but Chromium on macOS uses AES-128-CBC
    // Let's try with the 16-byte key first by padding it
    // Actually, if Discord uses v10 prefix, it's AES-256-GCM which needs 32 bytes
    // We need to extend the key or use a different approach

    // According to Chromium source, macOS uses AES-128-CBC, not AES-256-GCM
    // But the encrypted token format is v10 + nonce + ciphertext (GCM format)
    // Let's pad the key to 32 bytes for AES-256
    let mut full_key = [0u8; 32];
    full_key[..16].copy_from_slice(&derived_key);
    // The second half can stay as zeros, or we can derive more bytes

    Ok(full_key.to_vec())
}

#[cfg(target_os = "linux")]
fn try_extract_from_client(client: &DiscordClient) -> Result<Vec<String>> {
    use crate::logger::{log, sanitize_path, LogCategory, LogLevel};

    let profile_dirs = linux_profile_dirs(client)?;
    let mut tokens = HashSet::new();
    let mut oscrypt_keys = LinuxOscryptKeyCache::default();
    let mut checked_any_profile = false;
    let mut last_error = None;

    for discord_path in profile_dirs {
        if !discord_path.exists() {
            continue;
        }

        checked_any_profile = true;
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!(
                "Checking Linux Discord path: {}",
                sanitize_path(&discord_path.to_string_lossy())
            ),
            None,
        );

        match extract_from_linux_profile(client, &discord_path, &mut oscrypt_keys) {
            Ok(profile_tokens) => {
                for token in profile_tokens {
                    tokens.insert(token);
                }
            }
            Err(e) => {
                last_error = Some(e.to_string());
                log(
                    LogLevel::Debug,
                    LogCategory::TokenExtraction,
                    "No tokens from Linux Discord profile",
                    Some(&sanitize_path(&e.to_string())),
                );
            }
        }
    }

    if !checked_any_profile {
        anyhow::bail!("Discord profile path does not exist");
    }

    if tokens.is_empty() {
        if let Some(error) = last_error {
            anyhow::bail!("No tokens found in Linux Discord profile: {}", error);
        }
        anyhow::bail!("No tokens found in Linux Discord profile");
    }

    Ok(sorted_tokens(tokens))
}

#[cfg(target_os = "linux")]
fn linux_profile_dirs(client: &DiscordClient) -> Result<Vec<PathBuf>> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .context("Could not get HOME environment variable")?;
    let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let config_root = linux_config_root(&home, xdg_config_home.as_deref());
    let profile_name = client.path();
    let mut paths = vec![config_root.join(profile_name)];

    if matches!(client, DiscordClient::Stable) {
        paths.push(
            home.join("snap")
                .join("discord")
                .join("current")
                .join(".config")
                .join("discord"),
        );
        paths.push(
            home.join(".var")
                .join("app")
                .join("com.discordapp.Discord")
                .join("config")
                .join("discord"),
        );
    }

    Ok(paths)
}

#[cfg(target_os = "linux")]
fn linux_config_root(home: &Path, xdg_config_home: Option<&Path>) -> PathBuf {
    xdg_config_home
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".config"))
}

#[cfg(target_os = "linux")]
fn extract_from_linux_profile(
    client: &DiscordClient,
    discord_path: &Path,
    oscrypt_keys: &mut LinuxOscryptKeyCache,
) -> Result<Vec<String>> {
    use crate::logger::{log, LogCategory, LogLevel};

    let leveldb_path = discord_path.join("Local Storage").join("leveldb");

    if !leveldb_path.exists() {
        anyhow::bail!("LevelDB path does not exist");
    }

    let mut tokens = HashSet::new();
    let mut encrypted_payloads = Vec::new();
    let mut file_count = 0;

    for entry in fs::read_dir(&leveldb_path)? {
        let entry = entry?;
        let path = entry.path();

        if is_leveldb_log_or_table(&path) {
            file_count += 1;
            if let Ok(content) = fs::read(&path) {
                for token in find_plaintext_tokens(&content) {
                    tokens.insert(token);
                }
                encrypted_payloads.extend(find_encrypted_token_payloads(&content));
            }
        }
    }

    if !encrypted_payloads.is_empty() {
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!(
                "Found {} encrypted Linux token payloads to try",
                encrypted_payloads.len()
            ),
            None,
        );

        for payload in encrypted_payloads {
            if let Ok(token) = decrypt_linux_oscrypt_value(&payload, client, oscrypt_keys) {
                tokens.insert(token);
            }
        }
    }

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!(
            "Searched {} Linux LevelDB files, found {} candidate tokens",
            file_count,
            tokens.len()
        ),
        None,
    );

    Ok(sorted_tokens(tokens))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn try_extract_from_client(_client: &DiscordClient) -> Result<Vec<String>> {
    anyhow::bail!("Token extraction is only supported on Windows, macOS, and Linux")
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct LinuxOscryptKeyCache {
    v10: Option<zeroize::Zeroizing<[u8; 16]>>,
    v11: Option<zeroize::Zeroizing<[u8; 16]>>,
}

#[cfg(target_os = "linux")]
impl LinuxOscryptKeyCache {
    fn key_for(&mut self, version: &[u8], client: &DiscordClient) -> Result<&[u8]> {
        match version {
            b"v10" => {
                // Chromium's legacy Linux v10 format deliberately uses this public,
                // fixed compatibility value. It is only used to decrypt existing local
                // OSCrypt data; it is not an application credential or secret.
                let key = self
                    .v10
                    .get_or_insert_with(|| derive_linux_oscrypt_key(b"peanuts"));
                Ok(&key[..])
            }
            b"v11" => {
                if self.v11.is_none() {
                    let password = zeroize::Zeroizing::new(
                        get_linux_safe_storage_password(client)
                            .context("Could not get Linux safe storage key")?,
                    );
                    self.v11 = Some(derive_linux_oscrypt_key(&password));
                }

                self.v11
                    .as_deref()
                    .map(|key| &key[..])
                    .context("Linux safe storage key cache was not initialized")
            }
            version => anyhow::bail!("Unknown Linux OSCrypt version: {:?}", version),
        }
    }
}

#[cfg(target_os = "linux")]
fn derive_linux_oscrypt_key(password: &[u8]) -> zeroize::Zeroizing<[u8; 16]> {
    use pbkdf2::pbkdf2_hmac;
    use sha1::Sha1;

    let mut key = zeroize::Zeroizing::new([0u8; 16]);
    pbkdf2_hmac::<Sha1>(password, b"saltysalt", 1, &mut *key);
    key
}

#[cfg(target_os = "linux")]
fn decrypt_linux_oscrypt_value(
    encrypted_data: &[u8],
    client: &DiscordClient,
    oscrypt_keys: &mut LinuxOscryptKeyCache,
) -> Result<String> {
    use aes::cipher::{BlockDecryptMut, KeyIvInit};
    use cbc::Decryptor;
    use zeroize::Zeroizing;

    type Aes128CbcDec = Decryptor<aes::Aes128>;

    if encrypted_data.len() <= 3 {
        anyhow::bail!("Encrypted data is too short");
    }

    let key = oscrypt_keys.key_for(&encrypted_data[..3], client)?;

    let mut buf = Zeroizing::new(encrypted_data[3..].to_vec());
    let iv = b"                ";
    let cipher = Aes128CbcDec::new_from_slices(key, iv)
        .map_err(|e| anyhow::anyhow!("Failed to create Linux OSCrypt cipher: {:?}", e))?;
    let decrypted = cipher
        .decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buf)
        .map_err(|e| anyhow::anyhow!("Linux OSCrypt decryption failed: {:?}", e))?;

    String::from_utf8(decrypted.to_vec()).context("Decrypted data is not valid UTF-8")
}

#[cfg(target_os = "linux")]
fn get_linux_safe_storage_password(client: &DiscordClient) -> Result<Vec<u8>> {
    use secret_service::{blocking::SecretService, EncryptionType};
    use std::collections::HashMap;

    let ss = SecretService::connect(EncryptionType::Dh)
        .or_else(|_| SecretService::connect(EncryptionType::Plain))
        .context("Could not connect to Linux Secret Service")?;

    for application_name in client.linux_secret_application_names() {
        let mut attributes = HashMap::new();
        attributes.insert("application", *application_name);

        let search = ss
            .search_items(attributes)
            .with_context(|| format!("Could not search Secret Service for {}", application_name))?;

        for item in search.unlocked.iter().chain(search.locked.iter()) {
            if item.is_locked().unwrap_or(false) {
                item.unlock()
                    .context("Could not unlock Linux Secret Service item")?;
            }

            let item_attributes = item.get_attributes().unwrap_or_default();
            let label = item.get_label().unwrap_or_default();
            let looks_like_chromium_safe_storage = label == "Chromium Safe Storage"
                || item_attributes
                    .get("xdg:schema")
                    .map(|schema| schema == "chrome_libsecret_os_crypt_password_v2")
                    .unwrap_or(false);

            if !looks_like_chromium_safe_storage {
                continue;
            }

            let secret = item
                .get_secret()
                .context("Could not read Linux safe storage key")?;
            if !secret.is_empty() {
                return Ok(secret);
            }
        }
    }

    anyhow::bail!("Could not find Linux Discord safe storage key")
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn find_and_decrypt_tokens(data: &[u8], master_key: &[u8]) -> Vec<String> {
    let mut tokens = Vec::new();

    for encrypted_bytes in find_encrypted_token_payloads(data) {
        if let Ok(token) = decrypt_token(&encrypted_bytes, master_key) {
            tokens.push(token);
        }
    }

    tokens
}

fn find_encrypted_token_payloads(data: &[u8]) -> Vec<Vec<u8>> {
    let mut payloads = Vec::new();
    let content = String::from_utf8_lossy(data);

    // Pattern: dQw4w9WgXcQ:([Base64])
    let re = match Regex::new(r"dQw4w9WgXcQ:([A-Za-z0-9+/=]+)") {
        Ok(re) => re,
        Err(_) => return payloads,
    };

    for cap in re.captures_iter(&content) {
        if let Some(encrypted_token) = cap.get(1) {
            if let Ok(encrypted_bytes) = BASE64.decode(encrypted_token.as_str()) {
                payloads.push(encrypted_bytes);
            }
        }
    }

    payloads
}

#[cfg(target_os = "linux")]
fn find_plaintext_tokens(data: &[u8]) -> Vec<String> {
    let content = String::from_utf8_lossy(data);
    let re = match Regex::new(
        r"(?:mfa\.[A-Za-z0-9_-]{70,}|[A-Za-z0-9_-]{20,30}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{25,120})",
    ) {
        Ok(re) => re,
        Err(_) => return Vec::new(),
    };

    re.find_iter(&content)
        .map(|matched| matched.as_str().to_string())
        .collect()
}

#[cfg(target_os = "linux")]
fn is_leveldb_log_or_table(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("ldb" | "log")
    )
}

/// Decrypt token - uses different methods for Windows and macOS
#[cfg(target_os = "windows")]
fn decrypt_token(encrypted_data: &[u8], key: &[u8]) -> Result<String> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    // AES-256-GCM decryption (Windows)
    // First 3 bytes are version identifier "v10"
    if encrypted_data.len() < 15 {
        anyhow::bail!("Encrypted data is too short");
    }

    // Skip version identifier
    let encrypted_data = &encrypted_data[3..];

    // First 12 bytes are nonce/iv
    let nonce_bytes = &encrypted_data[..12];
    let ciphertext = &encrypted_data[12..];

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| anyhow::anyhow!("Could not create AES cipher"))?;

    let nonce = Nonce::from_slice(nonce_bytes);

    // Decrypt
    let decrypted = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("AES decryption failed"))?;

    // Convert to string
    String::from_utf8(decrypted).context("Decrypted data is not valid UTF-8")
}

/// Decrypt token - macOS uses AES-128-CBC
#[cfg(target_os = "macos")]
fn decrypt_token(encrypted_data: &[u8], key: &[u8]) -> Result<String> {
    use aes::cipher::{BlockDecryptMut, KeyIvInit};
    use cbc::Decryptor;

    type Aes128CbcDec = Decryptor<aes::Aes128>;

    // macOS Chromium format: "v10" + iv(16 bytes) + ciphertext
    // But we need to check the actual format
    if encrypted_data.len() < 3 {
        anyhow::bail!("Encrypted data is too short");
    }

    // Check version prefix
    let version = &encrypted_data[0..3];

    if version == b"v10" || version == b"v11" {
        // Chromium encrypted format
        let data = &encrypted_data[3..];

        if data.len() < 16 {
            anyhow::bail!("Encrypted data too short for IV");
        }

        // For macOS, Chromium uses a fixed IV of 16 spaces
        let iv = b"                "; // 16 spaces
        let ciphertext = data;

        // Use only the first 16 bytes of the key for AES-128
        if key.len() < 16 {
            anyhow::bail!("Key too short");
        }
        let key_128 = &key[..16];

        // Decrypt
        let mut buf = ciphertext.to_vec();
        let cipher = Aes128CbcDec::new_from_slices(key_128, iv)
            .map_err(|e| anyhow::anyhow!("Failed to create cipher: {:?}", e))?;

        let decrypted = cipher
            .decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buf)
            .map_err(|e| anyhow::anyhow!("Decryption failed: {:?}", e))?;

        String::from_utf8(decrypted.to_vec()).context("Decrypted data is not valid UTF-8")
    } else {
        // Might be unencrypted or different format
        anyhow::bail!("Unknown encryption version: {:?}", version)
    }
}

// `find_and_decrypt_tokens` (the only caller of `decrypt_token`) is compiled
// only on Windows/macOS. Linux uses its own LevelDB/OSCrypt path above.

/// Get the latest client_build_number from Discord JavaScript files
///
/// This function will:
/// 1. Request the Discord login page
/// 2. Parse HTML to find JavaScript resource files
/// 3. Request JS files and search for buildNumber
///
/// Reference: https://docs.discord.food/reference#client-properties-structure
pub async fn fetch_build_number_from_discord() -> Result<u64> {
    use crate::logger::{log, LogCategory, LogLevel};

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        "Fetching build number from Discord",
        None,
    );

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .connect_timeout(std::time::Duration::from_secs(8))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Failed to create HTTP client")?;

    // Request Discord login page
    let login_page = client
        .get("https://discord.com/login")
        .send()
        .await
        .context("Failed to fetch Discord login page")?
        .text()
        .await
        .context("Failed to read login page response")?;

    // Parse script tag to find JS file
    // Discord JS filename format: /assets/[name].[hash].js (e.g. web.ce2887aa135af11c.js)
    let script_re = Regex::new(r#"/assets/[a-z0-9-]+\.[a-f0-9]+\.js"#)?;
    let script_urls: Vec<String> = script_re
        .find_iter(&login_page)
        .map(|m| format!("https://discord.com{}", m.as_str()))
        .collect();

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!("Found {} JavaScript files", script_urls.len()),
        None,
    );

    if script_urls.is_empty() {
        // Try alternative URL patterns (handling potential quoted cases)
        let alt_script_re = Regex::new(r#"src="(/assets/[a-z0-9-]+\.[a-f0-9]+\.js)""#)?;
        let alt_urls: Vec<String> = alt_script_re
            .captures_iter(&login_page)
            .filter_map(|cap| {
                cap.get(1)
                    .map(|m| format!("https://discord.com{}", m.as_str()))
            })
            .collect();

        if alt_urls.is_empty() {
            anyhow::bail!("Could not find any JavaScript files in Discord login page");
        }

        return fetch_build_number_from_scripts(&client, &alt_urls).await;
    }

    fetch_build_number_from_scripts(&client, &script_urls).await
}

async fn fetch_build_number_from_scripts(
    client: &reqwest::Client,
    script_urls: &[String],
) -> Result<u64> {
    use crate::logger::{log, LogCategory, LogLevel};

    // Multiple possible buildNumber patterns
    let patterns = [
        r#"buildNumber["\s:]+(\d{5,})"#,
        r#"build_number["\s:]+(\d{5,})"#,
        r#"buildNumber:\s*"?(\d{5,})"?"#,
        r#""buildNumber"\s*:\s*(\d+)"#,
    ];

    // Check more bundles to handle Discord moving build metadata between assets
    let urls_to_check: Vec<&String> = script_urls.iter().rev().take(10).collect();

    for url in urls_to_check {
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!("Checking JS file: {}", url),
            None,
        );

        match client.get(url).send().await {
            Ok(response) => {
                if let Ok(js_content) = response.text().await {
                    // Try all patterns
                    for pattern in &patterns {
                        if let Ok(re) = Regex::new(pattern) {
                            if let Some(caps) = re.captures(&js_content) {
                                if let Some(num_match) = caps.get(1) {
                                    if let Ok(build_num) = num_match.as_str().parse::<u64>() {
                                        // BUILD NUMBER VALIDATION BOUNDS:
                                        // Lower bound (100000): Discord build numbers are typically 6+ digits
                                        // Upper bound (9999999): Allow for future growth to 7 digits
                                        // If Discord changes their numbering scheme significantly,
                                        // these bounds may need adjustment.
                                        if (100000..=9_999_999).contains(&build_num) {
                                            log(
                                                LogLevel::Info,
                                                LogCategory::TokenExtraction,
                                                &format!("Found build number: {}", build_num),
                                                None,
                                            );
                                            return Ok(build_num);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                log(
                    LogLevel::Debug,
                    LogCategory::TokenExtraction,
                    &format!("Failed to fetch JS file: {}", e),
                    None,
                );
            }
        }
    }

    anyhow::bail!("Could not find build number in any JavaScript file")
}

/// Discord Client Info (from update manifest)
#[derive(Debug, Clone)]
pub struct DiscordClientInfo {
    pub host_version: [u32; 3], // e.g., [1, 0, 9219]
    pub native_build_number: u64,
}

impl DiscordClientInfo {
    /// Get formatted client_version string (e.g. "1.0.9219")
    pub fn client_version(&self) -> String {
        format!(
            "{}.{}.{}",
            self.host_version[0], self.host_version[1], self.host_version[2]
        )
    }
}

/// Get client info from Discord Update API
///
/// API: https://updates.discord.com/distributions/app/manifests/latest
///
/// Reference: https://docs.discord.food/topics/client-distribution
pub async fn fetch_discord_client_info() -> Result<DiscordClientInfo> {
    use crate::logger::{log, LogCategory, LogLevel};

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        "Fetching Discord client info from update API",
        None,
    );

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .connect_timeout(std::time::Duration::from_secs(8))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Failed to create HTTP client")?;

    // Request Discord update manifest
    let url = "https://updates.discord.com/distributions/app/manifests/latest?channel=stable&platform=win&arch=x64";

    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to fetch Discord update manifest")?;

    if !response.status().is_success() {
        anyhow::bail!("Discord update API returned error: {}", response.status());
    }

    let manifest: serde_json::Value = response
        .json::<serde_json::Value>()
        .await
        .context("Failed to parse update manifest JSON")?;

    // Parse host_version array
    let host_version = manifest["host_version"]
        .as_array()
        .context("Missing host_version in manifest")?;

    if host_version.len() < 3 {
        anyhow::bail!("Invalid host_version format");
    }

    let version: [u32; 3] = [
        host_version[0].as_u64().unwrap_or(1) as u32,
        host_version[1].as_u64().unwrap_or(0) as u32,
        host_version[2].as_u64().unwrap_or_else(|| {
            crate::super_properties::DEFAULT_CLIENT_VERSION
                .rsplit('.')
                .next()
                .and_then(|patch| patch.parse::<u64>().ok())
                .unwrap_or(0)
        }) as u32,
    ];

    // native_build_number is usually the third number in host_version
    // but may also be a separate field in the manifest
    let native_build: u64 = manifest
        .get("native_module_version")
        .and_then(|v: &serde_json::Value| v.as_u64())
        .unwrap_or(version[2] as u64);

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "Got Discord client info: version={}.{}.{}, native_build={}",
            version[0], version[1], version[2], native_build
        ),
        None,
    );

    Ok(DiscordClientInfo {
        host_version: version,
        native_build_number: native_build,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn test_find_plaintext_linux_tokens() {
        let token = "abcdefghijklmnopqrstuvwx.abcdef.ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl";
        let data = format!("prefix\0{}\0suffix", token);

        let tokens = find_plaintext_tokens(data.as_bytes());

        assert_eq!(tokens, vec![token.to_string()]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_linux_config_root_respects_absolute_xdg_path() {
        let home = Path::new("/home/tester");

        assert_eq!(
            linux_config_root(home, Some(Path::new("/custom/config"))),
            PathBuf::from("/custom/config")
        );
        assert_eq!(
            linux_config_root(home, Some(Path::new("relative/config"))),
            PathBuf::from("/home/tester/.config")
        );
        assert_eq!(
            linux_config_root(home, None),
            PathBuf::from("/home/tester/.config")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_decrypt_linux_v10_basic_text_value() {
        use aes::cipher::{BlockEncryptMut, KeyIvInit};
        use cbc::Encryptor;
        use pbkdf2::pbkdf2_hmac;
        use sha1::Sha1;

        type Aes128CbcEnc = Encryptor<aes::Aes128>;

        let plaintext = b"dqht-linux-oscrypt-test-token";
        let mut key = [0u8; 16];
        pbkdf2_hmac::<Sha1>(b"peanuts", b"saltysalt", 1, &mut key);
        let ciphertext = Aes128CbcEnc::new_from_slices(&key, b"                ")
            .unwrap()
            .encrypt_padded_vec_mut::<aes::cipher::block_padding::Pkcs7>(plaintext);

        let mut encrypted = b"v10".to_vec();
        encrypted.extend(ciphertext);

        let decrypted = decrypt_linux_oscrypt_value(
            &encrypted,
            &DiscordClient::Stable,
            &mut LinuxOscryptKeyCache::default(),
        )
        .unwrap();

        assert_eq!(decrypted.as_bytes(), plaintext);
    }

    #[test]
    fn test_find_encrypted_token_payloads() {
        let payload = b"v10encrypted-placeholder";
        let encoded = BASE64.encode(payload);
        let data = format!("dQw4w9WgXcQ:{}", encoded);

        let payloads = find_encrypted_token_payloads(data.as_bytes());

        assert_eq!(payloads, vec![payload.to_vec()]);
    }

    #[test]
    #[ignore] // Only run when Discord is installed
    fn test_extract_tokens() {
        let result = extract_tokens();
        match result {
            Ok(tokens) => println!("Extracted {} tokens", tokens.len()),
            Err(e) => println!("Error: {}", e),
        }
    }
}

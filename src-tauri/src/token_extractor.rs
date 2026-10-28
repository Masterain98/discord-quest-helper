use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
#[cfg(target_os = "windows")]
use std::path::Path;

// Windows-specific imports
#[cfg(target_os = "windows")]
use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

// macOS-specific imports
#[cfg(target_os = "macos")]
use security_framework::passwords::get_generic_password;
mod leveldb;
mod profiles;
use profiles::ChromiumProfile;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use profiles::ClientSpec;

/// Find local session candidates. Validation against Discord remains the
/// responsibility of the existing native authentication command.
pub fn extract_tokens() -> Result<Vec<String>> {
    use crate::logger::{log, sanitize_path, LogCategory, LogLevel};

    let mut tokens = HashSet::new();
    let mut visited_profiles = HashSet::new();
    let mut last_error = None;
    for profile in profiles::discover_profiles()? {
        let canonical_root = match fs::canonicalize(&profile.root) {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                last_error = Some(format!(
                    "Could not access {} profile: {error}",
                    profile.client.display_name
                ));
                continue;
            }
        };
        if !visited_profiles.insert((profile.client.display_name, canonical_root)) {
            continue;
        }
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!("Checking {} profile", profile.client.display_name),
            Some(&sanitize_path(&profile.root.to_string_lossy())),
        );
        match extract_from_profile(&profile) {
            Ok(scan) => {
                log(
                    LogLevel::Debug,
                    LogCategory::TokenExtraction,
                    &format!(
                        "Read {} LevelDB files, {} encrypted candidates, {} tokens",
                        scan.files_read,
                        scan.encrypted_candidates,
                        scan.tokens.len()
                    ),
                    None,
                );
                tokens.extend(scan.tokens);
            }
            Err(error) => {
                let detail = sanitize_path(&format!("{error:#}"));
                log(
                    LogLevel::Debug,
                    LogCategory::TokenExtraction,
                    "Profile extraction failed",
                    Some(&detail),
                );
                last_error = Some(detail);
            }
        }
    }
    if tokens.is_empty() {
        if let Some(detail) = last_error {
            anyhow::bail!("Could not find tokens in any Discord client: {detail}");
        }
        anyhow::bail!("Could not find tokens in any Discord client");
    }
    Ok(sorted_tokens(tokens))
}

fn sorted_tokens(tokens: HashSet<String>) -> Vec<String> {
    let mut tokens: Vec<String> = tokens.into_iter().collect();
    tokens.sort();
    tokens
}

fn extract_from_profile(profile: &ChromiumProfile) -> Result<leveldb::ScanResult> {
    // Keys (including failed lookups) are cached for this profile only. A
    // Flatpak/native profile or another client must not inherit its key.
    #[cfg(target_os = "windows")]
    let mut keys = std::collections::HashMap::new();
    #[cfg(target_os = "macos")]
    let key = once_cell::unsync::OnceCell::new();
    #[cfg(target_os = "linux")]
    let mut keys = LinuxOscryptKeyCache::default();

    leveldb::scan_profile(&profile.root, |payload, _storage_root| {
        anyhow::ensure!(
            payload.len() >= 19 && (payload.starts_with(b"v10") || payload.starts_with(b"v11")),
            "Unsupported or truncated OSCrypt token format"
        );
        #[cfg(target_os = "windows")]
        {
            let session_state = _storage_root.join("Local State");
            let state_path = if session_state.is_file() {
                session_state
            } else {
                profile.root.join("Local State")
            };
            let key = keys.entry(state_path.clone()).or_insert_with(|| {
                load_windows_master_key(&state_path).map_err(|error| format!("{error:#}"))
            });
            let key = key.as_ref().map_err(|error| anyhow::anyhow!("{error}"))?;
            decrypt_token(payload, key)
        }
        #[cfg(target_os = "macos")]
        {
            let key = key.get_or_init(|| {
                get_master_key_from_keychain(profile.client).map_err(|error| format!("{error:#}"))
            });
            let key = key.as_ref().map_err(|error| anyhow::anyhow!("{error}"))?;
            decrypt_cbc_token(payload, &key[..])
        }
        #[cfg(target_os = "linux")]
        {
            decrypt_linux_oscrypt_value(payload, profile.client, &mut keys)
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        anyhow::bail!("Token extraction is not supported on this platform")
    })
}

#[cfg(target_os = "windows")]
fn load_windows_master_key(path: &Path) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    let content = zeroize::Zeroizing::new(
        fs::read_to_string(path).context("Could not read Local State file")?,
    );
    let state: serde_json::Value =
        serde_json::from_str(&content).context("Could not parse Local State JSON")?;
    let encrypted_key = state["os_crypt"]["encrypted_key"]
        .as_str()
        .context("Could not find encrypted_key")?;
    let encrypted_key = BASE64
        .decode(encrypted_key)
        .context("Could not decode encrypted_key")?;
    decrypt_with_dpapi(strip_dpapi_prefix(&encrypted_key)?).map(zeroize::Zeroizing::new)
}

#[cfg(target_os = "windows")]
fn strip_dpapi_prefix(data: &[u8]) -> Result<&[u8]> {
    data.strip_prefix(b"DPAPI")
        .context("Encrypted key is missing the DPAPI prefix")
}

#[cfg(target_os = "windows")]
fn decrypt_with_dpapi(data: &[u8]) -> Result<Vec<u8>> {
    use std::ptr;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};

    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len()).context("Encrypted key is too large")?,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };
        CryptUnprotectData(&input, None, None, None, None, 0, &mut output)
            .context("DPAPI decryption failed")?;
        anyhow::ensure!(!output.pbData.is_null(), "DPAPI returned no data");
        let decrypted = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        // CryptUnprotectData transfers ownership of a LocalAlloc buffer.
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(decrypted)
    }
}

#[cfg(target_os = "macos")]
fn get_master_key_from_keychain(client: &ClientSpec) -> Result<zeroize::Zeroizing<[u8; 16]>> {
    use pbkdf2::pbkdf2_hmac;
    use sha1::Sha1;

    for name in client.keychain_names {
        if let Ok(password) = read_keychain_password(name) {
            let mut key = zeroize::Zeroizing::new([0u8; 16]);
            pbkdf2_hmac::<Sha1>(&password, b"saltysalt", 1003, &mut *key);
            return Ok(key);
        }
    }
    anyhow::bail!("Could not read this client's Safe Storage key from macOS Keychain")
}

#[cfg(target_os = "macos")]
fn read_keychain_password(name: &str) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    let service = format!("{name} Safe Storage");
    let account = format!("{name} Key");
    if let Ok(password) = get_generic_password(&service, &account) {
        return Ok(zeroize::Zeroizing::new(password));
    }
    let output = std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            &service,
            "-a",
            &account,
            "-w",
        ])
        .output()
        .context("Failed to execute security command")?;
    anyhow::ensure!(
        output.status.success(),
        "Could not read Safe Storage key from Keychain"
    );
    let password = zeroize::Zeroizing::new(output.stdout);
    let password = password.strip_suffix(b"\n").unwrap_or(&password);
    let password = password.strip_suffix(b"\r").unwrap_or(password);
    Ok(zeroize::Zeroizing::new(password.to_vec()))
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct LinuxOscryptKeyCache {
    v10: Option<zeroize::Zeroizing<[u8; 16]>>,
    v11: once_cell::unsync::OnceCell<std::result::Result<zeroize::Zeroizing<[u8; 16]>, String>>,
}

#[cfg(target_os = "linux")]
impl LinuxOscryptKeyCache {
    fn key_for(&mut self, version: &[u8], client: &ClientSpec) -> Result<&[u8]> {
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
            b"v11" => self
                .v11
                .get_or_init(|| {
                    get_linux_safe_storage_password(client)
                        .map(|password| {
                            derive_linux_oscrypt_key(&zeroize::Zeroizing::new(password))
                        })
                        .map_err(|error| format!("{error:#}"))
                })
                .as_ref()
                .map(|key| &key[..])
                .map_err(|error| anyhow::anyhow!("{error}")),
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
    client: &ClientSpec,
    oscrypt_keys: &mut LinuxOscryptKeyCache,
) -> Result<String> {
    anyhow::ensure!(encrypted_data.len() > 3, "Encrypted data is too short");
    let key = oscrypt_keys.key_for(&encrypted_data[..3], client)?;
    decrypt_cbc_token(encrypted_data, key)
}

#[cfg(target_os = "linux")]
fn get_linux_safe_storage_password(client: &ClientSpec) -> Result<Vec<u8>> {
    use secret_service::{blocking::SecretService, EncryptionType};
    use std::collections::HashMap;

    let ss = SecretService::connect(EncryptionType::Dh)
        .or_else(|_| SecretService::connect(EncryptionType::Plain))
        .context("Could not connect to Linux Secret Service")?;

    for application_name in client.secret_applications {
        let mut attributes = HashMap::new();
        attributes.insert("application", *application_name);

        let search = ss
            .search_items(attributes)
            .with_context(|| format!("Could not search Secret Service for {}", application_name))?;

        for item in search.unlocked.iter().chain(search.locked.iter()) {
            if item.is_locked().unwrap_or(false) && item.unlock().is_err() {
                continue;
            }

            let item_attributes = item.get_attributes().unwrap_or_default();
            let label = item.get_label().unwrap_or_default();
            let looks_like_chromium_safe_storage = label == "Chromium Safe Storage"
                || label.ends_with(" Safe Storage")
                || item_attributes
                    .get("xdg:schema")
                    .map(|schema| schema == "chrome_libsecret_os_crypt_password_v2")
                    .unwrap_or(false);

            if !looks_like_chromium_safe_storage {
                continue;
            }

            if let Ok(secret) = item.get_secret() {
                if !secret.is_empty() {
                    return Ok(secret);
                }
            }
        }
    }

    anyhow::bail!("Could not find Linux Discord safe storage key")
}

static ENCRYPTED_PAYLOAD_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
    Regex::new(r"dQw4w9WgXcQ:([A-Za-z0-9+/=]+)").expect("encrypted token pattern is valid")
});

fn find_encrypted_token_payloads(content: &str) -> Vec<Vec<u8>> {
    ENCRYPTED_PAYLOAD_RE
        .captures_iter(content)
        .filter_map(|capture| BASE64.decode(capture.get(1)?.as_str()).ok())
        .collect()
}

static PLAINTEXT_TOKEN_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
    Regex::new(concat!(
        r"(?:mfa\.[A-Za-z0-9_-]{70,}|",
        r"[A-Za-z0-9_-]{20,30}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{25,120})"
    ))
    .expect("plaintext token pattern is valid")
});

fn find_plaintext_tokens(content: &str) -> Vec<String> {
    PLAINTEXT_TOKEN_RE
        .find_iter(content)
        .map(|matched| matched.as_str().to_string())
        .collect()
}

/// Decrypt legacy Windows OSCrypt AES-256-GCM data.
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

    anyhow::ensure!(
        encrypted_data.starts_with(b"v10") || encrypted_data.starts_with(b"v11"),
        "Unsupported Windows OSCrypt encryption version"
    );

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

/// Chromium's legacy macOS/Linux OSCrypt representation is a version prefix
/// followed by AES-128-CBC ciphertext with a fixed IV. Key derivation is OS-specific.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn decrypt_cbc_token(encrypted_data: &[u8], key: &[u8]) -> Result<String> {
    use aes::cipher::{BlockDecryptMut, KeyIvInit};
    use cbc::Decryptor;

    anyhow::ensure!(
        encrypted_data.starts_with(b"v10") || encrypted_data.starts_with(b"v11"),
        "Unsupported OSCrypt encryption version"
    );
    let mut buffer = zeroize::Zeroizing::new(encrypted_data[3..].to_vec());
    let cipher = Decryptor::<aes::Aes128>::new_from_slices(key, b"                ")
        .map_err(|_| anyhow::anyhow!("Invalid OSCrypt key length"))?;
    let plaintext = cipher
        .decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buffer)
        .map_err(|_| anyhow::anyhow!("OSCrypt decryption failed"))?;
    String::from_utf8(plaintext.to_vec()).context("Decrypted data is not valid UTF-8")
}

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
    pub native_build_number: Option<u64>,
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
    let platform = if cfg!(windows) {
        "win"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };
    let arch = crate::super_properties::host_architecture();
    let url = format!("https://updates.discord.com/distributions/app/manifests/latest?channel=stable&platform={platform}&arch={arch}");

    let response = client
        .get(&url)
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

    parse_discord_client_info(&manifest)
}

fn parse_discord_client_info(manifest: &serde_json::Value) -> Result<DiscordClientInfo> {
    let components = manifest["host_version"]
        .as_array()
        .context("Missing host_version")?;
    if components.len() != 3 {
        anyhow::bail!("Invalid host_version length");
    }
    let mut version = [0; 3];
    for (index, component) in components.iter().enumerate() {
        version[index] = u32::try_from(
            component
                .as_u64()
                .context("Invalid host_version component")?,
        )?;
    }
    // native_module_version and a host-version suffix are not host build numbers.
    let native_build = manifest
        .get("native_build_number")
        .and_then(|value| value.as_u64());
    Ok(DiscordClientInfo {
        host_version: version,
        native_build_number: native_build,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_does_not_guess_native_host_build_or_version_components() {
        let info = parse_discord_client_info(
            &serde_json::json!({"host_version":[1,2,3],"native_module_version":123}),
        )
        .unwrap();
        assert_eq!(info.client_version(), "1.2.3");
        assert_eq!(info.native_build_number, None);
        assert_eq!(
            parse_discord_client_info(
                &serde_json::json!({"host_version":[1,2,3],"native_build_number":456})
            )
            .unwrap()
            .native_build_number,
            Some(456)
        );
        for version in [
            serde_json::json!([1, 0]),
            serde_json::json!([1, 2, "3"]),
            serde_json::json!([1, 0, 4294967296u64]),
        ] {
            assert!(
                parse_discord_client_info(&serde_json::json!({"host_version":version})).is_err()
            );
        }
    }

    #[test]
    fn test_find_plaintext_tokens() {
        let token = "abcdefghijklmnopqrstuvwx.abcdef.ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl";
        let data = format!("prefix\0{}\0suffix", token);

        let tokens = find_plaintext_tokens(&data);

        assert_eq!(tokens, vec![token.to_string()]);
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
            &profiles::CLIENTS[0],
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

        let payloads = find_encrypted_token_payloads(&data);

        assert_eq!(payloads, vec![payload.to_vec()]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn rejects_short_or_unprefixed_dpapi_keys_without_panicking() {
        assert!(strip_dpapi_prefix(b"DP").is_err());
        assert!(strip_dpapi_prefix(b"OTHER").is_err());
        assert_eq!(strip_dpapi_prefix(b"DPAPIencrypted").unwrap(), b"encrypted");
    }

    #[cfg(target_os = "windows")]
    fn write_windows_fixture(
        dir: &leveldb::fixtures::TestDirectory,
        storage: &str,
        key: &[u8; 32],
        token: &str,
    ) {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce,
        };
        use windows::Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::Cryptography::CryptProtectData,
        };

        let input = CRYPT_INTEGER_BLOB {
            cbData: key.len() as u32,
            pbData: key.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let mut protected = b"DPAPI".to_vec();
        unsafe {
            CryptProtectData(
                &input,
                windows::core::PCWSTR::null(),
                None,
                None,
                None,
                0,
                &mut output,
            )
            .unwrap();
            protected.extend_from_slice(std::slice::from_raw_parts(
                output.pbData,
                output.cbData as usize,
            ));
            let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        }
        let state = serde_json::json!({"os_crypt": {"encrypted_key": BASE64.encode(protected)}});
        dir.write(
            &format!("{storage}Local State"),
            state.to_string().as_bytes(),
        );

        let nonce = b"test-nonce12";
        let mut payload = b"v10".to_vec();
        payload.extend_from_slice(nonce);
        payload.extend(
            Aes256Gcm::new_from_slice(key)
                .unwrap()
                .encrypt(Nonce::from_slice(nonce), token.as_bytes())
                .unwrap(),
        );
        dir.write(
            &format!("{storage}Local Storage/leveldb/one.log"),
            format!("dQw4w9WgXcQ:{}", BASE64.encode(payload)).as_bytes(),
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_storage_roots_use_their_own_dpapi_keys() {
        let dir = leveldb::fixtures::TestDirectory::new();
        write_windows_fixture(&dir, "", &[1; 32], "first-synthetic-token");
        write_windows_fixture(&dir, "sessionData/", &[2; 32], "second-synthetic-token");
        let profile = ChromiumProfile {
            client: &profiles::CLIENTS[3],
            root: dir.0.clone(),
        };
        assert_eq!(
            extract_from_profile(&profile).unwrap().tokens,
            vec!["first-synthetic-token", "second-synthetic-token"]
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_session_storage_can_use_the_parent_local_state() {
        let dir = leveldb::fixtures::TestDirectory::new();
        write_windows_fixture(&dir, "", &[3; 32], "parent-key-synthetic-token");
        let content = fs::read(dir.0.join("Local Storage/leveldb/one.log")).unwrap();
        fs::remove_file(dir.0.join("Local Storage/leveldb/one.log")).unwrap();
        dir.write("sessionData/Local Storage/leveldb/one.log", &content);
        let profile = ChromiumProfile {
            client: &profiles::CLIENTS[5],
            root: dir.0.clone(),
        };
        assert_eq!(
            extract_from_profile(&profile).unwrap().tokens,
            vec!["parent-key-synthetic-token"]
        );
    }

    #[test]
    fn plaintext_profiles_do_not_require_local_state_or_os_credentials() {
        let dir = leveldb::fixtures::TestDirectory::new();
        let token = "abcdefghijklmnopqrstuvwx.abcdef.ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl";
        dir.write(
            "sessionData/Local Storage/leveldb/one.log",
            token.as_bytes(),
        );
        let profile = ChromiumProfile {
            client: &profiles::CLIENTS[5],
            root: dir.0.clone(),
        };
        assert_eq!(extract_from_profile(&profile).unwrap().tokens, vec![token]);
    }

    #[test]
    fn unsupported_formats_fail_before_loading_os_credentials() {
        let dir = leveldb::fixtures::TestDirectory::new();
        let encoded = BASE64.encode(b"v20synthetic-unsupported-payload");
        dir.write(
            "Local Storage/leveldb/one.log",
            format!("dQw4w9WgXcQ:{encoded}").as_bytes(),
        );
        let profile = ChromiumProfile {
            client: &profiles::CLIENTS[5],
            root: dir.0.clone(),
        };
        let error = extract_from_profile(&profile).err().unwrap();
        assert!(format!("{error:#}").contains("Unsupported or truncated OSCrypt token format"));
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

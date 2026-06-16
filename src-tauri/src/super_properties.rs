// X-Super-Properties Management Module
// Implements hybrid strategy: prioritizes extraction from local Discord client, falls back to dynamic generation

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Discord client mod detection bits (128-bit mask)
/// Source: https://github.com/sparklost/endcord/blob/main/endcord/client_properties.py
const CLIENT_MOD_DETECTION_BITS: u128 = 0b00000000100000000001000000010000000010000001000000001000000000000010000010000001000000000100000000000001000000000000100000000000;
const DEFAULT_CLIENT_VERSION: &str = "1.0.9238";
const DEFAULT_CHROME_VERSION: &str = "138.0.7204.251";
const DEFAULT_ELECTRON_VERSION: &str = "37.6.0";

fn discord_user_agent(client_version: &str) -> String {
    format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/{} Chrome/{} Electron/{} Safari/537.36",
        client_version, DEFAULT_CHROME_VERSION, DEFAULT_ELECTRON_VERSION
    )
}

/// SuperProperties Source Mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceMode {
    /// Obtained via CDP from Discord client (most accurate)
    Cdp,
    /// Parsed from Discord website JavaScript
    RemoteJs,
    /// Use built-in default values (fallback)
    Default,
}

impl SourceMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceMode::Cdp => "cdp",
            SourceMode::RemoteJs => "remote_js",
            SourceMode::Default => "default",
        }
    }
    
    pub fn display_name(&self) -> &'static str {
        match self {
            SourceMode::Cdp => "CDP (Discord Client)",
            SourceMode::RemoteJs => "Remote JS",
            SourceMode::Default => "Default",
        }
    }
}

/// X-Super-Properties struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperProperties {
    pub os: String,
    pub browser: String,
    pub release_channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    pub os_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_arch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_arch: Option<String>,
    pub system_locale: String,
    pub has_client_mods: bool,
    pub browser_user_agent: String,
    pub browser_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_sdk_version: Option<String>,
    pub client_build_number: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_build_number: Option<u64>,
    pub client_event_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_launch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_heartbeat_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_app_state: Option<String>,
}

/// Request identity snapshot shared by User-Agent and X-Super-Properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientIdentity {
    pub user_agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    pub browser_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_build_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_build_number: Option<u64>,
    pub source: String,
}

/// Runtime request header profile. Sensitive values should stay in memory.
#[derive(Debug, Clone)]
pub struct HeaderProfile {
    pub timezone: String,
    pub timezone_source: String,
    pub locale: String,
    pub locale_source: String,
    pub accept_language: String,
    pub accept_language_source: String,
    pub installation_id: Option<String>,
    pub installation_id_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderProfilePreview {
    pub timezone: String,
    pub timezone_source: String,
    pub locale: String,
    pub locale_source: String,
    pub accept_language: String,
    pub accept_language_source: String,
    pub installation_id_present: bool,
    pub installation_id_source: String,
}

impl HeaderProfile {
    fn default_locale() -> (String, String) {
        let from_env = std::env::var("LANG")
            .ok()
            .and_then(|raw| raw.split('.').next().map(str::to_string))
            .map(|locale| locale.replace('_', "-"))
            .filter(|locale| {
                !locale.trim().is_empty()
                    && locale != "C"
                    && locale != "POSIX"
                    && !locale.starts_with("C-")
            });

        match from_env {
            Some(locale) => (locale, "system".to_string()),
            None => ("en-US".to_string(), "default".to_string()),
        }
    }

    fn default_timezone() -> (String, String) {
        match std::env::var("TZ") {
            Ok(timezone) if !timezone.trim().is_empty() => (timezone, "system".to_string()),
            _ => ("UTC".to_string(), "default".to_string()),
        }
    }

    fn accept_language_for_locale(locale: &str) -> String {
        if locale.eq_ignore_ascii_case("en-US") {
            "en-US,en;q=0.9".to_string()
        } else {
            format!("{},en-US;q=0.9,en;q=0.8", locale)
        }
    }

    pub fn new() -> Self {
        let (locale, locale_source) = Self::default_locale();
        let (timezone, timezone_source) = Self::default_timezone();

        Self {
            timezone,
            timezone_source,
            accept_language: Self::accept_language_for_locale(&locale),
            accept_language_source: locale_source.clone(),
            locale,
            locale_source,
            installation_id: None,
            installation_id_source: "absent".to_string(),
        }
    }

    pub fn preview(&self) -> HeaderProfilePreview {
        HeaderProfilePreview {
            timezone: self.timezone.clone(),
            timezone_source: self.timezone_source.clone(),
            locale: self.locale.clone(),
            locale_source: self.locale_source.clone(),
            accept_language: self.accept_language.clone(),
            accept_language_source: self.accept_language_source.clone(),
            installation_id_present: self.installation_id.is_some(),
            installation_id_source: self.installation_id_source.clone(),
        }
    }

    fn apply_headers(&mut self, headers: &HashMap<String, String>) {
        for (key, value) in headers {
            let key = key.to_ascii_lowercase();
            let value = value.trim();
            if value.is_empty() || value == "[redacted]" {
                continue;
            }

            match key.as_str() {
                "x-discord-timezone" => {
                    self.timezone = value.to_string();
                    self.timezone_source = "cdp".to_string();
                }
                "x-discord-locale" => {
                    self.locale = value.to_string();
                    self.locale_source = "cdp".to_string();
                }
                "accept-language" => {
                    self.accept_language = value.to_string();
                    self.accept_language_source = "cdp".to_string();
                }
                "x-installation-id" => {
                    self.installation_id = Some(value.to_string());
                    self.installation_id_source = "cdp".to_string();
                }
                _ => {}
            }
        }
    }
}

impl Default for HeaderProfile {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SuperProperties {
    fn default() -> Self {
        Self {
            os: "Windows".to_string(),
            browser: "Discord Client".to_string(),
            release_channel: "stable".to_string(),
            client_version: Some(DEFAULT_CLIENT_VERSION.to_string()),
            os_version: "10.0.19045".to_string(),
            os_arch: Some("x64".to_string()),
            app_arch: Some("x64".to_string()),
            system_locale: "en-US".to_string(),
            has_client_mods: false, // Must be false
            browser_user_agent: discord_user_agent(DEFAULT_CLIENT_VERSION),
            browser_version: DEFAULT_ELECTRON_VERSION.to_string(),
            os_sdk_version: Some("19045".to_string()),
            // FALLBACK BUILD NUMBER: Captured ~May 2026. This hardcoded value is used when
            // CDP extraction and remote JS fetch both fail. May need periodic updates as
            // Discord releases new versions. The actual build number is fetched dynamically
            // from Discord when possible.
            // Default build_number
            // May 21st, 2026
            client_build_number: 548544,
            native_build_number: Some(82474),
            client_event_source: None,
            launch_signature: None,
            client_launch_id: None,
            client_heartbeat_session_id: None,
            client_app_state: Some("focused".to_string()),
        }
    }
}

/// Generates a clean launch_signature (clears detection bits)
pub fn generate_clean_launch_signature() -> String {
    let uuid = Uuid::new_v4();
    let uuid_int = uuid.as_u128();
    
    // Clear detection bits
    let clean_mask = !CLIENT_MOD_DETECTION_BITS;
    let clean_signature = uuid_int & clean_mask;
    
    Uuid::from_u128(clean_signature).to_string()
}

/// Generates client_launch_id (called once per application launch)
pub fn generate_client_launch_id() -> String {
    Uuid::new_v4().to_string()
}

/// Generates client_heartbeat_session_id (generated once per session)
pub fn generate_client_heartbeat_session_id() -> String {
    Uuid::new_v4().to_string()
}

/// X-Super-Properties manager
/// Created at application startup, reuses the same ID during the session
pub struct XSuperPropertiesManager {
    client_launch_id: String,
    client_heartbeat_session_id: String,
    client_ad_session_id: String,
    launch_signature: String,
    cached_build_number: Option<u64>,
    cached_super_properties: Option<SuperProperties>,
    // Value extracted from Discord client
    extracted_base64: Option<String>,
    source_mode: SourceMode,  // Current data source mode
    source_client: Option<String>,  // e.g., "Stable", "Canary", "PTB"
    // Dynamically obtained client information
    client_version: Option<String>,  // e.g., "1.0.9219"
    native_build_number: Option<u64>,
    header_profile: HeaderProfile,
}

impl XSuperPropertiesManager {
    /// Creates a new manager instance (called at application startup)
    pub fn new() -> Self {
        Self {
            client_launch_id: generate_client_launch_id(),
            client_heartbeat_session_id: generate_client_heartbeat_session_id(),
            client_ad_session_id: generate_client_heartbeat_session_id(),
            launch_signature: generate_clean_launch_signature(),
            cached_build_number: None,
            cached_super_properties: None,
            extracted_base64: None,
            source_mode: SourceMode::Default,
            source_client: None,
            client_version: None,
            native_build_number: None,
            header_profile: HeaderProfile::new(),
        }
    }


    
    /// Sets client information obtained from Discord Update API
    pub fn set_client_info(&mut self, version: String, native_build: u64) {
        self.client_version = Some(version);
        self.native_build_number = Some(native_build);
        // Clear cache to regenerate with new information
        self.cached_super_properties = None;
    }
    
    /// Sets SuperProperties from CDP-obtained data
    pub fn set_from_cdp(&mut self, base64_value: &str, decoded: &serde_json::Value) {
        self.extracted_base64 = Some(base64_value.to_string());
        self.source_mode = SourceMode::Cdp;
        
        // Attempt to extract key information from decoded data
        if let Some(build_number) = decoded.get("client_build_number").and_then(|v| v.as_u64()) {
            self.cached_build_number = Some(build_number);
        }
        if let Some(version) = decoded.get("client_version").and_then(|v| v.as_str()) {
            self.client_version = Some(version.to_string());
        }
        if let Some(native_build) = decoded.get("native_build_number").and_then(|v| v.as_u64()) {
            self.native_build_number = Some(native_build);
        }
        
        // Clear cache to use new information
        self.cached_super_properties = None;
    }
    
    /// Sets build number obtained from remote JS
    pub fn set_from_remote_js(&mut self, build_number: u64) {
        self.cached_build_number = Some(build_number);
        self.source_mode = SourceMode::RemoteJs;
        // Clear other CDP data
        self.extracted_base64 = None;
        self.cached_super_properties = None;
    }
    
    /// Gets the current source mode
    pub fn get_mode(&self) -> SourceMode {
        self.source_mode
    }
    
    /// Gets the current build number
    pub fn get_build_number(&self) -> Option<u64> {
        self.cached_build_number
    }

    pub fn client_heartbeat_session_id(&self) -> String {
        self.client_heartbeat_session_id.clone()
    }

    pub fn client_ad_session_id(&self) -> String {
        self.client_ad_session_id.clone()
    }

    pub fn get_header_profile(&self) -> HeaderProfile {
        self.header_profile.clone()
    }

    pub fn update_header_profile_from_headers(&mut self, headers: &HashMap<String, String>) {
        self.header_profile.apply_headers(headers);
    }

    pub fn get_user_agent_string(&self) -> String {
        self.get_super_properties().browser_user_agent
    }

    pub fn get_client_identity_snapshot(&self) -> ClientIdentity {
        let props = self.get_super_properties();
        ClientIdentity {
            user_agent: props.browser_user_agent,
            client_version: props.client_version,
            browser_version: props.browser_version,
            client_build_number: Some(props.client_build_number),
            native_build_number: props.native_build_number,
            source: self.source_mode.as_str().to_string(),
        }
    }
    
    /// Resets to default state (for manual retry)
    pub fn reset(&mut self) {
        self.cached_build_number = None;
        self.cached_super_properties = None;
        self.extracted_base64 = None;
        self.source_mode = SourceMode::Default;
        self.client_version = None;
        self.native_build_number = None;
        // Regenerate session IDs
        self.client_launch_id = generate_client_launch_id();
        self.client_heartbeat_session_id = generate_client_heartbeat_session_id();
        self.client_ad_session_id = generate_client_heartbeat_session_id();
        self.launch_signature = generate_clean_launch_signature();
        self.header_profile = HeaderProfile::new();
    }

    /// Gets the Base64 encoded X-Super-Properties string
    /// Prioritizes returning the value extracted from the Discord client, replacing session IDs within it.
    pub fn get_super_properties_base64(&self) -> String {
        let props = self.get_super_properties();
        match serde_json::to_string(&props) {
            Ok(json) => BASE64.encode(json),
            Err(e) => {
                eprintln!("Failed to serialize fallback SuperProperties: {}", e);
                BASE64.encode("{}")
            }
        }
    }

    pub fn get_super_properties(&self) -> SuperProperties {
        if let Some(ref extracted) = self.extracted_base64 {
            // Decode the extracted value, replace session IDs, then re-encode
            if let Ok(decoded) = BASE64.decode(extracted) {
                if let Ok(json_str) = String::from_utf8(decoded) {
                    if let Ok(mut props) = serde_json::from_str::<SuperProperties>(&json_str) {
                        // Replace session-level IDs (new ones generated on each launch)
                        props.launch_signature = Some(self.launch_signature.clone());
                        props.client_launch_id = Some(self.client_launch_id.clone());
                        props.client_heartbeat_session_id = Some(self.client_heartbeat_session_id.clone());
                        return props;
                    }
                }
            }
        }
        // Fallback to auto-generation
        self.build_properties()
    }



    /// Gets debug information
    pub fn get_debug_info(&self) -> DebugInfo {
        // Get the actually used SuperProperties (consider extracted values)
        let props = self.get_super_properties();
        
        // Generate source display text
        let source = if let Some(ref client) = self.source_client {
            format!("{} ({})", self.source_mode.display_name(), client)
        } else {
            self.source_mode.display_name().to_string()
        };
        
        DebugInfo {
            x_super_properties_base64: self.get_super_properties_base64(),
            super_properties: props,
            client_launch_id: self.client_launch_id.clone(),
            client_heartbeat_session_id: self.client_heartbeat_session_id.clone(),
            launch_signature: self.launch_signature.clone(),
            source,
            client_identity: self.get_client_identity_snapshot(),
            header_profile: self.header_profile.preview(),
        }
    }

    fn build_properties(&self) -> SuperProperties {
        if let Some(ref cached) = self.cached_super_properties {
            return cached.clone();
        }

        let mut props = SuperProperties::default();
        props.launch_signature = Some(self.launch_signature.clone());
        props.client_launch_id = Some(self.client_launch_id.clone());
        props.client_heartbeat_session_id = Some(self.client_heartbeat_session_id.clone());
        
        if let Some(build_number) = self.cached_build_number {
            props.client_build_number = build_number;
        }
        
        // Use dynamically obtained client version information
        if let Some(ref version) = self.client_version {
            props.client_version = Some(version.clone());
            // Also update browser_user_agent
            props.browser_user_agent = discord_user_agent(version);
        }
        
        if let Some(native_build) = self.native_build_number {
            props.native_build_number = Some(native_build);
        }
        
        props
    }


}

impl Default for XSuperPropertiesManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Debug info struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugInfo {
    pub x_super_properties_base64: String,
    pub super_properties: SuperProperties,
    pub client_launch_id: String,
    pub client_heartbeat_session_id: String,
    pub launch_signature: String,
    pub source: String,  // "Auto-Generated" or "Discord Client (Extracted)"
    pub client_identity: ClientIdentity,
    pub header_profile: HeaderProfilePreview,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_clean_launch_signature() {
        let signature = generate_clean_launch_signature();
        
        // Verify it is a valid UUID format
        assert!(Uuid::parse_str(&signature).is_ok());
        
        // Verify detection bits are cleared
        let uuid = Uuid::parse_str(&signature).unwrap();
        let uuid_int = uuid.as_u128();
        assert_eq!(uuid_int & CLIENT_MOD_DETECTION_BITS, 0);
    }

    #[test]
    fn test_super_properties_serialization() {
        let props = SuperProperties::default();
        let json = serde_json::to_string(&props).unwrap();
        
        // Verify has_client_mods is false
        assert!(json.contains("\"has_client_mods\":false"));
    }

    #[test]
    fn test_manager_generates_unique_ids() {
        let manager1 = XSuperPropertiesManager::new();
        let manager2 = XSuperPropertiesManager::new();
        
        // Each manager creation should generate different IDs
        assert_ne!(manager1.client_launch_id, manager2.client_launch_id);
        assert_ne!(manager1.launch_signature, manager2.launch_signature);
    }

    #[test]
    fn test_base64_encoding() {
        let manager = XSuperPropertiesManager::new();
        let base64 = manager.get_super_properties_base64();
        
        // Verify it can be correctly decoded
        let decoded = BASE64.decode(&base64).unwrap();
        let json_str = String::from_utf8(decoded).unwrap();
        let props: SuperProperties = serde_json::from_str(&json_str).unwrap();
        
        assert_eq!(props.os, "Windows");
        assert!(props.launch_signature.is_some());
    }

    #[test]
    fn client_identity_keeps_user_agent_and_xsp_in_sync() {
        let mut manager = XSuperPropertiesManager::new();
        manager.set_client_info("1.0.9241".to_string(), 83924);
        manager.set_from_remote_js(562538);

        let identity = manager.get_client_identity_snapshot();
        let props = manager.get_super_properties();

        assert_eq!(identity.user_agent, props.browser_user_agent);
        assert_eq!(identity.client_version, props.client_version);
        assert!(identity.user_agent.contains("discord/1.0.9241"));
        assert_eq!(identity.client_build_number, Some(562538));
        assert_eq!(identity.native_build_number, Some(83924));
    }

    #[test]
    fn cdp_header_profile_redacts_installation_id_in_preview() {
        let mut manager = XSuperPropertiesManager::new();
        let headers = HashMap::from([
            ("x-discord-timezone".to_string(), "Asia/Shanghai".to_string()),
            ("x-discord-locale".to_string(), "en-US".to_string()),
            ("accept-language".to_string(), "en-US,zh-Hans-CN;q=0.9".to_string()),
            ("x-installation-id".to_string(), "installation-secret".to_string()),
        ]);

        manager.update_header_profile_from_headers(&headers);
        let profile = manager.get_header_profile();
        let preview = profile.preview();

        assert_eq!(profile.installation_id.as_deref(), Some("installation-secret"));
        assert!(preview.installation_id_present);
        assert_eq!(preview.installation_id_source, "cdp");
        assert_eq!(preview.timezone, "Asia/Shanghai");
    }
}

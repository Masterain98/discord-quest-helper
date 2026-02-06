// X-Super-Properties Management Module
// Implements hybrid strategy: prioritizes extraction from local Discord client, falls back to dynamic generation

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Discord client mod detection bits (128-bit mask)
/// Source: https://github.com/sparklost/endcord/blob/main/endcord/client_properties.py
const CLIENT_MOD_DETECTION_BITS: u128 = 0b00000000100000000001000000010000000010000001000000001000000000000010000010000001000000000100000000000001000000000000100000000000;

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

impl Default for SuperProperties {
    fn default() -> Self {
        Self {
            os: "Windows".to_string(),
            browser: "Discord Client".to_string(),
            release_channel: "stable".to_string(),
            client_version: Some("1.0.9219".to_string()),
            os_version: "10.0.19045".to_string(),
            os_arch: Some("x64".to_string()),
            app_arch: Some("x64".to_string()),
            system_locale: "en-US".to_string(),
            has_client_mods: false, // Must be false
            browser_user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/1.0.9219 Chrome/138.0.7204.251 Electron/37.6.0 Safari/537.36".to_string(),
            browser_version: "37.6.0".to_string(),
            os_sdk_version: Some("19045".to_string()),
            // FALLBACK BUILD NUMBER: Captured ~Jan 2026. This hardcoded value is used when
            // CDP extraction and remote JS fetch both fail. May need periodic updates as
            // Discord releases new versions. The actual build number is fetched dynamically
            // from Discord when possible.
            client_build_number: 493063,
            native_build_number: Some(73211),
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
}

impl XSuperPropertiesManager {
    /// Creates a new manager instance (called at application startup)
    pub fn new() -> Self {
        Self {
            client_launch_id: generate_client_launch_id(),
            client_heartbeat_session_id: generate_client_heartbeat_session_id(),
            launch_signature: generate_clean_launch_signature(),
            cached_build_number: None,
            cached_super_properties: None,
            extracted_base64: None,
            source_mode: SourceMode::Default,
            source_client: None,
            client_version: None,
            native_build_number: None,
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
        self.launch_signature = generate_clean_launch_signature();
    }

    /// Gets the Base64 encoded X-Super-Properties string
    /// Prioritizes returning the value extracted from the Discord client, replacing session IDs within it.
    pub fn get_super_properties_base64(&self) -> String {
        if let Some(ref extracted) = self.extracted_base64 {
            // Decode the extracted value, replace session IDs, then re-encode
            if let Ok(decoded) = BASE64.decode(extracted) {
                if let Ok(json_str) = String::from_utf8(decoded) {
                    if let Ok(mut props) = serde_json::from_str::<SuperProperties>(&json_str) {
                        // Replace session-level IDs (new ones generated on each launch)
                        props.launch_signature = Some(self.launch_signature.clone());
                        props.client_launch_id = Some(self.client_launch_id.clone());
                        props.client_heartbeat_session_id = Some(self.client_heartbeat_session_id.clone());
                        match serde_json::to_string(&props) {
                            Ok(json) => return BASE64.encode(json),
                            Err(e) => eprintln!("Failed to serialize updated SuperProperties: {}", e),
                        }
                    }
                }
            }
        }
        // Fallback to auto-generation
        let props = self.build_properties();
        match serde_json::to_string(&props) {
            Ok(json) => BASE64.encode(json),
            Err(e) => {
                eprintln!("Failed to serialize fallback SuperProperties: {}", e);
                // Last-resort non-empty value to avoid sending empty header
                BASE64.encode("{}")
            }
        }
    }



    /// Gets debug information
    pub fn get_debug_info(&self) -> DebugInfo {
        // Get the actually used SuperProperties (consider extracted values)
        let props = if let Some(ref extracted) = self.extracted_base64 {
            if let Ok(decoded) = BASE64.decode(extracted) {
                if let Ok(json_str) = String::from_utf8(decoded) {
                    if let Ok(mut p) = serde_json::from_str::<SuperProperties>(&json_str) {
                        p.launch_signature = Some(self.launch_signature.clone());
                        p.client_launch_id = Some(self.client_launch_id.clone());
                        p.client_heartbeat_session_id = Some(self.client_heartbeat_session_id.clone());
                        p
                    } else {
                        self.build_properties()
                    }
                } else {
                    self.build_properties()
                }
            } else {
                self.build_properties()
            }
        } else {
            self.build_properties()
        };
        
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
            props.browser_user_agent = format!(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/{} Chrome/138.0.7204.251 Electron/37.6.0 Safari/537.36",
                version
            );
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
}

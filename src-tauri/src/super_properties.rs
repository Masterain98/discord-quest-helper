// X-Super-Properties Management Module
// Implements hybrid strategy: prioritizes extraction from local Discord client, falls back to dynamic generation

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

mod environment;

pub fn host_architecture() -> String {
    environment::os_arch()
}
pub mod cdp;
mod heartbeat;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Discord client mod detection bits (128-bit mask)
/// Environment detection flags embedded in launch signatures.
pub(crate) const CLIENT_MOD_DETECTION_BITS: u128 = 0b00000000100000000001000000010000000010000001000000001000000000000010000010000001000000000100000000000001000000000000100000000000;

// ─────────────────────────────────────────────────────────────────────────────
// Discord client version constants — update these together when Discord ships
// a new client release.  Every other module references these instead of
// hardcoding their own values.
// ─────────────────────────────────────────────────────────────────────────────
/// Fallback build number when CDP extraction and remote JS fetch both fail.
/// Updated: October 17th, 2026
pub(crate) const DEFAULT_CLIENT_VERSION: &str = "1.0.9262";
pub(crate) const DEFAULT_CHROME_VERSION: &str = "148.0.7778.280";
pub(crate) const DEFAULT_ELECTRON_VERSION: &str = "42.11.11";
pub(crate) const DEFAULT_OS_VERSION: &str = "10.0.19045";
pub(crate) const DEFAULT_OS_SDK_VERSION: &str = "19045";
pub(crate) const DEFAULT_CLIENT_BUILD_NUMBER: u64 = 641004;
pub(crate) const DEFAULT_NATIVE_BUILD_NUMBER: u64 = 94186;

pub(crate) fn discord_user_agent(client_version: &str) -> String {
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
            os_version: DEFAULT_OS_VERSION.to_string(),
            os_arch: Some("x64".to_string()),
            app_arch: Some("x64".to_string()),
            system_locale: "en-US".to_string(),
            has_client_mods: false, // Must be false
            browser_user_agent: discord_user_agent(DEFAULT_CLIENT_VERSION),
            browser_version: DEFAULT_ELECTRON_VERSION.to_string(),
            os_sdk_version: Some(DEFAULT_OS_SDK_VERSION.to_string()),
            client_build_number: DEFAULT_CLIENT_BUILD_NUMBER,
            native_build_number: Some(DEFAULT_NATIVE_BUILD_NUMBER),
            client_event_source: None,
            launch_signature: None,
            client_launch_id: None,
            client_heartbeat_session_id: None,
            client_app_state: Some("focused".to_string()),
        }
    }
}

impl SuperProperties {
    /// Builds a Gateway Identify payload (op 2) from the current properties.
    /// The `token` parameter is the user's authentication token.
    pub fn to_gateway_identify_payload(&self, token: &str) -> serde_json::Value {
        serde_json::json!({
            "op": 2,
            "d": {
                "token": token,
                "capabilities": 30717,
                "properties": {
                    "os": &self.os,
                    "browser": &self.browser,
                    "device": "",
                    "system_locale": &self.system_locale,
                    "browser_user_agent": &self.browser_user_agent,
                    "browser_version": &self.browser_version,
                    "os_version": &self.os_version,
                    "referrer": "",
                    "referring_domain": "",
                    "referrer_current": "",
                    "referring_domain_current": "",
                    "release_channel": &self.release_channel,
                    "client_build_number": self.client_build_number,
                    "client_event_source": &self.client_event_source
                },
                "presence": {
                    "status": "online",
                    "since": 0,
                    "activities": [],
                    "afk": false
                },
                "compress": false,
                "client_state": {
                    "guild_versions": {},
                    "highest_last_message_id": "0",
                    "read_state_version": 0,
                    "user_guild_settings_version": -1,
                    "user_settings_version": -1,
                    "private_channels_version": "0",
                    "api_code_version": 0
                }
            }
        })
    }
}

/// Generates a clean launch_signature (clears detection bits)
pub fn generate_clean_launch_signature() -> String {
    signature_from_bytes(*Uuid::new_v4().as_bytes(), 0)
}

/// Stamp UUID version/variant bits and overwrite the environment flags.
pub fn signature_from_bytes(mut bytes: [u8; 16], detected: u128) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let value = (u128::from_be_bytes(bytes) & !CLIENT_MOD_DETECTION_BITS)
        | (detected & CLIENT_MOD_DETECTION_BITS);
    Uuid::from_u128(value).to_string()
}

/// Generates client_launch_id (called once per application launch)
pub fn generate_client_launch_id() -> String {
    Uuid::new_v4().to_string()
}

/// One immutable identity is used for all headers and decision query parameters.
#[derive(Clone)]
pub struct RequestIdentity {
    pub properties: Value,
    pub base64: String,
    pub user_agent: String,
    pub heartbeat_session_id: Option<String>,
    pub ad_session_id: String,
    pub header_profile: HeaderProfile,
}

/// Local launch identity survives source refreshes; imported identities belong
/// to a verified renderer/account and never overwrite the local fallback.
#[derive(Clone)]
pub struct XSuperPropertiesManager {
    client_ad_session_id: String,
    local_base: Value,
    properties: Value,
    encoded: String,
    pub session_key: String,
    pub account_id: Option<String>,
    pub cdp_port: Option<u16>,
    renderer_key: Option<String>,
    renderer_fallback: Option<Value>,
    source_mode: SourceMode,
    local_mode: SourceMode,
    header_profile: HeaderProfile,
    local_session: heartbeat::Heartbeat,
    focused: bool,
    pub field_sources: Map<String, Value>,
    pub degradation_reason: Option<String>,
    credential: Option<String>,
    metadata_sources: Map<String, Value>,
}

impl XSuperPropertiesManager {
    pub fn new() -> Self {
        let client_launch_id = generate_client_launch_id();
        let launch_signature = generate_clean_launch_signature();
        let local_base = local_properties(&client_launch_id, &launch_signature);
        let mut manager = Self {
            client_ad_session_id: Uuid::new_v4().to_string(),
            properties: local_base.clone(),
            local_base,
            encoded: String::new(),
            session_key: Uuid::new_v4().to_string(),
            account_id: None,
            credential: None,
            cdp_port: None,
            renderer_key: None,
            renderer_fallback: None,
            source_mode: SourceMode::Default,
            local_mode: SourceMode::Default,
            header_profile: HeaderProfile::new(),
            local_session: Default::default(),
            focused: false,
            field_sources: Map::new(),
            degradation_reason: None,
            metadata_sources: Map::new(),
        };
        manager.use_local(None);
        manager
    }

    pub fn candidate(&self, credential: String) -> Self {
        let mut next = self.clone();
        next.session_key = Uuid::new_v4().to_string();
        next.account_id = None;
        next.credential = Some(credential);
        next.local_session.session = None;
        next.local_session.credential = None;
        next.local_session.defer_writes = true;
        next.cdp_port = None;
        next.renderer_key = None;
        next.renderer_fallback = None;
        next.header_profile = HeaderProfile::new();
        next.use_local(None);
        next
    }

    pub fn configure_storage(&mut self, path: PathBuf) {
        self.local_session.path = Some(path);
    }

    pub fn commit(&mut self, current: &Self) {
        self.focused = current.focused;
        self.local_session.commit();
        self.activity_tick(now_ms());
    }

    pub fn authenticate(&mut self, account: String) {
        self.account_id = Some(account);
        if let Some(credential) = self.credential.clone() {
            self.local_session.bind(credential, now_ms());
        }
        self.activity_tick(now_ms());
    }

    pub fn logout(&mut self) {
        self.local_session.clear();
        self.account_id = None;
        self.credential = None;
        self.session_key = Uuid::new_v4().to_string();
        self.cdp_port = None;
        self.renderer_key = None;
        self.renderer_fallback = None;
        self.header_profile = HeaderProfile::new();
        self.use_local(None);
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.activity_tick(now_ms());
    }

    pub fn activity_tick(&mut self, now: u64) {
        self.local_session
            .update(self.account_id.is_some() && self.focused, now);
        if self.source_mode != SourceMode::Cdp {
            self.use_local_at(None, now);
        }
    }

    pub fn set_client_info(&mut self, version: String, native_build: Option<u64>) {
        self.local_base["client_version"] = version.clone().into();
        self.local_base["browser_user_agent"] =
            desktop_user_agent(&version, &self.local_base).into();
        self.metadata_sources
            .insert("client_version".into(), "update_manifest".into());
        self.metadata_sources
            .insert("browser_user_agent".into(), "generated".into());
        if let Some(build) = native_build {
            self.local_base["native_build_number"] = build.into();
            self.metadata_sources
                .insert("native_build_number".into(), "update_manifest".into());
        }
        if self.source_mode != SourceMode::Cdp {
            self.use_local(None);
        }
    }

    /// Accept a verified captured header. Encoding is retained if nothing changes.
    pub fn set_from_cdp(&mut self, base64_value: &str, decoded: &Value) {
        self.apply_cdp(
            base64_value,
            decoded,
            &Value::Object(Map::new()),
            None,
            None,
        );
    }

    pub fn apply_cdp(
        &mut self,
        encoded: &str,
        decoded: &Value,
        fallback: &Value,
        heartbeat: Option<Option<String>>,
        focus: Option<bool>,
    ) {
        if !decoded.is_object() {
            self.use_local(Some("invalid_cdp_properties".into()));
            return;
        }
        let mut properties = decoded.clone();
        self.field_sources = properties
            .as_object()
            .unwrap()
            .keys()
            .map(|key| (key.clone(), Value::from("cdp")))
            .collect();
        // Source fallback carries DiscordNative/UA/probe values. Do not replace
        // explicit nulls or supplement native data into a browser-only branch.
        for source in [fallback, &self.local_base] {
            if let Some(fields) = source.as_object() {
                for (key, value) in fields {
                    if matches!(
                        key.as_str(),
                        "client_heartbeat_session_id" | "client_app_state"
                    ) {
                        continue;
                    }
                    if key == "os_sdk_version" && properties["os"] == "Linux" {
                        continue;
                    }
                    if !properties.as_object().unwrap().contains_key(key) {
                        properties[key] = value.clone();
                        self.field_sources.insert(
                            key.clone(),
                            if std::ptr::eq(source, fallback) {
                                "cdp_environment".into()
                            } else {
                                "builtin".into()
                            },
                        );
                    }
                }
            }
        }
        if let Some(session) = heartbeat {
            match session {
                Some(id) => {
                    properties["client_heartbeat_session_id"] = id.into();
                }
                None => {
                    properties
                        .as_object_mut()
                        .unwrap()
                        .shift_remove("client_heartbeat_session_id");
                }
            }
            self.field_sources
                .insert("client_heartbeat_session_id".into(), "cdp_session".into());
        }
        if let Some(focused) = focus {
            properties["client_app_state"] = if focused { "focused" } else { "unfocused" }.into();
            self.field_sources
                .insert("client_app_state".into(), "cdp_window".into());
        }
        self.source_mode = SourceMode::Cdp;
        self.degradation_reason = None;
        self.encoded = if properties == *decoded && decode_object(encoded).as_ref() == Some(decoded)
        {
            encoded.into()
        } else {
            encode(&properties)
        };
        self.properties = properties;
    }

    pub fn accept_cdp(&mut self, snapshot: &cdp::Snapshot) -> Result<(), &'static str> {
        if self.account_id.as_deref() != Some(snapshot.account_id.as_str()) {
            self.use_local(Some("cdp_account_mismatch".into()));
            return Err("cdp_account_mismatch");
        }
        if self.renderer_key.as_deref() != Some(&snapshot.renderer_key) {
            let mut fallback = serde_json::json!({});
            fallback["client_launch_id"] = generate_client_launch_id().into();
            fallback["launch_signature"] =
                signature_from_bytes(*Uuid::new_v4().as_bytes(), snapshot.mod_bits).into();
            self.renderer_fallback = Some(fallback);
            self.renderer_key = Some(snapshot.renderer_key.clone());
        }
        let mut fallback = self.renderer_fallback.clone().unwrap();
        if let Some(fields) = snapshot.fallback.as_object() {
            for (key, value) in fields {
                fallback[key] = value.clone();
            }
        }
        let session = if snapshot.heartbeat_known {
            Some(snapshot.heartbeat_session_id.clone())
        } else if !snapshot
            .decoded
            .as_object()
            .is_some_and(|fields| fields.contains_key("client_heartbeat_session_id"))
        {
            Some(self.local_session.id(now_ms()))
        } else {
            None
        };
        self.apply_cdp(
            &snapshot.base64,
            &snapshot.decoded,
            &fallback,
            session,
            snapshot.focused,
        );
        for key in ["client_launch_id", "launch_signature"] {
            if snapshot.decoded.get(key).is_none() {
                self.field_sources.insert(key.into(), "generated".into());
            }
        }
        if !snapshot.heartbeat_known && session_is_generated(snapshot) {
            self.field_sources
                .insert("client_heartbeat_session_id".into(), "generated".into());
        }
        if !snapshot.heartbeat_known {
            self.degradation_reason = Some("cdp_session_getter_unavailable".into());
        }
        Ok(())
    }

    pub fn set_from_remote_js(&mut self, build_number: u64) {
        self.local_base["client_build_number"] = build_number.into();
        self.local_mode = SourceMode::RemoteJs;
        if self.source_mode != SourceMode::Cdp {
            self.use_local(None);
        }
    }

    pub fn use_local(&mut self, reason: Option<String>) {
        self.use_local_at(reason, now_ms());
    }
    fn use_local_at(&mut self, reason: Option<String>, now: u64) {
        self.source_mode = self.local_mode;
        if reason.is_some() {
            self.degradation_reason = reason;
        }
        self.properties = self.local_base.clone();
        let fields = self.properties.as_object_mut().unwrap();
        match self.local_session.id(now) {
            Some(id) => {
                fields.insert("client_heartbeat_session_id".into(), id.into());
            }
            None => {
                fields.shift_remove("client_heartbeat_session_id");
            }
        }
        fields.insert(
            "client_app_state".into(),
            if self.focused { "focused" } else { "unfocused" }.into(),
        );
        self.field_sources = fields
            .keys()
            .map(|key| {
                (
                    key.clone(),
                    Value::from(match key.as_str() {
                        "os"
                        | "os_version"
                        | "os_arch"
                        | "app_arch"
                        | "system_locale"
                        | "os_sdk_version"
                        | "window_manager"
                        | "display_server"
                        | "distro"
                        | "runtime_environment" => "system",
                        "client_launch_id"
                        | "launch_signature"
                        | "client_heartbeat_session_id"
                        | "client_app_state" => "generated",
                        "client_build_number" if self.local_mode == SourceMode::RemoteJs => {
                            "remote_js"
                        }
                        _ => "builtin",
                    }),
                )
            })
            .collect();
        self.encoded = encode(&self.properties);
        for (key, source) in &self.metadata_sources {
            self.field_sources.insert(key.clone(), source.clone());
        }
    }

    pub fn get_mode(&self) -> SourceMode {
        self.source_mode
    }
    pub fn get_build_number(&self) -> Option<u64> {
        self.properties["client_build_number"].as_u64()
    }
    #[cfg(test)]
    pub fn get_header_profile(&self) -> HeaderProfile {
        self.header_profile.clone()
    }
    pub fn update_header_profile_from_headers(&mut self, headers: &HashMap<String, String>) {
        self.header_profile.apply_headers(headers);
    }
    pub fn get_user_agent_string(&self) -> String {
        self.properties["browser_user_agent"]
            .as_str()
            .unwrap_or_else(|| self.local_base["browser_user_agent"].as_str().unwrap_or(""))
            .into()
    }
    #[cfg(test)]
    pub fn get_super_properties_base64(&self) -> String {
        self.encoded.clone()
    }
    #[cfg(test)]
    pub fn get_super_properties(&self) -> SuperProperties {
        serde_json::from_value(self.properties.clone()).unwrap_or_default()
    }
    pub fn get_client_identity_snapshot(&self) -> ClientIdentity {
        ClientIdentity {
            user_agent: self.get_user_agent_string(),
            client_version: self.properties["client_version"]
                .as_str()
                .map(str::to_string),
            browser_version: self.properties["browser_version"]
                .as_str()
                .unwrap_or("")
                .into(),
            client_build_number: self.get_build_number(),
            native_build_number: self.properties["native_build_number"].as_u64(),
            source: self.source_mode.as_str().into(),
        }
    }
    pub fn request_identity(&mut self) -> RequestIdentity {
        if self.source_mode != SourceMode::Cdp {
            self.use_local(None);
        }
        RequestIdentity {
            properties: self.properties.clone(),
            base64: self.encoded.clone(),
            user_agent: self.get_user_agent_string(),
            heartbeat_session_id: self.properties["client_heartbeat_session_id"]
                .as_str()
                .map(str::to_string),
            ad_session_id: self.client_ad_session_id.clone(),
            header_profile: self.header_profile.clone(),
        }
    }
    /// Retrying a source is not a new renderer launch or a credential change.
    pub fn reset(&mut self) {
        self.degradation_reason = None;
        self.use_local(None);
    }
    pub fn get_debug_info(&self) -> DebugInfo {
        let identity_status = [
            "client_launch_id",
            "launch_signature",
            "client_heartbeat_session_id",
        ]
        .into_iter()
        .map(|key| {
            let status = match self.properties.get(key) {
                None => "omitted",
                Some(Value::Null) => "null",
                _ if self.field_sources.get(key).and_then(Value::as_str) == Some("generated") => {
                    "generated"
                }
                _ => "reused",
            };
            (key.to_string(), Value::from(status))
        })
        .collect();
        DebugInfo {
            x_super_properties_base64: self.encoded.clone(),
            super_properties: self.properties.clone(),
            client_launch_id: self.properties["client_launch_id"]
                .as_str()
                .map(str::to_string),
            client_heartbeat_session_id: self.properties["client_heartbeat_session_id"]
                .as_str()
                .map(str::to_string),
            launch_signature: self.properties.get("launch_signature").cloned(),
            source: self.source_mode.display_name().into(),
            client_identity: self.get_client_identity_snapshot(),
            header_profile: self.header_profile.preview(),
            field_sources: self.field_sources.clone(),
            degradation_reason: self.degradation_reason.clone(),
            heartbeat_storage_error: self.local_session.storage_error,
            identity_status,
        }
    }
}

fn session_is_generated(snapshot: &cdp::Snapshot) -> bool {
    snapshot
        .decoded
        .get("client_heartbeat_session_id")
        .is_none()
}

fn decode_object(encoded: &str) -> Option<Value> {
    let decoded: Value = serde_json::from_slice(&BASE64.decode(encoded).ok()?).ok()?;
    decoded.is_object().then_some(decoded)
}
fn encode(value: &Value) -> String {
    BASE64.encode(serde_json::to_vec(value).expect("JSON property values serialize"))
}

fn desktop_user_agent(version: &str, properties: &Value) -> String {
    let architecture = properties["os_arch"].as_str().unwrap_or("x64");
    let platform = match properties["os"].as_str().unwrap_or("") {
        "Windows" => {
            if architecture == "arm64" {
                "Windows NT 10.0; Win64; arm64".into()
            } else if architecture == "ia32" {
                "Windows NT 10.0".into()
            } else {
                "Windows NT 10.0; Win64; x64".into()
            }
        }
        "Mac OS X" => "Macintosh; Intel Mac OS X 10_15_7".into(),
        _ => format!(
            "X11; Linux {}",
            if architecture == "x64" {
                "x86_64"
            } else {
                architecture
            }
        ),
    };
    format!("Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) discord/{version} Chrome/{DEFAULT_CHROME_VERSION} Electron/{DEFAULT_ELECTRON_VERSION} Safari/537.36")
}

fn local_properties(launch: &str, signature: &str) -> Value {
    let os = if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "Mac OS X"
    } else {
        "Linux"
    };
    let release = environment::release();
    let mut fields = Map::new();
    // Keep insertion order stable for consistent encoding.
    for (key, value) in [
        ("os", Value::from(os)),
        ("browser", "Discord Client".into()),
        ("release_channel", "stable".into()),
        ("client_version", DEFAULT_CLIENT_VERSION.into()),
        ("os_version", release.clone().into()),
        ("os_arch", environment::os_arch().into()),
        ("app_arch", environment::arch(std::env::consts::ARCH).into()),
        ("system_locale", environment::locale().into()),
        ("has_client_mods", false.into()),
        ("client_launch_id", launch.into()),
    ] {
        fields.insert(key.into(), value);
    }
    let ua = desktop_user_agent(DEFAULT_CLIENT_VERSION, &Value::Object(fields.clone()));
    fields.insert("browser_user_agent".into(), ua.into());
    fields.insert("browser_version".into(), DEFAULT_ELECTRON_VERSION.into());
    environment::platform_fields(&mut fields);
    if let Some(sdk) = release
        .split('.')
        .nth(if cfg!(windows) { 2 } else { 0 })
        .filter(|_| os != "Linux")
    {
        fields.insert("os_sdk_version".into(), sdk.into());
    }
    fields.insert(
        "client_build_number".into(),
        DEFAULT_CLIENT_BUILD_NUMBER.into(),
    );
    fields.insert(
        "native_build_number".into(),
        DEFAULT_NATIVE_BUILD_NUMBER.into(),
    );
    fields.insert("client_event_source".into(), Value::Null);
    fields.insert("launch_signature".into(), signature.into());
    Value::Object(fields)
}

impl Default for XSuperPropertiesManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugInfo {
    pub x_super_properties_base64: String,
    pub super_properties: Value,
    pub client_launch_id: Option<String>,
    pub client_heartbeat_session_id: Option<String>,
    pub launch_signature: Option<Value>,
    pub source: String,
    pub client_identity: ClientIdentity,
    pub header_profile: HeaderProfilePreview,
    pub field_sources: Map<String, Value>,
    pub degradation_reason: Option<String>,
    pub heartbeat_storage_error: bool,
    pub identity_status: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/super-properties.json")).unwrap()
    }

    fn snapshot(account: &str, renderer: &str) -> cdp::Snapshot {
        cdp::Snapshot {
            base64: fixture()["desktop"]["base64"].as_str().unwrap().into(),
            decoded: fixture()["desktop"]["properties"].clone(),
            fallback: serde_json::json!({}),
            focused: Some(true),
            heartbeat_known: true,
            heartbeat_session_id: Some("test-session".into()),
            account_id: account.into(),
            renderer_key: renderer.into(),
            mod_bit_indices: vec![],
            mod_bits: 0,
            module_ids: serde_json::json!({}),
            native_desktop: true,
        }
    }

    #[test]
    fn launch_signature_applies_each_environment_flag_with_nonzero_randomness() {
        for case in fixture()["signatures"].as_array().unwrap() {
            let bits = case["bits"]
                .as_array()
                .unwrap()
                .iter()
                .fold(0, |mask, bit| {
                    mask | (1u128 << (127 - bit.as_u64().unwrap()))
                });
            assert_eq!(
                signature_from_bytes([255; 16], bits),
                case["expected"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn property_encoding_preserves_order_unicode_and_null_values() {
        let fixture = fixture();
        let desktop = &fixture["desktop"];
        assert_eq!(
            encode(&desktop["properties"]),
            desktop["base64"].as_str().unwrap()
        );
        assert_eq!(desktop["properties"]["custom_text"], "中文测试");
        let mut manager = XSuperPropertiesManager::new();
        manager.set_from_cdp(desktop["base64"].as_str().unwrap(), &desktop["properties"]);
        assert_eq!(manager.encoded, desktop["base64"].as_str().unwrap());
        assert_eq!(manager.properties["launch_signature"], Value::Null);
        assert_eq!(manager.properties["client_launch_id"], "test-launch-id");
        assert_eq!(
            manager.properties["client_heartbeat_session_id"],
            "test-session"
        );
    }

    #[test]
    fn captured_bytes_unknown_fields_and_key_order_are_preserved() {
        let mut properties = fixture()["desktop"]["properties"].clone();
        properties["future_field"] = serde_json::json!({"emoji":"🍃", "nested":null});
        let raw = BASE64.encode(serde_json::to_string_pretty(&properties).unwrap());
        let mut manager = XSuperPropertiesManager::new();
        manager.set_from_cdp(&raw, &properties);
        assert_eq!(manager.encoded, raw);
        assert_eq!(manager.properties, properties);
        manager.apply_cdp(&raw, &properties, &Value::Null, Some(None), Some(false));
        let encoded: Value = decode_object(&manager.encoded).unwrap();
        assert_eq!(encoded["future_field"], properties["future_field"]);
        assert!(encoded.get("client_heartbeat_session_id").is_none());
        assert_eq!(encoded["client_app_state"], "unfocused");
        assert_eq!(encoded.as_object().unwrap().keys().next().unwrap(), "os");
    }

    #[test]
    fn cdp_account_renderer_failure_and_retry_boundaries() {
        let mut manager = XSuperPropertiesManager::new().candidate("credential".into());
        manager.authenticate("account-a".into());
        let local_launch = manager.local_base["client_launch_id"].clone();
        assert!(manager
            .accept_cdp(&snapshot("account-b", "renderer-1"))
            .is_err());
        assert_eq!(manager.properties["client_launch_id"], local_launch);
        let captured = snapshot("account-a", "renderer-1");
        manager.accept_cdp(&captured).unwrap();
        assert_eq!(manager.encoded, captured.base64);
        manager.use_local(Some("disconnected".into()));
        assert_eq!(manager.properties["client_launch_id"], local_launch);
        manager.accept_cdp(&captured).unwrap();
        assert_eq!(manager.properties["client_launch_id"], "test-launch-id");
        let mut missing = captured.clone();
        missing
            .decoded
            .as_object_mut()
            .unwrap()
            .shift_remove("launch_signature");
        manager.accept_cdp(&missing).unwrap();
        let generated = manager.properties["launch_signature"].clone();
        manager.reset();
        manager.accept_cdp(&missing).unwrap();
        assert_eq!(manager.properties["launch_signature"], generated);
        missing.renderer_key = "renderer-2".into();
        manager.accept_cdp(&missing).unwrap();
        assert_ne!(manager.properties["launch_signature"], generated);
    }

    #[test]
    fn cdp_missing_and_intentionally_absent_sessions_differ() {
        let mut manager = XSuperPropertiesManager::new().candidate("credential".into());
        manager.set_focused(true);
        manager.authenticate("account-a".into());
        let mut captured = snapshot("account-a", "renderer-1");
        captured
            .decoded
            .as_object_mut()
            .unwrap()
            .shift_remove("client_heartbeat_session_id");
        captured.heartbeat_session_id = None;
        manager.accept_cdp(&captured).unwrap();
        assert!(manager
            .properties
            .get("client_heartbeat_session_id")
            .is_none());
        captured.heartbeat_known = false;
        manager.accept_cdp(&captured).unwrap();
        assert_eq!(
            manager.request_identity().heartbeat_session_id,
            manager.local_session.id(now_ms())
        );
        assert_eq!(
            manager.field_sources["client_heartbeat_session_id"],
            "generated"
        );
    }

    #[test]
    fn linux_cdp_does_not_inherit_windows_sdk() {
        let mut manager = XSuperPropertiesManager::new();
        let mut properties = fixture()["desktop"]["properties"].clone();
        properties["os"] = "Linux".into();
        properties
            .as_object_mut()
            .unwrap()
            .shift_remove("os_sdk_version");
        manager.set_from_cdp(&encode(&properties), &properties);
        assert!(manager.properties.get("os_sdk_version").is_none());
    }

    #[test]
    fn auth_focus_and_refresh_keep_distinct_lifetimes() {
        let manager = XSuperPropertiesManager::new();
        let mut candidate = manager.candidate("credential".into());
        assert!(candidate.request_identity().heartbeat_session_id.is_none());
        candidate.set_focused(true);
        candidate.authenticate("account-a".into());
        let session = candidate.request_identity().heartbeat_session_id.unwrap();
        candidate.reset();
        assert_eq!(
            candidate.request_identity().heartbeat_session_id,
            Some(session)
        );
        candidate.set_focused(false);
        assert_eq!(
            candidate.request_identity().properties["client_app_state"],
            "unfocused"
        );
        candidate.logout();
        assert!(candidate.request_identity().heartbeat_session_id.is_none());
        assert_eq!(
            candidate.properties["client_launch_id"],
            manager.properties["client_launch_id"]
        );
        assert_eq!(
            candidate.properties["launch_signature"],
            manager.properties["launch_signature"]
        );
    }

    #[test]
    fn failed_candidate_cannot_overwrite_persisted_session() {
        let dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let path = dir.join("session.json");
        let mut current = XSuperPropertiesManager::new();
        current.configure_storage(path.clone());
        current.credential = Some("credential-a".into());
        current.set_focused(true);
        current.authenticate("account-a".into());
        let original = std::fs::read(&path).unwrap();
        let mut candidate = current.candidate("credential-b".into());
        candidate.authenticate("account-b".into());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        candidate.commit(&current);
        assert_ne!(std::fs::read(&path).unwrap(), original);
        candidate.logout();
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

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
        assert_ne!(
            manager1.local_base["client_launch_id"],
            manager2.local_base["client_launch_id"]
        );
        assert_ne!(
            manager1.local_base["launch_signature"],
            manager2.local_base["launch_signature"]
        );
    }

    #[test]
    fn test_base64_encoding() {
        let manager = XSuperPropertiesManager::new();
        let base64 = manager.get_super_properties_base64();

        // Verify it can be correctly decoded
        let decoded = BASE64.decode(&base64).unwrap();
        let json_str = String::from_utf8(decoded).unwrap();
        let props: SuperProperties = serde_json::from_str(&json_str).unwrap();

        assert_eq!(
            props.os,
            if cfg!(windows) {
                "Windows"
            } else if cfg!(target_os = "macos") {
                "Mac OS X"
            } else {
                "Linux"
            }
        );
        assert!(props.launch_signature.is_some());
    }

    #[test]
    fn client_identity_keeps_user_agent_and_xsp_in_sync() {
        let mut manager = XSuperPropertiesManager::new();
        manager.set_client_info("1.0.9241".to_string(), Some(83924));
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
            (
                "x-discord-timezone".to_string(),
                "Asia/Shanghai".to_string(),
            ),
            ("x-discord-locale".to_string(), "en-US".to_string()),
            (
                "accept-language".to_string(),
                "en-US,zh-Hans-CN;q=0.9".to_string(),
            ),
            (
                "x-installation-id".to_string(),
                "installation-secret".to_string(),
            ),
        ]);

        manager.update_header_profile_from_headers(&headers);
        let profile = manager.get_header_profile();
        let preview = profile.preview();

        assert_eq!(
            profile.installation_id.as_deref(),
            Some("installation-secret")
        );
        assert!(preview.installation_id_present);
        assert_eq!(preview.installation_id_source, "cdp");
        assert_eq!(preview.timezone, "Asia/Shanghai");
    }
}

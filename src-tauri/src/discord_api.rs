use crate::models::*;
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, REFERER, USER_AGENT};
use reqwest::{Method, RequestBuilder};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DISCORD_API_BASE: &str = "https://discord.com/api/v9";
const PROXY_STATE_CHECK_INTERVAL_MS: u64 = 5_000;
const QUEST_HOME_REFERER: &str = "https://discord.com/quest-home";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProxyState {
    fingerprint: u64,
    has_proxy: bool,
}

impl ProxyState {
    fn hash_setting(hasher: &mut DefaultHasher, key: &str, value: &str) {
        key.hash(hasher);
        value.trim().hash(hasher);
    }

    fn current() -> Self {
        let mut hasher = DefaultHasher::new();

        let http_proxy = std::env::var("HTTP_PROXY").unwrap_or_default();
        let https_proxy = std::env::var("HTTPS_PROXY").unwrap_or_default();
        let all_proxy = std::env::var("ALL_PROXY").unwrap_or_default();
        let no_proxy = std::env::var("NO_PROXY").unwrap_or_default();
        let http_proxy_lower = std::env::var("http_proxy").unwrap_or_default();
        let https_proxy_lower = std::env::var("https_proxy").unwrap_or_default();
        let all_proxy_lower = std::env::var("all_proxy").unwrap_or_default();
        let no_proxy_lower = std::env::var("no_proxy").unwrap_or_default();

        Self::hash_setting(&mut hasher, "HTTP_PROXY", &http_proxy);
        Self::hash_setting(&mut hasher, "HTTPS_PROXY", &https_proxy);
        Self::hash_setting(&mut hasher, "ALL_PROXY", &all_proxy);
        Self::hash_setting(&mut hasher, "NO_PROXY", &no_proxy);
        Self::hash_setting(&mut hasher, "http_proxy", &http_proxy_lower);
        Self::hash_setting(&mut hasher, "https_proxy", &https_proxy_lower);
        Self::hash_setting(&mut hasher, "all_proxy", &all_proxy_lower);
        Self::hash_setting(&mut hasher, "no_proxy", &no_proxy_lower);

        let mut has_proxy = !http_proxy.trim().is_empty()
            || !https_proxy.trim().is_empty()
            || !all_proxy.trim().is_empty()
            || !http_proxy_lower.trim().is_empty()
            || !https_proxy_lower.trim().is_empty()
            || !all_proxy_lower.trim().is_empty();

        #[cfg(windows)]
        {
            let maybe_settings = windows_registry::CURRENT_USER
                .open("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings");

            match maybe_settings {
                Ok(settings) => {
                    let proxy_enable = settings.get_u32("ProxyEnable").unwrap_or(0);
                    let proxy_server = settings.get_string("ProxyServer").unwrap_or_default();
                    let proxy_override = settings.get_string("ProxyOverride").unwrap_or_default();
                    let auto_config_url = settings.get_string("AutoConfigURL").unwrap_or_default();
                    let auto_detect = settings.get_u32("AutoDetect").unwrap_or(0);

                    "ProxyEnable".hash(&mut hasher);
                    proxy_enable.hash(&mut hasher);
                    Self::hash_setting(&mut hasher, "ProxyServer", &proxy_server);
                    Self::hash_setting(&mut hasher, "ProxyOverride", &proxy_override);
                    Self::hash_setting(&mut hasher, "AutoConfigURL", &auto_config_url);
                    "AutoDetect".hash(&mut hasher);
                    auto_detect.hash(&mut hasher);

                    has_proxy = has_proxy
                        || (proxy_enable == 1 && !proxy_server.trim().is_empty())
                        || !auto_config_url.trim().is_empty()
                        || auto_detect == 1;
                }
                Err(_) => {
                    "registry_unavailable".hash(&mut hasher);
                }
            }
        }

        Self {
            fingerprint: hasher.finish(),
            has_proxy,
        }
    }
}

/// Discord API client
#[derive(Clone)]
pub struct DiscordApiClient {
    client: Arc<ArcSwap<reqwest::Client>>,
    proxy_fingerprint: Arc<AtomicU64>,
    proxy_has_proxy: Arc<AtomicBool>,
    created_at: Arc<Instant>,
    last_proxy_check_elapsed_ms: Arc<AtomicU64>,
    token: String,
}

impl DiscordApiClient {
    fn normalize_video_timestamp(timestamp: f64) -> u64 {
        if !timestamp.is_finite() || timestamp <= 0.0 {
            return 0;
        }

        timestamp.round() as u64
    }

    fn build_default_headers(token: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(token).context("Invalid token format")?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        // Note: X-Super-Properties is no longer set here, but dynamically obtained on each request
        // This ensures the latest validation parameters (including data obtained from CDP) are used
        headers.insert(
            "x-debug-options",
            HeaderValue::from_static("bugReporterEnabled"),
        );
        headers.insert("accept", HeaderValue::from_static("*/*"));

        Ok(headers)
    }

    fn build_http_client(token: &str) -> Result<reqwest::Client> {
        let headers = Self::build_default_headers(token)?;

        reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .build()
            .context("Could not create HTTP client")
    }

    /// Create a new API client
    pub fn new(token: String) -> Result<Self> {
        use crate::logger::{log, LogCategory, LogLevel};

        let proxy_state = ProxyState::current();
        let client = Self::build_http_client(&token)?;

        log(
            LogLevel::Info,
            LogCategory::Api,
            "HTTP client initialized",
            Some(if proxy_state.has_proxy {
                "system proxy detected"
            } else {
                "no system proxy detected"
            }),
        );

        let created_at = Arc::new(Instant::now());

        Ok(Self {
            client: Arc::new(ArcSwap::from_pointee(client)),
            proxy_fingerprint: Arc::new(AtomicU64::new(proxy_state.fingerprint)),
            proxy_has_proxy: Arc::new(AtomicBool::new(proxy_state.has_proxy)),
            created_at,
            last_proxy_check_elapsed_ms: Arc::new(AtomicU64::new(0)),
            token,
        })
    }

    fn elapsed_millis_since_creation(&self) -> u64 {
        let millis = self.created_at.elapsed().as_millis();
        if millis > u64::MAX as u128 {
            u64::MAX
        } else {
            millis as u64
        }
    }

    fn apply_proxy_state_if_changed(&self, latest_proxy_state: ProxyState) -> bool {
        use crate::logger::{log, LogCategory, LogLevel};

        let previous_fingerprint = self.proxy_fingerprint.load(Ordering::Acquire);
        let previous_has_proxy = self.proxy_has_proxy.load(Ordering::Acquire);

        if latest_proxy_state.fingerprint == previous_fingerprint
            && latest_proxy_state.has_proxy == previous_has_proxy
        {
            return false;
        }

        let details = format!(
            "fingerprint={} -> {}, has_proxy={} -> {}",
            previous_fingerprint,
            latest_proxy_state.fingerprint,
            previous_has_proxy,
            latest_proxy_state.has_proxy
        );

        log(
            LogLevel::Info,
            LogCategory::Api,
            "System proxy state changed, rebuilding HTTP client",
            Some(&details),
        );

        match Self::build_http_client(&self.token) {
            Ok(client) => {
                self.client.store(Arc::new(client));
                self.proxy_fingerprint
                    .store(latest_proxy_state.fingerprint, Ordering::Release);
                self.proxy_has_proxy
                    .store(latest_proxy_state.has_proxy, Ordering::Release);
                true
            }
            Err(err) => {
                log(
                    LogLevel::Warn,
                    LogCategory::Api,
                    "Failed to rebuild HTTP client after proxy change; using previous client",
                    Some(&err.to_string()),
                );
                false
            }
        }
    }

    fn maybe_refresh_client_for_proxy_state_with<F>(&self, now_elapsed_ms: u64, probe: F) -> bool
    where
        F: FnOnce() -> ProxyState,
    {
        let last_check_ms = self.last_proxy_check_elapsed_ms.load(Ordering::Acquire);

        if now_elapsed_ms.saturating_sub(last_check_ms) < PROXY_STATE_CHECK_INTERVAL_MS {
            return false;
        }

        if self
            .last_proxy_check_elapsed_ms
            .compare_exchange(
                last_check_ms,
                now_elapsed_ms,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return false;
        }

        self.apply_proxy_state_if_changed(probe())
    }

    fn maybe_refresh_client_for_proxy_state(&self) {
        let now_elapsed_ms = self.elapsed_millis_since_creation();
        let _ = self.maybe_refresh_client_for_proxy_state_with(now_elapsed_ms, ProxyState::current);
    }

    fn current_client(&self) -> reqwest::Client {
        self.maybe_refresh_client_for_proxy_state();
        self.client.load_full().as_ref().clone()
    }

    fn header_value(value: &str, name: &str) -> Option<HeaderValue> {
        use crate::logger::{log, LogCategory, LogLevel};
        match HeaderValue::from_str(value) {
            Ok(header) => Some(header),
            Err(err) => {
                log(
                    LogLevel::Warn,
                    LogCategory::Api,
                    &format!("Skipping invalid {} header", name),
                    Some(&err.to_string()),
                );
                None
            }
        }
    }

    /// Get the current X-Super-Properties value (dynamically obtained to ensure latest data)
    fn get_super_properties_header(&self) -> HeaderValue {
        let super_props = {
            let manager = crate::SUPER_PROPERTIES_MANAGER
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            manager.get_super_properties_base64()
        };

        // Log the generated properties for audit purposes
        #[cfg(debug_assertions)]
        {
            use crate::logger::{log, LogCategory, LogLevel};
            // Decode to verify content
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&super_props) {
                if let Ok(s) = String::from_utf8(decoded) {
                    // Truncate for logging if too long, but show enough to verify structure
                    let preview = if s.len() > 100 {
                        format!("{}...", &s[..100])
                    } else {
                        s
                    };
                    log(
                        LogLevel::Debug,
                        LogCategory::Api,
                        &format!("Injecting X-Super-Properties: {}", preview),
                        None,
                    );
                }
            }
        }

        HeaderValue::from_str(&super_props).unwrap_or_else(|e| {
            eprintln!("Failed to create X-Super-Properties header: {}", e);
            // Fallback to minimal valid base64 JSON
            HeaderValue::from_static("e30=") // base64("{}")
        })
    }

    fn quest_referer_for_url(url: &str) -> Option<&'static str> {
        let parsed = reqwest::Url::parse(url).ok()?;
        let path = parsed.path();

        if path.starts_with("/api/v9/quests")
            || path == "/api/v9/users/@me/virtual-currency/balance"
        {
            Some(QUEST_HOME_REFERER)
        } else {
            None
        }
    }

    /// Centralized request builder to enforce security headers
    fn request(&self, method: Method, url: &str) -> RequestBuilder {
        let (user_agent, header_profile) = {
            let manager = crate::SUPER_PROPERTIES_MANAGER
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                manager.get_user_agent_string(),
                manager.get_header_profile(),
            )
        };

        let mut request = self
            .current_client()
            .request(method, url)
            .header("x-super-properties", self.get_super_properties_header());

        if let Some(value) = Self::header_value(&user_agent, "User-Agent") {
            request = request.header(USER_AGENT, value);
        }
        if let Some(value) = Self::header_value(&header_profile.timezone, "x-discord-timezone") {
            request = request.header("x-discord-timezone", value);
        }
        if let Some(value) = Self::header_value(&header_profile.locale, "x-discord-locale") {
            request = request.header("x-discord-locale", value);
        }
        if let Some(value) = Self::header_value(&header_profile.accept_language, "accept-language")
        {
            request = request.header("accept-language", value);
        }
        if let Some(installation_id) = header_profile.installation_id.as_deref() {
            if let Some(value) = Self::header_value(installation_id, "x-installation-id") {
                request = request.header("x-installation-id", value);
            }
        }
        if let Some(referer) = Self::quest_referer_for_url(url) {
            request = request.header(REFERER, HeaderValue::from_static(referer));
        }

        request
    }

    #[allow(dead_code)]
    pub fn get_token(&self) -> &str {
        &self.token
    }

    /// Get current user info
    pub async fn get_current_user(&self) -> Result<DiscordUser> {
        use crate::logger::{log, LogCategory, LogLevel};

        let url = format!("{}/users/@me", DISCORD_API_BASE);
        log(
            LogLevel::Debug,
            LogCategory::Api,
            "Requesting current user info",
            Some(&url),
        );

        let response = self.request(Method::GET, &url).send().await.map_err(|e| {
            log(
                LogLevel::Error,
                LogCategory::Api,
                "Network request failed for /users/@me",
                Some(&e.to_string()),
            );
            anyhow::anyhow!("Request for current user info failed: {}", e)
        })?;

        let status = response.status();
        log(
            LogLevel::Debug,
            LogCategory::Api,
            &format!("Response status for /users/@me: {}", status),
            None,
        );

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            // Use chars().take() for safe UTF-8 truncation
            let truncated_body: String = body.chars().take(200).collect();
            log(
                LogLevel::Error,
                LogCategory::Api,
                &format!("API error for /users/@me: {} - {}", status, truncated_body),
                None,
            );
            anyhow::bail!("Failed to get user info: {} - {}", status, body);
        }

        let user: DiscordUser = response.json().await.context("Failed to parse user info")?;

        log(
            LogLevel::Debug,
            LogCategory::Api,
            "Successfully retrieved user info",
            None,
        );

        Ok(user)
    }

    /// Get progress for a specific quest.
    ///
    /// Returns `(progress_seconds, completed)` by fetching the full quest list
    /// and extracting the relevant quest's user_status.
    pub async fn get_quest_progress(&self, quest_id: &str) -> Result<(f64, bool)> {
        let data = self.get_quests_raw().await?;
        let quests = data
            .get("quests")
            .and_then(|q| q.as_array())
            .ok_or_else(|| anyhow::anyhow!("Quest list missing 'quests' array"))?;

        let quest = quests
            .iter()
            .find(|q| q.get("id").and_then(|id| id.as_str()) == Some(quest_id))
            .ok_or_else(|| anyhow::anyhow!("Quest {} not found in quest list", quest_id))?;

        let user_status = quest.get("user_status");
        let completed = user_status
            .and_then(|us| us.get("completed_at"))
            .map(|v| !v.is_null())
            .unwrap_or(false);

        let mut progress_seconds = 0.0f64;
        if let Some(progress) = user_status
            .and_then(|us| us.get("progress"))
            .and_then(|p| p.as_object())
        {
            // progress is {"TASK_KEY": {"value": N}, ...}
            if let Some(first) = progress.values().next() {
                if let Some(val) = first.get("value").and_then(|v| v.as_f64()) {
                    progress_seconds = val;
                }
            }
        } else if let Some(sps) = user_status
            .and_then(|us| us.get("stream_progress_seconds"))
            .and_then(|v| v.as_f64())
        {
            progress_seconds = sps;
        }

        Ok((progress_seconds, completed))
    }

    /// Get raw quest list data (via /quests/@me endpoint)
    pub async fn get_quests_raw(&self) -> Result<serde_json::Value> {
        let url = format!("{}/quests/@me", DISCORD_API_BASE);

        println!("Requesting quest list: {}", url);

        let response = self
            .request(Method::GET, &url)
            .send()
            .await
            .context("Request for quest list failed")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        println!(
            "Quest list response: {} - received {} bytes",
            status,
            body.len()
        );

        if !status.is_success() {
            anyhow::bail!("Failed to get quest list: {} - {}", status, body);
        }

        let data: serde_json::Value =
            serde_json::from_str(&body).context("Failed to parse quest list")?;

        // Print quest count if available
        if let Some(quests) = data.get("quests").and_then(|q| q.as_array()) {
            println!("Successfully retrieved {} quests", quests.len());
        }

        Ok(data)
    }

    pub async fn get_quest_decision_debug(&self, placement: u64) -> Result<serde_json::Value> {
        let (heartbeat_session_id, ad_session_id) = {
            let manager = crate::SUPER_PROPERTIES_MANAGER
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                manager.client_heartbeat_session_id(),
                manager.client_ad_session_id(),
            )
        };

        let mut url = reqwest::Url::parse(&format!("{}/quests/decision", DISCORD_API_BASE))?;
        url.query_pairs_mut()
            .append_pair("placement", &placement.to_string())
            .append_pair("client_heartbeat_session_id", &heartbeat_session_id)
            .append_pair("client_ad_session_id", &ad_session_id);

        let response = self
            .request(Method::GET, url.as_str())
            .send()
            .await
            .context("Request for quest placement decision failed")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "Failed to get quest placement decision: {} - {}",
                status,
                body
            );
        }

        serde_json::from_str(&body).context("Failed to parse quest placement decision")
    }

    pub async fn get_quest_decisions_debug(
        &self,
        placement: u64,
        num: u64,
    ) -> Result<serde_json::Value> {
        let (heartbeat_session_id, ad_session_id) = {
            let manager = crate::SUPER_PROPERTIES_MANAGER
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                manager.client_heartbeat_session_id(),
                manager.client_ad_session_id(),
            )
        };

        let mut url = reqwest::Url::parse(&format!("{}/quests/get-decisions", DISCORD_API_BASE))?;
        url.query_pairs_mut()
            .append_pair("placement", &placement.to_string())
            .append_pair("num_decisions_requested", &num.to_string())
            .append_pair("client_heartbeat_session_id", &heartbeat_session_id)
            .append_pair("client_ad_session_id", &ad_session_id);

        let response = self
            .request(Method::GET, url.as_str())
            .send()
            .await
            .context("Request for quest placement decisions failed")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "Failed to get quest placement decisions: {} - {}",
                status,
                body
            );
        }

        serde_json::from_str(&body).context("Failed to parse quest placement decisions")
    }

    pub async fn get_virtual_currency_balance(&self) -> Result<serde_json::Value> {
        let url = format!("{}/users/@me/virtual-currency/balance", DISCORD_API_BASE);

        let response = self
            .request(Method::GET, &url)
            .send()
            .await
            .context("Request for virtual currency balance failed")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "Failed to get virtual currency balance: {} - {}",
                status,
                body
            );
        }

        serde_json::from_str(&body).context("Failed to parse virtual currency balance")
    }

    pub async fn claim_quest_reward(
        &self,
        quest_id: &str,
        platform: Option<String>,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/quests/{}/claim-reward", DISCORD_API_BASE, quest_id);
        let payload = match platform {
            Some(platform) if !platform.trim().is_empty() => {
                serde_json::json!({ "platform": platform })
            }
            _ => serde_json::json!({}),
        };

        let response = self
            .request(Method::POST, &url)
            .json(&payload)
            .send()
            .await
            .context("Request to claim quest reward failed")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Failed to claim quest reward: {} - {}", status, body);
        }

        serde_json::from_str(&body).context("Failed to parse claim reward response")
    }

    /// Update video watch progress
    pub async fn update_video_progress(&self, quest_id: &str, timestamp: f64) -> Result<bool> {
        let url = format!("{}/quests/{}/video-progress", DISCORD_API_BASE, quest_id);

        let payload = VideoProgressPayload {
            timestamp: Self::normalize_video_timestamp(timestamp),
        };

        println!(
            "Sending video progress: quest_id={}, timestamp={}",
            quest_id, payload.timestamp
        );

        let response = self
            .request(Method::POST, &url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send video progress")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to update video progress: {} - {}", status, body);
        }

        // Check if quest is completed from response
        let body: serde_json::Value = response.json().await.unwrap_or_default();
        let completed = body
            .get("completed_at")
            .map(|v| !v.is_null())
            .unwrap_or(false);

        Ok(completed)
    }

    /// Send stream heartbeat
    pub async fn send_stream_heartbeat(&self, quest_id: &str, stream_key: &str) -> Result<()> {
        let url = format!("{}/quests/{}/heartbeat", DISCORD_API_BASE, quest_id);

        let payload = HeartbeatPayload {
            stream_key: stream_key.to_string(),
        };

        let response = self
            .request(Method::POST, &url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send heartbeat")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to send heartbeat: {} - {}", status, body);
        }

        Ok(())
    }

    /// Send game heartbeat (for PLAY_ON_DESKTOP quests without running actual game)
    pub async fn send_game_heartbeat(
        &self,
        quest_id: &str,
        application_id: &str,
        terminal: bool,
    ) -> Result<bool> {
        let url = format!("{}/quests/{}/heartbeat", DISCORD_API_BASE, quest_id);

        let payload = GameHeartbeatPayload {
            application_id: application_id.to_string(),
            terminal,
        };

        println!(
            "Sending game heartbeat: quest_id={}, app_id={}, terminal={}",
            quest_id, application_id, terminal
        );

        let response = self
            .request(Method::POST, &url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send game heartbeat")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to send game heartbeat: {} - {}", status, body);
        }

        // Check if quest is completed from response
        let body: serde_json::Value = response.json().await.unwrap_or_default();
        let completed = body
            .get("completed_at")
            .map(|v| !v.is_null())
            .unwrap_or(false);

        Ok(completed)
    }

    /// Accept quest (enroll in quest)
    pub async fn accept_quest(&self, quest_id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/quests/{}/enroll", DISCORD_API_BASE, quest_id);

        println!("Accepting quest: quest_id={}", quest_id);

        // POST with enrollment payload from HAR capture
        let payload = serde_json::json!({
            "location": 11,
            "is_targeted": false,
            "metadata_raw": null
        });

        let response = self
            .request(Method::POST, &url)
            .json(&payload)
            .send()
            .await
            .context("Failed to accept quest")?;

        if response.status().is_success() {
            let body: serde_json::Value = response.json().await.unwrap_or_default();
            println!("Quest accepted successfully: {:?}", body);
            return Ok(body);
        }

        let first_status = response.status();
        let first_body = response.text().await.unwrap_or_default();

        let minimal_payload = serde_json::json!({ "location": 11 });
        let fallback_response = self
            .request(Method::POST, &url)
            .json(&minimal_payload)
            .send()
            .await
            .context("Failed to accept quest with minimal payload")?;

        if fallback_response.status().is_success() {
            let body: serde_json::Value = fallback_response.json().await.unwrap_or_default();
            println!(
                "Quest accepted successfully with minimal payload: {:?}",
                body
            );
            return Ok(body);
        }

        let fallback_status = fallback_response.status();
        let fallback_body = fallback_response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Failed to accept quest. Compatibility payload failed: {} - {}. Minimal payload failed: {} - {}",
            first_status,
            first_body,
            fallback_status,
            fallback_body
        );
    }

    /// Get detectable games list
    /// Get detectable games list (merges games and non-games)
    pub async fn fetch_detectable_games(&self) -> Result<Vec<DetectableGame>> {
        let games_url = format!("{}/applications/detectable", DISCORD_API_BASE);
        let apps_url = format!("{}/applications/non-games/detectable", DISCORD_API_BASE);

        println!("Requesting detectable games and apps lists...");

        // Helper to fetch a single URL
        let fetch_list = |url: String| async move {
            println!("Requesting: {}", url);
            let response = self
                .request(Method::GET, &url)
                .send()
                .await
                .context(format!("Failed to request {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                // Don't fail the whole process if one list fails, just return empty?
                // For now, let's log error and return empty vector to be robust
                println!("Failed to fetch list from {}: {} - {}", url, status, body);
                return Ok(Vec::<DetectableGame>::new());
            }

            let list: Vec<DetectableGame> = response
                .json()
                .await
                .context(format!("Failed to parse list from {}", url))?;

            Ok::<Vec<DetectableGame>, anyhow::Error>(list)
        };

        // Fetch both concurrently
        let (games_res, apps_res) = tokio::join!(fetch_list(games_url), fetch_list(apps_url));

        let mut all_items = Vec::new();

        match games_res {
            Ok(mut games) => {
                println!("Retrieved {} games", games.len());
                for game in &mut games {
                    game.type_name = Some("Game".to_string());
                }
                all_items.extend(games);
            }
            Err(e) => println!("Error fetching games: {}", e),
        }

        match apps_res {
            Ok(mut apps) => {
                println!("Retrieved {} non-game apps", apps.len());
                for app in &mut apps {
                    app.type_name = Some("App".to_string());
                }
                all_items.extend(apps);
            }
            Err(e) => println!("Error fetching apps: {}", e),
        }

        println!("Total detectable items merged: {}", all_items.len());

        Ok(all_items)
    }
}

#[allow(dead_code)]
fn convert_api_quest_to_quest(quest_json: &serde_json::Value) -> Option<Quest> {
    let id = quest_json.get("id")?.as_str()?.to_string();
    let config = quest_json.get("config")?;
    let messages = config.get("messages");
    let application = config.get("application");
    let user_status = quest_json.get("user_status");

    // Get quest name
    let name = messages
        .and_then(|m| m.get("quest_name"))
        .and_then(|n| n.as_str())
        .unwrap_or("Unknown Quest")
        .to_string();

    // Get task config and extract task info
    let task_config = config
        .get("task_config_v2")
        .or_else(|| config.get("task_config"));
    let (seconds_needed, task_type) = task_config
        .and_then(|tc| tc.get("tasks"))
        .and_then(|tasks| tasks.as_object())
        .map(|tasks| {
            for (task_name, task_data) in tasks {
                if let Some(target) = task_data.get("target").and_then(|t| t.as_u64()) {
                    return (target as u32, task_name.clone());
                }
            }
            (0u32, String::new())
        })
        .unwrap_or((0, String::new()));

    // Calculate progress
    let progress = user_status
        .and_then(|us| us.get("progress"))
        .and_then(|p| p.as_object())
        .map(|progress_map| {
            for (_, v) in progress_map {
                if let Some(val) = v.get("value").and_then(|v| v.as_f64()) {
                    return if seconds_needed > 0 {
                        (val / seconds_needed as f64 * 100.0).min(100.0)
                    } else {
                        0.0
                    };
                }
            }
            0.0
        })
        .unwrap_or(0.0);

    Some(Quest {
        id,
        name,
        description: messages
            .and_then(|m| m.get("game_publisher"))
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string(),
        progress,
        seconds_needed,
        task_type,
        application_id: application
            .and_then(|a| a.get("id"))
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string(),
        application_name: application
            .and_then(|a| a.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string(),
        application_icon: None, // Icon handling would require additional logic
        expires_at: config
            .get("expires_at")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string()),
        enrolled: user_status
            .and_then(|us| us.get("enrolled_at"))
            .map(|e| !e.is_null())
            .unwrap_or(false),
        completed: user_status
            .and_then(|us| us.get("completed_at"))
            .map(|c| !c.is_null())
            .unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    const PROXY_ENV_KEYS: [&str; 8] = [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
    ];

    fn env_snapshot() -> Vec<(String, Option<String>)> {
        PROXY_ENV_KEYS
            .iter()
            .map(|key| ((*key).to_string(), std::env::var(key).ok()))
            .collect()
    }

    fn restore_env(snapshot: &[(String, Option<String>)]) {
        for (key, value) in snapshot {
            match value {
                Some(v) => {
                    unsafe { std::env::set_var(key, v) };
                }
                None => {
                    unsafe { std::env::remove_var(key) };
                }
            }
        }
    }

    #[test]
    fn proxy_state_fingerprint_changes_when_env_changes() {
        let _guard = ENV_TEST_LOCK.lock().unwrap();
        let snapshot = env_snapshot();

        unsafe { std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7890") };
        let state_a = ProxyState::current();

        unsafe { std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7891") };
        let state_b = ProxyState::current();

        restore_env(&snapshot);

        assert_ne!(state_a.fingerprint, state_b.fingerprint);
    }

    #[test]
    fn proxy_refresh_respects_interval_and_rebuilds_on_change() {
        let client = DiscordApiClient::new("test-token".to_string()).unwrap();

        client
            .last_proxy_check_elapsed_ms
            .store(0, Ordering::Release);

        let original_fingerprint = client.proxy_fingerprint.load(Ordering::Acquire);
        let changed_state = ProxyState {
            fingerprint: original_fingerprint.wrapping_add(1),
            has_proxy: !client.proxy_has_proxy.load(Ordering::Acquire),
        };

        let before_interval_refresh = client.maybe_refresh_client_for_proxy_state_with(
            PROXY_STATE_CHECK_INTERVAL_MS.saturating_sub(1),
            || changed_state,
        );
        assert!(!before_interval_refresh);
        assert_eq!(
            client.proxy_fingerprint.load(Ordering::Acquire),
            original_fingerprint
        );

        let after_interval_refresh = client
            .maybe_refresh_client_for_proxy_state_with(PROXY_STATE_CHECK_INTERVAL_MS, || {
                changed_state
            });
        assert!(after_interval_refresh);
        assert_eq!(
            client.proxy_fingerprint.load(Ordering::Acquire),
            changed_state.fingerprint
        );
    }

    #[test]
    fn video_timestamp_is_normalized_to_integer_seconds() {
        assert_eq!(DiscordApiClient::normalize_video_timestamp(12.49), 12);
        assert_eq!(DiscordApiClient::normalize_video_timestamp(12.5), 13);
        assert_eq!(DiscordApiClient::normalize_video_timestamp(-1.0), 0);
        assert_eq!(DiscordApiClient::normalize_video_timestamp(f64::NAN), 0);
    }

    #[test]
    fn quest_referer_is_only_added_for_quest_context_routes() {
        assert_eq!(
            DiscordApiClient::quest_referer_for_url("https://discord.com/api/v9/quests/@me"),
            Some(QUEST_HOME_REFERER)
        );
        assert_eq!(
            DiscordApiClient::quest_referer_for_url(
                "https://discord.com/api/v9/users/@me/virtual-currency/balance"
            ),
            Some(QUEST_HOME_REFERER)
        );
        assert_eq!(
            DiscordApiClient::quest_referer_for_url("https://discord.com/api/v9/users/@me"),
            None
        );
    }

    #[test]
    fn request_injects_user_agent_matching_x_super_properties() {
        let client = DiscordApiClient::new("test-token".to_string()).unwrap();
        let request = client
            .request(Method::GET, "https://discord.com/api/v9/quests/@me")
            .build()
            .unwrap();

        let headers = request.headers();
        let user_agent = headers.get(USER_AGENT).unwrap().to_str().unwrap();
        let xsp = headers.get("x-super-properties").unwrap().to_str().unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(xsp)
            .unwrap();
        let props: serde_json::Value = serde_json::from_slice(&decoded).unwrap();

        assert_eq!(
            user_agent,
            props
                .get("browser_user_agent")
                .and_then(|value| value.as_str())
                .unwrap()
        );
        assert!(headers.get(REFERER).is_some());
        assert!(headers.get("x-discord-timezone").is_some());
        assert!(headers.get("x-discord-locale").is_some());
        assert!(headers.get("accept-language").is_some());
    }

    #[tokio::test]
    #[ignore] // Requires valid token
    async fn test_get_current_user() {
        let token = "YOUR_TOKEN_HERE";
        let client = DiscordApiClient::new(token.to_string()).unwrap();
        let user = client.get_current_user().await.unwrap();
        println!("User: {:?}", user);
    }
}

use crate::models::*;
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::{Method, RequestBuilder};
use base64::Engine;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

const DISCORD_API_BASE: &str = "https://discord.com/api/v9";
const USER_AGENT_STRING: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/1.0.9219 Chrome/138.0.7204.251 Electron/37.6.0 Safari/537.36";
const PROXY_STATE_CHECK_INTERVAL_MS: u64 = 5_000;

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
    last_proxy_check_at_ms: Arc<AtomicU64>,
    token: String,
}

impl DiscordApiClient {
    fn build_default_headers(token: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(token).context("Invalid token format")?,
        );
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(USER_AGENT_STRING),
        );
        // Note: X-Super-Properties is no longer set here, but dynamically obtained on each request
        // This ensures the latest validation parameters (including data obtained from CDP) are used
        headers.insert(
            "x-discord-timezone",
            HeaderValue::from_static("America/Los_Angeles"),
        );
        headers.insert(
            "x-discord-locale",
            HeaderValue::from_static("en-US"),
        );
        headers.insert(
            "x-debug-options",
            HeaderValue::from_static("bugReporterEnabled"),
        );
        headers.insert(
            "accept",
            HeaderValue::from_static("*/*"),
        );

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

        Ok(Self {
            client: Arc::new(ArcSwap::from_pointee(client)),
            proxy_fingerprint: Arc::new(AtomicU64::new(proxy_state.fingerprint)),
            proxy_has_proxy: Arc::new(AtomicBool::new(proxy_state.has_proxy)),
            last_proxy_check_at_ms: Arc::new(AtomicU64::new(Self::now_millis())),
            token,
        })
    }

    fn now_millis() -> u64 {
        match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => {
                let millis = duration.as_millis();
                if millis > u64::MAX as u128 {
                    u64::MAX
                } else {
                    millis as u64
                }
            }
            Err(_) => 0,
        }
    }

    fn maybe_refresh_client_for_proxy_state(&self) {
        use crate::logger::{log, LogCategory, LogLevel};

        let now_ms = Self::now_millis();
        let last_check_ms = self.last_proxy_check_at_ms.load(Ordering::Acquire);

        if now_ms.saturating_sub(last_check_ms) < PROXY_STATE_CHECK_INTERVAL_MS {
            return;
        }

        if self
            .last_proxy_check_at_ms
            .compare_exchange(last_check_ms, now_ms, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let latest_proxy_state = ProxyState::current();
        let previous_fingerprint = self.proxy_fingerprint.load(Ordering::Acquire);
        let previous_has_proxy = self.proxy_has_proxy.load(Ordering::Acquire);

        if latest_proxy_state.fingerprint == previous_fingerprint
            && latest_proxy_state.has_proxy == previous_has_proxy
        {
            return;
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
            }
            Err(err) => {
                log(
                    LogLevel::Warn,
                    LogCategory::Api,
                    "Failed to rebuild HTTP client after proxy change; using previous client",
                    Some(&err.to_string()),
                );
            }
        }
    }

    fn current_client(&self) -> reqwest::Client {
        self.maybe_refresh_client_for_proxy_state();
        self.client.load_full().as_ref().clone()
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
           use crate::logger::{log, LogLevel, LogCategory};
           // Decode to verify content
           if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&super_props) {
               if let Ok(s) = String::from_utf8(decoded) {
                   // Truncate for logging if too long, but show enough to verify structure
                   let preview = if s.len() > 100 { format!("{}...", &s[..100]) } else { s };
                   log(LogLevel::Debug, LogCategory::Api, &format!("Injecting X-Super-Properties: {}", preview), None);
               }
           }
        }

        HeaderValue::from_str(&super_props).unwrap_or_else(|e| {
            eprintln!("Failed to create X-Super-Properties header: {}", e);
            // Fallback to minimal valid base64 JSON
            HeaderValue::from_static("e30=") // base64("{}")
        })
    }

    /// Centralized request builder to enforce security headers
    fn request(&self, method: Method, url: &str) -> RequestBuilder {
        self.current_client()
            .request(method, url)
            .header("x-super-properties", self.get_super_properties_header())
    }

    #[allow(dead_code)]
    pub fn get_token(&self) -> &str {
        &self.token
    }

    /// Get current user info
    pub async fn get_current_user(&self) -> Result<DiscordUser> {
        use crate::logger::{log, LogLevel, LogCategory};
        
        let url = format!("{}/users/@me", DISCORD_API_BASE);
        log(LogLevel::Debug, LogCategory::Api, "Requesting current user info", Some(&url));
        
        
        let response = self.request(Method::GET, &url)
            .send()
            .await
            .map_err(|e| {
                log(LogLevel::Error, LogCategory::Api, 
                    "Network request failed for /users/@me", Some(&e.to_string()));
                anyhow::anyhow!("Request for current user info failed: {}", e)
            })?;

        let status = response.status();
        log(LogLevel::Debug, LogCategory::Api, 
            &format!("Response status for /users/@me: {}", status), None);

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            // Use chars().take() for safe UTF-8 truncation
            let truncated_body: String = body.chars().take(200).collect();
            log(LogLevel::Error, LogCategory::Api, 
                &format!("API error for /users/@me: {} - {}", status, truncated_body), None);
            anyhow::bail!("Failed to get user info: {} - {}", status, body);
        }

        let user: DiscordUser = response
            .json()
            .await
            .context("Failed to parse user info")?;
        
        log(LogLevel::Debug, LogCategory::Api, 
            "Successfully retrieved user info", None);

        Ok(user)
    }

    /// Get progress for a specific quest.
    ///
    /// Returns `(progress_seconds, completed)` by fetching the full quest list
    /// and extracting the relevant quest's user_status.
    pub async fn get_quest_progress(&self, quest_id: &str) -> Result<(f64, bool)> {
        let data = self.get_quests_raw().await?;
        let quests = data.get("quests")
            .and_then(|q| q.as_array())
            .ok_or_else(|| anyhow::anyhow!("Quest list missing 'quests' array"))?;

        let quest = quests.iter()
            .find(|q| q.get("id").and_then(|id| id.as_str()) == Some(quest_id))
            .ok_or_else(|| anyhow::anyhow!("Quest {} not found in quest list", quest_id))?;

        let user_status = quest.get("user_status");
        let completed = user_status
            .and_then(|us| us.get("completed_at"))
            .map(|v| !v.is_null())
            .unwrap_or(false);

        let mut progress_seconds = 0.0f64;
        if let Some(progress) = user_status.and_then(|us| us.get("progress")).and_then(|p| p.as_object()) {
            // progress is {"TASK_KEY": {"value": N}, ...}
            if let Some(first) = progress.values().next() {
                if let Some(val) = first.get("value").and_then(|v| v.as_f64()) {
                    progress_seconds = val;
                }
            }
        } else if let Some(sps) = user_status.and_then(|us| us.get("stream_progress_seconds")).and_then(|v| v.as_f64()) {
            progress_seconds = sps;
        }

        Ok((progress_seconds, completed))
    }

    /// Get raw quest list data (via /quests/@me endpoint)
    pub async fn get_quests_raw(&self) -> Result<serde_json::Value> {
        let url = format!("{}/quests/@me", DISCORD_API_BASE);
        
        println!("Requesting quest list: {}", url);
        
        let response = self.request(Method::GET, &url)
            .send()
            .await
            .context("Request for quest list failed")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        
        println!("Quest list response: {} - received {} bytes", status, body.len());

        if !status.is_success() {
            anyhow::bail!("Failed to get quest list: {} - {}", status, body);
        }

        let data: serde_json::Value = serde_json::from_str(&body)
            .context("Failed to parse quest list")?;

        // Print quest count if available
        if let Some(quests) = data.get("quests").and_then(|q| q.as_array()) {
            println!("Successfully retrieved {} quests", quests.len());
        }

        Ok(data)
    }


    /// Update video watch progress
    pub async fn update_video_progress(
        &self,
        quest_id: &str,
        timestamp: f64,
    ) -> Result<bool> {
        let url = format!("{}/quests/{}/video-progress", DISCORD_API_BASE, quest_id);
        
        let payload = VideoProgressPayload {
            timestamp,
        };

        println!("Sending video progress: quest_id={}, timestamp={:.1}", quest_id, timestamp);

        let response = self.request(Method::POST, &url)
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
        let completed = body.get("completed_at").map(|v| !v.is_null()).unwrap_or(false);
        
        Ok(completed)
    }

    /// Send stream heartbeat
    pub async fn send_stream_heartbeat(
        &self,
        quest_id: &str,
        stream_key: &str,
    ) -> Result<()> {
        let url = format!("{}/quests/{}/heartbeat", DISCORD_API_BASE, quest_id);
        
        let payload = HeartbeatPayload {
            stream_key: stream_key.to_string(),
        };

        let response = self.request(Method::POST, &url)
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

        println!("Sending game heartbeat: quest_id={}, app_id={}, terminal={}", quest_id, application_id, terminal);

        let response = self.request(Method::POST, &url)
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
        let completed = body.get("completed_at").map(|v| !v.is_null()).unwrap_or(false);
        
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

        let response = self.request(Method::POST, &url)
            .json(&payload)
            .send()
            .await
            .context("Failed to accept quest")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to accept quest: {} - {}", status, body);
        }

        let body: serde_json::Value = response.json().await.unwrap_or_default();
        println!("Quest accepted successfully: {:?}", body);
        
        Ok(body)
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
            let response = self.request(Method::GET, &url)
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
        let (games_res, apps_res) = tokio::join!(
            fetch_list(games_url),
            fetch_list(apps_url)
        );

        let mut all_items = Vec::new();

        match games_res {
            Ok(mut games) => {
                println!("Retrieved {} games", games.len());
                for game in &mut games {
                    game.type_name = Some("Game".to_string());
                }
                all_items.extend(games);
            },
            Err(e) => println!("Error fetching games: {}", e),
        }

        match apps_res {
            Ok(mut apps) => {
                println!("Retrieved {} non-game apps", apps.len());
                for app in &mut apps {
                     app.type_name = Some("App".to_string());
                }
                all_items.extend(apps);
            },
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
    let task_config = config.get("task_config_v2").or_else(|| config.get("task_config"));
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

    #[tokio::test]
    #[ignore] // Requires valid token
    async fn test_get_current_user() {
        let token = "YOUR_TOKEN_HERE";
        let client = DiscordApiClient::new(token.to_string()).unwrap();
        let user = client.get_current_user().await.unwrap();
        println!("User: {:?}", user);
    }
}

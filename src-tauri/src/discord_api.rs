use crate::{models::*, network::HttpClient};
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, REFERER, USER_AGENT};
use reqwest::{Method, RequestBuilder};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DISCORD_API_BASE: &str = "https://discord.com/api/v9";
const QUEST_HOME_REFERER: &str = "https://discord.com/quest-home";

pub(crate) fn parse_play_activity_heartbeat_response(
    body: &serde_json::Value,
) -> Result<PlayActivityHeartbeatStatus> {
    let progress_seconds = body
        .get("progress")
        .and_then(|progress| progress.get("PLAY_ACTIVITY"))
        .and_then(|progress| progress.get("value"))
        .and_then(|value| value.as_f64())
        .ok_or_else(|| {
            anyhow::anyhow!("Heartbeat response missing progress.PLAY_ACTIVITY.value")
        })?;

    let completed = body
        .get("completed_at")
        .map(|value| !value.is_null())
        .unwrap_or(false)
        || body
            .get("progress")
            .and_then(|progress| progress.get("PLAY_ACTIVITY"))
            .and_then(|progress| progress.get("completed_at"))
            .map(|value| !value.is_null())
            .unwrap_or(false);

    Ok(PlayActivityHeartbeatStatus {
        progress_seconds,
        completed,
    })
}

/// Whether a request failed before Discord answered, so its connection is suspect.
fn is_connection_error(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout() || error.is_request()
}

/// Execute `request`; after a connection error, ask `reconnect` for a fresh
/// client and retry GET requests once. Other methods are not retried, since
/// Discord may already have processed them.
async fn execute_reconnecting<F>(
    client: reqwest::Client,
    request: reqwest::Request,
    reconnect: F,
) -> reqwest::Result<reqwest::Response>
where
    F: FnOnce(&reqwest::Error) -> Option<reqwest::Client>,
{
    let retry = if request.method() == Method::GET {
        request.try_clone()
    } else {
        None
    };

    match client.execute(request).await {
        Err(error) if is_connection_error(&error) => {
            let client = reconnect(&error);
            match (client, retry) {
                (Some(client), Some(request)) => client.execute(request).await,
                _ => Err(error),
            }
        }
        result => result,
    }
}

trait SendReconnecting {
    /// Send through the API client, reconnecting after a connection error.
    async fn send_reconnecting(self, api: &DiscordApiClient) -> reqwest::Result<reqwest::Response>;
}

impl SendReconnecting for RequestBuilder {
    async fn send_reconnecting(self, api: &DiscordApiClient) -> reqwest::Result<reqwest::Response> {
        let (client, request) = self.build_split();
        execute_reconnecting(client, request?, |error| api.reset_connections(error)).await
    }
}

/// Discord API client
const ENRICHMENT_POSITIVE_CACHE_TTL: Duration = Duration::from_secs(600);
const ENRICHMENT_NEGATIVE_CACHE_TTL: Duration = Duration::from_secs(60);
const ENRICHMENT_LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);

type CachedAppCatalogEntry = (
    Option<(String, Vec<crate::cdp_game_spoof::ApplicationExecutable>)>,
    Instant,
);

/// HTTP API client for Discord quest and user endpoints.
#[derive(Clone)]
pub struct DiscordApiClient {
    client: Arc<HttpClient>,
    headers: HeaderMap,
    created_at: Arc<Instant>,
    token: String,
    identity: Arc<std::sync::Mutex<crate::super_properties::XSuperPropertiesManager>>,
    identity_key: String,
    identity_committed: Arc<AtomicBool>,
    cached_user_id: Arc<ArcSwap<Option<String>>>,
    last_user_id_lookup_failure_ms: Arc<AtomicU64>,
    cached_app_catalog:
        Arc<std::sync::Mutex<std::collections::HashMap<String, CachedAppCatalogEntry>>>,
}

impl DiscordApiClient {
    /// Extract the numeric Discord user snowflake encoded in the first segment of a user token.
    pub(crate) fn extract_user_id_from_token(token: &str) -> Option<String> {
        use base64::{
            engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
            Engine as _,
        };

        let raw = token
            .trim()
            .strip_prefix("Bot ")
            .or_else(|| token.trim().strip_prefix("Bearer "))
            .unwrap_or(token.trim());
        let first_segment = raw.split('.').next()?.trim();
        if first_segment.is_empty() {
            return None;
        }
        let decoded = URL_SAFE_NO_PAD
            .decode(first_segment)
            .or_else(|_| STANDARD_NO_PAD.decode(first_segment))
            .or_else(|_| URL_SAFE.decode(first_segment))
            .or_else(|_| STANDARD.decode(first_segment))
            .ok()?;
        let user_id = String::from_utf8(decoded).ok()?;
        let trimmed = user_id.trim();
        if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
            Some(trimmed.to_string())
        } else {
            None
        }
    }

    /// Normalize a floating-point video progress timestamp into a non-negative integer second count.
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

    fn configure_http(&self, builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
        // Keep-alive pings retire connections whose network route disappeared.
        builder
            .default_headers(self.headers.clone())
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .pool_idle_timeout(Duration::from_secs(30))
            .http2_keep_alive_interval(Duration::from_secs(10))
            .http2_keep_alive_timeout(Duration::from_secs(5))
            .http2_keep_alive_while_idle(true)
    }

    /// Create a new API client
    pub fn new(token: String) -> Result<Self> {
        let headers = Self::build_default_headers(&token)?;
        let created_at = Arc::new(Instant::now());
        let initial_user_id = Self::extract_user_id_from_token(&token);

        use sha2::{Digest, Sha256};
        let credential = format!("{:x}", Sha256::digest(token.as_bytes()));
        let identity = crate::SUPER_PROPERTIES_MANAGER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .candidate(credential);
        let identity_key = identity.session_key.clone();
        Ok(Self {
            identity: Arc::new(std::sync::Mutex::new(identity)),
            identity_key,
            identity_committed: Arc::new(AtomicBool::new(false)),
            client: Arc::new(HttpClient::default()),
            headers,
            created_at,
            token,
            cached_user_id: Arc::new(ArcSwap::from_pointee(initial_user_id)),
            last_user_id_lookup_failure_ms: Arc::new(AtomicU64::new(0)),
            cached_app_catalog: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// Return the number of milliseconds elapsed since this client instance was created.
    fn elapsed_millis_since_creation(&self) -> u64 {
        let millis = self.created_at.elapsed().as_millis();
        if millis > u64::MAX as u128 {
            u64::MAX
        } else {
            millis as u64
        }
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

    pub fn candidate_identity(
        &self,
    ) -> Arc<std::sync::Mutex<crate::super_properties::XSuperPropertiesManager>> {
        Arc::clone(&self.identity)
    }

    fn identity_manager(
        &self,
    ) -> Arc<std::sync::Mutex<crate::super_properties::XSuperPropertiesManager>> {
        if self.identity_committed.load(Ordering::Acquire) {
            Arc::clone(&crate::SUPER_PROPERTIES_MANAGER)
        } else {
            Arc::clone(&self.identity)
        }
    }

    pub fn commit_identity(&self) {
        let mut candidate = self
            .identity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut current = crate::SUPER_PROPERTIES_MANAGER
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        candidate.commit(&current);
        *current = candidate;
        self.identity_committed.store(true, Ordering::Release);
    }

    async fn prepare_identity(&self) -> Result<crate::super_properties::RequestIdentity> {
        let manager = self.identity_manager();
        crate::super_properties::cdp::prepare_for(&manager, &self.identity_key)
            .await
            .context("Authenticated session changed while preparing request")
    }

    fn quest_referer_for_url(url: &str) -> Option<&'static str> {
        let parsed = reqwest::Url::parse(url).ok()?;
        let path = parsed.path();

        if path.starts_with("/api/v9/quests")
            || path == "/api/v9/users/@me/virtual-currency/balance"
            || path == "/api/v9/users/@me/virtual-currency/total-redeemed"
            || path.starts_with("/api/v9/users/@me/billing/subscriptions")
            || path == "/api/v9/users/@me/program-rewards"
        {
            Some(QUEST_HOME_REFERER)
        } else {
            None
        }
    }

    /// Resolve dynamic state once, then use the same snapshot throughout a request.
    async fn request(&self, method: Method, url: &str) -> Result<RequestBuilder> {
        let identity = self.prepare_identity().await?;
        self.request_with_identity(method, url, &identity)
    }

    fn request_with_identity(
        &self,
        method: Method,
        url: &str,
        identity: &crate::super_properties::RequestIdentity,
    ) -> Result<RequestBuilder> {
        debug_assert!(identity.properties["browser_user_agent"]
            .as_str()
            .is_none_or(|ua| ua == identity.user_agent));
        let user_agent = &identity.user_agent;
        let header_profile = &identity.header_profile;
        let mut request = self
            .client
            .get(|builder| self.configure_http(builder))
            .map_err(|code| anyhow::anyhow!("Could not create Discord HTTP client ({code})"))?
            .request(method, url)
            .header("x-super-properties", &identity.base64);

        if let Some(value) = Self::header_value(user_agent, "User-Agent") {
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

        Ok(request)
    }

    /// Replace the HTTP client so no pooled connection from a previous network
    /// route is reused, returning the client to use from now on.
    fn reset_connections(&self, reason: &reqwest::Error) -> Option<reqwest::Client> {
        use crate::logger::{log, LogCategory, LogLevel};

        let result = self.client.reconnect(|builder| self.configure_http(builder));
        let message = if result.is_ok() {
            "Connection error, replaced Discord HTTP client"
        } else {
            "Connection error, could not replace Discord HTTP client"
        };
        log(LogLevel::Warn, LogCategory::Api, message, Some(&Self::error_chain(reason)));
        result.ok()
    }

    /// Join an error with its sources, like anyhow's `{:#}`.
    fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
        let mut message = error.to_string();
        let mut source = error.source();
        while let Some(cause) = source {
            message.push_str(": ");
            message.push_str(&cause.to_string());
            source = cause.source();
        }
        message
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

        let response = self
            .request(Method::GET, &url)
            .await?
            .send_reconnecting(self)
            .await
            .map_err(|e| {
                log(
                    LogLevel::Error,
                    LogCategory::Api,
                    "Network request failed for /users/@me",
                    Some(&Self::error_chain(&e)),
                );
                anyhow::Error::from(e).context("Request for current user info failed")
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
        if !user.id.trim().is_empty() {
            self.cached_user_id
                .store(Arc::new(Some(user.id.trim().to_string())));
        }

        log(
            LogLevel::Debug,
            LogCategory::Api,
            "Successfully retrieved user info",
            None,
        );

        Ok(user)
    }

    /// Resolve the authenticated Discord account's user ID from the cached token prefix or `/users/@me`.
    async fn resolve_user_id(&self) -> Option<String> {
        use crate::logger::{log, LogCategory, LogLevel};

        if let Some(existing) = self.cached_user_id.load().as_ref().clone() {
            if !existing.is_empty() {
                return Some(existing);
            }
        }
        let now_ms = self.elapsed_millis_since_creation();
        let last_fail_ms = self.last_user_id_lookup_failure_ms.load(Ordering::Acquire);
        if last_fail_ms > 0
            && now_ms.saturating_sub(last_fail_ms)
                < ENRICHMENT_NEGATIVE_CACHE_TTL.as_millis() as u64
        {
            return None;
        }
        let url = format!("{}/users/@me", DISCORD_API_BASE);
        match tokio::time::timeout(ENRICHMENT_LOOKUP_TIMEOUT, async {
            self.request(Method::GET, &url)
                .await?
                .send()
                .await
                .map_err(anyhow::Error::from)
        })
        .await
        {
            Ok(Ok(response)) if response.status().is_success() => {
                match tokio::time::timeout(
                    ENRICHMENT_LOOKUP_TIMEOUT,
                    response.json::<DiscordUser>(),
                )
                .await
                {
                    Ok(Ok(user)) => {
                        let trimmed = user.id.trim();
                        if !trimmed.is_empty() {
                            let resolved = trimmed.to_string();
                            self.cached_user_id.store(Arc::new(Some(resolved.clone())));
                            self.last_user_id_lookup_failure_ms
                                .store(0, Ordering::Release);
                            return Some(resolved);
                        }
                        self.last_user_id_lookup_failure_ms
                            .store(now_ms.max(1), Ordering::Release);
                    }
                    Ok(Err(err)) => {
                        log(
                            LogLevel::Warn,
                            LogCategory::Api,
                            "Failed to parse /users/@me response during heartbeat enrichment",
                            Some(&err.to_string()),
                        );
                    }
                    Err(_) => {
                        log(
                            LogLevel::Warn,
                            LogCategory::Api,
                            "Timed out reading /users/@me body during heartbeat enrichment",
                            None,
                        );
                        self.last_user_id_lookup_failure_ms
                            .store(now_ms.max(1), Ordering::Release);
                    }
                }
            }
            Ok(Ok(response)) => {
                let status = response.status();
                log(
                    LogLevel::Warn,
                    LogCategory::Api,
                    "Non-success status from /users/@me during heartbeat enrichment",
                    Some(&status.to_string()),
                );
                if status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS {
                    self.last_user_id_lookup_failure_ms
                        .store(now_ms.max(1), Ordering::Release);
                }
            }
            Ok(Err(err)) => {
                log(
                    LogLevel::Warn,
                    LogCategory::Api,
                    "Network error querying /users/@me during heartbeat enrichment",
                    Some(&err.to_string()),
                );
            }
            Err(_) => {
                log(
                    LogLevel::Warn,
                    LogCategory::Api,
                    "Timed out querying /users/@me during heartbeat enrichment",
                    None,
                );
                self.last_user_id_lookup_failure_ms
                    .store(now_ms.max(1), Ordering::Release);
            }
        }
        None
    }

    /// Resolve the host-OS executable path and PE64 bitness for `application_id` from `/applications/public` and live process hints.
    async fn resolve_game_exe_path(&self, application_id: &str) -> Option<(String, bool)> {
        use crate::logger::{log, LogCategory, LogLevel};

        let hints = crate::game_simulator::simulated_process_hints();
        let host_os = crate::cdp_game_spoof::DetectableOs::from_host();
        let now = Instant::now();
        if let Ok(cache) = self.cached_app_catalog.lock() {
            if let Some((cached, fetched_at)) = cache.get(application_id) {
                let ttl = if cached.is_some() {
                    ENRICHMENT_POSITIVE_CACHE_TTL
                } else {
                    ENRICHMENT_NEGATIVE_CACHE_TTL
                };
                if now.duration_since(*fetched_at) < ttl {
                    return cached.as_ref().and_then(|(name, executables)| {
                        crate::cdp_game_spoof::resolve_application_exe_metadata(
                            host_os,
                            name,
                            executables,
                            &hints,
                        )
                    });
                }
            }
        }

        let url = format!(
            "{}/applications/public?application_ids={}",
            DISCORD_API_BASE, application_id
        );
        match tokio::time::timeout(ENRICHMENT_LOOKUP_TIMEOUT, async {
            self.request(Method::GET, &url)
                .await?
                .send()
                .await
                .map_err(anyhow::Error::from)
        })
        .await
        {
            Ok(Ok(response)) if response.status().is_success() => {
                match tokio::time::timeout(
                    ENRICHMENT_LOOKUP_TIMEOUT,
                    response.json::<Vec<serde_json::Value>>(),
                )
                .await
                {
                    Ok(Ok(apps)) => {
                        if let Some(app) = apps.first() {
                            let app_name = app
                                .get("name")
                                .and_then(|v| v.as_str())
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(ToOwned::to_owned);
                            let executables: Vec<crate::cdp_game_spoof::ApplicationExecutable> =
                                app.get("executables")
                                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                                    .unwrap_or_default();
                            if let Some(name) = app_name {
                                let resolved =
                                    crate::cdp_game_spoof::resolve_application_exe_metadata(
                                        host_os,
                                        &name,
                                        &executables,
                                        &hints,
                                    );
                                if let Ok(mut cache) = self.cached_app_catalog.lock() {
                                    cache.insert(
                                        application_id.to_string(),
                                        (Some((name, executables)), Instant::now()),
                                    );
                                }
                                return resolved;
                            }
                        }
                        if let Ok(mut cache) = self.cached_app_catalog.lock() {
                            cache.insert(application_id.to_string(), (None, Instant::now()));
                        }
                    }
                    Ok(Err(err)) => {
                        log(
                            LogLevel::Warn,
                            LogCategory::Api,
                            "Failed to parse /applications/public response during heartbeat enrichment",
                            Some(&format!("app_id={application_id}: {err}")),
                        );
                    }
                    Err(_) => {
                        log(
                            LogLevel::Warn,
                            LogCategory::Api,
                            "Timed out reading /applications/public body during heartbeat enrichment",
                            Some(&format!("app_id={application_id}")),
                        );
                        if let Ok(mut cache) = self.cached_app_catalog.lock() {
                            cache.insert(application_id.to_string(), (None, Instant::now()));
                        }
                    }
                }
            }
            Ok(Ok(response)) => {
                let status = response.status();
                log(
                    LogLevel::Warn,
                    LogCategory::Api,
                    "Non-success status from /applications/public during heartbeat enrichment",
                    Some(&format!("app_id={application_id}, status={status}")),
                );
                if status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS {
                    if let Ok(mut cache) = self.cached_app_catalog.lock() {
                        cache.insert(application_id.to_string(), (None, Instant::now()));
                    }
                }
            }
            Ok(Err(err)) => {
                log(
                    LogLevel::Warn,
                    LogCategory::Api,
                    "Network error querying /applications/public during heartbeat enrichment",
                    Some(&format!("app_id={application_id}: {err}")),
                );
            }
            Err(_) => {
                log(
                    LogLevel::Warn,
                    LogCategory::Api,
                    "Timed out querying /applications/public during heartbeat enrichment",
                    Some(&format!("app_id={application_id}")),
                );
                if let Ok(mut cache) = self.cached_app_catalog.lock() {
                    cache.insert(application_id.to_string(), (None, Instant::now()));
                }
            }
        }
        None
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
            .await?
            .send_reconnecting(self)
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

    fn decision_request(
        &self,
        endpoint: &str,
        placement: u64,
        num: Option<u64>,
        identity: &crate::super_properties::RequestIdentity,
    ) -> Result<RequestBuilder> {
        let mut url = reqwest::Url::parse(&format!("{DISCORD_API_BASE}/quests/{endpoint}"))?;
        url.query_pairs_mut()
            .append_pair("placement", &placement.to_string());
        if let Some(num) = num {
            url.query_pairs_mut()
                .append_pair("num_decisions_requested", &num.to_string());
        }
        url.query_pairs_mut()
            .append_pair("client_ad_session_id", &identity.ad_session_id);
        if let Some(id) = &identity.heartbeat_session_id {
            url.query_pairs_mut()
                .append_pair("client_heartbeat_session_id", id);
        }
        self.request_with_identity(Method::GET, url.as_str(), identity)
    }

    pub async fn get_quest_decision_debug(&self, placement: u64) -> Result<serde_json::Value> {
        let identity = self.prepare_identity().await?;

        let response = self
            .decision_request("decision", placement, None, &identity)?
            .send_reconnecting(self)
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
        let identity = self.prepare_identity().await?;

        let response = self
            .decision_request("get-decisions", placement, Some(num), &identity)?
            .send_reconnecting(self)
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

    pub async fn get_current_user_badges(&self) -> Result<serde_json::Value> {
        let url = format!("{}/users/@me/badges", DISCORD_API_BASE);
        let response = self
            .request(Method::GET, &url)
            .await?
            .send_reconnecting(self)
            .await
            .context("Request for user badges failed")?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Failed to get user badges: {}", status);
        }
        response.json().await.context("Failed to parse user badges")
    }

    pub async fn get_virtual_currency_balance(&self) -> Result<serde_json::Value> {
        let url = format!("{}/users/@me/virtual-currency/balance", DISCORD_API_BASE);

        let response = self
            .request(Method::GET, &url)
            .await?
            .send_reconnecting(self)
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

    pub async fn get_virtual_currency_total_redeemed(&self) -> Result<serde_json::Value> {
        let url = format!(
            "{}/users/@me/virtual-currency/total-redeemed",
            DISCORD_API_BASE
        );
        let response = self
            .request(Method::GET, &url)
            .await?
            .send_reconnecting(self)
            .await
            .context("Request for virtual currency total redeemed failed")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "Failed to get virtual currency total redeemed: {} - {}",
                status,
                body
            );
        }

        serde_json::from_str(&body).context("Failed to parse virtual currency total redeemed")
    }

    /// Get the current user's billing subscriptions.
    ///
    /// Kept for callers that need billing-period information. Nitro Orbs
    /// countdowns use the dedicated program-rewards endpoint below.
    pub async fn get_billing_subscriptions(&self) -> Result<serde_json::Value> {
        let url = format!(
            "{}/users/@me/billing/subscriptions?sync_level=2",
            DISCORD_API_BASE
        );

        let response = self
            .request(Method::GET, &url)
            .await?
            .send_reconnecting(self)
            .await
            .context("Request for billing subscriptions failed")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Failed to get billing subscriptions: {} - {}", status, body);
        }

        serde_json::from_str(&body).context("Failed to parse billing subscriptions")
    }

    /// Get Discord's server-calculated reward-program countdown state.
    pub async fn get_program_rewards(&self) -> Result<serde_json::Value> {
        let url = format!("{}/users/@me/program-rewards", DISCORD_API_BASE);

        let response = self
            .request(Method::GET, &url)
            .await?
            .send_reconnecting(self)
            .await
            .context("Request for program rewards failed")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Failed to get program rewards: {} - {}", status, body);
        }

        serde_json::from_str(&body).context("Failed to parse program rewards")
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
            .await?
            .json(&payload)
            .send_reconnecting(self)
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
            .await?
            .json(&payload)
            .send_reconnecting(self)
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

        let (resolved_user_id, resolved_exe) = tokio::join!(
            self.resolve_user_id(),
            self.resolve_game_exe_path(application_id)
        );

        let (executable_path, executable_fingerprint) =
            match (resolved_user_id.as_deref(), resolved_exe.as_ref()) {
                (Some(user_id), Some((exe_path, is_pe64)))
                    if !user_id.is_empty() && !exe_path.is_empty() =>
                {
                    (
                        Some(crate::cdp_game_spoof::format_quest_executable_path(
                            exe_path,
                        )),
                        Some(
                            crate::cdp_game_spoof::synthesize_signed_executable_fingerprint(
                                user_id,
                                application_id,
                                exe_path,
                                None,
                                *is_pe64,
                            ),
                        ),
                    )
                }
                _ => (None, None),
            };

        let payload = GameHeartbeatPayload {
            application_id: application_id.to_string(),
            terminal,
            executable_path,
            executable_fingerprint,
        };

        println!(
            "Sending game heartbeat: quest_id={}, app_id={}, terminal={}",
            quest_id, application_id, terminal
        );

        let response = self
            .request(Method::POST, &url)
            .await?
            .json(&payload)
            .send_reconnecting(self)
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

    /// Send a heartbeat for a PLAY_ACTIVITY cloud-game quest.
    ///
    /// Discord starts/continues the timer with `application_id` and closes the
    /// timer session with a terminal-only payload.
    pub async fn send_play_activity_heartbeat(
        &self,
        quest_id: &str,
        application_id: Option<&str>,
        terminal: bool,
    ) -> Result<PlayActivityHeartbeatStatus> {
        let url = format!("{}/quests/{}/heartbeat", DISCORD_API_BASE, quest_id);
        let payload = PlayActivityHeartbeatPayload {
            application_id: application_id.map(ToOwned::to_owned),
            terminal,
        };

        let response = self
            .request(Method::POST, &url)
            .await?
            .json(&payload)
            .send_reconnecting(self)
            .await
            .context("Failed to send PLAY_ACTIVITY heartbeat")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let body_length = body.chars().count();
            let mut body_preview: String = body.chars().take(1024).collect();
            if body_preview.is_empty() {
                body_preview.push_str("<empty response body>");
            } else if body_length > 1024 {
                body_preview.push_str("… [truncated]");
            }
            anyhow::bail!(
                "Failed to send PLAY_ACTIVITY heartbeat: {} - {}",
                status,
                body_preview
            );
        }

        let body: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse PLAY_ACTIVITY heartbeat response")?;
        parse_play_activity_heartbeat_response(&body)
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
            .await?
            .json(&payload)
            .send_reconnecting(self)
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
            .await?
            .json(&minimal_payload)
            .send_reconnecting(self)
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

    /// Fetch the complete games and non-games catalog; partial results are not cached.
    pub async fn fetch_detectable_games(&self) -> Result<Vec<DetectableGame>> {
        let games_url = format!("{}/applications/detectable", DISCORD_API_BASE);
        let apps_url = format!("{}/applications/non-games/detectable", DISCORD_API_BASE);
        fetch_detectable_catalog(
            self.request(Method::GET, &games_url).await?,
            self.request(Method::GET, &apps_url).await?,
            |request| request.send_reconnecting(self),
        )
        .await
    }
}

async fn fetch_detectable_catalog<F, Fut>(
    games_request: reqwest::RequestBuilder,
    apps_request: reqwest::RequestBuilder,
    send: F,
) -> Result<Vec<DetectableGame>>
where
    F: Fn(reqwest::RequestBuilder) -> Fut,
    Fut: std::future::Future<Output = reqwest::Result<reqwest::Response>>,
{
    let fetch_list = |request: reqwest::RequestBuilder| {
        let response = send(request);
        async move {
            response
                .await?
                .error_for_status()?
                .json::<Vec<DetectableGame>>()
                .await
                .context("Invalid detectable catalog response")
        }
    };
    let (games_result, apps_result) =
        tokio::join!(fetch_list(games_request), fetch_list(apps_request));
    let mut games = games_result.context("Failed to fetch detectable games")?;
    let mut apps = apps_result.context("Failed to fetch detectable applications")?;
    for game in &mut games {
        game.type_name = Some("Game".to_string());
    }
    for app in &mut apps {
        app.type_name = Some("App".to_string());
    }
    games.extend(apps);
    Ok(games)
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

    /// Accepts connections until aborted and records each request head. The
    /// first `dropped` connections are closed after their request is read;
    /// later ones receive `status`.
    fn spawn_flaky_server(
        listener: tokio::net::TcpListener,
        dropped: usize,
        status: u16,
    ) -> (Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut buffer = [0; 4096];
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(count) => request.extend_from_slice(&buffer[..count]),
                    }
                }
                let index = {
                    let mut recorded = recorded.lock().unwrap();
                    recorded.push(String::from_utf8_lossy(&request).to_ascii_lowercase());
                    recorded.len() - 1
                };
                if index < dropped {
                    continue;
                }
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                // Keep the socket briefly so Windows does not reset the response.
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        });
        (requests, server)
    }

    fn proxyless_client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    /// A client whose requests the test server can tell apart from the original's.
    fn replacement_client() -> reqwest::Client {
        let mut headers = HeaderMap::new();
        headers.insert("x-test-client", HeaderValue::from_static("replacement"));
        reqwest::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn get_requests_retry_once_on_a_new_client_after_a_connection_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/quests/@me", listener.local_addr().unwrap());
        let (requests, server) = spawn_flaky_server(listener, 1, 200);
        let client = proxyless_client();
        let request = client.get(&url).build().unwrap();
        let mut reconnects = 0;

        let response = execute_reconnecting(client, request, |_| {
            reconnects += 1;
            Some(replacement_client())
        })
        .await;
        server.abort();

        assert_eq!(response.unwrap().status(), 200);
        assert_eq!(reconnects, 1);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(!requests[0].contains("x-test-client: replacement"));
        assert!(requests[1].contains("x-test-client: replacement"));
    }

    #[tokio::test]
    async fn non_get_requests_reconnect_without_retrying() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/quests/1/enroll", listener.local_addr().unwrap());
        let (requests, server) = spawn_flaky_server(listener, usize::MAX, 200);
        let client = proxyless_client();
        let request = client.post(&url).body("{}").build().unwrap();
        let mut reconnects = 0;

        let error = execute_reconnecting(client, request, |_| {
            reconnects += 1;
            Some(replacement_client())
        })
        .await
        .unwrap_err();
        server.abort();

        assert!(is_connection_error(&error));
        assert_eq!(reconnects, 1);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn http_error_statuses_do_not_reconnect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/quests/@me", listener.local_addr().unwrap());
        let (requests, server) = spawn_flaky_server(listener, 0, 500);
        let client = proxyless_client();
        let request = client.get(&url).build().unwrap();
        let mut reconnects = 0;

        let response = execute_reconnecting(client, request, |_| {
            reconnects += 1;
            Some(replacement_client())
        })
        .await;
        server.abort();

        assert_eq!(response.unwrap().status(), 500);
        assert_eq!(reconnects, 0);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    async fn detectable_catalog_response(
        games_status: u16,
        apps_status: u16,
        games_body: &str,
        apps_body: &str,
    ) -> Result<Vec<DetectableGame>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let games_body = games_body.to_string();
        let apps_body = apps_body.to_string();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut buffer = [0; 4096];
                    let count = stream.read(&mut buffer).await.unwrap();
                    assert!(count > 0, "request ended before the headers were received");
                    request.extend_from_slice(&buffer[..count]);
                }
                let is_app = String::from_utf8_lossy(&request).starts_with("GET /apps ");
                let (status, body) = if is_app {
                    (apps_status, &apps_body)
                } else {
                    (games_status, &games_body)
                };
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                // Like the CDP HTTP fixtures, retain the socket so Windows does
                // not reset a just-written response before the peer receives it.
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            fetch_detectable_catalog(
                client.get(format!("http://{address}/games")),
                client.get(format!("http://{address}/apps")),
                |request| request.send(),
            ),
        )
        .await
        .unwrap();
        server.await.unwrap();
        result
    }

    const DETECTABLE_ITEM: &str = r#"[{"id":"same","name":"Example","executables":[]}]"#;

    #[tokio::test]
    async fn detectable_catalog_merges_both_categories_without_dropping_shared_ids() {
        let items = detectable_catalog_response(200, 200, DETECTABLE_ITEM, DETECTABLE_ITEM)
            .await
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].type_name.as_deref(), Some("Game"));
        assert_eq!(items[1].type_name.as_deref(), Some("App"));
        assert_eq!(items[0].id, items[1].id);
    }

    #[tokio::test]
    async fn detectable_catalog_rejects_partial_http_failures_in_either_category() {
        for (games_status, apps_status, expected) in
            [(503, 200, "games"), (200, 503, "applications")]
        {
            let error = detectable_catalog_response(
                games_status,
                apps_status,
                DETECTABLE_ITEM,
                DETECTABLE_ITEM,
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains(expected));
            assert!(format!("{error:#}").contains("503"));
        }
    }

    #[tokio::test]
    async fn detectable_catalog_rejects_invalid_json_instead_of_returning_partial_data() {
        assert!(
            detectable_catalog_response(200, 200, DETECTABLE_ITEM, "invalid-json")
                .await
                .is_err()
        );
    }

    #[test]
    fn play_activity_payload_matches_start_and_terminal_har_shapes() {
        let running = PlayActivityHeartbeatPayload {
            application_id: Some("1369749990758678741".to_string()),
            terminal: false,
        };
        let terminal = PlayActivityHeartbeatPayload {
            application_id: None,
            terminal: true,
        };

        assert_eq!(
            serde_json::to_value(running).unwrap(),
            serde_json::json!({
                "application_id": "1369749990758678741",
                "terminal": false
            })
        );
        assert_eq!(
            serde_json::to_value(terminal).unwrap(),
            serde_json::json!({ "terminal": true })
        );
    }

    #[test]
    fn play_activity_response_uses_exact_progress_key() {
        let status = parse_play_activity_heartbeat_response(&serde_json::json!({
            "completed_at": null,
            "progress": {
                "PLAY_ACTIVITY": { "value": 48 }
            }
        }))
        .unwrap();

        assert_eq!(
            status,
            PlayActivityHeartbeatStatus {
                progress_seconds: 48.0,
                completed: false,
            }
        );

        let missing = parse_play_activity_heartbeat_response(&serde_json::json!({
            "progress": {
                "PLAY_ON_DESKTOP": { "value": 48 }
            }
        }));
        assert!(missing.is_err());
    }

    #[test]
    fn play_activity_response_reads_server_completion() {
        let status = parse_play_activity_heartbeat_response(&serde_json::json!({
            "completed_at": "2026-08-04T16:00:00+08:00",
            "progress": {
                "PLAY_ACTIVITY": { "value": 900 }
            }
        }))
        .unwrap();

        assert!(status.completed);
        assert_eq!(status.progress_seconds, 900.0);

        let nested_status = parse_play_activity_heartbeat_response(&serde_json::json!({
            "completed_at": null,
            "progress": {
                "PLAY_ACTIVITY": {
                    "value": 900,
                    "completed_at": "2026-08-04T16:00:00+08:00"
                }
            }
        }))
        .unwrap();
        assert!(nested_status.completed);
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
                "https://discord.com/api/v9/users/@me/virtual-currency/total-redeemed"
            ),
            Some(QUEST_HOME_REFERER)
        );
        assert_eq!(
            DiscordApiClient::quest_referer_for_url(
                "https://discord.com/api/v9/users/@me/virtual-currency/balance"
            ),
            Some(QUEST_HOME_REFERER)
        );
        assert_eq!(
            DiscordApiClient::quest_referer_for_url(
                "https://discord.com/api/v9/users/@me/program-rewards"
            ),
            Some(QUEST_HOME_REFERER)
        );
        assert_eq!(
            DiscordApiClient::quest_referer_for_url("https://discord.com/api/v9/users/@me"),
            None
        );
    }

    /// Verify that `DiscordApiClient::request` injects a `User-Agent` matching `x-super-properties`.
    #[tokio::test]
    async fn request_injects_user_agent_matching_x_super_properties() {
        use base64::Engine as _;

        let client = DiscordApiClient::new("test-token".to_string()).unwrap();
        let request = client
            .request(Method::GET, "https://discord.com/api/v9/quests/@me")
            .await
            .unwrap()
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
    async fn decision_query_headers_and_user_agent_share_a_frozen_snapshot() {
        use base64::Engine as _;
        let client = DiscordApiClient::new("synthetic-token".into()).unwrap();
        {
            let identity = client.candidate_identity();
            let mut manager = identity.lock().unwrap();
            manager.set_focused(true);
            manager.authenticate("synthetic-account".into());
        }
        let snapshot = client.prepare_identity().await.unwrap();
        // A later logout/source change must not alter an already prepared request.
        client
            .candidate_identity()
            .lock()
            .unwrap()
            .logout()
            .unwrap();
        for (endpoint, num) in [("decision", None), ("get-decisions", Some(2))] {
            let request = client
                .decision_request(endpoint, 1, num, &snapshot)
                .unwrap()
                .build()
                .unwrap();
            let properties: serde_json::Value = serde_json::from_slice(
                &base64::engine::general_purpose::STANDARD
                    .decode(request.headers()["x-super-properties"].to_str().unwrap())
                    .unwrap(),
            )
            .unwrap();
            let query: std::collections::HashMap<_, _> = request.url().query_pairs().collect();
            assert_eq!(
                query
                    .get("client_heartbeat_session_id")
                    .map(|id| id.as_ref()),
                properties["client_heartbeat_session_id"].as_str()
            );
            assert_eq!(
                request.headers()[USER_AGENT].to_str().unwrap(),
                properties["browser_user_agent"].as_str().unwrap()
            );
        }
        assert!(client.prepare_identity().await.is_err());
        let anonymous = DiscordApiClient::new("anonymous-candidate".into())
            .unwrap()
            .prepare_identity()
            .await
            .unwrap();
        let request = client
            .decision_request("decision", 1, None, &anonymous)
            .unwrap()
            .build()
            .unwrap();
        assert!(!request
            .url()
            .query_pairs()
            .any(|(key, _)| key == "client_heartbeat_session_id"));
    }

    #[tokio::test]
    async fn candidate_validation_has_no_other_accounts_session() {
        let first = DiscordApiClient::new("synthetic-first-token".into()).unwrap();
        let second = DiscordApiClient::new("synthetic-second-token".into()).unwrap();
        {
            let identity = first.candidate_identity();
            let mut manager = identity.lock().unwrap();
            manager.set_focused(true);
            manager.authenticate("first-account".into());
        }
        assert!(first
            .prepare_identity()
            .await
            .unwrap()
            .heartbeat_session_id
            .is_some());
        assert!(second
            .prepare_identity()
            .await
            .unwrap()
            .heartbeat_session_id
            .is_none());
    }

    #[tokio::test]
    async fn replaced_session_cannot_fall_back_to_the_committed_candidate() {
        let client = DiscordApiClient::new("retired-token".into()).unwrap();
        assert!(client.prepare_identity().await.is_ok());
        // A committed client must use the live manager even after its key changes.
        client.identity_committed.store(true, Ordering::Release);
        assert!(client.prepare_identity().await.is_err());
        assert!(client
            .request(Method::GET, "http://127.0.0.1:1/")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn explicit_null_user_agent_is_preserved_with_a_usable_http_fallback() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/super-properties.json"))
                .unwrap();
        let mut properties = fixture["desktop"]["properties"].clone();
        properties["browser_user_agent"] = serde_json::Value::Null;
        let raw = STANDARD.encode(serde_json::to_vec(&properties).unwrap());
        let client = DiscordApiClient::new("synthetic-token".into()).unwrap();
        client
            .candidate_identity()
            .lock()
            .unwrap()
            .set_from_cdp(&raw, &properties);
        let request = client
            .request(Method::GET, "https://discord.com/api/v9/users/@me")
            .await
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(
            request.headers()["x-super-properties"].to_str().unwrap(),
            raw
        );
        assert!(!request.headers()[USER_AGENT].to_str().unwrap().is_empty());
    }

    /// Verify that `DiscordApiClient::extract_user_id_from_token` decodes the numeric snowflake prefix.
    #[test]
    fn extract_user_id_from_token_parses_snowflake_prefix() {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let encoded = URL_SAFE_NO_PAD.encode("257900031351193600");
        let token = format!("{encoded}.X12345.fake_hmac_signature");
        assert_eq!(
            DiscordApiClient::extract_user_id_from_token(&token).as_deref(),
            Some("257900031351193600")
        );
        assert_eq!(
            DiscordApiClient::extract_user_id_from_token("test-token"),
            None
        );
    }

    /// Manual integration smoke test for `DiscordApiClient::get_current_user`.
    #[tokio::test]
    #[ignore] // Requires valid token
    async fn test_get_current_user() {
        let token = "YOUR_TOKEN_HERE";
        let client = DiscordApiClient::new(token.to_string()).unwrap();
        let user = client.get_current_user().await.unwrap();
        println!("User: {:?}", user);
    }
}

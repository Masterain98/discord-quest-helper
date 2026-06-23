//! CDP (Chrome DevTools Protocol) client for communicating with Discord
//!
//! Discord client based on Electron (Chromium), supports CDP protocol.
//! After starting Discord with the --remote-debugging-port parameter, it can communicate with the client via WebSocket.

use anyhow::{Context, Result};
use futures_util::{future::join_all, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Default CDP debugging port
pub const DEFAULT_CDP_PORT: u16 = 9223;

/// CDP target info (returned from /json endpoint)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CdpTarget {
    #[allow(dead_code)]
    pub id: String,
    #[serde(rename = "type")]
    pub target_type: String,
    pub title: String,
    pub url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub web_socket_debugger_url: Option<String>,
}

/// SuperProperties result obtained via CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpSuperProperties {
    pub base64: String,
    pub decoded: serde_json::Value,
}

/// CDP status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpStatus {
    pub available: bool,
    pub connected: bool,
    pub target_title: Option<String>,
    pub error: Option<String>,
}

/// Result of executing JS on a specific CDP target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpTargetExecutionResult {
    pub target_title: String,
    pub target_url: String,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// Captured Discord API request headers via CDP Network interception
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpCapturedHeaders {
    /// Total number of requests captured
    pub total_requests: usize,
    /// All captured requests with their URLs, methods, and headers
    pub requests: Vec<CapturedRequest>,
    /// Aggregated header key stats: header_name -> count
    pub header_key_counts: std::collections::HashMap<String, usize>,
    /// Aggregated header key-value stats: "header_name: value" -> count  
    /// (authorization values are redacted)
    pub header_kv_counts: std::collections::HashMap<String, usize>,
    /// Duration in seconds the capture ran
    pub capture_duration_secs: u64,
}

/// A single captured HTTP request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedRequest {
    pub url: String,
    pub method: String,
    pub headers: std::collections::HashMap<String, String>,
}

/// JavaScript code: Get SuperProperties
///
/// FRAGILE: This code relies on Discord's internal webpack module structure.
/// The webpackChunkdiscord_app.push trick is used to access Discord's module system.
///
/// This approach may break if Discord:
/// - Changes their webpack chunking mechanism
/// - Renames the global variable
/// - Modifies the module structure
/// - Updates their bundler
///
/// Fallback behavior: If extraction fails, the app falls back to:
/// 1. Remote JS (fetching from Discord's website)
/// 2. Built-in defaults
const JS_GET_SUPER_PROPERTIES: &str = r#"
(() => {
    try {
        if (typeof window !== "undefined" && !window.webpackChunkdiscord_app) {
            return JSON.stringify({ error: "Discord webpackChunkdiscord_app not found; the Discord client structure may have changed." });
        }

        let wpRequire = webpackChunkdiscord_app.push([[Symbol()], {}, r => r]);
        webpackChunkdiscord_app.pop();
        
        // Search for the correct SuperProperties module
        // Module must have both getSuperPropertiesBase64 and getSuperProperties methods
        // And getSuperPropertiesBase64() must return a string (base64 encoded)
        let superPropsModule = null;
        for (const m of Object.values(wpRequire.c)) {
            try {
                const exp = m?.exports?.default;
                if (exp && typeof exp.getSuperPropertiesBase64 === 'function' && typeof exp.getSuperProperties === 'function') {
                    const base64Result = exp.getSuperPropertiesBase64();
                    // The real SuperProperties returns a base64 string, not an object
                    if (typeof base64Result === 'string' && base64Result.length > 50) {
                        superPropsModule = m;
                        break;
                    }
                }
            } catch (e) {
                continue;
            }
        }
        
        if (!superPropsModule) return JSON.stringify({ error: "SuperProperties module not found" });
        
        const base64 = superPropsModule.exports.default.getSuperPropertiesBase64();
        const decoded = superPropsModule.exports.default.getSuperProperties();
        
        // Verify return value format
        if (typeof base64 !== 'string') {
            return JSON.stringify({ error: "getSuperPropertiesBase64 did not return a string" });
        }
        if (!decoded || typeof decoded !== 'object' || !decoded.client_build_number) {
            return JSON.stringify({ error: "getSuperProperties did not return valid object" });
        }
        
        return JSON.stringify({ base64, decoded });
    } catch (e) {
        let message = (e && e.message) ? e.message : String(e);
        try {
            if (typeof window !== "undefined" && !window.webpackChunkdiscord_app) {
                message = "Discord webpackChunkdiscord_app not found; variable missing during execution. Original error: " + message;
            }
        } catch (_) {}
        return JSON.stringify({ error: message });
    }
})()
"#;

/// Check if CDP port is available
pub async fn check_cdp_available(port: u16) -> CdpStatus {
    match get_cdp_targets(port).await {
        Ok(targets) => {
            if let Some(target) = pick_discord_target(&targets) {
                CdpStatus {
                    available: true,
                    connected: target.web_socket_debugger_url.is_some(),
                    target_title: Some(target.title.clone()),
                    error: None,
                }
            } else {
                CdpStatus {
                    available: true,
                    connected: false,
                    target_title: None,
                    error: Some("No Discord target found".to_string()),
                }
            }
        }
        Err(e) => CdpStatus {
            available: false,
            connected: false,
            target_title: None,
            error: Some(e.to_string()),
        },
    }
}

/// Get CDP target list
async fn get_cdp_targets(port: u16) -> Result<Vec<CdpTarget>> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(3))
        .build()?;

    let url = format!("http://127.0.0.1:{}/json", port);
    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to connect to CDP endpoint")?
        .error_for_status()
        .context("CDP endpoint returned non-success status")?;

    let targets: Vec<CdpTarget> = response
        .json()
        .await
        .context("Failed to parse CDP targets")?;

    Ok(targets)
}

/// Select Discord main window target (skip updater)
fn pick_discord_target(targets: &[CdpTarget]) -> Option<&CdpTarget> {
    // Prioritize targets with type "page" and title containing "Discord" (but not "updater")
    let pages: Vec<_> = targets.iter().filter(|t| t.target_type == "page").collect();

    // Find Discord main application
    for target in &pages {
        if is_discord_target(target) {
            return Some(target);
        }
    }

    // Fallback: return the first page
    pages.first().copied()
}

/// Return true if this target looks like a Discord app page.
fn is_discord_target(target: &CdpTarget) -> bool {
    if target.target_type != "page" {
        return false;
    }

    let title_lower = target.title.to_lowercase();
    let url_lower = target.url.to_lowercase();

    (title_lower.contains("discord") && !title_lower.contains("updater"))
        || url_lower.contains("discord.com")
        || url_lower.contains("discordapp.com")
}

fn select_discord_targets<'a>(targets: &'a [CdpTarget]) -> Vec<&'a CdpTarget> {
    let mut selected_targets: Vec<&CdpTarget> = targets
        .iter()
        .filter(|t| is_discord_target(t) && t.web_socket_debugger_url.is_some())
        .collect();

    // Fallback: keep old behavior if detection fails and use the best single target.
    if selected_targets.is_empty() {
        if let Some(target) =
            pick_discord_target(targets).filter(|t| t.web_socket_debugger_url.is_some())
        {
            selected_targets.push(target);
        }
    }

    selected_targets
}

pub async fn get_primary_discord_target(port: u16) -> Result<CdpTarget> {
    let targets = get_cdp_targets(port).await?;

    pick_discord_target(&targets)
        .cloned()
        .context("No Discord target found")
}

pub async fn navigate_primary_discord_target(
    port: u16,
    url: &str,
    timeout_secs: u64,
) -> Result<()> {
    use crate::logger::{log, LogCategory, LogLevel};

    let target = get_primary_discord_target(port).await?;
    let ws_url = target
        .web_socket_debugger_url
        .as_ref()
        .context("Target has no WebSocket URL")?;

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!(
            "navigate_primary_discord_target: from={} to={} timeout={}s",
            target.url, url, timeout_secs
        ),
        None,
    );

    navigate_target_via_ws(ws_url, url, timeout_secs).await
}

pub async fn bring_primary_discord_target_to_front(port: u16) -> Result<()> {
    use crate::logger::{log, LogCategory, LogLevel};

    let target = get_primary_discord_target(port).await?;
    let ws_url = target
        .web_socket_debugger_url
        .as_ref()
        .context("Target has no WebSocket URL")?;

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!(
            "bring_primary_discord_target_to_front: target_url={}",
            target.url
        ),
        None,
    );

    bring_target_to_front_via_ws(ws_url, 5).await
}

pub async fn execute_js_via_primary_discord_target(
    port: u16,
    js_code: &str,
    await_promise: bool,
    timeout_secs: u64,
) -> Result<String> {
    use crate::logger::{log, LogCategory, LogLevel};

    let target = get_primary_discord_target(port).await?;
    let ws_url = target
        .web_socket_debugger_url
        .as_ref()
        .context("Target has no WebSocket URL")?;

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!(
            "execute_js_via_primary_discord_target: target_url={} await_promise={} timeout={}s code_len={}",
            target.url,
            await_promise,
            timeout_secs,
            js_code.len()
        ),
        None,
    );

    execute_js_via_ws(ws_url, js_code, await_promise, timeout_secs).await
}

/// Get SuperProperties via CDP
pub async fn fetch_super_properties_via_cdp(port: u16) -> Result<CdpSuperProperties> {
    use crate::logger::{log, LogCategory, LogLevel};

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "Attempting to fetch SuperProperties via CDP on port {}",
            port
        ),
        None,
    );

    // Get targets
    let targets = get_cdp_targets(port).await?;
    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!("Found {} CDP targets", targets.len()),
        None,
    );

    let target = pick_discord_target(&targets).context("No Discord target found")?;

    let ws_url = target
        .web_socket_debugger_url
        .as_ref()
        .context("Target has no WebSocket URL")?;

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!(
            "Connecting to CDP target: {} (URL: {})",
            target.title, ws_url
        ),
        None,
    );

    // Establish WebSocket connection
    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("Failed to connect to CDP WebSocket")?;

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        "WebSocket connection established",
        None,
    );

    let (mut write, mut read) = ws_stream.split();

    // Send Runtime.evaluate request
    let request = serde_json::json!({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": {
            "expression": JS_GET_SUPER_PROPERTIES,
            "returnByValue": true,
            "awaitPromise": false
        }
    });

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        "Sending Runtime.evaluate request",
        None,
    );

    write
        .send(Message::Text(request.to_string().into()))
        .await
        .context("Failed to send CDP request")?;

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        "Request sent, waiting for response...",
        None,
    );

    // Read response
    let response = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    log(
                        LogLevel::Debug,
                        LogCategory::TokenExtraction,
                        &format!(
                            "Received message: {}...",
                            &text.chars().take(200).collect::<String>()
                        ),
                        None,
                    );

                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if json.get("id") == Some(&serde_json::json!(1)) {
                            return Ok(json);
                        }
                    }
                }
                Ok(other) => {
                    log(
                        LogLevel::Debug,
                        LogCategory::TokenExtraction,
                        &format!("Received non-text message: {:?}", other),
                        None,
                    );
                    continue;
                }
                Err(e) => {
                    log(
                        LogLevel::Error,
                        LogCategory::TokenExtraction,
                        &format!("WebSocket error: {}", e),
                        None,
                    );
                    return Err(anyhow::anyhow!("WebSocket error: {}", e));
                }
            }
        }
        log(
            LogLevel::Error,
            LogCategory::TokenExtraction,
            "WebSocket closed unexpectedly",
            None,
        );
        Err(anyhow::anyhow!("WebSocket closed unexpectedly"))
    })
    .await
    .context("CDP request timed out (10s)")??;

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        "Received valid CDP response",
        None,
    );

    // Close connection
    let _ = write.close().await;

    // Parse response
    let result_value = response
        .get("result")
        .and_then(|r| r.get("result"))
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_str())
        .context("Invalid CDP response structure")?;

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!(
            "JavaScript returned: {}...",
            &result_value.chars().take(100).collect::<String>()
        ),
        None,
    );

    let parsed: serde_json::Value =
        serde_json::from_str(result_value).context("Failed to parse JavaScript result")?;

    // Check for errors
    if let Some(error) = parsed.get("error") {
        log(
            LogLevel::Error,
            LogCategory::TokenExtraction,
            &format!("JavaScript error: {}", error),
            None,
        );
        anyhow::bail!("JavaScript error: {}", error);
    }

    let super_props: CdpSuperProperties =
        serde_json::from_value(parsed).context("Failed to parse SuperProperties")?;

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "Successfully fetched SuperProperties via CDP. Build number: {}",
            super_props
                .decoded
                .get("client_build_number")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        ),
        None,
    );

    Ok(super_props)
}

/// Capture Discord API request headers via CDP Network interception.
///
/// Enables CDP Network domain, listens for ALL outgoing requests for `duration_secs`,
/// and collects all headers with statistics.
pub async fn capture_discord_headers_via_cdp(
    port: u16,
    duration_secs: u64,
) -> Result<CdpCapturedHeaders> {
    use crate::logger::{log, LogCategory, LogLevel};
    use std::collections::HashMap;

    let duration_secs = duration_secs.min(120).max(5); // clamp 5..120

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "Capturing all request headers via CDP Network on port {} for {}s",
            port, duration_secs
        ),
        None,
    );

    let targets = get_cdp_targets(port).await?;
    let target = pick_discord_target(&targets).context("No Discord target found")?;
    let ws_url = target
        .web_socket_debugger_url
        .as_ref()
        .context("Target has no WebSocket URL")?;

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!("Connecting to CDP target: {}", target.title),
        None,
    );

    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("Failed to connect to CDP WebSocket")?;
    let (mut write, mut read) = ws_stream.split();

    // Enable Network domain
    let enable_request = serde_json::json!({
        "id": 1,
        "method": "Network.enable",
        "params": {}
    });
    write
        .send(Message::Text(enable_request.to_string().into()))
        .await
        .context("Failed to send Network.enable")?;

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        "Network.enable sent, collecting all requests...",
        None,
    );

    let mut requests: Vec<CapturedRequest> = Vec::new();
    let mut header_key_counts: HashMap<String, usize> = HashMap::new();
    let mut header_kv_counts: HashMap<String, usize> = HashMap::new();

    // Sensitive headers whose values should be redacted in kv stats
    let redact_values = ["authorization", "cookie", "set-cookie"];

    // Collect for the specified duration
    let _ = tokio::time::timeout(Duration::from_secs(duration_secs), async {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    let json = match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(j) => j,
                        Err(_) => continue,
                    };

                    if json.get("method").and_then(|v| v.as_str())
                        != Some("Network.requestWillBeSent")
                    {
                        continue;
                    }

                    let params = match json.get("params") {
                        Some(p) => p,
                        None => continue,
                    };
                    let request = match params.get("request") {
                        Some(r) => r,
                        None => continue,
                    };
                    let url = request
                        .get("url")
                        .and_then(|u| u.as_str())
                        .unwrap_or("")
                        .to_string();
                    let method = request
                        .get("method")
                        .and_then(|m| m.as_str())
                        .unwrap_or("GET")
                        .to_string();

                    let headers_obj = match request.get("headers").and_then(|h| h.as_object()) {
                        Some(h) => h,
                        None => continue,
                    };

                    let mut req_headers: HashMap<String, String> = HashMap::new();

                    for (key, value) in headers_obj {
                        let val_str = value.as_str().unwrap_or("").to_string();
                        let key_lower = key.to_lowercase();

                        // Count header key occurrence
                        *header_key_counts.entry(key_lower.clone()).or_insert(0) += 1;

                        // Fully redact sensitive values
                        let display_val = if redact_values.contains(&key_lower.as_str()) {
                            "[redacted]".to_string()
                        } else {
                            val_str.clone()
                        };

                        // Count header key-value occurrence
                        let kv_key = format!("{}: {}", key_lower, display_val);
                        *header_kv_counts.entry(kv_key).or_insert(0) += 1;

                        // Store in per-request headers
                        req_headers.insert(key_lower, display_val);
                    }

                    requests.push(CapturedRequest {
                        url,
                        method,
                        headers: req_headers,
                    });
                }
                Err(e) => {
                    log(
                        LogLevel::Warn,
                        LogCategory::TokenExtraction,
                        &format!("WebSocket error during capture: {}", e),
                        None,
                    );
                    break;
                }
                _ => continue,
            }
        }
    })
    .await;

    // Disable Network domain and close connection
    let disable_request = serde_json::json!({
        "id": 2,
        "method": "Network.disable",
        "params": {}
    });
    let _ = write
        .send(Message::Text(disable_request.to_string().into()))
        .await;
    let _ = write.close().await;

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "Capture complete. {} requests collected in {}s",
            requests.len(),
            duration_secs
        ),
        None,
    );

    Ok(CdpCapturedHeaders {
        total_requests: requests.len(),
        requests,
        header_key_counts,
        header_kv_counts,
        capture_duration_secs: duration_secs,
    })
}

/// Execute JS on every Discord-like CDP page target.
///
/// This is used for best-effort cleanup, ensuring spoof state is removed even when
/// Discord exposes multiple page targets and the "active" one changes between calls.
pub async fn execute_js_via_all_discord_targets(
    port: u16,
    js_code: &str,
    await_promise: bool,
    timeout_secs: u64,
) -> Result<Vec<CdpTargetExecutionResult>> {
    use crate::logger::{log, LogCategory, LogLevel};

    let targets = get_cdp_targets(port).await?;

    let selected_targets = select_discord_targets(&targets);

    if selected_targets.is_empty() {
        anyhow::bail!("No CDP page targets found");
    }

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!(
            "execute_js_via_all_discord_targets: running on {} target(s)",
            selected_targets.len()
        ),
        None,
    );

    // Execute all target evaluations concurrently. Each task still respects per-target
    // timeout via execute_js_via_ws().
    let tasks = selected_targets.into_iter().map(|target| async move {
        let mut item = CdpTargetExecutionResult {
            target_title: target.title.clone(),
            target_url: target.url.clone(),
            result: None,
            error: None,
        };

        if let Some(ws_url) = target.web_socket_debugger_url.as_ref() {
            match execute_js_via_ws(ws_url, js_code, await_promise, timeout_secs).await {
                Ok(result) => item.result = Some(result),
                Err(e) => item.error = Some(e.to_string()),
            }
        } else {
            item.error = Some("Target has no WebSocket URL".to_string());
        }

        item
    });

    let results = join_all(tasks).await;

    Ok(results)
}

/// Find the activity iframe CDP target (discordsays.com).
pub async fn find_activity_iframe_target(port: u16) -> Result<CdpTarget> {
    let targets = get_cdp_targets(port).await?;

    let iframe_target = targets.iter().find(|t| {
        let is_activity_host = reqwest::Url::parse(&t.url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .map(|host| host == "discordsays.com" || host.ends_with(".discordsays.com"))
            .unwrap_or(false);

        (t.target_type == "iframe" || t.target_type == "page")
            && is_activity_host
            && t.web_socket_debugger_url.is_some()
    });

    iframe_target
        .cloned()
        .context("No activity iframe target found. Make sure the Activity is launched in Discord.")
}

/// Execute JavaScript on a specific CDP target via its WebSocket URL.
pub async fn execute_js_on_target(
    ws_url: &str,
    js_code: &str,
    await_promise: bool,
    timeout_secs: u64,
) -> Result<String> {
    execute_js_via_ws(ws_url, js_code, await_promise, timeout_secs).await
}

async fn execute_js_via_ws(
    ws_url: &str,
    js_code: &str,
    await_promise: bool,
    timeout_secs: u64,
) -> Result<String> {
    use crate::logger::{log, LogCategory, LogLevel};

    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("Failed to connect to CDP WebSocket")?;
    let (mut write, mut read) = ws_stream.split();

    let request = serde_json::json!({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": {
            "expression": js_code,
            "returnByValue": true,
            "awaitPromise": await_promise
        }
    });

    write
        .send(Message::Text(request.to_string().into()))
        .await
        .context("Failed to send CDP request")?;

    let response = tokio::time::timeout(Duration::from_secs(timeout_secs), async {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if json.get("id") == Some(&serde_json::json!(1)) {
                            return Ok(json);
                        }
                    }
                }
                Ok(_) => continue,
                Err(e) => return Err(anyhow::anyhow!("WebSocket error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("WebSocket closed unexpectedly"))
    })
    .await
    .context(format!("CDP request timed out ({}s)", timeout_secs))??;

    let _ = write.close().await;

    // Check for CDP-level errors (e.g., method not found, invalid params)
    if let Some(error) = response.get("error") {
        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown CDP error");
        anyhow::bail!("CDP error (code {}): {}", code, message);
    }

    // Extract the result value from the CDP response
    // For successful evaluations: response.result.result.value (string)
    // For exceptions: response.result.exceptionDetails
    if let Some(exception) = response
        .get("result")
        .and_then(|r| r.get("exceptionDetails"))
    {
        let text = exception
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("Unknown JS exception");
        anyhow::bail!("JavaScript exception: {}", text);
    }

    let result_value = response
        .get("result")
        .and_then(|r| r.get("result"))
        .and_then(|r| {
            // Handle both string values and other types
            if let Some(s) = r.get("value").and_then(|v| v.as_str()) {
                Some(s.to_string())
            } else if let Some(v) = r.get("value") {
                // If value is not a string (e.g., object with returnByValue), serialize it
                Some(v.to_string())
            } else {
                // No value field — check if type is "undefined"
                let rtype = r.get("type").and_then(|t| t.as_str()).unwrap_or("");
                log(
                    LogLevel::Warn,
                    LogCategory::TokenExtraction,
                    &format!(
                        "CDP result has no value field. type={}, full result: {}",
                        rtype,
                        serde_json::to_string(r).unwrap_or_default()
                    ),
                    None,
                );
                None
            }
        })
        .unwrap_or_default();

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!(
            "execute_js_via_cdp result: {}...",
            &result_value.chars().take(200).collect::<String>()
        ),
        None,
    );

    Ok(result_value)
}

async fn navigate_target_via_ws(ws_url: &str, url: &str, timeout_secs: u64) -> Result<()> {
    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("Failed to connect to CDP WebSocket")?;
    let (mut write, mut read) = ws_stream.split();

    let enable_request = serde_json::json!({
        "id": 1,
        "method": "Page.enable",
        "params": {}
    });

    write
        .send(Message::Text(enable_request.to_string().into()))
        .await
        .context("Failed to send CDP Page.enable request")?;

    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if json.get("id") == Some(&serde_json::json!(1)) {
                            if let Some(error) = json.get("error") {
                                let code = error
                                    .get("code")
                                    .and_then(|value| value.as_i64())
                                    .unwrap_or(0);
                                let message = error
                                    .get("message")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or("Unknown CDP error");
                                return Err(anyhow::anyhow!(
                                    "CDP error (code {}): {}",
                                    code,
                                    message
                                ));
                            }

                            return Ok(());
                        }
                    }
                }
                Ok(_) => continue,
                Err(e) => return Err(anyhow::anyhow!("WebSocket error: {}", e)),
            }
        }

        Err(anyhow::anyhow!(
            "WebSocket closed before Page.enable acknowledgement"
        ))
    })
    .await
    .context("CDP Page.enable timed out")??;

    let navigate_request = serde_json::json!({
        "id": 2,
        "method": "Page.navigate",
        "params": {
            "url": url,
        }
    });

    write
        .send(Message::Text(navigate_request.to_string().into()))
        .await
        .context("Failed to send CDP Page.navigate request")?;

    let mut navigation_acknowledged = false;

    tokio::time::timeout(Duration::from_secs(timeout_secs), async {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if json.get("id") == Some(&serde_json::json!(2)) {
                            if let Some(error) = json.get("error") {
                                let code = error
                                    .get("code")
                                    .and_then(|value| value.as_i64())
                                    .unwrap_or(0);
                                let message = error
                                    .get("message")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or("Unknown CDP error");
                                return Err(anyhow::anyhow!(
                                    "CDP error (code {}): {}",
                                    code,
                                    message
                                ));
                            }

                            if let Some(error_text) = json
                                .get("result")
                                .and_then(|value| value.get("errorText"))
                                .and_then(|value| value.as_str())
                            {
                                return Err(anyhow::anyhow!(
                                    "Page.navigate failed: {}",
                                    error_text
                                ));
                            }

                            navigation_acknowledged = true;
                            continue;
                        }

                        if navigation_acknowledged {
                            match json.get("method").and_then(|value| value.as_str()) {
                                Some("Page.loadEventFired")
                                | Some("Page.navigatedWithinDocument")
                                | Some("Page.frameStoppedLoading") => return Ok(()),
                                _ => continue,
                            }
                        }
                    }
                }
                Ok(_) => continue,
                Err(e) => return Err(anyhow::anyhow!("WebSocket error: {}", e)),
            }
        }

        if navigation_acknowledged {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "WebSocket closed before Page.navigate acknowledgement"
            ))
        }
    })
    .await
    .context(format!("CDP page navigation timed out ({}s)", timeout_secs))??;

    let _ = write.close().await;

    Ok(())
}

async fn bring_target_to_front_via_ws(ws_url: &str, timeout_secs: u64) -> Result<()> {
    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("Failed to connect to CDP WebSocket")?;
    let (mut write, mut read) = ws_stream.split();

    let request = serde_json::json!({
        "id": 1,
        "method": "Page.bringToFront",
        "params": {}
    });

    write
        .send(Message::Text(request.to_string().into()))
        .await
        .context("Failed to send CDP Page.bringToFront request")?;

    tokio::time::timeout(Duration::from_secs(timeout_secs), async {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if json.get("id") == Some(&serde_json::json!(1)) {
                            if let Some(error) = json.get("error") {
                                let code = error
                                    .get("code")
                                    .and_then(|value| value.as_i64())
                                    .unwrap_or(0);
                                let message = error
                                    .get("message")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or("Unknown CDP error");
                                return Err(anyhow::anyhow!(
                                    "CDP error (code {}): {}",
                                    code,
                                    message
                                ));
                            }

                            return Ok(());
                        }
                    }
                }
                Ok(_) => continue,
                Err(e) => return Err(anyhow::anyhow!("WebSocket error: {}", e)),
            }
        }

        Err(anyhow::anyhow!(
            "WebSocket closed before Page.bringToFront acknowledgement"
        ))
    })
    .await
    .context(format!(
        "CDP Page.bringToFront timed out ({}s)",
        timeout_secs
    ))??;

    let _ = write.close().await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_target(target_type: &str, title: &str, url: &str) -> CdpTarget {
        mk_target_opt_ws(target_type, title, url, Some("ws://example".to_string()))
    }

    fn mk_target_opt_ws(
        target_type: &str,
        title: &str,
        url: &str,
        ws: Option<String>,
    ) -> CdpTarget {
        CdpTarget {
            id: format!("{}-{}", target_type, title),
            target_type: target_type.to_string(),
            title: title.to_string(),
            url: url.to_string(),
            web_socket_debugger_url: ws,
        }
    }

    #[test]
    fn test_pick_discord_target() {
        let targets = vec![
            CdpTarget {
                id: "1".to_string(),
                target_type: "page".to_string(),
                title: "Discord Updater".to_string(),
                url: "about:blank".to_string(),
                web_socket_debugger_url: Some("ws://...".to_string()),
            },
            CdpTarget {
                id: "2".to_string(),
                target_type: "page".to_string(),
                title: "Discord".to_string(),
                url: "https://discord.com/app".to_string(),
                web_socket_debugger_url: Some("ws://...".to_string()),
            },
        ];

        let picked = pick_discord_target(&targets);
        assert!(picked.is_some());
        assert_eq!(picked.unwrap().id, "2");
    }

    #[test]
    fn test_is_discord_target_domain_and_updater_filter() {
        let discord_app = mk_target("page", "Some Title", "https://discordapp.com/channels/@me");
        let discord_updater = mk_target("page", "Discord Updater", "about:blank");
        let worker = mk_target("worker", "Discord", "https://discord.com/app");

        assert!(is_discord_target(&discord_app));
        assert!(!is_discord_target(&discord_updater));
        assert!(!is_discord_target(&worker));
    }

    #[test]
    fn test_pick_discord_target_fallback_to_first_page() {
        let targets = vec![
            mk_target("page", "Not Discord 1", "https://example.com/a"),
            mk_target("page", "Not Discord 2", "https://example.com/b"),
        ];

        let picked = pick_discord_target(&targets);
        assert!(picked.is_some());
        assert_eq!(picked.unwrap().url, "https://example.com/a");
    }

    #[test]
    fn test_select_discord_targets_filters_and_fallbacks() {
        let targets = vec![
            mk_target("page", "Discord Updater", "about:blank"),
            mk_target("page", "Discord", "https://discord.com/app"),
            mk_target("page", "Other", "https://discordapp.com/channels/@me"),
            mk_target("page", "Other Site", "https://example.com"),
        ];

        let selected = select_discord_targets(&targets);
        assert_eq!(selected.len(), 2);
        assert!(selected.iter().any(|t| t.url.contains("discord.com")));
        assert!(selected.iter().any(|t| t.url.contains("discordapp.com")));

        let no_match_targets = vec![
            mk_target("page", "Page A", "https://example.com/a"),
            mk_target("page", "Page B", "https://example.com/b"),
        ];
        let fallback = select_discord_targets(&no_match_targets);
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].url, "https://example.com/a");

        let with_missing_ws = vec![
            mk_target_opt_ws("page", "Discord Main", "https://discord.com/app", None),
            mk_target(
                "page",
                "Discord Secondary",
                "https://discordapp.com/channels/@me",
            ),
        ];
        let filtered = select_discord_targets(&with_missing_ws);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Discord Secondary");

        let fallback_missing_ws = vec![
            mk_target_opt_ws("page", "Page A", "https://example.com/a", None),
            mk_target_opt_ws("page", "Page B", "https://example.com/b", None),
        ];
        let fallback_none = select_discord_targets(&fallback_missing_ws);
        assert_eq!(fallback_none.len(), 0);
    }
}

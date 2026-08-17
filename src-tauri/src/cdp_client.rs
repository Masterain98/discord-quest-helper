//! CDP (Chrome DevTools Protocol) client for communicating with Discord
//!
//! Discord client based on Electron (Chromium), supports CDP protocol.
//! After starting Discord with the --remote-debugging-port parameter, it can communicate with the client via WebSocket.

use anyhow::{Context, Result};
use discord_cdp_launch_core::{is_discord_target, pick_discord_target, CdpTarget};
use futures_util::{future::join_all, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use zeroize::Zeroizing;

/// Default CDP debugging port
pub use discord_cdp_launch_core::DEFAULT_CDP_PORT;

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

/// Read-only snapshot of Discord's currently loaded game detector state.
///
/// The nested game values intentionally use `serde_json::Value` because
/// Discord's internal RunningGame schema changes independently of this app.
/// The CDP probe converts JavaScript `undefined` to the explicit string
/// `<undefined>` so the debug page can distinguish it from a missing key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpRunningGamesSnapshot {
    pub captured_at: u64,
    pub page_title: String,
    pub page_url: String,
    pub store_found: bool,
    pub store_path: Option<String>,
    pub native_module_found: bool,
    pub native_module_name: String,
    pub native_module_methods: Vec<String>,
    pub games: Vec<serde_json::Value>,
    pub visible_games: Vec<serde_json::Value>,
    pub native_diagnostics: Vec<serde_json::Value>,
    pub errors: Vec<String>,
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

fn select_discord_targets(targets: &[CdpTarget]) -> Vec<&CdpTarget> {
    targets
        .iter()
        .filter(|t| is_discord_target(t) && t.web_socket_debugger_url.is_some())
        .collect()
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

const JS_READ_RUNNING_GAMES: &str = r###"
(async () => {
  const result = {
    captured_at: Date.now(),
    page_title: document.title || "",
    page_url: location.href || "",
    store_found: false,
    store_path: null,
    native_module_found: false,
    native_module_name: "discord_utils",
    native_module_methods: [],
    games: [],
    visible_games: [],
    native_diagnostics: [],
    errors: []
  };

  const serialize = (value, seen = new WeakSet(), depth = 0) => {
    if (value === undefined) return "<undefined>";
    if (value === null || typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
      return value;
    }
    if (typeof value === "bigint") return String(value);
    if (depth > 6) return "<max-depth>";
    if (typeof value === "object") {
      if (seen.has(value)) return "<circular>";
      seen.add(value);
      if (Array.isArray(value)) return value.map(item => serialize(item, seen, depth + 1));
      const output = {};
      for (const key of Object.keys(value)) {
        try { output[key] = serialize(value[key], seen, depth + 1); }
        catch (error) { output[key] = "<read-error: " + String(error) + ">"; }
      }
      return output;
    }
    return String(value);
  };

  try {
    const native = window.DiscordNative && window.DiscordNative.nativeModules;
    if (native && typeof native.requireModule === "function") {
      try {
        const utils = native.requireModule("discord_utils");
        if (utils) {
          result.native_module_found = true;
          result.native_module_methods = Object.keys(utils).sort();
          result._native_utils = utils;
        }
      } catch (error) {
        result.errors.push("discord_utils: " + String(error));
      }
    } else {
      result.errors.push("DiscordNative.nativeModules.requireModule is unavailable");
    }
  } catch (error) {
    result.errors.push("native module discovery: " + String(error));
  }

  try {
    const chunk = window.webpackChunkdiscord_app;
    if (!chunk) {
      result.errors.push("webpackChunkdiscord_app is unavailable");
    } else {
      // Discord's webpack jsonp hook calls the runtime callback with a secondary
      // require whose module cache is tiny and does not contain Flux stores.
      // The push() return value of `r => r` is the real __webpack_require__.
      const webpackRequire = chunk.push([[Symbol()], {}, r => r]);
      chunk.pop();
      const cache = webpackRequire && webpackRequire.c;
      if (!cache) {
        result.errors.push("webpack require cache is unavailable");
      } else {
        for (const [moduleId, moduleValue] of Object.entries(cache)) {
          const exp = moduleValue && moduleValue.exports;
          if (!exp) continue;
          let found = false;
          for (const key of Object.keys(exp)) {
            let store;
            try { store = exp[key]; } catch (_) { continue; }
            // i18n modules also export getRunningGames, but they return
            // {locale, ast} instead of an Array.
            if (typeof store?.getRunningGames !== "function") continue;
            let games;
            try { games = store.getRunningGames(); } catch (_) { continue; }
            if (!Array.isArray(games)) continue;
            result.store_found = true;
            result.store_path = "module[" + moduleId + "].exports." + key;
            result.games = serialize(games);
            try {
              if (typeof store.getVisibleRunningGames === "function") {
                const rawVisibleGames = serialize(store.getVisibleRunningGames());
                if (Array.isArray(rawVisibleGames)) result.visible_games = rawVisibleGames;
                else result.errors.push("getVisibleRunningGames returned a non-array value");
              }
            } catch (error) {
              result.errors.push("getVisibleRunningGames: " + String(error));
            }
            found = true;
            break;
          }
          if (found) break;
        }
        if (!result.store_found) result.errors.push("RunningGameStore is not loaded in the selected Discord target");
      }
    }
  } catch (error) {
    result.errors.push("RunningGameStore discovery: " + String(error));
  }

  const utils = result._native_utils;
  delete result._native_utils;
  const games = Array.isArray(result.games) ? result.games : [];
  if (utils && typeof utils.getExecutableFingerprintForProcess === "function") {
    for (const game of games) {
      const pid = Number(game && game.pid);
      const diagnostic = { pid: Number.isFinite(pid) ? pid : null, fingerprint: "<unavailable>" };
      if (!Number.isInteger(pid) || pid <= 0) {
        diagnostic.error = "game.pid is not a positive integer";
        result.native_diagnostics.push(diagnostic);
        continue;
      }
      try {
        diagnostic.fingerprint = await new Promise(resolve => {
          let settled = false;
          const finish = value => {
            if (settled) return;
            settled = true;
            resolve(typeof value === "string" ? value : serialize(value));
          };
          try {
            utils.getExecutableFingerprintForProcess(pid, finish);
            setTimeout(() => finish("<timeout>"), 2500);
          } catch (error) {
            finish("<error: " + String(error) + ">");
          }
        });
        diagnostic.length = typeof diagnostic.fingerprint === "string" ? diagnostic.fingerprint.length : null;
      } catch (error) {
        diagnostic.error = String(error);
      }
      result.native_diagnostics.push(diagnostic);
    }
  } else if (games.length > 0) {
    result.errors.push("discord_utils.getExecutableFingerprintForProcess is unavailable");
  }

  return JSON.stringify(result);
})()
"###;

/// Read the currently loaded Discord game detector state without changing the
/// client, its stores, callbacks, or network state.
pub async fn fetch_running_games_via_cdp(port: u16) -> Result<CdpRunningGamesSnapshot> {
    let raw = execute_js_via_primary_discord_target(port, JS_READ_RUNNING_GAMES, true, 10).await?;
    serde_json::from_str(&raw).context("Failed to parse Discord running-games snapshot")
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
                            text.chars().take(200).collect::<String>()
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
            result_value.chars().take(100).collect::<String>()
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

    let duration_secs = duration_secs.clamp(5, 120);

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

/// An authenticated Discord session captured over CDP.
///
/// Capturing the raw `Authorization` token is the whole point of this type, so
/// it is deliberately hard to leak: it does **not** implement `Serialize`, its
/// `Debug` redacts the token, and the token is zeroized on drop. It never
/// crosses the Tauri IPC boundary — `auto_login_via_cdp` returns only the
/// resolved `DiscordUser`.
pub struct CapturedDiscordSession {
    /// Bearer/user token from a Discord API request's `Authorization` header.
    pub authorization: Zeroizing<String>,
    /// `x-super-properties` (base64) from the same request, when present.
    pub super_properties: Option<String>,
}

impl std::fmt::Debug for CapturedDiscordSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapturedDiscordSession")
            .field("authorization", &"[redacted]")
            .field(
                "super_properties",
                &self.super_properties.as_ref().map(|_| "[present]"),
            )
            .finish()
    }
}

/// Read the current Discord session from the client's in-memory auth store.
///
/// Discord's webpack export shape changes frequently, so inspect the root
/// export, its default export, and its named exports. The value travels only
/// over the loopback CDP WebSocket into Rust; it is never returned across Tauri
/// IPC or logged. Natural Network events remain the fallback if no compatible
/// auth store is found.
const JS_READ_AUTH_TOKEN: &str = r#"
(() => {
    try {
        if (typeof window === "undefined" || !window.webpackChunkdiscord_app) {
            return "";
        }
        const req = webpackChunkdiscord_app.push([[Symbol()], {}, r => r]);
        webpackChunkdiscord_app.pop();
        for (const m of Object.values(req.c)) {
            try {
                const root = m && m.exports;
                const candidates = [
                    root,
                    root && root.default,
                    ...(root && typeof root === "object" ? Object.values(root) : [])
                ];
                for (const candidate of candidates) {
                    if (!candidate || typeof candidate.getToken !== "function") continue;
                    const token = candidate.getToken();
                    if (typeof token === "string" && token.length > 20) return token;
                }
            } catch (e) { /* keep scanning */ }
        }
        return "";
    } catch (e) {
        return "";
    }
})()
"#;

const AUTH_TOKEN_EVALUATION_ID: u64 = 2;

/// Extract a non-empty string from our `Runtime.evaluate` response. Responses
/// to every other CDP command/event are ignored. The caller immediately wraps
/// the value in `Zeroizing` so it cannot leak through allocator reuse.
fn extract_auth_from_runtime_response(text: &str) -> Option<Zeroizing<String>> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    if json.get("id").and_then(|value| value.as_u64()) != Some(AUTH_TOKEN_EVALUATION_ID) {
        return None;
    }
    let token = json.get("result")?.get("result")?.get("value")?.as_str()?;
    if token.len() <= 20 || token == "undefined" {
        return None;
    }
    Some(Zeroizing::new(token.to_string()))
}

/// Case-insensitive header lookup that rejects empty / `undefined` values.
fn cdp_header_value(
    headers: &serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Option<String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.as_str())
        .map(str::to_string)
        .filter(|value| !value.is_empty() && value != "undefined")
}

/// True only for first-party Discord REST API requests (`https://<discord
/// host>/api/...`), excluding CDN and webhook URLs, so we never treat an
/// unrelated bearer token as the client's session.
fn is_discord_api_url(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let is_api_host = matches!(
        host.to_ascii_lowercase().as_str(),
        "discord.com" | "www.discord.com" | "canary.discord.com" | "ptb.discord.com"
    );
    if !is_api_host {
        return false;
    }
    let path = parsed.path();
    path.starts_with("/api/") && !path.starts_with("/api/webhooks") && !path.contains("/webhooks/")
}

/// Correlation state for the two CDP events that together describe one request.
///
/// Chromium emits `Network.requestWillBeSent` (which carries the URL) and
/// `Network.requestWillBeSentExtraInfo` (which carries the real, on-the-wire
/// header set) from different layers, with **no ordering guarantee** between
/// them. So ExtraInfo headers seen before their counterpart are buffered by
/// `requestId` and reconciled once the URL classification arrives.
#[derive(Default)]
struct CdpAuthCorrelator {
    /// `requestId` → whether it is a first-party Discord API request.
    classified: std::collections::HashMap<String, bool>,
    /// `requestId` → headers from an ExtraInfo event whose `requestWillBeSent`
    /// hasn't been seen yet.
    unmatched_extra_info: std::collections::HashMap<String, (Zeroizing<String>, Option<String>)>,
}

/// Cap on buffered, still-unclassified ExtraInfo headers. A capture window is a
/// few seconds long, so this is only a guard against a pathologically chatty
/// client; hitting it just means we fall back to the in-order path.
const MAX_UNMATCHED_EXTRA_INFO: usize = 256;

/// Cap on remembered request classifications. Correlation only matters between
/// an event pair that arrives back to back, so dropping the oldest generation
/// wholesale costs nothing in practice and keeps the map bounded however long
/// the capture window runs.
const MAX_CLASSIFIED_REQUESTS: usize = 1024;

/// Pull an `(authorization, super_properties)` pair out of a single CDP Network
/// event, correlating `...ExtraInfo` events back to a known Discord API request
/// by `requestId` regardless of which of the two events arrives first.
///
/// The authorization value stays inside `Zeroizing` end to end so the captured
/// session token is never left in a plain `String` the allocator can hand out
/// again — see [`CapturedDiscordSession`].
fn extract_auth_from_cdp_event(
    text: &str,
    state: &mut CdpAuthCorrelator,
) -> Option<(Zeroizing<String>, Option<String>)> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let method = json.get("method").and_then(|v| v.as_str())?;
    let params = json.get("params")?;

    match method {
        "Network.requestWillBeSent" => {
            let request_id = params
                .get("requestId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let request = params.get("request")?;
            let url = request.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let is_api = is_discord_api_url(url);

            let buffered = if request_id.is_empty() {
                None
            } else {
                if state.classified.len() >= MAX_CLASSIFIED_REQUESTS {
                    state.classified.clear();
                }
                state.classified.insert(request_id.to_string(), is_api);
                state.unmatched_extra_info.remove(request_id)
            };

            if !is_api {
                return None;
            }

            // The request's own header map often omits `authorization`; that is
            // exactly what ExtraInfo exists for, so fall back to the buffer.
            if let Some(headers) = request.get("headers").and_then(|h| h.as_object()) {
                if let Some(authorization) = cdp_header_value(headers, "authorization") {
                    return Some((
                        Zeroizing::new(authorization),
                        cdp_header_value(headers, "x-super-properties"),
                    ));
                }
            }

            buffered
        }
        "Network.requestWillBeSentExtraInfo" => {
            let request_id = params.get("requestId").and_then(|v| v.as_str())?;
            let headers = params.get("headers")?.as_object()?;
            let authorization = cdp_header_value(headers, "authorization")?;
            let super_properties = cdp_header_value(headers, "x-super-properties");

            match state.classified.get(request_id) {
                Some(true) => Some((Zeroizing::new(authorization), super_properties)),
                // Known non-API request: drop the headers, never buffer them.
                Some(false) => None,
                // Arrived first — hold on to it until the URL is classified.
                None => {
                    if state.unmatched_extra_info.len() < MAX_UNMATCHED_EXTRA_INFO {
                        state.unmatched_extra_info.insert(
                            request_id.to_string(),
                            (Zeroizing::new(authorization), super_properties),
                        );
                    }
                    None
                }
            }
        }
        _ => None,
    }
}

/// Capture the currently logged-in Discord session over CDP.
///
/// Enables the Network domain on the primary Discord target and watches for a
/// first-party authenticated API request. For idle clients, a Runtime query
/// reads Discord's in-memory auth store over the same loopback WebSocket. The
/// raw token never leaves Rust; callers validate it and keep it in the API
/// client only.
pub async fn capture_discord_auth_via_cdp(
    port: u16,
    timeout: Duration,
) -> Result<CapturedDiscordSession> {
    use crate::logger::{log, LogCategory, LogLevel};

    let targets = get_cdp_targets(port).await?;
    let target = pick_discord_target(&targets).context("No Discord target found")?;
    let ws_url = target
        .web_socket_debugger_url
        .as_ref()
        .context("Target has no WebSocket URL")?;

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!("CDP auto-login: connecting to target '{}'", target.title),
        None,
    );

    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("Failed to connect to CDP WebSocket")?;
    let (mut write, mut read) = ws_stream.split();

    write
        .send(Message::Text(
            serde_json::json!({ "id": 1, "method": "Network.enable", "params": {} })
                .to_string()
                .into(),
        ))
        .await
        .context("Failed to send Network.enable")?;

    // An idle Discord client may not emit any REST request during the capture
    // window. Query its in-memory auth store immediately; Network events below
    // remain a compatibility fallback for future webpack changes.
    write
        .send(Message::Text(
            serde_json::json!({
                "id": AUTH_TOKEN_EVALUATION_ID,
                "method": "Runtime.evaluate",
                "params": {
                    "expression": JS_READ_AUTH_TOKEN,
                    "returnByValue": true,
                    "awaitPromise": false
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .context("Failed to query Discord auth state over CDP")?;

    let mut correlator = CdpAuthCorrelator::default();

    let outcome = tokio::time::timeout(timeout, async {
        while let Some(msg) = read.next().await {
            let text = match msg {
                Ok(Message::Text(text)) => text,
                Ok(_) => continue,
                Err(error) => {
                    log(
                        LogLevel::Warn,
                        LogCategory::TokenExtraction,
                        &format!("CDP auto-login WebSocket receive failed: {error}"),
                        None,
                    );
                    break;
                }
            };
            if let Some(authorization) = extract_auth_from_runtime_response(&text) {
                return Some((authorization, None));
            }
            if let Some(found) = extract_auth_from_cdp_event(&text, &mut correlator) {
                return Some(found);
            }
        }
        None
    })
    .await
    .ok()
    .flatten();

    // Best-effort teardown — disable the Network domain and close cleanly.
    let _ = write
        .send(Message::Text(
            serde_json::json!({ "id": 3, "method": "Network.disable", "params": {} })
                .to_string()
                .into(),
        ))
        .await;
    let _ = write.close().await;

    match outcome {
        Some((authorization, super_properties)) => {
            log(
                LogLevel::Info,
                LogCategory::TokenExtraction,
                "CDP auto-login: captured an authenticated Discord API request",
                None,
            );
            Ok(CapturedDiscordSession {
                authorization,
                super_properties,
            })
        }
        None => anyhow::bail!(
            "No authenticated Discord API request was observed over CDP before the timeout. \
             Make sure Discord is running with CDP enabled and you are logged in."
        ),
    }
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

fn activity_target_host(target: &CdpTarget) -> Option<String> {
    reqwest::Url::parse(&target.url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
}

fn activity_target_application_id(target: &CdpTarget) -> Option<String> {
    let host = activity_target_host(target)?;
    host.strip_suffix(".discordsays.com")
        .filter(|prefix| !prefix.is_empty())
        .map(str::to_owned)
}

fn describe_activity_host_for_log(host: &str) -> String {
    if let Some(application_id) = host.strip_suffix(".discordsays.com") {
        if !application_id.is_empty() && application_id.chars().all(|value| value.is_ascii_digit())
        {
            return format!(
                "{}.discordsays.com",
                crate::logger::sanitize_user_id(application_id)
            );
        }
    }

    host.to_string()
}

fn is_activity_target(target: &CdpTarget) -> bool {
    let is_activity_host = activity_target_host(target)
        .map(|host| host == "discordsays.com" || host.ends_with(".discordsays.com"))
        .unwrap_or(false);

    (target.target_type == "iframe" || target.target_type == "page")
        && is_activity_host
        && target.web_socket_debugger_url.is_some()
}

fn is_activity_iframe_target(target: &CdpTarget) -> bool {
    target.target_type == "iframe"
}

fn describe_activity_targets(targets: &[CdpTarget]) -> String {
    if targets.is_empty() {
        return "none".to_string();
    }

    targets
        .iter()
        .map(|target| {
            let host = activity_target_host(target).unwrap_or_else(|| "unknown-host".to_string());
            format!(
                "{} ({})",
                describe_activity_host_for_log(&host),
                target.target_type
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Find the activity iframe CDP target (discordsays.com).
#[allow(dead_code)]
pub async fn find_activity_iframe_target(port: u16) -> Result<CdpTarget> {
    find_activity_iframe_target_for_application(port, None).await
}

/// Find the activity iframe CDP target for a specific Discord application.
pub async fn find_activity_iframe_target_for_application(
    port: u16,
    application_id: Option<&str>,
) -> Result<CdpTarget> {
    use crate::logger::{log, LogCategory, LogLevel};

    let targets = get_cdp_targets(port).await?;

    let activity_targets = targets
        .into_iter()
        .filter(is_activity_target)
        .collect::<Vec<_>>();

    if activity_targets.is_empty() {
        anyhow::bail!(
            "No activity iframe target found. Make sure the Activity is launched in Discord."
        );
    }

    let requested_application_id = application_id
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(app_id) = requested_application_id {
        let app_id_hint = crate::logger::sanitize_user_id(app_id);
        let matching_targets = activity_targets
            .iter()
            .filter(|target| activity_target_application_id(target).as_deref() == Some(app_id))
            .cloned()
            .collect::<Vec<_>>();

        if !matching_targets.is_empty() {
            if let Some(target) = matching_targets
                .iter()
                .find(|target| is_activity_iframe_target(target))
            {
                return Ok(target.clone());
            }

            // Fall back to any matching activity target (e.g. page type)
            // since some valid activities load as page targets, not iframes.
            if let Some(target) = matching_targets.first() {
                return Ok(target.clone());
            }

            anyhow::bail!(
                "No activity target matched application_id_hint={}. Found matching activity targets: {}",
                app_id_hint,
                describe_activity_targets(&matching_targets)
            );
        }

        anyhow::bail!(
            "No activity iframe target matched application_id_hint={}. Found activity targets: {}",
            app_id_hint,
            describe_activity_targets(&activity_targets)
        );
    }

    let iframe_targets = activity_targets
        .iter()
        .filter(|target| is_activity_iframe_target(target))
        .collect::<Vec<_>>();

    if let Some(target) = iframe_targets.first() {
        if activity_targets.len() > 1 {
            log(
                LogLevel::Warn,
                LogCategory::TokenExtraction,
                &format!(
                    "Multiple activity targets found with no application_id_hint; defaulting to first iframe target. activity_targets={}",
                    describe_activity_targets(&activity_targets)
                ),
                None,
            );
        }
        return Ok((*target).clone());
    }

    if activity_targets.len() > 1 {
        log(
            LogLevel::Warn,
            LogCategory::TokenExtraction,
            &format!(
                "Multiple non-iframe activity targets found with no application_id_hint; defaulting to first target. activity_targets={}",
                describe_activity_targets(&activity_targets)
            ),
            None,
        );
    }

    Ok(activity_targets[0].clone())
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
            result_value.chars().take(200).collect::<String>()
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
    fn running_games_js_uses_webpack_push_return_value_and_array_store() {
        assert!(
            JS_READ_RUNNING_GAMES.contains("{}, r => r]"),
            "must capture webpack require from push() return value, not the runtime callback argument"
        );
        assert!(JS_READ_RUNNING_GAMES.contains("Array.isArray(games)"));
        assert!(!JS_READ_RUNNING_GAMES
            .contains("runtimeRequire => { webpackRequire = runtimeRequire; }"));
        assert!(!JS_READ_RUNNING_GAMES
            .contains("if (typeof value.getRunningGames === \"function\") return { value, path }"));
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
    fn test_pick_discord_target_skips_overlay_popout() {
        let targets = vec![
            mk_target("page", "Discord Overlay", "https://discord.com/popout"),
            mk_target("page", "Friends", "https://discord.com/channels/@me"),
        ];

        let picked = pick_discord_target(&targets).unwrap();
        assert_eq!(picked.title, "Friends");
        assert!(picked.url.contains("/channels/"));
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
    fn test_pick_discord_target_does_not_fallback_to_unrelated_page() {
        let targets = vec![
            mk_target("page", "Unrelated Page 1", "https://example.com/a"),
            mk_target("page", "Unrelated Page 2", "https://example.com/b"),
        ];

        let picked = pick_discord_target(&targets);
        assert!(picked.is_none());
    }

    #[test]
    fn test_select_discord_targets_filters_without_unrelated_fallbacks() {
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
        assert!(fallback.is_empty());

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

    fn request_will_be_sent(request_id: &str, url: &str, authorization: Option<&str>) -> String {
        let headers = match authorization {
            Some(value) => serde_json::json!({ "authorization": value }),
            None => serde_json::json!({ "accept": "*/*" }),
        };
        serde_json::json!({
            "method": "Network.requestWillBeSent",
            "params": { "requestId": request_id, "request": { "url": url, "headers": headers } }
        })
        .to_string()
    }

    fn extra_info(request_id: &str, authorization: &str) -> String {
        serde_json::json!({
            "method": "Network.requestWillBeSentExtraInfo",
            "params": {
                "requestId": request_id,
                "headers": {
                    "authorization": authorization,
                    "x-super-properties": "eyJvcyI6ICJXaW5kb3dzIn0="
                }
            }
        })
        .to_string()
    }

    #[test]
    fn test_is_discord_api_url_rejects_cdn_and_webhooks() {
        assert!(is_discord_api_url("https://discord.com/api/v9/users/@me"));
        assert!(is_discord_api_url(
            "https://canary.discord.com/api/v9/users/@me"
        ));
        assert!(!is_discord_api_url("https://cdn.discordapp.com/api/v9/x"));
        assert!(!is_discord_api_url(
            "https://discord.com/api/webhooks/1/abc"
        ));
        assert!(!is_discord_api_url("http://discord.com/api/v9/users/@me"));
        assert!(!is_discord_api_url(
            "https://evil.example.com/api/v9/users/@me"
        ));
    }

    #[test]
    fn test_extract_auth_matches_extra_info_after_request() {
        let mut state = CdpAuthCorrelator::default();
        let api = request_will_be_sent("req-1", "https://discord.com/api/v9/users/@me", None);
        assert_eq!(extract_auth_from_cdp_event(&api, &mut state), None);

        let found = extract_auth_from_cdp_event(&extra_info("req-1", "token-a"), &mut state);
        assert_eq!(
            found.as_ref().map(|(auth, _)| auth.as_str()),
            Some("token-a")
        );
        assert!(found.unwrap().1.is_some());
    }

    #[test]
    fn test_extract_auth_buffers_extra_info_arriving_first() {
        let mut state = CdpAuthCorrelator::default();
        // CDP gives no ordering guarantee between the two events.
        assert_eq!(
            extract_auth_from_cdp_event(&extra_info("req-2", "token-b"), &mut state),
            None
        );

        let api = request_will_be_sent("req-2", "https://discord.com/api/v9/users/@me", None);
        let found = extract_auth_from_cdp_event(&api, &mut state);
        assert_eq!(
            found.as_ref().map(|(auth, _)| auth.as_str()),
            Some("token-b")
        );
        assert!(found.unwrap().1.is_some());
    }

    #[test]
    fn test_extract_auth_ignores_extra_info_for_non_api_requests() {
        let mut state = CdpAuthCorrelator::default();

        // ExtraInfo first, then a non-Discord-API URL: nothing must be emitted.
        assert_eq!(
            extract_auth_from_cdp_event(&extra_info("req-3", "not-a-session"), &mut state),
            None
        );
        let cdn = request_will_be_sent("req-3", "https://cdn.discordapp.com/avatars/1/a.png", None);
        assert_eq!(extract_auth_from_cdp_event(&cdn, &mut state), None);

        // The buffer must also be dropped, not replayed by a later ExtraInfo.
        assert_eq!(
            extract_auth_from_cdp_event(&extra_info("req-3", "not-a-session"), &mut state),
            None
        );
    }

    #[test]
    fn test_extract_auth_prefers_request_headers_when_present() {
        let mut state = CdpAuthCorrelator::default();
        let api = request_will_be_sent(
            "req-4",
            "https://discord.com/api/v9/users/@me",
            Some("token-inline"),
        );
        let found = extract_auth_from_cdp_event(&api, &mut state);
        assert_eq!(
            found.as_ref().map(|(auth, _)| auth.as_str()),
            Some("token-inline")
        );
    }

    #[test]
    fn test_extract_auth_from_runtime_response_accepts_only_our_evaluation() {
        let response = serde_json::json!({
            "id": AUTH_TOKEN_EVALUATION_ID,
            "result": { "result": { "type": "string", "value": "a-valid-looking-session-token" } }
        })
        .to_string();
        let token = extract_auth_from_runtime_response(&response).unwrap();
        assert_eq!(token.as_str(), "a-valid-looking-session-token");

        let unrelated = serde_json::json!({
            "id": 99,
            "result": { "result": { "type": "string", "value": "must-not-be-read" } }
        })
        .to_string();
        assert!(extract_auth_from_runtime_response(&unrelated).is_none());
    }

    #[test]
    fn test_extract_auth_from_runtime_response_rejects_empty_or_short_values() {
        for value in ["", "undefined", "too-short"] {
            let response = serde_json::json!({
                "id": AUTH_TOKEN_EVALUATION_ID,
                "result": { "result": { "type": "string", "value": value } }
            })
            .to_string();
            assert!(extract_auth_from_runtime_response(&response).is_none());
        }
    }
}

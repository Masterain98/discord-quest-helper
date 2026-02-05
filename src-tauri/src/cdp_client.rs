//! CDP (Chrome DevTools Protocol) client for communicating with Discord
//! 
//! Discord client based on Electron (Chromium), supports CDP protocol.
//! After starting Discord with the --remote-debugging-port parameter, it can communicate with the client via WebSocket.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};

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
        return JSON.stringify({ error: e.toString() });
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
        .timeout(Duration::from_secs(3))
        .build()?;
    
    let url = format!("http://127.0.0.1:{}/json", port);
    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to connect to CDP endpoint")?;
    
    let targets: Vec<CdpTarget> = response
        .json()
        .await
        .context("Failed to parse CDP targets")?;
    
    Ok(targets)
}

/// Select Discord main window target (skip updater)
fn pick_discord_target(targets: &[CdpTarget]) -> Option<&CdpTarget> {
    // Prioritize targets with type "page" and title containing "Discord" (but not "updater")
    let pages: Vec<_> = targets
        .iter()
        .filter(|t| t.target_type == "page")
        .collect();
    
    // Find Discord main application
    for target in &pages {
        let title_lower = target.title.to_lowercase();
        let url_lower = target.url.to_lowercase();
        
        if (title_lower.contains("discord") && !title_lower.contains("updater"))
            || url_lower.contains("discord.com")
        {
            return Some(target);
        }
    }
    
    // Fallback: return the first page
    pages.first().copied()
}

/// Get SuperProperties via CDP
pub async fn fetch_super_properties_via_cdp(port: u16) -> Result<CdpSuperProperties> {
    use crate::logger::{log, LogLevel, LogCategory};
    
    log(LogLevel::Info, LogCategory::TokenExtraction, 
        &format!("Attempting to fetch SuperProperties via CDP on port {}", port), None);
    
    // Get targets
    let targets = get_cdp_targets(port).await?;
    log(LogLevel::Debug, LogCategory::TokenExtraction, 
        &format!("Found {} CDP targets", targets.len()), None);
    
    let target = pick_discord_target(&targets)
        .context("No Discord target found")?;
    
    let ws_url = target
        .web_socket_debugger_url
        .as_ref()
        .context("Target has no WebSocket URL")?;
    
    log(LogLevel::Debug, LogCategory::TokenExtraction, 
        &format!("Connecting to CDP target: {} (URL: {})", target.title, ws_url), None);
    
    // Establish WebSocket connection
    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("Failed to connect to CDP WebSocket")?;
    
    log(LogLevel::Debug, LogCategory::TokenExtraction, 
        "WebSocket connection established", None);
    
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
    
    log(LogLevel::Debug, LogCategory::TokenExtraction, 
        "Sending Runtime.evaluate request", None);
    
    write
        .send(Message::Text(request.to_string().into()))
        .await
        .context("Failed to send CDP request")?;
    
    log(LogLevel::Debug, LogCategory::TokenExtraction, 
        "Request sent, waiting for response...", None);
    
    // Read response
    let response = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    log(LogLevel::Debug, LogCategory::TokenExtraction, 
                        &format!("Received message: {}...", &text.chars().take(200).collect::<String>()), None);
                    
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if json.get("id") == Some(&serde_json::json!(1)) {
                            return Ok(json);
                        }
                    }
                }
                Ok(other) => {
                    log(LogLevel::Debug, LogCategory::TokenExtraction, 
                        &format!("Received non-text message: {:?}", other), None);
                    continue;
                }
                Err(e) => {
                    log(LogLevel::Error, LogCategory::TokenExtraction, 
                        &format!("WebSocket error: {}", e), None);
                    return Err(anyhow::anyhow!("WebSocket error: {}", e));
                }
            }
        }
        log(LogLevel::Error, LogCategory::TokenExtraction, 
            "WebSocket closed unexpectedly", None);
        Err(anyhow::anyhow!("WebSocket closed unexpectedly"))
    })
    .await
    .context("CDP request timed out (10s)")??;
    
    log(LogLevel::Debug, LogCategory::TokenExtraction, 
        "Received valid CDP response", None);
    
    // Close connection
    let _ = write.close().await;
    
    // Parse response
    let result_value = response
        .get("result")
        .and_then(|r| r.get("result"))
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_str())
        .context("Invalid CDP response structure")?;
    
    log(LogLevel::Debug, LogCategory::TokenExtraction, 
        &format!("JavaScript returned: {}...", &result_value.chars().take(100).collect::<String>()), None);
    
    let parsed: serde_json::Value = serde_json::from_str(result_value)
        .context("Failed to parse JavaScript result")?;
    
    // Check for errors
    if let Some(error) = parsed.get("error") {
        log(LogLevel::Error, LogCategory::TokenExtraction, 
            &format!("JavaScript error: {}", error), None);
        anyhow::bail!("JavaScript error: {}", error);
    }
    
    let super_props: CdpSuperProperties = serde_json::from_value(parsed)
        .context("Failed to parse SuperProperties")?;
    
    log(LogLevel::Info, LogCategory::TokenExtraction, 
        &format!("Successfully fetched SuperProperties via CDP. Build number: {}", 
            super_props.decoded.get("client_build_number").and_then(|v| v.as_u64()).unwrap_or(0)), None);
    
    Ok(super_props)
}

#[cfg(test)]
mod tests {
    use super::*;
    
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
}

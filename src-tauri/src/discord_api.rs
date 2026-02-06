use crate::models::*;
use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use std::sync::Arc;

const DISCORD_API_BASE: &str = "https://discord.com/api/v9";
#[allow(dead_code)]
const USER_AGENT_STRING: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

/// Discord API client
#[derive(Clone)]
pub struct DiscordApiClient {
    client: Arc<reqwest::Client>,
    #[allow(dead_code)]
    token: String,
}

impl DiscordApiClient {
    /// Create a new API client
    pub fn new(token: String) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&token).context("Invalid token format")?,
        );
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/1.0.9219 Chrome/138.0.7204.251 Electron/37.6.0 Safari/537.36"),
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

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("Could not create HTTP client")?;

        Ok(Self {
            client: Arc::new(client),
            token,
        })
    }

    /// Get the current X-Super-Properties value (dynamically obtained to ensure latest data)
    fn get_super_properties_header(&self) -> HeaderValue {
        let super_props = {
            let manager = crate::SUPER_PROPERTIES_MANAGER
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            manager.get_super_properties_base64()
        };
        HeaderValue::from_str(&super_props).unwrap_or_else(|e| {
            eprintln!("Failed to create X-Super-Properties header: {}", e);
            // Fallback to minimal valid base64 JSON
            HeaderValue::from_static("e30=") // base64("{}")
        })
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
        
        let response = self.client
            .get(&url)
            .header("x-super-properties", self.get_super_properties_header())
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

    /// Get raw quest list data (via /quests/@me endpoint)
    pub async fn get_quests_raw(&self) -> Result<serde_json::Value> {
        let url = format!("{}/quests/@me", DISCORD_API_BASE);
        
        println!("Requesting quest list: {}", url);
        
        let response = self.client
            .get(&url)
            .header("x-super-properties", self.get_super_properties_header())
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

        let response = self.client
            .post(&url)
            .header("x-super-properties", self.get_super_properties_header())
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

        let response = self.client
            .post(&url)
            .header("x-super-properties", self.get_super_properties_header())
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

        let response = self.client
            .post(&url)
            .header("x-super-properties", self.get_super_properties_header())
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

        let response = self.client
            .post(&url)
            .header("x-super-properties", self.get_super_properties_header())
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
            let response = self.client
                .get(&url)
                .header("x-super-properties", self.get_super_properties_header())
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

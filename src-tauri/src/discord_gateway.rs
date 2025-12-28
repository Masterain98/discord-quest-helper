use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::models::Quest;

#[allow(dead_code)]
const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=9&encoding=json&compress=zlib-stream";

/// Discord Gateway opcodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum GatewayOpcode {
    Dispatch = 0,
    Heartbeat = 1,
    Identify = 2,
    Hello = 10,
    HeartbeatAck = 11,
}

/// Generic Gateway payload
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GatewayPayload {
    op: u8,
    #[serde(default)]
    t: Option<String>,
    #[serde(default)]
    d: Option<Value>,
}

/// Hello event data
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct HelloData {
    heartbeat_interval: u64,
}

/// Quest from READY payload
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ReadyQuest {
    id: String,
    config: ReadyQuestConfig,
    #[serde(default)]
    user_status: Option<ReadyQuestUserStatus>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ReadyQuestConfig {
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    messages: Option<ReadyQuestMessages>,
    #[serde(default)]
    application: Option<ReadyQuestApplication>,
    #[serde(default, rename = "taskConfig")]
    task_config: Option<Value>,
    #[serde(default, rename = "taskConfigV2")]
    task_config_v2: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ReadyQuestMessages {
    #[serde(default, rename = "questName")]
    quest_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ReadyQuestApplication {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ReadyQuestUserStatus {
    #[serde(default, rename = "enrolledAt")]
    enrolled_at: Option<String>,
    #[serde(default, rename = "completedAt")]
    completed_at: Option<String>,
    #[serde(default)]
    progress: Option<Value>,
}

#[allow(dead_code)]
pub async fn get_quests_from_gateway(token: &str) -> Result<Vec<Quest>> {
    println!("Connecting to Discord Gateway...");

    // Use non-compressed JSON mode for simplicity
    let gateway_url = "wss://gateway.discord.gg/?v=9&encoding=json";
    
    // Connect to Gateway
    let (ws_stream, _) = connect_async(gateway_url)
        .await
        .context("Could not connect to Discord Gateway")?;

    let (mut write, mut read) = ws_stream.split();

    // Read messages until we get READY
    let mut quests: Vec<Quest> = Vec::new();

    while let Some(msg) = read.next().await {
        let msg = msg.context("WebSocket message read error")?;

        match msg {
            Message::Text(utf8_text) => {
                let text: String = utf8_text.to_string();
                // Parse JSON directly
                if let Ok(payload) = serde_json::from_str::<GatewayPayload>(&text) {
                    println!("Received Gateway message: op={}, t={:?}", payload.op, payload.t);
                    
                    match payload.op {
                        10 => {
                            // HELLO
                            println!("Received HELLO event");

                            // Send Identify with better client properties
                            let identify = json!({
                                "op": 2,
                                "d": {
                                    "token": token,
                                    "capabilities": 30717,
                                    "properties": {
                                        "os": "Windows",
                                        "browser": "Discord Client",
                                        "device": "",
                                        "system_locale": "en-US",
                                        "browser_user_agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/1.0.9015 Chrome/108.0.5359.215 Electron/22.3.2 Safari/537.36",
                                        "browser_version": "22.3.2",
                                        "os_version": "10.0.19045",
                                        "referrer": "",
                                        "referring_domain": "",
                                        "referrer_current": "",
                                        "referring_domain_current": "",
                                        "release_channel": "stable",
                                        "client_build_number": 175517,
                                        "client_event_source": null
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
                            });

                            write
                                .send(Message::Text(identify.to_string().into()))
                                .await
                                .context("Failed to send Identify")?;

                            println!("Identify sent");
                        }
                        0 => {
                            // DISPATCH
                            if let Some(event_type) = &payload.t {
                                println!("Received DISPATCH event: {}", event_type);
                                
                                // Check for quests in various events
                                if let Some(d) = &payload.d {
                                    // Debug: print available keys for key events
                                    if event_type == "READY" || event_type == "READY_SUPPLEMENTAL" {
                                        if let Some(obj) = d.as_object() {
                                            println!("{} payload keys: {:?}", event_type, obj.keys().collect::<Vec<_>>());
                                        }
                                    }
                                    
                                    // Try to find quests in any event
                                    if let Some(quest_array) = d.get("quests") {
                                        println!("Found quests field in {} with {} items", 
                                            event_type,
                                            quest_array.as_array().map(|a| a.len()).unwrap_or(0));
                                        
                                        if let Ok(ready_quests) =
                                            serde_json::from_value::<Vec<ReadyQuest>>(
                                                quest_array.clone(),
                                            )
                                        {
                                            quests = ready_quests
                                                .into_iter()
                                                .map(convert_ready_quest_to_quest)
                                                .collect();
                                            println!(
                                                "Successfully parsed {} quests",
                                                quests.len()
                                            );
                                            
                                            // Found quests, close and return
                                            let _ = write.close().await;
                                            return Ok(quests);
                                        }
                                    }
                                }
                                
                                // After READY_SUPPLEMENTAL, if still no quests, return empty
                                if event_type == "READY_SUPPLEMENTAL" {
                                    println!("No quests in READY_SUPPLEMENTAL either, returning empty list");
                                    let _ = write.close().await;
                                    return Ok(quests);
                                }
                            }
                        }
                        11 => {
                            // HEARTBEAT_ACK
                            println!("Received heartbeat ack");
                        }
                        1 => {
                            // HEARTBEAT request from server
                            println!("Server requested heartbeat, sending...");
                            let heartbeat = json!({"op": 1, "d": null});
                            let _ = write.send(Message::Text(heartbeat.to_string().into())).await;
                        }
                        9 => {
                            // Invalid Session
                            println!("Invalid session (op=9)");
                            break;
                        }
                        7 => {
                            // Reconnect
                            println!("Server requested reconnect (op=7)");
                            break;
                        }
                        _ => {
                            println!("Received unknown opcode: {}", payload.op);
                        }
                    }
                } else {
                    println!("Could not parse JSON: {}", &text[..text.len().min(200)]);
                }
            }
            Message::Close(frame) => {
                println!("Gateway connection closed: {:?}", frame);
                break;
            }
            _ => {}
        }
    }

    Ok(quests)
}

#[allow(dead_code)]
fn convert_ready_quest_to_quest(rq: ReadyQuest) -> Quest {
    let config = &rq.config;
    let messages = config.messages.as_ref();
    let application = config.application.as_ref();
    let user_status = rq.user_status.as_ref();

    // Calculate progress from user_status
    let progress = user_status
        .and_then(|us| us.progress.as_ref())
        .and_then(|p| {
            // Try to extract progress value
            if let Some(obj) = p.as_object() {
                for (_, v) in obj {
                    if let Some(val) = v.get("value").and_then(|v| v.as_f64()) {
                        return Some(val);
                    }
                }
            }
            None
        })
        .unwrap_or(0.0);

    // Get seconds needed from task config
    let task_config = config
        .task_config_v2
        .as_ref()
        .or(config.task_config.as_ref());

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

    Quest {
        id: rq.id,
        name: messages
            .and_then(|m| m.quest_name.clone())
            .unwrap_or_else(|| "Unknown Quest".to_string()),
        description: String::new(),
        progress: progress / seconds_needed as f64 * 100.0,
        seconds_needed,
        task_type,
        application_id: application
            .and_then(|a| a.id.clone())
            .unwrap_or_default(),
        application_name: application
            .and_then(|a| a.name.clone())
            .unwrap_or_default(),
        application_icon: application.and_then(|a| a.icon.clone()),
        expires_at: config.expires_at.clone(),
        enrolled: user_status.and_then(|us| us.enrolled_at.clone()).is_some(),
        completed: user_status
            .and_then(|us| us.completed_at.clone())
            .is_some(),
    }
}

// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod discord_api;
mod discord_gateway;
mod game_simulator;
mod models;
mod quest_completer;
mod token_extractor;

use discord_api::DiscordApiClient;
use models::*;
use std::sync::Mutex;
use tauri::{Emitter, Listener, State};

/// Global state: Discord API client
struct AppState {
    client: Mutex<Option<DiscordApiClient>>,
    quest_state: Mutex<Option<QuestState>>,
}

/// Auto-detect Discord tokens (returns all valid accounts found)
#[tauri::command]
async fn auto_detect_token(_state: State<'_, AppState>) -> Result<Vec<ExtractedAccount>, String> {
    // Extract tokens
    let tokens = token_extractor::extract_tokens()
        .map_err(|e| format!("Token extraction failed: {}", e))?;

    let mut valid_accounts = Vec::new();
    let mut last_error = String::new();
    
    println!("Validating {} tokens...", tokens.len());

    for (index, token) in tokens.iter().enumerate() {
        println!("Validating token {}/{}", index + 1, tokens.len());
        // Create API client
        if let Ok(client) = DiscordApiClient::new(token.clone()) {
            // Validate token
            match client.get_current_user().await {
                Ok(user) => {
                    println!("Token {} valid", index);
                    valid_accounts.push(ExtractedAccount {
                        token: token.clone(),
                        user,
                    });
                }
                Err(e) => {
                    println!("Token {} invalid: {}", index, e);
                    last_error = format!("Token validation failed: {}", e);
                    // Continue to next token
                }
            }
        }
    }
    
    println!("Found {} valid accounts", valid_accounts.len());

    if valid_accounts.is_empty() {
        return Err(if !last_error.is_empty() { 
            format!("No valid accounts found. Last error: {}", last_error) 
        } else { 
            "No valid accounts found".to_string() 
        });
    }

    // Sort accounts? Maybe by username? Or keep order.
    
    Ok(valid_accounts)
}

/// Login with provided token
#[tauri::command]
async fn set_token(token: String, state: State<'_, AppState>) -> Result<DiscordUser, String> {
    // Create API client
    let client = DiscordApiClient::new(token)
        .map_err(|e| format!("Failed to create API client: {}", e))?;

    // Validate token
    let user = client
        .get_current_user()
        .await
        .map_err(|e| format!("Failed to validate token: {}", e))?;

    // Save client
    *state.client.lock().unwrap() = Some(client);

    Ok(user)
}

/// Get quest list (via HTTP API /quests/@me endpoint)
#[tauri::command]
async fn get_quests(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    let quests = client
        .get_quests_raw()
        .await
        .map_err(|e| format!("Failed to get quest list: {}", e))?;

    // Return the "quests" array directly
    Ok(quests.get("quests").cloned().unwrap_or(serde_json::Value::Array(vec![])))
}

/// Start video quest
#[tauri::command]
async fn start_video_quest(
    quest_id: String,
    seconds_needed: u32,
    initial_progress: f64,
    speed_multiplier: f64,
    heartbeat_interval: u64,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Stop current quest (if any)
    stop_quest_internal(&state).await;

    let client = state.client.lock().unwrap();
    let client = client
        .as_ref()
        .ok_or_else(|| "Not logged in".to_string())?
        .clone();

    // Create cancel channel
    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::channel::<()>(1);

    // Save quest state
    *state.quest_state.lock().unwrap() = Some(QuestState {
        quest_id: quest_id.clone(),
        cancel_flag: cancel_tx,
    });

    // Run in background task
    tokio::spawn(async move {
        let result = quest_completer::complete_video_quest(
            &client,
            quest_id,
            seconds_needed,
            initial_progress,
            speed_multiplier,
            heartbeat_interval,
            app_handle.clone(),
            cancel_rx,
        )
        .await;

        if let Err(e) = result {
            let _ = app_handle.emit("quest-error", format!("Video quest failed: {}", e));
        }
    });

    Ok(())
}

/// Start stream quest
#[tauri::command]
async fn start_stream_quest(
    quest_id: String,
    stream_key: String,
    seconds_needed: u32,
    initial_progress: f64,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Stop current quest (if any)
    stop_quest_internal(&state).await;

    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    // Create cancel channel
    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::channel::<()>(1);

    // Save quest state
    *state.quest_state.lock().unwrap() = Some(QuestState {
        quest_id: quest_id.clone(),
        cancel_flag: cancel_tx,
    });

    // Run in background task
    tokio::spawn(async move {
        let result = quest_completer::complete_stream_quest(
            &client,
            quest_id,
            stream_key,
            seconds_needed,
            initial_progress,
            app_handle.clone(),
            cancel_rx,
        )
        .await;

        if let Err(e) = result {
            let _ = app_handle.emit("quest-error", format!("Stream quest failed: {}", e));
        }
    });

    Ok(())
}

/// Start game quest via direct heartbeat (without running simulated game)
#[tauri::command]
async fn start_game_heartbeat_quest(
    quest_id: String,
    application_id: String,
    seconds_needed: u32,
    initial_progress: f64,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Stop current quest (if any)
    stop_quest_internal(&state).await;

    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    // Create cancel channel
    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::channel::<()>(1);

    // Save quest state
    *state.quest_state.lock().unwrap() = Some(QuestState {
        quest_id: quest_id.clone(),
        cancel_flag: cancel_tx,
    });

    // Run in background task
    tokio::spawn(async move {
        let result = quest_completer::complete_game_quest_via_heartbeat(
            &client,
            quest_id,
            application_id,
            seconds_needed,
            initial_progress,
            app_handle.clone(),
            cancel_rx,
        )
        .await;

        if let Err(e) = result {
            let _ = app_handle.emit("quest-error", format!("Game heartbeat quest failed: {}", e));
        }
    });

    Ok(())
}

/// Stop current quest
#[tauri::command]
async fn stop_quest(state: State<'_, AppState>) -> Result<(), String> {
    stop_quest_internal(&state).await;
    Ok(())
}

async fn stop_quest_internal(state: &State<'_, AppState>) {
    let quest = {
        let mut quest_state = state.quest_state.lock().unwrap();
        quest_state.take()
    };
    
    if let Some(quest) = quest {
        let _ = quest.cancel_flag.send(()).await;
        println!("Quest stopped");
    }
}

/// Create simulated game
#[tauri::command]
async fn create_simulated_game(
    path: String,
    executable_name: String,
    app_id: String,
) -> Result<(), String> {
    game_simulator::create_simulated_game(&path, &executable_name, &app_id)
        .map_err(|e| format!("Failed to create simulated game: {}", e))
}

/// Run simulated game
#[tauri::command]
async fn run_simulated_game(
    name: String,
    path: String,
    executable_name: String,
    app_id: String,
) -> Result<(), String> {
    game_simulator::run_simulated_game(&name, &path, &executable_name, &app_id)
        .map_err(|e| format!("Failed to run simulated game: {}", e))
}

/// Stop simulated game
#[tauri::command]
async fn stop_simulated_game(exec_name: String) -> Result<(), String> {
    game_simulator::stop_simulated_game(&exec_name)
        .map_err(|e| format!("Failed to stop simulated game: {}", e))
}

/// Get detectable games list
#[tauri::command]
async fn fetch_detectable_games(state: State<'_, AppState>) -> Result<Vec<DetectableGame>, String> {
    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    let games = client
        .fetch_detectable_games()
        .await
        .map_err(|e| format!("Failed to get games list: {}", e))?;

    Ok(games)
}

/// Accept quest
#[tauri::command]
async fn accept_quest(quest_id: String, state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    let result = client
        .accept_quest(&quest_id)
        .await
        .map_err(|e| format!("Failed to accept quest: {}", e))?;

    Ok(result)
}

mod rpc;
mod runner;

use once_cell::sync::OnceCell;
static DISCORD_RPC_CLIENT: OnceCell<Mutex<Option<rpc::Client>>> = OnceCell::new();

fn get_discord_rpc_client() -> &'static Mutex<Option<rpc::Client>> {
    DISCORD_RPC_CLIENT.get_or_init(|| Mutex::new(None))
}

#[tauri::command(rename_all = "snake_case")]
fn connect_to_discord_rpc(handle: tauri::AppHandle, activity_json: String, action: String) {
    let _ = action;
    let app = handle.clone();

    let event_connecting = "client_connecting";
    let event_connected = "client_connected";
    let event_disconnect = "event_disconnect";
    
    let activity = runner::parse_activity_json(&activity_json).unwrap();

    let connecting_payload = serde_json::json!({
        "app_id": activity.app_id,
    });

    // Clear existing client
    {
        let mut client_guard = get_discord_rpc_client().lock().unwrap();
        client_guard.take();
    }

    let task = tauri::async_runtime::spawn(async move {
        handle
            .emit(event_connecting, connecting_payload)
            .unwrap_or_else(|e| eprintln!("Failed to emit event: {}", e));

        let client_result = runner::set_activity(activity_json).await;
            
        match client_result {
            Ok(client) => {
                let connected_payload = serde_json::json!({
                    "app_id": activity.app_id,
                });

                {
                    let mut client_guard = get_discord_rpc_client().lock().unwrap();
                    *client_guard = Some(client);
                }

                handle
                    .emit(event_connected, connected_payload)
                    .unwrap_or_else(|e| {
                        eprintln!("Failed to emit event: {}", e);
                    });

                handle.listen(event_disconnect, move |_| {
                    println!("Disconnecting from Discord RPC inner");
                    let _ = tauri::async_runtime::spawn(async move {
                        let client_option = {
                            let mut client_guard = get_discord_rpc_client().lock().unwrap();
                            client_guard.take()
                        };
                        if let Some(client) = client_option {
                            client.discord.disconnect().await;
                            println!("Disconnected from Discord RPC inner");
                        }
                    });
                });
            },
            Err(e) => {
                println!("Failed to set activity: {}", e);
            }
        }
    });

    app.listen(event_disconnect, move |_| {
        println!("Disconnecting from Discord RPC...");
        task.abort();
    });
}

#[tauri::command]
async fn open_in_explorer(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let mut path = path.replace("/", "\\");
        // Explorer generally doesn't like the \\?\ prefix for opening folders
        if path.starts_with("\\\\?\\") {
            path = path[4..].to_string();
        }
        println!("Opening explorer at: {}", path);
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open explorer: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        println!("Opening Finder at: {}", path);
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open Finder: {}", e))?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = path; // Suppress unused variable warning on other platforms
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState {
            client: Mutex::new(None),
            quest_state: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            auto_detect_token,
            set_token,
            get_quests,
            start_video_quest,
            start_stream_quest,
            start_game_heartbeat_quest,
            stop_quest,
            create_simulated_game,
            run_simulated_game,
            stop_simulated_game,
            fetch_detectable_games,
            accept_quest,
            connect_to_discord_rpc,
            open_in_explorer,
            force_video_progress
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Force update video progress (used for ensuring final progress is saved on stop)
#[tauri::command]
async fn force_video_progress(
    quest_id: String,
    timestamp: f64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    client.update_video_progress(&quest_id, timestamp)
        .await
        .map_err(|e| format!("Failed to force video progress: {}", e))?;

    Ok(())
}

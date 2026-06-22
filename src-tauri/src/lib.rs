// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cdp_client;
mod cdp_quest;
mod discord_api;
pub mod discord_cdp_launcher;
mod discord_gateway;
mod game_simulator;
mod logger;
mod models;
mod quest_completer;
mod stealth;
mod super_properties;
mod token_extractor;

use discord_api::DiscordApiClient;
use models::*;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use super_properties::XSuperPropertiesManager;
use tauri::{Emitter, Listener, Manager, State};

/// Global X-Super-Properties manager (session-level)
/// Automatically generates key validation fields, fetches latest version info from Discord after login
static SUPER_PROPERTIES_MANAGER: Lazy<Mutex<XSuperPropertiesManager>> =
    Lazy::new(|| Mutex::new(XSuperPropertiesManager::new()));

/// Global state: Discord API client
struct AppState {
    client: Mutex<Option<DiscordApiClient>>,
    quest_state: Mutex<Option<QuestState>>,
}

/// Auto-detect Discord tokens (returns all valid accounts found)
#[tauri::command]
async fn auto_detect_token(_state: State<'_, AppState>) -> Result<Vec<ExtractedAccount>, String> {
    use crate::logger::{log, LogCategory, LogLevel};

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        "Starting auto token detection",
        None,
    );

    // Extract tokens
    let tokens = token_extractor::extract_tokens().map_err(|e| {
        log(
            LogLevel::Error,
            LogCategory::TokenExtraction,
            "Token extraction failed",
            Some(&e.to_string()),
        );
        format!("Token extraction failed: {}", e)
    })?;

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!("Extracted {} potential tokens", tokens.len()),
        None,
    );

    let mut valid_accounts = Vec::new();
    let mut last_error = String::new();

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!("Validating {} tokens", tokens.len()),
        None,
    );

    for (index, token) in tokens.iter().enumerate() {
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!("Validating token {}/{}", index + 1, tokens.len()),
            None,
        );
        // Create API client
        if let Ok(client) = DiscordApiClient::new(token.clone()) {
            // Validate token
            match client.get_current_user().await {
                Ok(user) => {
                    log(
                        LogLevel::Info,
                        LogCategory::TokenExtraction,
                        &format!("Token {} validated successfully", index + 1),
                        None,
                    );
                    valid_accounts.push(ExtractedAccount {
                        token: token.clone(),
                        user,
                    });
                }
                Err(e) => {
                    log(
                        LogLevel::Warn,
                        LogCategory::TokenExtraction,
                        &format!("Token {} validation failed", index + 1),
                        Some(&e.to_string()),
                    );
                    last_error = format!("Token validation failed: {}", e);
                    // Continue to next token
                }
            }
        }
    }

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "Token detection complete: {} valid accounts found",
            valid_accounts.len()
        ),
        None,
    );

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
    use crate::logger::{log, LogCategory, LogLevel};

    // Create API client
    let client =
        DiscordApiClient::new(token).map_err(|e| format!("Failed to create API client: {}", e))?;

    // Validate token
    let user = client
        .get_current_user()
        .await
        .map_err(|e| format!("Failed to validate token: {}", e))?;

    // Fetch latest build_number and client info before returning (so frontend await can rely on completion)

    // Priority 1: Try CDP
    let mut cdp_success = false;
    let cdp_port = cdp_client::DEFAULT_CDP_PORT;

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "Attempting to fetch SuperProperties via CDP on port {}",
            cdp_port
        ),
        None,
    );

    if let Ok(cdp_result) = cdp_client::fetch_super_properties_via_cdp(cdp_port).await {
        log(
            LogLevel::Info,
            LogCategory::TokenExtraction,
            &format!(
                "Successfully fetched SuperProperties via CDP. Build: {}",
                cdp_result
                    .decoded
                    .get("client_build_number")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
            ),
            None,
        );
        if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
            manager.set_from_cdp(&cdp_result.base64, &cdp_result.decoded);
        }
        cdp_success = true;
    } else {
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            "CDP fetch failed, falling back to JS scraping",
            None,
        );
    }

    // Priority 2: Remote JS (Fallback)
    if !cdp_success {
        // Get build_number
        match token_extractor::fetch_build_number_from_discord().await {
            Ok(build_number) => {
                log(
                    LogLevel::Info,
                    LogCategory::TokenExtraction,
                    &format!(
                        "Successfully fetched build number from JS: {}",
                        build_number
                    ),
                    None,
                );
                if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
                    manager.set_from_remote_js(build_number);
                }
            }
            Err(e) => {
                log(
                    LogLevel::Warn,
                    LogCategory::TokenExtraction,
                    &format!("Failed to fetch build number from JS: {}", e),
                    None,
                );
            }
        }
    }

    // Get client info (native_build_number and version)
    match token_extractor::fetch_discord_client_info().await {
        Ok(info) => {
            log(
                LogLevel::Info,
                LogCategory::TokenExtraction,
                &format!(
                    "Successfully fetched client info: version={}, native_build={}",
                    info.client_version(),
                    info.native_build_number
                ),
                None,
            );
            if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
                manager.set_client_info(info.client_version(), info.native_build_number);
            }
        }
        Err(e) => {
            log(
                LogLevel::Warn,
                LogCategory::TokenExtraction,
                &format!("Failed to fetch client info: {}", e),
                None,
            );
        }
    }

    // Save client AFTER initializing SuperProperties to avoid race conditions
    // where other commands might use the client with stale properties
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
    Ok(quests
        .get("quests")
        .cloned()
        .unwrap_or(serde_json::Value::Array(vec![])))
}

/// Get full quest list response, preserving excluded quests and enrollment block status.
#[tauri::command]
async fn get_quests_full(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    client
        .get_quests_raw()
        .await
        .map_err(|e| format!("Failed to get quest list: {}", e))
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

/// Start a quest via CDP injection
///
/// Dispatches to the appropriate CDP completion function based on quest_type.
#[tauri::command]
async fn start_cdp_quest(
    quest_id: String,
    quest_type: String,
    application_id: String,
    application_name: String,
    seconds_needed: u32,
    initial_progress: f64,
    cdp_port: u16,
    checkpoint_times: Option<Vec<u32>>,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Stop current quest (if any)
    stop_quest_internal(&state).await;

    // Create cancel channel
    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::channel::<()>(1);

    // Save quest state
    *state.quest_state.lock().unwrap() = Some(QuestState {
        quest_id: quest_id.clone(),
        cancel_flag: cancel_tx,
    });

    let quest_type_clone = quest_type.clone();

    // Clone the API client for progress polling (play/stream quests)
    let client = state.client.lock().unwrap().clone();

    // Run in background task
    tokio::spawn(async move {
        let result = match quest_type_clone.as_str() {
            "play" => {
                cdp_quest::complete_play_quest_via_cdp(
                    cdp_port,
                    quest_id,
                    application_id,
                    application_name,
                    seconds_needed,
                    initial_progress,
                    client,
                    app_handle.clone(),
                    cancel_rx,
                )
                .await
            }
            "stream" => {
                cdp_quest::complete_stream_quest_via_cdp(
                    cdp_port,
                    quest_id,
                    application_id,
                    seconds_needed,
                    initial_progress,
                    client,
                    app_handle.clone(),
                    cancel_rx,
                )
                .await
            }
            "video" => {
                cdp_quest::complete_video_quest_via_cdp(
                    cdp_port,
                    quest_id,
                    seconds_needed,
                    initial_progress,
                    app_handle.clone(),
                    cancel_rx,
                )
                .await
            }
            "activity" => {
                let times = checkpoint_times
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| vec![180, 180, 180]);
                cdp_quest::complete_activity_quest_via_cdp(
                    cdp_port,
                    quest_id,
                    times,
                    app_handle.clone(),
                    cancel_rx,
                )
                .await
            }
            _ => Err(anyhow::anyhow!(
                "Unknown CDP quest type: {}",
                quest_type_clone
            )),
        };

        if let Err(e) = result {
            let _ = app_handle.emit("quest-error", format!("CDP quest failed: {:#}", e));
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

/// Navigate Discord client SPA to a specific path (no reload)
#[tauri::command]
async fn navigate_discord_spa(target_path: String, cdp_port: u16) -> Result<(), String> {
    cdp_quest::navigate_discord_spa(cdp_port, &target_path)
        .await
        .map_err(|e| format!("Failed to navigate Discord SPA: {}", e))
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

/// Get detectable games list (works with or without login)
#[tauri::command]
async fn fetch_detectable_games(state: State<'_, AppState>) -> Result<Vec<DetectableGame>, String> {
    // Use the authenticated client when available (carries auth headers + super-properties).
    // When not logged in, fall back to a plain public HTTP request — the detectable-games
    // endpoints require no authentication.
    let auth_client = {
        let guard = state.client.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(client) = auth_client {
        return client
            .fetch_detectable_games()
            .await
            .map_err(|e| format!("Failed to get games list: {}", e));
    }

    // ── Unauthenticated fallback ──────────────────────────────────────────
    let http = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/1.0.9219 Chrome/138.0.7204.251 Electron/37.6.0 Safari/537.36")
        .connect_timeout(std::time::Duration::from_secs(8))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    const API_BASE: &str = "https://discord.com/api/v9";
    let games_url = format!("{}/applications/detectable", API_BASE);
    let apps_url = format!("{}/applications/non-games/detectable", API_BASE);

    let (games_res, apps_res) =
        tokio::join!(http.get(&games_url).send(), http.get(&apps_url).send());

    let mut all_items: Vec<DetectableGame> = Vec::new();

    if let Ok(resp) = games_res {
        if resp.status().is_success() {
            if let Ok(mut list) = resp.json::<Vec<DetectableGame>>().await {
                for g in &mut list {
                    g.type_name = Some("Game".to_string());
                }
                all_items.extend(list);
            }
        }
    }

    if let Ok(resp) = apps_res {
        if resp.status().is_success() {
            if let Ok(mut list) = resp.json::<Vec<DetectableGame>>().await {
                for a in &mut list {
                    a.type_name = Some("App".to_string());
                }
                all_items.extend(list);
            }
        }
    }

    Ok(all_items)
}

/// Accept quest
#[tauri::command]
async fn accept_quest(
    quest_id: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
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

#[tauri::command]
async fn get_virtual_currency_balance(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    client
        .get_virtual_currency_balance()
        .await
        .map_err(|e| format!("Failed to get virtual currency balance: {}", e))
}

#[tauri::command]
async fn get_quest_decision_debug(
    placement: u64,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    client
        .get_quest_decision_debug(placement)
        .await
        .map_err(|e| format!("Failed to get quest placement decision: {}", e))
}

#[tauri::command]
async fn get_quest_decisions_debug(
    placement: u64,
    num: u64,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    client
        .get_quest_decisions_debug(placement, num)
        .await
        .map_err(|e| format!("Failed to get quest placement decisions: {}", e))
}

#[tauri::command]
async fn claim_quest_reward(
    quest_id: String,
    platform: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    client
        .claim_quest_reward(&quest_id, platform)
        .await
        .map_err(|e| format!("Failed to claim quest reward: {}", e))
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
            }
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

/// Ensure stealth mode and run application
///
/// This is the new entry point that replaces direct run() call
pub fn ensure_stealth_and_run() {
    // Try to enter stealth mode
    stealth::ensure_stealth_mode();

    // Set up cleanup hook for panics with recursion guard
    use std::sync::atomic::{AtomicBool, Ordering};
    static CLEANUP_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        if !CLEANUP_IN_PROGRESS.swap(true, Ordering::SeqCst) {
            // Use catch_unwind to safely run cleanup
            let cleanup_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                stealth::cleanup_on_exit();
            }));

            if cleanup_result.is_err() {
                eprintln!("[Stealth] Error: panic occurred during cleanup in panic hook");
            }

            // Do NOT reset flag - if we panicked, we don't want to try cleaning up again
            // CLEANUP_IN_PROGRESS.store(false, Ordering::SeqCst);
        }
        // Wrap original_hook call in catch_unwind to prevent nested panics
        let hook_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            original_hook(panic_info);
        }));
        if hook_result.is_err() {
            eprintln!("[Stealth] Error: original panic hook panicked");
        }
    }));

    // Register Ctrl+C handler
    if let Err(e) = ctrlc::set_handler(move || {
        // Kill all simulated game child processes before exiting
        let cleanup_games_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            game_simulator::cleanup_all_simulated_games();
        }));
        if cleanup_games_result.is_err() {
            eprintln!("[Cleanup] Error: panic during game cleanup in Ctrl+C handler");
        }

        // Wrap stealth cleanup in catch_unwind to log any errors before exiting
        let cleanup_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stealth::cleanup_on_exit();
        }));
        if cleanup_result.is_err() {
            eprintln!("[Stealth] Error: panic occurred during cleanup in Ctrl+C handler");
        }
        std::process::exit(0);
    }) {
        eprintln!("Warning: Failed to register Ctrl+C handler: {}", e);
    }

    // Run main application
    run();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            client: Mutex::new(None),
            quest_state: Mutex::new(None),
        })
        .setup(|app| {
            // Set random window title in stealth mode
            if stealth::is_stealth_mode() {
                if let Some(window) = app.get_webview_window("main") {
                    let stealth_title = stealth::generate_stealth_window_title();
                    println!("[Stealth] Setting window title to: {}", stealth_title);
                    if let Err(err) = window.set_title(&stealth_title) {
                        eprintln!(
                            "[Stealth] Failed to set window title to '{}': {}",
                            stealth_title, err
                        );
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auto_detect_token,
            set_token,
            get_quests,
            get_quests_full,
            start_video_quest,
            start_stream_quest,
            start_game_heartbeat_quest,
            start_cdp_quest,
            stop_quest,
            create_simulated_game,
            run_simulated_game,
            stop_simulated_game,
            fetch_detectable_games,
            accept_quest,
            get_virtual_currency_balance,
            get_quest_decision_debug,
            get_quest_decisions_debug,
            claim_quest_reward,
            connect_to_discord_rpc,
            open_in_explorer,
            force_video_progress,
            export_logs,
            get_debug_info,
            get_runner_info,
            check_cdp_status,
            fetch_super_properties_cdp,
            is_discord_running,
            launch_discord_cdp,
            restart_discord_cdp,
            install_discord_cdp_launcher,
            create_discord_cdp_launcher_shortcut,
            create_discord_debug_shortcut,
            get_super_properties_mode,
            auto_fetch_super_properties,
            retry_super_properties,
            capture_discord_headers_cdp,
            navigate_discord_spa
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Stop all simulated game processes that were started by this app.
                // When the main app exits the RPC connection drops, so the child
                // processes become useless — kill them to avoid orphaned runners.
                game_simulator::cleanup_all_simulated_games();

                // Disconnect Discord RPC client (if connected)
                {
                    let client_option = {
                        let mut guard = get_discord_rpc_client().lock().unwrap();
                        guard.take()
                    };
                    if let Some(client) = client_option {
                        // Fire-and-forget async disconnect
                        tauri::async_runtime::spawn(async move {
                            client.discord.disconnect().await;
                            println!("Discord RPC disconnected on app exit");
                        });
                    }
                }

                // Clean up stealth mode artifacts
                stealth::cleanup_on_exit();
            }
        })
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

    client
        .update_video_progress(&quest_id, timestamp)
        .await
        .map_err(|e| format!("Failed to force video progress: {}", e))?;

    Ok(())
}

/// Export application logs as JSON
#[tauri::command]
async fn export_logs() -> Result<String, String> {
    logger::export_logs().map_err(|e| format!("Failed to export logs: {}", e))
}

/// Get debug info including X-Super-Properties
#[tauri::command]
async fn get_debug_info() -> Result<super_properties::DebugInfo, String> {
    let manager = SUPER_PROPERTIES_MANAGER.lock().map_err(|e| e.to_string())?;
    Ok(manager.get_debug_info())
}

/// Get embedded runner version information
#[tauri::command]
async fn get_runner_info() -> game_simulator::RunnerInfo {
    game_simulator::get_runner_info()
}

/// Check CDP status
#[tauri::command]
async fn check_cdp_status(port: Option<u16>) -> cdp_client::CdpStatus {
    let port = port.unwrap_or(cdp_client::DEFAULT_CDP_PORT);
    cdp_client::check_cdp_available(port).await
}

/// Fetch SuperProperties via CDP
#[tauri::command]
async fn fetch_super_properties_cdp(
    port: Option<u16>,
) -> Result<cdp_client::CdpSuperProperties, String> {
    let port = port.unwrap_or(cdp_client::DEFAULT_CDP_PORT);
    let result = cdp_client::fetch_super_properties_via_cdp(port)
        .await
        .map_err(|e| e.to_string())?;

    // Update global SuperProperties Manager
    if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
        manager.set_from_cdp(&result.base64, &result.decoded);
    }

    Ok(result)
}

/// Capture Discord API request headers via CDP Network interception
#[tauri::command]
async fn capture_discord_headers_cdp(
    port: Option<u16>,
    duration_secs: Option<u64>,
) -> Result<cdp_client::CdpCapturedHeaders, String> {
    let port = port.unwrap_or(cdp_client::DEFAULT_CDP_PORT);
    let duration = duration_secs.unwrap_or(30);
    let captured = cdp_client::capture_discord_headers_via_cdp(port, duration)
        .await
        .map_err(|e| e.to_string())?;

    let mut manager = SUPER_PROPERTIES_MANAGER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for request in &captured.requests {
        manager.update_header_profile_from_headers(&request.headers);
    }

    Ok(captured)
}

/// Get current SuperProperties source mode and build number
#[tauri::command]
fn get_super_properties_mode() -> serde_json::Value {
    let manager = SUPER_PROPERTIES_MANAGER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    serde_json::json!({
        "mode": manager.get_mode().as_str(),
        "mode_display": manager.get_mode().display_name(),
        "build_number": manager.get_build_number()
    })
}

/// Auto-fetch SuperProperties with fallback: CDP -> Remote JS -> Default
#[tauri::command]
async fn auto_fetch_super_properties(cdp_port: Option<u16>) -> serde_json::Value {
    use crate::logger::{log, LogCategory, LogLevel};

    let port = cdp_port.unwrap_or(cdp_client::DEFAULT_CDP_PORT);

    // Priority 1: Try CDP
    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!("Auto-fetching SuperProperties, trying CDP on port {}", port),
        None,
    );

    if let Ok(cdp_result) = cdp_client::fetch_super_properties_via_cdp(port).await {
        if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
            manager.set_from_cdp(&cdp_result.base64, &cdp_result.decoded);
            log(
                LogLevel::Info,
                LogCategory::TokenExtraction,
                &format!(
                    "SuperProperties obtained via CDP. Build: {:?}",
                    manager.get_build_number()
                ),
                None,
            );
            return serde_json::json!({
                "success": true,
                "mode": "cdp",
                "build_number": manager.get_build_number()
            });
        }
    }

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        "CDP failed, falling back to Remote JS",
        None,
    );

    // Priority 2: Try Remote JS
    if let Ok(build_number) = token_extractor::fetch_build_number_from_discord().await {
        if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
            manager.set_from_remote_js(build_number);
            log(
                LogLevel::Info,
                LogCategory::TokenExtraction,
                &format!(
                    "SuperProperties obtained via Remote JS. Build: {}",
                    build_number
                ),
                None,
            );
            return serde_json::json!({
                "success": true,
                "mode": "remote_js",
                "build_number": build_number
            });
        }
    }

    log(
        LogLevel::Warn,
        LogCategory::TokenExtraction,
        "All fetch methods failed, using default values",
        None,
    );

    // Priority 3: Use default values
    let build_number = if let Ok(manager) = SUPER_PROPERTIES_MANAGER.lock() {
        manager.get_build_number()
    } else {
        None
    };

    serde_json::json!({
        "success": false,
        "mode": "default",
        "build_number": build_number
    })
}

/// Retry fetching SuperProperties (resets and tries again)
#[tauri::command]
async fn retry_super_properties(cdp_port: Option<u16>) -> serde_json::Value {
    // Reset state
    if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
        manager.reset();
    }

    // Retry fetch
    auto_fetch_super_properties(cdp_port).await
}

#[tauri::command]
fn is_discord_running(channel: Option<String>) -> Result<bool, String> {
    let channel = discord_cdp_launcher::parse_discord_channel(channel.as_deref())?;
    discord_cdp_launcher::is_discord_running(channel)
}

#[tauri::command]
async fn launch_discord_cdp(
    port: Option<u16>,
    channel: Option<String>,
) -> Result<discord_cdp_launcher::LaunchResult, String> {
    let channel = discord_cdp_launcher::parse_discord_channel(channel.as_deref())?;
    discord_cdp_launcher::launch_discord_with_cdp(discord_cdp_launcher::LaunchOptions {
        port: port.unwrap_or(cdp_client::DEFAULT_CDP_PORT),
        channel,
        restart_existing: false,
        ..Default::default()
    })
    .await
}

#[tauri::command]
async fn restart_discord_cdp(
    port: Option<u16>,
    channel: Option<String>,
) -> Result<discord_cdp_launcher::LaunchResult, String> {
    let channel = discord_cdp_launcher::parse_discord_channel(channel.as_deref())?;
    discord_cdp_launcher::restart_discord_with_cdp(discord_cdp_launcher::LaunchOptions {
        port: port.unwrap_or(cdp_client::DEFAULT_CDP_PORT),
        channel,
        ..Default::default()
    })
    .await
}

#[tauri::command]
async fn install_discord_cdp_launcher(app_handle: tauri::AppHandle) -> Result<String, String> {
    install_discord_cdp_launcher_internal(&app_handle)
        .await
        .map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
async fn create_discord_cdp_launcher_shortcut(
    app_handle: tauri::AppHandle,
    port: Option<u16>,
    channel: Option<String>,
) -> Result<String, String> {
    let channel = discord_cdp_launcher::parse_discord_channel(channel.as_deref())?;
    let port = port.unwrap_or(cdp_client::DEFAULT_CDP_PORT);
    create_discord_cdp_launcher_shortcut_internal(&app_handle, port, channel).await
}

/// Backward compatible command name. It now creates a long-lived CDP launcher shortcut.
#[tauri::command]
async fn create_discord_debug_shortcut(
    app_handle: tauri::AppHandle,
    port: Option<u16>,
) -> Result<String, String> {
    create_discord_cdp_launcher_shortcut_internal(
        &app_handle,
        port.unwrap_or(cdp_client::DEFAULT_CDP_PORT),
        None,
    )
    .await
}

async fn install_discord_cdp_launcher_internal(
    app_handle: &tauri::AppHandle,
) -> Result<std::path::PathBuf, String> {
    use std::fs;

    let source = find_bundled_cdp_launcher(app_handle)?;
    let target = stable_cdp_launcher_path()?;

    let source_size = fs::metadata(&source)
        .map(|m| m.len())
        .unwrap_or(0);
    println!(
        "[cdp-launcher-install] source='{}' ({} bytes), target='{}'",
        source.display(),
        source_size,
        target.display()
    );

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create CDP launcher directory: {}", e))?;
    }

    if source != target {
        fs::copy(&source, &target).map_err(|e| {
            format!(
                "Failed to install CDP launcher to stable path from '{}' to '{}': {}",
                source.display(),
                target.display(),
                e
            )
        })?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to mark CDP launcher executable: {}", e))?;
    }

    Ok(target)
}

fn stable_cdp_launcher_path() -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        let local_appdata = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| "Could not get LOCALAPPDATA".to_string())?;
        return Ok(std::path::PathBuf::from(local_appdata)
            .join("DiscordQuestHelper")
            .join("DiscordCdpLauncher.exe"));
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| "Could not get HOME".to_string())?;
        return Ok(std::path::PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Discord Quest Helper")
            .join("discord-cdp-launcher"));
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err("Discord CDP launcher is only supported on Windows and macOS.".to_string())
    }
}

fn find_bundled_cdp_launcher(app_handle: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let names = cdp_launcher_binary_names();
    let mut candidate_dirs = Vec::new();

    // Dev mode: cwd-based paths (cwd is typically the repo root during `tauri dev`)
    if let Ok(cwd) = std::env::current_dir() {
        candidate_dirs.push(cwd.join("src-tauri").join("binaries"));
        candidate_dirs.push(cwd.join("binaries"));
    }

    // Release / packaged mode: resource_dir and exe-relative paths
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        candidate_dirs.push(resource_dir.clone());
        candidate_dirs.push(resource_dir.join("binaries"));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidate_dirs.push(parent.to_path_buf());
            candidate_dirs.push(parent.join("binaries"));
            #[cfg(target_os = "macos")]
            candidate_dirs.push(parent.join("../Resources"));
        }
    }

    for dir in &candidate_dirs {
        for name in &names {
            let candidate = dir.join(name);
            if candidate.exists() {
                // Reject empty placeholder files (created by build.rs for fresh checkouts)
                match std::fs::metadata(&candidate) {
                    Ok(m) if m.len() == 0 => continue,
                    Err(_) => continue,
                    _ => return Ok(candidate),
                }
            }
        }
    }

    let searched: Vec<String> = candidate_dirs
        .iter()
        .map(|d| d.display().to_string())
        .collect();
    Err(format!(
        "Failed to find bundled CDP launcher (names: {:?}, searched: {:?}). \
         Run `pnpm build:cdp-launcher` and try again.",
        names, searched
    ))
}

fn cdp_launcher_binary_names() -> Vec<&'static str> {
    #[cfg(target_os = "windows")]
    {
        return vec!["discord-cdp-launcher-sidecar-x86_64-pc-windows-msvc.exe"];
    }

    #[cfg(target_os = "macos")]
    {
        #[cfg(target_arch = "aarch64")]
        {
            return vec!["discord-cdp-launcher-sidecar-aarch64-apple-darwin"];
        }
        #[cfg(target_arch = "x86_64")]
        {
            return vec!["discord-cdp-launcher-sidecar-x86_64-apple-darwin"];
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Vec::new()
    }
}

async fn create_discord_cdp_launcher_shortcut_internal(
    app_handle: &tauri::AppHandle,
    port: u16,
    channel: Option<discord_cdp_launcher::DiscordChannel>,
) -> Result<String, String> {
    let launcher_path = install_discord_cdp_launcher_internal(app_handle).await?;
    create_platform_cdp_launcher_shortcut(&launcher_path, port, channel)
}

#[cfg(target_os = "windows")]
fn create_platform_cdp_launcher_shortcut(
    launcher_path: &std::path::Path,
    port: u16,
    channel: Option<discord_cdp_launcher::DiscordChannel>,
) -> Result<String, String> {
    use std::path::PathBuf;
    use std::process::Command;

    let desktop = std::env::var("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Desktop"))
        .map_err(|_| "Could not get desktop path".to_string())?;

    let shortcut_path = desktop.join("Discord CDP Launcher.lnk");
    let legacy_shortcut_path = desktop.join("Discord (Debug Mode).lnk");
    let launcher_dir = launcher_path
        .parent()
        .ok_or_else(|| "Could not get launcher directory".to_string())?;
    let channel_arg = channel.map(|c| c.as_str()).unwrap_or("auto");
    let args = format!("--port {} --channel {} --wait-cdp", port, channel_arg);

    let shortcut_path_ps = ps_single_quote(&shortcut_path.to_string_lossy());
    let legacy_shortcut_path_ps = ps_single_quote(&legacy_shortcut_path.to_string_lossy());
    let launcher_path_ps = ps_single_quote(&launcher_path.to_string_lossy());
    let launcher_dir_ps = ps_single_quote(&launcher_dir.to_string_lossy());
    let args_ps = ps_single_quote(&args);

    let ps_script = format!(
        r#"
$ErrorActionPreference = 'Stop'
$WshShell = New-Object -ComObject WScript.Shell
function Set-CdpShortcut($Path) {{
  $Shortcut = $WshShell.CreateShortcut($Path)
  $Shortcut.TargetPath = '{launcher_path}'
  $Shortcut.Arguments = '{args}'
  $Shortcut.WorkingDirectory = '{launcher_dir}'
  $Shortcut.Description = 'Launch Discord with CDP enabled for Discord Quest Helper'
  $Shortcut.IconLocation = '{launcher_path},0'
  $Shortcut.Save()
}}
Set-CdpShortcut '{shortcut_path}'
if (Test-Path -LiteralPath '{legacy_shortcut_path}') {{
  Set-CdpShortcut '{legacy_shortcut_path}'
}}
"#,
        launcher_path = launcher_path_ps,
        args = args_ps,
        launcher_dir = launcher_dir_ps,
        shortcut_path = shortcut_path_ps,
        legacy_shortcut_path = legacy_shortcut_path_ps,
    );

    let script_path = std::env::temp_dir().join(format!(
        "discord_cdp_launcher_shortcut_{}.ps1",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&script_path, &ps_script)
        .map_err(|e| format!("Failed to write temporary PowerShell script: {}", e))?;

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script_path.to_string_lossy(),
        ])
        .output();

    let _ = std::fs::remove_file(&script_path);
    let output = output.map_err(|e| format!("Failed to execute PowerShell: {}", e))?;

    if output.status.success() {
        Ok(shortcut_path.to_string_lossy().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "Failed to create desktop shortcut: {}",
            stderr.trim()
        ))
    }
}

#[cfg(target_os = "windows")]
fn ps_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(target_os = "macos")]
fn create_platform_cdp_launcher_shortcut(
    launcher_path: &std::path::Path,
    port: u16,
    channel: Option<discord_cdp_launcher::DiscordChannel>,
) -> Result<String, String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let home = std::env::var_os("HOME").ok_or_else(|| "Could not get HOME".to_string())?;
    let desktop = std::path::PathBuf::from(home).join("Desktop");
    let script_path = desktop.join("Discord CDP Launcher.command");
    let legacy_script_path = desktop.join("Discord Debug Mode.command");
    let channel_arg = channel.map(|c| c.as_str()).unwrap_or("auto");

    // Use single quotes to prevent shell metacharacter expansion ($, `, \, ")
    fn shell_single_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    let script_content = format!(
        "#!/bin/bash\n{} --port {} --channel {} --wait-cdp\n",
        shell_single_quote(&launcher_path.to_string_lossy()),
        port,
        channel_arg
    );

    std::fs::write(&script_path, &script_content)
        .map_err(|e| format!("Failed to write launcher command: {}", e))?;
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("Failed to mark launcher command executable: {}", e))?;

    if legacy_script_path.exists() {
        if let Ok(mut file) = std::fs::File::create(&legacy_script_path) {
            let _ = file.write_all(script_content.as_bytes());
            let _ = std::fs::set_permissions(
                &legacy_script_path,
                std::fs::Permissions::from_mode(0o755),
            );
        }
    }

    Ok(script_path.to_string_lossy().to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn create_platform_cdp_launcher_shortcut(
    _launcher_path: &std::path::Path,
    _port: u16,
    _channel: Option<discord_cdp_launcher::DiscordChannel>,
) -> Result<String, String> {
    Err("Shortcut creation is only supported on Windows and macOS.".to_string())
}

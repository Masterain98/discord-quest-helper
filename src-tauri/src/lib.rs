// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod discord_api;
mod discord_gateway;
mod game_simulator;
mod models;
mod quest_completer;
mod super_properties;
mod token_extractor;
mod logger;
mod cdp_client;
mod stealth;

use discord_api::DiscordApiClient;
use models::*;
use std::sync::Mutex;
use tauri::{Emitter, Listener, Manager, State};
use super_properties::XSuperPropertiesManager;
use once_cell::sync::Lazy;

/// Global X-Super-Properties manager (session-level)
/// Automatically generates key validation fields, fetches latest version info from Discord after login
static SUPER_PROPERTIES_MANAGER: Lazy<Mutex<XSuperPropertiesManager>> = Lazy::new(|| {
    Mutex::new(XSuperPropertiesManager::new())
});

/// Global state: Discord API client
struct AppState {
    client: Mutex<Option<DiscordApiClient>>,
    quest_state: Mutex<Option<QuestState>>,
}

/// Auto-detect Discord tokens (returns all valid accounts found)
#[tauri::command]
async fn auto_detect_token(_state: State<'_, AppState>) -> Result<Vec<ExtractedAccount>, String> {
    use crate::logger::{log, LogLevel, LogCategory};
    
    log(LogLevel::Info, LogCategory::TokenExtraction, "Starting auto token detection", None);
    
    // Extract tokens
    let tokens = token_extractor::extract_tokens()
        .map_err(|e| {
            log(LogLevel::Error, LogCategory::TokenExtraction, "Token extraction failed", Some(&e.to_string()));
            format!("Token extraction failed: {}", e)
        })?;
    
    log(LogLevel::Info, LogCategory::TokenExtraction, &format!("Extracted {} potential tokens", tokens.len()), None);

    let mut valid_accounts = Vec::new();
    let mut last_error = String::new();
    
    log(LogLevel::Debug, LogCategory::TokenExtraction, 
        &format!("Validating {} tokens", tokens.len()), None);

    for (index, token) in tokens.iter().enumerate() {
        log(LogLevel::Debug, LogCategory::TokenExtraction, 
            &format!("Validating token {}/{}", index + 1, tokens.len()), None);
        // Create API client
        if let Ok(client) = DiscordApiClient::new(token.clone()) {
            // Validate token
            match client.get_current_user().await {
                Ok(user) => {
                    log(LogLevel::Info, LogCategory::TokenExtraction, 
                        &format!("Token {} validated successfully", index + 1), None);
                    valid_accounts.push(ExtractedAccount {
                        token: token.clone(),
                        user,
                    });
                }
                Err(e) => {
                    log(LogLevel::Warn, LogCategory::TokenExtraction, 
                        &format!("Token {} validation failed", index + 1), Some(&e.to_string()));
                    last_error = format!("Token validation failed: {}", e);
                    // Continue to next token
                }
            }
        }
    }
    
    log(LogLevel::Info, LogCategory::TokenExtraction, 
        &format!("Token detection complete: {} valid accounts found", valid_accounts.len()), None);

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
    use crate::logger::{log, LogLevel, LogCategory};
    
    // Create API client
    let client = DiscordApiClient::new(token)
        .map_err(|e| format!("Failed to create API client: {}", e))?;

    // Validate token
    let user = client
        .get_current_user()
        .await
        .map_err(|e| format!("Failed to validate token: {}", e))?;

    // Fetch latest build_number and client info before returning (so frontend await can rely on completion)
    // Get build_number
    match token_extractor::fetch_build_number_from_discord().await {
        Ok(build_number) => {
            log(LogLevel::Info, LogCategory::TokenExtraction, 
                &format!("Successfully fetched build number: {}", build_number), None);
            if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
                manager.set_from_remote_js(build_number);
            }
        }
        Err(e) => {
            log(LogLevel::Warn, LogCategory::TokenExtraction, 
                &format!("Failed to fetch build number: {}", e), None);
        }
    }
    
    // Get client info (native_build_number and version)
    match token_extractor::fetch_discord_client_info().await {
        Ok(info) => {
            log(LogLevel::Info, LogCategory::TokenExtraction, 
                &format!("Successfully fetched client info: version={}, native_build={}", 
                    info.client_version(), info.native_build_number), None);
            if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
                manager.set_client_info(info.client_version(), info.native_build_number);
            }
        }
        Err(e) => {
            log(LogLevel::Warn, LogCategory::TokenExtraction, 
                &format!("Failed to fetch client info: {}", e), None);
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
        // Wrap cleanup in catch_unwind to log any errors before exiting
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
                        eprintln!("[Stealth] Failed to set window title to '{}': {}", stealth_title, err);
                    }
                }
            }
            Ok(())
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
            force_video_progress,
            export_logs,
            get_debug_info,
            check_cdp_status,
            fetch_super_properties_cdp,
            create_discord_debug_shortcut,
            get_super_properties_mode,
            auto_fetch_super_properties,
            retry_super_properties
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Clean up on window close
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

    client.update_video_progress(&quest_id, timestamp)
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

/// Check CDP status
#[tauri::command]
async fn check_cdp_status(port: Option<u16>) -> cdp_client::CdpStatus {
    let port = port.unwrap_or(cdp_client::DEFAULT_CDP_PORT);
    cdp_client::check_cdp_available(port).await
}

/// Fetch SuperProperties via CDP
#[tauri::command]
async fn fetch_super_properties_cdp(port: Option<u16>) -> Result<cdp_client::CdpSuperProperties, String> {
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
    use crate::logger::{log, LogLevel, LogCategory};
    
    let port = cdp_port.unwrap_or(cdp_client::DEFAULT_CDP_PORT);
    
    // Priority 1: Try CDP
    log(LogLevel::Info, LogCategory::TokenExtraction, 
        &format!("Auto-fetching SuperProperties, trying CDP on port {}", port), None);
    
    if let Ok(cdp_result) = cdp_client::fetch_super_properties_via_cdp(port).await {
        if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
            manager.set_from_cdp(&cdp_result.base64, &cdp_result.decoded);
            log(LogLevel::Info, LogCategory::TokenExtraction, 
                &format!("SuperProperties obtained via CDP. Build: {:?}", manager.get_build_number()), None);
            return serde_json::json!({
                "success": true,
                "mode": "cdp",
                "build_number": manager.get_build_number()
            });
        }
    }
    
    log(LogLevel::Debug, LogCategory::TokenExtraction, 
        "CDP failed, falling back to Remote JS", None);
    
    // Priority 2: Try Remote JS
    if let Ok(build_number) = token_extractor::fetch_build_number_from_discord().await {
        if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
            manager.set_from_remote_js(build_number);
            log(LogLevel::Info, LogCategory::TokenExtraction, 
                &format!("SuperProperties obtained via Remote JS. Build: {}", build_number), None);
            return serde_json::json!({
                "success": true,
                "mode": "remote_js",
                "build_number": build_number
            });
        }
    }
    
    log(LogLevel::Warn, LogCategory::TokenExtraction, 
        "All fetch methods failed, using default values", None);
    
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

/// Create Discord debug shortcut on desktop
#[tauri::command]
async fn create_discord_debug_shortcut(port: Option<u16>) -> Result<String, String> {
    let port = port.unwrap_or(cdp_client::DEFAULT_CDP_PORT);
    create_discord_shortcut_internal(port).await
}

#[cfg(target_os = "windows")]
async fn create_discord_shortcut_internal(port: u16) -> Result<String, String> {
    use std::process::Command;
    use std::path::PathBuf;
    
    // Find Discord executable
    let discord_exe = find_discord_executable()
        .ok_or_else(|| "Could not find Discord installation".to_string())?;
    
    // Get desktop path
    let desktop = std::env::var("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Desktop"))
        .map_err(|_| "Could not get desktop path".to_string())?;
    
    let shortcut_path = desktop.join("Discord (Debug Mode).lnk");
    
    // Create shortcut using PowerShell with single-quoted strings for safety
    // Escape embedded single quotes by doubling them
    let shortcut_path_ps = shortcut_path.to_string_lossy().replace('\'', "''");
    let discord_exe_ps = discord_exe.to_string_lossy().replace('\'', "''");
    
    let ps_script = format!(
        r#"
$WshShell = New-Object -comObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut('{}')
$Shortcut.TargetPath = '{}'
$Shortcut.Arguments = "--remote-debugging-port={} --remote-allow-origins=*"
$Shortcut.Description = "Discord with DevTools Protocol enabled for Quest Helper"
$Shortcut.Save()
"#,
        shortcut_path_ps,
        discord_exe_ps,
        port
    );
    
    // Use a temporary file for the script to avoid issues with special characters in arguments
    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join(format!("discord_shortcut_{}.ps1", uuid::Uuid::new_v4()));
    
    std::fs::write(&script_path, &ps_script)
        .map_err(|e| format!("Failed to write temporary PowerShell script: {}", e))?;
        
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", &script_path.to_string_lossy()])
        .output();
        
    // Clean up temporary script
    let _ = std::fs::remove_file(&script_path);

    let output = output.map_err(|e| format!("Failed to execute PowerShell: {}", e))?;
    
    if output.status.success() {
        Ok(shortcut_path.to_string_lossy().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Failed to create shortcut: {}", stderr))
    }
}

#[cfg(target_os = "windows")]
fn find_discord_executable() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    
    let local_appdata = std::env::var("LOCALAPPDATA").ok()?;
    let base = PathBuf::from(local_appdata);
    
    // Map channel folder to executable name
    let channels = [
        ("Discord", "Discord.exe"),
        ("DiscordPTB", "DiscordPTB.exe"),
        ("DiscordCanary", "DiscordCanary.exe"),
    ];
    
    for (channel, exe_name) in channels {
        let channel_path = base.join(channel);
        
        // Find latest app-* directory
        if let Ok(entries) = std::fs::read_dir(&channel_path) {
            let mut app_dirs: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("app-")
                })
                .collect();
            // Sort by version number (extract numeric parts for proper ordering)
            // e.g., "app-1.0.9219" -> parse version numerically
            app_dirs.sort_by(|a, b| {
                let extract_version = |name: &std::ffi::OsStr| -> Vec<u32> {
                    name.to_string_lossy()
                        .strip_prefix("app-")
                        .unwrap_or("")
                        .split('.')
                        .filter_map(|s| s.parse().ok())
                        .collect()
                };
                let va = extract_version(&a.file_name());
                let vb = extract_version(&b.file_name());
                vb.cmp(&va) // Descending order (latest first)
            });
            
            if let Some(latest) = app_dirs.first() {
                let exe_path = latest.path().join(exe_name);
                if exe_path.exists() {
                    return Some(exe_path);
                }
            }
        }
        
        // Check root directory directly
        let direct_exe = channel_path.join(exe_name);
        if direct_exe.exists() {
            return Some(direct_exe);
        }
    }
    
    None
}

#[cfg(target_os = "macos")]
async fn create_discord_shortcut_internal(port: u16) -> Result<String, String> {
    // macOS: Create a shell script or suggest terminal command
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME")?;
    let desktop = format!("{}/Desktop", home);
    let script_path = format!("{}/Discord Debug Mode.command", desktop);
    
    let discord_path = find_discord_executable_macos()
        .ok_or_else(|| "Could not find Discord installation".to_string())?;
    
    let script_content = format!(
        r#"#!/bin/bash
# Discord with DevTools Protocol enabled for Quest Helper
"{}" --remote-debugging-port={} --remote-allow-origins=*
"#,
        discord_path, port
    );
    
    std::fs::write(&script_path, script_content)
        .map_err(|e| format!("Failed to write script: {}", e))?;
    
    // Make executable
    std::process::Command::new("chmod")
        .args(["+x", &script_path])
        .output()
        .map_err(|e| format!("Failed to make script executable: {}", e))?;
    
    Ok(script_path)
}

#[cfg(target_os = "macos")]
fn find_discord_executable_macos() -> Option<String> {
    let paths = [
        "/Applications/Discord.app/Contents/MacOS/Discord",
        "/Applications/Discord Canary.app/Contents/MacOS/Discord",
        "/Applications/Discord PTB.app/Contents/MacOS/Discord",
    ];
    
    for path in paths {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    
    None
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
async fn create_discord_shortcut_internal(_port: u16) -> Result<String, String> {
    Err("Shortcut creation is only supported on Windows and macOS".to_string())
}


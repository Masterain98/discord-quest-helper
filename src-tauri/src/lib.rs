// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cdp_client;
mod cdp_game_spoof;
mod cdp_quest;
mod discord_api;
mod discord_cdp_commands;
mod discord_gateway;
mod game_simulator;
mod logger;
mod models;
mod platform_capabilities;
mod quest_completer;
#[cfg(unix)]
mod runtime_bridge;
mod runtime_identity;
#[cfg(windows)]
#[cfg_attr(debug_assertions, allow(dead_code))]
mod stealth_pe;
mod super_properties;
mod token_extractor;

use discord_api::DiscordApiClient;
use models::*;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use super_properties::XSuperPropertiesManager;
use tauri::ipc::Channel;
use tauri::{Emitter, Listener, Manager, State, WebviewWindowBuilder};

/// Global X-Super-Properties manager (session-level)
/// Automatically generates key validation fields, fetches latest version info from Discord after login
static SUPER_PROPERTIES_MANAGER: Lazy<Mutex<XSuperPropertiesManager>> =
    Lazy::new(|| Mutex::new(XSuperPropertiesManager::new()));

const APP_EXIT_RPC_DISCONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
/// Bound for waiting on a cancelled quest task. CDP cancel cleanup uses one
/// 15s evaluation; keep headroom for a poll-loop select to notice cancel.
const QUEST_STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(45);
/// Last-chance wait inside `exit_app_now` after the frontend's short prepare
/// deadline. Covers verified manual CDP cleanup (five 15s evaluations) plus
/// a cancelled quest task so `process::exit` does not abort in-flight rollback.
const APP_EXIT_FINAL_CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Tracks whether process-local exit cleanup and verified active-work cleanup
/// have completed. Local cleanup is one-shot; active-work cleanup stays
/// retryable until it succeeds so a failed CDP rollback cannot permanently
/// skip later `prepare_app_exit` calls.
struct AppExitCleanupState {
    prepared: AtomicBool,
    local_done: AtomicBool,
}

impl AppExitCleanupState {
    const fn new() -> Self {
        Self {
            prepared: AtomicBool::new(false),
            local_done: AtomicBool::new(false),
        }
    }

    fn is_prepared(&self) -> bool {
        self.prepared.load(Ordering::SeqCst)
    }

    fn mark_prepared(&self) {
        self.prepared.store(true, Ordering::SeqCst);
    }

    fn claim_local_cleanup(&self) -> bool {
        !self.local_done.swap(true, Ordering::SeqCst)
    }
}

static APP_EXIT_CLEANUP: AppExitCleanupState = AppExitCleanupState::new();

/// Global state: Discord API client
struct AppState {
    client: Mutex<Option<DiscordApiClient>>,
    quest_state: Mutex<Option<QuestState>>,
    manual_cdp_game: tokio::sync::Mutex<ManualCdpGameSessionState>,
    /// Serializes quest startup, manual CDP startup, and active-work teardown
    /// so the two Discord-activity owners cannot both pass their idle checks.
    activity_gate: tokio::sync::Mutex<()>,
}

#[derive(Debug, Default)]
struct ManualCdpGameSessionState {
    active: Option<ManualCdpGameSimulation>,
}

impl ManualCdpGameSessionState {
    fn ensure_idle(&self) -> Result<(), String> {
        match &self.active {
            Some(session) => Err(format!(
                "A manual CDP game simulation is already active for {}",
                session.app_name
            )),
            None => Ok(()),
        }
    }

    fn activate(&mut self, session: ManualCdpGameSimulation) {
        self.active = Some(session);
    }

    fn active(&self) -> Option<ManualCdpGameSimulation> {
        self.active.clone()
    }

    fn clear(&mut self) {
        self.active = None;
    }

    fn finish_cleanup(&mut self, result: Result<(), String>) -> Result<(), String> {
        result?;
        self.clear();
        Ok(())
    }
}

#[cfg(test)]
mod manual_cdp_game_session_tests {
    use super::*;

    fn session(name: &str) -> ManualCdpGameSimulation {
        ManualCdpGameSimulation {
            app_id: "123456".to_string(),
            app_name: name.to_string(),
            cdp_port: 9223,
        }
    }

    #[test]
    fn only_one_manual_cdp_game_can_be_active() {
        let mut state = ManualCdpGameSessionState::default();
        state.ensure_idle().unwrap();
        state.activate(session("First"));

        assert!(state.ensure_idle().is_err());
        assert_eq!(state.active().unwrap().app_name, "First");
    }

    #[test]
    fn failed_start_does_not_record_a_session() {
        let state = ManualCdpGameSessionState::default();
        state.ensure_idle().unwrap();

        // CDP startup failed before activate() was called.
        assert!(state.active().is_none());
    }

    #[test]
    fn cleanup_failure_keeps_the_session_for_retry() {
        let mut state = ManualCdpGameSessionState::default();
        state.activate(session("Retry Me"));

        assert!(state
            .finish_cleanup(Err("Discord target disconnected".to_string()))
            .is_err());
        assert_eq!(state.active().unwrap().app_name, "Retry Me");

        state.finish_cleanup(Ok(())).unwrap();
        assert!(state.active().is_none());
    }

    #[test]
    fn session_uses_the_frontend_camel_case_contract() {
        let value = serde_json::to_value(session("Contract")).unwrap();
        assert_eq!(value["appId"], "123456");
        assert_eq!(value["appName"], "Contract");
        assert_eq!(value["cdpPort"], 9223);
        assert!(value.get("app_id").is_none());
    }
}

#[cfg(test)]
mod app_exit_cleanup_state_tests {
    use super::AppExitCleanupState;

    #[test]
    fn failed_active_work_cleanup_leaves_exit_retryable() {
        let state = AppExitCleanupState::new();
        assert!(state.claim_local_cleanup());
        assert!(!state.claim_local_cleanup());
        assert!(!state.is_prepared());
    }

    #[test]
    fn successful_exit_preparation_skips_later_attempts() {
        let state = AppExitCleanupState::new();
        assert!(state.claim_local_cleanup());
        state.mark_prepared();
        assert!(state.is_prepared());
    }
}

#[cfg(test)]
mod quest_stop_wait_tests {
    use super::await_quest_task;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn waiting_for_a_quest_task_observes_cleanup_before_returning() {
        let cleaned = Arc::new(AtomicBool::new(false));
        let cleaned_for_task = cleaned.clone();
        let join = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cleaned_for_task.store(true, Ordering::SeqCst);
        });

        await_quest_task(join).await;
        assert!(cleaned.load(Ordering::SeqCst));
    }
}

/// Auto-detect Discord tokens (returns all valid accounts found)
#[tauri::command]
async fn auto_detect_token(
    _state: State<'_, AppState>,
    on_progress: Channel<AuthProgress>,
) -> Result<Vec<ExtractedAccount>, String> {
    use crate::logger::{log, LogCategory, LogLevel};

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        "Starting auto token detection",
        None,
    );

    let _ = on_progress.send(AuthProgress::phase(AuthProgressPhase::ExtractingTokens));

    // Extract tokens. Local profile scans and Linux Secret Service access are
    // blocking operations, so keep them off the async command thread.
    let tokens = tauri::async_runtime::spawn_blocking(token_extractor::extract_tokens)
        .await
        .map_err(|e| {
            log(
                LogLevel::Error,
                LogCategory::TokenExtraction,
                "Token extraction task failed",
                Some(&e.to_string()),
            );
            format!("Token extraction task failed: {}", e)
        })?
        .map_err(|e| {
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

    log(
        LogLevel::Debug,
        LogCategory::TokenExtraction,
        &format!("Validating {} tokens", tokens.len()),
        None,
    );

    let progress_channel = on_progress.clone();
    let (valid_accounts, last_error) = validate_extracted_tokens(
        tokens,
        |token, index| async move {
            log(
                LogLevel::Debug,
                LogCategory::TokenExtraction,
                &format!("Validating token {}", index),
                None,
            );
            let client = DiscordApiClient::new(token.clone())
                .map_err(|error| format!("Failed to create API client: {error}"))?;
            match client.get_current_user().await {
                Ok(user) => {
                    log(
                        LogLevel::Info,
                        LogCategory::TokenExtraction,
                        &format!("Token {} validated successfully", index),
                        None,
                    );
                    Ok(ExtractedAccount { token, user })
                }
                Err(error) => {
                    log(
                        LogLevel::Warn,
                        LogCategory::TokenExtraction,
                        &format!("Token {} validation failed", index),
                        Some(&error.to_string()),
                    );
                    Err(format!("Token validation failed: {error}"))
                }
            }
        },
        move |progress| {
            let _ = progress_channel.send(progress);
        },
    )
    .await;

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
        return Err(if let Some(last_error) = last_error {
            format!("No valid accounts found. Last error: {}", last_error)
        } else {
            "No valid accounts found".to_string()
        });
    }

    // Sort accounts? Maybe by username? Or keep order.

    Ok(valid_accounts)
}

async fn validate_extracted_tokens<F, Fut, P>(
    tokens: Vec<String>,
    mut validate: F,
    mut report: P,
) -> (Vec<ExtractedAccount>, Option<String>)
where
    F: FnMut(String, usize) -> Fut,
    Fut: std::future::Future<Output = Result<ExtractedAccount, String>>,
    P: FnMut(AuthProgress),
{
    let total = tokens.len();
    let mut valid_accounts = Vec::new();
    let mut last_error = None;

    report(AuthProgress::validating(0, total));
    for (index, token) in tokens.into_iter().enumerate() {
        let current = index + 1;
        report(AuthProgress::validating(current, total));
        match validate(token, current).await {
            Ok(account) => valid_accounts.push(account),
            Err(error) => last_error = Some(error),
        }
    }
    report(AuthProgress::accounts_found(total, valid_accounts.len()));

    (valid_accounts, last_error)
}

async fn capture_cdp_session_with_progress<T, E, Fut, P>(
    capture: Fut,
    mut report: P,
) -> Result<T, E>
where
    Fut: std::future::Future<Output = Result<T, E>>,
    P: FnMut(AuthProgress),
{
    report(AuthProgress::phase(AuthProgressPhase::CapturingCdpSession));
    capture.await
}

#[cfg(test)]
mod auth_progress_tests {
    use super::{capture_cdp_session_with_progress, validate_extracted_tokens};
    use crate::models::{AuthProgress, AuthProgressPhase, DiscordUser, ExtractedAccount};
    use std::sync::{Arc, Mutex};

    fn account(token: String) -> ExtractedAccount {
        ExtractedAccount {
            token,
            user: DiscordUser {
                id: "test-user".to_string(),
                username: "tester".to_string(),
                discriminator: "0".to_string(),
                avatar: None,
                global_name: None,
                premium_type: None,
            },
        }
    }

    #[tokio::test]
    async fn empty_token_scan_reports_zero_counts_without_validation() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let (accounts, last_error) = validate_extracted_tokens(
            Vec::new(),
            |token, _| async move { Ok(account(token)) },
            move |progress| captured.lock().unwrap().push(progress),
        )
        .await;

        assert!(accounts.is_empty());
        assert!(last_error.is_none());
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                AuthProgress::validating(0, 0),
                AuthProgress::accounts_found(0, 0),
            ]
        );
    }

    #[tokio::test]
    async fn partial_validation_reports_each_index_and_keeps_valid_accounts() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let (accounts, last_error) = validate_extracted_tokens(
            vec!["invalid-secret".to_string(), "valid-secret".to_string()],
            |token, _| async move {
                if token.starts_with("valid-") {
                    Ok(account(token))
                } else {
                    Err("rejected".to_string())
                }
            },
            move |progress| captured.lock().unwrap().push(progress),
        )
        .await;

        assert_eq!(accounts.len(), 1);
        assert_eq!(last_error.as_deref(), Some("rejected"));
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                AuthProgress::validating(0, 2),
                AuthProgress::validating(1, 2),
                AuthProgress::validating(2, 2),
                AuthProgress::accounts_found(2, 1),
            ]
        );

        let serialized = serde_json::to_string(&*events.lock().unwrap()).unwrap();
        assert!(!serialized.contains("invalid-secret"));
        assert!(!serialized.contains("valid-secret"));
    }

    #[tokio::test]
    async fn failed_cdp_capture_stops_after_the_capture_phase() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let result: Result<(), &str> =
            capture_cdp_session_with_progress(async { Err("capture failed") }, move |progress| {
                captured.lock().unwrap().push(progress)
            })
            .await;

        assert_eq!(result, Err("capture failed"));
        assert_eq!(
            *events.lock().unwrap(),
            vec![AuthProgress::phase(AuthProgressPhase::CapturingCdpSession)]
        );
    }
}

/// Login with provided token
#[tauri::command]
async fn set_token(
    token: String,
    state: State<'_, AppState>,
    on_progress: Channel<AuthProgress>,
) -> Result<DiscordUser, String> {
    use crate::logger::{log, LogCategory, LogLevel};

    let _ = on_progress.send(AuthProgress::phase(AuthProgressPhase::ValidatingToken));

    // Create API client
    let client =
        DiscordApiClient::new(token).map_err(|e| format!("Failed to create API client: {}", e))?;

    // Validate token
    let user = client
        .get_current_user()
        .await
        .map_err(|e| format!("Failed to validate token: {}", e))?;

    let _ = on_progress.send(AuthProgress::phase(AuthProgressPhase::PreparingSession));

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

    let _ = on_progress.send(AuthProgress::phase(AuthProgressPhase::SyncingClientInfo));

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

    let _ = on_progress.send(AuthProgress::phase(AuthProgressPhase::Complete));

    Ok(user)
}

/// CDP auto-login: capture the currently logged-in Discord session over CDP and
/// establish a DQH login from it. This is the primary login path on Linux.
///
/// The raw token is captured, validated, and stored **entirely on the Rust
/// side** — only the resolved `DiscordUser` is returned to the frontend. This
/// deliberately avoids `auto_detect_token`'s pattern of handing raw tokens to
/// the WebView: a running client has exactly one current account. Requires
/// Discord to be running with CDP enabled. Works on every platform; on Linux it
/// is the primary login path (local keyring extraction is a later phase).
#[tauri::command]
async fn auto_login_via_cdp(
    port: Option<u16>,
    state: State<'_, AppState>,
    on_progress: Channel<AuthProgress>,
) -> Result<DiscordUser, String> {
    use crate::logger::{log, LogCategory, LogLevel};
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

    let cdp_port = port.unwrap_or(cdp_client::DEFAULT_CDP_PORT);

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!("Starting CDP auto-login on port {}", cdp_port),
        None,
    );

    // 1. Capture the current session's Authorization over CDP. The token stays
    //    inside `session` (a zero-on-drop wrapper) and is never returned to the
    //    UI, logged, or persisted.
    let progress_channel = on_progress.clone();
    let session = capture_cdp_session_with_progress(
        cdp_client::capture_discord_auth_via_cdp(cdp_port, std::time::Duration::from_secs(20)),
        move |progress| {
            let _ = progress_channel.send(progress);
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    let _ = on_progress.send(AuthProgress::phase(AuthProgressPhase::ValidatingCdpSession));

    // 2. Build an API client from the captured token and validate it via
    //    /users/@me. An invalid capture is rejected here.
    let client = DiscordApiClient::new(session.authorization.to_string())
        .map_err(|e| format!("Failed to create API client: {}", e))?;
    let user = client
        .get_current_user()
        .await
        .map_err(|e| format!("Captured Discord session is not valid: {}", e))?;

    let _ = on_progress.send(AuthProgress::phase(AuthProgressPhase::PreparingSession));

    // 3. Bootstrap SuperProperties. Prefer the exact `x-super-properties` we
    //    captured (the value the client actually sends); fall back to a fresh
    //    CDP fetch. Either way the manager has built-in defaults on failure.
    let mut super_properties_ready = false;
    if let Some(base64) = session.super_properties.as_ref() {
        if let Ok(decoded_bytes) = BASE64.decode(base64) {
            if let Ok(decoded) = serde_json::from_slice::<serde_json::Value>(&decoded_bytes) {
                if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
                    manager.set_from_cdp(base64, &decoded);
                    super_properties_ready = true;
                }
            }
        }
    }
    if !super_properties_ready {
        if let Ok(cdp_result) = cdp_client::fetch_super_properties_via_cdp(cdp_port).await {
            if let Ok(mut manager) = SUPER_PROPERTIES_MANAGER.lock() {
                manager.set_from_cdp(&cdp_result.base64, &cdp_result.decoded);
            }
        }
    }

    // 4. Save the client last (mirrors set_token) so no request runs with stale
    //    super properties.
    *state.client.lock().unwrap() = Some(client);

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        "CDP auto-login succeeded",
        None,
    );

    let _ = on_progress.send(AuthProgress::phase(AuthProgressPhase::Complete));

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
    let _gate = state.activity_gate.lock().await;
    stop_active_work_internal(&state).await?;

    let client = state.client.lock().unwrap();
    let client = client
        .as_ref()
        .ok_or_else(|| "Not logged in".to_string())?
        .clone();

    let quest_id_for_state = quest_id.clone();
    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::channel::<()>(1);
    let join = tokio::spawn(async move {
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
    store_running_quest(&state, quest_id_for_state, cancel_tx, join);

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
    let _gate = state.activity_gate.lock().await;
    stop_active_work_internal(&state).await?;

    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    let quest_id_for_state = quest_id.clone();
    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::channel::<()>(1);
    let join = tokio::spawn(async move {
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
    store_running_quest(&state, quest_id_for_state, cancel_tx, join);

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
    let _gate = state.activity_gate.lock().await;
    stop_active_work_internal(&state).await?;

    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not logged in".to_string())?
            .clone()
    };

    let quest_id_for_state = quest_id.clone();
    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::channel::<()>(1);
    let join = tokio::spawn(async move {
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
    store_running_quest(&state, quest_id_for_state, cancel_tx, join);

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayActivityTransport {
    DirectApi,
    Cdp,
}

impl TryFrom<&str> for PlayActivityTransport {
    type Error = String;

    fn try_from(mode: &str) -> Result<Self, Self::Error> {
        match mode {
            "simulate" | "heartbeat" => Ok(Self::DirectApi),
            "cdp" => Ok(Self::Cdp),
            _ => Err(format!("Unsupported PLAY_ACTIVITY mode: {}", mode)),
        }
    }
}

#[cfg(test)]
mod play_activity_transport_tests {
    use super::PlayActivityTransport;

    #[test]
    fn maps_supported_frontend_modes_to_a_transport() {
        assert_eq!(
            PlayActivityTransport::try_from("simulate"),
            Ok(PlayActivityTransport::DirectApi)
        );
        assert_eq!(
            PlayActivityTransport::try_from("heartbeat"),
            Ok(PlayActivityTransport::DirectApi)
        );
        assert_eq!(
            PlayActivityTransport::try_from("cdp"),
            Ok(PlayActivityTransport::Cdp)
        );
        assert!(PlayActivityTransport::try_from("unknown").is_err());
    }
}

/// Start a PLAY_ACTIVITY cloud-game quest using the current game quest mode.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn start_play_activity_quest(
    quest_id: String,
    application_id: String,
    seconds_needed: u32,
    initial_progress: f64,
    mode: String,
    cdp_port: u16,
    heartbeat_interval: u64,
    progress_polling_interval: u64,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let transport = PlayActivityTransport::try_from(mode.as_str())?;
    if heartbeat_interval == 0 {
        return Err("PLAY_ACTIVITY heartbeat interval must be greater than zero".to_string());
    }
    if progress_polling_interval == 0 {
        return Err(
            "PLAY_ACTIVITY progress polling interval must be greater than zero".to_string(),
        );
    }

    let client = state.client.lock().unwrap().clone();
    if transport == PlayActivityTransport::DirectApi && client.is_none() {
        return Err("Not logged in".to_string());
    }

    let _gate = state.activity_gate.lock().await;
    stop_active_work_internal(&state).await?;
    let quest_id_for_state = quest_id.clone();
    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::channel::<()>(1);
    let join = tokio::spawn(async move {
        let result = if transport == PlayActivityTransport::Cdp {
            cdp_quest::complete_play_activity_via_cdp(
                cdp_port,
                quest_id,
                application_id,
                seconds_needed,
                initial_progress,
                heartbeat_interval,
                progress_polling_interval,
                app_handle.clone(),
                cancel_rx,
            )
            .await
        } else {
            quest_completer::complete_play_activity_via_heartbeat(
                client
                    .as_ref()
                    .expect("direct PLAY_ACTIVITY mode validated an API client"),
                quest_id,
                application_id,
                seconds_needed,
                initial_progress,
                heartbeat_interval,
                progress_polling_interval,
                app_handle.clone(),
                cancel_rx,
            )
            .await
        };

        if let Err(error) = result {
            let _ = app_handle.emit(
                "quest-error",
                format!("PLAY_ACTIVITY quest failed: {:#}", error),
            );
        }
    });
    store_running_quest(&state, quest_id_for_state, cancel_tx, join);

    Ok(())
}

/// Start a quest via CDP injection
///
/// Dispatches to the appropriate CDP completion function based on quest_type.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
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
    let _gate = state.activity_gate.lock().await;
    stop_active_work_internal(&state).await?;
    let quest_id_for_state = quest_id.clone();
    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::channel::<()>(1);

    let quest_type_clone = quest_type.clone();

    // Clone the API client for progress polling (play/stream quests)
    let client = state.client.lock().unwrap().clone();

    let join = tokio::spawn(async move {
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
                    application_id,
                    initial_progress,
                    times,
                    client,
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
    store_running_quest(&state, quest_id_for_state, cancel_tx, join);

    Ok(())
}

/// Stop current quest
#[tauri::command]
async fn stop_quest(state: State<'_, AppState>) -> Result<(), String> {
    let _gate = state.activity_gate.lock().await;
    stop_active_work_internal(&state).await
}

fn store_running_quest(
    state: &State<'_, AppState>,
    quest_id: String,
    cancel_flag: tokio::sync::mpsc::Sender<()>,
    join: tokio::task::JoinHandle<()>,
) {
    *state.quest_state.lock().unwrap() = Some(QuestState {
        quest_id,
        cancel_flag,
        join: Some(join),
    });
}

async fn stop_quest_internal(state: &State<'_, AppState>) {
    let quest = {
        let mut quest_state = state.quest_state.lock().unwrap();
        quest_state.take()
    };

    if let Some(quest) = quest {
        let _ = quest.cancel_flag.send(()).await;
        if let Some(join) = quest.join {
            await_quest_task(join).await;
        }
        println!("Quest stopped");
    }
}

async fn await_quest_task(join: tokio::task::JoinHandle<()>) {
    match tokio::time::timeout(QUEST_STOP_WAIT, join).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            eprintln!("Quest task ended with a join error after cancel: {error}");
        }
        Err(_) => {
            eprintln!(
                "Quest task did not finish within {QUEST_STOP_WAIT:?} after cancel; leftover Discord activity may still be cleaned up in the background"
            );
        }
    }
}

async fn ensure_no_active_quest(state: &State<'_, AppState>) -> Result<(), String> {
    let stale = {
        let mut quest_state = state.quest_state.lock().unwrap();
        match quest_state.as_ref() {
            Some(quest) if !quest.cancel_flag.is_closed() => {
                return Err(
                    "Stop the active quest before starting a manual CDP game simulation"
                        .to_string(),
                );
            }
            Some(_) => {
                // Completed background tasks close their receiver. Discard that
                // stale bookkeeping entry without treating it as an active quest,
                // but still wait so any in-flight CDP cleanup can finish.
                quest_state.take()
            }
            None => None,
        }
    };
    if let Some(quest) = stale {
        if let Some(join) = quest.join {
            await_quest_task(join).await;
        }
    }
    Ok(())
}

async fn stop_manual_cdp_game_simulation_internal(
    state: &State<'_, AppState>,
) -> Result<(), String> {
    // Keep the lock for the full verified cleanup so a concurrent start cannot
    // install a new spoof between cleanup and clearing the saved session.
    let mut sessions = state.manual_cdp_game.lock().await;
    let Some(session) = sessions.active() else {
        return Ok(());
    };

    let cleanup_result = cdp_quest::stop_manual_game_spoof(session.cdp_port)
        .await
        .map_err(|error| {
            format!(
                "Failed to stop manual CDP game simulation: {error}. Restart Discord if the simulated game remains visible."
            )
        });
    sessions.finish_cleanup(cleanup_result)
}

async fn stop_active_work_internal(state: &State<'_, AppState>) -> Result<(), String> {
    stop_quest_internal(state).await;
    stop_manual_cdp_game_simulation_internal(state).await
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
    tauri::async_runtime::spawn_blocking(move || {
        game_simulator::run_simulated_game(&name, &path, &executable_name, &app_id)
    })
    .await
    .map_err(|e| format!("Game simulator task failed: {}", e))?
    .map_err(|e| format!("Failed to run simulated game: {}", e))
}

/// Stop simulated game
#[tauri::command]
async fn stop_simulated_game(exec_name: String) -> Result<(), String> {
    game_simulator::stop_simulated_game(&exec_name)
        .map_err(|e| format!("Failed to stop simulated game: {}", e))
}

/// Start a persistent manual game simulation inside Discord via CDP.
#[tauri::command]
async fn start_manual_cdp_game_simulation(
    app_id: String,
    app_name: String,
    cdp_port: u16,
    state: State<'_, AppState>,
) -> Result<ManualCdpGameSimulation, String> {
    let app_id = app_id.trim().to_string();
    let app_name = app_name.trim().to_string();
    if app_id.is_empty() {
        return Err("Application ID is required for CDP game simulation".to_string());
    }
    if app_name.is_empty() {
        return Err("Application name is required for CDP game simulation".to_string());
    }
    if cdp_port == 0 {
        return Err("CDP port must be between 1 and 65535".to_string());
    }

    // Keep the gate through activation so a concurrent quest start cannot
    // pass its idle check, install QuestState, and then allow this command
    // to inject a second Discord activity.
    let _gate = state.activity_gate.lock().await;
    ensure_no_active_quest(&state).await?;

    let mut sessions = state.manual_cdp_game.lock().await;
    sessions.ensure_idle()?;

    let status = cdp_client::check_cdp_available(cdp_port).await;
    if !status.connected {
        return Err(status
            .error
            .unwrap_or_else(|| format!("Discord CDP is not connected on port {cdp_port}")));
    }

    cdp_quest::start_manual_game_spoof(cdp_port, &app_id, &app_name)
        .await
        .map_err(|error| format!("Failed to start manual CDP game simulation: {error}"))?;

    let session = ManualCdpGameSimulation {
        app_id,
        app_name,
        cdp_port,
    };
    sessions.activate(session.clone());
    Ok(session)
}

/// Stop and fully verify cleanup of the current manual CDP game simulation.
#[tauri::command]
async fn stop_manual_cdp_game_simulation(state: State<'_, AppState>) -> Result<(), String> {
    let _gate = state.activity_gate.lock().await;
    stop_manual_cdp_game_simulation_internal(&state).await
}

/// Return the backend-owned manual CDP game simulation, if one is active.
#[tauri::command]
async fn get_manual_cdp_game_simulation(
    state: State<'_, AppState>,
) -> Result<Option<ManualCdpGameSimulation>, String> {
    Ok(state.manual_cdp_game.lock().await.active())
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
        .user_agent(super_properties::discord_user_agent(
            super_properties::DEFAULT_CLIENT_VERSION,
        ))
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
async fn get_billing_subscriptions(
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
        .get_billing_subscriptions()
        .await
        .map_err(|e| format!("Failed to get billing subscriptions: {}", e))
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
                    drop(tauri::async_runtime::spawn(async move {
                        let client_option = {
                            let mut client_guard = get_discord_rpc_client().lock().unwrap();
                            client_guard.take()
                        };
                        if let Some(client) = client_option {
                            client.discord.disconnect().await;
                            println!("Disconnected from Discord RPC inner");
                        }
                    }));
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
async fn disconnect_from_discord_rpc(app: tauri::AppHandle) -> Result<(), String> {
    // Cancel a connection task that may still be waiting for Discord. Without
    // this, a stop click immediately after launch could be followed by the
    // pending task storing a new RPC client and restoring the presence.
    let _ = app.emit("event_disconnect", ());

    let client = get_discord_rpc_client()
        .lock()
        .map_err(|_| "Discord RPC state lock is poisoned".to_string())?
        .take();

    if let Some(client) = client {
        client.discord.disconnect().await;
    }

    Ok(())
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
    #[cfg(target_os = "linux")]
    {
        println!("Opening file manager at: {}", path);
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open directory: {}", e))?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = path; // Suppress unused variable warning on other platforms
    }
    Ok(())
}

/// Initialize the platform runtime identity before creating any window.
pub fn initialize_runtime_identity_and_run() {
    configure_linux_webkit_runtime();

    runtime_identity::initialize();

    // Set up cleanup hook for panics with recursion guard
    use std::sync::atomic::{AtomicBool, Ordering};
    static CLEANUP_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        if !CLEANUP_IN_PROGRESS.swap(true, Ordering::SeqCst) {
            // Use catch_unwind to safely run cleanup
            let cleanup_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime_identity::cleanup_on_exit();
            }));

            if cleanup_result.is_err() {
                eprintln!("[Runtime] Error: panic occurred during cleanup in panic hook");
            }

            // Do NOT reset flag - if we panicked, we don't want to try cleaning up again
            // CLEANUP_IN_PROGRESS.store(false, Ordering::SeqCst);
        }
        // Wrap original_hook call in catch_unwind to prevent nested panics
        let hook_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            original_hook(panic_info);
        }));
        if hook_result.is_err() {
            eprintln!("[Runtime] Error: original panic hook panicked");
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

        // Wrap runtime cleanup in catch_unwind to log any errors before exiting
        let cleanup_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime_identity::cleanup_on_exit();
        }));
        if cleanup_result.is_err() {
            eprintln!("[Runtime] Error: panic occurred during cleanup in Ctrl+C handler");
        }
        std::process::exit(0);
    }) {
        eprintln!("Warning: Failed to register Ctrl+C handler: {}", e);
    }

    // Run main application
    run();
}

/// WebKitGTK can create a window but render an entirely blank surface when its
/// accelerated compositing path runs inside a VMware guest with 3D enabled.
/// Configure the upstream-supported fallback before Tauri initializes GTK.
#[cfg(target_os = "linux")]
fn configure_linux_webkit_runtime() {
    // Tauri's AppImage GTK hook currently forces GDK_BACKEND=x11. If no X11
    // display exists but a Wayland socket was explicitly supplied, restore the
    // only usable backend before Tauri initializes GTK.
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("GDK_BACKEND").as_deref() == Some(std::ffi::OsStr::new("x11"))
    {
        std::env::set_var("GDK_BACKEND", "wayland");
    }

    if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_some()
        || std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some()
    {
        return;
    }

    let product_name =
        std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
    let system_vendor = std::fs::read_to_string("/sys/class/dmi/id/sys_vendor").unwrap_or_default();

    if linux_webkit_needs_software_compositing(&product_name, &system_vendor) {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        println!("[WebKit] Disabled accelerated compositing for VMware compatibility");
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_linux_webkit_runtime() {}

#[cfg(target_os = "linux")]
fn linux_webkit_needs_software_compositing(product_name: &str, system_vendor: &str) -> bool {
    product_name.to_ascii_lowercase().contains("vmware")
        || system_vendor.to_ascii_lowercase().contains("vmware")
}

#[cfg(all(test, target_os = "linux"))]
mod linux_webkit_runtime_tests {
    use super::linux_webkit_needs_software_compositing;

    #[test]
    fn detects_vmware_without_matching_physical_hosts() {
        assert!(linux_webkit_needs_software_compositing(
            "VMware Virtual Platform",
            "VMware, Inc."
        ));
        assert!(!linux_webkit_needs_software_compositing(
            "Precision 7680",
            "Dell Inc."
        ));
    }
}

fn create_main_window(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let window_config = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or("missing window configuration")?;

    let mut builder = WebviewWindowBuilder::from_config(app.handle(), &window_config)?;

    if runtime_identity::uses_temporary_runtime() {
        let title = runtime_identity::runtime_window_title();
        builder = builder.title(&title);
        if let Some(user_data) = runtime_identity::webview_user_data_dir() {
            std::fs::create_dir_all(&user_data)?;
            builder = builder.data_directory(user_data);
        }
    }

    builder.build()?;
    Ok(())
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
            manual_cdp_game: tokio::sync::Mutex::new(ManualCdpGameSessionState::default()),
            activity_gate: tokio::sync::Mutex::new(()),
        })
        .setup(|app| {
            // `pnpm tauri:dev` rebuilds the bundled launcher before Tauri
            // starts. If a Linux launcher entry was created previously,
            // refresh its binary, desktop entry, and icon on every dev start
            // so developers always test the current launcher build.
            #[cfg(all(debug_assertions, target_os = "linux"))]
            if let Some((port, channel)) = linux_existing_cdp_launcher_options() {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    match create_discord_cdp_launcher_shortcut_internal(&app_handle, port, channel)
                        .await
                    {
                        Ok(path) => {
                            println!("[cdp-launcher-dev] Refreshed existing Linux launcher: {path}")
                        }
                        Err(error) => eprintln!(
                            "[cdp-launcher-dev] Failed to refresh existing Linux launcher: {error}"
                        ),
                    }
                });
            }

            create_main_window(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auto_detect_token,
            set_token,
            auto_login_via_cdp,
            get_quests,
            get_quests_full,
            start_video_quest,
            start_stream_quest,
            start_game_heartbeat_quest,
            start_play_activity_quest,
            start_cdp_quest,
            stop_quest,
            create_simulated_game,
            run_simulated_game,
            stop_simulated_game,
            start_manual_cdp_game_simulation,
            stop_manual_cdp_game_simulation,
            get_manual_cdp_game_simulation,
            fetch_detectable_games,
            accept_quest,
            get_virtual_currency_balance,
            get_billing_subscriptions,
            get_quest_decision_debug,
            get_quest_decisions_debug,
            claim_quest_reward,
            connect_to_discord_rpc,
            disconnect_from_discord_rpc,
            open_in_explorer,
            force_video_progress,
            export_logs,
            get_debug_info,
            get_runner_info,
            check_cdp_status,
            fetch_super_properties_cdp,
            fetch_running_games_cdp,
            discord_cdp_commands::is_discord_running,
            discord_cdp_commands::list_running_discord_cdp_sessions,
            discord_cdp_commands::launch_discord_cdp,
            discord_cdp_commands::restart_discord_cdp,
            create_discord_cdp_launcher_shortcut,
            create_discord_debug_shortcut,
            start_discord_normal_restore_helper,
            prepare_app_exit,
            exit_app_now,
            get_super_properties_mode,
            auto_fetch_super_properties,
            retry_super_properties,
            capture_discord_headers_cdp,
            navigate_discord_spa,
            platform_capabilities::get_platform_capabilities,
            runtime_identity::get_runtime_identity_status,
            runtime_identity::get_runtime_identity_audit
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                prepare_app_exit_fallback();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
async fn prepare_app_exit(state: State<'_, AppState>) -> Result<(), String> {
    prepare_active_work_and_local_cleanup(&state).await
}

/// End the main process after the frontend has completed its best-effort
/// cleanup.  This must not go through Tauri's window-close machinery: that
/// machinery is intentionally intercepted to show the CDP warning dialog,
/// and routing the confirmed action back through it can leave the window
/// alive with the frontend's close guard latched.
#[tauri::command]
async fn exit_app_now(state: State<'_, AppState>) -> Result<(), String> {
    // The close UI fail-opens after a short prepare deadline so a hung Discord
    // evaluation cannot trap the window. This command is the last chance to
    // finish or retry CDP rollback before the process disappears.
    match tokio::time::timeout(
        APP_EXIT_FINAL_CLEANUP_TIMEOUT,
        prepare_active_work_and_local_cleanup(&state),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            eprintln!("Active-work cleanup failed during process exit: {error}");
            prepare_app_exit_fallback();
        }
        Err(_) => {
            eprintln!(
                "Final exit cleanup timed out after {APP_EXIT_FINAL_CLEANUP_TIMEOUT:?}; terminating anyway"
            );
            prepare_app_exit_fallback();
        }
    }
    std::process::exit(0);
}

async fn prepare_active_work_and_local_cleanup(state: &State<'_, AppState>) -> Result<(), String> {
    if APP_EXIT_CLEANUP.is_prepared() {
        return Ok(());
    }

    let _gate = state.activity_gate.lock().await;
    if APP_EXIT_CLEANUP.is_prepared() {
        return Ok(());
    }

    // Manual CDP injections must be removed while the Discord targets are
    // still reachable. Preserve any error until the remaining local cleanup
    // has run so an RPC/game cleanup failure cannot strand another resource.
    // Do not mark exit prepared until this cleanup succeeds; otherwise a
    // later prepare_app_exit (or a retried close) would skip rollback.
    let active_work_error = stop_active_work_internal(state).await.err();
    cleanup_local_resources_on_exit().await;
    match active_work_error {
        Some(error) => Err(error),
        None => {
            APP_EXIT_CLEANUP.mark_prepared();
            Ok(())
        }
    }
}

fn prepare_app_exit_fallback() {
    if APP_EXIT_CLEANUP.is_prepared() {
        return;
    }
    // Fallback cannot reach Discord via CDP (no AppState). Still run the
    // one-shot local cleanup if prepare_app_exit has not claimed it yet.
    cleanup_local_resources_on_exit_sync();
}

fn take_discord_rpc_client_for_exit() -> Option<rpc::Client> {
    match get_discord_rpc_client().lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => {
            eprintln!("Discord RPC state lock is poisoned during app exit");
            None
        }
    }
}

async fn cleanup_local_resources_on_exit() {
    if !APP_EXIT_CLEANUP.claim_local_cleanup() {
        return;
    }
    game_simulator::cleanup_all_simulated_games();
    if let Some(client) = take_discord_rpc_client_for_exit() {
        if tokio::time::timeout(APP_EXIT_RPC_DISCONNECT_TIMEOUT, client.discord.disconnect())
            .await
            .is_err()
        {
            eprintln!("Discord RPC disconnect timed out during app exit");
        }
    }
    runtime_identity::cleanup_on_exit();
}

fn cleanup_local_resources_on_exit_sync() {
    if !APP_EXIT_CLEANUP.claim_local_cleanup() {
        return;
    }
    game_simulator::cleanup_all_simulated_games();
    if let Some(client) = take_discord_rpc_client_for_exit() {
        tauri::async_runtime::spawn(async move {
            client.discord.disconnect().await;
        });
    }
    runtime_identity::cleanup_on_exit();
}

#[tauri::command]
async fn start_discord_normal_restore_helper(app_handle: tauri::AppHandle) -> Result<(), String> {
    let launcher = find_bundled_cdp_launcher(&app_handle)?;
    #[cfg(unix)]
    runtime_bridge::verify_bundled_for_execution(&launcher).inspect_err(|error| {
        runtime_identity::record_helper_identity(Err(error.clone()));
    })?;
    tauri::async_runtime::spawn_blocking(move || spawn_restore_helper(&launcher))
        .await
        .map_err(|error| format!("Discord restore helper task failed: {error}"))?
}

fn spawn_restore_helper(launcher: &std::path::Path) -> Result<(), String> {
    use std::process::{Command, Stdio};

    let mut command = Command::new(launcher);
    command
        .arg("--restore-normal-all")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Failed to start Discord restore helper: {error}"))
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

/// Read Discord's currently loaded game detector state via CDP.
#[tauri::command]
async fn fetch_running_games_cdp(
    port: Option<u16>,
) -> Result<cdp_client::CdpRunningGamesSnapshot, String> {
    let port = port.unwrap_or(cdp_client::DEFAULT_CDP_PORT);
    cdp_client::fetch_running_games_via_cdp(port)
        .await
        .map_err(|e| e.to_string())
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
async fn create_discord_cdp_launcher_shortcut(
    app_handle: tauri::AppHandle,
    port: Option<u16>,
    channel: Option<String>,
) -> Result<String, String> {
    let channel = discord_cdp_launch_core::parse_discord_channel(channel.as_deref())
        .map_err(|error| error.to_string())?;
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
    let app_handle = app_handle.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        install_discord_cdp_launcher_impl(&app_handle)
    })
    .await
    .map_err(|error| format!("Runtime bridge installation task failed: {error}"))?;
    match &result {
        Ok((_, Some(warning))) => runtime_identity::record_helper_degraded(warning.clone()),
        Ok((_, None)) => runtime_identity::record_helper_identity(Ok(())),
        Err(error) => runtime_identity::record_helper_identity(Err(error.clone())),
    }
    result.map(|(path, _)| path)
}

fn install_discord_cdp_launcher_impl(
    app_handle: &tauri::AppHandle,
) -> Result<(std::path::PathBuf, Option<String>), String> {
    let source = find_bundled_cdp_launcher(app_handle)?;

    #[cfg(unix)]
    {
        let data_root = unix_runtime_data_root()?;
        let legacy = legacy_unix_cdp_launcher_path()?;
        let report = runtime_bridge::install(&source, &data_root, &legacy)?;
        Ok((report.executable, report.legacy_cleanup_warning))
    }

    #[cfg(windows)]
    {
        use std::fs;
        let target = stable_cdp_launcher_path()?;

        let source_size = fs::metadata(&source).map(|m| m.len()).unwrap_or(0);
        if cfg!(debug_assertions) {
            println!("[Runtime] Installing bridge payload ({source_size} bytes)");
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create CDP launcher directory: {}", e))?;
        }

        if source != target {
            fs::copy(&source, &target)
                .map_err(|e| format!("Failed to install runtime bridge: {e}"))?;
        }

        runtime_identity::strip_zone_identifier(&target);

        if let (Some(file_name), Some(stem)) = (
            target.file_name().and_then(|n| n.to_str()),
            target.file_stem().and_then(|n| n.to_str()),
        ) {
            if let Err(err) = stealth_pe::rewrite_copy_identity(&target, file_name, stem) {
                eprintln!("[Runtime] Failed to rewrite bridge version info: {err}");
            }
        }
        if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") {
            migrate_legacy_windows_cdp_launcher_at(std::path::Path::new(&local_appdata), &target);
        }
        Ok((target, None))
    }
}

#[cfg(windows)]
fn stable_cdp_launcher_path() -> Result<std::path::PathBuf, String> {
    let local_appdata =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| "Could not get LOCALAPPDATA".to_string())?;
    let pointer = windows_cdp_runtime_pointer_path()?;
    Ok(resolve_windows_cdp_runtime_path(
        std::path::Path::new(&local_appdata),
        &pointer,
    ))
}

#[cfg(target_os = "macos")]
fn unix_runtime_data_root() -> Result<std::path::PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "Could not get HOME".to_string())?;
    Ok(std::path::PathBuf::from(home)
        .join("Library")
        .join("Application Support"))
}

#[cfg(target_os = "linux")]
fn unix_runtime_data_root() -> Result<std::path::PathBuf, String> {
    linux_xdg_data_home()
}

#[cfg(target_os = "macos")]
fn legacy_unix_cdp_launcher_path() -> Result<std::path::PathBuf, String> {
    Ok(unix_runtime_data_root()?
        .join("Discord Quest Helper")
        .join("discord-cdp-launcher"))
}

#[cfg(target_os = "linux")]
fn legacy_unix_cdp_launcher_path() -> Result<std::path::PathBuf, String> {
    Ok(unix_runtime_data_root()?
        .join("discord-quest-helper")
        .join("bin")
        .join("discord-cdp-launcher"))
}

#[cfg(any(windows, test))]
const WINDOWS_CDP_APP_CONFIG_DIR: &str = "com.masterain.discord-quest-helper";
#[cfg(any(windows, test))]
const WINDOWS_CDP_RUNTIME_POINTER: &str = "cdp-runtime-exe.txt";
#[cfg(any(windows, test))]
const WINDOWS_LEGACY_CDP_DIR: &str = "DiscordQuestHelper";
#[cfg(any(windows, test))]
const WINDOWS_LEGACY_CDP_EXE: &str = "DiscordCdpLauncher.exe";

#[cfg(any(windows, test))]
fn windows_cdp_runtime_pointer_path_from(appdata: &std::path::Path) -> std::path::PathBuf {
    appdata
        .join(WINDOWS_CDP_APP_CONFIG_DIR)
        .join(WINDOWS_CDP_RUNTIME_POINTER)
}

#[cfg(windows)]
fn windows_cdp_runtime_pointer_path() -> Result<std::path::PathBuf, String> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| "Could not get APPDATA".to_string())?;
    Ok(windows_cdp_runtime_pointer_path_from(std::path::Path::new(
        &appdata,
    )))
}

/// `%LOCALAPPDATA%/<16 hex>/<12 hex>.exe` — layout only, not a full-path
/// substring scan (user profile names can contain product tokens).
#[cfg(any(windows, test))]
fn is_windows_bland_runtime_exe(path: &std::path::Path, local_appdata: &std::path::Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("exe") => {}
        _ => return false,
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if !runtime_identity::is_hex_str(stem, runtime_identity::FILE_HEX_LEN) {
        return false;
    }
    let parent = match path.parent() {
        Some(dir) => dir,
        None => return false,
    };
    let parent_name = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if !runtime_identity::is_hex_str(parent_name, runtime_identity::DIR_HEX_LEN) {
        return false;
    }
    let Some(grandparent) = parent.parent() else {
        return false;
    };
    runtime_identity::paths_eq(grandparent, local_appdata)
}

#[cfg(any(windows, test))]
fn allocate_windows_bland_runtime_exe(local_appdata: &std::path::Path) -> std::path::PathBuf {
    local_appdata
        .join(runtime_identity::generate_random_suffix(
            runtime_identity::DIR_HEX_LEN,
        ))
        .join(format!(
            "{}.exe",
            runtime_identity::generate_random_suffix(runtime_identity::FILE_HEX_LEN)
        ))
}

#[cfg(any(windows, test))]
fn resolve_windows_cdp_runtime_path(
    local_appdata: &std::path::Path,
    pointer_file: &std::path::Path,
) -> std::path::PathBuf {
    if let Ok(stored) = std::fs::read_to_string(pointer_file) {
        let stored = std::path::PathBuf::from(stored.trim());
        if is_windows_bland_runtime_exe(&stored, local_appdata) && stored.is_file() {
            return stored;
        }
    }
    let next = allocate_windows_bland_runtime_exe(local_appdata);
    if let Some(parent) = pointer_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(pointer_file, next.to_string_lossy().as_bytes());
    next
}

#[cfg(any(windows, test))]
fn migrate_legacy_windows_cdp_launcher_at(
    local_appdata: &std::path::Path,
    new_target: &std::path::Path,
) {
    let old_dir = local_appdata.join(WINDOWS_LEGACY_CDP_DIR);
    let old_exe = old_dir.join(WINDOWS_LEGACY_CDP_EXE);
    if old_exe.is_file() && !new_target.exists() {
        if let Some(parent) = new_target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(&old_exe, new_target);
    }
    if old_dir.exists() {
        let _ = std::fs::remove_dir_all(&old_dir);
    }
}

#[cfg(any(windows, test))]
fn windows_shortcut_temp_ps1_name() -> String {
    format!(
        "{}.ps1",
        runtime_identity::generate_random_suffix(runtime_identity::DIR_HEX_LEN)
    )
}

#[cfg(test)]
fn runtime_name_has_product_tokens(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("discord")
        || lower.contains("quest")
        || lower.contains("cdp")
        || lower.contains("helper")
}

#[cfg(target_os = "linux")]
fn linux_xdg_data_home() -> Result<std::path::PathBuf, String> {
    std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".local").join("share"))
        })
        .ok_or_else(|| "Could not determine XDG data home".to_string())
}

#[cfg(target_os = "linux")]
fn linux_cdp_launcher_desktop_path() -> Result<std::path::PathBuf, String> {
    Ok(linux_xdg_data_home()?
        .join("applications")
        .join("com.masterain.discord-quest-helper.cdp.desktop"))
}

#[cfg(target_os = "linux")]
fn linux_existing_cdp_launcher_options(
) -> Option<(u16, Option<discord_cdp_launch_core::DiscordChannel>)> {
    let desktop_path = linux_cdp_launcher_desktop_path().ok()?;
    if !desktop_path.exists() {
        return None;
    }

    let contents = std::fs::read_to_string(desktop_path).unwrap_or_default();
    Some(linux_cdp_launcher_options_from_desktop(&contents))
}

#[cfg(target_os = "linux")]
fn linux_cdp_launcher_options_from_desktop(
    contents: &str,
) -> (u16, Option<discord_cdp_launch_core::DiscordChannel>) {
    let mut port = cdp_client::DEFAULT_CDP_PORT;
    let mut channel = None;
    let Some(exec) = contents.lines().find_map(|line| line.strip_prefix("Exec=")) else {
        return (port, channel);
    };
    let args: Vec<&str> = exec.split_whitespace().collect();

    for pair in args.windows(2) {
        match pair[0] {
            "--port" => {
                if let Ok(value) = pair[1].parse::<u16>() {
                    if value != 0 {
                        port = value;
                    }
                }
            }
            "--channel" => {
                if let Ok(value) = discord_cdp_launch_core::parse_discord_channel(Some(pair[1])) {
                    channel = value;
                }
            }
            _ => {}
        }
    }

    (port, channel)
}

fn find_bundled_cdp_launcher(app_handle: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let names = cdp_launcher_binary_names();
    let mut candidate_dirs = Vec::new();

    // Dev mode: cwd-based paths (cwd is typically the repo root during `tauri dev`)
    // Also covers packaged installers (MSI/NSIS) where cwd == install dir and
    // the sidecar binary lives at the install root.
    if let Ok(cwd) = std::env::current_dir() {
        candidate_dirs.push(cwd.clone());
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

    #[cfg(target_os = "windows")]
    add_windows_cdp_launcher_install_dirs(&mut candidate_dirs);

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

    if cfg!(debug_assertions) {
        let searched: Vec<String> = candidate_dirs
            .iter()
            .map(|directory| directory.display().to_string())
            .collect();
        Err(format!(
            "Runtime bridge is unavailable (names: {names:?}, searched: {searched:?}). \
             Run `pnpm build:cdp-launcher` and try again."
        ))
    } else {
        Err(
            "The packaged runtime bridge is unavailable or invalid. Reinstall the application."
                .to_string(),
        )
    }
}

#[cfg(target_os = "windows")]
fn add_windows_cdp_launcher_install_dirs(candidate_dirs: &mut Vec<std::path::PathBuf>) {
    const PRODUCT_DIR: &str = "Discord Quest Helper";

    for var_name in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(var_name) {
            candidate_dirs.push(std::path::PathBuf::from(root).join(PRODUCT_DIR));
        }
    }

    if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") {
        let local_appdata = std::path::PathBuf::from(local_appdata);
        candidate_dirs.push(local_appdata.join("Programs").join(PRODUCT_DIR));
        candidate_dirs.push(local_appdata.join(PRODUCT_DIR));
    }
}

fn cdp_launcher_binary_names() -> Vec<&'static str> {
    #[cfg(target_os = "windows")]
    {
        vec![
            // Tauri bundles externalBin sidecars under the base name in installed apps.
            "waybridge.exe",
            // Dev/build trees keep the target triple because Tauri validates this input name.
            "waybridge-x86_64-pc-windows-msvc.exe",
        ]
    }

    #[cfg(target_os = "macos")]
    {
        #[cfg(target_arch = "aarch64")]
        {
            vec!["waybridge", "waybridge-aarch64-apple-darwin"]
        }
        #[cfg(target_arch = "x86_64")]
        {
            vec!["waybridge", "waybridge-x86_64-apple-darwin"]
        }
    }

    #[cfg(target_os = "linux")]
    {
        #[cfg(target_arch = "aarch64")]
        {
            vec!["waybridge", "waybridge-aarch64-unknown-linux-gnu"]
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            vec!["waybridge", "waybridge-x86_64-unknown-linux-gnu"]
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Vec::new()
    }
}

async fn create_discord_cdp_launcher_shortcut_internal(
    app_handle: &tauri::AppHandle,
    port: u16,
    channel: Option<discord_cdp_launch_core::DiscordChannel>,
) -> Result<String, String> {
    let launcher_path = install_discord_cdp_launcher_internal(app_handle).await?;
    create_platform_cdp_launcher_shortcut(&launcher_path, port, channel)
}

#[cfg(target_os = "windows")]
fn create_platform_cdp_launcher_shortcut(
    launcher_path: &std::path::Path,
    port: u16,
    channel: Option<discord_cdp_launch_core::DiscordChannel>,
) -> Result<String, String> {
    use std::path::PathBuf;
    use std::process::Command;

    let desktop = std::env::var("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Desktop"))
        .map_err(|_| "Could not get desktop path".to_string())?;

    let shortcut_path = desktop.join("Discord CDP Launcher.lnk");
    let launcher_dir = launcher_path
        .parent()
        .ok_or_else(|| "Could not get launcher directory".to_string())?;
    let channel_arg = channel.map(|c| c.as_str()).unwrap_or("auto");
    let args = format!("--port {} --channel {}", port, channel_arg);

    let shortcut_path_ps = ps_single_quote(&shortcut_path.to_string_lossy());
    let launcher_path_ps = ps_single_quote(&launcher_path.to_string_lossy());
    let launcher_dir_ps = ps_single_quote(&launcher_dir.to_string_lossy());
    let args_ps = ps_single_quote(&args);

    let ps_script = format!(
        r#"
$ErrorActionPreference = 'Stop'
$WshShell = New-Object -ComObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut('{shortcut_path}')
$Shortcut.TargetPath = '{launcher_path}'
$Shortcut.Arguments = '{args}'
$Shortcut.WorkingDirectory = '{launcher_dir}'
$Shortcut.Description = 'Launch Discord with CDP enabled for Discord Quest Helper'
$Shortcut.IconLocation = '{launcher_path},0'
$Shortcut.Save()
"#,
        launcher_path = launcher_path_ps,
        args = args_ps,
        launcher_dir = launcher_dir_ps,
        shortcut_path = shortcut_path_ps,
    );

    let script_path = std::env::temp_dir().join(windows_shortcut_temp_ps1_name());
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
    channel: Option<discord_cdp_launch_core::DiscordChannel>,
) -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt;

    let home = std::env::var_os("HOME").ok_or_else(|| "Could not get HOME".to_string())?;
    let desktop = std::path::PathBuf::from(home).join("Desktop");
    let script_path = desktop.join("Discord CDP Launcher.command");
    let channel_arg = channel.map(|c| c.as_str()).unwrap_or("auto");

    // Use single quotes to prevent shell metacharacter expansion ($, `, \, ")
    fn shell_single_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    let script_content = format!(
        "#!/bin/bash\n{} --port {} --channel {}\n",
        shell_single_quote(&launcher_path.to_string_lossy()),
        port,
        channel_arg
    );

    std::fs::write(&script_path, &script_content)
        .map_err(|e| format!("Failed to write launcher command: {}", e))?;
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("Failed to mark launcher command executable: {}", e))?;

    Ok(script_path.to_string_lossy().to_string())
}

#[cfg(target_os = "linux")]
fn create_platform_cdp_launcher_shortcut(
    launcher_path: &std::path::Path,
    port: u16,
    channel: Option<discord_cdp_launch_core::DiscordChannel>,
) -> Result<String, String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    if port == 0 {
        return Err("CDP port must be between 1 and 65535.".to_string());
    }

    let data_home = linux_xdg_data_home()?;

    let applications_dir = data_home.join("applications");
    std::fs::create_dir_all(&applications_dir)
        .map_err(|e| format!("Failed to create applications directory: {}", e))?;

    // Desktop Entry icon names resolve through the freedesktop icon theme, not
    // through Tauri's bundled resources. Install the launcher's dedicated icon
    // alongside the .desktop entry so GNOME/KDE do not fall back to a generic
    // executable icon (especially in dev builds where the main app is not
    // installed system-wide).
    const ICON_NAME: &str = "com.masterain.discord-quest-helper.cdp";
    const ICON_BYTES: &[u8] = include_bytes!("../../public/icons/launcher-logo.png");
    let icon_theme_dir = data_home.join("icons").join("hicolor");
    let icon_dir = icon_theme_dir.join("512x512").join("apps");
    std::fs::create_dir_all(&icon_dir)
        .map_err(|e| format!("Failed to create launcher icon directory: {}", e))?;
    let icon_path = icon_dir.join(format!("{ICON_NAME}.png"));
    std::fs::write(&icon_path, ICON_BYTES)
        .map_err(|e| format!("Failed to install CDP launcher icon: {}", e))?;

    let desktop_path = linux_cdp_launcher_desktop_path()?;

    // `channel` is a Rust enum, so `as_str()` is always a fixed, safe token.
    let channel_arg = channel.map(|c| c.as_str()).unwrap_or("auto");
    let launcher_display = launcher_path.to_string_lossy();
    // A newline anywhere in the path would close the `Exec=`/`TryExec=` value
    // and let the rest be parsed as further Desktop Entry keys. Quoting cannot
    // express control characters, so reject them outright rather than emit a
    // file whose meaning depends on the reader's leniency.
    if launcher_display.contains(char::is_control) {
        return Err(
            "Launcher path contains control characters; refusing to write a desktop entry."
                .to_string(),
        );
    }
    let exec_program = desktop_entry_exec_quote(&launcher_display);

    let contents = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Discord CDP Launcher\n\
         Comment=Launch Discord with CDP enabled\n\
         Exec={exec} --port {port} --channel {channel}\n\
         TryExec={tryexec}\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=Utility;\n\
         StartupNotify=true\n",
        exec = exec_program,
        port = port,
        channel = channel_arg,
        tryexec = launcher_display,
        // Use the absolute path in the desktop entry. GNOME Shell can retain a
        // generic fallback cached before a newly installed themed icon exists;
        // a direct path avoids that stale theme lookup entirely.
        icon = icon_path.to_string_lossy(),
    );

    // Write to a temp file in the same directory, then atomically replace any
    // existing desktop entry. `rename` replaces the destination on Linux.
    let tmp_path = applications_dir.join(format!(
        ".com.masterain.discord-quest-helper.cdp.desktop.{}.tmp",
        std::process::id()
    ));
    {
        let mut file = std::fs::File::create(&tmp_path)
            .map_err(|e| format!("Failed to write desktop entry: {}", e))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("Failed to write desktop entry: {}", e))?;
        file.set_permissions(std::fs::Permissions::from_mode(0o644))
            .map_err(|e| format!("Failed to set desktop entry permissions: {}", e))?;
    }
    std::fs::rename(&tmp_path, &desktop_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("Failed to install desktop entry: {}", e)
    })?;

    // Best-effort refresh of the desktop database; failure is non-fatal.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&applications_dir)
        .status();
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .args(["-f", "-t"])
        .arg(&icon_theme_dir)
        .status();

    Ok(desktop_path.to_string_lossy().to_string())
}

/// Escape a value for use inside a double-quoted Desktop Entry `Exec` argument.
/// Reserved characters are escaped with a backslash; backslash is escaped first.
/// Field codes (`%f`, `%u`, …) are expanded before quoting is undone, so a
/// literal percent sign must be written as `%%` even inside quotes.
#[cfg(target_os = "linux")]
fn desktop_entry_exec_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%");
    format!("\"{}\"", escaped)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn create_platform_cdp_launcher_shortcut(
    _launcher_path: &std::path::Path,
    _port: u16,
    _channel: Option<discord_cdp_launch_core::DiscordChannel>,
) -> Result<String, String> {
    Err("Shortcut creation is only supported on Windows, macOS and Linux.".to_string())
}

#[cfg(all(test, target_os = "linux"))]
mod desktop_entry_tests {
    use super::{desktop_entry_exec_quote, linux_cdp_launcher_options_from_desktop};
    use discord_cdp_launch_core::DiscordChannel;

    #[test]
    fn quotes_plain_paths() {
        assert_eq!(
            desktop_entry_exec_quote("/usr/bin/discord-cdp-launcher"),
            "\"/usr/bin/discord-cdp-launcher\""
        );
    }

    #[test]
    fn escapes_reserved_shell_characters() {
        assert_eq!(
            desktop_entry_exec_quote(r#"/tmp/we"ir$d`\path"#),
            r#""/tmp/we\"ir\$d\`\\path""#
        );
    }

    #[test]
    fn doubles_literal_percent_so_it_is_not_read_as_a_field_code() {
        // `%f`/`%u` are expanded before quoting is undone, so a path containing
        // a percent sign must be written `%%` or the entry silently mangles it.
        assert_eq!(
            desktop_entry_exec_quote("/opt/My %f App/launcher"),
            "\"/opt/My %%f App/launcher\""
        );
    }

    #[test]
    fn keeps_existing_launcher_port_and_channel_during_dev_refresh() {
        let desktop = r#"[Desktop Entry]
Exec="/opt/Discord Quest Helper/discord-cdp-launcher" --port 9444 --channel canary
"#;
        assert_eq!(
            linux_cdp_launcher_options_from_desktop(desktop),
            (9444, Some(DiscordChannel::Canary))
        );
    }

    #[test]
    fn invalid_existing_launcher_options_fall_back_to_defaults() {
        let desktop = "Exec=/tmp/launcher --port 0 --channel unsupported\n";
        assert_eq!(
            linux_cdp_launcher_options_from_desktop(desktop),
            (super::cdp_client::DEFAULT_CDP_PORT, None)
        );
    }
}

#[cfg(test)]
mod windows_cdp_runtime_path_tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn unique_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "dqh-cdp-runtime-{}",
            runtime_identity::generate_random_suffix(8)
        ))
    }

    fn assert_bland_leaves(path: &Path) {
        let file = path.file_stem().and_then(|s| s.to_str()).unwrap();
        let dir = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap();
        assert!(
            runtime_identity::is_hex_str(dir, runtime_identity::DIR_HEX_LEN),
            "{dir}"
        );
        assert!(
            runtime_identity::is_hex_str(file, runtime_identity::FILE_HEX_LEN),
            "{file}"
        );
        assert!(!runtime_name_has_product_tokens(dir));
        assert!(!runtime_name_has_product_tokens(file));
        assert!(!runtime_name_has_product_tokens(&format!("{file}.exe")));
    }

    #[test]
    fn pointer_file_uses_app_config_dir() {
        let pointer = windows_cdp_runtime_pointer_path_from(Path::new("/roaming"));
        assert_eq!(
            pointer.file_name().and_then(|n| n.to_str()),
            Some(WINDOWS_CDP_RUNTIME_POINTER)
        );
        assert_eq!(
            pointer
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some(WINDOWS_CDP_APP_CONFIG_DIR)
        );
    }

    #[test]
    fn allocates_hex_layout_without_product_names() {
        let root = unique_root();
        let local = root.join("Local");
        let pointer = windows_cdp_runtime_pointer_path_from(&root.join("Roaming"));
        fs::create_dir_all(&local).unwrap();
        let path = resolve_windows_cdp_runtime_path(&local, &pointer);
        assert_bland_leaves(&path);
        assert_eq!(
            path.parent().and_then(|p| p.parent()),
            Some(local.as_path())
        );
        let stored = fs::read_to_string(&pointer).unwrap();
        assert_eq!(PathBuf::from(stored.trim()), path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reuses_pointer_when_file_still_exists() {
        let root = unique_root();
        let local = root.join("Local");
        let pointer = windows_cdp_runtime_pointer_path_from(&root.join("Roaming"));
        fs::create_dir_all(&local).unwrap();
        let first = resolve_windows_cdp_runtime_path(&local, &pointer);
        fs::create_dir_all(first.parent().unwrap()).unwrap();
        fs::write(&first, b"exe").unwrap();
        let second = resolve_windows_cdp_runtime_path(&local, &pointer);
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reallocates_when_pointer_target_is_missing() {
        let root = unique_root();
        let local = root.join("Local");
        let pointer = windows_cdp_runtime_pointer_path_from(&root.join("Roaming"));
        fs::create_dir_all(&local).unwrap();
        let first = resolve_windows_cdp_runtime_path(&local, &pointer);
        let second = resolve_windows_cdp_runtime_path(&local, &pointer);
        assert_ne!(first, second);
        assert_bland_leaves(&second);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ignores_legacy_product_path_in_pointer() {
        let root = unique_root();
        let local = root.join("Local");
        let pointer = windows_cdp_runtime_pointer_path_from(&root.join("Roaming"));
        fs::create_dir_all(pointer.parent().unwrap()).unwrap();
        fs::create_dir_all(&local).unwrap();
        let legacy = local
            .join(WINDOWS_LEGACY_CDP_DIR)
            .join(WINDOWS_LEGACY_CDP_EXE);
        fs::write(&pointer, legacy.to_string_lossy().as_bytes()).unwrap();
        let path = resolve_windows_cdp_runtime_path(&local, &pointer);
        assert!(is_windows_bland_runtime_exe(&path, &local));
        assert_bland_leaves(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migrates_legacy_discord_quest_helper_dir() {
        let root = unique_root();
        let local = root.join("Local");
        let old_dir = local.join(WINDOWS_LEGACY_CDP_DIR);
        fs::create_dir_all(&old_dir).unwrap();
        fs::write(old_dir.join(WINDOWS_LEGACY_CDP_EXE), b"old").unwrap();
        let new_target = allocate_windows_bland_runtime_exe(&local);
        migrate_legacy_windows_cdp_launcher_at(&local, &new_target);
        assert!(!old_dir.exists());
        assert!(new_target.is_file());
        assert_eq!(fs::read(&new_target).unwrap(), b"old");
        assert_bland_leaves(&new_target);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_does_not_overwrite_existing_target() {
        let root = unique_root();
        let local = root.join("Local");
        let old_dir = local.join(WINDOWS_LEGACY_CDP_DIR);
        fs::create_dir_all(&old_dir).unwrap();
        fs::write(old_dir.join(WINDOWS_LEGACY_CDP_EXE), b"old").unwrap();
        let new_target = allocate_windows_bland_runtime_exe(&local);
        fs::create_dir_all(new_target.parent().unwrap()).unwrap();
        fs::write(&new_target, b"new").unwrap();
        migrate_legacy_windows_cdp_launcher_at(&local, &new_target);
        assert!(!old_dir.exists());
        assert_eq!(fs::read(&new_target).unwrap(), b"new");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn shortcut_temp_script_is_hex_named() {
        let name = windows_shortcut_temp_ps1_name();
        let stem = name.strip_suffix(".ps1").unwrap();
        assert!(runtime_identity::is_hex_str(
            stem,
            runtime_identity::DIR_HEX_LEN
        ));
        assert!(!runtime_name_has_product_tokens(&name));
        assert!(!name.to_ascii_lowercase().contains("discord"));
    }
}

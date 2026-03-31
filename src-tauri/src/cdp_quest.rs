//! CDP-based quest completion module
//!
//! Injects JavaScript into the Discord client via Chrome DevTools Protocol to manipulate
//! Discord's internal webpack stores (RunningGameStore, QuestsStore, FluxDispatcher, etc.),
//! making Discord itself send signed heartbeats for quest progress.
//!
//! Based on aamiaa's CompleteDiscordQuest.js approach (GPL-3.0), adapted for CDP injection.
//! https://gist.github.com/aamiaa/204cd9d42013ded9faf646fae7f89fbb


use anyhow::{Context, Result};
use std::time::Duration;
use tauri::Emitter;
use tokio::time::sleep;

use crate::cdp_client;

/// JavaScript: Initialize quest-related Discord webpack modules and store them in window.__dqh_cdp.
///
/// Finds and caches references to:
/// - `RunningGameStore` — for spoofing running games
/// - `QuestsStore` — for querying quest progress
/// - `FluxDispatcher` — for dispatching state change events
/// - `ApplicationStreamingStore` — for spoofing stream metadata
/// - `api` — Discord's internal HTTP module (for video quests)
///
/// FRAGILE: Relies on Discord's internal webpack module structure.
const JS_INIT_QUEST_MODULES: &str = r#"
(() => {
    try {
        if (window.__dqh_cdp && window.__dqh_cdp.initialized) {
            return JSON.stringify({ success: true, cached: true });
        }

        delete window.$;
        let wpRequire = webpackChunkdiscord_app.push([[Symbol()], {}, r => r]);
        webpackChunkdiscord_app.pop();

        let modules = {
            RunningGameStore: null,
            QuestsStore: null,
            FluxDispatcher: null,
            ApplicationStreamingStore: null,
            api: null
        };

        // Scan all webpack modules — mirrors aamiaa's gist detection patterns
        // Each module is checked for ALL targets (no early continue)
        let scanned = 0;
        for (const m of Object.values(wpRequire.c)) {
            try {
                const exp = m?.exports;
                if (!exp) continue;
                scanned++;

                for (const key of Object.keys(exp)) {
                    try {
                        const val = exp[key];
                        if (!val) continue;

                        // FluxDispatcher: __proto__ has flushWaitQueue (gist pattern)
                        if (!modules.FluxDispatcher && val?.__proto__?.flushWaitQueue) {
                            modules.FluxDispatcher = val;
                        }

                        // ApplicationStreamingStore: __proto__ has getStreamerActiveStreamMetadata
                        if (!modules.ApplicationStreamingStore && val?.__proto__?.getStreamerActiveStreamMetadata) {
                            modules.ApplicationStreamingStore = val;
                        }

                        // RunningGameStore: direct access to getRunningGames (gist does NOT use __proto__)
                        if (!modules.RunningGameStore && val?.getRunningGames) {
                            modules.RunningGameStore = val;
                        }

                        // QuestsStore: __proto__ has getQuest
                        if (!modules.QuestsStore && val?.__proto__?.getQuest) {
                            modules.QuestsStore = val;
                        }

                        // API module: direct access to get (gist does NOT use __proto__)
                        if (!modules.api && val?.get && typeof val.get === 'function' && typeof val.post === 'function') {
                            modules.api = val;
                        }
                    } catch(e) {}
                }
            } catch (e) {
                continue;
            }
        }

        let missing = [];
        for (const [name, mod] of Object.entries(modules)) {
            if (!mod) missing.push(name);
        }

        if (missing.length > 0) {
            return JSON.stringify({ success: false, error: "Missing modules: " + missing.join(", ") + " (scanned " + scanned + " webpack modules)" });
        }

        window.__dqh_cdp = {
            ...modules,
            initialized: true,
            // Save original functions for cleanup
            _origGetRunningGames: modules.RunningGameStore.getRunningGames,
            _origGetGameForPID: modules.RunningGameStore.getGameForPID || null,
            _origGetStreamerActiveStreamMetadata: modules.ApplicationStreamingStore.getStreamerActiveStreamMetadata
        };

        return JSON.stringify({ success: true, cached: false });
    } catch (e) {
        return JSON.stringify({ success: false, error: String(e) });
    }
})()
"#;

/// Generate JS to spoof a running game in RunningGameStore.
///
/// Overrides `getRunningGames()` to return an array containing the spoofed game,
/// then dispatches `RUNNING_GAMES_CHANGE` so Discord's heartbeat system picks it up.
fn js_spoof_play_game(app_id: &str, app_name: &str) -> String {
    format!(r#"
(async () => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh || !dqh.initialized) return JSON.stringify({{ success: false, error: "Modules not initialized" }});

        const pid = Math.floor(Math.random() * 30000) + 1000;
        const applicationId = "{app_id}";
        const applicationName = "{app_name}";

        // Fetch real exe info from Discord's public API (same as gist)
        let exeName = applicationName.replace(/[\/\\:*?"<>|]/g, "") + ".exe";
        try {{
            const res = await dqh.api.get({{ url: "/applications/public?application_ids=" + applicationId }});
            if (res && res.body && res.body[0]) {{
                const appData = res.body[0];
                const exe = appData.executables?.find(x => x.os === "win32");
                if (exe && exe.name) {{
                    exeName = exe.name.replace(">","");
                }}
            }}
        }} catch(e) {{}}

        const fakeGame = {{
            cmdLine: "C:\\Program Files\\" + applicationName + "\\" + exeName,
            exeName: exeName,
            exePath: "c:/program files/" + applicationName.toLowerCase() + "/" + exeName,
            hidden: false,
            isLauncher: false,
            id: applicationId,
            name: applicationName,
            pid: pid,
            pidPath: [pid],
            processName: applicationName,
            start: Date.now()
        }};

        const realGames = dqh._origGetRunningGames();
        const fakeGames = [fakeGame];

        // Override store methods directly (same pattern as gist)
        dqh.RunningGameStore.getRunningGames = () => fakeGames;
        dqh.RunningGameStore.getGameForPID = (p) => fakeGames.find(x => x.pid === p);

        // Save fakeGame so cleanup can properly remove it
        dqh._fakeGame = fakeGame;

        dqh.FluxDispatcher.dispatch({{ type: "RUNNING_GAMES_CHANGE", removed: realGames, added: [fakeGame], games: fakeGames }});

        // Subscribe to heartbeat success events (same as gist) to track progress
        dqh._lastProgress = 0;
        dqh._completed = false;
        let heartbeatFn = data => {{
            try {{
                let progress = 0;
                if (data && data.userStatus) {{
                    if (data.userStatus.progress) {{
                        const vals = Object.values(data.userStatus.progress);
                        if (vals.length > 0 && vals[0].value !== undefined) {{
                            progress = Math.floor(vals[0].value);
                        }}
                    }} else if (data.userStatus.streamProgressSeconds !== undefined) {{
                        progress = data.userStatus.streamProgressSeconds;
                    }}
                    dqh._completed = !!data.userStatus.completedAt;
                }}
                dqh._lastProgress = progress;
            }} catch(e) {{}}
        }};
        dqh._heartbeatFn = heartbeatFn;
        dqh.FluxDispatcher.subscribe("QUESTS_SEND_HEARTBEAT_SUCCESS", heartbeatFn);

        return JSON.stringify({{ success: true, pid: pid }});
    }} catch (e) {{
        return JSON.stringify({{ success: false, error: String(e) }});
    }}
}})()
"#, app_id = app_id, app_name = app_name)
}

/// Generate JS to spoof streaming metadata in ApplicationStreamingStore.
///
/// Overrides `getStreamerActiveStreamMetadata()` to return metadata indicating
/// the user is streaming the specified application.
fn js_spoof_stream(app_id: &str) -> String {
    format!(r#"
(() => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh || !dqh.initialized) return JSON.stringify({{ success: false, error: "Modules not initialized" }});

        const pid = Math.floor(Math.random() * 30000) + 1000;

        dqh.ApplicationStreamingStore.getStreamerActiveStreamMetadata = () => ({{
            id: "{app_id}",
            pid: pid,
            sourceName: null
        }});

        // Subscribe to heartbeat success events for progress tracking
        dqh._lastProgress = 0;
        dqh._completed = false;
        let heartbeatFn = data => {{
            try {{
                let progress = 0;
                if (data && data.userStatus) {{
                    if (data.userStatus.progress) {{
                        const vals = Object.values(data.userStatus.progress);
                        if (vals.length > 0 && vals[0].value !== undefined) {{
                            progress = Math.floor(vals[0].value);
                        }}
                    }} else if (data.userStatus.streamProgressSeconds !== undefined) {{
                        progress = data.userStatus.streamProgressSeconds;
                    }}
                    dqh._completed = !!data.userStatus.completedAt;
                }}
                dqh._lastProgress = progress;
            }} catch(e) {{}}
        }};
        dqh._heartbeatFn = heartbeatFn;
        dqh.FluxDispatcher.subscribe("QUESTS_SEND_HEARTBEAT_SUCCESS", heartbeatFn);

        return JSON.stringify({{ success: true }});
    }} catch (e) {{
        return JSON.stringify({{ success: false, error: String(e) }});
    }}
}})()
"#, app_id = app_id)
}

/// Generate JS for video quest completion.
///
/// Uses Discord's internal `api.post()` to send video-progress updates,
/// bypassing external API signature requirements. Returns a Promise (async).
///
/// Mirrors the gist's time-bound approach: Discord validates that the
/// submitted timestamp doesn't exceed `(now - enrolledAt) + maxFuture`.
fn js_start_video_quest(quest_id: &str, seconds_needed: u32, initial_seconds: f64) -> String {
    format!(r#"
(async () => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh || !dqh.initialized) return JSON.stringify({{ success: false, error: "Modules not initialized" }});

        const questId = "{quest_id}";
        const secondsNeeded = {seconds_needed};
        let secondsDone = {initial_seconds};

        // Read enrolledAt from QuestsStore for time-bound calculation
        const quest = dqh.QuestsStore.getQuest(questId);
        if (!quest || !quest.userStatus || !quest.userStatus.enrolledAt) {{
            return JSON.stringify({{ success: false, error: "Quest not found or not enrolled" }});
        }}
        const enrolledAt = new Date(quest.userStatus.enrolledAt).getTime();

        // Time-bound parameters (same as gist)
        const maxFuture = 10;
        const speed = 7;
        const interval = 1;
        let completed = false;
        let consecutiveErrors = 0;
        const maxErrors = 10;

        // Store progress in dqh so polling can read it
        dqh._videoProgress = secondsDone;
        dqh._videoCompleted = false;
        dqh._videoError = null;

        while (true) {{
            const maxAllowed = Math.floor((Date.now() - enrolledAt) / 1000) + maxFuture;
            const diff = maxAllowed - secondsDone;
            const timestamp = secondsDone + speed;

            if (diff >= speed) {{
                try {{
                    const res = await dqh.api.post({{
                        url: "/quests/" + questId + "/video-progress",
                        body: {{ timestamp: Math.min(secondsNeeded, timestamp + Math.random()) }}
                    }});
                    completed = res?.body?.completed_at != null;
                    secondsDone = Math.min(secondsNeeded, timestamp);
                    consecutiveErrors = 0;
                    dqh._videoProgress = secondsDone;
                    dqh._videoCompleted = completed;
                }} catch (e) {{
                    consecutiveErrors++;
                    dqh._videoError = String(e);
                    if (consecutiveErrors >= maxErrors) {{
                        return JSON.stringify({{ success: false, error: "Too many consecutive errors (" + maxErrors + "): " + String(e), secondsDone: secondsDone }});
                    }}
                    await new Promise(r => setTimeout(r, 5000));
                    continue;
                }}
            }}

            if (timestamp >= secondsNeeded) {{
                break;
            }}
            await new Promise(r => setTimeout(r, interval * 1000));
        }}

        // Final submission to ensure completion
        if (!completed) {{
            try {{
                const res = await dqh.api.post({{
                    url: "/quests/" + questId + "/video-progress",
                    body: {{ timestamp: secondsNeeded }}
                }});
                completed = res?.body?.completed_at != null;
                dqh._videoCompleted = completed;
            }} catch(e) {{
                dqh._videoError = "Final post failed: " + String(e);
            }}
        }}

        dqh._videoProgress = secondsDone;
        return JSON.stringify({{ success: true, finalSeconds: secondsDone, completed: completed }});
    }} catch (e) {{
        if (window.__dqh_cdp) window.__dqh_cdp._videoError = String(e);
        return JSON.stringify({{ success: false, error: String(e) }});
    }}
}})()
"#, quest_id = quest_id, seconds_needed = seconds_needed, initial_seconds = initial_seconds)
}

/// Generate JS to query quest progress — reads from heartbeat subscription first, falls back to QuestsStore.
fn js_query_progress(quest_id: &str) -> String {
    format!(r#"
(() => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh || !dqh.initialized) return JSON.stringify({{ success: false, error: "Modules not initialized" }});

        // Check video quest progress (set by video JS loop)
        if (dqh._videoProgress !== undefined && dqh._videoProgress > 0) {{
            return JSON.stringify({{ success: true, progress: dqh._videoProgress, completed: !!dqh._videoCompleted, source: "video", error: dqh._videoError || null }});
        }}

        // Check heartbeat subscription data (for play/stream quests)
        if (dqh._lastProgress !== undefined && dqh._lastProgress > 0) {{
            return JSON.stringify({{ success: true, progress: dqh._lastProgress, completed: !!dqh._completed, source: "heartbeat" }});
        }}

        // Check if video quest has error (even at 0 progress)
        if (dqh._videoError) {{
            return JSON.stringify({{ success: true, progress: 0, completed: false, source: "video_error", error: dqh._videoError }});
        }}

        // Fallback: read from QuestsStore
        const quest = dqh.QuestsStore.getQuest("{quest_id}");
        if (!quest) return JSON.stringify({{ success: false, error: "Quest not found in QuestsStore" }});

        const userStatus = quest.userStatus;
        if (!userStatus) return JSON.stringify({{ success: true, progress: 0, completed: false, source: "store_no_status" }});

        const completed = !!userStatus.completedAt;

        let progressSeconds = 0;
        if (userStatus.progress) {{
            const vals = Object.values(userStatus.progress);
            if (vals.length > 0 && vals[0].value !== undefined) {{
                progressSeconds = vals[0].value;
            }}
        }} else if (userStatus.streamProgressSeconds !== undefined) {{
            progressSeconds = userStatus.streamProgressSeconds;
        }}

        return JSON.stringify({{ success: true, progress: progressSeconds, completed: completed, source: "store" }});
    }} catch (e) {{
        return JSON.stringify({{ success: false, error: String(e) }});
    }}
}})()
"#, quest_id = quest_id)
}

/// JavaScript: Cleanup spoofed store functions, restoring originals.
const JS_CLEANUP_SPOOF: &str = r#"
(() => {
    try {
        const dqh = window.__dqh_cdp;
        if (!dqh) return JSON.stringify({ success: true, message: "Nothing to clean up" });

        // Restore original functions (same pattern as gist)
        if (dqh._origGetRunningGames) {
            dqh.RunningGameStore.getRunningGames = dqh._origGetRunningGames;
        }
        if (dqh._origGetGameForPID) {
            dqh.RunningGameStore.getGameForPID = dqh._origGetGameForPID;
        }
        if (dqh._origGetStreamerActiveStreamMetadata) {
            dqh.ApplicationStreamingStore.getStreamerActiveStreamMetadata = dqh._origGetStreamerActiveStreamMetadata;
        }

        // Dispatch change to remove the spoofed game so Discord's heartbeat stops
        if (dqh.FluxDispatcher && dqh._fakeGame) {
            dqh.FluxDispatcher.dispatch({ type: "RUNNING_GAMES_CHANGE", removed: [dqh._fakeGame], added: [], games: [] });
        }

        // Unsubscribe any heartbeat listener
        if (dqh.FluxDispatcher && dqh._heartbeatFn) {
            dqh.FluxDispatcher.unsubscribe("QUESTS_SEND_HEARTBEAT_SUCCESS", dqh._heartbeatFn);
        }

        delete window.__dqh_cdp;
        return JSON.stringify({ success: true });
    } catch (e) {
        return JSON.stringify({ success: false, error: String(e) });
    }
})()
"#;

/// Initialize Discord webpack modules via CDP.
async fn cdp_init_modules(port: u16) -> Result<()> {
    use crate::logger::{log, LogLevel, LogCategory};

    let result = cdp_client::execute_js_via_cdp(port, JS_INIT_QUEST_MODULES, false, 15).await?;

    log(LogLevel::Debug, LogCategory::TokenExtraction,
        &format!("cdp_init_modules raw result: {}", &result), None);

    if result.is_empty() {
        anyhow::bail!("CDP returned empty result — JS expression may have returned undefined");
    }

    let parsed: serde_json::Value = serde_json::from_str(&result)
        .with_context(|| format!("Failed to parse init response as JSON: {}", &result.chars().take(500).collect::<String>()))?;

    if parsed.get("success") != Some(&serde_json::json!(true)) {
        let error = parsed.get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown init error");
        anyhow::bail!("CDP module initialization failed: {}", error);
    }

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!("CDP modules initialized successfully (cached: {})",
            parsed.get("cached").and_then(|c| c.as_bool()).unwrap_or(false)), None);

    Ok(())
}

/// Cleanup spoofed stores via CDP.
async fn cdp_cleanup(port: u16) {
    let _ = cdp_client::execute_js_via_cdp(port, JS_CLEANUP_SPOOF, false, 5).await;
}

/// Poll quest progress from Discord's heartbeat subscription / QuestsStore and emit Tauri events.
///
/// Returns `(progress_seconds, completed)`.
async fn cdp_poll_progress(port: u16, quest_id: &str) -> Result<(f64, bool)> {
    use crate::logger::{log, LogLevel, LogCategory};

    let js = js_query_progress(quest_id);
    let result = cdp_client::execute_js_via_cdp(port, &js, false, 10).await?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .context("Failed to parse progress response")?;

    if parsed.get("success") != Some(&serde_json::json!(true)) {
        let error = parsed.get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown progress error");
        anyhow::bail!("Progress query failed: {}", error);
    }

    let source = parsed.get("source")
        .and_then(|s| s.as_str())
        .unwrap_or("unknown");

    // Log video JS errors if present
    if let Some(err) = parsed.get("error").and_then(|e| e.as_str()) {
        log(LogLevel::Warn, LogCategory::TokenExtraction,
            &format!("CDP progress source: {} (JS error: {})", source, err), None);
    } else {
        log(LogLevel::Debug, LogCategory::TokenExtraction,
            &format!("CDP progress source: {}", source), None);
    }

    let progress = parsed.get("progress")
        .and_then(|p| p.as_f64())
        .unwrap_or(0.0);
    let completed = parsed.get("completed")
        .and_then(|c| c.as_bool())
        .unwrap_or(false);

    Ok((progress, completed))
}

/// Complete a PLAY_ON_DESKTOP quest via CDP.
///
/// 1. Initialize webpack modules
/// 2. Spoof RunningGameStore with the target game
/// 3. Discord's internal heartbeat takes over (sends signed heartbeats)
/// 4. Poll QuestsStore for progress until completion
/// 5. Cleanup spoofed stores
pub async fn complete_play_quest_via_cdp(
    port: u16,
    quest_id: String,
    app_id: String,
    app_name: String,
    seconds_needed: u32,
    initial_progress: f64,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    use crate::logger::{log, LogLevel, LogCategory};

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!("CDP play quest: quest_id={}, app_id={}, app_name={}", quest_id, app_id, app_name), None);

    // 1. Init modules
    cdp_init_modules(port).await
        .context("Failed to initialize CDP modules for play quest")?;

    // 2. Spoof running game
    let js = js_spoof_play_game(&app_id, &app_name);
    let result = cdp_client::execute_js_via_cdp(port, &js, true, 15).await?;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
    if parsed.get("success") != Some(&serde_json::json!(true)) {
        cdp_cleanup(port).await;
        anyhow::bail!("Failed to spoof running game: {}", 
            parsed.get("error").and_then(|e| e.as_str()).unwrap_or("unknown"));
    }

    log(LogLevel::Info, LogCategory::TokenExtraction,
        "CDP: Game spoofed successfully, Discord heartbeat active. Polling progress...", None);

    // 3. Poll progress
    let poll_interval = Duration::from_secs(20);
    let initial_pct = if seconds_needed > 0 { (initial_progress / seconds_needed as f64) * 100.0 } else { 0.0 };
    let _ = app_handle.emit("quest-progress", initial_pct);

    loop {
        tokio::select! {
            _ = sleep(poll_interval) => {},
            _ = cancel_rx.recv() => {
                log(LogLevel::Info, LogCategory::TokenExtraction, "CDP play quest cancelled", None);
                cdp_cleanup(port).await;
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }

        match cdp_poll_progress(port, &quest_id).await {
            Ok((progress_secs, completed)) => {
                let pct = if seconds_needed > 0 {
                    (progress_secs / seconds_needed as f64 * 100.0).min(100.0)
                } else { 0.0 };

                let _ = app_handle.emit("quest-progress", pct);
                log(LogLevel::Debug, LogCategory::TokenExtraction,
                    &format!("CDP play quest progress: {:.1}% ({:.0}/{}s)", pct, progress_secs, seconds_needed), None);

                if completed || pct >= 100.0 {
                    log(LogLevel::Info, LogCategory::TokenExtraction, "CDP play quest completed!", None);
                    cdp_cleanup(port).await;
                    let _ = app_handle.emit("quest-complete", ());
                    return Ok(());
                }
            }
            Err(e) => {
                log(LogLevel::Warn, LogCategory::TokenExtraction,
                    &format!("CDP progress poll failed (will retry): {}", e), None);
                // Continue polling despite errors — Discord may temporarily reject
            }
        }
    }
}

/// Complete a STREAM_ON_DESKTOP quest via CDP.
///
/// Similar to play quest but spoofs ApplicationStreamingStore.
pub async fn complete_stream_quest_via_cdp(
    port: u16,
    quest_id: String,
    app_id: String,
    seconds_needed: u32,
    initial_progress: f64,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    use crate::logger::{log, LogLevel, LogCategory};

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!("CDP stream quest: quest_id={}, app_id={}", quest_id, app_id), None);

    // 1. Init modules
    cdp_init_modules(port).await
        .context("Failed to initialize CDP modules for stream quest")?;

    // 2. Spoof streaming metadata
    let js = js_spoof_stream(&app_id);
    let result = cdp_client::execute_js_via_cdp(port, &js, false, 10).await?;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
    if parsed.get("success") != Some(&serde_json::json!(true)) {
        cdp_cleanup(port).await;
        anyhow::bail!("Failed to spoof stream: {}",
            parsed.get("error").and_then(|e| e.as_str()).unwrap_or("unknown"));
    }

    // Also spoof running game (stream quests also need the game running)
    let js_game = js_spoof_play_game(&app_id, "StreamedApp");
    let _ = cdp_client::execute_js_via_cdp(port, &js_game, true, 15).await;

    log(LogLevel::Info, LogCategory::TokenExtraction,
        "CDP: Stream spoofed successfully. Polling progress...", None);

    // 3. Poll progress
    let poll_interval = Duration::from_secs(20);
    let initial_pct = if seconds_needed > 0 { (initial_progress / seconds_needed as f64) * 100.0 } else { 0.0 };
    let _ = app_handle.emit("quest-progress", initial_pct);

    loop {
        tokio::select! {
            _ = sleep(poll_interval) => {},
            _ = cancel_rx.recv() => {
                log(LogLevel::Info, LogCategory::TokenExtraction, "CDP stream quest cancelled", None);
                cdp_cleanup(port).await;
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }

        match cdp_poll_progress(port, &quest_id).await {
            Ok((progress_secs, completed)) => {
                let pct = if seconds_needed > 0 {
                    (progress_secs / seconds_needed as f64 * 100.0).min(100.0)
                } else { 0.0 };

                let _ = app_handle.emit("quest-progress", pct);
                log(LogLevel::Debug, LogCategory::TokenExtraction,
                    &format!("CDP stream quest progress: {:.1}%", pct), None);

                if completed || pct >= 100.0 {
                    log(LogLevel::Info, LogCategory::TokenExtraction, "CDP stream quest completed!", None);
                    cdp_cleanup(port).await;
                    let _ = app_handle.emit("quest-complete", ());
                    return Ok(());
                }
            }
            Err(e) => {
                log(LogLevel::Warn, LogCategory::TokenExtraction,
                    &format!("CDP stream progress poll failed (will retry): {}", e), None);
            }
        }
    }
}

/// Complete a WATCH_VIDEO quest via CDP.
///
/// Uses Discord's internal `api.post()` to submit video progress,
/// bypassing the need for external API headers/signatures.
/// The JS runs as an async loop inside Discord's context.
pub async fn complete_video_quest_via_cdp(
    port: u16,
    quest_id: String,
    seconds_needed: u32,
    initial_progress: f64,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    use crate::logger::{log, LogLevel, LogCategory};

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!("CDP video quest: quest_id={}, target={}s, initial={:.0}s", 
            quest_id, seconds_needed, initial_progress), None);

    // 1. Init modules
    cdp_init_modules(port).await
        .context("Failed to initialize CDP modules for video quest")?;

    let initial_pct = if seconds_needed > 0 { (initial_progress / seconds_needed as f64) * 100.0 } else { 0.0 };
    let _ = app_handle.emit("quest-progress", initial_pct);

    // 2. Run the async video completion script inside Discord.
    //    This uses awaitPromise=true so CDP waits for the entire loop to finish.
    //    Meanwhile, we poll progress separately to emit events.
    let js = js_start_video_quest(&quest_id, seconds_needed, initial_progress);

    // Spawn the JS execution (runs for potentially minutes)
    // Timeout: at ~7x speed, needs ~(remaining/7) seconds + buffer for time-bound waits
    let remaining = (seconds_needed as f64 - initial_progress).max(0.0);
    let timeout = ((remaining / 7.0).ceil() as u64 + 120).max(180); // generous buffer

    let port_clone = port;
    let js_clone = js.clone();
    let js_handle = tokio::spawn(async move {
        cdp_client::execute_js_via_cdp(port_clone, &js_clone, true, timeout).await
    });

    // 3. Poll progress while the video JS loop runs
    let poll_interval = Duration::from_secs(10);
    loop {
        tokio::select! {
            _ = sleep(poll_interval) => {},
            _ = cancel_rx.recv() => {
                log(LogLevel::Info, LogCategory::TokenExtraction, "CDP video quest cancelled", None);
                js_handle.abort();
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }

        // Check if the JS execution has finished
        if js_handle.is_finished() {
            break;
        }

        // Poll progress
        match cdp_poll_progress(port, &quest_id).await {
            Ok((progress_secs, completed)) => {
                let pct = if seconds_needed > 0 {
                    (progress_secs / seconds_needed as f64 * 100.0).min(100.0)
                } else { 0.0 };

                let _ = app_handle.emit("quest-progress", pct);
                log(LogLevel::Debug, LogCategory::TokenExtraction,
                    &format!("CDP video quest progress: {:.1}% ({:.0}/{}s)", pct, progress_secs, seconds_needed), None);

                if completed || pct >= 100.0 {
                    log(LogLevel::Info, LogCategory::TokenExtraction, "CDP video quest completed via progress poll!", None);
                    js_handle.abort();
                    let _ = app_handle.emit("quest-complete", ());
                    return Ok(());
                }
            }
            Err(e) => {
                log(LogLevel::Warn, LogCategory::TokenExtraction,
                    &format!("CDP video progress poll failed (will retry): {}", e), None);
            }
        }
    }

    // JS execution finished — check result
    match js_handle.await {
        Ok(Ok(result)) => {
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
            if parsed.get("success") == Some(&serde_json::json!(true)) {
                log(LogLevel::Info, LogCategory::TokenExtraction, "CDP video quest completed!", None);
                let _ = app_handle.emit("quest-progress", 100.0f64);
                let _ = app_handle.emit("quest-complete", ());
            } else {
                let error = parsed.get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("Unknown video error");
                log(LogLevel::Error, LogCategory::TokenExtraction,
                    &format!("CDP video quest JS error: {}", error), None);
                let _ = app_handle.emit("quest-error", format!("Video quest failed: {}", error));
            }
        }
        Ok(Err(e)) => {
            log(LogLevel::Error, LogCategory::TokenExtraction,
                &format!("CDP video quest execution error: {}", e), None);
            let _ = app_handle.emit("quest-error", format!("Video quest CDP error: {}", e));
        }
        Err(e) => {
            if e.is_cancelled() {
                log(LogLevel::Info, LogCategory::TokenExtraction, "CDP video quest task was cancelled", None);
            } else {
                log(LogLevel::Error, LogCategory::TokenExtraction,
                    &format!("CDP video quest task panic: {}", e), None);
                let _ = app_handle.emit("quest-error", format!("Video quest task error: {}", e));
            }
        }
    }

    Ok(())
}

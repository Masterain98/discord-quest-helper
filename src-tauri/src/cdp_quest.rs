//! CDP-based quest completion module
//!
//! Injects JavaScript into the Discord client via Chrome DevTools Protocol to manipulate
//! Discord's internal webpack stores (RunningGameStore, QuestsStore, FluxDispatcher, etc.),
//! making Discord itself send signed heartbeats for quest progress.
//!
//! Inspired by the approach described in aamiaa's CompleteDiscordQuest.js
//! (https://gist.github.com/aamiaa/204cd9d42013ded9faf646fae7f89fbb).
//! This is a clean-room Rust/CDP reimplementation; no source code was copied.


use anyhow::{Context, Result};
use std::time::Duration;
use tauri::Emitter;
use tokio::time::sleep;

use crate::cdp_client;

const QUEST_HOME_URL: &str = "https://discord.com/quest-home";
const QUEST_HOME_DETOUR_URL: &str = "https://discord.com/store";
const QUEST_WARMUP_NAV_TIMEOUT_SECS: u64 = 20;
const QUEST_WARMUP_DWELL_MS: u64 = 1500;
const QUEST_WARMUP_RESTORE_SETTLE_MS: u64 = 800;

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
(async () => {
    try {
        const DQH_INIT_VERSION = 4;
        if (window.__dqh_cdp && window.__dqh_cdp.initialized && window.__dqh_cdp._initVersion === DQH_INIT_VERSION) {
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

        // Phase 1: Scan all webpack modules for stores (prototype-based detection)
        // and collect API module candidates (anything with get + post)
        let scanned = 0;
        let apiCandidates = [];
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

                        // Collect API candidates: any module with get + post functions
                        if (typeof val?.get === 'function' && typeof val?.post === 'function') {
                            apiCandidates.push(val);
                        }
                    } catch(e) {}
                }
            } catch (e) {
                continue;
            }
        }

        // Phase 2: Identify the real HTTP API module via behavioral test.
        // Multiple webpack modules may have get/post that return Promises, but only
        // the real HTTP API module's Promises actually settle (resolve/reject).
        // Other modules (e.g. router, RPC) return Promises that may never settle.
        // We test by calling .get({url:""}) and racing it against a 3s timeout.
        // The real API will reject quickly with a 404-type error.
        const TIMEOUT_MS = 3000;
        let apiTestedCount = 0;
        for (const candidate of apiCandidates) {
            try {
                const r = candidate.get({url: ""});
                if (!r || typeof r.then !== 'function') continue;
                apiTestedCount++;

                // Race the test call against a timeout
                const settled = await Promise.race([
                    r.then(() => "ok", () => "err"),
                    new Promise(resolve => setTimeout(() => resolve("timeout"), TIMEOUT_MS))
                ]);

                if (settled !== "timeout") {
                    // This candidate's Promise actually settled — it's the real HTTP API
                    modules.api = candidate;
                    break;
                }
                // Timed out — not the real API, try next candidate
            } catch(e) {
                // Sync throw = not HTTP API
            }
        }

        let missing = [];
        for (const [name, mod] of Object.entries(modules)) {
            if (!mod) missing.push(name);
        }

        if (missing.length > 0) {
            return JSON.stringify({ success: false, error: "Missing modules: " + missing.join(", ") + " (scanned " + scanned + " modules, " + apiCandidates.length + " API candidates, " + apiTestedCount + " tested)" });
        }

        window.__dqh_cdp = {
            ...modules,
            initialized: true,
            _initVersion: DQH_INIT_VERSION,
            // Save original functions for cleanup
            _origGetRunningGames: modules.RunningGameStore.getRunningGames,
            _origGetGameForPID: modules.RunningGameStore.getGameForPID || null,
            _origGetStreamerActiveStreamMetadata: modules.ApplicationStreamingStore.getStreamerActiveStreamMetadata
        };

        return JSON.stringify({ success: true, cached: false, apiCandidates: apiCandidates.length, apiTested: apiTestedCount });
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
    // Safely escape values for embedding in JS string literals
    let safe_app_id = serde_json::to_string(app_id).unwrap_or_else(|_| "\"\"" .to_string());
    let safe_app_name = serde_json::to_string(app_name).unwrap_or_else(|_| "\"\"" .to_string());
    format!(r#"
(async () => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh || !dqh.initialized) return JSON.stringify({{ success: false, error: "Modules not initialized" }});

        const pid = Math.floor(Math.random() * 30000) + 1000;
        const applicationId = {safe_app_id};
        const applicationName = {safe_app_name};

        // Fetch real exe info from Discord's public API (same as gist)
        let exeName = applicationName.replace(/[\/\\:*?"<>|]/g, "") + ".exe";
        let allExeNames = [];
        let appDataDebug = null;
        try {{
            const res = await dqh.api.get({{ url: "/applications/public?application_ids=" + applicationId }});
            if (res && res.body && res.body[0]) {{
                const appData = res.body[0];
                appDataDebug = appData.name;
                const allExes = (appData.executables || []).filter(x => x.os === "win32");
                allExeNames = allExes.map(x => x.name);
                const exe = allExes[0];
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

        // Call original function WITH proper this-context (unlike dqh._origGetRunningGames())
        let realGames = [];
        try {{ realGames = dqh._origGetRunningGames.call(dqh.RunningGameStore); }} catch(e) {{
            try {{ realGames = dqh._origGetRunningGames(); }} catch(e2) {{ realGames = []; }}
        }}
        const fakeGames = [fakeGame];

        // Override store methods directly (same pattern as gist)
        dqh.RunningGameStore.getRunningGames = () => fakeGames;
        dqh.RunningGameStore.getGameForPID = (p) => fakeGames.find(x => x.pid === p);

        // Save fakeGame so cleanup can properly remove it
        dqh._fakeGame = fakeGame;
        dqh._spoofActive = true;

        // Gist-style broad patch: re-scan ALL webpack modules and override every
        // getRunningGames reference found. Discord holds multiple module copies —
        // patching only the one found during init scan is not always sufficient.
        let patchCount = 1; // already patched dqh.RunningGameStore above
        const broadPatched = [];
        try {{
            const wpReq = webpackChunkdiscord_app.push([[Symbol()], {{}}, r => r]);
            webpackChunkdiscord_app.pop();
            for (const m of Object.values(wpReq.c)) {{
                try {{
                    const exp = m?.exports;
                    if (!exp) continue;
                    for (const key of Object.keys(exp)) {{
                        try {{
                            const val = exp[key];
                            if (val && val !== dqh.RunningGameStore && typeof val.getRunningGames === 'function') {{
                                const origFn = val.getRunningGames;
                                const origPidFn = typeof val.getGameForPID === 'function' ? val.getGameForPID : null;
                                val.getRunningGames = () => fakeGames;
                                if (origPidFn) val.getGameForPID = (p) => fakeGames.find(x => x.pid === p);
                                broadPatched.push({{ val, origFn, origPidFn }});
                                patchCount++;
                            }}
                        }} catch(e) {{}}
                    }}
                }} catch(e) {{}}
            }}
        }} catch(e) {{}}
        dqh._broadPatched = broadPatched;

        // CRITICAL: Hook FluxDispatcher.dispatch to intercept RUNNING_GAMES_CHANGE events.
        // Discord's native game scanner runs periodically and dispatches RUNNING_GAMES_CHANGE
        // with the REAL (empty) process list. This clears our fake game from the heartbeat
        // manager's state, preventing quest progress. By hooking dispatch, we ensure our
        // fake game is always present in any RUNNING_GAMES_CHANGE event, even those
        // dispatched by the native scanner.
        dqh._dispatchInterceptCount = 0;
        if (!dqh._origDispatch) {{
            const origDispatch = dqh.FluxDispatcher.dispatch.bind(dqh.FluxDispatcher);
            dqh._origDispatch = origDispatch;
            dqh.FluxDispatcher.dispatch = function(event) {{
                if (event && event.type === "RUNNING_GAMES_CHANGE" && dqh._fakeGame && dqh._spoofActive) {{
                    if (!event.games) event.games = [];
                    const hasFake = event.games.some(g => g.id === dqh._fakeGame.id || g.pid === dqh._fakeGame.pid);
                    if (!hasFake) {{
                        // Native scanner cleared our fake game — re-inject it
                        event.games.push(dqh._fakeGame);
                        if (!event.added) event.added = [];
                        event.added.push(dqh._fakeGame);
                        if (event.removed) {{
                            event.removed = event.removed.filter(g => g.id !== dqh._fakeGame.id && g.pid !== dqh._fakeGame.pid);
                        }}
                        dqh._dispatchInterceptCount++;
                    }}
                }}
                return origDispatch(event);
            }};
        }}

        dqh.FluxDispatcher.dispatch({{ type: "RUNNING_GAMES_CHANGE", removed: realGames, added: [fakeGame], games: fakeGames }});

        // Subscribe to heartbeat success events (same as gist) to track progress
        dqh._lastProgress = 0;
        dqh._completed = false;
        dqh._heartbeatCount = 0;
        dqh._lastHeartbeatRaw = null;
        let heartbeatFn = data => {{
            try {{
                dqh._heartbeatCount++;
                try {{ dqh._lastHeartbeatRaw = JSON.stringify(data).substring(0, 500); }} catch(e2) {{}}
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

        // Also subscribe to heartbeat failure for diagnostics
        dqh._lastHeartbeatFailure = null;
        let heartbeatFailFn = data => {{
            try {{
                dqh._lastHeartbeatFailure = JSON.stringify(data).substring(0, 500);
            }} catch(e) {{
                dqh._lastHeartbeatFailure = "failed to serialize";
            }}
        }};
        dqh._heartbeatFailFn = heartbeatFailFn;
        dqh.FluxDispatcher.subscribe("QUESTS_SEND_HEARTBEAT_FAILURE", heartbeatFailFn);

        return JSON.stringify({{ success: true, pid: pid, patchCount: patchCount, exeName: exeName, allExeNames: allExeNames, appDataName: appDataDebug, realGamesCount: realGames.length }});
    }} catch (e) {{
        return JSON.stringify({{ success: false, error: String(e) }});
    }}
}})()
"#)
}

/// Generate JS to spoof streaming metadata in ApplicationStreamingStore.
///
/// Overrides `getStreamerActiveStreamMetadata()` to return metadata indicating
/// the user is streaming the specified application.
fn js_spoof_stream(app_id: &str) -> String {
    let safe_app_id = serde_json::to_string(app_id).unwrap_or_else(|_| "\"\"".to_string());
    format!(r#"
(() => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh || !dqh.initialized) return JSON.stringify({{ success: false, error: "Modules not initialized" }});

        const pid = Math.floor(Math.random() * 30000) + 1000;

        dqh.ApplicationStreamingStore.getStreamerActiveStreamMetadata = () => ({{
            id: {safe_app_id},
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
"#)
}

/// Generate JS for video quest completion (fire-and-forget pattern).
///
/// Uses Discord's internal `api.post()` to send video-progress updates,
/// bypassing external API signature requirements.
///
/// The async loop is launched and stored as a global Promise (to prevent GC).
/// Progress/completion/errors are written to `window.__dqh_cdp._video*` fields
/// and polled from Rust. This avoids CDP's `awaitPromise` which is fragile for
/// long-running Promises ("Promise was collected" error).
///
/// Mirrors the gist's time-bound approach: Discord validates that the
/// submitted timestamp doesn't exceed `(now - enrolledAt) + maxFuture`.
fn js_start_video_quest(quest_id: &str, seconds_needed: u32, initial_seconds: f64) -> String {
    format!(r#"
(() => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh || !dqh.initialized) return JSON.stringify({{ success: false, error: "Modules not initialized" }});

        const questId = "{quest_id}";
        const secondsNeeded = {seconds_needed};

        // Read enrolledAt from QuestsStore for time-bound calculation
        const quest = dqh.QuestsStore.getQuest(questId);
        if (!quest || !quest.userStatus || !quest.userStatus.enrolledAt) {{
            return JSON.stringify({{ success: false, error: "Quest not found or not enrolled" }});
        }}

        // Initialize video state fields (polled by Rust)
        dqh._videoQuestId = questId;
        dqh._videoProgress = {initial_seconds};
        dqh._videoCompleted = false;
        dqh._videoError = null;
        dqh._videoResult = null;
        dqh._videoRunning = true;

        // Launch the async loop and store the Promise globally to prevent V8 GC
        dqh._videoPromise = (async () => {{
            try {{
                let secondsDone = {initial_seconds};
                const enrolledAt = new Date(quest.userStatus.enrolledAt).getTime();
                const maxFuture = 10;
                const speed = 7;
                const interval = 1;
                let completed = false;
                let consecutiveErrors = 0;
                const maxErrors = 10;
                let debugFirstResponse = null;
                let apiCallCount = 0;
                const API_TIMEOUT = 15000; // 15s timeout per API call

                // Helper: call api.post with a timeout to prevent hanging on wrong module
                function apiPost(opts) {{
                    return Promise.race([
                        dqh.api.post(opts),
                        new Promise((_, reject) => setTimeout(() => reject(new Error("API call timed out after " + API_TIMEOUT + "ms — possible wrong API module")), API_TIMEOUT))
                    ]);
                }}

                while (true) {{
                    const maxAllowed = Math.floor((Date.now() - enrolledAt) / 1000) + maxFuture;
                    const diff = maxAllowed - secondsDone;
                    const timestamp = secondsDone + speed;

                    if (diff >= speed) {{
                        try {{
                            const res = await apiPost({{
                                url: "/quests/" + questId + "/video-progress",
                                body: {{ timestamp: Math.min(secondsNeeded, timestamp + Math.random()) }}
                            }});
                            apiCallCount++;
                            if (!debugFirstResponse) {{
                                try {{ debugFirstResponse = JSON.stringify(res).substring(0, 500); }} catch(e2) {{ debugFirstResponse = String(res); }}
                                // Validate: real API returns object with body, wrong module returns locale/ast
                                if (res && !res.body && (res.locale || res.ast !== undefined)) {{
                                    const err = "API module mismatch: got i18n/locale response instead of HTTP API. Response: " + debugFirstResponse;
                                    dqh._videoError = err;
                                    dqh._videoResult = JSON.stringify({{ success: false, error: err, apiModuleWrong: true }});
                                    dqh._videoRunning = false;
                                    return;
                                }}
                            }}
                            completed = res?.body?.completed_at != null;
                            secondsDone = Math.min(secondsNeeded, timestamp);
                            consecutiveErrors = 0;
                            dqh._videoProgress = secondsDone;
                            dqh._videoCompleted = completed;
                        }} catch (e) {{
                            consecutiveErrors++;
                            dqh._videoError = String(e);
                            if (consecutiveErrors >= maxErrors) {{
                                dqh._videoResult = JSON.stringify({{ success: false, error: "Too many consecutive errors (" + maxErrors + "): " + String(e), secondsDone, apiCallCount, debugFirstResponse }});
                                dqh._videoRunning = false;
                                return;
                            }}
                            await new Promise(r => setTimeout(r, 5000));
                            continue;
                        }}
                    }}

                    if (completed || secondsDone >= secondsNeeded) {{
                        break;
                    }}
                    await new Promise(r => setTimeout(r, interval * 1000));
                }}

                // Final submission to ensure completion
                if (!completed) {{
                    try {{
                        const res = await apiPost({{
                            url: "/quests/" + questId + "/video-progress",
                            body: {{ timestamp: secondsNeeded }}
                        }});
                        apiCallCount++;
                        if (!debugFirstResponse) {{
                            try {{ debugFirstResponse = JSON.stringify(res).substring(0, 500); }} catch(e2) {{ debugFirstResponse = String(res); }}
                        }}
                        completed = res?.body?.completed_at != null;
                        dqh._videoCompleted = completed;
                    }} catch(e) {{
                        dqh._videoError = "Final post failed: " + String(e);
                    }}
                }}

                // Read actual quest status from QuestsStore for verification
                let storeProgress = null;
                let storeCompleted = false;
                try {{
                    const q = dqh.QuestsStore.getQuest(questId);
                    if (q && q.userStatus) {{
                        storeCompleted = !!q.userStatus.completedAt;
                        if (q.userStatus.progress) {{
                            const vals = Object.values(q.userStatus.progress);
                            if (vals.length > 0 && vals[0].value !== undefined) {{
                                storeProgress = vals[0].value;
                            }}
                        }}
                    }}
                }} catch(e) {{}}

                dqh._videoProgress = secondsDone;
                dqh._videoResult = JSON.stringify({{ success: true, finalSeconds: secondsDone, completed, apiCallCount, debugFirstResponse, storeProgress, storeCompleted }});
            }} catch (e) {{
                dqh._videoError = String(e);
                dqh._videoResult = JSON.stringify({{ success: false, error: String(e) }});
            }} finally {{
                dqh._videoRunning = false;
            }}
        }})();

        return JSON.stringify({{ success: true, started: true }});
    }} catch (e) {{
        return JSON.stringify({{ success: false, error: String(e) }});
    }}
}})()
"#, quest_id = quest_id, seconds_needed = seconds_needed, initial_seconds = initial_seconds)
}

/// Generate JS to query quest progress.
///
/// Priority order:
/// 1. Video quest state (set by JS video loop, polled from `_videoProgress`)
/// 2. Direct API call via `dqh.api.get("/quests/@me")` — most reliable for play/stream quests
///    because QuestsStore cache is stale and QUESTS_SEND_HEARTBEAT_SUCCESS may not fire reliably
/// 3. Heartbeat subscription data (`_lastProgress`)
/// 4. QuestsStore fallback (may be stale)
fn js_query_progress(quest_id: &str) -> String {
    format!(r#"
(async () => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh || !dqh.initialized) return JSON.stringify({{ success: false, error: "Modules not initialized" }});

        // Check video quest progress (set by video JS loop) — only if this quest owns the video state
        const isVideoQuest = dqh._videoQuestId === "{quest_id}";
        if (isVideoQuest && dqh._videoProgress !== undefined && dqh._videoProgress > 0) {{
            return JSON.stringify({{ success: true, progress: dqh._videoProgress, completed: !!dqh._videoCompleted, source: "video", error: dqh._videoError || null, videoResult: dqh._videoResult || null, videoRunning: !!dqh._videoRunning }});
        }}
        if (isVideoQuest && dqh._videoResult) {{
            return JSON.stringify({{ success: true, progress: dqh._videoProgress || 0, completed: !!dqh._videoCompleted, source: "video_result", videoResult: dqh._videoResult, videoRunning: false }});
        }}
        if (isVideoQuest && dqh._videoError) {{
            return JSON.stringify({{ success: true, progress: 0, completed: false, source: "video_error", error: dqh._videoError, videoRunning: !!dqh._videoRunning }});
        }}

        // Diagnostics: running games count + heartbeat failure info + dispatch intercept count
        let diagRunning = -1;
        try {{ diagRunning = dqh.RunningGameStore.getRunningGames().length; }} catch(e) {{}}
        const diagHbFail = dqh._lastHeartbeatFailure || null;
        const diagHbProgress = dqh._lastProgress || 0;
        const diagHbCount = dqh._heartbeatCount || 0;
        const diagInterceptCount = dqh._dispatchInterceptCount || 0;

        // For play/stream quests: fetch fresh progress directly from Discord API.
        // QuestsStore cache is stale and QUESTS_SEND_HEARTBEAT_SUCCESS may not fire.
        if (dqh.api) {{
            try {{
                const res = await dqh.api.get({{ url: "/quests/@me" }});
                if (res && res.body && Array.isArray(res.body)) {{
                    const quest = res.body.find(q => q.id === "{quest_id}");
                    if (quest && quest.user_status) {{
                        const completed = !!quest.user_status.completed_at;
                        let progressSeconds = 0;
                        if (quest.user_status.progress) {{
                            const vals = Object.values(quest.user_status.progress);
                            if (vals.length > 0 && vals[0].value !== undefined) {{
                                progressSeconds = vals[0].value;
                            }}
                        }} else if (quest.user_status.stream_progress_seconds !== undefined) {{
                            progressSeconds = quest.user_status.stream_progress_seconds;
                        }}
                        return JSON.stringify({{ success: true, progress: progressSeconds, completed, source: "api",
                            diagRunningGames: diagRunning, diagHeartbeatFailure: diagHbFail,
                            diagHeartbeatProgress: diagHbProgress, diagHeartbeatCount: diagHbCount, diagInterceptCount: diagInterceptCount }});
                    }}
                }}
            }} catch(e) {{
                // API call failed, fall through to other sources
            }}
        }}

        // Heartbeat subscription data
        if (dqh._lastProgress !== undefined && dqh._lastProgress > 0) {{
            return JSON.stringify({{ success: true, progress: dqh._lastProgress, completed: !!dqh._completed, source: "heartbeat",
                diagRunningGames: diagRunning, diagHeartbeatFailure: diagHbFail, diagInterceptCount: diagInterceptCount }});
        }}

        // Fallback: QuestsStore (may be stale)
        const quest = dqh.QuestsStore.getQuest("{quest_id}");
        if (!quest) return JSON.stringify({{ success: false, error: "Quest not found in QuestsStore" }});

        const userStatus = quest.userStatus;
        if (!userStatus) return JSON.stringify({{ success: true, progress: 0, completed: false, source: "store_no_status",
            diagRunningGames: diagRunning, diagHeartbeatFailure: diagHbFail, diagInterceptCount: diagInterceptCount }});

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

        return JSON.stringify({{ success: true, progress: progressSeconds, completed, source: "store",
            diagRunningGames: diagRunning, diagHeartbeatFailure: diagHbFail, diagInterceptCount: diagInterceptCount }});
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

        // Deactivate spoof FIRST so the dispatch interceptor stops re-injecting
        dqh._spoofActive = false;

        // Restore FluxDispatcher.dispatch (remove our interceptor)
        if (dqh._origDispatch) {
            dqh.FluxDispatcher.dispatch = dqh._origDispatch;
            delete dqh._origDispatch;
        }

        // Restore original functions (same pattern as gist)
        if (dqh._origGetRunningGames) {
            dqh.RunningGameStore.getRunningGames = dqh._origGetRunningGames;
        }
        if (typeof dqh._origGetGameForPID === "function") {
            dqh.RunningGameStore.getGameForPID = dqh._origGetGameForPID;
        } else {
            // If the original store had no getGameForPID, remove the spoofed method.
            try {
                delete dqh.RunningGameStore.getGameForPID;
            } catch(e) {
                dqh.RunningGameStore.getGameForPID = undefined;
            }
        }
        if (dqh._origGetStreamerActiveStreamMetadata) {
            dqh.ApplicationStreamingStore.getStreamerActiveStreamMetadata = dqh._origGetStreamerActiveStreamMetadata;
        }

        // Restore broadly patched modules BEFORE dispatching removal event
        // Otherwise heartbeat manager may re-query getRunningGames() from a still-patched
        // module and re-detect the fake game immediately after removal
        if (Array.isArray(dqh._broadPatched)) {
            for (const patch of dqh._broadPatched) {
                try {
                    patch.val.getRunningGames = patch.origFn;
                    if (patch.origPidFn) patch.val.getGameForPID = patch.origPidFn;
                } catch(e) {}
            }
        }

        // Unsubscribe heartbeat listeners BEFORE dispatching removal
        if (dqh.FluxDispatcher && dqh._heartbeatFn) {
            dqh.FluxDispatcher.unsubscribe("QUESTS_SEND_HEARTBEAT_SUCCESS", dqh._heartbeatFn);
        }
        if (dqh.FluxDispatcher && dqh._heartbeatFailFn) {
            dqh.FluxDispatcher.unsubscribe("QUESTS_SEND_HEARTBEAT_FAILURE", dqh._heartbeatFailFn);
        }

        // NOW dispatch removal event — all patches are already restored
        if (dqh.FluxDispatcher && dqh._fakeGame) {
            dqh.FluxDispatcher.dispatch({ type: "RUNNING_GAMES_CHANGE", removed: [dqh._fakeGame], added: [], games: [] });
        }

        delete window.__dqh_cdp;
        return JSON.stringify({ success: true });
    } catch (e) {
        return JSON.stringify({ success: false, error: String(e) });
    }
})()
"#;

/// JavaScript: verify whether spoof state is still present in this page target.
const JS_VERIFY_CLEANUP_STATE: &str = r#"
(() => {
    try {
        const dqh = window.__dqh_cdp;
        return JSON.stringify({
            success: true,
            dqhPresent: !!dqh,
            spoofActive: !!dqh?._spoofActive,
            fakeGamePresent: !!dqh?._fakeGame,
            hasDispatchHook: !!dqh?._origDispatch,
            broadPatchCount: Array.isArray(dqh?._broadPatched) ? dqh._broadPatched.length : 0
        });
    } catch (e) {
        return JSON.stringify({ success: false, error: String(e) });
    }
})()
"#;

struct CdpJsonExecutionSummary {
    total_targets: usize,
    successful_results: Vec<serde_json::Value>,
    target_failures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QuestRouteWarmupPlan {
    original_url: String,
    warmup_url: String,
    restore_url: String,
    already_on_quest_home: bool,
}

fn build_quest_route_warmup_plan(current_url: &str) -> Option<QuestRouteWarmupPlan> {
    let current = reqwest::Url::parse(current_url).ok()?;
    if !matches!(current.scheme(), "http" | "https") {
        return None;
    }

    let already_on_quest_home = current.path().eq_ignore_ascii_case("/quest-home");
    let warmup_url = if already_on_quest_home {
        current.join(QUEST_HOME_DETOUR_URL).ok()?
    } else {
        current.join(QUEST_HOME_URL).ok()?
    };

    Some(QuestRouteWarmupPlan {
        original_url: current_url.to_string(),
        warmup_url: warmup_url.to_string(),
        restore_url: current_url.to_string(),
        already_on_quest_home,
    })
}

fn js_warmup_quest_route(plan: &QuestRouteWarmupPlan) -> String {
    let warmup_url = serde_json::to_string(&plan.warmup_url).unwrap_or_else(|_| "\"\"".to_string());
    let restore_url = serde_json::to_string(&plan.restore_url).unwrap_or_else(|_| "\"\"".to_string());

    format!(r#"
(async () => {{
    try {{
        const warmupUrl = new URL({warmup_url}, window.location.href);
        const restoreUrl = new URL({restore_url}, window.location.href);
        const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
        const pathFor = url => url.pathname + url.search + url.hash;
        const currentPath = () => window.location.pathname + window.location.search + window.location.hash;

        let wpRequire = null;
        try {{
            if (typeof webpackChunkdiscord_app !== "undefined") {{
                wpRequire = webpackChunkdiscord_app.push([[Symbol()], {{}}, r => r]);
                webpackChunkdiscord_app.pop();
            }}
        }} catch (_) {{}}

        function findRouter() {{
            if (!wpRequire || !wpRequire.c) return null;

            const seen = new Set();
            const inspect = value => {{
                if (!value || (typeof value !== "object" && typeof value !== "function") || seen.has(value)) {{
                    return null;
                }}
                seen.add(value);

                if (typeof value.transitionTo === "function" && (
                    typeof value.replaceWith === "function"
                    || typeof value.navigate === "function"
                    || typeof value.back === "function"
                )) {{
                    return value;
                }}

                if (value.router && typeof value.router.transitionTo === "function") {{
                    return value.router;
                }}

                return null;
            }};

            for (const moduleRecord of Object.values(wpRequire.c)) {{
                try {{
                    const exportsObj = moduleRecord?.exports;
                    if (!exportsObj) continue;

                    const direct = inspect(exportsObj);
                    if (direct) return direct;

                    for (const key of Object.keys(exportsObj)) {{
                        const candidate = inspect(exportsObj[key]);
                        if (candidate) return candidate;
                    }}
                }} catch (_) {{}}
            }}

            return null;
        }}

        async function waitForPath(expectedPath, timeoutMs) {{
            const start = Date.now();
            while (Date.now() - start < timeoutMs) {{
                if (currentPath() === expectedPath) return true;
                await sleep(50);
            }}
            return currentPath() === expectedPath;
        }}

        async function navigateWithinApp(targetUrl) {{
            const targetPath = pathFor(targetUrl);
            const failures = [];
            if (currentPath() === targetPath) {{
                return {{ success: true, method: "already-there", targetPath, failures }};
            }}

            const router = findRouter();
            if (router) {{
                if (typeof router.transitionTo === "function") {{
                    try {{
                        await Promise.resolve(router.transitionTo(targetPath));
                        if (await waitForPath(targetPath, 2500)) {{
                            return {{ success: true, method: "router.transitionTo", targetPath, failures }};
                        }}
                        failures.push("router.transitionTo:no-route-change");
                    }} catch (e) {{
                        failures.push("router.transitionTo:" + String(e));
                    }}
                }}

                if (typeof router.replaceWith === "function") {{
                    try {{
                        await Promise.resolve(router.replaceWith(targetPath));
                        if (await waitForPath(targetPath, 2500)) {{
                            return {{ success: true, method: "router.replaceWith", targetPath, failures }};
                        }}
                        failures.push("router.replaceWith:no-route-change");
                    }} catch (e) {{
                        failures.push("router.replaceWith:" + String(e));
                    }}
                }}

                if (typeof router.navigate === "function") {{
                    try {{
                        await Promise.resolve(router.navigate(targetPath));
                        if (await waitForPath(targetPath, 2500)) {{
                            return {{ success: true, method: "router.navigate", targetPath, failures }};
                        }}
                        failures.push("router.navigate:no-route-change");
                    }} catch (e) {{
                        failures.push("router.navigate:" + String(e));
                    }}
                }}
            }} else {{
                failures.push("router:not-found");
            }}

            try {{
                history.pushState(history.state, "", targetPath);
                window.dispatchEvent(new PopStateEvent("popstate", {{ state: history.state }}));
                window.dispatchEvent(new Event("locationchange"));
                document.dispatchEvent(new Event("locationchange"));
                if (await waitForPath(targetPath, 1200)) {{
                    return {{ success: true, method: "history.pushState", targetPath, failures }};
                }}
                failures.push("history.pushState:no-route-change");
            }} catch (e) {{
                failures.push("history.pushState:" + String(e));
            }}

            return {{ success: false, method: null, targetPath, failures }};
        }}

        const warmupResult = await navigateWithinApp(warmupUrl);
        if (!warmupResult.success) {{
            return JSON.stringify({{
                success: false,
                stage: "warmup",
                error: "Failed to navigate within Discord SPA",
                details: warmupResult.failures,
                currentUrl: window.location.href
            }});
        }}

        await sleep({dwell_ms});

        const restoreResult = await navigateWithinApp(restoreUrl);
        if (!restoreResult.success) {{
            return JSON.stringify({{
                success: false,
                stage: "restore",
                error: "Failed to restore original Discord SPA route",
                details: restoreResult.failures,
                warmupMethod: warmupResult.method,
                currentUrl: window.location.href
            }});
        }}

        await sleep({restore_settle_ms});

        return JSON.stringify({{
            success: true,
            warmupMethod: warmupResult.method,
            restoreMethod: restoreResult.method,
            finalUrl: window.location.href,
            finalPath: currentPath(),
        }});
    }} catch (e) {{
        return JSON.stringify({{ success: false, error: String(e) }});
    }}
}})()
"#, dwell_ms = QUEST_WARMUP_DWELL_MS, restore_settle_ms = QUEST_WARMUP_RESTORE_SETTLE_MS)
}

fn cdp_result_succeeded(parsed: &serde_json::Value) -> bool {
    parsed.get("success").and_then(|value| value.as_bool()).unwrap_or(false)
}

fn summarize_target_failures(failures: &[String]) -> String {
    if failures.is_empty() {
        return "no target details".to_string();
    }

    let sample = failures.iter().take(3).cloned().collect::<Vec<_>>().join(" | ");
    if failures.len() > 3 {
        format!("{} | ... +{} more", sample, failures.len() - 3)
    } else {
        sample
    }
}

fn log_partial_target_failures(operation: &str, failures: &[String]) {
    use crate::logger::{log, LogCategory, LogLevel};

    if failures.is_empty() {
        return;
    }

    log(
        LogLevel::Warn,
        LogCategory::TokenExtraction,
        &format!(
            "CDP {} had {} target failure(s): {}",
            operation,
            failures.len(),
            summarize_target_failures(failures)
        ),
        None,
    );
}

async fn cdp_execute_json_on_all_targets(
    port: u16,
    js_code: &str,
    await_promise: bool,
    timeout_secs: u64,
    operation: &str,
) -> Result<CdpJsonExecutionSummary> {
    let results = cdp_client::execute_js_via_all_discord_targets(port, js_code, await_promise, timeout_secs)
        .await
        .with_context(|| format!("Failed to execute CDP {} across Discord targets", operation))?;

    let total_targets = results.len();
    let mut successful_results = Vec::new();
    let mut target_failures = Vec::new();

    for item in results {
        let target_prefix = format!("target='{}' url='{}'", item.target_title, item.target_url);

        if let Some(err) = item.error {
            target_failures.push(format!("{} err={}", target_prefix, err));
            continue;
        }

        let raw = item.result.unwrap_or_default();
        if raw.is_empty() {
            target_failures.push(format!("{} err=empty result", target_prefix));
            continue;
        }

        let parsed: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(parsed) => parsed,
            Err(err) => {
                target_failures.push(format!(
                    "{} parse_err={} raw={}",
                    target_prefix,
                    err,
                    raw.chars().take(200).collect::<String>()
                ));
                continue;
            }
        };

        if cdp_result_succeeded(&parsed) {
            successful_results.push(parsed);
            continue;
        }

        let error = parsed
            .get("error")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| raw.chars().take(200).collect::<String>());
        target_failures.push(format!("{} err={}", target_prefix, error));
    }

    if successful_results.is_empty() {
        anyhow::bail!(
            "CDP {} failed on all {} target(s): {}",
            operation,
            total_targets,
            summarize_target_failures(&target_failures)
        );
    }

    Ok(CdpJsonExecutionSummary {
        total_targets,
        successful_results,
        target_failures,
    })
}

async fn cdp_warmup_quest_route(port: u16) {
    use crate::logger::{log, LogCategory, LogLevel};

    let primary_target = match cdp_client::get_primary_discord_target(port).await {
        Ok(target) => target,
        Err(err) => {
            log(
                LogLevel::Warn,
                LogCategory::TokenExtraction,
                &format!("CDP quest route warmup skipped: unable to inspect primary target: {}", err),
                None,
            );
            return;
        }
    };

    let plan = match build_quest_route_warmup_plan(&primary_target.url) {
        Some(plan) => plan,
        None => {
            log(
                LogLevel::Warn,
                LogCategory::TokenExtraction,
                &format!("CDP quest route warmup skipped: unsupported target URL {}", primary_target.url),
                None,
            );
            return;
        }
    };

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "CDP quest route warmup: current_url={} warmup_url={} restore_url={} already_on_quest_home={}",
            plan.original_url,
            plan.warmup_url,
            plan.restore_url,
            plan.already_on_quest_home
        ),
        None,
    );

    let spa_warmup_js = js_warmup_quest_route(&plan);
    match cdp_client::execute_js_via_primary_discord_target(port, &spa_warmup_js, true, QUEST_WARMUP_NAV_TIMEOUT_SECS).await {
        Ok(raw) => {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
                if cdp_result_succeeded(&parsed) {
                    log(
                        LogLevel::Info,
                        LogCategory::TokenExtraction,
                        &format!(
                            "CDP quest route warmup completed via in-app navigation (warmupMethod={}, restoreMethod={}, finalUrl={})",
                            parsed.get("warmupMethod").and_then(|value| value.as_str()).unwrap_or("unknown"),
                            parsed.get("restoreMethod").and_then(|value| value.as_str()).unwrap_or("unknown"),
                            parsed.get("finalUrl").and_then(|value| value.as_str()).unwrap_or("unknown")
                        ),
                        None,
                    );
                    return;
                }

                log(
                    LogLevel::Warn,
                    LogCategory::TokenExtraction,
                    &format!(
                        "CDP quest route warmup SPA attempt failed (stage={}, error={}, details={:?}); falling back to Page.navigate",
                        parsed.get("stage").and_then(|value| value.as_str()).unwrap_or("unknown"),
                        parsed.get("error").and_then(|value| value.as_str()).unwrap_or("unknown"),
                        parsed.get("details")
                    ),
                    None,
                );
            } else {
                log(
                    LogLevel::Warn,
                    LogCategory::TokenExtraction,
                    &format!("CDP quest route warmup SPA attempt returned non-JSON result: {}", raw),
                    None,
                );
            }
        }
        Err(err) => {
            log(
                LogLevel::Warn,
                LogCategory::TokenExtraction,
                &format!("CDP quest route warmup SPA attempt failed to execute: {}; falling back to Page.navigate", err),
                None,
            );
        }
    }

    if let Err(err) = cdp_client::navigate_primary_discord_target(port, &plan.warmup_url, QUEST_WARMUP_NAV_TIMEOUT_SECS).await {
        log(
            LogLevel::Warn,
            LogCategory::TokenExtraction,
            &format!("CDP quest route warmup failed while navigating to {}: {}", plan.warmup_url, err),
            None,
        );
        return;
    }

    sleep(Duration::from_millis(QUEST_WARMUP_DWELL_MS)).await;

    if let Err(err) = cdp_client::navigate_primary_discord_target(port, &plan.restore_url, QUEST_WARMUP_NAV_TIMEOUT_SECS).await {
        log(
            LogLevel::Warn,
            LogCategory::TokenExtraction,
            &format!("CDP quest route warmup failed while restoring {}: {}", plan.restore_url, err),
            None,
        );
        return;
    }

    sleep(Duration::from_millis(QUEST_WARMUP_RESTORE_SETTLE_MS)).await;

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "CDP quest route warmup completed via {} and restored to {}",
            plan.warmup_url,
            plan.restore_url
        ),
        None,
    );
}

/// Initialize Discord webpack modules via CDP.
async fn cdp_init_modules(port: u16) -> Result<()> {
    use crate::logger::{log, LogLevel, LogCategory};

    let summary = cdp_execute_json_on_all_targets(port, JS_INIT_QUEST_MODULES, true, 60, "module initialization").await?;

    log_partial_target_failures("module initialization", &summary.target_failures);

    let cached_targets = summary
        .successful_results
        .iter()
        .filter(|parsed| parsed.get("cached").and_then(|value| value.as_bool()).unwrap_or(false))
        .count();

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!(
            "CDP modules initialized on {}/{} target(s) (cached on {})",
            summary.successful_results.len(),
            summary.total_targets,
            cached_targets
        ),
        None,
    );

    Ok(())
}

/// Cleanup spoofed stores via CDP.
async fn cdp_cleanup(port: u16) {
    use crate::logger::{log, LogLevel, LogCategory};

    // Try cleanup up to 2 times — CDP connection can be flaky
    for attempt in 1..=2 {
        let mut cleanup_success_count = 0usize;

        match cdp_client::execute_js_via_all_discord_targets(port, JS_CLEANUP_SPOOF, false, 5).await {
            Ok(results) => {
                let mut error_count = 0usize;

                for item in results {
                    if let Some(err) = item.error {
                        error_count += 1;
                        log(LogLevel::Warn, LogCategory::TokenExtraction,
                            &format!(
                                "CDP cleanup target error (attempt {}): target='{}' url='{}' err={}",
                                attempt, item.target_title, item.target_url, err
                            ),
                            None,
                        );
                        continue;
                    }

                    let raw = item.result.unwrap_or_default();
                    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
                    let target_success = parsed.get("success") == Some(&serde_json::json!(true));

                    if target_success {
                        cleanup_success_count += 1;
                        log(LogLevel::Info, LogCategory::TokenExtraction,
                            &format!(
                                "CDP cleanup target ok (attempt {}): target='{}' url='{}' result={}",
                                attempt, item.target_title, item.target_url, raw
                            ),
                            None,
                        );
                    } else {
                        error_count += 1;
                        log(LogLevel::Warn, LogCategory::TokenExtraction,
                            &format!(
                                "CDP cleanup target returned failure (attempt {}): target='{}' url='{}' result={}",
                                attempt, item.target_title, item.target_url, raw
                            ),
                            None,
                        );
                    }
                }

                if cleanup_success_count == 0 {
                    log(LogLevel::Warn, LogCategory::TokenExtraction,
                        &format!(
                            "CDP cleanup had no successful target (attempt {}, failed_targets={})",
                            attempt, error_count
                        ),
                        None,
                    );
                }
            }
            Err(e) => {
                log(LogLevel::Warn, LogCategory::TokenExtraction,
                    &format!("CDP cleanup request failed (attempt {}): {}", attempt, e), None);
            }
        }

        // Verify cleanup state across all page targets.
        match cdp_client::execute_js_via_all_discord_targets(port, JS_VERIFY_CLEANUP_STATE, false, 5).await {
            Ok(results) => {
                let mut verify_checked = 0usize;
                let mut verify_dirty = 0usize;
                let mut verify_errors = 0usize;

                for item in results {
                    if let Some(err) = item.error {
                        verify_errors += 1;
                        log(LogLevel::Warn, LogCategory::TokenExtraction,
                            &format!(
                                "CDP cleanup verify target error (attempt {}): target='{}' url='{}' err={}",
                                attempt, item.target_title, item.target_url, err
                            ),
                            None,
                        );
                        continue;
                    }

                    let raw = item.result.unwrap_or_default();
                    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
                    let verify_success = parsed.get("success") == Some(&serde_json::json!(true));

                    if !verify_success {
                        verify_dirty += 1;
                        log(LogLevel::Warn, LogCategory::TokenExtraction,
                            &format!(
                                "CDP cleanup verify parse failure (attempt {}): target='{}' url='{}' result={}",
                                attempt, item.target_title, item.target_url, raw
                            ),
                            None,
                        );
                        continue;
                    }

                    verify_checked += 1;
                    let dqh_present = parsed.get("dqhPresent").and_then(|v| v.as_bool()).unwrap_or(false);
                    let spoof_active = parsed.get("spoofActive").and_then(|v| v.as_bool()).unwrap_or(false);
                    let fake_game_present = parsed.get("fakeGamePresent").and_then(|v| v.as_bool()).unwrap_or(false);
                    let has_dispatch_hook = parsed.get("hasDispatchHook").and_then(|v| v.as_bool()).unwrap_or(false);
                    let broad_patch_count = parsed.get("broadPatchCount").and_then(|v| v.as_u64()).unwrap_or(0);

                    let target_dirty = dqh_present || spoof_active || fake_game_present || has_dispatch_hook || broad_patch_count > 0;
                    if target_dirty {
                        verify_dirty += 1;
                        log(LogLevel::Warn, LogCategory::TokenExtraction,
                            &format!(
                                "CDP cleanup verify found residual state (attempt {}): target='{}' url='{}' dqhPresent={} spoofActive={} fakeGamePresent={} hasDispatchHook={} broadPatchCount={}",
                                attempt,
                                item.target_title,
                                item.target_url,
                                dqh_present,
                                spoof_active,
                                fake_game_present,
                                has_dispatch_hook,
                                broad_patch_count
                            ),
                            None,
                        );
                    }
                }

                if verify_dirty == 0 && verify_checked > 0 && verify_errors == 0 {
                    log(LogLevel::Info, LogCategory::TokenExtraction,
                        &format!(
                            "CDP cleanup verified (attempt {}): checked_targets={}, cleanup_success_targets={}, verify_errors={}",
                            attempt, verify_checked, cleanup_success_count, verify_errors
                        ),
                        None,
                    );
                    return;
                }

                log(LogLevel::Warn, LogCategory::TokenExtraction,
                    &format!(
                        "CDP cleanup verification incomplete (attempt {}): checked_targets={}, dirty_targets={}, verify_errors={}, cleanup_success_targets={}",
                        attempt, verify_checked, verify_dirty, verify_errors, cleanup_success_count
                    ),
                    None,
                );
            }
            Err(e) => {
                log(LogLevel::Warn, LogCategory::TokenExtraction,
                    &format!("CDP cleanup verify request failed (attempt {}): {}", attempt, e), None);
            }
        }

        if attempt < 2 {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    log(LogLevel::Error, LogCategory::TokenExtraction,
        "CDP cleanup failed after all retries — spoof may still be active in Discord!", None);
}

/// Poll quest progress via CDP. Uses direct API call for fresh data.
///
/// Returns `(progress_seconds, completed)`.
async fn cdp_poll_progress(port: u16, quest_id: &str) -> Result<(f64, bool)> {
    use crate::logger::{log, LogLevel, LogCategory};

    let js = js_query_progress(quest_id);
    let summary = cdp_execute_json_on_all_targets(port, &js, true, 15, "progress query").await?;
    let mut parsed = summary
        .successful_results
        .first()
        .context("CDP progress query returned no successful target results")?;
    let mut best_progress = parsed.get("progress").and_then(|value| value.as_f64()).unwrap_or(0.0);
    let mut best_completed = parsed.get("completed").and_then(|value| value.as_bool()).unwrap_or(false);

    for candidate in summary.successful_results.iter().skip(1) {
        let candidate_progress = candidate.get("progress").and_then(|value| value.as_f64()).unwrap_or(0.0);
        let candidate_completed = candidate.get("completed").and_then(|value| value.as_bool()).unwrap_or(false);

        let is_better = (!best_completed && candidate_completed)
            || (best_completed == candidate_completed && candidate_progress > best_progress);

        if is_better {
            parsed = candidate;
            best_progress = candidate_progress;
            best_completed = candidate_completed;
        }
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

    // Log game-detection diagnostics (present for "store" source in play quests)
    if let Some(n) = parsed.get("diagRunningGames").and_then(|v| v.as_i64()) {
        if n == 0 {
            log(LogLevel::Warn, LogCategory::TokenExtraction,
                "CDP game diag: RunningGameStore.getRunningGames() returns 0 games — spoof patch may not be active", None);
        } else {
            log(LogLevel::Debug, LogCategory::TokenExtraction,
                &format!("CDP game diag: RunningGameStore returns {} game(s)", n), None);
        }
    }
    if let Some(fail_info) = parsed.get("diagHeartbeatFailure").and_then(|v| v.as_str()) {
        log(LogLevel::Warn, LogCategory::TokenExtraction,
            &format!("CDP game diag: QUESTS_SEND_HEARTBEAT_FAILURE event received: {}", fail_info), None);
    }
    let hb_count = parsed.get("diagHeartbeatCount").and_then(|v| v.as_u64()).unwrap_or(0);
    let hb_progress = parsed.get("diagHeartbeatProgress").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if hb_count > 0 || hb_progress > 0.0 {
        log(LogLevel::Debug, LogCategory::TokenExtraction,
            &format!("CDP game diag: heartbeat subscription fired {} times, lastProgress={:.0}", hb_count, hb_progress), None);
    }
    let intercept_count = parsed.get("diagInterceptCount").and_then(|v| v.as_u64()).unwrap_or(0);
    if intercept_count > 0 {
        log(LogLevel::Info, LogCategory::TokenExtraction,
            &format!("CDP game diag: dispatch interceptor caught {} native scanner events (fake game re-injected)", intercept_count), None);
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
    client: Option<crate::discord_api::DiscordApiClient>,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    use crate::logger::{log, LogLevel, LogCategory};

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!("CDP play quest: quest_id={}, app_id={}, app_name={}", quest_id, app_id, app_name), None);

    // Defensive pre-cleanup: prevent stale spoof state from a previous run from leaking
    // into the new quest session.
    cdp_cleanup(port).await;
    cdp_warmup_quest_route(port).await;

    // 1. Init modules
    cdp_init_modules(port).await
        .context("Failed to initialize CDP modules for play quest")?;

    // 2. Spoof running game
    let js = js_spoof_play_game(&app_id, &app_name);
    let spoof_summary = match cdp_execute_json_on_all_targets(port, &js, true, 15, "play quest spoof").await {
        Ok(summary) => summary,
        Err(err) => {
            cdp_cleanup(port).await;
            return Err(err);
        }
    };

    log_partial_target_failures("play quest spoof", &spoof_summary.target_failures);

    let parsed = spoof_summary
        .successful_results
        .iter()
        .max_by_key(|value| value.get("patchCount").and_then(|patches| patches.as_u64()).unwrap_or(0))
        .context("CDP play quest spoof returned no successful target result")?;

    let patch_count = parsed.get("patchCount").and_then(|p| p.as_u64()).unwrap_or(1);
    let exe_name = parsed.get("exeName").and_then(|e| e.as_str()).unwrap_or("?");
    let all_exes = parsed.get("allExeNames")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!(
            "CDP: Game spoofed successfully on {}/{} target(s) (max {} RunningGameStore patches, exe={}, allExes=[{}], dispatch interceptor active). Polling progress...",
            spoof_summary.successful_results.len(),
            spoof_summary.total_targets,
            patch_count,
            exe_name,
            all_exes
        ),
        None,
    );

    // 3. Poll progress using Rust API client (reliable) with CDP fallback
    let poll_interval = Duration::from_secs(15);
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

        // Primary: poll via Rust API client (same as quest list refresh)
        let poll_result = if let Some(ref api_client) = client {
            match api_client.get_quest_progress(&quest_id).await {
                Ok((progress_secs, completed)) => {
                    log(LogLevel::Debug, LogCategory::TokenExtraction,
                        &format!("CDP play quest poll (API): {:.0}/{}s completed={}", progress_secs, seconds_needed, completed), None);
                    Some((progress_secs, completed))
                }
                Err(e) => {
                    log(LogLevel::Warn, LogCategory::TokenExtraction,
                        &format!("API progress poll failed, falling back to CDP: {}", e), None);
                    None
                }
            }
        } else {
            None
        };

        // Fallback: poll via CDP JS
        let (progress_secs, completed) = match poll_result {
            Some(r) => r,
            None => {
                match cdp_poll_progress(port, &quest_id).await {
                    Ok(r) => r,
                    Err(e) => {
                        log(LogLevel::Warn, LogCategory::TokenExtraction,
                            &format!("CDP progress poll also failed (will retry): {}", e), None);
                        continue;
                    }
                }
            }
        };

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
    client: Option<crate::discord_api::DiscordApiClient>,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    use crate::logger::{log, LogLevel, LogCategory};

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!("CDP stream quest: quest_id={}, app_id={}", quest_id, app_id), None);

    // Defensive pre-cleanup: ensure previous spoof state is removed before applying new patches.
    cdp_cleanup(port).await;
    cdp_warmup_quest_route(port).await;

    // 1. Init modules
    cdp_init_modules(port).await
        .context("Failed to initialize CDP modules for stream quest")?;

    // 2. Spoof streaming metadata
    let js = js_spoof_stream(&app_id);
    let stream_summary = match cdp_execute_json_on_all_targets(port, &js, false, 10, "stream quest spoof").await {
        Ok(summary) => summary,
        Err(err) => {
            cdp_cleanup(port).await;
            return Err(err);
        }
    };

    log_partial_target_failures("stream quest spoof", &stream_summary.target_failures);

    // Also spoof running game (stream quests also need the game running)
    let js_game = js_spoof_play_game(&app_id, "StreamedApp");
    if let Ok(game_summary) = cdp_execute_json_on_all_targets(port, &js_game, true, 15, "stream companion game spoof").await {
        log_partial_target_failures("stream companion game spoof", &game_summary.target_failures);
    }

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!(
            "CDP: Stream spoofed successfully on {}/{} target(s). Polling progress...",
            stream_summary.successful_results.len(),
            stream_summary.total_targets
        ),
        None,
    );

    // 3. Poll progress using Rust API client (reliable) with CDP fallback
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

        // Primary: poll via Rust API client
        let poll_result = if let Some(ref api_client) = client {
            match api_client.get_quest_progress(&quest_id).await {
                Ok((progress_secs, completed)) => {
                    log(LogLevel::Debug, LogCategory::TokenExtraction,
                        &format!("CDP stream quest poll (API): {:.0}/{}s completed={}", progress_secs, seconds_needed, completed), None);
                    Some((progress_secs, completed))
                }
                Err(e) => {
                    log(LogLevel::Warn, LogCategory::TokenExtraction,
                        &format!("API progress poll failed, falling back to CDP: {}", e), None);
                    None
                }
            }
        } else {
            None
        };

        // Fallback: poll via CDP JS
        let (progress_secs, completed) = match poll_result {
            Some(r) => r,
            None => {
                match cdp_poll_progress(port, &quest_id).await {
                    Ok(r) => r,
                    Err(e) => {
                        log(LogLevel::Warn, LogCategory::TokenExtraction,
                            &format!("CDP stream progress poll also failed (will retry): {}", e), None);
                        continue;
                    }
                }
            }
        };

        let pct = if seconds_needed > 0 {
            (progress_secs / seconds_needed as f64 * 100.0).min(100.0)
        } else { 0.0 };

        let _ = app_handle.emit("quest-progress", pct);
        log(LogLevel::Debug, LogCategory::TokenExtraction,
            &format!("CDP stream quest progress: {:.1}% ({:.0}/{}s)", pct, progress_secs, seconds_needed), None);

        if completed || pct >= 100.0 {
            log(LogLevel::Info, LogCategory::TokenExtraction, "CDP stream quest completed!", None);
            cdp_cleanup(port).await;
            let _ = app_handle.emit("quest-complete", ());
            return Ok(());
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

    // Defensive pre-cleanup for cross-quest consistency.
    cdp_cleanup(port).await;
    cdp_warmup_quest_route(port).await;

    // 1. Init modules
    cdp_init_modules(port).await
        .context("Failed to initialize CDP modules for video quest")?;

    let initial_pct = if seconds_needed > 0 { (initial_progress / seconds_needed as f64) * 100.0 } else { 0.0 };
    let _ = app_handle.emit("quest-progress", initial_pct);

    // 2. Fire-and-forget: launch the async video JS loop inside Discord.
    //    The JS stores its Promise globally (prevents V8 GC) and writes progress
    //    to window.__dqh_cdp._video* fields. We poll those from Rust.
    //    This avoids CDP "Promise was collected" errors from awaitPromise=true.
    let js = js_start_video_quest(&quest_id, seconds_needed, initial_progress);

    let start_summary = cdp_execute_json_on_all_targets(port, &js, false, 15, "video quest start").await
        .context("Failed to launch video quest JS")?;

    log_partial_target_failures("video quest start", &start_summary.target_failures);

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!(
            "CDP video quest JS launched on {}/{} target(s) (fire-and-forget). Polling progress...",
            start_summary.successful_results.len(),
            start_summary.total_targets
        ),
        None,
    );

    // 3. Poll progress until the JS loop finishes (videoRunning=false) or quest completes
    let poll_interval = Duration::from_secs(5);
    let max_duration = Duration::from_secs(
        ((seconds_needed as f64 - initial_progress).max(0.0) / 7.0 * 2.0) as u64 + 300
    );
    let start_time = std::time::Instant::now();

    loop {
        tokio::select! {
            _ = sleep(poll_interval) => {},
            _ = cancel_rx.recv() => {
                log(LogLevel::Info, LogCategory::TokenExtraction, "CDP video quest cancelled", None);
                // Try to stop the JS loop
                let _ = cdp_execute_json_on_all_targets(
                    port,
                    "(() => { if (window.__dqh_cdp) { window.__dqh_cdp._videoRunning = false; } return JSON.stringify({ success: true, stopped: true }); })()",
                    false,
                    5,
                    "video quest stop signal"
                ).await;
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }

        // Timeout safety
        if start_time.elapsed() > max_duration {
            log(LogLevel::Error, LogCategory::TokenExtraction,
                &format!("CDP video quest timed out after {:?}", start_time.elapsed()), None);
            let _ = app_handle.emit("quest-error", "Video quest timed out".to_string());
            return Ok(());
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
                    log(LogLevel::Info, LogCategory::TokenExtraction, "CDP video quest completed!", None);
                    let _ = app_handle.emit("quest-progress", 100.0f64);
                    let _ = app_handle.emit("quest-complete", ());
                    return Ok(());
                }
            }
            Err(e) => {
                log(LogLevel::Warn, LogCategory::TokenExtraction,
                    &format!("CDP video progress poll failed (will retry): {}", e), None);
            }
        }

        // Check if the JS loop has finished by reading _videoResult
        match cdp_execute_json_on_all_targets(
            port,
            "(() => { const d = window.__dqh_cdp; return JSON.stringify({ success: true, running: !!d?._videoRunning, result: d?._videoResult || null }); })()",
            false,
            10,
            "video quest status"
        ).await {
            Ok(status_summary) => {
                let status = status_summary
                    .successful_results
                    .iter()
                    .find(|parsed| parsed.get("result").and_then(|value| value.as_str()).is_some())
                    .or_else(|| {
                        status_summary.successful_results.iter().find(|parsed| {
                            parsed.get("running").and_then(|value| value.as_bool()) == Some(false)
                        })
                    })
                    .or_else(|| status_summary.successful_results.first());

                if let Some(status) = status {
                    let running = status.get("running").and_then(|v| v.as_bool()).unwrap_or(true);
                    if !running {
                        if let Some(result_str) = status.get("result").and_then(|v| v.as_str()) {
                            let parsed: serde_json::Value = serde_json::from_str(result_str).unwrap_or_default();
                            if cdp_result_succeeded(&parsed) {
                                let final_secs = parsed.get("finalSeconds").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                let js_completed = parsed.get("completed").and_then(|v| v.as_bool()).unwrap_or(false);
                                let api_calls = parsed.get("apiCallCount").and_then(|v| v.as_u64()).unwrap_or(0);
                                let debug_resp = parsed.get("debugFirstResponse")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("null");
                                let store_progress = parsed.get("storeProgress").and_then(|v| v.as_f64());
                                let store_completed = parsed.get("storeCompleted").and_then(|v| v.as_bool()).unwrap_or(false);
                                log(LogLevel::Info, LogCategory::TokenExtraction,
                                    &format!("CDP video quest finished: finalSeconds={}, serverCompleted={}, apiCalls={}, storeProgress={:?}, storeCompleted={}",
                                        final_secs, js_completed, api_calls, store_progress, store_completed), None);
                                log(LogLevel::Debug, LogCategory::TokenExtraction,
                                    &format!("CDP video quest first API response: {}", debug_resp), None);

                                // Only emit quest-complete if server confirmed completion
                                if js_completed || store_completed {
                                    let _ = app_handle.emit("quest-progress", 100.0f64);
                                    let _ = app_handle.emit("quest-complete", ());
                                } else {
                                    log(LogLevel::Warn, LogCategory::TokenExtraction,
                                        &format!("CDP video quest JS succeeded but server has not confirmed completion (completed={}, storeCompleted={}). Not emitting quest-complete.", js_completed, store_completed), None);
                                    let progress_pct = store_progress.unwrap_or(0.0).min(99.0);
                                    let _ = app_handle.emit("quest-progress", progress_pct);
                                    let _ = app_handle.emit("quest-error", "Video quest finished but server has not confirmed completion. Please check quest status in Discord.".to_string());
                                }
                                return Ok(());
                            } else {
                                let error = parsed.get("error")
                                    .and_then(|e| e.as_str())
                                    .unwrap_or("Unknown video error");
                                log(LogLevel::Error, LogCategory::TokenExtraction,
                                    &format!("CDP video quest JS error: {}", error), None);

                                if parsed.get("apiModuleWrong") == Some(&serde_json::json!(true)) {
                                    log(LogLevel::Warn, LogCategory::TokenExtraction,
                                        "API module mismatch detected — invalidating CDP module cache", None);
                                    let _ = cdp_execute_json_on_all_targets(
                                        port,
                                        "(() => { delete window.__dqh_cdp; return JSON.stringify({ success: true, cleared: true }); })()",
                                        false,
                                        5,
                                        "video quest cache clear"
                                    ).await;
                                }

                                let _ = app_handle.emit("quest-error", format!("Video quest failed: {}", error));
                                return Ok(());
                            }
                        } else {
                            // JS loop stopped but no result — check error
                            log(LogLevel::Warn, LogCategory::TokenExtraction,
                                "CDP video quest JS stopped without result", None);
                            let _ = app_handle.emit("quest-error", "Video quest JS stopped unexpectedly".to_string());
                            return Ok(());
                        }
                    }
                }
            }
            Err(e) => {
                log(LogLevel::Warn, LogCategory::TokenExtraction,
                    &format!("Failed to check video JS status: {}", e), None);
            }
        }
    }
}

/// Generate JS to dispatch an event on the Activity iframe's bridge.
fn js_dispatch_message_event(event_type: &str, payload_json: &str) -> String {
    let safe_type = serde_json::to_string(event_type).unwrap_or_else(|_| "\"\"".to_string());
    let safe_payload = if payload_json.is_empty() {
        "null".to_string()
    } else {
        payload_json.to_string()
    };

    format!(
        r#"JSON.stringify((function() {{ try {{ var payload = {payload}; var evt = new MessageEvent("message", {{ data: {{ type: {type}, payload: payload }}, origin: window.location.origin }}); window.dispatchEvent(evt); return {{ success: true, dispatched: {type}, payload: payload }}; }} catch(e) {{ return {{ success: false, error: String(e) }}; }} }})())"#,
        type = safe_type,
        payload = safe_payload
    )
}

/// Generate JS to call Discord SDK commands inside the activity iframe.
fn js_init_activity_quest(quest_id: &str) -> String {
    let safe_quest_id = serde_json::to_string(quest_id).unwrap_or_else(|_| "\"\"".to_string());
    format!(r#"
(async () => {{
    try {{
        const questId = {safe_quest_id};
        const sdk = window.discordSDK;
        if (!sdk || !sdk.commands) {{
            return JSON.stringify({{ success: false, error: "Discord SDK not found in iframe" }});
        }}

        try {{
            await sdk.commands.setActivity({{
                activity: {{
                    state: "Playing",
                    details: "Completing Quest"
                }}
            }});
        }} catch(e) {{
            console.warn("[DQH] setActivity failed:", e);
        }}

        try {{
            await sdk.commands.questStartTimer({{ quest_id: questId }});
        }} catch(e) {{
            return JSON.stringify({{ success: false, error: "questStartTimer failed: " + String(e) }});
        }}

        let questInfo = null;
        try {{
            questInfo = await sdk.commands.getQuest();
        }} catch(e) {{
            return JSON.stringify({{ success: false, error: "getQuest failed: " + String(e) }});
        }}

        return JSON.stringify({{
            success: true,
            questId: questId,
            questInfo: questInfo
        }});
    }} catch (e) {{
        return JSON.stringify({{ success: false, error: String(e) }});
    }}
}})()
"#)
}

/// Generate JS to check quest completion status inside the activity iframe.
fn js_check_activity_quest_status() -> String {
    r#"
(async () => {
    try {
        const sdk = window.discordSDK;
        if (!sdk || !sdk.commands) {
            return JSON.stringify({ success: false, error: "Discord SDK not found" });
        }

        const quest = await sdk.commands.getQuest();
        if (!quest) {
            return JSON.stringify({ success: false, error: "No quest data" });
        }

        return JSON.stringify({
            success: true,
            questId: quest.quest_id,
            enrolledAt: quest.enrolled_at,
            completedAt: quest.completed_at,
            completed: !!quest.completed_at
        });
    } catch (e) {
        return JSON.stringify({ success: false, error: String(e) });
    }
})()
"#.to_string()
}

/// Generate JS to navigate Discord's SPA to a specific path.
fn js_navigate_spa(target_path: &str) -> String {
    let safe_path = serde_json::to_string(target_path).unwrap_or_else(|_| "\"\"".to_string());
    format!(r#"
(async () => {{
    try {{
        const targetPath = {safe_path};
        const currentPath = () => window.location.pathname + window.location.search + window.location.hash;
        const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

        if (currentPath() === targetPath) {{
            return JSON.stringify({{ success: true, method: "already-there" }});
        }}

        let wpRequire = null;
        try {{
            if (typeof webpackChunkdiscord_app !== "undefined") {{
                wpRequire = webpackChunkdiscord_app.push([[Symbol()], {{}}, r => r]);
                webpackChunkdiscord_app.pop();
            }}
        }} catch (_) {{}}

        function findRouter() {{
            if (!wpRequire || !wpRequire.c) return null;
            const seen = new Set();
            const inspect = value => {{
                if (!value || (typeof value !== "object" && typeof value !== "function") || seen.has(value)) return null;
                seen.add(value);
                if (typeof value.transitionTo === "function" && (
                    typeof value.replaceWith === "function"
                    || typeof value.navigate === "function"
                    || typeof value.back === "function"
                )) return value;
                if (value.router && typeof value.router.transitionTo === "function") return value.router;
                return null;
            }};
            for (const m of Object.values(wpRequire.c)) {{
                try {{
                    const exp = m?.exports;
                    if (!exp) continue;
                    const direct = inspect(exp);
                    if (direct) return direct;
                    for (const key of Object.keys(exp)) {{
                        try {{
                            const result = inspect(exp[key]);
                            if (result) return result;
                        }} catch(e) {{}}
                    }}
                }} catch(e) {{}}
            }}
            return null;
        }}

        const router = findRouter();
        if (router) {{
            const methods = ["transitionTo", "replaceWith", "navigate"];
            for (const method of methods) {{
                if (typeof router[method] === "function") {{
                    try {{
                        await Promise.resolve(router[method](targetPath));
                        await sleep(500);
                        if (currentPath() === targetPath) {{
                            return JSON.stringify({{ success: true, method: "router." + method }});
                        }}
                    }} catch (e) {{}}
                }}
            }}
        }}

        try {{
            history.pushState(history.state, "", targetPath);
            window.dispatchEvent(new PopStateEvent("popstate", {{ state: history.state }}));
            window.dispatchEvent(new Event("locationchange"));
            document.dispatchEvent(new Event("locationchange"));
            await sleep(500);
            return JSON.stringify({{ success: true, method: "history.pushState" }});
        }} catch (e) {{
            return JSON.stringify({{ success: false, error: "All navigation methods failed", details: String(e) }});
        }}
    }} catch (e) {{
        return JSON.stringify({{ success: false, error: String(e) }});
    }}
}})()
"#)
}

/// Navigate Discord's SPA to a specific path and bring Discord to the front.
pub async fn navigate_discord_spa(port: u16, target_path: &str) -> Result<()> {
    use crate::logger::{log, LogCategory, LogLevel};

    let js = js_navigate_spa(target_path);
    let summary = cdp_execute_json_on_all_targets(port, &js, true, 15, "SPA navigation").await?;

    log_partial_target_failures("SPA navigation", &summary.target_failures);

    let parsed = summary
        .successful_results
        .first()
        .context("SPA navigation returned no successful target results")?;

    let success = parsed.get("success").and_then(|s| s.as_bool()).unwrap_or(false);
    let method = parsed.get("method").and_then(|m| m.as_str()).unwrap_or("unknown");

    if success {
        log(LogLevel::Info, LogCategory::TokenExtraction,
            &format!("Discord SPA navigation successful (method={})", method), None);
        cdp_client::bring_primary_discord_target_to_front(port)
            .await
            .context("Failed to bring Discord window to front after SPA navigation")?;
        Ok(())
    } else {
        let error = parsed.get("error").and_then(|e| e.as_str()).unwrap_or("Unknown error");
        anyhow::bail!("Discord SPA navigation failed: {} (method={})", error, method)
    }
}

/// Complete an ACHIEVEMENT_IN_ACTIVITY quest via CDP.
pub async fn complete_activity_quest_via_cdp(
    port: u16,
    quest_id: String,
    checkpoint_times: Vec<u32>,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    use crate::logger::{log, LogCategory, LogLevel};

    let total_checkpoints = checkpoint_times.len();
    let total_seconds: u32 = checkpoint_times.iter().sum();

    if total_checkpoints == 0 || total_seconds == 0 {
        anyhow::bail!("Activity quest requires at least one checkpoint interval");
    }

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!("CDP activity quest: quest_id={}, checkpoints={}, total={}s, times={:?}",
            quest_id, total_checkpoints, total_seconds, checkpoint_times), None);

    let iframe_target = cdp_client::find_activity_iframe_target(port).await
        .context("Failed to find activity iframe target")?;

    let ws_url = iframe_target.web_socket_debugger_url
        .context("Activity iframe target has no WebSocket URL")?;

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!("CDP activity quest: found iframe target '{}' url='{}'",
            iframe_target.title, iframe_target.url), None);

    let init_js = js_init_activity_quest(&quest_id);
    let init_result = cdp_client::execute_js_on_target(&ws_url, &init_js, true, 15).await
        .context("Failed to initialize activity quest via CDP")?;

    log(LogLevel::Info, LogCategory::TokenExtraction,
        &format!("CDP activity quest init result: {}", init_result), None);

    let init_parsed: serde_json::Value = serde_json::from_str(&init_result).unwrap_or_default();
    if init_parsed.get("success") != Some(&serde_json::json!(true)) {
        let error = init_parsed.get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown init error");
        anyhow::bail!("Activity quest init failed: {}", error);
    }

    let _ = app_handle.emit("quest-progress", 0.0f64);

    let mut elapsed_secs = 0u32;
    for (i, checkpoint_secs) in checkpoint_times.iter().enumerate() {
        let is_last = i == total_checkpoints - 1;
        let checkpoint_num = i + 1;

        log(LogLevel::Info, LogCategory::TokenExtraction,
            &format!("CDP activity quest: waiting for checkpoint {}/{} ({}s)",
                checkpoint_num, total_checkpoints, checkpoint_secs), None);

        tokio::select! {
            _ = sleep(Duration::from_secs(*checkpoint_secs as u64)) => {},
            _ = cancel_rx.recv() => {
                log(LogLevel::Info, LogCategory::TokenExtraction, "CDP activity quest cancelled", None);
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }

        elapsed_secs += checkpoint_secs;
        let progress_pct = if is_last {
            100.0
        } else {
            ((elapsed_secs as f64) / (total_seconds as f64) * 100.0).min(99.0)
        };
        let _ = app_handle.emit("quest-progress", progress_pct);

        if is_last {
            log(LogLevel::Info, LogCategory::TokenExtraction,
                "CDP activity quest: dispatching quest-completed event", None);

            let completed_payload = serde_json::json!({
                "quest_id": quest_id.as_str(),
                "completed": true
            }).to_string();
            let completed_js = js_dispatch_message_event("quest-completed", &completed_payload);
            match cdp_client::execute_js_on_target(&ws_url, &completed_js, false, 10).await {
                Ok(result) => log(LogLevel::Info, LogCategory::TokenExtraction,
                    &format!("CDP activity quest: quest-completed dispatch result: {}", result), None),
                Err(e) => log(LogLevel::Warn, LogCategory::TokenExtraction,
                    &format!("CDP activity quest: quest-completed dispatch failed: {}", e), None),
            }
        } else {
            log(LogLevel::Info, LogCategory::TokenExtraction,
                &format!("CDP activity quest: checkpoint {}/{} reached, dispatching progress step {} (ui={:.1}%)",
                    checkpoint_num, total_checkpoints, checkpoint_num, progress_pct), None);

            let progress_payload = checkpoint_num.to_string();
            let progress_js = js_dispatch_message_event("quest-progress", &progress_payload);
            match cdp_client::execute_js_on_target(&ws_url, &progress_js, false, 10).await {
                Ok(result) => log(LogLevel::Info, LogCategory::TokenExtraction,
                    &format!("CDP activity quest: quest-progress dispatch result: {}", result), None),
                Err(e) => log(LogLevel::Warn, LogCategory::TokenExtraction,
                    &format!("CDP activity quest: quest-progress dispatch failed: {}", e), None),
            }
        }
    }

    log(LogLevel::Info, LogCategory::TokenExtraction,
        "CDP activity quest: verifying completion...", None);

    let verify_js = js_check_activity_quest_status();
    match cdp_client::execute_js_on_target(&ws_url, &verify_js, true, 15).await {
        Ok(result) => {
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
            let completed = parsed.get("completed").and_then(|c| c.as_bool()).unwrap_or(false);
            let completed_at = parsed.get("completedAt").and_then(|c| c.as_str()).unwrap_or("null");

            log(LogLevel::Info, LogCategory::TokenExtraction,
                &format!("CDP activity quest verification: completed={}, completedAt={}", completed, completed_at), None);

            if completed {
                let _ = app_handle.emit("quest-progress", 100.0f64);
                let _ = app_handle.emit("quest-complete", ());
            } else {
                let _ = app_handle.emit("quest-error",
                    "Activity quest completed but server has not confirmed. Please check quest status in Discord.".to_string());
            }
        }
        Err(e) => {
            log(LogLevel::Warn, LogCategory::TokenExtraction,
                &format!("CDP activity quest verification failed: {}", e), None);
            let _ = app_handle.emit(
                "quest-error",
                format!("Activity quest verification failed: {}", e),
            );
        }
    }

    log(LogLevel::Info, LogCategory::TokenExtraction, "CDP activity quest finished", None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_quest_route_warmup_plan_for_non_quest_page() {
        let plan = build_quest_route_warmup_plan("https://discord.com/channels/@me").unwrap();

        assert_eq!(plan.original_url, "https://discord.com/channels/@me");
        assert_eq!(plan.warmup_url, QUEST_HOME_URL);
        assert_eq!(plan.restore_url, "https://discord.com/channels/@me");
        assert!(!plan.already_on_quest_home);
    }

    #[test]
    fn test_build_quest_route_warmup_plan_for_quest_home() {
        let plan = build_quest_route_warmup_plan("https://discord.com/quest-home").unwrap();

        assert_eq!(plan.warmup_url, QUEST_HOME_DETOUR_URL);
        assert_eq!(plan.restore_url, QUEST_HOME_URL);
        assert!(plan.already_on_quest_home);
    }

    #[test]
    fn test_build_quest_route_warmup_plan_rejects_invalid_urls() {
        assert!(build_quest_route_warmup_plan("not-a-url").is_none());
        assert!(build_quest_route_warmup_plan("chrome://version").is_none());
    }
}


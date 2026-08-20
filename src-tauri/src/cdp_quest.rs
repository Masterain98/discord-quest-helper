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
use tokio::time::{sleep, sleep_until, Instant};

use crate::cdp_client;
use crate::cdp_game_spoof::{
    align_play_activity_heartbeat_secs, cdp_video_timeout_secs, cdp_video_timing,
    cleanup_verify_from_json, cleanup_verify_is_clean, path_templates, with_bridge, DetectableOs,
    SimulatedProcessHint,
};
use crate::models::PlayActivityHeartbeatStatus;
use discord_cdp_launch_core::is_discord_auxiliary_page;

const QUEST_HOME_URL: &str = "https://discord.com/quest-home";
const QUEST_HOME_DETOUR_URL: &str = "https://discord.com/store";
const QUEST_WARMUP_NAV_TIMEOUT_SECS: u64 = 20;
const QUEST_WARMUP_DWELL_MS: u64 = 1500;
const QUEST_WARMUP_RESTORE_SETTLE_MS: u64 = 800;
const CDP_CLEANUP_ATTEMPTS: u32 = 5;
const CDP_CLEANUP_CANCEL_ATTEMPTS: u32 = 1;
const CDP_CLEANUP_VERIFY_TIMEOUT_SECS: u64 = 10;

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
        const DQH_INIT_VERSION = 7;
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
            api: null,
            NativeUtils: null,
            DetectableGameStore: null
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

                        // RunningGameStore: Flux store whose getRunningGames() returns an
                        // Array. Dozens of i18n modules expose a getRunningGames function
                        // that returns {locale, ast}; those must not win.
                        if (!modules.RunningGameStore && typeof val?.getRunningGames === "function") {
                            try {
                                const games = val.getRunningGames();
                                if (Array.isArray(games)) {
                                    modules.RunningGameStore = val;
                                }
                            } catch(e) {}
                        }

                        // QuestsStore: __proto__ has getQuest
                        if (!modules.QuestsStore && val?.__proto__?.getQuest) {
                            modules.QuestsStore = val;
                        }

                        // Native utils wrapper used by RunningGameStore to register
                        // setObservedGamesCallback. Unique vs i18n decoys because it
                        // also exposes getDiscordUtils + setGameCandidateOverrides.
                        if (!modules.NativeUtils && typeof val?.getDiscordUtils === "function" && typeof val?.setObservedGamesCallback === "function" && typeof val?.setGameCandidateOverrides === "function") {
                            modules.NativeUtils = val;
                        }

                        // DetectableGameStore: getGameByExecutable distinguishes the
                        // real module from i18n getDetectableGame decoys.
                        if (!modules.DetectableGameStore && typeof val?.getDetectableGame === "function" && typeof val?.getGameByExecutable === "function" && typeof val?.findGame === "function") {
                            modules.DetectableGameStore = val;
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
            if (!mod && name !== "NativeUtils" && name !== "DetectableGameStore") missing.push(name);
        }

        if (missing.length > 0) {
            return JSON.stringify({ success: false, error: "Missing modules: " + missing.join(", ") + " (scanned " + scanned + " modules, " + apiCandidates.length + " API candidates, " + apiTestedCount + " tested)" });
        }

        Object.defineProperty(window, "__dqh_cdp", {
            value: {
                ...modules,
                initialized: true,
                _initVersion: DQH_INIT_VERSION,
                // Save original functions for cleanup
            _origGetRunningGames: modules.RunningGameStore.getRunningGames,
            _origGetGameForPID: modules.RunningGameStore.getGameForPID || null,
            _origGetVisibleRunningGames: modules.RunningGameStore.getVisibleRunningGames || null,
            _origGetVisibleGame: modules.RunningGameStore.getVisibleGame || null,
            _origGetCurrentGameForAnalytics: modules.RunningGameStore.getCurrentGameForAnalytics || null,
            _origGetGameForName: modules.RunningGameStore.getGameForName || null,
            _origGetStreamerActiveStreamMetadata: modules.ApplicationStreamingStore.getStreamerActiveStreamMetadata
            },
            writable: true,
            configurable: true,
            enumerable: false
        });

        return JSON.stringify({ success: true, cached: false, apiCandidates: apiCandidates.length, apiTested: apiTestedCount });
    } catch (e) {
        return JSON.stringify({ success: false, error: String(e) });
    }
})()
"#;

fn js_play_activity_heartbeat(
    quest_id: &str,
    application_id: Option<&str>,
    terminal: bool,
) -> String {
    let quest_id_json = serde_json::to_string(quest_id).unwrap_or_else(|_| "\"\"".to_string());
    let payload = if terminal {
        serde_json::json!({ "terminal": true })
    } else {
        serde_json::json!({
            "application_id": application_id.unwrap_or_default(),
            "terminal": false
        })
    };
    let payload_json = payload.to_string();

    format!(
        r#"
(async () => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh?.initialized || !dqh.api) {{
            return JSON.stringify({{ success: false, error: "Modules not initialized" }});
        }}

        const questId = {quest_id_json};
        const response = await Promise.race([
            dqh.api.post({{
                url: "/quests/" + questId + "/heartbeat",
                body: {payload_json}
            }}),
            new Promise((_, reject) => setTimeout(
                () => reject(new Error("PLAY_ACTIVITY heartbeat timed out")),
                15000
            ))
        ]);
        const body = response?.body;
        const progress = Number(body?.progress?.PLAY_ACTIVITY?.value);
        if (!body || !Number.isFinite(progress)) {{
            return JSON.stringify({{
                success: false,
                error: "Heartbeat response missing progress.PLAY_ACTIVITY.value"
            }});
        }}

        return JSON.stringify({{
            success: true,
            progress,
            completed: body.completed_at != null
                || body?.progress?.PLAY_ACTIVITY?.completed_at != null
        }});
    }} catch (error) {{
        return JSON.stringify({{ success: false, error: String(error) }});
    }}
}})()
"#,
    )
}

fn js_play_activity_status(quest_id: &str) -> String {
    let quest_id_json = serde_json::to_string(quest_id).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"
(async () => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh?.initialized || !dqh.api) {{
            return JSON.stringify({{ success: false, error: "Modules not initialized" }});
        }}

        const questId = {quest_id_json};
        const response = await Promise.race([
            dqh.api.get({{ url: "/quests/@me" }}),
            new Promise((_, reject) => setTimeout(
                () => reject(new Error("PLAY_ACTIVITY status request timed out")),
                15000
            ))
        ]);
        const body = response?.body;
        const quests = Array.isArray(body) ? body : body?.quests;
        if (!Array.isArray(quests)) {{
            return JSON.stringify({{ success: false, error: "Quest list response has no quests array" }});
        }}

        const quest = quests.find(item => item?.id === questId);
        if (!quest) {{
            return JSON.stringify({{ success: false, error: "Quest not found in /quests/@me" }});
        }}
        const userStatus = quest.user_status ?? quest.userStatus;
        const progress = Number(
            userStatus?.progress?.PLAY_ACTIVITY?.value ?? 0
        );
        const completedAt = userStatus?.completed_at ?? userStatus?.completedAt ?? null;

        return JSON.stringify({{
            success: true,
            progress: Number.isFinite(progress) ? progress : 0,
            completed: completedAt != null
        }});
    }} catch (error) {{
        return JSON.stringify({{ success: false, error: String(error) }});
    }}
}})()
"#,
    )
}

/// Generate JS to spoof a running game in RunningGameStore.
///
/// Always injects via the native observer callback (append, never prepend),
/// then dispatches `RUNNING_GAMES_CHANGE` so Discord's own heartbeat system
/// picks the synthetic game up. Does not wait for or reuse a native detection.
fn js_spoof_play_game(app_id: &str, app_name: &str) -> String {
    js_spoof_play_game_for(
        app_id,
        app_name,
        DetectableOs::from_host(),
        crate::game_simulator::simulated_process_hints(),
    )
}

fn js_spoof_play_game_for(
    app_id: &str,
    app_name: &str,
    host_os: DetectableOs,
    process_hints: Vec<SimulatedProcessHint>,
) -> String {
    let safe_app_id = serde_json::to_string(app_id).unwrap_or_else(|_| "\"\"".to_string());
    let safe_app_name = serde_json::to_string(app_name).unwrap_or_else(|_| "\"\"".to_string());
    let host_os_json =
        serde_json::to_string(host_os.as_api_tag()).unwrap_or_else(|_| "\"win32\"".to_string());
    let os_priority_json = serde_json::to_string(host_os.cdp_executable_os_priority())
        .unwrap_or_else(|_| "[\"win32\"]".to_string());
    let templates = path_templates(host_os);
    let path_templates_json = serde_json::json!({
        "cmdLine": templates.cmd_line,
        "exePath": templates.exe_path,
    })
    .to_string();
    let hints_json = serde_json::to_string(&process_hints).unwrap_or_else(|_| "[]".to_string());
    let unix_host = if host_os.is_unix() { "true" } else { "false" };

    format!(
        r#"
(async () => {{
    try {{
        const dqh = window.__dqh_cdp;
        if (!dqh || !dqh.initialized) return JSON.stringify({{ success: false, error: "Modules not initialized" }});

        const hostOs = {host_os_json};
        const osPriority = {os_priority_json};
        const pathTemplates = {path_templates_json};
        const processHints = {hints_json};
        const unixHost = {unix_host};

        function sanitizeApp(name) {{
            return String(name || "Game").replace(/[\/\\:*?"<>|]/g, " ").replace(/\s+/g, " ").trim();
        }}
        function normalizeExe(raw) {{
            let file = String(raw || "").replace(/^>+/, "");
            file = file.split(/[\/\\]/).pop() || file;
            if (unixHost && /\.exe$/i.test(file)) file = file.replace(/\.exe$/i, "");
            return file;
        }}
        function appSlug(app) {{
            return app.toLowerCase().split(/\s+/).filter(Boolean).join("-");
        }}
        function render(template, app, exe) {{
            return String(template)
                .split("{{app}}").join(app)
                .split("{{app_lower}}").join(app.toLowerCase())
                .split("{{app_slug}}").join(appSlug(app))
                .split("{{exe}}").join(exe);
        }}
        function sameGame(game, fake) {{
            if (!game || !fake) return false;
            if (String(game.id) === String(fake.id)) return true;
            return fake.pid != null && game.pid === fake.pid;
        }}
        function mergeFake(games, fake) {{
            const list = Array.isArray(games) ? games.slice() : [];
            if (!fake) return list;
            if (list.some(g => sameGame(g, fake))) return list;
            list.push(fake);
            return list;
        }}
        function detectableGamesPayload() {{
            const store = dqh.DetectableGameStore;
            if (!store) return [];
            let raw = store.games;
            if (typeof raw === "function") {{
                try {{ raw = store.games(); }} catch(e) {{ return []; }}
            }}
            if (raw && typeof raw.values === "function") return Array.from(raw.values());
            if (Array.isArray(raw)) return raw;
            if (raw && typeof raw === "object") return Object.values(raw);
            return [];
        }}
        function reregisterObserver() {{
            try {{
                const games = detectableGamesPayload();
                dqh._detectableGamesPayload = games;
                if (games.length > 0 && dqh.FluxDispatcher) {{
                    const pending = dqh.FluxDispatcher.dispatch({{ type: "GAMES_DATABASE_UPDATE", games }});
                    if (pending && typeof pending.then === "function") pending.catch(() => {{}});
                }}
            }} catch(e) {{}}
        }}

        const applicationId = {safe_app_id};
        const applicationName = {safe_app_name};
        let exeName = unixHost ? sanitizeApp(applicationName) : sanitizeApp(applicationName) + ".exe";
        let allExeNames = [];
        let selectedOs = null;
        let appDataDebug = null;
        try {{
            const res = await dqh.api.get({{ url: "/applications/public?application_ids=" + applicationId }});
            if (res && res.body && res.body[0]) {{
                const appData = res.body[0];
                appDataDebug = appData.name;
                const allExes = appData.executables || [];
                allExeNames = allExes.map(x => x && x.name).filter(Boolean);
                let selected = null;
                for (const os of osPriority) {{
                    selected = allExes.find(x => x && x.os === os && x.name);
                    if (selected) {{ selectedOs = os; break; }}
                }}
                if (!selected && allExes[0] && allExes[0].name) {{
                    selected = allExes[0];
                    selectedOs = allExes[0].os || null;
                }}
                if (selected && selected.name) {{
                    exeName = normalizeExe(selected.name);
                }}
            }}
        }} catch(e) {{}}

        exeName = normalizeExe(exeName);
        const hint = (processHints || []).find(h => {{
            const base = String(h.exePath || "").split(/[\/\\]/).pop() || "";
            return (h.exeName && h.exeName.toLowerCase() === exeName.toLowerCase())
                || base.toLowerCase() === exeName.toLowerCase();
        }});
        const builtCmd = render(pathTemplates.cmdLine, sanitizeApp(applicationName), exeName);
        const builtPath = render(pathTemplates.exePath, sanitizeApp(applicationName), exeName);

        let seenMatch = null;
        try {{
            const seen = typeof dqh.RunningGameStore.getGamesSeen === "function"
                ? dqh.RunningGameStore.getGamesSeen(false)
                : [];
            if (Array.isArray(seen)) {{
                seenMatch = seen.find(g => g && String(g.id) === String(applicationId)) || null;
            }}
        }} catch(e) {{}}

        const fakeGame = {{
            id: applicationId,
            name: applicationName,
            origGameName: applicationName,
            processName: applicationName,
            hidden: false,
            elevated: false,
            sandboxed: false,
            lastFocused: 0,
            start: Date.now(),
            exePath: (hint && hint.exePath) || (seenMatch && seenMatch.exePath) || builtPath,
            exeName: exeName,
            cmdLine: (hint && hint.cmdLine) || (seenMatch && seenMatch.cmdLine) || builtCmd,
            windowHandle: null,
            fullscreenType: null,
            isLauncher: false,
            distributor: seenMatch && seenMatch.distributor !== undefined ? seenMatch.distributor : undefined,
            sku: seenMatch && seenMatch.sku !== undefined ? seenMatch.sku : undefined,
            gameMetadata: undefined,
            executableFingerprint: undefined
        }};
        if (seenMatch && seenMatch.nativeProcessObserverId != null) {{
            fakeGame.nativeProcessObserverId = seenMatch.nativeProcessObserverId;
        }}
        if (hint && hint.pid) {{
            fakeGame.pid = hint.pid;
            fakeGame.pidPath = [hint.pid];
            try {{
                const utils = dqh.NativeUtils && dqh.NativeUtils.getDiscordUtils && dqh.NativeUtils.getDiscordUtils();
                if (utils && typeof utils.getWindowHandleFromPid === "function") {{
                    const handle = utils.getWindowHandleFromPid(hint.pid);
                    if (handle != null && handle !== "0") fakeGame.windowHandle = String(handle);
                }}
                if (utils && typeof utils.getWindowFullscreenTypeByPid === "function") {{
                    const fullscreen = utils.getWindowFullscreenTypeByPid(hint.pid);
                    if (fullscreen != null) fakeGame.fullscreenType = fullscreen;
                }} else if (dqh.NativeUtils && typeof dqh.NativeUtils.GetWindowFullscreenTypeByPid === "function") {{
                    const fullscreen = dqh.NativeUtils.GetWindowFullscreenTypeByPid(hint.pid);
                    if (fullscreen != null) fakeGame.fullscreenType = fullscreen;
                }}
            }} catch(e) {{}}
        }}

        let realGames = [];
        try {{ realGames = dqh._origGetRunningGames.call(dqh.RunningGameStore); }} catch(e) {{
            try {{ realGames = dqh._origGetRunningGames(); }} catch(e2) {{ realGames = []; }}
        }}
        if (!Array.isArray(realGames)) realGames = [];

        dqh._spoofActive = true;
        dqh._fakeGame = fakeGame;
        dqh._fakeApplicationId = applicationId;

        function subscribeHeartbeats() {{
            dqh._lastProgress = 0;
            dqh._completed = false;
            dqh._heartbeatCount = 0;
            dqh._lastHeartbeatRaw = null;
            if (dqh._heartbeatFn && dqh.FluxDispatcher) {{
                try {{ dqh.FluxDispatcher.unsubscribe("QUESTS_SEND_HEARTBEAT_SUCCESS", dqh._heartbeatFn); }} catch(e) {{}}
            }}
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

            dqh._lastHeartbeatFailure = null;
            if (dqh._heartbeatFailFn && dqh.FluxDispatcher) {{
                try {{ dqh.FluxDispatcher.unsubscribe("QUESTS_SEND_HEARTBEAT_FAILURE", dqh._heartbeatFailFn); }} catch(e) {{}}
            }}
            let heartbeatFailFn = data => {{
                try {{
                    dqh._lastHeartbeatFailure = JSON.stringify(data).substring(0, 500);
                }} catch(e) {{
                    dqh._lastHeartbeatFailure = "failed to serialize";
                }}
            }};
            dqh._heartbeatFailFn = heartbeatFailFn;
            dqh.FluxDispatcher.subscribe("QUESTS_SEND_HEARTBEAT_FAILURE", heartbeatFailFn);
        }}

        function patchedGetRunningGames() {{
            let games = [];
            try {{ games = dqh._origGetRunningGames.call(dqh.RunningGameStore); }} catch(e) {{
                try {{ games = dqh._origGetRunningGames(); }} catch(e2) {{ games = []; }}
            }}
            return mergeFake(games, dqh._fakeGame);
        }}
        function patchedGetGameForPID(p) {{
            return patchedGetRunningGames().find(x => x.pid === p);
        }}
        function patchedGetGameForName(name) {{
            const needle = String(name || "").toLowerCase();
            return patchedGetRunningGames().find(x => x && String(x.name || "").toLowerCase() === needle) || null;
        }}
        function patchedGetVisibleGame() {{
            if (dqh._spoofActive && dqh._fakeGame && !dqh._fakeGame.hidden) return dqh._fakeGame;
            let visible = null;
            try {{ visible = dqh._origGetVisibleGame && dqh._origGetVisibleGame.call(dqh.RunningGameStore); }} catch(e) {{}}
            if (visible) return visible;
            return patchedGetRunningGames().find(g => g && !g.hidden) || dqh._fakeGame || null;
        }}
        function patchedGetCurrentGameForAnalytics() {{
            if (dqh._spoofActive && dqh._fakeGame) return dqh._fakeGame;
            let current = null;
            try {{ current = dqh._origGetCurrentGameForAnalytics && dqh._origGetCurrentGameForAnalytics.call(dqh.RunningGameStore); }} catch(e) {{}}
            if (current) return current;
            return patchedGetRunningGames()[0] || dqh._fakeGame || null;
        }}
        function patchVisibleAccessor(store, origVisible) {{
            if (typeof origVisible !== "function") return;
            store.getVisibleRunningGames = function () {{
                let games = [];
                try {{ games = origVisible.call(store); }} catch(e) {{
                    try {{ games = origVisible(); }} catch(e2) {{ games = []; }}
                }}
                return mergeFake(games, dqh._fakeGame);
            }};
        }}
        dqh.RunningGameStore.getRunningGames = patchedGetRunningGames;
        dqh.RunningGameStore.getGameForPID = patchedGetGameForPID;
        if (typeof dqh._origGetGameForName === "function") dqh.RunningGameStore.getGameForName = patchedGetGameForName;
        if (typeof dqh._origGetVisibleGame === "function") dqh.RunningGameStore.getVisibleGame = patchedGetVisibleGame;
        if (typeof dqh._origGetCurrentGameForAnalytics === "function") dqh.RunningGameStore.getCurrentGameForAnalytics = patchedGetCurrentGameForAnalytics;
        patchVisibleAccessor(dqh.RunningGameStore, dqh._origGetVisibleRunningGames);

        let patchCount = 1;
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
                                let sample = null;
                                try {{ sample = val.getRunningGames(); }} catch(e) {{ continue; }}
                                if (!Array.isArray(sample)) continue;
                                const origFn = val.getRunningGames;
                                const origPidFn = typeof val.getGameForPID === 'function' ? val.getGameForPID : null;
                                const origVisibleFn = typeof val.getVisibleRunningGames === 'function' ? val.getVisibleRunningGames : null;
                                val.getRunningGames = patchedGetRunningGames;
                                if (origPidFn) val.getGameForPID = patchedGetGameForPID;
                                patchVisibleAccessor(val, origVisibleFn);
                                broadPatched.push({{ val, origFn, origPidFn, origVisibleFn }});
                                patchCount++;
                            }}
                        }} catch(e) {{}}
                    }}
                }} catch(e) {{}}
            }}
        }} catch(e) {{}}
        dqh._broadPatched = broadPatched;

        let wrappedObserver = false;
        if (dqh.NativeUtils && typeof dqh.NativeUtils.setObservedGamesCallback === "function" && !dqh._origSetObservedGamesCallback) {{
            const origObserved = dqh.NativeUtils.setObservedGamesCallback.bind(dqh.NativeUtils);
            dqh._origSetObservedGamesCallback = origObserved;
            dqh.NativeUtils.setObservedGamesCallback = function(config, flag, cb, userId) {{
                const wrappedCb = function(games) {{
                    if (!dqh._spoofActive || !dqh._fakeGame) return cb(games);
                    return cb(mergeFake(games, dqh._fakeGame));
                }};
                return origObserved(config, flag, wrappedCb, userId);
            }};
            wrappedObserver = true;
            reregisterObserver();
        }}

        dqh._dispatchInterceptCount = 0;
        if (!dqh._origDispatch) {{
            const origDispatch = dqh.FluxDispatcher.dispatch.bind(dqh.FluxDispatcher);
            dqh._origDispatch = origDispatch;
            dqh.FluxDispatcher.dispatch = function(event) {{
                if (event && event.type === "RUNNING_GAMES_CHANGE" && dqh._fakeGame && dqh._spoofActive) {{
                    const before = Array.isArray(event.games) ? event.games.length : 0;
                    const already = Array.isArray(event.games) && event.games.some(g => sameGame(g, dqh._fakeGame));
                    event.games = mergeFake(event.games, dqh._fakeGame);
                    if (!already) {{
                        if (!event.added) event.added = [];
                        if (!event.added.some(g => sameGame(g, dqh._fakeGame))) {{
                            event.added.push(dqh._fakeGame);
                        }}
                    }}
                    if (event.removed) {{
                        event.removed = event.removed.filter(g => !sameGame(g, dqh._fakeGame));
                    }}
                    if (event.games.length !== before) dqh._dispatchInterceptCount++;
                }}
                return origDispatch(event);
            }};
        }}

        const mergedGames = mergeFake(realGames, fakeGame);
        subscribeHeartbeats();
        dqh.FluxDispatcher.dispatch({{ type: "RUNNING_GAMES_CHANGE", removed: [], added: [fakeGame], games: mergedGames }});

        return JSON.stringify({{ success: true, wrappedObserver, pid: fakeGame.pid || null, patchCount: patchCount, exeName: exeName, allExeNames: allExeNames, appDataName: appDataDebug, realGamesCount: realGames.length, mergedCount: mergedGames.length, hostOs: hostOs, selectedOs: selectedOs, usedHint: !!hint }});
    }} catch (e) {{
        return JSON.stringify({{ success: false, error: String(e) }});
    }}
}})()
"#
    )
}

/// Generate JS to spoof streaming metadata in ApplicationStreamingStore.
///
/// Overrides `getStreamerActiveStreamMetadata()` to return metadata indicating
/// the user is streaming the specified application.
fn js_spoof_stream(app_id: &str) -> String {
    let safe_app_id = serde_json::to_string(app_id).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"
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
"#
    )
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
    let timing = cdp_video_timing();
    format!(
        r#"
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
                const speed = {video_speed};
                const interval = {video_interval};
                const maxFuture = {video_max_future};
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
"#,
        quest_id = quest_id,
        seconds_needed = seconds_needed,
        initial_seconds = initial_seconds,
        video_speed = timing.speed,
        video_interval = timing.interval,
        video_max_future = timing.max_future
    )
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
    format!(
        r#"
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
"#,
        quest_id = quest_id
    )
}

/// JavaScript: Cleanup spoofed store functions, restoring originals.
///
/// Discovers leftover bridges from earlier app processes (`__n` + 10 hex) and
/// the historical `__dqh_cdp` name. The historical name is split so
/// `with_bridge` cannot rewrite the lookup.
const JS_CLEANUP_SPOOF: &str = r#"
(async () => {
    try {
        const historical = "__" + "dqh_cdp";
        const names = Object.getOwnPropertyNames(window).filter(k =>
            k === historical || /^__n[0-9a-f]{10}$/.test(k)
        );
        function isDqhBridge(obj) {
            return !!(obj && typeof obj === "object" && (
                typeof obj._origGetRunningGames === "function"
                || obj._spoofActive === true
                || obj._fakeGame
                || obj._origDispatch
                || obj._origSetObservedGamesCallback
                || (obj.initialized === true && obj.RunningGameStore)
            ));
        }
        function detectableGamesPayload(dqh) {
            if (Array.isArray(dqh._detectableGamesPayload) && dqh._detectableGamesPayload.length) {
                return dqh._detectableGamesPayload;
            }
            const store = dqh.DetectableGameStore;
            if (!store) return [];
            let raw = store.games;
            if (typeof raw === "function") {
                try { raw = store.games(); } catch(e) { return []; }
            }
            if (raw && typeof raw.values === "function") return Array.from(raw.values());
            if (Array.isArray(raw)) return raw;
            if (raw && typeof raw === "object") return Object.values(raw);
            return [];
        }
        async function cleanupOne(dqh, name) {
            dqh._spoofActive = false;
            let awaitedObserverRefresh = false;

            if (dqh._origDispatch && dqh.FluxDispatcher) {
                dqh.FluxDispatcher.dispatch = dqh._origDispatch;
                delete dqh._origDispatch;
            }

            if (dqh.RunningGameStore) {
                if (dqh._origGetRunningGames) {
                    dqh.RunningGameStore.getRunningGames = dqh._origGetRunningGames;
                }
                if (typeof dqh._origGetGameForPID === "function") {
                    dqh.RunningGameStore.getGameForPID = dqh._origGetGameForPID;
                } else {
                    try {
                        delete dqh.RunningGameStore.getGameForPID;
                    } catch(e) {
                        dqh.RunningGameStore.getGameForPID = undefined;
                    }
                }
                if (typeof dqh._origGetVisibleRunningGames === "function") {
                    dqh.RunningGameStore.getVisibleRunningGames = dqh._origGetVisibleRunningGames;
                }
                if (typeof dqh._origGetVisibleGame === "function") {
                    dqh.RunningGameStore.getVisibleGame = dqh._origGetVisibleGame;
                }
                if (typeof dqh._origGetCurrentGameForAnalytics === "function") {
                    dqh.RunningGameStore.getCurrentGameForAnalytics = dqh._origGetCurrentGameForAnalytics;
                }
                if (typeof dqh._origGetGameForName === "function") {
                    dqh.RunningGameStore.getGameForName = dqh._origGetGameForName;
                }
            }
            if (dqh._origGetStreamerActiveStreamMetadata && dqh.ApplicationStreamingStore) {
                dqh.ApplicationStreamingStore.getStreamerActiveStreamMetadata = dqh._origGetStreamerActiveStreamMetadata;
            }

            if (Array.isArray(dqh._broadPatched)) {
                for (const patch of dqh._broadPatched) {
                    try {
                        patch.val.getRunningGames = patch.origFn;
                        if (patch.origPidFn) patch.val.getGameForPID = patch.origPidFn;
                        if (patch.origVisibleFn) patch.val.getVisibleRunningGames = patch.origVisibleFn;
                    } catch(e) {}
                }
            }

            if (dqh._origSetObservedGamesCallback && dqh.NativeUtils) {
                dqh.NativeUtils.setObservedGamesCallback = dqh._origSetObservedGamesCallback;
                delete dqh._origSetObservedGamesCallback;
                try {
                    const games = detectableGamesPayload(dqh);
                    if (games.length > 0 && dqh.FluxDispatcher) {
                        const pending = dqh.FluxDispatcher.dispatch({ type: "GAMES_DATABASE_UPDATE", games });
                        if (pending && typeof pending.then === "function") await pending.catch(() => {});
                    }
                } catch(e) {}
                awaitedObserverRefresh = true;
            }

            if (dqh.FluxDispatcher && dqh._heartbeatFn) {
                dqh.FluxDispatcher.unsubscribe("QUESTS_SEND_HEARTBEAT_SUCCESS", dqh._heartbeatFn);
            }
            if (dqh.FluxDispatcher && dqh._heartbeatFailFn) {
                dqh.FluxDispatcher.unsubscribe("QUESTS_SEND_HEARTBEAT_FAILURE", dqh._heartbeatFailFn);
            }

            let remaining = [];
            try {
                remaining = dqh._origGetRunningGames && dqh.RunningGameStore
                    ? dqh._origGetRunningGames.call(dqh.RunningGameStore)
                    : [];
            } catch (e) {
                remaining = [];
            }
            if (!Array.isArray(remaining)) remaining = [];
            if (dqh._fakeGame) {
                remaining = remaining.filter(game => {
                    if (!game) return true;
                    if (game === dqh._fakeGame) return false;
                    const sameId = String(game.id) === String(dqh._fakeGame.id);
                    const samePid = dqh._fakeGame.pid != null && game.pid === dqh._fakeGame.pid;
                    if (samePid) return false;
                    // Same application with no pid is the injected row. A genuine
                    // native detection of that application keeps a real pid.
                    if (sameId && (game.pid == null || game.pid === undefined)) return false;
                    return true;
                });
            }
            if (dqh.FluxDispatcher && dqh._fakeGame) {
                dqh.FluxDispatcher.dispatch({ type: "RUNNING_GAMES_CHANGE", removed: [dqh._fakeGame], added: [], games: remaining });
            }

            try {
                delete window[name];
            } catch (e) {
                try { window[name] = undefined; } catch (e2) {}
            }
            return awaitedObserverRefresh;
        }

        let cleaned = 0;
        let needsObserverSettle = false;
        for (const name of names) {
            const dqh = window[name];
            if (!isDqhBridge(dqh)) continue;
            needsObserverSettle = (await cleanupOne(dqh, name)) || needsObserverSettle;
            cleaned++;
        }
        if (needsObserverSettle) {
            await new Promise(resolve => setTimeout(resolve, 3000));
        }
        return JSON.stringify({ success: true, cleaned });
    } catch (e) {
        return JSON.stringify({ success: false, error: String(e) });
    }
})()
"#;

/// JavaScript: verify whether spoof state is still present in this page target.
const JS_VERIFY_CLEANUP_STATE: &str = r#"
(() => {
    try {
        const historical = "__" + "dqh_cdp";
        const names = Object.getOwnPropertyNames(window).filter(k =>
            k === historical || /^__n[0-9a-f]{10}$/.test(k)
        );
        function isDqhBridge(obj) {
            return !!(obj && typeof obj === "object" && (
                typeof obj._origGetRunningGames === "function"
                || obj._spoofActive === true
                || obj._fakeGame
                || obj._origDispatch
                || obj._origSetObservedGamesCallback
                || (obj.initialized === true && obj.RunningGameStore)
            ));
        }
        let dqhPresent = false;
        let spoofActive = false;
        let fakeGamePresent = false;
        let hasDispatchHook = false;
        let broadPatchCount = 0;
        let observerHook = false;
        let fakeApplicationId = null;
        for (const name of names) {
            const dqh = window[name];
            if (!isDqhBridge(dqh)) continue;
            dqhPresent = true;
            spoofActive = spoofActive || !!dqh._spoofActive;
            fakeGamePresent = fakeGamePresent || !!dqh._fakeGame;
            hasDispatchHook = hasDispatchHook || !!dqh._origDispatch;
            observerHook = observerHook || !!dqh._origSetObservedGamesCallback;
            broadPatchCount += Array.isArray(dqh._broadPatched) ? dqh._broadPatched.length : 0;
            if (!fakeApplicationId) fakeApplicationId = dqh._fakeApplicationId || (dqh._fakeGame && dqh._fakeGame.id) || null;
        }
        let fakeInRunningGames = false;
        let debugGamePresent = false;
        try {
            const webpackRequire = webpackChunkdiscord_app.push([[Symbol()], {}, r => r]);
            webpackChunkdiscord_app.pop();
            let store = null;
            let nativeUtils = null;
            for (const mod of Object.values(webpackRequire.c || {})) {
                try {
                    const exp = mod && mod.exports;
                    if (!exp) continue;
                    for (const key of Object.keys(exp)) {
                        try {
                            const val = exp[key];
                            if (!val) continue;
                            if (!store && typeof val.getRunningGames === "function") {
                                try {
                                    const games = val.getRunningGames();
                                    if (Array.isArray(games)) store = val;
                                } catch(e) {}
                            }
                            if (!nativeUtils && typeof val.getDiscordUtils === "function" && typeof val.setObservedGamesCallback === "function" && typeof val.setGameCandidateOverrides === "function") {
                                nativeUtils = val;
                            }
                        } catch(e) {}
                    }
                } catch(e) {}
                if (store && nativeUtils) break;
            }
            if (store) {
                try {
                    const debugGame = store.getDebugRunningGame?.();
                    debugGamePresent = !!(
                        fakeApplicationId
                        && debugGame
                        && String(debugGame.id) === String(fakeApplicationId)
                    );
                    const games = store.getRunningGames() || [];
                    if (fakeApplicationId) {
                        fakeInRunningGames = games.some(g => g && String(g.id) === String(fakeApplicationId));
                    }
                } catch(e) {}
            }
            if (nativeUtils && nativeUtils.setObservedGamesCallback && nativeUtils.setObservedGamesCallback.toString && nativeUtils.setObservedGamesCallback.toString().includes("wrappedCb")) {
                observerHook = true;
            }
        } catch(e) {}
        return JSON.stringify({
            success: true,
            dqhPresent,
            spoofActive,
            fakeGamePresent,
            hasDispatchHook,
            broadPatchCount,
            observerHook,
            fakeInRunningGames,
            debugGamePresent
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
    let restore_url =
        serde_json::to_string(&plan.restore_url).unwrap_or_else(|_| "\"\"".to_string());

    format!(
        r#"
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
"#,
        dwell_ms = QUEST_WARMUP_DWELL_MS,
        restore_settle_ms = QUEST_WARMUP_RESTORE_SETTLE_MS
    )
}

fn cdp_result_succeeded(parsed: &serde_json::Value) -> bool {
    parsed
        .get("success")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn summarize_target_failures(failures: &[String]) -> String {
    if failures.is_empty() {
        return "no target details".to_string();
    }

    let sample = failures
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ");
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
    let rewritten = with_bridge(js_code);
    let results = cdp_client::execute_js_via_all_discord_targets(
        port,
        &rewritten,
        await_promise,
        timeout_secs,
    )
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
                &format!(
                    "CDP quest route warmup skipped: unable to inspect primary target: {}",
                    err
                ),
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
                &format!(
                    "CDP quest route warmup skipped: unsupported target URL {}",
                    primary_target.url
                ),
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
    match cdp_client::execute_js_via_primary_discord_target(
        port,
        &spa_warmup_js,
        true,
        QUEST_WARMUP_NAV_TIMEOUT_SECS,
    )
    .await
    {
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
                    &format!(
                        "CDP quest route warmup SPA attempt returned non-JSON result: {}",
                        raw
                    ),
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

    if let Err(err) = cdp_client::navigate_primary_discord_target(
        port,
        &plan.warmup_url,
        QUEST_WARMUP_NAV_TIMEOUT_SECS,
    )
    .await
    {
        log(
            LogLevel::Warn,
            LogCategory::TokenExtraction,
            &format!(
                "CDP quest route warmup failed while navigating to {}: {}",
                plan.warmup_url, err
            ),
            None,
        );
        return;
    }

    sleep(Duration::from_millis(QUEST_WARMUP_DWELL_MS)).await;

    if let Err(err) = cdp_client::navigate_primary_discord_target(
        port,
        &plan.restore_url,
        QUEST_WARMUP_NAV_TIMEOUT_SECS,
    )
    .await
    {
        log(
            LogLevel::Warn,
            LogCategory::TokenExtraction,
            &format!(
                "CDP quest route warmup failed while restoring {}: {}",
                plan.restore_url, err
            ),
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
            plan.warmup_url, plan.restore_url
        ),
        None,
    );
}

/// Initialize Discord webpack modules via CDP.
async fn cdp_init_modules(port: u16) -> Result<()> {
    use crate::logger::{log, LogCategory, LogLevel};

    let summary = cdp_execute_json_on_all_targets(
        port,
        JS_INIT_QUEST_MODULES,
        true,
        60,
        "module initialization",
    )
    .await?;

    log_partial_target_failures("module initialization", &summary.target_failures);

    let cached_targets = summary
        .successful_results
        .iter()
        .filter(|parsed| {
            parsed
                .get("cached")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        })
        .count();

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
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

fn parse_play_activity_cdp_status(raw: &str) -> Result<PlayActivityHeartbeatStatus> {
    let parsed: serde_json::Value =
        serde_json::from_str(raw).context("PLAY_ACTIVITY CDP result was not valid JSON")?;
    if parsed.get("success").and_then(|value| value.as_bool()) != Some(true) {
        let error = parsed
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("Unknown PLAY_ACTIVITY CDP error");
        anyhow::bail!(error.to_string());
    }

    let progress_seconds = parsed
        .get("progress")
        .and_then(|value| value.as_f64())
        .ok_or_else(|| anyhow::anyhow!("PLAY_ACTIVITY CDP result missing progress"))?;
    let completed = parsed
        .get("completed")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    Ok(PlayActivityHeartbeatStatus {
        progress_seconds,
        completed,
    })
}

async fn cdp_init_modules_on_primary(port: u16) -> Result<()> {
    let raw = cdp_client::execute_js_via_primary_discord_target(
        port,
        &with_bridge(JS_INIT_QUEST_MODULES),
        true,
        60,
    )
    .await
    .context("Failed to initialize Discord modules on the primary CDP target")?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).context("CDP module initialization returned invalid JSON")?;
    if parsed.get("success").and_then(|value| value.as_bool()) == Some(true) {
        return Ok(());
    }

    anyhow::bail!(
        "CDP module initialization failed: {}",
        parsed
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown error")
    )
}

async fn cdp_send_play_activity_heartbeat(
    port: u16,
    quest_id: &str,
    application_id: Option<&str>,
    terminal: bool,
) -> Result<PlayActivityHeartbeatStatus> {
    let js = js_play_activity_heartbeat(quest_id, application_id, terminal);
    let raw = cdp_client::execute_js_via_primary_discord_target(port, &with_bridge(&js), true, 20)
        .await
        .context("Failed to execute PLAY_ACTIVITY heartbeat on the primary CDP target")?;
    parse_play_activity_cdp_status(&raw)
}

async fn cdp_get_play_activity_status(
    port: u16,
    quest_id: &str,
) -> Result<PlayActivityHeartbeatStatus> {
    let js = js_play_activity_status(quest_id);
    let raw = cdp_client::execute_js_via_primary_discord_target(port, &with_bridge(&js), true, 20)
        .await
        .context("Failed to query PLAY_ACTIVITY status on the primary CDP target")?;
    parse_play_activity_cdp_status(&raw)
}

fn log_cdp_cleanup_failure(context: &str, err: &anyhow::Error) {
    use crate::logger::{log, LogCategory, LogLevel};
    log(
        LogLevel::Error,
        LogCategory::TokenExtraction,
        &format!(
            "CDP cleanup failed ({context}): {err}. Restart Discord if a spoofed game remains visible."
        ),
        None,
    );
}

async fn cdp_cleanup_best_effort(port: u16) {
    let _ = cdp_cleanup_with_attempts(port, CDP_CLEANUP_ATTEMPTS).await;
}

async fn cdp_cleanup_after_stop(port: u16, context: &str, cancelled: bool) {
    let attempts = if cancelled {
        CDP_CLEANUP_CANCEL_ATTEMPTS
    } else {
        CDP_CLEANUP_ATTEMPTS
    };
    if let Err(err) = cdp_cleanup_with_attempts(port, attempts).await {
        log_cdp_cleanup_failure(context, &err);
    }
}

/// Cleanup spoofed stores via CDP and verify every Discord page target is clean.
async fn cdp_cleanup_with_attempts(port: u16, max_attempts: u32) -> Result<()> {
    use crate::logger::{log, LogCategory, LogLevel};

    let max_attempts = max_attempts.max(1);
    let cleanup_js = with_bridge(JS_CLEANUP_SPOOF);
    let verify_js = with_bridge(JS_VERIFY_CLEANUP_STATE);

    for attempt in 1..=max_attempts {
        let mut cleanup_success_count = 0usize;

        match cdp_client::execute_js_via_all_discord_targets(port, &cleanup_js, true, 15).await {
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
                    log(
                        LogLevel::Warn,
                        LogCategory::TokenExtraction,
                        &format!(
                            "CDP cleanup had no successful target (attempt {}, failed_targets={})",
                            attempt, error_count
                        ),
                        None,
                    );
                }
            }
            Err(e) => {
                log(
                    LogLevel::Warn,
                    LogCategory::TokenExtraction,
                    &format!("CDP cleanup request failed (attempt {}): {}", attempt, e),
                    None,
                );
            }
        }

        match cdp_client::execute_js_via_all_discord_targets(
            port,
            &verify_js,
            false,
            CDP_CLEANUP_VERIFY_TIMEOUT_SECS,
        )
        .await
        {
            Ok(results) => {
                let mut verify_checked = 0usize;
                let mut verify_dirty = 0usize;
                let mut verify_errors = 0usize;

                for item in results {
                    if is_discord_auxiliary_page(&item.target_title, &item.target_url) {
                        log(
                            LogLevel::Debug,
                            LogCategory::TokenExtraction,
                            &format!(
                                "CDP cleanup skipping overlay/popout verify (attempt {}): target='{}' url='{}'",
                                attempt, item.target_title, item.target_url
                            ),
                            None,
                        );
                        continue;
                    }

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
                    let Some(verify) = cleanup_verify_from_json(&parsed) else {
                        verify_dirty += 1;
                        log(LogLevel::Warn, LogCategory::TokenExtraction,
                            &format!(
                                "CDP cleanup verify parse failure (attempt {}): target='{}' url='{}' result={}",
                                attempt, item.target_title, item.target_url, raw
                            ),
                            None,
                        );
                        continue;
                    };

                    verify_checked += 1;
                    if !cleanup_verify_is_clean(&verify) {
                        verify_dirty += 1;
                        log(LogLevel::Warn, LogCategory::TokenExtraction,
                            &format!(
                                "CDP cleanup verify found residual state (attempt {}): target='{}' url='{}' dqhPresent={} spoofActive={} fakeGamePresent={} hasDispatchHook={} broadPatchCount={} observerHook={} fakeInRunningGames={} debugGamePresent={}",
                                attempt,
                                item.target_title,
                                item.target_url,
                                verify.dqh_present,
                                verify.spoof_active,
                                verify.fake_game_present,
                                verify.has_dispatch_hook,
                                verify.broad_patch_count,
                                verify.observer_hook,
                                verify.fake_in_running_games,
                                verify.debug_game_present
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
                    return Ok(());
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
                log(
                    LogLevel::Warn,
                    LogCategory::TokenExtraction,
                    &format!(
                        "CDP cleanup verify request failed (attempt {}): {}",
                        attempt, e
                    ),
                    None,
                );
            }
        }

        if attempt < max_attempts {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    anyhow::bail!("CDP cleanup failed after all retries — spoof may still be active in Discord")
}

/// Start a persistent, user-controlled running-game spoof without binding it
/// to a quest. The caller owns the lifecycle and must invoke
/// `stop_manual_game_spoof` when the user stops the simulation.
pub async fn start_manual_game_spoof(port: u16, app_id: &str, app_name: &str) -> Result<()> {
    use crate::logger::{log, LogCategory, LogLevel};

    // Always begin from a verified clean state. This also recovers a spoof
    // left behind by an earlier app process that exited unexpectedly.
    cdp_cleanup_with_attempts(port, CDP_CLEANUP_ATTEMPTS)
        .await
        .context("Failed to clear an existing CDP game simulation")?;
    cdp_warmup_quest_route(port).await;
    cdp_init_modules(port)
        .await
        .context("Failed to initialize CDP modules for manual game simulation")?;

    let js = js_spoof_play_game(app_id, app_name);
    let summary = match cdp_execute_json_on_all_targets(
        port,
        &js,
        true,
        15,
        "manual game simulation",
    )
    .await
    {
        Ok(summary) => summary,
        Err(err) => {
            // Evaluation can fail after a target has already mutated RunningGameStore.
            // Roll back immediately so the caller can refuse to record a session
            // without leaving Discord displaying an untracked simulated game.
            if let Err(cleanup_err) = cdp_cleanup_with_attempts(port, CDP_CLEANUP_ATTEMPTS).await {
                log_cdp_cleanup_failure("manual game simulation start failed", &cleanup_err);
                return Err(err.context(format!(
                    "start failed and the injected game could not be rolled back ({cleanup_err}). Restart Discord if the simulated game remains visible"
                )));
            }
            return Err(err);
        }
    };
    log_partial_target_failures("manual game simulation", &summary.target_failures);

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "CDP manual game simulation started for app_id={} on {}/{} target(s)",
            app_id,
            summary.successful_results.len(),
            summary.total_targets
        ),
        None,
    );
    Ok(())
}

/// Stop a persistent manual game spoof and verify that every main Discord page
/// target is clean before reporting success.
pub async fn stop_manual_game_spoof(port: u16) -> Result<()> {
    cdp_cleanup_with_attempts(port, CDP_CLEANUP_ATTEMPTS)
        .await
        .context("Failed to clean up the manual CDP game simulation")
}

/// Poll quest progress via CDP. Uses direct API call for fresh data.
///
/// Returns `(progress_seconds, completed)`.
async fn cdp_poll_progress(port: u16, quest_id: &str) -> Result<(f64, bool)> {
    use crate::logger::{log, LogCategory, LogLevel};

    let js = js_query_progress(quest_id);
    let summary = cdp_execute_json_on_all_targets(port, &js, true, 15, "progress query").await?;
    let mut parsed = summary
        .successful_results
        .first()
        .context("CDP progress query returned no successful target results")?;
    let mut best_progress = parsed
        .get("progress")
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0);
    let mut best_completed = parsed
        .get("completed")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    for candidate in summary.successful_results.iter().skip(1) {
        let candidate_progress = candidate
            .get("progress")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0);
        let candidate_completed = candidate
            .get("completed")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let is_better = (!best_completed && candidate_completed)
            || (best_completed == candidate_completed && candidate_progress > best_progress);

        if is_better {
            parsed = candidate;
            best_progress = candidate_progress;
            best_completed = candidate_completed;
        }
    }

    let source = parsed
        .get("source")
        .and_then(|s| s.as_str())
        .unwrap_or("unknown");

    // Log video JS errors if present
    if let Some(err) = parsed.get("error").and_then(|e| e.as_str()) {
        log(
            LogLevel::Warn,
            LogCategory::TokenExtraction,
            &format!("CDP progress source: {} (JS error: {})", source, err),
            None,
        );
    } else {
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!("CDP progress source: {}", source),
            None,
        );
    }

    // Log game-detection diagnostics (present for "store" source in play quests)
    if let Some(n) = parsed.get("diagRunningGames").and_then(|v| v.as_i64()) {
        if n == 0 {
            log(LogLevel::Warn, LogCategory::TokenExtraction,
                "CDP game diag: RunningGameStore.getRunningGames() returns 0 games — spoof patch may not be active", None);
        } else {
            log(
                LogLevel::Debug,
                LogCategory::TokenExtraction,
                &format!("CDP game diag: RunningGameStore returns {} game(s)", n),
                None,
            );
        }
    }
    if let Some(fail_info) = parsed.get("diagHeartbeatFailure").and_then(|v| v.as_str()) {
        log(
            LogLevel::Warn,
            LogCategory::TokenExtraction,
            &format!(
                "CDP game diag: QUESTS_SEND_HEARTBEAT_FAILURE event received: {}",
                fail_info
            ),
            None,
        );
    }
    let hb_count = parsed
        .get("diagHeartbeatCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let hb_progress = parsed
        .get("diagHeartbeatProgress")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if hb_count > 0 || hb_progress > 0.0 {
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!(
                "CDP game diag: heartbeat subscription fired {} times, lastProgress={:.0}",
                hb_count, hb_progress
            ),
            None,
        );
    }
    let intercept_count = parsed
        .get("diagInterceptCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if intercept_count > 0 {
        log(LogLevel::Info, LogCategory::TokenExtraction,
            &format!("CDP game diag: dispatch interceptor caught {} native scanner events (fake game re-injected)", intercept_count), None);
    }

    let progress = parsed
        .get("progress")
        .and_then(|p| p.as_f64())
        .unwrap_or(0.0);
    let completed = parsed
        .get("completed")
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
#[allow(clippy::too_many_arguments)]
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
    use crate::logger::{log, LogCategory, LogLevel};

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "CDP play quest: quest_id={}, app_id={}, app_name={}",
            quest_id, app_id, app_name
        ),
        None,
    );

    // Defensive pre-cleanup: prevent stale spoof state from a previous run from leaking
    // into the new quest session.
    cdp_cleanup_best_effort(port).await;
    cdp_warmup_quest_route(port).await;

    // 1. Init modules
    cdp_init_modules(port)
        .await
        .context("Failed to initialize CDP modules for play quest")?;

    // 2. Spoof running game
    let js = js_spoof_play_game(&app_id, &app_name);
    let spoof_summary =
        match cdp_execute_json_on_all_targets(port, &js, true, 15, "play quest spoof").await {
            Ok(summary) => summary,
            Err(err) => {
                cdp_cleanup_after_stop(port, "play quest spoof failed", false).await;
                return Err(err);
            }
        };

    log_partial_target_failures("play quest spoof", &spoof_summary.target_failures);

    let parsed = spoof_summary
        .successful_results
        .iter()
        .max_by_key(|value| {
            value
                .get("patchCount")
                .and_then(|patches| patches.as_u64())
                .unwrap_or(0)
        })
        .context("CDP play quest spoof returned no successful target result")?;

    let patch_count = parsed
        .get("patchCount")
        .and_then(|p| p.as_u64())
        .unwrap_or(1);
    let exe_name = parsed
        .get("exeName")
        .and_then(|e| e.as_str())
        .unwrap_or("?");
    let all_exes = parsed
        .get("allExeNames")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
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
    let initial_pct = if seconds_needed > 0 {
        (initial_progress / seconds_needed as f64) * 100.0
    } else {
        0.0
    };
    let _ = app_handle.emit("quest-progress", initial_pct);

    loop {
        tokio::select! {
            _ = sleep(poll_interval) => {},
            _ = cancel_rx.recv() => {
                log(LogLevel::Info, LogCategory::TokenExtraction, "CDP play quest cancelled", None);
                cdp_cleanup_after_stop(port, "play quest cancelled", true).await;
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }

        // Primary: poll via Rust API client (same as quest list refresh)
        let poll_result = if let Some(ref api_client) = client {
            match api_client.get_quest_progress(&quest_id).await {
                Ok((progress_secs, completed)) => {
                    log(
                        LogLevel::Debug,
                        LogCategory::TokenExtraction,
                        &format!(
                            "CDP play quest poll (API): {:.0}/{}s completed={}",
                            progress_secs, seconds_needed, completed
                        ),
                        None,
                    );
                    Some((progress_secs, completed))
                }
                Err(e) => {
                    log(
                        LogLevel::Warn,
                        LogCategory::TokenExtraction,
                        &format!("API progress poll failed, falling back to CDP: {}", e),
                        None,
                    );
                    None
                }
            }
        } else {
            None
        };

        // Fallback: poll via CDP JS
        let (progress_secs, completed) = match poll_result {
            Some(r) => r,
            None => match cdp_poll_progress(port, &quest_id).await {
                Ok(r) => r,
                Err(e) => {
                    log(
                        LogLevel::Warn,
                        LogCategory::TokenExtraction,
                        &format!("CDP progress poll also failed (will retry): {}", e),
                        None,
                    );
                    continue;
                }
            },
        };

        let pct = if seconds_needed > 0 {
            (progress_secs / seconds_needed as f64 * 100.0).min(100.0)
        } else {
            0.0
        };

        let _ = app_handle.emit("quest-progress", pct);
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!(
                "CDP play quest progress: {:.1}% ({:.0}/{}s)",
                pct, progress_secs, seconds_needed
            ),
            None,
        );

        if completed || pct >= 100.0 {
            log(
                LogLevel::Info,
                LogCategory::TokenExtraction,
                "CDP play quest completed!",
                None,
            );
            cdp_cleanup_after_stop(port, "play quest completed", false).await;
            let _ = app_handle.emit("quest-complete", ());
            return Ok(());
        }
    }
}

/// Complete a STREAM_ON_DESKTOP quest via CDP.
///
/// Similar to play quest but spoofs ApplicationStreamingStore.
#[allow(clippy::too_many_arguments)]
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
    use crate::logger::{log, LogCategory, LogLevel};

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!("CDP stream quest: quest_id={}, app_id={}", quest_id, app_id),
        None,
    );

    // Defensive pre-cleanup: ensure previous spoof state is removed before applying new patches.
    cdp_cleanup_best_effort(port).await;
    cdp_warmup_quest_route(port).await;

    // 1. Init modules
    cdp_init_modules(port)
        .await
        .context("Failed to initialize CDP modules for stream quest")?;

    // 2. Spoof streaming metadata
    let js = js_spoof_stream(&app_id);
    let stream_summary =
        match cdp_execute_json_on_all_targets(port, &js, false, 10, "stream quest spoof").await {
            Ok(summary) => summary,
            Err(err) => {
                cdp_cleanup_after_stop(port, "stream quest spoof failed", false).await;
                return Err(err);
            }
        };

    log_partial_target_failures("stream quest spoof", &stream_summary.target_failures);

    // Also spoof running game (stream quests also need the game running)
    let js_game = js_spoof_play_game(&app_id, "StreamedApp");
    if let Ok(game_summary) =
        cdp_execute_json_on_all_targets(port, &js_game, true, 15, "stream companion game spoof")
            .await
    {
        log_partial_target_failures("stream companion game spoof", &game_summary.target_failures);
    }

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "CDP: Stream spoofed successfully on {}/{} target(s). Polling progress...",
            stream_summary.successful_results.len(),
            stream_summary.total_targets
        ),
        None,
    );

    // 3. Poll progress using Rust API client (reliable) with CDP fallback
    let poll_interval = Duration::from_secs(20);
    let initial_pct = if seconds_needed > 0 {
        (initial_progress / seconds_needed as f64) * 100.0
    } else {
        0.0
    };
    let _ = app_handle.emit("quest-progress", initial_pct);

    loop {
        tokio::select! {
            _ = sleep(poll_interval) => {},
            _ = cancel_rx.recv() => {
                log(LogLevel::Info, LogCategory::TokenExtraction, "CDP stream quest cancelled", None);
                cdp_cleanup_after_stop(port, "stream quest cancelled", true).await;
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }

        // Primary: poll via Rust API client
        let poll_result = if let Some(ref api_client) = client {
            match api_client.get_quest_progress(&quest_id).await {
                Ok((progress_secs, completed)) => {
                    log(
                        LogLevel::Debug,
                        LogCategory::TokenExtraction,
                        &format!(
                            "CDP stream quest poll (API): {:.0}/{}s completed={}",
                            progress_secs, seconds_needed, completed
                        ),
                        None,
                    );
                    Some((progress_secs, completed))
                }
                Err(e) => {
                    log(
                        LogLevel::Warn,
                        LogCategory::TokenExtraction,
                        &format!("API progress poll failed, falling back to CDP: {}", e),
                        None,
                    );
                    None
                }
            }
        } else {
            None
        };

        // Fallback: poll via CDP JS
        let (progress_secs, completed) = match poll_result {
            Some(r) => r,
            None => match cdp_poll_progress(port, &quest_id).await {
                Ok(r) => r,
                Err(e) => {
                    log(
                        LogLevel::Warn,
                        LogCategory::TokenExtraction,
                        &format!("CDP stream progress poll also failed (will retry): {}", e),
                        None,
                    );
                    continue;
                }
            },
        };

        let pct = if seconds_needed > 0 {
            (progress_secs / seconds_needed as f64 * 100.0).min(100.0)
        } else {
            0.0
        };

        let _ = app_handle.emit("quest-progress", pct);
        log(
            LogLevel::Debug,
            LogCategory::TokenExtraction,
            &format!(
                "CDP stream quest progress: {:.1}% ({:.0}/{}s)",
                pct, progress_secs, seconds_needed
            ),
            None,
        );

        if completed || pct >= 100.0 {
            log(
                LogLevel::Info,
                LogCategory::TokenExtraction,
                "CDP stream quest completed!",
                None,
            );
            cdp_cleanup_after_stop(port, "stream quest completed", false).await;
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
    use crate::logger::{log, LogCategory, LogLevel};

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "CDP video quest: quest_id={}, target={}s, initial={:.0}s",
            quest_id, seconds_needed, initial_progress
        ),
        None,
    );

    // Defensive pre-cleanup for cross-quest consistency.
    cdp_cleanup_best_effort(port).await;
    cdp_warmup_quest_route(port).await;

    // 1. Init modules
    cdp_init_modules(port)
        .await
        .context("Failed to initialize CDP modules for video quest")?;

    let initial_pct = if seconds_needed > 0 {
        (initial_progress / seconds_needed as f64) * 100.0
    } else {
        0.0
    };
    let _ = app_handle.emit("quest-progress", initial_pct);

    // 2. Fire-and-forget: launch the async video JS loop inside Discord.
    //    The JS stores its Promise globally (prevents V8 GC) and writes progress
    //    to window.__dqh_cdp._video* fields. We poll those from Rust.
    //    This avoids CDP "Promise was collected" errors from awaitPromise=true.
    let js = js_start_video_quest(&quest_id, seconds_needed, initial_progress);

    let start_summary = cdp_execute_json_on_all_targets(port, &js, false, 15, "video quest start")
        .await
        .context("Failed to launch video quest JS")?;

    log_partial_target_failures("video quest start", &start_summary.target_failures);

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "CDP video quest JS launched on {}/{} target(s) (fire-and-forget). Polling progress...",
            start_summary.successful_results.len(),
            start_summary.total_targets
        ),
        None,
    );

    // 3. Poll progress until the JS loop finishes (videoRunning=false) or quest completes
    let poll_interval = Duration::from_secs(5);
    let max_duration =
        Duration::from_secs(cdp_video_timeout_secs(seconds_needed, initial_progress));
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
                cdp_cleanup_after_stop(port, "video quest cancelled", true).await;
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }

        // Timeout safety
        if start_time.elapsed() > max_duration {
            log(
                LogLevel::Error,
                LogCategory::TokenExtraction,
                &format!("CDP video quest timed out after {:?}", start_time.elapsed()),
                None,
            );
            let _ = app_handle.emit("quest-error", "Video quest timed out".to_string());
            return Ok(());
        }

        // Poll progress
        match cdp_poll_progress(port, &quest_id).await {
            Ok((progress_secs, completed)) => {
                let pct = if seconds_needed > 0 {
                    (progress_secs / seconds_needed as f64 * 100.0).min(100.0)
                } else {
                    0.0
                };

                let _ = app_handle.emit("quest-progress", pct);
                log(
                    LogLevel::Debug,
                    LogCategory::TokenExtraction,
                    &format!(
                        "CDP video quest progress: {:.1}% ({:.0}/{}s)",
                        pct, progress_secs, seconds_needed
                    ),
                    None,
                );

                if completed || pct >= 100.0 {
                    log(
                        LogLevel::Info,
                        LogCategory::TokenExtraction,
                        "CDP video quest completed!",
                        None,
                    );
                    let _ = app_handle.emit("quest-progress", 100.0f64);
                    let _ = app_handle.emit("quest-complete", ());
                    cdp_cleanup_after_stop(port, "video quest completed", false).await;
                    return Ok(());
                }
            }
            Err(e) => {
                log(
                    LogLevel::Warn,
                    LogCategory::TokenExtraction,
                    &format!("CDP video progress poll failed (will retry): {}", e),
                    None,
                );
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
                                cdp_cleanup_after_stop(port, "video quest finished", false).await;
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

const JS_ACTIVITY_HELPERS: &str = r#"
    const DQH_SDK_WAIT_TIMEOUT_MS = 12000;
    const DQH_SDK_READY_TIMEOUT_MS = 5000;
    const DQH_COMMAND_TIMEOUT_MS = 5000;
    const DQH_QUEST_START_TIMER_TIMEOUT_MS = 10000;

    function withTimeout(promise, timeoutMs, label) {
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                reject(new Error(label + " timed out after " + timeoutMs + "ms"));
            }, timeoutMs);
            Promise.resolve(promise).then(
                value => {
                    clearTimeout(timer);
                    resolve(value);
                },
                error => {
                    clearTimeout(timer);
                    reject(error);
                }
            );
        });
    }

    function sanitizeScalar(value) {
        return String(value).replace(/\b\d{17,19}\b/g, "[ID]");
    }

    function describeError(error) {
        if (error && typeof error === "object") {
            const details = {};
            for (const key of ["name", "message", "code", "type", "status", "statusCode", "reason"]) {
                if (error[key] !== undefined) {
                    details[key] = typeof error[key] === "string" ? sanitizeScalar(error[key]) : error[key];
                }
            }
            const keys = Object.keys(details);
            if (keys.length > 0) {
                return JSON.stringify(details);
            }
        }
        return sanitizeScalar(error);
    }

    function commandNames(sdk) {
        try {
            return Object.keys(sdk?.commands || {})
                .filter(key => typeof sdk.commands[key] === "function")
                .sort();
        } catch (_) {
            return [];
        }
    }

    function isKnownBenignQuestStartTimerError(value) {
        return !!value
            && typeof value === "object"
            && value.code === 4002
            && String(value.message || "").includes("Quest not found");
    }
"#;

/// Generate JS to call Discord SDK commands inside the activity iframe.
fn js_init_activity_quest(quest_id: &str) -> String {
    let safe_quest_id = serde_json::to_string(quest_id).unwrap_or_else(|_| "\"\"".to_string());
    r#"
(async () => {
    const questId = __DQH_QUEST_ID__;
    const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
__DQH_ACTIVITY_HELPERS__

    async function waitForSdk() {
        const startedAt = Date.now();
        let lastState = "window.discordSDK missing";

        while (Date.now() - startedAt < DQH_SDK_WAIT_TIMEOUT_MS) {
            const sdk = window.discordSDK;
            if (sdk && sdk.commands) {
                const commands = commandNames(sdk);
                if (typeof sdk.commands.questStartTimer === "function") {
                    return { sdk, commands, waitedMs: Date.now() - startedAt };
                }
                lastState = "questStartTimer missing; commands=[" + commands.join(", ") + "]";
            } else if (sdk) {
                lastState = "window.discordSDK present but commands missing";
            }
            await sleep(250);
        }

        throw new Error("Discord SDK not ready: " + lastState);
    }

    try {
        let sdkState;
        try {
            sdkState = await waitForSdk();
        } catch (e) {
            return JSON.stringify({ success: false, error: describeError(e) });
        }

        const sdk = sdkState.sdk;
        const commands = sdkState.commands;
        const waitedMs = sdkState.waitedMs;

        if (typeof sdk.ready === "function") {
            try {
                await withTimeout(sdk.ready(), DQH_SDK_READY_TIMEOUT_MS, "discordSDK.ready");
            } catch (e) {
                return JSON.stringify({
                    success: false,
                    error: "discordSDK.ready failed: " + describeError(e),
                    commands,
                    waitedMs
                });
            }
        }

        let setActivityError = null;
        try {
            if (typeof sdk.commands.setActivity === "function") {
                await withTimeout(sdk.commands.setActivity({
                    activity: {
                        state: "Playing",
                        details: "Completing Quest"
                    }
                }), DQH_COMMAND_TIMEOUT_MS, "setActivity");
            }
        } catch(e) {
            setActivityError = describeError(e);
            console.warn("[DQH] setActivity failed:", e);
        }

        let questInfoBefore = null;
        let getQuestBeforeError = null;
        if (typeof sdk.commands.getQuest === "function") {
            try {
                questInfoBefore = await withTimeout(sdk.commands.getQuest(), DQH_COMMAND_TIMEOUT_MS, "getQuest before start");
            } catch (e) {
                getQuestBeforeError = describeError(e);
            }
        }
        const questInfoBeforeQuestId = String(questInfoBefore?.quest_id || "");
        const questInfoBeforeMatches = questInfoBeforeQuestId !== "" && questInfoBeforeQuestId === String(questId);
        const questInfoBeforeMismatched = questInfoBeforeQuestId !== "" && !questInfoBeforeMatches;
        const questInfoBeforeCompleted = !!questInfoBefore?.completed_at;

        let enrollmentStatusBefore = null;
        let enrollmentStatusBeforeError = null;
        if (typeof sdk.commands.getQuestEnrollmentStatus === "function") {
            try {
                enrollmentStatusBefore = await withTimeout(
                    sdk.commands.getQuestEnrollmentStatus({ quest_id: questId }),
                    DQH_COMMAND_TIMEOUT_MS,
                    "getQuestEnrollmentStatus before start"
                );
            } catch (e) {
                enrollmentStatusBeforeError = describeError(e);
            }
        }
        const enrollmentStatusBeforeMatches = String(enrollmentStatusBefore?.quest_id || "") === String(questId);
        const enrollmentStatusBeforeEnrolled =
            enrollmentStatusBeforeMatches && enrollmentStatusBefore?.is_enrolled === true;

        const hasCurrentQuestContext =
            questInfoBeforeMatches || enrollmentStatusBeforeEnrolled;

        let startTimerResult = null;
        let startTimerError = null;
        let startTimerIgnored = false;
        try {
            startTimerResult = await withTimeout(
                sdk.commands.questStartTimer({ quest_id: questId }),
                DQH_QUEST_START_TIMER_TIMEOUT_MS,
                "questStartTimer"
            );
        } catch(e) {
            startTimerError = describeError(e);
            if (isKnownBenignQuestStartTimerError(e) && hasCurrentQuestContext) {
                startTimerIgnored = true;
                console.warn("[DQH] questStartTimer returned Quest not found for the current iframe quest; continuing:", e);
            } else {
                return JSON.stringify({
                    success: false,
                    error: "questStartTimer failed: " + startTimerError,
                    commands,
                    waitedMs,
                    setActivityError,
                    getQuestBeforeError,
                    questInfoBeforeMatches,
                    questInfoBeforeMismatched,
                    questInfoBeforeCompleted,
                    enrollmentStatusBeforeMatches,
                    enrollmentStatusBeforeEnrolled,
                    enrollmentStatusBeforeError
                });
            }
        }

        if (startTimerResult && typeof startTimerResult === "object" && startTimerResult.success === false) {
            startTimerError = describeError(startTimerResult);
            if (isKnownBenignQuestStartTimerError(startTimerResult) && hasCurrentQuestContext) {
                startTimerIgnored = true;
                console.warn("[DQH] questStartTimer returned success=false for the current iframe quest; continuing:", startTimerResult);
            } else {
                return JSON.stringify({
                    success: false,
                    error: "questStartTimer returned failure: " + startTimerError,
                    commands,
                    waitedMs,
                    setActivityError,
                    getQuestBeforeError,
                    questInfoBeforeMatches,
                    questInfoBeforeMismatched,
                    questInfoBeforeCompleted,
                    enrollmentStatusBeforeMatches,
                    enrollmentStatusBeforeEnrolled,
                    enrollmentStatusBeforeError
                });
            }
        }

        let questInfo = null;
        let getQuestAfterError = null;
        let enrollmentStatus = null;
        let enrollmentStatusError = null;

        if (typeof sdk.commands.getQuest === "function") {
            try {
                questInfo = await withTimeout(sdk.commands.getQuest(), DQH_COMMAND_TIMEOUT_MS, "getQuest after start");
            } catch(e) {
                getQuestAfterError = describeError(e);
            }
        }
        const questInfoAfterQuestId = String(questInfo?.quest_id || "");
        const questInfoAfterMatches = questInfoAfterQuestId !== "" && questInfoAfterQuestId === String(questId);
        const questInfoAfterMismatched = questInfoAfterQuestId !== "" && !questInfoAfterMatches;
        const questInfoAfterCompleted = !!questInfo?.completed_at;

        if (typeof sdk.commands.getQuestEnrollmentStatus === "function") {
            try {
                enrollmentStatus = await withTimeout(
                    sdk.commands.getQuestEnrollmentStatus({ quest_id: questId }),
                    DQH_COMMAND_TIMEOUT_MS,
                    "getQuestEnrollmentStatus"
                );
            } catch(e) {
                enrollmentStatusError = describeError(e);
            }
        }
        const enrollmentStatusMatches = String(enrollmentStatus?.quest_id || "") === String(questId);
        const enrollmentStatusEnrolled =
            enrollmentStatusMatches && enrollmentStatus?.is_enrolled === true;

        return JSON.stringify({
            success: true,
            startTimerResultReturned: startTimerResult != null,
            questInfoBeforeMatches,
            questInfoBeforeMismatched,
            questInfoBeforeCompleted,
            questInfoAfterMatches,
            questInfoAfterMismatched,
            questInfoAfterCompleted,
            getQuestAfterError,
            enrollmentStatusMatches,
            enrollmentStatusEnrolled,
            enrollmentStatusError,
            enrollmentStatusBeforeMatches,
            enrollmentStatusBeforeEnrolled,
            enrollmentStatusBeforeError,
            commands,
            waitedMs,
            setActivityError,
            startTimerError,
            startTimerIgnored
        });
    } catch (e) {
        return JSON.stringify({ success: false, error: describeError(e) });
    }
})()
"#
    .replace("__DQH_QUEST_ID__", &safe_quest_id)
    .replace("__DQH_ACTIVITY_HELPERS__", JS_ACTIVITY_HELPERS)
}

/// Generate JS to check quest completion status inside the activity iframe.
fn js_check_activity_quest_status(quest_id: &str) -> String {
    let safe_quest_id = serde_json::to_string(quest_id).unwrap_or_else(|_| "\"\"".to_string());
    r#"
(async () => {
    const questId = __DQH_QUEST_ID__;
__DQH_ACTIVITY_HELPERS__

    try {
        const sdk = window.discordSDK;
        if (!sdk || !sdk.commands) {
            return JSON.stringify({ success: false, error: "Discord SDK not found" });
        }

        let quest = null;
        let questIdMatches = false;
        let getQuestError = null;
        if (typeof sdk.commands.getQuest === "function") {
            try {
                quest = await sdk.commands.getQuest();
                questIdMatches = String(quest?.quest_id || "") === String(questId);
                if (questIdMatches) {
                    return JSON.stringify({
                        success: true,
                        completedAt: quest.completed_at,
                        completed: !!quest.completed_at,
                        questIdMatches
                    });
                }
            } catch (e) {
                getQuestError = describeError(e);
            }
        }

        if (typeof sdk.commands.getQuestEnrollmentStatus === "function") {
            const enrollmentStatus = await sdk.commands.getQuestEnrollmentStatus({ quest_id: questId });
            const enrollmentQuestIdMatches = String(enrollmentStatus?.quest_id || "") === String(questId);
            if (!enrollmentQuestIdMatches) {
                return JSON.stringify({
                    success: false,
                    error: "Activity quest enrollment verification mismatch",
                    completed: false,
                    completedAt: null,
                    questInfoMismatched: quest !== null && !questIdMatches,
                    questIdMatches,
                    enrollmentQuestIdMatches,
                    getQuestError
                });
            }

            return JSON.stringify({
                success: true,
                completed: false,
                completedAt: null,
                cannotVerifyCompletion: true,
                questInfoMismatched: quest !== null && !questIdMatches,
                questIdMatches,
                enrollmentQuestIdMatches,
                enrolled: enrollmentStatus?.is_enrolled === true,
                getQuestError
            });
        }

        if (quest) {
            return JSON.stringify({
                success: true,
                completed: false,
                completedAt: null,
                cannotVerifyCompletion: true,
                questIdMatches,
                questInfoMismatched: !questIdMatches,
                getQuestError
            });
        }

        return JSON.stringify({
            success: false,
            error: "No SDK command available to verify activity quest completion",
            commands: Object.keys(sdk.commands || {})
        });
    } catch (e) {
        return JSON.stringify({ success: false, error: describeError(e) });
    }
})()
"#
    .replace("__DQH_QUEST_ID__", &safe_quest_id)
    .replace("__DQH_ACTIVITY_HELPERS__", JS_ACTIVITY_HELPERS)
}

/// Generate JS to navigate Discord's SPA to a specific path.
fn js_navigate_spa(target_path: &str) -> String {
    let safe_path = serde_json::to_string(target_path).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"
(async () => {{
    try {{
        const targetPath = {safe_path};
        const currentFull = () => window.location.pathname + window.location.search + window.location.hash;
        const currentBase = () => window.location.pathname + window.location.search;
        const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

        if (currentFull() === targetPath) {{
            return JSON.stringify({{ success: true, method: "already-there" }});
        }}

        // If already on the same page (same pathname+search) but different hash,
        // force a re-navigation by going away first then coming back.
        const targetBase = targetPath.split('#')[0];
        const needsReroute = currentBase() === targetBase && targetPath.includes('#');

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

        async function navigateWithRouter(router, path) {{
            const methods = ["transitionTo", "replaceWith", "navigate"];
            for (const method of methods) {{
                if (typeof router[method] === "function") {{
                    try {{
                        await Promise.resolve(router[method](path));
                        await sleep(500);
                        if (currentFull() === path) return true;
                    }} catch (e) {{}}
                }}
            }}
            return false;
        }}

        const router = findRouter();

        if (needsReroute && router) {{
            // Navigate away to force a clean re-render, then navigate to target
            await navigateWithRouter(router, "/channels/@me");
            await sleep(300);
            const ok = await navigateWithRouter(router, targetPath);
            if (ok) return JSON.stringify({{ success: true, method: "router.reroute" }});
        }}

        if (router) {{
            if (await navigateWithRouter(router, targetPath)) {{
                return JSON.stringify({{ success: true, method: "router.direct" }});
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
"#
    )
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

    let success = parsed
        .get("success")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    let method = parsed
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown");

    if success {
        log(
            LogLevel::Info,
            LogCategory::TokenExtraction,
            &format!("Discord SPA navigation successful (method={})", method),
            None,
        );
        cdp_client::bring_primary_discord_target_to_front(port)
            .await
            .context("Failed to bring Discord window to front after SPA navigation")?;
        Ok(())
    } else {
        let error = parsed
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown error");
        anyhow::bail!(
            "Discord SPA navigation failed: {} (method={})",
            error,
            method
        )
    }
}

async fn confirm_play_activity_via_cdp(
    port: u16,
    quest_id: &str,
    seconds_needed: u32,
    status: PlayActivityHeartbeatStatus,
    app_handle: &tauri::AppHandle,
    cancel_rx: &mut tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    let terminal_status = cdp_send_play_activity_heartbeat(port, quest_id, None, true)
        .await
        .ok();
    let mut confirmed = status.completed
        || terminal_status
            .map(|terminal| terminal.completed)
            .unwrap_or(false);

    for attempt in 1..=6 {
        if confirmed {
            break;
        }

        if let Ok(server_status) = cdp_get_play_activity_status(port, quest_id).await {
            let _ = app_handle.emit(
                "quest-progress",
                server_status.progress_percentage(seconds_needed),
            );
            confirmed = server_status.completed;
        }

        if !confirmed && attempt < 6 {
            tokio::select! {
                _ = sleep(Duration::from_secs(2)) => {},
                _ = cancel_rx.recv() => {
                    cdp_cleanup_after_stop(port, "PLAY_ACTIVITY confirm cancelled", true).await;
                    let _ = app_handle.emit("quest-stopped", ());
                    return Ok(());
                }
            }
        }
    }

    cdp_cleanup_after_stop(port, "PLAY_ACTIVITY confirm finished", false).await;
    if confirmed {
        let _ = app_handle.emit("quest-progress", 100.0f64);
        let _ = app_handle.emit("quest-complete", ());
        return Ok(());
    }

    anyhow::bail!("PLAY_ACTIVITY reached its target, but Discord did not confirm completion")
}

/// Complete a PLAY_ACTIVITY cloud-game quest through Discord's internal HTTP
/// module on exactly one primary CDP target.
#[allow(clippy::too_many_arguments)]
pub async fn complete_play_activity_via_cdp(
    port: u16,
    quest_id: String,
    application_id: String,
    seconds_needed: u32,
    initial_progress: f64,
    heartbeat_interval_secs: u64,
    progress_polling_interval_secs: u64,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    use crate::logger::{log, LogCategory, LogLevel};

    const RETRY_DELAY_SECS: u64 = 5;
    const MAX_CONSECUTIVE_ERRORS: u32 = 3;
    const TIMEOUT_GRACE_SECS: u64 = 300;

    if seconds_needed == 0 {
        anyhow::bail!("PLAY_ACTIVITY target must be greater than zero");
    }
    if heartbeat_interval_secs == 0 || progress_polling_interval_secs == 0 {
        anyhow::bail!("PLAY_ACTIVITY intervals must be greater than zero");
    }

    cdp_cleanup_best_effort(port).await;
    cdp_warmup_quest_route(port).await;
    if let Err(error) = cdp_init_modules_on_primary(port).await {
        cdp_cleanup_after_stop(port, "PLAY_ACTIVITY init failed", false).await;
        return Err(error.context("Failed to initialize CDP modules for PLAY_ACTIVITY"));
    }

    let remaining_seconds = (seconds_needed as f64 - initial_progress.max(0.0))
        .max(0.0)
        .ceil() as u64;
    let max_duration = Duration::from_secs(remaining_seconds.saturating_add(TIMEOUT_GRACE_SECS));
    let timeout_at = Instant::now() + max_duration;
    let heartbeat_interval_secs = align_play_activity_heartbeat_secs(heartbeat_interval_secs);
    let heartbeat_interval = Duration::from_secs(heartbeat_interval_secs);
    let progress_polling_interval = Duration::from_secs(progress_polling_interval_secs);
    let mut next_progress_poll = Instant::now() + progress_polling_interval;
    let mut session_started = false;
    let mut consecutive_errors = 0u32;

    let _ = app_handle.emit(
        "quest-progress",
        PlayActivityHeartbeatStatus {
            progress_seconds: initial_progress,
            completed: false,
        }
        .progress_percentage(seconds_needed),
    );

    loop {
        if cancel_rx.try_recv().is_ok() {
            if session_started {
                let _ = cdp_send_play_activity_heartbeat(port, &quest_id, None, true).await;
            }
            cdp_cleanup_after_stop(port, "PLAY_ACTIVITY cancelled", true).await;
            let _ = app_handle.emit("quest-stopped", ());
            return Ok(());
        }

        if Instant::now() >= timeout_at {
            if session_started {
                let _ = cdp_send_play_activity_heartbeat(port, &quest_id, None, true).await;
            }
            cdp_cleanup_after_stop(port, "PLAY_ACTIVITY timed out", false).await;
            anyhow::bail!("PLAY_ACTIVITY timed out before Discord confirmed completion");
        }

        let status = match cdp_send_play_activity_heartbeat(
            port,
            &quest_id,
            Some(&application_id),
            false,
        )
        .await
        {
            Ok(status) => {
                session_started = true;
                consecutive_errors = 0;
                status
            }
            Err(error) => {
                consecutive_errors += 1;
                log(
                    LogLevel::Warn,
                    LogCategory::Quest,
                    &format!(
                        "CDP PLAY_ACTIVITY heartbeat failed ({}/{}): {}",
                        consecutive_errors, MAX_CONSECUTIVE_ERRORS, error
                    ),
                    None,
                );

                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    if session_started {
                        let _ = cdp_send_play_activity_heartbeat(port, &quest_id, None, true).await;
                    }
                    cdp_cleanup_after_stop(port, "PLAY_ACTIVITY heartbeat failed", false).await;
                    return Err(error.context("CDP PLAY_ACTIVITY heartbeat failed three times"));
                }

                let _ = cdp_init_modules_on_primary(port).await;
                tokio::select! {
                    _ = sleep(Duration::from_secs(RETRY_DELAY_SECS)) => {},
                    _ = cancel_rx.recv() => {
                        if session_started {
                            let _ = cdp_send_play_activity_heartbeat(port, &quest_id, None, true).await;
                        }
                        cdp_cleanup_after_stop(port, "PLAY_ACTIVITY cancelled during retry", true).await;
                        let _ = app_handle.emit("quest-stopped", ());
                        return Ok(());
                    }
                }
                continue;
            }
        };

        if status.reached_target(seconds_needed) {
            return confirm_play_activity_via_cdp(
                port,
                &quest_id,
                seconds_needed,
                status,
                &app_handle,
                &mut cancel_rx,
            )
            .await;
        }

        let next_heartbeat = Instant::now() + heartbeat_interval;
        loop {
            let wake_at = next_heartbeat.min(next_progress_poll).min(timeout_at);
            tokio::select! {
                _ = sleep_until(wake_at) => {},
                _ = cancel_rx.recv() => {
                    let _ = cdp_send_play_activity_heartbeat(port, &quest_id, None, true).await;
                    cdp_cleanup_after_stop(port, "PLAY_ACTIVITY cancelled while waiting", true).await;
                    let _ = app_handle.emit("quest-stopped", ());
                    return Ok(());
                }
            }

            let now = Instant::now();
            if now >= timeout_at {
                let _ = cdp_send_play_activity_heartbeat(port, &quest_id, None, true).await;
                cdp_cleanup_after_stop(port, "PLAY_ACTIVITY timed out while waiting", false).await;
                anyhow::bail!("PLAY_ACTIVITY timed out before Discord confirmed completion");
            }

            if now >= next_progress_poll {
                if let Ok(polled_status) = cdp_get_play_activity_status(port, &quest_id).await {
                    consecutive_errors = 0;
                    let _ = app_handle.emit(
                        "quest-progress",
                        polled_status.progress_percentage(seconds_needed),
                    );
                    if polled_status.reached_target(seconds_needed) {
                        return confirm_play_activity_via_cdp(
                            port,
                            &quest_id,
                            seconds_needed,
                            polled_status,
                            &app_handle,
                            &mut cancel_rx,
                        )
                        .await;
                    }
                }
                next_progress_poll = Instant::now() + progress_polling_interval;
            }

            if Instant::now() >= next_heartbeat {
                break;
            }
        }
    }
}

/// Complete an ACHIEVEMENT_IN_ACTIVITY quest via CDP.
#[allow(clippy::too_many_arguments)]
pub async fn complete_activity_quest_via_cdp(
    port: u16,
    quest_id: String,
    application_id: String,
    initial_progress: f64,
    checkpoint_times: Vec<u32>,
    client: Option<crate::discord_api::DiscordApiClient>,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    use crate::logger::{log, sanitize_user_id, LogCategory, LogLevel};

    let remaining_checkpoints = checkpoint_times.len();
    let completed_checkpoints = initial_progress.max(0.0).floor() as usize;
    let total_checkpoints = completed_checkpoints + remaining_checkpoints;
    let total_seconds: u32 = checkpoint_times.iter().sum();
    let quest_id_hint = sanitize_user_id(&quest_id);
    let application_id_hint = if application_id.trim().is_empty() {
        "none".to_string()
    } else {
        sanitize_user_id(application_id.trim())
    };

    if remaining_checkpoints == 0 || total_checkpoints == 0 || total_seconds == 0 {
        anyhow::bail!("Activity quest requires at least one checkpoint interval");
    }

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "CDP activity quest: quest_id_hint={}, application_id_hint={}, completed_checkpoints={}, remaining_checkpoints={}, total_checkpoints={}, remaining_total={}s, times={:?}",
            quest_id_hint,
            application_id_hint,
            completed_checkpoints,
            remaining_checkpoints,
            total_checkpoints,
            total_seconds,
            checkpoint_times
        ),
        None,
    );

    let application_id_filter = if application_id.trim().is_empty() {
        None
    } else {
        Some(application_id.trim())
    };

    let iframe_target =
        cdp_client::find_activity_iframe_target_for_application(port, application_id_filter)
            .await
            .context("Failed to find activity iframe target")?;

    let ws_url = iframe_target
        .web_socket_debugger_url
        .context("Activity iframe target has no WebSocket URL")?;

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!(
            "CDP activity quest: found iframe target '{}' url='{}'",
            iframe_target.title, iframe_target.url
        ),
        None,
    );

    let init_js = js_init_activity_quest(&quest_id);
    let init_result = cdp_client::execute_js_on_target(&ws_url, &init_js, true, 15)
        .await
        .context("Failed to initialize activity quest via CDP")?;

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        &format!("CDP activity quest init result: {}", init_result),
        None,
    );

    let init_parsed: serde_json::Value = serde_json::from_str(&init_result).unwrap_or_default();
    if init_parsed.get("success") != Some(&serde_json::json!(true)) {
        let error = init_parsed
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown init error");
        anyhow::bail!("Activity quest init failed: {}", error);
    }

    if init_parsed
        .get("startTimerIgnored")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        let start_timer_error = init_parsed
            .get("startTimerError")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        log(
            LogLevel::Warn,
            LogCategory::TokenExtraction,
            &format!(
                "CDP activity quest: questStartTimer was rejected but getQuest confirmed the current iframe quest; continuing with checkpoints (error={})",
                start_timer_error
            ),
            None,
        );
    }

    let initial_pct =
        ((completed_checkpoints as f64) / (total_checkpoints as f64) * 100.0).clamp(0.0, 99.0);
    let _ = app_handle.emit("quest-progress", initial_pct);

    for (i, checkpoint_secs) in checkpoint_times.iter().enumerate() {
        let checkpoint_num = completed_checkpoints + i + 1;
        let is_last = checkpoint_num >= total_checkpoints;

        log(
            LogLevel::Info,
            LogCategory::TokenExtraction,
            &format!(
                "CDP activity quest: waiting for checkpoint {}/{} ({}s)",
                checkpoint_num, total_checkpoints, checkpoint_secs
            ),
            None,
        );

        tokio::select! {
            _ = sleep(Duration::from_secs(*checkpoint_secs as u64)) => {},
            _ = cancel_rx.recv() => {
                log(LogLevel::Info, LogCategory::TokenExtraction, "CDP activity quest cancelled", None);
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }

        let progress_pct =
            ((checkpoint_num as f64) / (total_checkpoints as f64) * 100.0).clamp(initial_pct, 99.0);
        let _ = app_handle.emit("quest-progress", progress_pct);

        if is_last {
            log(
                LogLevel::Info,
                LogCategory::TokenExtraction,
                "CDP activity quest: dispatching quest-completed event",
                None,
            );

            let completed_payload = serde_json::json!({
                "quest_id": quest_id.as_str(),
                "completed": true
            })
            .to_string();
            let completed_js = js_dispatch_message_event("quest-completed", &completed_payload);
            match cdp_client::execute_js_on_target(&ws_url, &completed_js, false, 10).await {
                Ok(result) => log(
                    LogLevel::Info,
                    LogCategory::TokenExtraction,
                    &format!(
                        "CDP activity quest: quest-completed dispatch result: {}",
                        result
                    ),
                    None,
                ),
                Err(e) => log(
                    LogLevel::Warn,
                    LogCategory::TokenExtraction,
                    &format!("CDP activity quest: quest-completed dispatch failed: {}", e),
                    None,
                ),
            }
        } else {
            log(LogLevel::Info, LogCategory::TokenExtraction,
                &format!("CDP activity quest: checkpoint {}/{} reached, dispatching progress step {} (ui={:.1}%)",
                    checkpoint_num, total_checkpoints, checkpoint_num, progress_pct), None);

            let progress_payload = checkpoint_num.to_string();
            let progress_js = js_dispatch_message_event("quest-progress", &progress_payload);
            match cdp_client::execute_js_on_target(&ws_url, &progress_js, false, 10).await {
                Ok(result) => log(
                    LogLevel::Info,
                    LogCategory::TokenExtraction,
                    &format!(
                        "CDP activity quest: quest-progress dispatch result: {}",
                        result
                    ),
                    None,
                ),
                Err(e) => log(
                    LogLevel::Warn,
                    LogCategory::TokenExtraction,
                    &format!("CDP activity quest: quest-progress dispatch failed: {}", e),
                    None,
                ),
            }
        }
    }

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        "CDP activity quest: verifying completion...",
        None,
    );

    let verify_js = js_check_activity_quest_status(&quest_id);
    let mut verified_completed = false;
    match cdp_client::execute_js_on_target(&ws_url, &verify_js, true, 15).await {
        Ok(result) => {
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
            let completed = parsed
                .get("completed")
                .and_then(|c| c.as_bool())
                .unwrap_or(false);
            let completed_at = parsed
                .get("completedAt")
                .and_then(|c| c.as_str())
                .unwrap_or("null");

            log(
                LogLevel::Info,
                LogCategory::TokenExtraction,
                &format!(
                    "CDP activity quest verification: completed={}, completedAt={}",
                    completed, completed_at
                ),
                None,
            );

            if completed {
                verified_completed = true;
            } else {
                log(
                    LogLevel::Warn,
                    LogCategory::TokenExtraction,
                    "CDP activity quest iframe verification did not confirm completion; checking server status",
                    None,
                );
            }
        }
        Err(e) => {
            log(
                LogLevel::Warn,
                LogCategory::TokenExtraction,
                &format!("CDP activity quest verification failed: {}", e),
                None,
            );
        }
    }

    if !verified_completed {
        if let Some(api_client) = client.as_ref() {
            log(
                LogLevel::Info,
                LogCategory::TokenExtraction,
                "CDP activity quest: verifying completion via Discord API...",
                None,
            );

            for attempt in 1..=6 {
                match api_client.get_quest_progress(&quest_id).await {
                    Ok((progress, completed)) => {
                        log(
                            LogLevel::Info,
                            LogCategory::TokenExtraction,
                            &format!(
                                "CDP activity quest API verification attempt {}/6: progress={}, completed={}",
                                attempt, progress, completed
                            ),
                            None,
                        );

                        if completed {
                            verified_completed = true;
                            break;
                        }
                    }
                    Err(e) => {
                        log(
                            LogLevel::Warn,
                            LogCategory::TokenExtraction,
                            &format!(
                                "CDP activity quest API verification attempt {}/6 failed: {}",
                                attempt, e
                            ),
                            None,
                        );
                    }
                }

                if attempt < 6 {
                    tokio::select! {
                        _ = sleep(Duration::from_secs(2)) => {},
                        _ = cancel_rx.recv() => {
                            log(LogLevel::Info, LogCategory::TokenExtraction, "CDP activity quest cancelled during final verification", None);
                            let _ = app_handle.emit("quest-stopped", ());
                            return Ok(());
                        }
                    }
                }
            }
        } else {
            log(
                LogLevel::Warn,
                LogCategory::TokenExtraction,
                "CDP activity quest: no Discord API client available for server-side completion verification",
                None,
            );
        }
    }

    if verified_completed {
        let _ = app_handle.emit("quest-progress", 100.0f64);
        let _ = app_handle.emit("quest-complete", ());
    } else {
        let _ = app_handle.emit(
            "quest-error",
            "Activity quest finished locally, but Discord has not confirmed completion yet. Refresh quests or check Discord.".to_string(),
        );
    }

    log(
        LogLevel::Info,
        LogCategory::TokenExtraction,
        "CDP activity quest finished",
        None,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_activity_cdp_js_matches_har_payload_shapes() {
        let running =
            js_play_activity_heartbeat("1523842681770475550", Some("1369749990758678741"), false);
        let terminal = js_play_activity_heartbeat("1523842681770475550", None, true);

        assert!(running.contains("PLAY_ACTIVITY"));
        assert!(running.contains("1369749990758678741"));
        assert!(running.contains(r#""terminal":false"#));
        assert!(running.contains(r#""application_id":"1369749990758678741""#));
        assert!(terminal.contains(r#""terminal":true"#));
        assert!(!terminal.contains("application_id"));
    }

    #[test]
    fn play_activity_cdp_status_accepts_both_quest_list_shapes() {
        let js = js_play_activity_status("1523842681770475550");

        assert!(js.contains("Array.isArray(body) ? body : body?.quests"));
        assert!(js.contains("progress?.PLAY_ACTIVITY"));
    }

    #[test]
    fn play_activity_cdp_result_requires_explicit_success_and_progress() {
        let parsed =
            parse_play_activity_cdp_status(r#"{"success":true,"progress":48,"completed":false}"#)
                .unwrap();
        assert_eq!(parsed.progress_seconds, 48.0);
        assert!(!parsed.completed);

        assert!(
            parse_play_activity_cdp_status(r#"{"success":false,"error":"missing progress"}"#)
                .is_err()
        );
        assert!(parse_play_activity_cdp_status(r#"{"success":true}"#).is_err());
    }

    #[test]
    fn play_activity_cdp_progress_waits_for_server_completion() {
        assert_eq!(
            PlayActivityHeartbeatStatus {
                progress_seconds: 900.0,
                completed: false,
            }
            .progress_percentage(900),
            99.0
        );
        assert_eq!(
            PlayActivityHeartbeatStatus {
                progress_seconds: 900.0,
                completed: true,
            }
            .progress_percentage(900),
            100.0
        );
    }

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

    #[test]
    fn init_js_requires_getrunninggames_to_return_array() {
        assert!(JS_INIT_QUEST_MODULES.contains("Array.isArray(games)"));
        assert!(JS_INIT_QUEST_MODULES.contains("const games = val.getRunningGames();"));
        assert!(!JS_INIT_QUEST_MODULES
            .contains("if (!modules.RunningGameStore && val?.getRunningGames)"));
    }

    #[test]
    fn play_spoof_js_includes_live_scanner_fields() {
        let js = js_spoof_play_game_for("123", "Cool Game", DetectableOs::Win32, Vec::new());
        for field in [
            "nativeProcessObserverId",
            "origGameName",
            "elevated",
            "sandboxed",
            "lastFocused",
            "windowHandle",
            "fullscreenType",
            "getVisibleRunningGames",
            "distributor",
            "sku",
            "gameMetadata",
            "executableFingerprint",
            "start",
            "setObservedGamesCallback",
            "GAMES_DATABASE_UPDATE",
        ] {
            assert!(js.contains(field), "missing live scanner field {field}");
        }
        assert!(js.contains("lastFocused: 0") || js.contains("lastFocused:0"));
        assert!(js.contains("start: Date.now()"));
        assert!(js.contains("seenMatch.nativeProcessObserverId"));
        assert!(js.contains("wrappedCb"));
        assert!(!js.contains("nativeProcessObserverId: pid"));
        assert!(!js.contains("Math.floor(Math.random() * 30000)"));
        assert!(js.contains("if (!Array.isArray(sample)) continue"));
        assert!(js.contains("patchVisibleAccessor"));
        assert!(js.contains("patchedGetVisibleGame"));
        assert!(js.contains("patchedGetCurrentGameForAnalytics"));
        assert!(js.contains(r#"split("{app}")"#));
        assert!(!js.contains(r#"split("{{app}}")"#));
    }

    #[test]
    fn play_spoof_js_appends_synthetic_game_instead_of_replacing() {
        let js = js_spoof_play_game_for("123", "Cool Game", DetectableOs::Win32, Vec::new());
        assert!(js.contains("if (list.some(g => sameGame(g, fake))) return list"));
        assert!(!js.contains("reuseNativeAndReturn"));
        assert!(!js.contains("notifyGameLaunched"));
        assert!(!js.contains("nativeAlreadyPresent"));
        assert!(!js.contains("RUNNING_GAME_SET_DEBUG_GAME"));
        assert!(js.contains(
            "if (dqh._spoofActive && dqh._fakeGame && !dqh._fakeGame.hidden) return dqh._fakeGame"
        ));
        assert!(js.contains("if (dqh._spoofActive && dqh._fakeGame) return dqh._fakeGame"));
        assert!(js.contains("if (visible) return visible"));
        assert!(js.contains("const already = Array.isArray(event.games)"));
        assert!(js.contains("wrappedCb"));
        let subscribe_call = js
            .rfind("subscribeHeartbeats();")
            .expect("heartbeat subscription should be invoked");
        let running_games_dispatch = js
            .find(r#"type: "RUNNING_GAMES_CHANGE", removed: [], added: [fakeGame]"#)
            .expect("spoof should dispatch RUNNING_GAMES_CHANGE");
        assert!(
            subscribe_call < running_games_dispatch,
            "heartbeat listeners must be attached before RUNNING_GAMES_CHANGE"
        );
    }

    #[test]
    fn play_spoof_js_embeds_real_process_hints() {
        let js = js_spoof_play_game_for(
            "123",
            "Cool Game",
            DetectableOs::Win32,
            vec![SimulatedProcessHint {
                pid: 4242,
                exe_name: "Cool Game.exe".into(),
                exe_path: r"C:\Games\Cool Game.exe".into(),
                cmd_line: r#""C:\Games\Cool Game.exe""#.into(),
            }],
        );
        assert!(js.contains("4242"));
        assert!(js.contains(r"C:\\Games\\Cool Game.exe") || js.contains(r"C:\Games\Cool Game.exe"));
        assert!(js.contains("fakeGame.pid = hint.pid"));
        assert!(js.contains("getWindowHandleFromPid"));
        assert!(!js.contains("nativeProcessObserverId: pid"));
    }

    #[test]
    fn overlay_and_popout_are_skipped_during_cleanup_verify() {
        assert!(is_discord_auxiliary_page(
            "Discord Overlay",
            "https://discord.com/popout"
        ));
        assert!(is_discord_auxiliary_page(
            "Friends",
            "https://discord.com/popout"
        ));
        assert!(!is_discord_auxiliary_page(
            "Friends",
            "https://discord.com/channels/@me"
        ));
        assert!(JS_VERIFY_CLEANUP_STATE.contains("debugGamePresent"));
        assert!(JS_VERIFY_CLEANUP_STATE.contains("getDebugRunningGame"));
        assert!(
            JS_VERIFY_CLEANUP_STATE.contains("String(debugGame.id) === String(fakeApplicationId)")
        );
    }

    #[test]
    fn test_activity_init_allows_getquest_mismatch_before_start() {
        let js = js_init_activity_quest("151912345678908740");

        assert!(!js.contains("Activity iframe quest mismatch"));
        assert!(js.contains("questInfoBeforeMismatched"));
        assert!(js.contains("sdk.commands.questStartTimer({ quest_id: questId })"));
    }

    #[test]
    fn test_activity_status_falls_back_to_enrollment_on_getquest_mismatch() {
        let js = js_check_activity_quest_status("151912345678908740");

        assert!(!js.contains("Activity quest verification mismatch"));
        assert!(js.contains(
            "const enrollmentStatus = await sdk.commands.getQuestEnrollmentStatus({ quest_id: questId });"
        ));
        assert_eq!(
            js.matches("questInfoMismatched: quest !== null && !questIdMatches")
                .count(),
            2
        );
    }

    #[test]
    fn play_spoof_js_on_linux_and_macos_uses_unix_paths() {
        for os in [DetectableOs::Linux, DetectableOs::Darwin] {
            let js = js_spoof_play_game_for("123", "Cool Game", os, Vec::new());
            assert!(!js.contains("Program Files"), "{os:?} leaked Windows paths");
            assert!(
                js.contains("mergeFake"),
                "{os:?} must merge real running games"
            );
            assert!(
                js.contains("removed: []"),
                "{os:?} must not drop real games"
            );
            assert!(!js.contains("const fakeGames = [fakeGame]"));
        }

        let linux = js_spoof_play_game_for("123", "Cool Game", DetectableOs::Linux, Vec::new());
        assert!(linux.contains(path_templates(DetectableOs::Linux).exe_path));
        assert!(linux.contains("\"linux\""));

        let darwin = js_spoof_play_game_for("123", "Cool Game", DetectableOs::Darwin, Vec::new());
        assert!(darwin.contains("/Applications/"));
        assert!(darwin.contains("\"darwin\""));
        assert!(darwin.contains(".app/Contents/MacOS/"));
    }

    #[test]
    fn play_spoof_js_on_windows_keeps_program_files() {
        let js = js_spoof_play_game_for("123", "Cool Game", DetectableOs::Win32, Vec::new());
        assert!(js.contains("Program Files"));
        assert!(js.contains("mergeFake"));
    }

    #[test]
    fn video_cdp_js_uses_realtime_speed() {
        let js = js_start_video_quest("qid", 100, 0.0);
        let timing = cdp_video_timing();
        assert_eq!(timing.speed, timing.interval);
        assert!(js.contains(&format!("const speed = {};", timing.speed)));
        assert!(js.contains(&format!("const interval = {};", timing.interval)));
        assert!(!js.contains("const speed = 7;"));
    }

    #[test]
    fn cleanup_js_restores_real_games_instead_of_empty_list() {
        assert!(JS_CLEANUP_SPOOF.contains("games: remaining"));
        assert!(!JS_CLEANUP_SPOOF.contains("games: []"));
        assert!(JS_CLEANUP_SPOOF.contains("remaining = remaining.filter"));
        assert!(!JS_CLEANUP_SPOOF.contains("remaining.splice"));
        assert!(JS_CLEANUP_SPOOF.contains("sameId && (game.pid == null || game.pid === undefined)"));
        assert!(JS_CLEANUP_SPOOF.contains("needsObserverSettle"));
        assert!(JS_CLEANUP_SPOOF.contains(r#"__" + "dqh_cdp""#));
        assert!(JS_CLEANUP_SPOOF.contains("^__n[0-9a-f]{10}$"));
        assert!(JS_CLEANUP_SPOOF.contains("_origSetObservedGamesCallback"));
        assert!(JS_CLEANUP_SPOOF.contains("GAMES_DATABASE_UPDATE"));
        assert!(JS_VERIFY_CLEANUP_STATE.contains("^__n[0-9a-f]{10}$"));
        assert!(JS_VERIFY_CLEANUP_STATE.contains("observerHook"));
        assert!(JS_VERIFY_CLEANUP_STATE.contains("fakeInRunningGames"));
        assert!(JS_VERIFY_CLEANUP_STATE.contains("debugGamePresent"));
        assert!(JS_VERIFY_CLEANUP_STATE
            .contains("try {\n                            const val = exp[key];"));
        assert!(JS_INIT_QUEST_MODULES.contains("NativeUtils"));
        assert!(JS_INIT_QUEST_MODULES.contains("DetectableGameStore"));
        assert!(JS_INIT_QUEST_MODULES.contains("const DQH_INIT_VERSION = 7"));
    }

    fn snapshot_game_by_id<'a>(
        snapshot: &'a crate::cdp_client::CdpRunningGamesSnapshot,
        app_id: &str,
    ) -> Option<&'a serde_json::Value> {
        snapshot.games.iter().find(|game| game_id_is(game, app_id))
    }

    fn game_id_is(game: &serde_json::Value, app_id: &str) -> bool {
        match game.get("id") {
            Some(serde_json::Value::String(value)) => value == app_id,
            Some(serde_json::Value::Number(value)) => value.to_string() == app_id,
            _ => false,
        }
    }

    fn json_id(value: Option<&serde_json::Value>) -> Option<String> {
        value.and_then(|game| match game.get("id") {
            Some(serde_json::Value::String(id)) => Some(id.clone()),
            Some(serde_json::Value::Number(id)) => Some(id.to_string()),
            _ => None,
        })
    }

    #[tokio::test]
    #[ignore = "requires a live Discord CDP session on the default debugging port"]
    async fn live_cdp_running_game_spoof_ab() {
        let port = crate::cdp_client::DEFAULT_CDP_PORT;
        let before = crate::cdp_client::fetch_running_games_via_cdp(port)
            .await
            .expect("Discord CDP snapshot should be reachable");
        assert!(before.store_found, "RunningGameStore should be loaded");

        let app_id = "1158877933042143272".to_string();
        let app_name = "Counter-Strike 2".to_string();

        let init = cdp_init_modules(port).await;
        if init.is_err() {
            let _ = cdp_cleanup_with_attempts(port, CDP_CLEANUP_ATTEMPTS).await;
        }
        init.expect("quest module init should succeed on the main renderer");

        let spoof_js = with_bridge(&js_spoof_play_game(&app_id, &app_name));
        let spoof_raw =
            crate::cdp_client::execute_js_via_primary_discord_target(port, &spoof_js, true, 30)
                .await;
        let during = crate::cdp_client::fetch_running_games_via_cdp(port).await;
        let cleanup = cdp_cleanup_with_attempts(port, CDP_CLEANUP_ATTEMPTS).await;
        let after = crate::cdp_client::fetch_running_games_via_cdp(port).await;

        let spoof_raw = spoof_raw.expect("spoof JS should execute");
        let spoof_parsed: serde_json::Value =
            serde_json::from_str(&spoof_raw).expect("spoof JS should return JSON");
        assert_eq!(
            spoof_parsed.get("success"),
            Some(&serde_json::json!(true)),
            "spoof failed: {spoof_raw}"
        );
        assert_eq!(
            spoof_parsed.get("wrappedObserver"),
            Some(&serde_json::json!(true)),
            "spoof should wrap setObservedGamesCallback: {spoof_raw}"
        );
        cleanup.expect("cleanup should succeed even if Overlay/popout targets exist");

        let during = during.expect("snapshot during spoof");
        let after = after.expect("snapshot after cleanup");
        assert_eq!(json_id(during.games.first()), Some(app_id.clone()));
        assert_eq!(json_id(during.visible_game.as_ref()), Some(app_id.clone()));
        assert_eq!(
            json_id(during.analytics_game.as_ref()),
            Some(app_id.clone())
        );
        let spoofed = snapshot_game_by_id(&during, &app_id).expect("spoofed game");
        assert_eq!(spoofed.get("lastFocused"), Some(&serde_json::json!(0)));
        assert!(spoofed
            .get("start")
            .and_then(|value| value.as_u64())
            .is_some());
        let pid = spoofed.get("pid");
        assert!(
            pid.is_none()
                || pid == Some(&serde_json::Value::Null)
                || pid == Some(&serde_json::json!("<undefined>")),
            "spoof must not invent a pid"
        );
        assert!(
            snapshot_game_by_id(&after, &app_id).is_none(),
            "cleanup must remove the synthetic running game"
        );

        let verify_raw = crate::cdp_client::execute_js_via_primary_discord_target(
            port,
            &with_bridge(JS_VERIFY_CLEANUP_STATE),
            false,
            CDP_CLEANUP_VERIFY_TIMEOUT_SECS,
        )
        .await;
        let verify_raw = verify_raw.expect("cleanup verify should execute");
        let verify_parsed: serde_json::Value =
            serde_json::from_str(&verify_raw).expect("cleanup verify should return JSON");
        let verify =
            cleanup_verify_from_json(&verify_parsed).expect("cleanup verify should report success");
        assert!(
            cleanup_verify_is_clean(&verify),
            "main renderer still dirty after cleanup: {verify_raw}"
        );
        assert!(
            after.debug_game.is_none()
                || after.debug_game == Some(serde_json::Value::Null)
                || after.debug_game == Some(serde_json::json!("<undefined>")),
            "getDebugRunningGame must stay unset"
        );
    }
}

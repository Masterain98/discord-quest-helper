use crate::models::DetectableGame;
use crate::simulation_history::SimulationHistory;
use rand::seq::SliceRandom;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Emitter;

const UPCOMING_LENGTH: usize = 5;
const RECENT_LENGTH: usize = 5;
const MAX_CONSECUTIVE_FAILURES: usize = 5;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GameIdleMode {
    Process,
    Cdp,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GameIdleSelectionMode {
    #[default]
    Random,
    Manual,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GameIdleCandidateType {
    #[default]
    All,
    Game,
    App,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GameIdleDurationMode {
    #[default]
    Fixed,
    Random,
}

/// A per-cycle duration that is either a constant or sampled from an
/// inclusive range. Units are implied by the field that carries it.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct GameIdleDuration {
    pub mode: GameIdleDurationMode,
    pub fixed: u32,
    pub min: u32,
    pub max: u32,
}

impl Default for GameIdleDuration {
    fn default() -> Self {
        Self {
            mode: GameIdleDurationMode::Fixed,
            fixed: 1,
            min: 1,
            max: 1,
        }
    }
}

impl GameIdleDuration {
    fn resolve(&self) -> u32 {
        match self.mode {
            GameIdleDurationMode::Fixed => self.fixed,
            // The swap guard keeps a hand-edited config from panicking the
            // session worker before `validate` ever sees it.
            GameIdleDurationMode::Random => {
                let (low, high) = (self.min.min(self.max), self.min.max(self.max));
                rand::rng().random_range(low..=high)
            }
        }
    }

    fn validate(&self, field: &str, minimum: u32) -> Result<(), String> {
        let valid = match self.mode {
            GameIdleDurationMode::Fixed => self.fixed >= minimum,
            GameIdleDurationMode::Random => self.min >= minimum && self.min <= self.max,
        };
        if valid {
            Ok(())
        } else {
            Err(format!(
                "{field} must be a fixed value of at least {minimum}, or a random range where min >= {minimum} and min <= max"
            ))
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct GameIdleConfig {
    pub mode: GameIdleMode,
    pub selection_mode: GameIdleSelectionMode,
    pub candidate_type: GameIdleCandidateType,
    pub selected_app_ids: Vec<String>,
    pub play_duration: GameIdleDuration,
    pub rest_duration: GameIdleDuration,
    pub cdp_port: u16,
    pub simulation_path: String,
}

impl Default for GameIdleConfig {
    fn default() -> Self {
        Self {
            mode: GameIdleMode::Process,
            selection_mode: GameIdleSelectionMode::default(),
            candidate_type: GameIdleCandidateType::default(),
            selected_app_ids: Vec::new(),
            play_duration: GameIdleDuration::default(),
            rest_duration: GameIdleDuration::default(),
            cdp_port: 0,
            simulation_path: String::new(),
        }
    }
}

impl GameIdleConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.play_duration.validate("Game simulation duration", 1)?;
        self.rest_duration.validate("Rest duration", 0)?;
        if self.mode == GameIdleMode::Cdp && self.cdp_port == 0 {
            return Err("CDP port must be between 1 and 65535".to_string());
        }
        if self.mode == GameIdleMode::Process && self.simulation_path.trim().is_empty() {
            return Err("Simulation directory is required for process mode".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameIdleItem {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub type_name: Option<String>,
    /// Stable identity for this occurrence in the idle reel. The same Discord
    /// application can appear again in a later shuffle cycle.
    pub occurrence_id: String,
    #[serde(skip)]
    executable_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GameIdlePhase {
    Starting,
    Playing,
    Resting,
    Stopping,
    Error,
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameIdleStatus {
    pub session_id: String,
    pub mode: GameIdleMode,
    pub selection_mode: GameIdleSelectionMode,
    pub candidate_type: GameIdleCandidateType,
    pub selected_app_ids: Vec<String>,
    pub phase: GameIdlePhase,
    /// Resolved minutes of the current cycle; `play_duration`/`rest_duration`
    /// carry the configured fixed-or-random spec the UI editor restores from.
    pub play_minutes: u32,
    pub rest_minutes: u32,
    pub play_duration: GameIdleDuration,
    pub rest_duration: GameIdleDuration,
    pub current: Option<GameIdleItem>,
    pub recent: Vec<GameIdleItem>,
    pub upcoming: Vec<GameIdleItem>,
    pub phase_started_at: i64,
    pub phase_ends_at: Option<i64>,
    pub accumulated_played_seconds: u64,
    pub warning: Option<String>,
}

impl GameIdleStatus {
    fn starting(config: &GameIdleConfig, candidates: &[GameIdleItem]) -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            mode: config.mode,
            selection_mode: config.selection_mode,
            candidate_type: config.candidate_type,
            selected_app_ids: if config.selection_mode == GameIdleSelectionMode::Manual {
                candidates.iter().map(|item| item.id.clone()).collect()
            } else {
                Vec::new()
            },
            phase: GameIdlePhase::Starting,
            play_minutes: config.play_duration.resolve(),
            rest_minutes: config.rest_duration.resolve(),
            play_duration: config.play_duration,
            rest_duration: config.rest_duration,
            current: None,
            recent: Vec::new(),
            upcoming: Vec::new(),
            phase_started_at: now_millis(),
            phase_ends_at: None,
            accumulated_played_seconds: 0,
            warning: None,
        }
    }
}

#[derive(Debug, Clone)]
enum RunningSimulation {
    Process { executable_name: String },
    Cdp { port: u16 },
}

struct StartSimulationError {
    message: String,
    running: Option<RunningSimulation>,
}

impl From<String> for StartSimulationError {
    fn from(message: String) -> Self {
        Self {
            message,
            running: None,
        }
    }
}

#[derive(Debug)]
struct IdleShared {
    status: GameIdleStatus,
    all: Vec<GameIdleItem>,
    bag: Vec<GameIdleItem>,
    next_manual_index: usize,
    removed_this_cycle: HashSet<String>,
    upcoming: VecDeque<GameIdleItem>,
    recent: VecDeque<GameIdleItem>,
    running: Option<RunningSimulation>,
}

impl IdleShared {
    fn refresh_status_lists(&mut self) {
        self.status.recent = self.recent.iter().cloned().collect();
        self.status.upcoming = self.upcoming.iter().cloned().collect();
    }

    fn refill_bag(&mut self) {
        // Exhausting the bag ends the current shuffle cycle, so a game the user
        // removed may become eligible again in the next one. Keeping the
        // exclusion across refills would strand it forever: an excluded game
        // never enters `upcoming`, so it could never be played and never
        // released.
        self.removed_this_cycle.clear();
        self.bag = self.all.clone();
        shuffle(&mut self.bag);
    }

    fn fill_upcoming(&mut self) {
        if self.all.is_empty() {
            self.refresh_status_lists();
            return;
        }
        if self.status.selection_mode == GameIdleSelectionMode::Manual {
            // Small playlists may show the same application in a later lap.
            // Occurrence identities distinguish those entries from the active one.
            while self.upcoming.len() < UPCOMING_LENGTH.min(self.all.len()) {
                let mut item = self.all[self.next_manual_index].clone();
                self.next_manual_index = (self.next_manual_index + 1) % self.all.len();
                item.occurrence_id = uuid::Uuid::new_v4().to_string();
                self.upcoming.push_back(item);
            }
            self.refresh_status_lists();
            return;
        }
        let mut attempts = 0usize;
        let max_attempts = self.all.len().saturating_mul(4).max(20);
        while self.upcoming.len() < UPCOMING_LENGTH && attempts < max_attempts {
            attempts += 1;
            if self.bag.is_empty() {
                self.refill_bag();
            }
            let Some(candidate) = self.bag.pop() else {
                break;
            };
            if self.removed_this_cycle.contains(&candidate.id) {
                continue;
            }
            let duplicates_active_queue = (self.all.len() > 1
                && self
                    .status
                    .current
                    .as_ref()
                    .is_some_and(|current| current.id == candidate.id))
                || self.upcoming.iter().any(|item| item.id == candidate.id);
            if duplicates_active_queue {
                continue;
            }
            let recently_played = self.recent.iter().any(|item| item.id == candidate.id);
            if recently_played && self.all.len() > UPCOMING_LENGTH + RECENT_LENGTH {
                continue;
            }
            let mut occurrence = candidate;
            occurrence.occurrence_id = uuid::Uuid::new_v4().to_string();
            self.upcoming.push_back(occurrence);
        }
        self.refresh_status_lists();
    }

    fn remove_upcoming_item(&mut self, app_id: &str, occurrence_id: &str) -> bool {
        let Some(position) = self
            .upcoming
            .iter()
            .position(|item| item.id == app_id && item.occurrence_id == occurrence_id)
        else {
            return false;
        };
        self.upcoming.remove(position);
        if self.status.selection_mode == GameIdleSelectionMode::Manual {
            // Skip only this occurrence. Defer filling until the next handoff,
            // so removing from a tiny playlist does not visibly bounce it back.
            self.refresh_status_lists();
            return true;
        }
        self.removed_this_cycle.insert(app_id.to_string());
        self.fill_upcoming();
        // A tiny pool may need to cross into a new shuffle cycle immediately,
        // which clears the cycle exclusion set. Never let the item the user
        // just removed bounce straight back into the visible queue; showing
        // fewer than five candidates is preferable in that case.
        self.upcoming.retain(|item| item.id != app_id);
        self.refresh_status_lists();
        true
    }
}

struct ActiveIdleSession {
    shared: Arc<tokio::sync::Mutex<IdleShared>>,
    cancel: Option<tokio::sync::watch::Sender<bool>>,
    join: Option<tauri::async_runtime::JoinHandle<()>>,
}

#[derive(Default)]
pub struct GameIdleManager {
    active: tokio::sync::Mutex<Option<ActiveIdleSession>>,
}

impl GameIdleManager {
    pub async fn ensure_idle(&self) -> Result<(), String> {
        if self.active.lock().await.is_some() {
            Err("Stop game idle mode before starting another activity".to_string())
        } else {
            Ok(())
        }
    }

    pub async fn start(
        &self,
        config: GameIdleConfig,
        games: Vec<DetectableGame>,
        user_id: String,
        history: Arc<SimulationHistory>,
        history_path: PathBuf,
        app: tauri::AppHandle,
    ) -> Result<GameIdleStatus, String> {
        config.validate()?;
        let mut active = self.active.lock().await;
        if active.is_some() {
            return Err("Game idle mode is already running".to_string());
        }

        let candidates = configured_candidates(&config, games)?;
        if candidates.is_empty() {
            return Err(match config.mode {
                GameIdleMode::Process => {
                    "No Discord-detectable applications have an executable compatible with this system"
                        .to_string()
                }
                GameIdleMode::Cdp => "Discord returned no detectable games or applications".to_string(),
            });
        }

        let status = GameIdleStatus::starting(&config, &candidates);
        let mut shared_value = IdleShared {
            status,
            all: candidates,
            bag: Vec::new(),
            next_manual_index: 0,
            removed_this_cycle: HashSet::new(),
            upcoming: VecDeque::new(),
            recent: VecDeque::new(),
            running: None,
        };
        if config.selection_mode == GameIdleSelectionMode::Random {
            shared_value.refill_bag();
        }
        shared_value.fill_upcoming();
        // Pick and expose the first queue entry before the worker begins its
        // native launch. The UI can render the complete queue immediately,
        // reducing perceived startup latency while the first game is prepared.
        shared_value.status.current = shared_value.upcoming.front().cloned();
        shared_value.refresh_status_lists();
        let initial = shared_value.status.clone();
        emit_status(&app, &initial);
        let shared = Arc::new(tokio::sync::Mutex::new(shared_value));
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let task_shared = Arc::clone(&shared);
        let task_app = app.clone();
        let join = tauri::async_runtime::spawn(async move {
            run_session(
                config,
                task_shared,
                cancel_rx,
                user_id,
                history,
                history_path,
                task_app,
            )
            .await;
        });
        *active = Some(ActiveIdleSession {
            shared,
            cancel: Some(cancel_tx),
            join: Some(join),
        });
        Ok(initial)
    }

    pub async fn status(&self) -> Option<GameIdleStatus> {
        let shared = self
            .active
            .lock()
            .await
            .as_ref()
            .map(|session| Arc::clone(&session.shared));
        match shared {
            Some(shared) => Some(shared.lock().await.status.clone()),
            None => None,
        }
    }

    pub async fn remove_upcoming(
        &self,
        session_id: &str,
        app_id: &str,
        occurrence_id: &str,
        app: &tauri::AppHandle,
    ) -> Result<GameIdleStatus, String> {
        let shared = self
            .active
            .lock()
            .await
            .as_ref()
            .map(|session| Arc::clone(&session.shared))
            .ok_or_else(|| "Game idle mode is not running".to_string())?;
        let mut shared = shared.lock().await;
        if shared.status.session_id != session_id {
            return Err("The game idle session has changed".to_string());
        }
        if shared.status.phase == GameIdlePhase::Starting
            && shared
                .upcoming
                .front()
                .is_some_and(|item| item.id == app_id && item.occurrence_id == occurrence_id)
        {
            return Err("The game currently being started cannot be removed".to_string());
        }
        if !shared.remove_upcoming_item(app_id, occurrence_id) {
            return Err("Only upcoming games can be removed".to_string());
        }
        emit_status(app, &shared.status);
        Ok(shared.status.clone())
    }

    pub async fn stop(
        &self,
        history: Arc<SimulationHistory>,
        app: &tauri::AppHandle,
    ) -> Result<Option<GameIdleStatus>, String> {
        let (shared, join) = {
            let mut active = self.active.lock().await;
            let Some(session) = active.as_mut() else {
                return Ok(None);
            };
            {
                let mut shared = session.shared.lock().await;
                shared.status.phase = GameIdlePhase::Stopping;
                shared.status.phase_ends_at = None;
                emit_status(app, &shared.status);
            }
            if let Some(cancel) = session.cancel.take() {
                let _ = cancel.send(true);
            }
            (Arc::clone(&session.shared), session.join.take())
        };

        if let Some(join) = join {
            // Do not abort an in-flight spawn_blocking call: Tokio cannot
            // cancel that OS work, and it could otherwise launch a child after
            // the manager had already declared the session stopped.
            let _ = join.await;
        }

        let cleanup_result = {
            let mut shared = shared.lock().await;
            if let Some(running) = shared.running.clone() {
                match stop_simulation(&running, app).await {
                    Ok(()) => {
                        shared.running = None;
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else {
                Ok(())
            }
        };
        if let Err(error) = cleanup_result {
            let mut shared = shared.lock().await;
            shared.status.phase = GameIdlePhase::Error;
            shared.status.warning = Some(error.clone());
            emit_status(app, &shared.status);
            return Err(error);
        }

        if let Err(error) = history.finish_active(Some(app)).await {
            let mut shared = shared.lock().await;
            shared.status.phase = GameIdlePhase::Error;
            shared.status.phase_ends_at = None;
            shared.status.warning = Some(error.clone());
            emit_status(app, &shared.status);
            // Keep the manager entry so the user can retry Stop. Its native
            // resource is already gone, while the history segment remains
            // active until the final interval is durably persisted.
            return Err(error);
        }
        let final_status = {
            let mut shared = shared.lock().await;
            shared.status.phase = GameIdlePhase::Stopped;
            shared.status.phase_ends_at = None;
            emit_status(app, &shared.status);
            shared.status.clone()
        };
        *self.active.lock().await = None;
        Ok(Some(final_status))
    }
}

async fn run_session(
    config: GameIdleConfig,
    shared: Arc<tokio::sync::Mutex<IdleShared>>,
    mut cancel: tokio::sync::watch::Receiver<bool>,
    user_id: String,
    history: Arc<SimulationHistory>,
    history_path: PathBuf,
    app: tauri::AppHandle,
) {
    let mut consecutive_failures = 0usize;
    loop {
        if *cancel.borrow() {
            break;
        }
        let candidate = {
            let mut state = shared.lock().await;
            state.status.phase = GameIdlePhase::Starting;
            state.status.phase_started_at = now_millis();
            state.status.phase_ends_at = None;
            if state.upcoming.is_empty() {
                state.fill_upcoming();
            }
            // Keep the candidate in the visible future queue until its
            // activity has actually started. This preserves the completed
            // game at the center throughout rest/start retries and makes the
            // reel advance only on a successful handoff.
            let candidate = state.upcoming.front().cloned();
            emit_status(&app, &state.status);
            candidate
        };
        let Some(candidate) = candidate else {
            set_error(&shared, &app, "No eligible game is available".to_string()).await;
            return;
        };

        let mut running = None;
        let mut last_error = String::new();
        for attempt in 0..2 {
            match start_simulation(&candidate, &config, &app).await {
                Ok(value) => {
                    running = Some(value);
                    break;
                }
                Err(error) => {
                    last_error = error.message;
                    if let Some(running) = error.running {
                        // The attempted rollback failed. Retain ownership for
                        // the normal Stop retry instead of launching it again.
                        shared.lock().await.running = Some(running);
                        set_error(&shared, &app, last_error).await;
                        return;
                    }
                    if attempt == 0 {
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(2)) => {},
                            _ = cancel.changed() => { break; }
                        }
                    }
                }
            }
        }

        // Stopping during the retry delay is not a failed game start. Exit
        // without removing the candidate, emitting a warning, or incrementing
        // the consecutive-failure circuit breaker.
        if *cancel.borrow() && running.is_none() {
            break;
        }

        let Some(running) = running else {
            consecutive_failures += 1;
            {
                let mut state = shared.lock().await;
                if state
                    .upcoming
                    .front()
                    .is_some_and(|queued| queued.id == candidate.id)
                {
                    state.upcoming.pop_front();
                }
                state.status.current = state.recent.back().cloned();
                state.status.warning = Some(format!(
                    "Skipped {} after two failed start attempts: {}",
                    candidate.name, last_error
                ));
                state.fill_upcoming();
                emit_status(&app, &state.status);
            }
            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                set_error(
                    &shared,
                    &app,
                    "Five games failed to start consecutively. Check Discord and the selected simulation mode."
                        .to_string(),
                )
                .await;
                return;
            }
            continue;
        };
        consecutive_failures = 0;

        {
            let mut state = shared.lock().await;
            // Record ownership before beginning history persistence so any
            // cleanup failure still leaves a retryable Stop path.
            state.running = Some(running.clone());
        }

        if let Err(error) = history
            .begin(
                history_path.clone(),
                app.clone(),
                user_id.clone(),
                candidate.id.clone(),
                candidate.name.clone(),
            )
            .await
        {
            let cleanup_error = stop_simulation(&running, &app).await.err();
            if cleanup_error.is_none() {
                shared.lock().await.running = None;
            }
            let detail = cleanup_error
                .map(|cleanup| format!("{error}. Cleanup also failed: {cleanup}"))
                .unwrap_or(error);
            set_error(&shared, &app, detail).await;
            return;
        }

        let play_started = Instant::now();
        let play_minutes = config.play_duration.resolve();
        {
            let mut state = shared.lock().await;
            if state
                .upcoming
                .front()
                .is_some_and(|queued| queued.id == candidate.id)
            {
                state.upcoming.pop_front();
            }
            state.status.current = Some(candidate.clone());
            state.recent.push_back(candidate.clone());
            while state.recent.len() > RECENT_LENGTH {
                state.recent.pop_front();
            }
            state.fill_upcoming();
            state.status.play_minutes = play_minutes;
            state.status.phase = GameIdlePhase::Playing;
            state.status.phase_started_at = now_millis();
            state.status.phase_ends_at =
                Some(state.status.phase_started_at + i64::from(play_minutes) * 60_000);
            emit_status(&app, &state.status);
        }

        let play_duration = Duration::from_secs(u64::from(play_minutes) * 60);
        let cancelled = tokio::select! {
            _ = tokio::time::sleep(play_duration) => false,
            _ = cancel.changed() => true,
        };

        let cleanup = stop_simulation(&running, &app).await;
        if let Err(error) = cleanup {
            set_error(
                &shared,
                &app,
                format!("Failed to stop {}: {}", candidate.name, error),
            )
            .await;
            return;
        }
        {
            let mut state = shared.lock().await;
            state.running = None;
            state.status.accumulated_played_seconds = state
                .status
                .accumulated_played_seconds
                .saturating_add(play_started.elapsed().as_secs());
        }
        if let Err(error) = history.finish(&history_path, Some(&app)).await {
            set_error(&shared, &app, error).await;
            return;
        }
        if cancelled || *cancel.borrow() {
            break;
        }

        let rest_minutes = config.rest_duration.resolve();
        if rest_minutes > 0 {
            {
                let mut state = shared.lock().await;
                state.status.rest_minutes = rest_minutes;
                state.status.phase = GameIdlePhase::Resting;
                state.status.phase_started_at = now_millis();
                state.status.phase_ends_at =
                    Some(state.status.phase_started_at + i64::from(rest_minutes) * 60_000);
                emit_status(&app, &state.status);
            }
            let rest_duration = Duration::from_secs(u64::from(rest_minutes) * 60);
            tokio::select! {
                _ = tokio::time::sleep(rest_duration) => {},
                _ = cancel.changed() => { break; }
            }
        }
    }

    let mut state = shared.lock().await;
    state.status.phase = GameIdlePhase::Stopped;
    state.status.phase_ends_at = None;
    emit_status(&app, &state.status);
}

async fn start_simulation(
    game: &GameIdleItem,
    config: &GameIdleConfig,
    _app: &tauri::AppHandle,
) -> Result<RunningSimulation, StartSimulationError> {
    match config.mode {
        GameIdleMode::Process => {
            let executable_name = game
                .executable_name
                .clone()
                .ok_or_else(|| "No compatible executable is available".to_string())?;
            let path = config.simulation_path.clone();
            let name = game.name.clone();
            let app_id = game.id.clone();
            let executable_for_task = executable_name.clone();
            tauri::async_runtime::spawn_blocking(move || {
                crate::game_simulator::create_simulated_game(&path, &executable_for_task, &app_id)?;
                crate::game_simulator::run_simulated_game(
                    &name,
                    &path,
                    &executable_for_task,
                    &app_id,
                )
            })
            .await
            .map_err(|error| format!("Process simulator task failed: {error}"))?
            .map_err(|error| error.to_string())?;

            let activity = serde_json::json!({
                "app_id": game.id,
                "state": "In Game",
                "details": format!("Playing {}", game.name),
                "large_image_key": "logo",
                "large_image_text": game.name,
                "start_timestamp": chrono::Utc::now().timestamp_millis(),
            });
            if let Err(error) = crate::replace_discord_rpc(activity.to_string()).await {
                let executable = executable_name.clone();
                let cleanup = tauri::async_runtime::spawn_blocking(move || {
                    crate::game_simulator::stop_simulated_game(&executable)
                })
                .await
                .map_err(|join_error| join_error.to_string())
                .and_then(|result| result.map_err(|stop_error| stop_error.to_string()));
                return match cleanup {
                    Ok(()) => Err(error.into()),
                    Err(cleanup_error) => Err(StartSimulationError {
                        message: format!("{error}. Process cleanup also failed: {cleanup_error}"),
                        running: Some(RunningSimulation::Process { executable_name }),
                    }),
                };
            }
            Ok(RunningSimulation::Process { executable_name })
        }
        GameIdleMode::Cdp => {
            crate::cdp_quest::start_manual_game_spoof(config.cdp_port, &game.id, &game.name)
                .await
                .map_err(|error| error.to_string())?;
            Ok(RunningSimulation::Cdp {
                port: config.cdp_port,
            })
        }
    }
}

async fn stop_simulation(
    running: &RunningSimulation,
    _app: &tauri::AppHandle,
) -> Result<(), String> {
    match running {
        RunningSimulation::Process { executable_name } => {
            let rpc_error = crate::clear_discord_rpc().await.err();
            let executable_name = executable_name.clone();
            let process_error = tauri::async_runtime::spawn_blocking(move || {
                crate::game_simulator::stop_simulated_game(&executable_name)
            })
            .await
            .map_err(|error| format!("Process cleanup task failed: {error}"))?
            .err()
            .map(|error| error.to_string());
            match (rpc_error, process_error) {
                (None, None) => Ok(()),
                (Some(rpc), None) => Err(rpc),
                (None, Some(process)) => Err(process),
                (Some(rpc), Some(process)) => {
                    Err(format!("{rpc}. Process cleanup also failed: {process}"))
                }
            }
        }
        RunningSimulation::Cdp { port } => crate::cdp_quest::stop_manual_game_spoof(*port)
            .await
            .map_err(|error| error.to_string()),
    }
}

fn configured_candidates(
    config: &GameIdleConfig,
    games: Vec<DetectableGame>,
) -> Result<Vec<GameIdleItem>, String> {
    if config.selection_mode == GameIdleSelectionMode::Random {
        let filtered = games
            .into_iter()
            .filter(|game| match config.candidate_type {
                GameIdleCandidateType::All => true,
                GameIdleCandidateType::Game => game.type_name.as_deref() == Some("Game"),
                GameIdleCandidateType::App => game.type_name.as_deref() == Some("App"),
            })
            .collect();
        return Ok(eligible_games(config.mode, filtered));
    }
    if config.selected_app_ids.is_empty() {
        return Err("Select at least one game or application for the idle playlist".into());
    }
    let available = eligible_games(config.mode, games);
    let mut seen = HashSet::new();
    config.selected_app_ids.iter().filter(|id| seen.insert((*id).clone())).map(|id| {
        available.iter().find(|game| &game.id == id).cloned().ok_or_else(|| {
            format!("Playlist application {id} is missing or incompatible with the selected simulation mode")
        })
    }).collect()
}

fn eligible_games(mode: GameIdleMode, games: Vec<DetectableGame>) -> Vec<GameIdleItem> {
    let mut seen = HashSet::new();
    games
        .into_iter()
        .filter_map(|game| {
            let executable_name = select_executable(&game);
            if mode == GameIdleMode::Process && executable_name.is_none() {
                return None;
            }
            if !seen.insert(game.id.clone()) {
                return None;
            }
            Some(GameIdleItem {
                id: game.id,
                name: game.name,
                icon: game.icon,
                type_name: game.type_name,
                occurrence_id: String::new(),
                executable_name,
            })
        })
        .collect()
}

fn select_executable(game: &DetectableGame) -> Option<String> {
    #[cfg(target_os = "linux")]
    let preferred = "linux";
    #[cfg(not(target_os = "linux"))]
    let preferred = "win32";
    game.executables
        .iter()
        .find(|executable| executable.os == preferred)
        .map(|executable| executable.name.clone())
}

fn shuffle<T>(values: &mut [T]) {
    values.shuffle(&mut rand::rng());
}

async fn set_error(
    shared: &Arc<tokio::sync::Mutex<IdleShared>>,
    app: &tauri::AppHandle,
    error: String,
) {
    let mut state = shared.lock().await;
    state.status.phase = GameIdlePhase::Error;
    state.status.phase_ends_at = None;
    state.status.warning = Some(error);
    emit_status(app, &state.status);
}

fn emit_status(app: &tauri::AppHandle, status: &GameIdleStatus) {
    let _ = app.emit("game-idle-status-changed", status.clone());
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::GameExecutable;

    fn game(id: &str, os: &str) -> DetectableGame {
        DetectableGame {
            id: id.into(),
            name: format!("Game {id}"),
            executables: vec![GameExecutable {
                name: format!("{id}.exe"),
                os: os.into(),
            }],
            icon: None,
            type_name: Some("Game".into()),
        }
    }

    fn config() -> GameIdleConfig {
        serde_json::from_value(serde_json::json!({
            "mode": "cdp",
            "playDuration": { "mode": "fixed", "fixed": 1, "min": 1, "max": 1 },
            "restDuration": { "mode": "fixed", "fixed": 1, "min": 1, "max": 1 },
            "cdpPort": 9223, "simulationPath": "simulation"
        }))
        .unwrap()
    }

    fn fixed_duration(minutes: u32) -> GameIdleDuration {
        GameIdleDuration {
            mode: GameIdleDurationMode::Fixed,
            fixed: minutes,
            min: minutes,
            max: minutes,
        }
    }

    fn manual_queue(count: usize) -> IdleShared {
        IdleShared {
            status: GameIdleStatus {
                session_id: "manual".into(),
                mode: GameIdleMode::Cdp,
                selection_mode: GameIdleSelectionMode::Manual,
                candidate_type: GameIdleCandidateType::All,
                selected_app_ids: (0..count).map(|id| id.to_string()).collect(),
                phase: GameIdlePhase::Starting,
                play_minutes: 1,
                rest_minutes: 1,
                play_duration: fixed_duration(1),
                rest_duration: fixed_duration(1),
                current: None,
                recent: vec![],
                upcoming: vec![],
                phase_started_at: 0,
                phase_ends_at: None,
                accumulated_played_seconds: 0,
                warning: None,
            },
            all: eligible_games(
                GameIdleMode::Cdp,
                (0..count)
                    .map(|id| game(&id.to_string(), "win32"))
                    .collect(),
            ),
            bag: vec![],
            next_manual_index: 0,
            removed_this_cycle: HashSet::new(),
            upcoming: VecDeque::new(),
            recent: VecDeque::new(),
            running: None,
        }
    }

    #[test]
    fn old_config_defaults_to_random_all() {
        let config = config();
        assert_eq!(config.selection_mode, GameIdleSelectionMode::Random);
        assert_eq!(config.candidate_type, GameIdleCandidateType::All);
        assert!(config.selected_app_ids.is_empty());
    }

    #[test]
    fn random_type_filter_runs_before_deduplication() {
        let mut app = game("same", "win32");
        app.type_name = Some("App".into());
        let games = vec![game("same", "win32"), app, game("game", "win32")];
        let mut config = config();
        config.candidate_type = GameIdleCandidateType::App;
        let apps = configured_candidates(&config, games.clone()).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].type_name.as_deref(), Some("App"));
        config.candidate_type = GameIdleCandidateType::Game;
        assert_eq!(
            configured_candidates(&config, games.clone()).unwrap().len(),
            2
        );
        config.candidate_type = GameIdleCandidateType::All;
        assert_eq!(configured_candidates(&config, games).unwrap().len(), 2);
    }

    #[test]
    fn manual_selection_preserves_order_and_ignores_random_type_filter() {
        let mut config = config();
        config.selection_mode = GameIdleSelectionMode::Manual;
        config.candidate_type = GameIdleCandidateType::App;
        config.selected_app_ids = vec!["b".into(), "a".into(), "b".into()];
        let selected =
            configured_candidates(&config, vec![game("a", "win32"), game("b", "win32")]).unwrap();
        let status = GameIdleStatus::starting(&config, &selected);
        assert_eq!(status.selected_app_ids, vec!["b", "a"]);
        let serialized = serde_json::to_value(status).unwrap();
        assert_eq!(serialized["selectedAppIds"], serde_json::json!(["b", "a"]));
        assert_eq!(
            selected
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
    }

    #[test]
    fn manual_selection_rejects_empty_missing_and_incompatible_items() {
        let mut config = config();
        config.selection_mode = GameIdleSelectionMode::Manual;
        assert!(configured_candidates(&config, vec![]).is_err());
        config.selected_app_ids = vec!["missing".into()];
        assert!(configured_candidates(&config, vec![game("a", "win32")]).is_err());
        config.selected_app_ids = vec!["a".into()];
        let mut without_exe = game("a", "win32");
        without_exe.executables.clear();
        assert_eq!(
            configured_candidates(&config, vec![without_exe.clone()])
                .unwrap()
                .len(),
            1
        );
        config.mode = GameIdleMode::Process;
        assert!(configured_candidates(&config, vec![without_exe]).is_err());
    }

    #[test]
    fn manual_queue_repeats_in_exact_order_for_small_and_large_lists() {
        for count in [1, 2, 5, 8] {
            let mut queue = manual_queue(count);
            let mut occurrences = HashSet::new();
            for index in 0..count * 4 {
                queue.fill_upcoming();
                assert_eq!(queue.upcoming.len(), count.min(UPCOMING_LENGTH));
                let item = queue.upcoming.pop_front().unwrap();
                assert_eq!(item.id, (index % count).to_string());
                assert!(occurrences.insert(item.occurrence_id.clone()));
                queue.status.current = Some(item.clone());
                queue.recent.push_back(item);
            }
        }
    }

    #[test]
    fn manual_skip_removes_one_occurrence_and_keeps_saved_list_and_later_laps() {
        let mut queue = manual_queue(2);
        queue.fill_upcoming();
        let removed = queue.upcoming.front().unwrap().clone();
        assert!(!queue.remove_upcoming_item(&removed.id, "stale"));
        assert!(queue.remove_upcoming_item(&removed.id, &removed.occurrence_id));
        assert_eq!(queue.upcoming.len(), 1);
        queue.fill_upcoming();
        assert_eq!(
            queue
                .upcoming
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "0"]
        );
        assert_ne!(queue.upcoming[1].occurrence_id, removed.occurrence_id);
        assert_eq!(queue.all.len(), 2);
        assert_eq!(queue.status.selected_app_ids, vec!["0", "1"]);
        assert!(queue.removed_this_cycle.is_empty());
    }

    #[test]
    fn duration_validation_rejects_zero_play_time() {
        let mut config = GameIdleConfig {
            mode: GameIdleMode::Cdp,
            cdp_port: 9223,
            ..GameIdleConfig::default()
        };
        config.play_duration = GameIdleDuration {
            mode: GameIdleDurationMode::Fixed,
            fixed: 0,
            min: 0,
            max: 0,
        };
        assert!(config.validate().is_err());
        config.play_duration = GameIdleDuration {
            mode: GameIdleDurationMode::Random,
            fixed: 1,
            min: 0,
            max: 5,
        };
        assert!(config.validate().is_err());
        config.play_duration = GameIdleDuration {
            mode: GameIdleDurationMode::Random,
            fixed: 1,
            min: 6,
            max: 5,
        };
        assert!(config.validate().is_err());
        config.play_duration = GameIdleDuration {
            mode: GameIdleDurationMode::Random,
            fixed: 1,
            min: 1,
            max: 5,
        };
        config.rest_duration = GameIdleDuration {
            mode: GameIdleDurationMode::Fixed,
            fixed: 0,
            min: 0,
            max: 0,
        };
        assert!(config.validate().is_ok());
        config.rest_duration = GameIdleDuration {
            mode: GameIdleDurationMode::Random,
            fixed: 0,
            min: 0,
            max: 3,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn duration_resolution_stays_within_bounds() {
        let random = GameIdleDuration {
            mode: GameIdleDurationMode::Random,
            fixed: 1,
            min: 2,
            max: 5,
        };
        for _ in 0..64 {
            assert!((2..=5).contains(&random.resolve()));
        }
        let fixed = GameIdleDuration {
            mode: GameIdleDurationMode::Fixed,
            fixed: 7,
            min: 2,
            max: 5,
        };
        assert_eq!(fixed.resolve(), 7);
    }

    #[test]
    fn cdp_pool_deduplicates_but_keeps_entries_without_executables() {
        let games = vec![
            game("1", "win32"),
            game("1", "win32"),
            DetectableGame {
                id: "2".into(),
                name: "App 2".into(),
                executables: vec![],
                icon: None,
                type_name: Some("App".into()),
            },
        ];
        let eligible = eligible_games(GameIdleMode::Cdp, games);
        assert_eq!(eligible.len(), 2);
    }

    #[test]
    fn process_pool_only_keeps_host_compatible_entries() {
        let eligible = eligible_games(
            GameIdleMode::Process,
            vec![game("windows", "win32"), game("linux", "linux")],
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            eligible
                .iter()
                .map(|game| game.id.as_str())
                .collect::<Vec<_>>(),
            vec!["linux"]
        );
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            eligible
                .iter()
                .map(|game| game.id.as_str())
                .collect::<Vec<_>>(),
            vec!["windows"]
        );
    }

    #[test]
    fn queue_remains_unique_when_the_pool_is_large_enough() {
        let all: Vec<_> = (0..20)
            .map(|index| GameIdleItem {
                id: index.to_string(),
                name: index.to_string(),
                icon: None,
                type_name: None,
                occurrence_id: String::new(),
                executable_name: Some(format!("{index}.exe")),
            })
            .collect();
        let mut shared = IdleShared {
            status: GameIdleStatus {
                session_id: "session".into(),
                mode: GameIdleMode::Process,
                selection_mode: GameIdleSelectionMode::Random,
                candidate_type: GameIdleCandidateType::All,
                selected_app_ids: vec![],
                phase: GameIdlePhase::Starting,
                play_minutes: 1,
                rest_minutes: 0,
                play_duration: fixed_duration(1),
                rest_duration: fixed_duration(0),
                current: None,
                recent: vec![],
                upcoming: vec![],
                phase_started_at: 0,
                phase_ends_at: None,
                accumulated_played_seconds: 0,
                warning: None,
            },
            all,
            bag: vec![],
            next_manual_index: 0,
            removed_this_cycle: HashSet::new(),
            upcoming: VecDeque::new(),
            recent: VecDeque::new(),
            running: None,
        };
        shared.refill_bag();
        shared.fill_upcoming();
        let ids: HashSet<_> = shared
            .upcoming
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(shared.upcoming.len(), UPCOMING_LENGTH);
        assert_eq!(ids.len(), UPCOMING_LENGTH);
    }

    #[test]
    fn removing_from_a_tiny_pool_does_not_immediately_reinsert_the_item() {
        let all: Vec<_> = (0..5)
            .map(|index| GameIdleItem {
                id: index.to_string(),
                name: index.to_string(),
                icon: None,
                type_name: None,
                occurrence_id: String::new(),
                executable_name: Some(format!("{index}.exe")),
            })
            .collect();
        let mut shared = IdleShared {
            status: GameIdleStatus {
                session_id: "session".into(),
                mode: GameIdleMode::Process,
                selection_mode: GameIdleSelectionMode::Random,
                candidate_type: GameIdleCandidateType::All,
                selected_app_ids: vec![],
                phase: GameIdlePhase::Starting,
                play_minutes: 1,
                rest_minutes: 0,
                play_duration: fixed_duration(1),
                rest_duration: fixed_duration(0),
                current: None,
                recent: vec![],
                upcoming: vec![],
                phase_started_at: 0,
                phase_ends_at: None,
                accumulated_played_seconds: 0,
                warning: None,
            },
            all,
            bag: vec![],
            next_manual_index: 0,
            removed_this_cycle: HashSet::new(),
            upcoming: VecDeque::new(),
            recent: VecDeque::new(),
            running: None,
        };
        shared.refill_bag();
        shared.fill_upcoming();
        let removed_item = shared.upcoming.front().unwrap().clone();
        let removed = removed_item.id.clone();

        assert!(!shared.remove_upcoming_item(&removed, "stale-occurrence"));
        assert!(shared.remove_upcoming_item(&removed, &removed_item.occurrence_id));
        assert!(shared.upcoming.iter().all(|item| item.id != removed));
    }

    #[test]
    fn a_removed_item_is_released_once_the_shuffle_cycle_ends() {
        // A pool larger than `UPCOMING_LENGTH + RECENT_LENGTH` keeps the queue
        // free of duplicates and, importantly, leaves the bag non-empty after
        // the removal, so the exclusion is not cleared by a mid-pass refill.
        let all: Vec<_> = (0..(UPCOMING_LENGTH + RECENT_LENGTH + 2))
            .map(|index| GameIdleItem {
                id: index.to_string(),
                name: index.to_string(),
                icon: None,
                type_name: None,
                occurrence_id: String::new(),
                executable_name: Some(format!("{index}.exe")),
            })
            .collect();
        let mut shared = IdleShared {
            status: GameIdleStatus {
                session_id: "session".into(),
                mode: GameIdleMode::Process,
                selection_mode: GameIdleSelectionMode::Random,
                candidate_type: GameIdleCandidateType::All,
                selected_app_ids: vec![],
                phase: GameIdlePhase::Starting,
                play_minutes: 1,
                rest_minutes: 0,
                play_duration: fixed_duration(1),
                rest_duration: fixed_duration(0),
                current: None,
                recent: vec![],
                upcoming: vec![],
                phase_started_at: 0,
                phase_ends_at: None,
                accumulated_played_seconds: 0,
                warning: None,
            },
            all,
            bag: vec![],
            next_manual_index: 0,
            removed_this_cycle: HashSet::new(),
            upcoming: VecDeque::new(),
            recent: VecDeque::new(),
            running: None,
        };
        shared.refill_bag();
        shared.fill_upcoming();
        let removed_item = shared.upcoming.front().unwrap().clone();
        let removed = removed_item.id.clone();

        assert!(shared.remove_upcoming_item(&removed, &removed_item.occurrence_id));
        assert!(shared.removed_this_cycle.contains(&removed));

        // The exclusion is scoped to the current shuffle cycle. Holding it
        // across bag refills would strand the game for the rest of the session:
        // an excluded game can never enter `upcoming`, so it can neither be
        // played nor released again.
        shared.refill_bag();
        assert!(
            !shared.removed_this_cycle.contains(&removed),
            "a new shuffle cycle must release the removal exclusion"
        );
    }

    #[test]
    fn a_single_candidate_pool_stays_short_instead_of_repeating_adjacent_items() {
        let candidates = eligible_games(GameIdleMode::Cdp, vec![game("only", "win32")]);
        let mut shared = IdleShared {
            status: GameIdleStatus {
                session_id: "session".into(),
                mode: GameIdleMode::Cdp,
                selection_mode: GameIdleSelectionMode::Random,
                candidate_type: GameIdleCandidateType::All,
                selected_app_ids: vec![],
                phase: GameIdlePhase::Starting,
                play_minutes: 1,
                rest_minutes: 0,
                play_duration: fixed_duration(1),
                rest_duration: fixed_duration(0),
                current: None,
                recent: vec![],
                upcoming: vec![],
                phase_started_at: 0,
                phase_ends_at: None,
                accumulated_played_seconds: 0,
                warning: None,
            },
            all: candidates,
            bag: vec![],
            next_manual_index: 0,
            removed_this_cycle: HashSet::new(),
            upcoming: VecDeque::new(),
            recent: VecDeque::new(),
            running: None,
        };
        shared.refill_bag();
        shared.fill_upcoming();

        assert_eq!(shared.upcoming.len(), 1);
        let first = shared.upcoming.pop_front().unwrap();
        shared.status.current = Some(first.clone());
        shared.bag.clear();
        shared.fill_upcoming();

        assert_eq!(shared.upcoming.len(), 1);
        assert_eq!(shared.upcoming.front().unwrap().id, first.id);
        assert_ne!(
            shared.upcoming.front().unwrap().occurrence_id,
            first.occurrence_id
        );
    }

    #[test]
    fn medium_pool_never_duplicates_the_active_queue_across_a_bag_refill() {
        let candidates = eligible_games(
            GameIdleMode::Cdp,
            (0..6)
                .map(|index| game(&index.to_string(), "win32"))
                .collect(),
        );
        let mut shared = IdleShared {
            status: GameIdleStatus {
                session_id: "session".into(),
                mode: GameIdleMode::Cdp,
                selection_mode: GameIdleSelectionMode::Random,
                candidate_type: GameIdleCandidateType::All,
                selected_app_ids: vec![],
                phase: GameIdlePhase::Starting,
                play_minutes: 1,
                rest_minutes: 0,
                play_duration: fixed_duration(1),
                rest_duration: fixed_duration(0),
                current: Some(candidates[0].clone()),
                recent: vec![],
                upcoming: vec![],
                phase_started_at: 0,
                phase_ends_at: None,
                accumulated_played_seconds: 0,
                warning: None,
            },
            all: candidates.clone(),
            // Force a mid-fill cycle boundary after one candidate.
            bag: vec![candidates[1].clone()],
            next_manual_index: 0,
            removed_this_cycle: HashSet::new(),
            upcoming: VecDeque::new(),
            recent: VecDeque::new(),
            running: None,
        };

        shared.fill_upcoming();

        let ids: HashSet<_> = shared
            .upcoming
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids.len(), shared.upcoming.len());
        assert!(shared
            .upcoming
            .iter()
            .all(|item| item.id != shared.status.current.as_ref().unwrap().id));
        assert!(shared
            .upcoming
            .iter()
            .all(|item| !item.occurrence_id.is_empty()));
    }
}

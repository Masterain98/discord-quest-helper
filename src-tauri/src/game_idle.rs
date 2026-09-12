use crate::models::DetectableGame;
use crate::simulation_history::SimulationHistory;
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameIdleConfig {
    pub mode: GameIdleMode,
    pub play_minutes: u32,
    pub rest_minutes: u32,
    pub cdp_port: u16,
    pub simulation_path: String,
}

impl GameIdleConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.play_minutes == 0 {
            return Err("Game simulation duration must be a positive integer".to_string());
        }
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
    pub phase: GameIdlePhase,
    pub play_minutes: u32,
    pub rest_minutes: u32,
    pub current: Option<GameIdleItem>,
    pub recent: Vec<GameIdleItem>,
    pub upcoming: Vec<GameIdleItem>,
    pub phase_started_at: i64,
    pub phase_ends_at: Option<i64>,
    pub accumulated_played_seconds: u64,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
enum RunningSimulation {
    Process { executable_name: String },
    Cdp { port: u16 },
}

#[derive(Debug)]
struct IdleShared {
    status: GameIdleStatus,
    all: Vec<GameIdleItem>,
    bag: Vec<GameIdleItem>,
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
        self.removed_this_cycle.clear();
        self.bag = self.all.clone();
        shuffle(&mut self.bag);
    }

    fn fill_upcoming(&mut self) {
        if self.all.is_empty() {
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
            let already_visible = self
                .status
                .current
                .as_ref()
                .is_some_and(|current| current.id == candidate.id)
                || self.upcoming.iter().any(|item| item.id == candidate.id)
                || self.recent.iter().any(|item| item.id == candidate.id);
            if already_visible && self.all.len() > UPCOMING_LENGTH + RECENT_LENGTH {
                continue;
            }
            if self
                .upcoming
                .back()
                .is_some_and(|previous| previous.id == candidate.id)
            {
                continue;
            }
            self.upcoming.push_back(candidate);
        }
        self.refresh_status_lists();
    }

    fn remove_upcoming_item(&mut self, app_id: &str) -> bool {
        let Some(position) = self.upcoming.iter().position(|item| item.id == app_id) else {
            return false;
        };
        self.upcoming.remove(position);
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

        let candidates = eligible_games(config.mode, games);
        if candidates.is_empty() {
            return Err(match config.mode {
                GameIdleMode::Process => {
                    "No Discord-detectable applications have an executable compatible with this system"
                        .to_string()
                }
                GameIdleMode::Cdp => "Discord returned no detectable games or applications".to_string(),
            });
        }

        let session_id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        let status = GameIdleStatus {
            session_id,
            mode: config.mode,
            phase: GameIdlePhase::Starting,
            play_minutes: config.play_minutes,
            rest_minutes: config.rest_minutes,
            current: None,
            recent: Vec::new(),
            upcoming: Vec::new(),
            phase_started_at: now,
            phase_ends_at: None,
            accumulated_played_seconds: 0,
            warning: None,
        };
        let mut shared_value = IdleShared {
            status,
            all: candidates,
            bag: Vec::new(),
            removed_this_cycle: HashSet::new(),
            upcoming: VecDeque::new(),
            recent: VecDeque::new(),
            running: None,
        };
        shared_value.refill_bag();
        shared_value.fill_upcoming();
        let initial = shared_value.status.clone();
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
        if shared.status.phase == GameIdlePhase::Starting {
            return Err("Wait until the next game has finished starting".to_string());
        }
        if !shared.remove_upcoming_item(app_id) {
            return Err("Only upcoming games can be removed".to_string());
        }
        emit_status(app, &shared.status);
        Ok(shared.status.clone())
    }

    pub async fn stop(
        &self,
        history: Arc<SimulationHistory>,
        history_path: PathBuf,
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

        history.finish(&history_path, Some(app)).await?;
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
                    last_error = error;
                    if attempt == 0 {
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(2)) => {},
                            _ = cancel.changed() => { break; }
                        }
                    }
                }
            }
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
            state.status.phase = GameIdlePhase::Playing;
            state.status.phase_started_at = now_millis();
            state.status.phase_ends_at =
                Some(state.status.phase_started_at + i64::from(config.play_minutes) * 60_000);
            emit_status(&app, &state.status);
        }

        let play_duration = Duration::from_secs(u64::from(config.play_minutes) * 60);
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

        if config.rest_minutes > 0 {
            {
                let mut state = shared.lock().await;
                state.status.phase = GameIdlePhase::Resting;
                state.status.phase_started_at = now_millis();
                state.status.phase_ends_at =
                    Some(state.status.phase_started_at + i64::from(config.rest_minutes) * 60_000);
                emit_status(&app, &state.status);
            }
            let rest_duration = Duration::from_secs(u64::from(config.rest_minutes) * 60);
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
) -> Result<RunningSimulation, String> {
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
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    crate::game_simulator::stop_simulated_game(&executable)
                })
                .await;
                return Err(error);
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
            crate::clear_discord_rpc().await?;
            let executable_name = executable_name.clone();
            tauri::async_runtime::spawn_blocking(move || {
                crate::game_simulator::stop_simulated_game(&executable_name)
            })
            .await
            .map_err(|error| format!("Process cleanup task failed: {error}"))?
            .map_err(|error| error.to_string())
        }
        RunningSimulation::Cdp { port } => crate::cdp_quest::stop_manual_game_spoof(*port)
            .await
            .map_err(|error| error.to_string()),
    }
}

fn eligible_games(mode: GameIdleMode, games: Vec<DetectableGame>) -> Vec<GameIdleItem> {
    let mut seen = HashSet::new();
    games
        .into_iter()
        .filter_map(|game| {
            if !seen.insert(game.id.clone()) {
                return None;
            }
            let executable_name = select_executable(&game);
            if mode == GameIdleMode::Process && executable_name.is_none() {
                return None;
            }
            Some(GameIdleItem {
                id: game.id,
                name: game.name,
                icon: game.icon,
                type_name: game.type_name,
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
    if values.len() < 2 {
        return;
    }
    let mut rng = rand::rng();
    for index in (1..values.len()).rev() {
        let other = rng.random_range(0..=index);
        values.swap(index, other);
    }
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

    #[test]
    fn duration_validation_rejects_zero_play_time() {
        let config = GameIdleConfig {
            mode: GameIdleMode::Cdp,
            play_minutes: 0,
            rest_minutes: 0,
            cdp_port: 9223,
            simulation_path: String::new(),
        };
        assert!(config.validate().is_err());
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
                executable_name: Some(format!("{index}.exe")),
            })
            .collect();
        let mut shared = IdleShared {
            status: GameIdleStatus {
                session_id: "session".into(),
                mode: GameIdleMode::Process,
                phase: GameIdlePhase::Starting,
                play_minutes: 1,
                rest_minutes: 0,
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
                executable_name: Some(format!("{index}.exe")),
            })
            .collect();
        let mut shared = IdleShared {
            status: GameIdleStatus {
                session_id: "session".into(),
                mode: GameIdleMode::Process,
                phase: GameIdlePhase::Starting,
                play_minutes: 1,
                rest_minutes: 0,
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
            removed_this_cycle: HashSet::new(),
            upcoming: VecDeque::new(),
            recent: VecDeque::new(),
            running: None,
        };
        shared.refill_bag();
        shared.fill_upcoming();
        let removed = shared.upcoming.front().unwrap().id.clone();

        assert!(shared.remove_upcoming_item(&removed));
        assert!(shared.upcoming.iter().all(|item| item.id != removed));
    }

    #[test]
    fn a_single_candidate_pool_stays_short_instead_of_repeating_adjacent_items() {
        let candidates = eligible_games(GameIdleMode::Cdp, vec![game("only", "win32")]);
        let mut shared = IdleShared {
            status: GameIdleStatus {
                session_id: "session".into(),
                mode: GameIdleMode::Cdp,
                phase: GameIdlePhase::Starting,
                play_minutes: 1,
                rest_minutes: 0,
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
            removed_this_cycle: HashSet::new(),
            upcoming: VecDeque::new(),
            recent: VecDeque::new(),
            running: None,
        };
        shared.refill_bag();
        shared.fill_upcoming();

        assert_eq!(shared.upcoming.len(), 1);
    }
}

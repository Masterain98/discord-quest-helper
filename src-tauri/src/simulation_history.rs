use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tauri::{Emitter, Manager, Runtime};

const HISTORY_VERSION: u32 = 1;
const HISTORY_FILE: &str = "game-simulation-history.v1.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameSimulationHistoryEntry {
    pub app_id: String,
    pub app_name: String,
    pub total_seconds: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SimulationHistoryStatus {
    pub active: bool,
    pub pending_finish: bool,
    pub app_id: Option<String>,
    pub app_name: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedHistory {
    #[serde(default = "history_version")]
    version: u32,
    #[serde(default)]
    accounts: HashMap<String, HashMap<String, GameSimulationHistoryEntry>>,
}

fn history_version() -> u32 {
    HISTORY_VERSION
}

#[derive(Debug)]
struct ActiveUsage {
    id: u64,
    user_id: String,
    app_id: String,
    app_name: String,
    path: PathBuf,
    checkpoint_at: Instant,
    /// Frozen elapsed interval after native activity has stopped. A failed
    /// final save can then be retried without counting the retry delay.
    pending_finish_seconds: Option<u64>,
}

#[derive(Debug, Default)]
struct HistoryRuntime {
    loaded: bool,
    next_usage_id: u64,
    data: PersistedHistory,
    active: Option<ActiveUsage>,
}

#[derive(Debug, Default)]
pub struct SimulationHistory {
    runtime: tokio::sync::Mutex<HistoryRuntime>,
}

impl SimulationHistory {
    pub fn file_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
        let directory = app
            .path()
            .app_local_data_dir()
            .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
        Ok(directory.join(HISTORY_FILE))
    }

    async fn ensure_loaded(&self, path: &Path) -> Result<(), String> {
        let mut runtime = self.runtime.lock().await;
        if runtime.loaded {
            return Ok(());
        }
        if !path.exists() {
            runtime.data = PersistedHistory {
                version: HISTORY_VERSION,
                ..PersistedHistory::default()
            };
            runtime.loaded = true;
            return Ok(());
        }
        let bytes = std::fs::read(path)
            .map_err(|error| format!("Failed to read game simulation history: {error}"))?;
        runtime.data = match serde_json::from_slice::<PersistedHistory>(&bytes) {
            Ok(data) => data,
            Err(error) => {
                // A truncated or externally modified file must not brick usage
                // tracking. Without this recovery the parse error would repeat
                // on every `ensure_loaded`, so `begin` would fail forever and
                // (now that history is mandatory) block every simulation until
                // the file was deleted by hand. Preserve the unreadable bytes
                // for diagnosis and start from an empty history.
                let quarantine =
                    path.with_extension(format!("json.corrupt-{}", uuid::Uuid::new_v4()));
                std::fs::rename(path, &quarantine).map_err(|rename_error| {
                    format!(
                        "Game simulation history at {} is unreadable ({error}) and could not be preserved at {}: {rename_error}",
                        path.display(),
                        quarantine.display()
                    )
                })?;
                eprintln!(
                    "Game simulation history at {} is unreadable ({error}); moved it to {} and starting with an empty history",
                    path.display(),
                    quarantine.display()
                );
                PersistedHistory::default()
            }
        };
        runtime.data.version = HISTORY_VERSION;
        runtime.loaded = true;
        Ok(())
    }

    fn save_locked(runtime: &HistoryRuntime, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create history directory: {error}"))?;
        }
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&runtime.data)
            .map_err(|error| format!("Failed to serialize game simulation history: {error}"))?;
        {
            let mut file = std::fs::File::create(&temporary)
                .map_err(|error| format!("Failed to write game simulation history: {error}"))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| format!("Failed to flush game simulation history: {error}"))?;
        }
        replace_file(&temporary, path)
    }

    fn add_elapsed(
        runtime: &mut HistoryRuntime,
        seconds: u64,
    ) -> Option<GameSimulationHistoryEntry> {
        if seconds == 0 {
            return None;
        }
        let active = runtime.active.as_ref()?;
        let entry = runtime
            .data
            .accounts
            .entry(active.user_id.clone())
            .or_default()
            .entry(active.app_id.clone())
            .or_insert_with(|| GameSimulationHistoryEntry {
                app_id: active.app_id.clone(),
                app_name: active.app_name.clone(),
                total_seconds: 0,
                updated_at: chrono::Utc::now().to_rfc3339(),
            });
        entry.app_name = active.app_name.clone();
        entry.total_seconds = entry.total_seconds.saturating_add(seconds);
        entry.updated_at = chrono::Utc::now().to_rfc3339();
        Some(entry.clone())
    }

    pub async fn begin<R: Runtime>(
        self: &Arc<Self>,
        path: PathBuf,
        app: tauri::AppHandle<R>,
        user_id: String,
        app_id: String,
        app_name: String,
    ) -> Result<(), String> {
        self.ensure_loaded(&path).await?;
        let usage_id = {
            let mut runtime = self.runtime.lock().await;
            if runtime.active.is_some() {
                return Err("Another application simulation is already being recorded".to_string());
            }
            let entry = runtime
                .data
                .accounts
                .entry(user_id.clone())
                .or_default()
                .entry(app_id.clone())
                .or_insert_with(|| GameSimulationHistoryEntry {
                    app_id: app_id.clone(),
                    app_name: app_name.clone(),
                    total_seconds: 0,
                    updated_at: chrono::Utc::now().to_rfc3339(),
                });
            entry.app_name = app_name.clone();
            entry.updated_at = chrono::Utc::now().to_rfc3339();
            Self::save_locked(&runtime, &path)?;
            runtime.next_usage_id = runtime.next_usage_id.saturating_add(1);
            let usage_id = runtime.next_usage_id;
            runtime.active = Some(ActiveUsage {
                id: usage_id,
                user_id,
                app_id,
                app_name,
                path: path.clone(),
                checkpoint_at: Instant::now(),
                pending_finish_seconds: None,
            });
            usage_id
        };

        let history = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.tick().await;
            loop {
                interval.tick().await;
                match history.checkpoint(usage_id, &path).await {
                    Ok(Some(entry)) => {
                        let _ = app.emit("game-simulation-history-updated", entry);
                    }
                    Ok(None) => break,
                    Err(error) => {
                        // Keep retrying on the next minute. `checkpoint` rolls
                        // back the failed addition and deliberately preserves
                        // its time marker, so a transient filesystem failure is
                        // recovered without losing or double-counting time.
                        eprintln!("Game simulation history checkpoint failed: {error}");
                    }
                }
            }
        });
        Ok(())
    }

    async fn checkpoint(
        &self,
        usage_id: u64,
        path: &Path,
    ) -> Result<Option<GameSimulationHistoryEntry>, String> {
        let mut runtime = self.runtime.lock().await;
        let Some(active) = runtime.active.as_ref() else {
            return Ok(None);
        };
        if active.id != usage_id {
            return Ok(None);
        }
        if active.pending_finish_seconds.is_some() {
            // Native activity has stopped and final persistence is waiting for
            // an explicit retry. The frozen interval must not keep growing.
            return Ok(None);
        }
        let seconds = active.checkpoint_at.elapsed().as_secs();
        if seconds == 0 {
            return Ok(Some(GameSimulationHistoryEntry {
                app_id: active.app_id.clone(),
                app_name: active.app_name.clone(),
                total_seconds: runtime
                    .data
                    .accounts
                    .get(&active.user_id)
                    .and_then(|apps| apps.get(&active.app_id))
                    .map(|entry| entry.total_seconds)
                    .unwrap_or(0),
                updated_at: chrono::Utc::now().to_rfc3339(),
            }));
        }
        // Snapshot the entry before mutating so a failed save can be reverted
        // exactly. See the rollback below for why that matters.
        let previous_entry = runtime
            .data
            .accounts
            .get(&active.user_id)
            .and_then(|apps| apps.get(&active.app_id))
            .cloned();
        let (user_id, app_id) = (active.user_id.clone(), active.app_id.clone());

        // Add the elapsed interval and persist it *before* advancing the
        // checkpoint marker. If persistence fails, `checkpoint_at` still points
        // at the start of the unsaved interval, so the same duration is retried
        // (or captured by `finish`) instead of being silently dropped.
        let entry = Self::add_elapsed(&mut runtime, seconds);
        if let Err(error) = Self::save_locked(&runtime, path) {
            // Revert the in-memory addition. `checkpoint_at` is deliberately not
            // advanced, so `finish` will measure this same interval again;
            // leaving the total inflated here would double-count it.
            if let Some(apps) = runtime.data.accounts.get_mut(&user_id) {
                match previous_entry {
                    Some(previous) => {
                        apps.insert(app_id, previous);
                    }
                    None => {
                        apps.remove(&app_id);
                    }
                }
            }
            return Err(error);
        }
        if let Some(active) = runtime.active.as_mut() {
            active.checkpoint_at = Instant::now();
        }
        Ok(entry)
    }

    pub async fn finish<R: Runtime>(
        &self,
        path: &Path,
        app: Option<&tauri::AppHandle<R>>,
    ) -> Result<Option<GameSimulationHistoryEntry>, String> {
        self.ensure_loaded(path).await?;
        let mut runtime = self.runtime.lock().await;
        let Some(active) = runtime.active.as_ref() else {
            return Ok(None);
        };
        let seconds = active
            .pending_finish_seconds
            .unwrap_or_else(|| active.checkpoint_at.elapsed().as_secs());
        let (user_id, app_id) = (active.user_id.clone(), active.app_id.clone());
        let previous_entry = runtime
            .data
            .accounts
            .get(&user_id)
            .and_then(|apps| apps.get(&app_id))
            .cloned();
        if let Some(active) = runtime.active.as_mut() {
            active.pending_finish_seconds = Some(seconds);
        }
        let entry = Self::add_elapsed(&mut runtime, seconds);
        if let Err(error) = Self::save_locked(&runtime, path) {
            if let Some(apps) = runtime.data.accounts.get_mut(&user_id) {
                match previous_entry {
                    Some(previous) => {
                        apps.insert(app_id, previous);
                    }
                    None => {
                        apps.remove(&app_id);
                    }
                }
            }
            return Err(error);
        }
        // Only release the segment after its final elapsed interval is durable.
        // A failed save deliberately leaves it active so Stop can be retried.
        runtime.active = None;
        if let (Some(app), Some(entry)) = (app, entry.as_ref()) {
            let _ = app.emit("game-simulation-history-updated", entry.clone());
        }
        Ok(entry)
    }

    /// Finish the current segment using the path captured when it began.
    /// Cleanup must not depend on resolving the application data directory a
    /// second time after the native simulation has already started.
    pub async fn finish_active<R: Runtime>(
        &self,
        app: Option<&tauri::AppHandle<R>>,
    ) -> Result<Option<GameSimulationHistoryEntry>, String> {
        let path = {
            let runtime = self.runtime.lock().await;
            runtime.active.as_ref().map(|active| active.path.clone())
        };
        match path {
            Some(path) => self.finish(&path, app).await,
            None => Ok(None),
        }
    }

    pub async fn entries_for_user(
        &self,
        path: &Path,
        user_id: &str,
    ) -> Result<Vec<GameSimulationHistoryEntry>, String> {
        self.ensure_loaded(path).await?;
        let runtime = self.runtime.lock().await;
        let mut entries: Vec<_> = runtime
            .data
            .accounts
            .get(user_id)
            .map(|apps| apps.values().cloned().collect())
            .unwrap_or_default();
        entries.sort_by(|left, right| left.app_name.cmp(&right.app_name));
        Ok(entries)
    }

    pub async fn status(&self) -> SimulationHistoryStatus {
        let runtime = self.runtime.lock().await;
        let Some(active) = runtime.active.as_ref() else {
            return SimulationHistoryStatus {
                active: false,
                pending_finish: false,
                app_id: None,
                app_name: None,
            };
        };
        SimulationHistoryStatus {
            active: true,
            pending_finish: active.pending_finish_seconds.is_some(),
            app_id: Some(active.app_id.clone()),
            app_name: Some(active.app_name.clone()),
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file(temporary: &Path, target: &Path) -> Result<(), String> {
    std::fs::rename(temporary, target)
        .map_err(|error| format!("Failed to commit game simulation history: {error}"))
}

#[cfg(target_os = "windows")]
fn replace_file(temporary: &Path, target: &Path) -> Result<(), String> {
    if !target.exists() {
        return std::fs::rename(temporary, target)
            .map_err(|error| format!("Failed to commit game simulation history: {error}"));
    }

    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACE_FILE_FLAGS};

    let target_wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let temporary_wide: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        ReplaceFileW(
            PCWSTR(target_wide.as_ptr()),
            PCWSTR(temporary_wide.as_ptr()),
            PCWSTR::null(),
            REPLACE_FILE_FLAGS(0),
            None,
            None,
        )
    }
    .map_err(|error| format!("Failed to atomically replace game simulation history: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_time_is_isolated_by_account_and_app() {
        let mut runtime = HistoryRuntime {
            active: Some(ActiveUsage {
                id: 1,
                user_id: "account-a".into(),
                app_id: "app-a".into(),
                app_name: "Game A".into(),
                path: PathBuf::new(),
                checkpoint_at: Instant::now(),
                pending_finish_seconds: None,
            }),
            ..HistoryRuntime::default()
        };
        SimulationHistory::add_elapsed(&mut runtime, 65);
        runtime.active = Some(ActiveUsage {
            id: 2,
            user_id: "account-b".into(),
            app_id: "app-a".into(),
            app_name: "Game A".into(),
            path: PathBuf::new(),
            checkpoint_at: Instant::now(),
            pending_finish_seconds: None,
        });
        SimulationHistory::add_elapsed(&mut runtime, 30);
        assert_eq!(
            runtime.data.accounts["account-a"]["app-a"].total_seconds,
            65
        );
        assert_eq!(
            runtime.data.accounts["account-b"]["app-a"].total_seconds,
            30
        );
    }

    #[tokio::test]
    async fn persisted_history_is_recovered_by_a_new_store() {
        let path =
            std::env::temp_dir().join(format!("dqh-game-history-{}.json", uuid::Uuid::new_v4()));
        let first = SimulationHistory::default();
        first.ensure_loaded(&path).await.unwrap();
        {
            let mut runtime = first.runtime.lock().await;
            runtime.active = Some(ActiveUsage {
                id: 1,
                user_id: "account-a".into(),
                app_id: "app-a".into(),
                app_name: "Game A".into(),
                path: path.clone(),
                checkpoint_at: Instant::now(),
                pending_finish_seconds: None,
            });
            SimulationHistory::add_elapsed(&mut runtime, 125);
            // `runtime` is a MutexGuard here, not the struct itself, so this is
            // a field write on the guard's target rather than a
            // `field_reassign_with_default` pattern.
            runtime.active = None;
            SimulationHistory::save_locked(&runtime, &path).unwrap();
        }

        let recovered = SimulationHistory::default();
        recovered.ensure_loaded(&path).await.unwrap();
        let runtime = recovered.runtime.lock().await;
        assert_eq!(
            runtime.data.accounts["account-a"]["app-a"].total_seconds,
            125
        );
        drop(runtime);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.tmp"));
    }

    #[tokio::test]
    async fn corrupt_history_is_quarantined_and_replaced_with_an_empty_one() {
        let path =
            std::env::temp_dir().join(format!("dqh-corrupt-history-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"{ this is not valid json").unwrap();

        let history = SimulationHistory::default();
        history
            .ensure_loaded(&path)
            .await
            .expect("a corrupt file must not make history loading fail permanently");

        {
            let runtime = history.runtime.lock().await;
            assert!(runtime.loaded);
            assert_eq!(runtime.data.version, HISTORY_VERSION);
            assert!(runtime.data.accounts.is_empty());
        }

        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        let quarantine = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|candidate| {
                candidate.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with(&format!("{file_name}.corrupt-"))
                })
            })
            .expect("the unreadable file should be preserved under a unique .corrupt name");
        assert!(
            !path.exists() && quarantine.exists(),
            "the unreadable file should be preserved under a unique .corrupt name"
        );

        let _ = std::fs::remove_file(&quarantine);
    }

    #[tokio::test]
    async fn failed_checkpoint_reverts_the_unpersisted_interval() {
        // A regular file used as a parent directory makes `create_dir_all`
        // (and therefore `save_locked`) fail deterministically.
        let blocker =
            std::env::temp_dir().join(format!("dqh-ckpt-blocker-{}", uuid::Uuid::new_v4()));
        std::fs::write(&blocker, b"not a directory").unwrap();
        let path = blocker.join("history.json");

        let history = SimulationHistory::default();
        {
            let mut runtime = history.runtime.lock().await;
            runtime.active = Some(ActiveUsage {
                id: 1,
                user_id: "account-a".into(),
                app_id: "app-a".into(),
                app_name: "Game A".into(),
                path: path.clone(),
                checkpoint_at: Instant::now() - std::time::Duration::from_secs(5),
                pending_finish_seconds: None,
            });
        }

        assert!(
            history.checkpoint(1, &path).await.is_err(),
            "checkpoint must report the persistence failure"
        );

        let runtime = history.runtime.lock().await;
        assert!(
            runtime
                .data
                .accounts
                .get("account-a")
                .and_then(|apps| apps.get("app-a"))
                .is_none(),
            "a failed checkpoint must not leave the interval counted in memory"
        );
        let active = runtime
            .active
            .as_ref()
            .expect("the active usage is still tracked");
        assert!(
            active.checkpoint_at.elapsed() >= std::time::Duration::from_secs(5),
            "the checkpoint marker must not advance, so `finish` still captures the interval"
        );
        drop(runtime);

        let _ = std::fs::remove_file(&blocker);
    }

    #[tokio::test]
    async fn failed_finish_preserves_the_active_interval_for_retry() {
        let blocker =
            std::env::temp_dir().join(format!("dqh-finish-blocker-{}", uuid::Uuid::new_v4()));
        std::fs::write(&blocker, b"not a directory").unwrap();
        let path = blocker.join("history.json");

        let history = SimulationHistory::default();
        {
            let mut runtime = history.runtime.lock().await;
            runtime.active = Some(ActiveUsage {
                id: 1,
                user_id: "account-a".into(),
                app_id: "app-a".into(),
                app_name: "Game A".into(),
                path: path.clone(),
                checkpoint_at: Instant::now() - std::time::Duration::from_secs(5),
                pending_finish_seconds: None,
            });
        }

        assert!(history.finish::<tauri::Wry>(&path, None).await.is_err());

        let runtime = history.runtime.lock().await;
        let active = runtime
            .active
            .as_ref()
            .expect("the final interval must remain retryable");
        assert_eq!(active.pending_finish_seconds, Some(5));
        assert!(
            runtime
                .data
                .accounts
                .get("account-a")
                .and_then(|apps| apps.get("app-a"))
                .is_none(),
            "a failed final save must not remain counted only in memory"
        );
        drop(runtime);

        let _ = std::fs::remove_file(&blocker);
    }
}

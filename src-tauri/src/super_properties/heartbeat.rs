use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const EXPIRES_MS: u64 = 30 * 60 * 1000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub uuid: String,
    pub created_at_timestamp: u64,
    pub last_used_timestamp: u64,
    pub version: u8,
}

impl Session {
    pub fn valid(&self, now: u64) -> bool {
        self.version == 1
            && uuid::Uuid::parse_str(&self.uuid).is_ok()
            && self.created_at_timestamp <= self.last_used_timestamp
            && self.last_used_timestamp <= now
            && now - self.last_used_timestamp < EXPIRES_MS
    }
}

#[derive(Clone, Default)]
pub struct Heartbeat {
    pub session: Option<Session>,
    pub path: Option<PathBuf>,
    pub credential: Option<String>,
    last_save: Option<u64>,
    pub storage_error: bool,
    pub defer_writes: bool,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    credential: String,
    session: Session,
}

impl Heartbeat {
    pub fn bind(&mut self, credential: String, now: u64) {
        if self.credential.as_ref() == Some(&credential) {
            return;
        }
        self.session = self
            .path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
            .filter(|record| record.credential == credential && record.session.valid(now))
            .map(|record| record.session);
        self.credential = Some(credential);
        self.last_save = None;
    }

    pub fn id(&self, now: u64) -> Option<String> {
        self.session
            .as_ref()
            .filter(|session| session.valid(now))
            .map(|session| session.uuid.clone())
    }

    // Only the activity lifecycle creates/renews sessions. The header getter is read-only.
    pub fn update(&mut self, active: bool, now: u64) {
        if !active || self.credential.is_none() {
            return;
        }
        if self.id(now).is_none() {
            self.session = Some(Session {
                uuid: uuid::Uuid::new_v4().to_string(),
                created_at_timestamp: now,
                last_used_timestamp: now,
                version: 1,
            });
        }
        self.session.as_mut().unwrap().last_used_timestamp = now;
        self.persist(now);
    }

    fn persist(&mut self, now: u64) {
        if self.defer_writes
            || self
                .last_save
                .is_some_and(|last| now.saturating_sub(last) < 1000)
        {
            return;
        }
        if let Some(path) = &self.path {
            let record = Stored {
                credential: self.credential.clone().unwrap(),
                session: self.session.clone().unwrap(),
            };
            self.storage_error = save(path, &record).is_err();
        }
        self.last_save = Some(now);
    }

    pub fn commit(&mut self, now: u64) {
        self.defer_writes = false;
        self.last_save = None;
        if self.id(now).is_some() {
            self.persist(now);
            return;
        }
        // A changed credential invalidates the old persisted record even when
        // the newly committed client is currently unfocused.
        if let Some(path) = &self.path {
            let previous = std::fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok());
            if previous.is_some_and(|record| Some(&record.credential) != self.credential.as_ref()) {
                self.storage_error = std::fs::remove_file(path).is_err();
            }
        }
    }

    pub fn clear(&mut self) -> std::io::Result<()> {
        if let Some(path) = &self.path {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    self.storage_error = true;
                    return Err(error);
                }
            }
        }
        self.session = None;
        self.credential = None;
        self.last_save = None;
        self.storage_error = false;
        Ok(())
    }
}

fn save(path: &Path, record: &Stored) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(path.parent().ok_or(std::io::ErrorKind::InvalidInput)?)?;
    let temporary = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(&serde_json::to_vec(record)?)?;
    file.sync_all()?;
    drop(file);
    // std::fs::rename replaces an existing file on Unix. MoveFileExW provides
    // the corresponding atomic replacement on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                windows::core::PCWSTR(from.as_ptr()),
                windows::core::PCWSTR(to.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(std::io::Error::other)?;
    }
    #[cfg(not(windows))]
    std::fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activity_expiry_and_read_only_getter() {
        let mut state = Heartbeat::default();
        state.bind("synthetic-credential".into(), 1);
        assert!(state.id(1).is_none());
        state.update(true, 1000);
        let first = state.id(1000).unwrap();
        state.update(false, 1000 + EXPIRES_MS - 1);
        assert_eq!(state.id(1000 + EXPIRES_MS - 1), Some(first.clone()));
        assert!(state.id(1000 + EXPIRES_MS).is_none());
        state.update(true, 1000 + EXPIRES_MS);
        assert_ne!(state.id(1000 + EXPIRES_MS), Some(first));
        state.clear().unwrap();
        assert!(state.id(1000 + EXPIRES_MS).is_none());
    }
    #[test]
    fn persistence_credential_changes_and_invalid_version() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let mut state = Heartbeat {
            path: Some(dir.join("heartbeat.json")),
            ..Default::default()
        };
        state.bind("credential-a".into(), 1000);
        state.update(true, 1000);
        let mut restored = Heartbeat {
            path: state.path.clone(),
            ..Default::default()
        };
        restored.bind("credential-a".into(), 2000);
        assert_eq!(restored.id(2000), state.id(2000));
        restored.bind("credential-b".into(), 2000);
        assert!(restored.id(2000).is_none());
        state.session.as_mut().unwrap().version = 2;
        assert!(state.id(2000).is_none());
        restored.clear().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn commit_persists_deferred_session_without_renewing_activity() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let path = dir.join("heartbeat.json");
        let mut state = Heartbeat {
            path: Some(path.clone()),
            defer_writes: true,
            ..Default::default()
        };
        state.bind("credential".into(), 1000);
        state.update(true, 1000);
        assert!(!path.exists());
        state.update(false, 2000);
        state.commit(2000);
        let stored: Stored = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored.session.last_used_timestamp, 1000);
        assert_eq!(Some(stored.session.uuid), state.id(2000));
        let mut restored = Heartbeat {
            path: Some(path),
            ..Default::default()
        };
        restored.bind("credential".into(), 2000);
        assert_eq!(restored.id(2000), state.id(2000));
        state.clear().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_clear_preserves_session_and_reports_storage_error() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let mut state = Heartbeat::default();
        state.bind("credential".into(), 1000);
        state.update(true, 1000);
        let id = state.id(1000);
        state.path = Some(dir.clone());
        assert!(state.clear().is_err());
        assert!(state.storage_error);
        assert_eq!(state.id(1000), id);
        assert_eq!(state.credential.as_deref(), Some("credential"));
        std::fs::remove_dir_all(dir).unwrap();
        state.clear().unwrap();
        assert!(state.id(1000).is_none());
    }
}

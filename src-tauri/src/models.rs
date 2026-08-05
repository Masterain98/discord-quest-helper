use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordUser {
    pub id: String,
    pub username: String,
    pub discriminator: String,
    pub avatar: Option<String>,
    pub global_name: Option<String>,
    /// Nitro subscription type: 0=None, 1=Nitro Classic, 2=Nitro, 3=Nitro Basic
    #[serde(default)]
    pub premium_type: Option<u8>,
}

/// Simplified Quest model for frontend display
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Quest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub progress: f64,
    pub seconds_needed: u32,
    pub task_type: String,
    pub application_id: String,
    pub application_name: String,
    pub application_icon: Option<String>,
    pub expires_at: Option<String>,
    pub enrolled: bool,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectableGame {
    pub id: String,
    pub name: String,
    pub executables: Vec<GameExecutable>,
    #[serde(alias = "icon_hash")]
    pub icon: Option<String>,
    pub type_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameExecutable {
    pub name: String,
    pub os: String,
}

// Discord API response types (legacy, kept for reference)
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct QuestsResponse {
    pub quests: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct VideoProgressPayload {
    pub timestamp: u64,
}

#[derive(Debug, Serialize)]
pub struct HeartbeatPayload {
    pub stream_key: String,
}

#[derive(Debug, Serialize)]
pub struct GameHeartbeatPayload {
    pub application_id: String,
    pub terminal: bool,
}

#[derive(Debug, Serialize)]
pub struct PlayActivityHeartbeatPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    pub terminal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayActivityHeartbeatStatus {
    pub progress_seconds: f64,
    pub completed: bool,
}

impl PlayActivityHeartbeatStatus {
    pub fn progress_percentage(self, target_seconds: u32) -> f64 {
        if self.completed {
            return 100.0;
        }
        if target_seconds == 0 {
            return 0.0;
        }
        (self.progress_seconds / target_seconds as f64 * 100.0).clamp(0.0, 99.0)
    }

    pub fn reached_target(self, target_seconds: u32) -> bool {
        self.completed || self.progress_seconds >= target_seconds as f64
    }
}

// Internal state
pub struct QuestState {
    #[allow(dead_code)]
    pub quest_id: String,
    pub cancel_flag: tokio::sync::mpsc::Sender<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedAccount {
    pub token: String,
    pub user: DiscordUser,
}

/// Machine-readable authentication progress sent over a command-scoped IPC
/// channel. Deliberately contains no token, user ID, path, or error detail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthProgress {
    pub phase: AuthProgressPhase,
    pub current: Option<usize>,
    pub total: Option<usize>,
    pub valid_accounts: Option<usize>,
}

impl AuthProgress {
    pub fn phase(phase: AuthProgressPhase) -> Self {
        Self {
            phase,
            current: None,
            total: None,
            valid_accounts: None,
        }
    }

    pub fn validating(current: usize, total: usize) -> Self {
        Self {
            phase: AuthProgressPhase::ValidatingTokens,
            current: Some(current),
            total: Some(total),
            valid_accounts: None,
        }
    }

    pub fn accounts_found(total: usize, valid_accounts: usize) -> Self {
        Self {
            phase: AuthProgressPhase::AccountsFound,
            current: Some(total),
            total: Some(total),
            valid_accounts: Some(valid_accounts),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthProgressPhase {
    ExtractingTokens,
    ValidatingTokens,
    AccountsFound,
    ValidatingToken,
    CapturingCdpSession,
    ValidatingCdpSession,
    PreparingSession,
    SyncingClientInfo,
    Complete,
}

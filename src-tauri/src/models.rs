use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordUser {
    pub id: String,
    pub username: String,
    pub discriminator: String,
    pub avatar: Option<String>,
    pub global_name: Option<String>,
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
    pub timestamp: f64,
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

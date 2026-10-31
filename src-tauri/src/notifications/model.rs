use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    QuestStarted,
    QuestCompleted,
    QueueCompleted,
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct Events {
    pub quest_started: bool,
    pub quest_completed: bool,
    pub queue_completed: bool,
}
impl Default for Events {
    fn default() -> Self {
        Self {
            quest_started: false,
            quest_completed: true,
            queue_completed: true,
        }
    }
}
impl Events {
    pub(super) fn allows(&self, kind: EventKind) -> bool {
        match kind {
            EventKind::QuestStarted => self.quest_started,
            EventKind::QuestCompleted => self.quest_completed,
            EventKind::QueueCompleted => self.queue_completed,
        }
    }
}

#[derive(Clone, Deserialize)]
pub struct NotificationEvent {
    pub id: String,
    pub kind: EventKind,
    #[serde(default)]
    pub quest_name: String,
    pub locale: String,
    #[serde(default)]
    pub reward_image: Option<String>,
    #[serde(default)]
    pub estimated_complete_at: Option<String>,
    #[serde(default)]
    pub succeeded: u32,
    #[serde(default)]
    pub failed: u32,
    #[serde(default)]
    pub skipped: u32,
}
pub(super) fn messages(locale: &str) -> serde_json::Value {
    let json = match locale {
        "zh" => include_str!("../../../src/locales/zh.json"),
        "zh-TW" => include_str!("../../../src/locales/zh-TW.json"),
        "de" => include_str!("../../../src/locales/de.json"),
        "es" => include_str!("../../../src/locales/es.json"),
        "fr" => include_str!("../../../src/locales/fr.json"),
        "id" => include_str!("../../../src/locales/id.json"),
        "ja" => include_str!("../../../src/locales/ja.json"),
        "ko" => include_str!("../../../src/locales/ko.json"),
        "pl" => include_str!("../../../src/locales/pl.json"),
        "pt-BR" => include_str!("../../../src/locales/pt-BR.json"),
        "pt-PT" => include_str!("../../../src/locales/pt-PT.json"),
        "ru" => include_str!("../../../src/locales/ru.json"),
        "th" => include_str!("../../../src/locales/th.json"),
        "tr" => include_str!("../../../src/locales/tr.json"),
        "vi" => include_str!("../../../src/locales/vi.json"),
        _ => include_str!("../../../src/locales/en.json"),
    };
    serde_json::from_str::<serde_json::Value>(json).unwrap_or_default()["notifications"].clone()
}

impl NotificationEvent {
    pub(super) fn sample(locale: String) -> Self {
        Self {
            id: String::new(),
            kind: EventKind::QuestCompleted,
            locale,
            quest_name: String::new(),
            reward_image: None,
            estimated_complete_at: None,
            succeeded: 0,
            failed: 0,
            skipped: 0,
        }
    }
}

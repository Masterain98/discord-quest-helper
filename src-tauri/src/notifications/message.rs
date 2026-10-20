use super::model::{messages, EventKind, NotificationEvent};
use chrono::{DateTime, Local, Utc};

#[derive(Clone)]
pub struct Message {
    pub title: String,
    pub detail: String,
    pub reward_image: Option<String>,
    pub estimated_at: Option<DateTime<Utc>>,
    pub timestamp: DateTime<Utc>,
    pub is_test: bool,
    generic_test: bool,
    pub kind: EventKind,
    labels: serde_json::Value,
    pub counts: [u32; 3],
}

pub(super) fn reward_source(raw: &str) -> Option<String> {
    if raw == "/icons/orbs.png" {
        return Some(raw.into());
    }
    let url = url::Url::parse(raw).ok()?;
    (url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("cdn.discordapp.com" | "media.discordapp.net")
        )
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none())
    .then(|| url.into())
}

pub(super) fn render(event: &NotificationEvent, generic_test: bool) -> Message {
    let labels = messages(&event.locale);
    let key = if generic_test {
        "message_test"
    } else {
        match event.kind {
            EventKind::QuestStarted => "message_started",
            EventKind::QuestCompleted => "message_completed",
            EventKind::QueueCompleted => "message_queue",
        }
    };
    Message {
        title: labels[key]
            .as_str()
            .unwrap_or("Discord Quest Helper")
            .into(),
        detail: if generic_test {
            labels["test_body"]
                .as_str()
                .unwrap_or("Notification test")
                .into()
        } else if event.kind == EventKind::QueueCompleted {
            String::new()
        } else {
            event
                .quest_name
                .chars()
                .filter(|c| !c.is_control())
                .take(300)
                .collect()
        },
        reward_image: if generic_test || event.kind == EventKind::QueueCompleted {
            None
        } else {
            event.reward_image.as_deref().and_then(reward_source)
        },
        estimated_at: if !generic_test && event.kind == EventKind::QuestStarted {
            event
                .estimated_complete_at
                .as_deref()
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .map(|date| date.with_timezone(&Utc))
        } else {
            None
        },
        timestamp: Utc::now(),
        is_test: generic_test,
        generic_test,
        kind: event.kind,
        counts: [event.succeeded, event.failed, event.skipped],
        labels,
    }
}
impl Message {
    fn label(&self, key: &str) -> &str {
        self.labels[key].as_str().unwrap_or("")
    }
    pub fn fields(&self) -> Vec<(String, String)> {
        if self.kind == EventKind::QueueCompleted && !self.generic_test {
            ["succeeded", "failed", "skipped"]
                .into_iter()
                .zip(self.counts)
                .map(|(key, n)| (self.label(key).into(), n.to_string()))
                .collect()
        } else if let Some(date) = self.estimated_at {
            vec![(
                self.label("estimated_completion").into(),
                date.with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M:%S %:z")
                    .to_string(),
            )]
        } else {
            Vec::new()
        }
    }
    pub fn footer(&self) -> String {
        if self.is_test {
            format!("Discord Quest Helper · {}", self.label("message_test"))
        } else {
            "Discord Quest Helper".into()
        }
    }
    pub fn plain_body(&self) -> String {
        let mut lines = Vec::new();
        if !self.detail.is_empty() {
            lines.push(self.detail.clone());
        }
        lines.extend(
            self.fields()
                .into_iter()
                .map(|(label, value)| format!("{label}: {value}")),
        );
        if self.is_test {
            lines.push(self.label("message_test").into());
        }
        lines.push(
            self.timestamp
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S %:z")
                .to_string(),
        );
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::event;
    use super::*;
    #[test]
    fn test_notifications_use_requested_translations_for_every_message_kind() {
        let english = render(&NotificationEvent::sample("en".into()), true);
        for locale in [
            "en", "zh", "zh-TW", "de", "es", "fr", "id", "ja", "ko", "pl", "pt-BR", "pt-PT", "ru",
            "th", "tr", "vi",
        ] {
            let labels = messages(locale);
            let generic = render(&NotificationEvent::sample(locale.into()), true);
            assert_eq!(generic.title, labels["message_test"].as_str().unwrap());
            assert_eq!(generic.detail, labels["test_body"].as_str().unwrap());
            assert!(generic
                .plain_body()
                .starts_with(labels["test_body"].as_str().unwrap()));
            assert!(generic
                .footer()
                .ends_with(labels["message_test"].as_str().unwrap()));
            for (kind, title_key) in [
                (EventKind::QuestStarted, "message_started"),
                (EventKind::QuestCompleted, "message_completed"),
                (EventKind::QueueCompleted, "message_queue"),
            ] {
                let mut event = event(kind);
                event.locale = locale.into();
                event.quest_name = labels["sample_quest"].as_str().unwrap().into();
                let mut preview = render(&event, false);
                preview.is_test = true;
                assert_eq!(preview.title, labels[title_key].as_str().unwrap());
                assert!(preview
                    .plain_body()
                    .contains(labels["message_test"].as_str().unwrap()));
                if kind != EventKind::QueueCompleted {
                    assert_eq!(preview.detail, labels["sample_quest"].as_str().unwrap());
                }
            }
        }
        for locale in ["zh", "zh-TW", "ja"] {
            let localized = render(&NotificationEvent::sample(locale.into()), true);
            assert_ne!(localized.title, english.title);
            assert_ne!(localized.detail, english.detail);
        }
        let fallback = render(&NotificationEvent::sample("unsupported".into()), true);
        assert_eq!(fallback.title, english.title);
        assert_eq!(fallback.detail, english.detail);
    }

    #[test]
    fn artwork_estimate_and_plain_text_preserve_event_semantics() {
        let start = render(&event(EventKind::QuestStarted), false);
        assert!(start.plain_body().contains("Estimated completion"));
        let done = render(&event(EventKind::QuestCompleted), false);
        assert!(done.estimated_at.is_none());
        assert!(done.reward_image.is_some());
        let mut queue = render(&event(EventKind::QueueCompleted), false);
        queue.is_test = true;
        assert!(queue.reward_image.is_none());
        assert!(queue.estimated_at.is_none());
        assert!(queue.plain_body().contains("Notification test"));
        for source in [
            "http://cdn.discordapp.com/a.png",
            "https://cdn.discordapp.com.evil.test/a.png",
            "file:///C:/secret.png",
            "https://token@cdn.discordapp.com/a.png",
        ] {
            assert!(reward_source(source).is_none());
        }
        let mut invalid = event(EventKind::QuestStarted);
        invalid.estimated_complete_at = Some("invalid".into());
        invalid.reward_image = Some("http://127.0.0.1/private.png".into());
        let message = render(&invalid, false);
        assert!(message.reward_image.is_none());
        assert!(message.estimated_at.is_none());
        assert!(!message.plain_body().is_empty());
    }
}

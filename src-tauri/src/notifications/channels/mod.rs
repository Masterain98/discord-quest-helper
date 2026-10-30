//! The only production registration point for built-in notification channels.
mod custom_webhook;
mod discord;
mod system;
mod telegram;

use super::{
    channel::{NotificationChannel, Registry},
    error::Result,
    http,
};
use std::sync::Arc;

pub(super) fn builtin(app: tauri::AppHandle) -> Result<Registry> {
    with_system(Arc::new(system::SystemChannel::new(app)))
}

fn with_system(system: Arc<dyn NotificationChannel>) -> Result<Registry> {
    let client = http::Client::default();
    Registry::new(vec![
        system,
        Arc::new(telegram::TelegramChannel::new(client.clone())),
        Arc::new(discord::DiscordChannel::new(client.clone())),
        Arc::new(custom_webhook::CustomWebhookChannel::new(client)),
    ])
}

#[cfg(test)]
pub(super) fn headless() -> Registry {
    with_system(Arc::new(super::testing::FakeChannel::system())).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::{
        channel::Draft,
        config::{ChannelConfig, Config},
        model::EventKind,
        secrets::Secrets,
        service::NotificationService,
        testing::{event, MemorySecrets},
    };
    #[tokio::test]
    async fn unavailable_http_client_does_not_prevent_other_channels_from_delivering() {
        let registry = Registry::new(vec![
            Arc::new(super::super::testing::FakeChannel::system()),
            Arc::new(telegram::TelegramChannel::new(http::Client::unavailable())),
        ])
        .unwrap();
        let secrets = Arc::new(MemorySecrets::default());
        secrets
            .set("fixture-reference", "123456:dummy-token")
            .unwrap();
        let mut cfg = Config::defaults(&registry);
        cfg.channels.insert(
            "telegram".into(),
            ChannelConfig {
                enabled: true,
                values: [("chat_id".into(), "123456".into())].into(),
                secret_refs: [("token".into(), "fixture-reference".into())].into(),
            },
        );
        let root =
            std::env::temp_dir().join(format!("dqh-unavailable-http-{}", uuid::Uuid::new_v4()));
        let path = root.join("notifications.json");
        super::super::atomic::save(&path, &cfg).unwrap();
        let service = Arc::new(NotificationService::new(
            registry,
            secrets,
            Arc::new(|_| {}),
        ));
        let pending = service
            .dispatch(path.clone(), event(EventKind::QuestCompleted))
            .await
            .unwrap();
        let results: Vec<_> = futures_util::future::join_all(pending)
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();
        assert!(results.iter().any(|r| r.channel == "system" && r.success));
        assert!(results
            .iter()
            .any(|r| r.channel == "telegram" && r.code == "network" && !r.success));
        assert!(
            service
                .test(path, "system".into(), Draft::default(), "en".into(), None)
                .await
                .unwrap()
                .success
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

//! Notification commands. Channel implementations stay behind the registry interface.
mod artwork;
mod atomic;
mod channel;
mod channels;
mod config;
mod error;
mod http;
mod message;
mod model;
mod secrets;
mod service;
#[cfg(test)]
mod testing;

use channel::Draft;
use error::{Code, Result};
use model::{Events, NotificationEvent};
use secrets::NativeSecrets;
pub use service::NotificationService;
use service::{DeliveryResult, Settings};
use std::{path::PathBuf, sync::Arc};
use tauri::{Emitter, Manager, State};

pub fn initialize(app: tauri::AppHandle) -> Result<NotificationService> {
    let registry = channels::builtin(app.clone())?;
    Ok(NotificationService::new(
        registry,
        Arc::new(NativeSecrets),
        Arc::new(move |result| {
            let _ = app.emit("notification-delivery", result);
        }),
    ))
}
fn file_path(app: &tauri::AppHandle) -> Result<PathBuf> {
    app.path()
        .app_local_data_dir()
        .map(|dir| dir.join("notifications.json"))
        .map_err(|_| Code::Storage.into())
}
#[tauri::command]
pub async fn get_notification_settings(
    app: tauri::AppHandle,
    state: State<'_, Arc<NotificationService>>,
) -> Result<Settings> {
    state.settings(file_path(&app)?).await
}
#[tauri::command]
pub async fn save_notification_events(
    app: tauri::AppHandle,
    state: State<'_, Arc<NotificationService>>,
    events: Events,
) -> Result<Settings> {
    state
        .transaction(file_path(&app)?, move |cfg, path, _, _| {
            cfg.events = events;
            atomic::save(path, cfg)
        })
        .await
}
#[tauri::command]
pub async fn set_notification_channel_enabled(
    app: tauri::AppHandle,
    state: State<'_, Arc<NotificationService>>,
    channel: String,
    enabled: bool,
) -> Result<Settings> {
    state.set_enabled(file_path(&app)?, channel, enabled).await
}
#[tauri::command]
pub async fn save_notification_channel(
    app: tauri::AppHandle,
    state: State<'_, Arc<NotificationService>>,
    channel: String,
    draft: Draft,
) -> Result<Settings> {
    state
        .transaction(file_path(&app)?, move |cfg, path, registry, secrets| {
            config::update(cfg, path, registry.get(&channel)?.as_ref(), &draft, secrets)
        })
        .await
}
#[tauri::command]
pub async fn clear_notification_channel(
    app: tauri::AppHandle,
    state: State<'_, Arc<NotificationService>>,
    channel: String,
) -> Result<Settings> {
    state
        .transaction(file_path(&app)?, move |cfg, path, registry, secrets| {
            registry.get(&channel)?;
            config::clear(cfg, path, &channel, secrets)
        })
        .await
}
#[tauri::command]
pub async fn test_notification_channel(
    app: tauri::AppHandle,
    state: State<'_, Arc<NotificationService>>,
    channel: String,
    draft: Draft,
    locale: String,
    event: Option<NotificationEvent>,
) -> Result<DeliveryResult> {
    state
        .test(file_path(&app)?, channel, draft, locale, event)
        .await
}
#[tauri::command]
pub async fn dispatch_notification(
    app: tauri::AppHandle,
    state: State<'_, Arc<NotificationService>>,
    event: NotificationEvent,
) -> Result<()> {
    // Each spawned delivery records its own result. Dropping handles does not cancel tasks.
    state.dispatch(file_path(&app)?, event).await?;
    Ok(())
}

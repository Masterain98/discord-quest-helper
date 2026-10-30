#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod native;
#[cfg(not(target_os = "windows"))]
#[path = "other.rs"]
mod native;
use super::super::{
    artwork,
    channel::{Descriptor, NotificationChannel, Receipt, ResolvedConfig},
    error::{Code, Error, Result},
    message::Message,
};
use futures_util::future::BoxFuture;
use tauri::Manager;

pub(super) struct SystemChannel {
    app: tauri::AppHandle,
}
impl SystemChannel {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}
impl NotificationChannel for SystemChannel {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            template_variables: &[],
            id: "system",
            name_key: "notifications.system",
            description_key: Some("notifications.system_desc"),
            icon: Some("monitor"),
            fields: &[],
            help_links: &[],
            enabled_by_default: true,
        }
    }
    fn validate(&self, _: &ResolvedConfig) -> Result<()> {
        Ok(())
    }
    fn send<'a>(
        &'a self,
        _: &'a ResolvedConfig,
        message: &'a Message,
    ) -> BoxFuture<'a, Result<Receipt>> {
        Box::pin(async move {
            let directory = self
                .app
                .path()
                .app_local_data_dir()
                .map_err(|_| Code::Storage)?;
            let image = match message.reward_image.as_deref() {
                Some(source) => artwork::prepare(&directory, source).await,
                None => None,
            };
            let title = message.title.clone();
            let body = message.plain_body();
            #[cfg(not(target_os = "windows"))]
            let app = self.app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                #[cfg(target_os = "windows")]
                let result = native::send(&directory, &title, &body, image.as_deref());
                #[cfg(not(target_os = "windows"))]
                let result = native::send(&app, &title, &body, image.as_deref());
                result.map_err(|_| Error::new(Code::SystemUnavailable))
            })
            .await
            .map_err(|_| Code::SystemUnavailable)??;
            Ok(Receipt::Submitted)
        })
    }
}

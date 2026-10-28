use std::path::Path;
pub fn send(
    app: &tauri::AppHandle,
    title: &str,
    body: &str,
    image: Option<&Path>,
) -> Result<(), &'static str> {
    use tauri_plugin_notification::NotificationExt;
    if let Some(image) = image {
        #[cfg(target_os = "macos")]
        let _ = notify_rust::set_application(if tauri::is_dev() {
            "com.apple.Terminal"
        } else {
            &app.config().identifier
        });
        let mut notification = notify_rust::Notification::new();
        notification
            .summary(title)
            .body(body)
            .image_path(&image.to_string_lossy());
        #[cfg(target_os = "linux")]
        notification.appname("Discord Quest Helper").auto_icon();
        if notification.show().is_ok() {
            return Ok(());
        }
        // A server that rejects artwork still gets a text-only attempt.
    }
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|_| "system_unavailable")
}

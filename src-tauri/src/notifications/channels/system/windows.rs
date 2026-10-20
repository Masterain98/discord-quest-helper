use std::path::Path;

pub fn send(
    directory: &Path,
    title: &str,
    body: &str,
    image: Option<&Path>,
) -> Result<(), &'static str> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    // A notification identity is independent of the randomized *process* AUMID.
    // Register per-user so the same code also works in an uninstalled portable build.
    const AUMID: &str = "com.masterain.discord-quest-helper.notifications";
    let result = (|| -> anyhow::Result<()> {
        std::fs::create_dir_all(directory)?;
        let icon = directory.join("notification-icon.png");
        if !icon.exists() {
            std::fs::write(&icon, include_bytes!("../../../../icons/128x128.png"))?;
        }
        let registration = windows_registry::CURRENT_USER
            .create(format!("Software\\Classes\\AppUserModelId\\{AUMID}"))?;
        registration.set_string("DisplayName", "Discord Quest Helper")?;
        registration.set_string("IconUri", icon.to_string_lossy().as_ref())?;

        // Build text through DOM nodes so quest names cannot inject toast XML.
        let xml = XmlDocument::new()?;
        xml.LoadXml(&HSTRING::from(
            "<toast><visual><binding template=\"ToastGeneric\"><text/><text/></binding></visual></toast>",
        ))?;
        let texts = xml.GetElementsByTagName(&HSTRING::from("text"))?;
        texts
            .Item(0)?
            .AppendChild(&xml.CreateTextNode(&HSTRING::from(title))?)?;
        texts
            .Item(1)?
            .AppendChild(&xml.CreateTextNode(&HSTRING::from(body))?)?;
        if let Some(uri) = image.and_then(|path| url::Url::from_file_path(path).ok()) {
            let binding = xml
                .GetElementsByTagName(&HSTRING::from("binding"))?
                .Item(0)?;
            let artwork = xml.CreateElement(&HSTRING::from("image"))?;
            artwork.SetAttribute(
                &HSTRING::from("placement"),
                &HSTRING::from("appLogoOverride"),
            )?;
            artwork.SetAttribute(&HSTRING::from("src"), &HSTRING::from(uri.as_str()))?;
            binding.AppendChild(&artwork)?;
        }
        let toast = ToastNotification::CreateToastNotification(&xml)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)?;
        Ok(())
    })();
    if result.is_err() && image.is_some() {
        return send(directory, title, body, None);
    }
    result.map_err(|_| "system_unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::{message, model::NotificationEvent};
    #[test]
    #[ignore = "submits a real system notification; run explicitly on Windows"]
    fn windows_system_smoke() {
        let directory = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
            .join("com.masterain.discord-quest-helper");
        let artwork = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("public/icons/orbs.png");
        // A standalone smoke test has no UI locale; allow an explicit selection
        // and use the same localized generic test message as the settings page.
        let locale = std::env::var("DQH_NOTIFICATION_TEST_LOCALE").unwrap_or_else(|_| "en".into());
        let message = message::render(&NotificationEvent::sample(locale), true);
        send(
            &directory,
            &message.title,
            &message.plain_body(),
            Some(&artwork),
        )
        .unwrap();
        let registration = windows_registry::CURRENT_USER.open("Software\\Classes\\AppUserModelId\\com.masterain.discord-quest-helper.notifications").unwrap();
        assert_eq!(
            registration.get_string("DisplayName").unwrap(),
            "Discord Quest Helper"
        );
    }
}

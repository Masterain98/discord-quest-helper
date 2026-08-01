use discord_cdp_launch_core as cdp_launch;

#[derive(Debug, serde::Serialize)]
pub(crate) struct DiscordCdpLaunchResultDto {
    launched_path: String,
    channel: cdp_launch::DiscordChannel,
    port: u16,
    cdp_connected: bool,
}

impl From<cdp_launch::LaunchResult> for DiscordCdpLaunchResultDto {
    fn from(value: cdp_launch::LaunchResult) -> Self {
        Self {
            launched_path: value.launched_path.to_string_lossy().into_owned(),
            channel: value.channel,
            port: value.port,
            cdp_connected: value.cdp_connected,
        }
    }
}

#[tauri::command]
pub(crate) async fn is_discord_running(channel: Option<String>) -> Result<bool, String> {
    let channel =
        cdp_launch::parse_discord_channel(channel.as_deref()).map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || cdp_launch::is_discord_running(channel))
        .await
        .map_err(|error| format!("Discord process scan task failed: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn launch_discord_cdp(
    port: Option<u16>,
    channel: Option<String>,
) -> Result<DiscordCdpLaunchResultDto, String> {
    launch(port, channel, false).await
}

#[tauri::command]
pub(crate) async fn restart_discord_cdp(
    port: Option<u16>,
    channel: Option<String>,
) -> Result<DiscordCdpLaunchResultDto, String> {
    launch(port, channel, true).await
}

async fn launch(
    port: Option<u16>,
    channel: Option<String>,
    restart_existing: bool,
) -> Result<DiscordCdpLaunchResultDto, String> {
    let channel =
        cdp_launch::parse_discord_channel(channel.as_deref()).map_err(|error| error.to_string())?;
    let options = cdp_launch::LaunchOptions {
        port: port.unwrap_or(cdp_launch::DEFAULT_CDP_PORT),
        channel,
        restart_existing,
        ..Default::default()
    };

    tauri::async_runtime::spawn_blocking(move || cdp_launch::launch_discord_with_cdp(options))
        .await
        .map_err(|error| format!("CDP launcher task failed: {error}"))?
        .map(Into::into)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdp_launch::{DiscordChannel, LaunchOutcome, LaunchResult};
    use std::path::PathBuf;

    #[test]
    fn launch_dto_keeps_the_frontend_json_contract() {
        let dto = DiscordCdpLaunchResultDto::from(LaunchResult {
            outcome: LaunchOutcome::Spawned,
            launched_path: PathBuf::from("C:\\Discord\\Discord.exe"),
            channel: DiscordChannel::Stable,
            port: 9223,
            pid: Some(1234),
            cdp_connected: true,
        });
        let value = serde_json::to_value(dto).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "launched_path": "C:\\Discord\\Discord.exe",
                "channel": "stable",
                "port": 9223,
                "cdp_connected": true
            })
        );
    }
}

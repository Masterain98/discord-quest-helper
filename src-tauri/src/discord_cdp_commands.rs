use discord_cdp_launch_core as cdp_launch;

#[derive(Debug, serde::Serialize)]
pub(crate) struct RunningCdpSessionDto {
    channel: cdp_launch::DiscordChannel,
    port: u16,
}

impl From<cdp_launch::RunningCdpSession> for RunningCdpSessionDto {
    fn from(value: cdp_launch::RunningCdpSession) -> Self {
        Self {
            channel: value.channel,
            port: value.port,
        }
    }
}

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
pub(crate) async fn list_running_discord_cdp_sessions() -> Result<Vec<RunningCdpSessionDto>, String>
{
    tauri::async_runtime::spawn_blocking(cdp_launch::list_running_discord_cdp_sessions)
        .await
        .map_err(|error| format!("Discord CDP process scan task failed: {error}"))?
        .map(|sessions| sessions.into_iter().map(Into::into).collect())
        .map_err(|error| error.to_string())
}

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopClientInventoryDto {
    official_installed: bool,
    vesktop_installed: bool,
    official_running: bool,
    vesktop_running: bool,
    cdp_owner: &'static str,
    stable_installed: bool,
    ptb_installed: bool,
    canary_installed: bool,
    stable_running: bool,
    ptb_running: bool,
    canary_running: bool,
}

fn desktop_client_inventory(
    installs: &[cdp_launch::DiscordInstall],
    vesktop_installed: bool,
    vesktop_running: bool,
    official_running: bool,
    channel_running: [bool; 3],
    cdp_owner: &'static str,
) -> DesktopClientInventoryDto {
    let stable_installed = installs
        .iter()
        .any(|install| install.channel == cdp_launch::DiscordChannel::Stable);
    let ptb_installed = installs
        .iter()
        .any(|install| install.channel == cdp_launch::DiscordChannel::Ptb);
    let canary_installed = installs
        .iter()
        .any(|install| install.channel == cdp_launch::DiscordChannel::Canary);
    DesktopClientInventoryDto {
        official_installed: !installs.is_empty(),
        vesktop_installed,
        official_running,
        vesktop_running,
        cdp_owner,
        stable_installed,
        ptb_installed,
        canary_installed,
        stable_running: channel_running[0],
        ptb_running: channel_running[1],
        canary_running: channel_running[2],
    }
}

#[tauri::command]
pub(crate) async fn list_desktop_clients(
    port: Option<u16>,
) -> Result<DesktopClientInventoryDto, String> {
    let port = port.unwrap_or(cdp_launch::DEFAULT_CDP_PORT);
    tauri::async_runtime::spawn_blocking(move || {
        let installs = cdp_launch::find_discord_installs().unwrap_or_default();
        Ok(desktop_client_inventory(
            &installs,
            cdp_launch::find_vesktop_install().is_some(),
            cdp_launch::is_vesktop_running().unwrap_or(false),
            cdp_launch::is_discord_running(None).unwrap_or(false),
            [
                cdp_launch::is_discord_running(Some(cdp_launch::DiscordChannel::Stable))
                    .unwrap_or(false),
                cdp_launch::is_discord_running(Some(cdp_launch::DiscordChannel::Ptb))
                    .unwrap_or(false),
                cdp_launch::is_discord_running(Some(cdp_launch::DiscordChannel::Canary))
                    .unwrap_or(false),
            ],
            cdp_launch::inspect_cdp_port_owner(port).as_str(),
        ))
    })
    .await
    .map_err(|error| format!("Desktop client scan task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn launch_discord_cdp(
    port: Option<u16>,
    channel: Option<String>,
    client: Option<String>,
) -> Result<DiscordCdpLaunchResultDto, String> {
    launch(port, channel, client, false).await
}

#[tauri::command]
pub(crate) async fn restart_discord_cdp(
    port: Option<u16>,
    channel: Option<String>,
    client: Option<String>,
) -> Result<DiscordCdpLaunchResultDto, String> {
    launch(port, channel, client, true).await
}

async fn launch(
    port: Option<u16>,
    channel: Option<String>,
    client: Option<String>,
    restart_existing: bool,
) -> Result<DiscordCdpLaunchResultDto, String> {
    let channel =
        cdp_launch::parse_discord_channel(channel.as_deref()).map_err(|error| error.to_string())?;
    let client = cdp_launch::parse_desktop_client_preference(client.as_deref())
        .map_err(|error| error.to_string())?;
    let options = cdp_launch::LaunchOptions {
        port: port.unwrap_or(cdp_launch::DEFAULT_CDP_PORT),
        channel,
        client,
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

    #[test]
    fn inventory_dto_lists_each_official_channel_and_vesktop() {
        let installs = vec![
            cdp_launch::DiscordInstall {
                channel: DiscordChannel::Stable,
                executable_path: PathBuf::from("C:\\Discord\\Discord.exe"),
                working_dir: PathBuf::from("C:\\Discord"),
            },
            cdp_launch::DiscordInstall {
                channel: DiscordChannel::Canary,
                executable_path: PathBuf::from("C:\\DiscordCanary\\DiscordCanary.exe"),
                working_dir: PathBuf::from("C:\\DiscordCanary"),
            },
        ];
        let dto =
            desktop_client_inventory(&installs, true, false, true, [true, false, false], "none");
        assert_eq!(
            serde_json::to_value(dto).unwrap(),
            serde_json::json!({
                "officialInstalled": true,
                "vesktopInstalled": true,
                "officialRunning": true,
                "vesktopRunning": false,
                "cdpOwner": "none",
                "stableInstalled": true,
                "ptbInstalled": false,
                "canaryInstalled": true,
                "stableRunning": true,
                "ptbRunning": false,
                "canaryRunning": false
            })
        );
    }

    #[test]
    fn running_session_dto_keeps_the_frontend_json_contract() {
        let dto = RunningCdpSessionDto::from(cdp_launch::RunningCdpSession {
            channel: DiscordChannel::Ptb,
            port: 9333,
        });
        assert_eq!(
            serde_json::to_value(dto).unwrap(),
            serde_json::json!({ "channel": "ptb", "port": 9333 })
        );
    }
}

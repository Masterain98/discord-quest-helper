use discord_cdp_launch_core as cdp_launch;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::Manager;

const DESKTOP_CLIENTS_CONFIG_FILE: &str = "desktop-clients.v1.json";
const DESKTOP_CLIENT_SESSIONS_FILE: &str = "desktop-client-sessions.v1.json";
static STATE_REVISION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub(crate) struct DiscordCdpLaunchResultDto {
    launched_path: String,
    channel: cdp_launch::DiscordChannel,
    port: u16,
    cdp_connected: bool,
    #[serde(rename = "providerId")]
    provider_id: cdp_launch::ProviderId,
    #[serde(rename = "installationId")]
    installation_id: Option<cdp_launch::InstallationId>,
    #[serde(rename = "variantId")]
    variant_id: Option<cdp_launch::VariantId>,
    ownership: cdp_launch::SessionOwnership,
}

impl From<cdp_launch::LaunchResult> for DiscordCdpLaunchResultDto {
    fn from(value: cdp_launch::LaunchResult) -> Self {
        Self {
            launched_path: value.launched_path.to_string_lossy().into_owned(),
            channel: value.channel,
            port: value.port,
            cdp_connected: value.cdp_connected,
            provider_id: value.provider_id,
            installation_id: value.installation_id,
            variant_id: value.variant_id,
            ownership: value.ownership,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClientProcessDto {
    provider_id: cdp_launch::ProviderId,
    installation_id: cdp_launch::InstallationId,
    variant_id: Option<cdp_launch::VariantId>,
    executable_path: Option<String>,
    running: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CdpEndpointDto {
    port: u16,
    status: &'static str,
    owner: &'static str,
    owner_provider_id: Option<cdp_launch::ProviderId>,
    target_title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveryIssueDto {
    provider_id: Option<cdp_launch::ProviderId>,
    code: &'static str,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopClientStateDto {
    installations: Vec<cdp_launch::ClientInstallation>,
    processes: Vec<ClientProcessDto>,
    endpoint: CdpEndpointDto,
    selection: cdp_launch::LaunchSelector,
    discovery_issues: Vec<DiscoveryIssueDto>,
    port: u16,
    revision: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopClientsConfig {
    #[serde(default = "config_version")]
    version: u8,
    #[serde(default)]
    installations: Vec<cdp_launch::ClientInstallation>,
    #[serde(default)]
    selection: cdp_launch::LaunchSelector,
}

const fn config_version() -> u8 {
    1
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopClientCommandError {
    code: &'static str,
    params: serde_json::Value,
    message: String,
}

impl DesktopClientCommandError {
    fn new(code: &'static str, params: serde_json::Value, message: impl Into<String>) -> Self {
        Self {
            code,
            params,
            message: message.into(),
        }
    }
}

impl From<cdp_launch::LaunchError> for DesktopClientCommandError {
    fn from(error: cdp_launch::LaunchError) -> Self {
        let (code, params) = match &error {
            cdp_launch::LaunchError::InstallNotFound { .. } => {
                ("installation_missing", serde_json::json!({}))
            }
            cdp_launch::LaunchError::InvalidInstallation { details } => (
                "installation_invalid",
                serde_json::json!({ "details": details }),
            ),
            cdp_launch::LaunchError::CdpOwnedByOtherClient { port, owner } => (
                "endpoint_owner_conflict",
                serde_json::json!({ "port": port, "owner": owner }),
            ),
            cdp_launch::LaunchError::DesktopClientAlreadyRunning { client } => {
                ("restart_required", serde_json::json!({ "client": client }))
            }
            cdp_launch::LaunchError::DiscordAlreadyRunning { channel } => (
                "restart_required",
                serde_json::json!({ "client": channel.map(|value| value.display_name()) }),
            ),
            cdp_launch::LaunchError::PortOccupied { port } => {
                ("port_occupied", serde_json::json!({ "port": port }))
            }
            cdp_launch::LaunchError::NonDiscordCdpTarget { port } => {
                ("non_discord_cdp", serde_json::json!({ "port": port }))
            }
            cdp_launch::LaunchError::ShutdownTimeout { .. } => {
                ("process_ambiguous", serde_json::json!({}))
            }
            cdp_launch::LaunchError::ReadinessTimeout { port, .. } => {
                ("cdp_readiness_timeout", serde_json::json!({ "port": port }))
            }
            _ => ("launch_failed", serde_json::json!({})),
        };
        Self::new(code, params, error.to_string())
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

#[tauri::command]
pub(crate) async fn list_running_desktop_cdp_sessions(
    app: tauri::AppHandle,
) -> Result<Vec<cdp_launch::DesktopCdpSession>, DesktopClientCommandError> {
    let journal = load_session_journal(&app)?;
    let mut sessions =
        tauri::async_runtime::spawn_blocking(cdp_launch::list_running_desktop_cdp_sessions)
            .await
            .map_err(|error| {
                DesktopClientCommandError::new(
                    "scan_failed",
                    serde_json::json!({}),
                    format!("Desktop client session scan failed: {error}"),
                )
            })?
            .map_err(DesktopClientCommandError::from)?;
    for session in &mut sessions {
        if journal
            .iter()
            .any(|managed| session_key_matches(managed, session))
        {
            session.ownership = cdp_launch::SessionOwnership::Managed;
        }
    }
    let live: Vec<_> = journal
        .into_iter()
        .filter(|managed| {
            sessions
                .iter()
                .any(|session| session_key_matches(managed, session))
        })
        .collect();
    save_session_journal(&app, &live)?;
    Ok(sessions)
}

#[tauri::command]
pub(crate) async fn restore_desktop_client_session(
    app: tauri::AppHandle,
    installation_id: String,
    port: u16,
    confirm_external: Option<bool>,
) -> Result<(), DesktopClientCommandError> {
    let installation_id = cdp_launch::InstallationId(installation_id);
    let journal = load_session_journal(&app)?;
    let managed = journal.iter().any(|session| {
        session.installation_id.as_ref() == Some(&installation_id) && session.port == port
    });
    if !managed && !confirm_external.unwrap_or(false) {
        return Err(DesktopClientCommandError::new(
            "external_confirmation_required",
            serde_json::json!({ "installationId": installation_id, "port": port }),
            "This CDP session was started outside Helper and requires explicit confirmation.",
        ));
    }
    let config = load_config(&app)?;
    let state =
        tauri::async_runtime::spawn_blocking(move || build_desktop_client_state(config, port))
            .await
            .map_err(|error| {
                DesktopClientCommandError::new(
                    "scan_failed",
                    serde_json::json!({}),
                    format!("Desktop client scan failed: {error}"),
                )
            })?;
    let installation = state
        .installations
        .into_iter()
        .find(|install| install.id == installation_id)
        .ok_or_else(|| {
            DesktopClientCommandError::new(
                "installation_missing",
                serde_json::json!({ "installationId": installation_id }),
                "The CDP session installation could not be located for restoration.",
            )
        })?;
    tauri::async_runtime::spawn_blocking(move || {
        cdp_launch::restore_desktop_client_to_normal(&installation, port)
    })
    .await
    .map_err(|error| {
        DesktopClientCommandError::new(
            "restore_failed",
            serde_json::json!({ "port": port }),
            format!("Desktop client restore task failed: {error}"),
        )
    })?
    .map_err(|error| {
        DesktopClientCommandError::new(
            "restore_failed",
            serde_json::json!({ "port": port }),
            error.to_string(),
        )
    })?;
    let mut journal = load_session_journal(&app)?;
    journal.retain(|session| {
        !(session.installation_id.as_ref() == Some(&installation_id) && session.port == port)
    });
    save_session_journal(&app, &journal)
}

#[derive(Debug, Serialize, PartialEq, Eq)]
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

#[cfg(test)]
fn desktop_client_inventory(
    installs: &[cdp_launch::DiscordInstall],
    vesktop_installed: bool,
    vesktop_running: bool,
    official_running: bool,
    channel_running: [bool; 3],
    cdp_owner: &'static str,
) -> DesktopClientInventoryDto {
    DesktopClientInventoryDto {
        official_installed: !installs.is_empty(),
        vesktop_installed,
        official_running,
        vesktop_running,
        cdp_owner,
        stable_installed: installs
            .iter()
            .any(|install| install.channel == cdp_launch::DiscordChannel::Stable),
        ptb_installed: installs
            .iter()
            .any(|install| install.channel == cdp_launch::DiscordChannel::Ptb),
        canary_installed: installs
            .iter()
            .any(|install| install.channel == cdp_launch::DiscordChannel::Canary),
        stable_running: channel_running[0],
        ptb_running: channel_running[1],
        canary_running: channel_running[2],
    }
}

/// Compatibility adapter retained for one release. New UI consumes the atomic snapshot.
#[tauri::command]
pub(crate) async fn list_desktop_clients(
    app: tauri::AppHandle,
    port: Option<u16>,
) -> Result<DesktopClientInventoryDto, String> {
    let state = get_desktop_client_state(app, port)
        .await
        .map_err(|error| error.message)?;
    let official = cdp_launch::ProviderId::official_discord();
    let vesktop = cdp_launch::ProviderId::vesktop();
    let installed = |provider: &cdp_launch::ProviderId, variant: Option<&str>| {
        state.installations.iter().any(|install| {
            &install.provider_id == provider
                && variant
                    .is_none_or(|value| install.variant_id.as_ref().is_some_and(|id| id.0 == value))
                && install.validation == cdp_launch::ValidationState::Valid
        })
    };
    let running = |provider: &cdp_launch::ProviderId, variant: Option<&str>| {
        state.processes.iter().any(|process| {
            &process.provider_id == provider
                && process.running
                && variant
                    .is_none_or(|value| process.variant_id.as_ref().is_some_and(|id| id.0 == value))
        })
    };
    Ok(DesktopClientInventoryDto {
        official_installed: installed(&official, None),
        vesktop_installed: installed(&vesktop, None),
        official_running: running(&official, None),
        vesktop_running: running(&vesktop, None),
        cdp_owner: state.endpoint.owner,
        stable_installed: installed(&official, Some("stable")),
        ptb_installed: installed(&official, Some("ptb")),
        canary_installed: installed(&official, Some("canary")),
        stable_running: running(&official, Some("stable")),
        ptb_running: running(&official, Some("ptb")),
        canary_running: running(&official, Some("canary")),
    })
}

#[tauri::command]
pub(crate) async fn get_desktop_client_state(
    app: tauri::AppHandle,
    port: Option<u16>,
) -> Result<DesktopClientStateDto, DesktopClientCommandError> {
    let port = port.unwrap_or(cdp_launch::DEFAULT_CDP_PORT);
    if port == 0 {
        return Err(DesktopClientCommandError::new(
            "invalid_port",
            serde_json::json!({ "port": port }),
            "CDP port must be between 1 and 65535.",
        ));
    }
    let config = load_config(&app)?;
    tauri::async_runtime::spawn_blocking(move || build_desktop_client_state(config, port))
        .await
        .map_err(|error| {
            DesktopClientCommandError::new(
                "scan_failed",
                serde_json::json!({}),
                format!("Desktop client scan task failed: {error}"),
            )
        })
}

#[tauri::command]
pub(crate) async fn add_desktop_client_installation(
    app: tauri::AppHandle,
    provider_id: String,
    path: String,
    port: Option<u16>,
) -> Result<DesktopClientStateDto, DesktopClientCommandError> {
    let provider_id = cdp_launch::ProviderId(provider_id);
    let mut installation =
        cdp_launch::custom_executable_installation(&provider_id, PathBuf::from(path))
            .map_err(DesktopClientCommandError::from)?;
    if installation.validation != cdp_launch::ValidationState::Valid {
        return Err(DesktopClientCommandError::new(
            "installation_missing",
            serde_json::json!({ "installationId": installation.id }),
            "The selected executable does not exist.",
        ));
    }
    installation.source = cdp_launch::DiscoverySource::User;
    let mut config = load_config(&app)?;
    config
        .installations
        .retain(|existing| existing.id != installation.id);
    config.installations.insert(0, installation.clone());
    config.selection = cdp_launch::LaunchSelector::Installation {
        installation_id: installation.id,
    };
    save_config(&app, &config)?;
    get_desktop_client_state(app, port).await
}

#[tauri::command]
pub(crate) async fn remove_desktop_client_installation(
    app: tauri::AppHandle,
    installation_id: String,
    port: Option<u16>,
) -> Result<DesktopClientStateDto, DesktopClientCommandError> {
    let installation_id = cdp_launch::InstallationId(installation_id);
    let mut config = load_config(&app)?;
    config
        .installations
        .retain(|install| install.id != installation_id);
    if matches!(&config.selection, cdp_launch::LaunchSelector::Installation { installation_id: selected } if selected == &installation_id)
    {
        config.selection = cdp_launch::LaunchSelector::Auto;
    }
    save_config(&app, &config)?;
    get_desktop_client_state(app, port).await
}

#[tauri::command]
pub(crate) async fn set_desktop_client_selection(
    app: tauri::AppHandle,
    selection: cdp_launch::LaunchSelector,
    port: Option<u16>,
) -> Result<DesktopClientStateDto, DesktopClientCommandError> {
    let mut config = load_config(&app)?;
    if let cdp_launch::LaunchSelector::Installation { installation_id } = &selection {
        if !config
            .installations
            .iter()
            .any(|install| &install.id == installation_id)
        {
            let scan_port = port.unwrap_or(cdp_launch::DEFAULT_CDP_PORT);
            let requested_id = installation_id.clone();
            let scan_config = config.clone();
            let discovered = tauri::async_runtime::spawn_blocking(move || {
                build_desktop_client_state(scan_config, scan_port)
                    .installations
                    .into_iter()
                    .find(|install| install.id == requested_id)
            })
            .await
            .map_err(|error| {
                DesktopClientCommandError::new(
                    "scan_failed",
                    serde_json::json!({}),
                    format!("Desktop client scan task failed: {error}"),
                )
            })?
            .ok_or_else(|| {
                DesktopClientCommandError::new(
                    "installation_missing",
                    serde_json::json!({ "installationId": installation_id }),
                    "The selected installation is no longer available.",
                )
            })?;
            config.installations.push(discovered);
        }
    }
    config.selection = selection;
    save_config(&app, &config)?;
    get_desktop_client_state(app, port).await
}

#[tauri::command]
pub(crate) async fn launch_desktop_client_cdp(
    app: tauri::AppHandle,
    port: Option<u16>,
    selection: Option<cdp_launch::LaunchSelector>,
    restart_existing: Option<bool>,
) -> Result<DiscordCdpLaunchResultDto, DesktopClientCommandError> {
    let config = load_config(&app)?;
    let selector = selection.unwrap_or_else(|| config.selection.clone());
    let port = port.unwrap_or(cdp_launch::DEFAULT_CDP_PORT);
    let restart_existing = restart_existing.unwrap_or(false);
    if restart_existing {
        let selector_for_resolution = selector.clone();
        let config_for_resolution = config.clone();
        tauri::async_runtime::spawn_blocking(move || {
            resolve_conflicting_endpoint(&selector_for_resolution, &config_for_resolution, port)
        })
        .await
        .map_err(|error| {
            DesktopClientCommandError::new(
                "restore_failed",
                serde_json::json!({ "port": port }),
                format!("CDP owner switch task failed: {error}"),
            )
        })??;
    }
    let options = options_for_selector(port, &selector, &config, restart_existing)?;
    let result =
        tauri::async_runtime::spawn_blocking(move || cdp_launch::launch_discord_with_cdp(options))
            .await
            .map_err(|error| {
                DesktopClientCommandError::new(
                    "launch_task_failed",
                    serde_json::json!({}),
                    format!("CDP launcher task failed: {error}"),
                )
            })?
            .map_err(DesktopClientCommandError::from)?;
    record_managed_session(&app, &result)?;
    Ok(result.into())
}

fn resolve_conflicting_endpoint(
    selector: &cdp_launch::LaunchSelector,
    config: &DesktopClientsConfig,
    port: u16,
) -> Result<(), DesktopClientCommandError> {
    let (selected_provider, selected_installation_id) = match selector {
        cdp_launch::LaunchSelector::Auto => return Ok(()),
        cdp_launch::LaunchSelector::Provider { provider_id, .. } => (provider_id.clone(), None),
        cdp_launch::LaunchSelector::Installation { installation_id } => (
            config
                .installations
                .iter()
                .find(|install| &install.id == installation_id)
                .map(|install| install.provider_id.clone())
                .ok_or_else(|| {
                    DesktopClientCommandError::new(
                        "installation_missing",
                        serde_json::json!({ "installationId": installation_id }),
                        "The selected installation is no longer configured.",
                    )
                })?,
            Some(installation_id.clone()),
        ),
    };
    let owner_provider = match cdp_launch::inspect_cdp_port_owner(port) {
        cdp_launch::CdpPortOwner::Official => cdp_launch::ProviderId::official_discord(),
        cdp_launch::CdpPortOwner::Vesktop => cdp_launch::ProviderId::vesktop(),
        _ => return Ok(()),
    };
    if owner_provider == selected_provider && selected_installation_id.is_none() {
        return Ok(());
    }
    let sessions =
        cdp_launch::list_running_desktop_cdp_sessions().map_err(DesktopClientCommandError::from)?;
    let owner_session = sessions
        .into_iter()
        .find(|session| session.port == port && session.provider_id == owner_provider)
        .ok_or_else(|| {
            DesktopClientCommandError::new(
                "process_ambiguous",
                serde_json::json!({ "port": port, "providerId": owner_provider }),
                "The current CDP owner could not be mapped to one exact installation.",
            )
        })?;
    if owner_provider == selected_provider
        && owner_session.installation_id == selected_installation_id
    {
        return Ok(());
    }
    let installation_id = owner_session.installation_id.ok_or_else(|| {
        DesktopClientCommandError::new(
            "process_ambiguous",
            serde_json::json!({ "port": port, "providerId": owner_provider }),
            "The current CDP owner installation is ambiguous.",
        )
    })?;
    let state = build_desktop_client_state(config.clone(), port);
    let installation = state
        .installations
        .iter()
        .find(|install| install.id == installation_id)
        .ok_or_else(|| {
            DesktopClientCommandError::new(
                "installation_missing",
                serde_json::json!({ "installationId": installation_id }),
                "The current CDP owner installation could not be located.",
            )
        })?;
    cdp_launch::restore_desktop_client_to_normal(installation, port).map_err(|error| {
        DesktopClientCommandError::new(
            "restore_failed",
            serde_json::json!({ "port": port, "providerId": owner_provider }),
            error.to_string(),
        )
    })
}

#[tauri::command]
pub(crate) async fn launch_discord_cdp(
    app: tauri::AppHandle,
    port: Option<u16>,
    channel: Option<String>,
    client: Option<String>,
) -> Result<DiscordCdpLaunchResultDto, String> {
    launch_compat(app, port, channel, client, false).await
}

#[tauri::command]
pub(crate) async fn restart_discord_cdp(
    app: tauri::AppHandle,
    port: Option<u16>,
    channel: Option<String>,
    client: Option<String>,
) -> Result<DiscordCdpLaunchResultDto, String> {
    launch_compat(app, port, channel, client, true).await
}

async fn launch_compat(
    app: tauri::AppHandle,
    port: Option<u16>,
    channel: Option<String>,
    client: Option<String>,
    restart_existing: bool,
) -> Result<DiscordCdpLaunchResultDto, String> {
    let channel =
        cdp_launch::parse_discord_channel(channel.as_deref()).map_err(|error| error.to_string())?;
    let client = cdp_launch::parse_desktop_client_preference(client.as_deref())
        .map_err(|error| error.to_string())?;
    let config = load_config(&app).map_err(|error| error.message)?;
    let installation = matching_saved_installation(&config, client);
    let options = cdp_launch::LaunchOptions {
        port: port.unwrap_or(cdp_launch::DEFAULT_CDP_PORT),
        channel,
        client,
        installation,
        restart_existing,
        ..Default::default()
    };
    let result =
        tauri::async_runtime::spawn_blocking(move || cdp_launch::launch_discord_with_cdp(options))
            .await
            .map_err(|error| format!("CDP launcher task failed: {error}"))?
            .map_err(|error| error.to_string())?;
    record_managed_session(&app, &result).map_err(|error| error.message)?;
    Ok(result.into())
}

fn build_desktop_client_state(config: DesktopClientsConfig, port: u16) -> DesktopClientStateDto {
    let mut issues = Vec::new();
    let mut installations: Vec<_> = config
        .installations
        .iter()
        .cloned()
        .map(cdp_launch::refresh_installation_validation)
        .collect();
    let mut seen: HashSet<_> = installations
        .iter()
        .map(|install| install.id.clone())
        .collect();

    #[cfg(windows)]
    for install in windows_registry_vesktop_installations() {
        if seen.insert(install.id.clone()) {
            installations.push(install);
        }
    }

    let (discovered, discovery_errors) = cdp_launch::discover_client_installations();
    for install in discovered {
        if seen.insert(install.id.clone()) {
            installations.push(install);
        }
    }
    issues.extend(
        discovery_errors
            .into_iter()
            .map(|message| DiscoveryIssueDto {
                provider_id: None,
                code: "scan_failed",
                message,
            }),
    );

    let processes = installations
        .iter()
        .filter_map(|install| {
            let running = cdp_launch::is_client_installation_running(install).unwrap_or(false);
            running.then(|| ClientProcessDto {
                provider_id: install.provider_id.clone(),
                installation_id: install.id.clone(),
                variant_id: install.variant_id.clone(),
                executable_path: installation_executable_path(install)
                    .map(|path| path.to_string_lossy().into_owned()),
                running,
            })
        })
        .collect();
    DesktopClientStateDto {
        installations,
        processes,
        endpoint: endpoint_dto(port),
        selection: config.selection,
        discovery_issues: issues,
        port,
        revision: STATE_REVISION.fetch_add(1, Ordering::SeqCst),
    }
}

fn endpoint_dto(port: u16) -> CdpEndpointDto {
    let owner = cdp_launch::inspect_cdp_port_owner(port);
    let (status, target_title) = match cdp_launch::probe_cdp(port) {
        cdp_launch::CdpProbeStatus::Unreachable => ("unreachable", None),
        cdp_launch::CdpProbeStatus::PortOccupied => ("occupied", None),
        cdp_launch::CdpProbeStatus::CdpWithoutDiscordTarget => ("nonDiscordCdp", None),
        cdp_launch::CdpProbeStatus::DiscordReady { target_title } => ("discordReady", target_title),
    };
    let owner_provider_id = match owner {
        cdp_launch::CdpPortOwner::Official => Some(cdp_launch::ProviderId::official_discord()),
        cdp_launch::CdpPortOwner::Vesktop => Some(cdp_launch::ProviderId::vesktop()),
        _ => None,
    };
    CdpEndpointDto {
        port,
        status,
        owner: owner.as_str(),
        owner_provider_id,
        target_title,
    }
}

fn options_for_selector(
    port: u16,
    selector: &cdp_launch::LaunchSelector,
    config: &DesktopClientsConfig,
    restart_existing: bool,
) -> Result<cdp_launch::LaunchOptions, DesktopClientCommandError> {
    let (client, installation, channel) = match selector {
        cdp_launch::LaunchSelector::Auto => (cdp_launch::DesktopClientPreference::Auto, None, None),
        cdp_launch::LaunchSelector::Provider {
            provider_id,
            variant_id,
        } if provider_id == &cdp_launch::ProviderId::official_discord() => {
            let channel = variant_id
                .as_ref()
                .map(|variant| cdp_launch::parse_discord_channel(Some(&variant.0)))
                .transpose()
                .map_err(DesktopClientCommandError::from)?
                .flatten();
            (cdp_launch::DesktopClientPreference::Official, None, channel)
        }
        cdp_launch::LaunchSelector::Provider { provider_id, .. }
            if provider_id == &cdp_launch::ProviderId::vesktop() =>
        {
            (cdp_launch::DesktopClientPreference::Vesktop, None, None)
        }
        cdp_launch::LaunchSelector::Provider { provider_id, .. } => {
            return Err(DesktopClientCommandError::new(
                "provider_unsupported",
                serde_json::json!({ "providerId": provider_id }),
                format!(
                    "Unsupported desktop client provider: {}",
                    provider_id.as_str()
                ),
            ))
        }
        cdp_launch::LaunchSelector::Installation { installation_id } => {
            let installation = config
                .installations
                .iter()
                .find(|install| &install.id == installation_id)
                .cloned()
                .ok_or_else(|| {
                    DesktopClientCommandError::new(
                        "installation_missing",
                        serde_json::json!({ "installationId": installation_id }),
                        "The selected desktop client installation is no longer configured.",
                    )
                })?;
            let installation = cdp_launch::refresh_installation_validation(installation);
            if installation.validation != cdp_launch::ValidationState::Valid {
                return Err(DesktopClientCommandError::new(
                    "installation_missing",
                    serde_json::json!({ "installationId": installation_id }),
                    "The selected desktop client installation must be relocated.",
                ));
            }
            let client = if installation.provider_id == cdp_launch::ProviderId::vesktop() {
                cdp_launch::DesktopClientPreference::Vesktop
            } else {
                cdp_launch::DesktopClientPreference::Official
            };
            let channel = installation
                .variant_id
                .as_ref()
                .and_then(|variant| cdp_launch::parse_discord_channel(Some(&variant.0)).ok())
                .flatten();
            (client, Some(installation), channel)
        }
    };
    Ok(cdp_launch::LaunchOptions {
        port,
        channel,
        client,
        installation,
        restart_existing,
        ..Default::default()
    })
}

fn matching_saved_installation(
    config: &DesktopClientsConfig,
    client: cdp_launch::DesktopClientPreference,
) -> Option<cdp_launch::ClientInstallation> {
    let cdp_launch::LaunchSelector::Installation { installation_id } = &config.selection else {
        return None;
    };
    config.installations.iter().find_map(|install| {
        if &install.id != installation_id {
            return None;
        }
        let provider_matches = match client {
            cdp_launch::DesktopClientPreference::Auto => true,
            cdp_launch::DesktopClientPreference::Official => {
                install.provider_id == cdp_launch::ProviderId::official_discord()
            }
            cdp_launch::DesktopClientPreference::Vesktop => {
                install.provider_id == cdp_launch::ProviderId::vesktop()
            }
        };
        provider_matches.then(|| cdp_launch::refresh_installation_validation(install.clone()))
    })
}

fn installation_executable_path(install: &cdp_launch::ClientInstallation) -> Option<&Path> {
    match &install.launch_target {
        cdp_launch::LaunchTarget::Executable { path, .. } => Some(path),
        cdp_launch::LaunchTarget::MacBundle {
            executable_path, ..
        } => Some(executable_path),
        cdp_launch::LaunchTarget::Flatpak { .. } => None,
    }
}

fn config_path(app: &tauri::AppHandle) -> Result<PathBuf, DesktopClientCommandError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(DESKTOP_CLIENTS_CONFIG_FILE))
        .map_err(|error| {
            DesktopClientCommandError::new(
                "config_unavailable",
                serde_json::json!({}),
                format!("Desktop client config directory is unavailable: {error}"),
            )
        })
}

fn load_config(app: &tauri::AppHandle) -> Result<DesktopClientsConfig, DesktopClientCommandError> {
    let path = config_path(app)?;
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            DesktopClientCommandError::new(
                "config_invalid",
                serde_json::json!({}),
                format!("Desktop client config is invalid: {error}"),
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DesktopClientsConfig {
            version: config_version(),
            ..Default::default()
        }),
        Err(error) => Err(DesktopClientCommandError::new(
            "config_unavailable",
            serde_json::json!({}),
            format!("Desktop client config could not be read: {error}"),
        )),
    }
}

fn save_config(
    app: &tauri::AppHandle,
    config: &DesktopClientsConfig,
) -> Result<(), DesktopClientCommandError> {
    let path = config_path(app)?;
    let parent = path.parent().ok_or_else(|| {
        DesktopClientCommandError::new(
            "config_unavailable",
            serde_json::json!({}),
            "Desktop client config path has no parent directory.",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        DesktopClientCommandError::new(
            "config_unavailable",
            serde_json::json!({}),
            format!("Desktop client config directory could not be created: {error}"),
        )
    })?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(config).map_err(|error| {
        DesktopClientCommandError::new(
            "config_invalid",
            serde_json::json!({}),
            format!("Desktop client config could not be serialized: {error}"),
        )
    })?;
    std::fs::write(&temporary, bytes)
        .and_then(|_| std::fs::rename(&temporary, &path))
        .map_err(|error| {
            DesktopClientCommandError::new(
                "config_unavailable",
                serde_json::json!({}),
                format!("Desktop client config could not be saved: {error}"),
            )
        })
}

fn session_journal_path(app: &tauri::AppHandle) -> Result<PathBuf, DesktopClientCommandError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(DESKTOP_CLIENT_SESSIONS_FILE))
        .map_err(|error| {
            DesktopClientCommandError::new(
                "config_unavailable",
                serde_json::json!({}),
                format!("Desktop client session journal is unavailable: {error}"),
            )
        })
}

fn load_session_journal(
    app: &tauri::AppHandle,
) -> Result<Vec<cdp_launch::DesktopCdpSession>, DesktopClientCommandError> {
    let path = session_journal_path(app)?;
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            DesktopClientCommandError::new(
                "config_invalid",
                serde_json::json!({}),
                format!("Desktop client session journal is invalid: {error}"),
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(DesktopClientCommandError::new(
            "config_unavailable",
            serde_json::json!({}),
            format!("Desktop client session journal could not be read: {error}"),
        )),
    }
}

fn save_session_journal(
    app: &tauri::AppHandle,
    sessions: &[cdp_launch::DesktopCdpSession],
) -> Result<(), DesktopClientCommandError> {
    let path = session_journal_path(app)?;
    let parent = path.parent().ok_or_else(|| {
        DesktopClientCommandError::new(
            "config_unavailable",
            serde_json::json!({}),
            "Desktop client session journal path has no parent directory.",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        DesktopClientCommandError::new(
            "config_unavailable",
            serde_json::json!({}),
            format!("Desktop client session journal directory could not be created: {error}"),
        )
    })?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(sessions).map_err(|error| {
        DesktopClientCommandError::new(
            "config_invalid",
            serde_json::json!({}),
            format!("Desktop client session journal could not be serialized: {error}"),
        )
    })?;
    std::fs::write(&temporary, bytes)
        .and_then(|_| std::fs::rename(&temporary, path))
        .map_err(|error| {
            DesktopClientCommandError::new(
                "config_unavailable",
                serde_json::json!({}),
                format!("Desktop client session journal could not be saved: {error}"),
            )
        })
}

fn record_managed_session(
    app: &tauri::AppHandle,
    result: &cdp_launch::LaunchResult,
) -> Result<(), DesktopClientCommandError> {
    if result.ownership != cdp_launch::SessionOwnership::Managed {
        return Ok(());
    }
    let mut journal = load_session_journal(app)?;
    let session = cdp_launch::DesktopCdpSession {
        provider_id: result.provider_id.clone(),
        installation_id: result.installation_id.clone(),
        variant_id: result.variant_id.clone(),
        port: result.port,
        ownership: cdp_launch::SessionOwnership::Managed,
        executable_path: (!result.launched_path.as_os_str().is_empty())
            .then(|| result.launched_path.clone()),
    };
    journal.retain(|existing| !session_key_matches(existing, &session));
    journal.push(session);
    save_session_journal(app, &journal)
}

fn session_key_matches(
    left: &cdp_launch::DesktopCdpSession,
    right: &cdp_launch::DesktopCdpSession,
) -> bool {
    left.provider_id == right.provider_id
        && left.installation_id == right.installation_id
        && left.port == right.port
}

#[cfg(windows)]
fn windows_registry_vesktop_installations() -> Vec<cdp_launch::ClientInstallation> {
    let mut installs = Vec::new();
    for root in [
        windows_registry::CURRENT_USER,
        windows_registry::LOCAL_MACHINE,
    ] {
        for key_path in [
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ] {
            let Ok(uninstall) = root.open(key_path) else {
                continue;
            };
            let Ok(keys) = uninstall.keys() else {
                continue;
            };
            for name in keys {
                let Ok(entry) = uninstall.open(&name) else {
                    continue;
                };
                if !entry
                    .get_string("DisplayName")
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains("vesktop")
                {
                    continue;
                }
                let location = entry.get_string("InstallLocation").unwrap_or_default();
                let icon = entry.get_string("DisplayIcon").unwrap_or_default();
                let icon = icon.trim_matches('"').split(',').next().unwrap_or("");
                let mut candidates = Vec::new();
                if !location.trim().is_empty() {
                    candidates.push(PathBuf::from(location).join("vesktop.exe"));
                }
                if !icon.trim().is_empty() {
                    candidates.push(PathBuf::from(icon));
                }
                for candidate in candidates {
                    let Ok(mut install) = cdp_launch::custom_executable_installation(
                        &cdp_launch::ProviderId::vesktop(),
                        candidate,
                    ) else {
                        continue;
                    };
                    if install.validation == cdp_launch::ValidationState::Valid {
                        install.source = cdp_launch::DiscoverySource::OsMetadata;
                        installs.push(install);
                        break;
                    }
                }
            }
        }
    }
    installs
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdp_launch::{
        DiscordChannel, InstallationId, LaunchOutcome, LaunchResult, LaunchSelector, ProviderId,
        SessionOwnership, VariantId,
    };

    #[test]
    fn generic_selection_uses_the_frontend_camel_case_contract() {
        let provider = serde_json::to_value(LaunchSelector::Provider {
            provider_id: ProviderId::vesktop(),
            variant_id: None,
        })
        .unwrap();
        let installation = serde_json::to_value(LaunchSelector::Installation {
            installation_id: InstallationId("vencord.vesktop:test".into()),
        })
        .unwrap();

        assert_eq!(provider["kind"], "provider");
        assert_eq!(provider["providerId"], "vencord.vesktop");
        assert!(provider.get("provider_id").is_none());
        assert_eq!(installation["installationId"], "vencord.vesktop:test");
    }

    #[test]
    fn launch_dto_keeps_legacy_fields_and_adds_real_identity() {
        let dto = DiscordCdpLaunchResultDto::from(LaunchResult {
            outcome: LaunchOutcome::Spawned,
            launched_path: PathBuf::from("C:\\Discord\\Discord.exe"),
            channel: DiscordChannel::Stable,
            port: 9223,
            pid: Some(1234),
            cdp_connected: true,
            provider_id: ProviderId::official_discord(),
            installation_id: Some(InstallationId("discord.official:test".into())),
            variant_id: Some(VariantId("stable".into())),
            ownership: SessionOwnership::Managed,
        });
        let value = serde_json::to_value(dto).unwrap();
        assert_eq!(value["launched_path"], "C:\\Discord\\Discord.exe");
        assert_eq!(value["cdp_connected"], true);
        assert_eq!(value["channel"], "stable");
        assert_eq!(value["providerId"], "discord.official");
        assert_eq!(value["ownership"], "managed");
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
        assert!(
            dto.official_installed
                && dto.vesktop_installed
                && dto.stable_installed
                && dto.canary_installed
                && !dto.ptb_installed
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

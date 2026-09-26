use crate::DiscordChannel;
use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_CDP_PORT: u16 = 9223;
pub const OFFICIAL_DISCORD_PROVIDER_ID: &str = "discord.official";
pub const VESKTOP_PROVIDER_ID: &str = "vencord.vesktop";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn official_discord() -> Self {
        Self(OFFICIAL_DISCORD_PROVIDER_ID.to_string())
    }

    pub fn vesktop() -> Self {
        Self(VESKTOP_PROVIDER_ID.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct VariantId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct InstallationId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum DiscoverySource {
    User,
    RunningProcess,
    OsMetadata,
    StandardPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum ValidationState {
    Valid,
    Missing,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ClientCapabilities {
    pub cdp: bool,
    pub local_token: bool,
    pub restore_normal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", rename_all = "camelCase"))]
pub enum LaunchTarget {
    Executable {
        path: PathBuf,
        #[cfg_attr(feature = "serde", serde(rename = "workingDir"))]
        working_dir: PathBuf,
        #[cfg_attr(feature = "serde", serde(default, rename = "prefixArgs"))]
        prefix_args: Vec<String>,
    },
    MacBundle {
        #[cfg_attr(feature = "serde", serde(rename = "bundlePath"))]
        bundle_path: PathBuf,
        #[cfg_attr(feature = "serde", serde(rename = "executablePath"))]
        executable_path: PathBuf,
    },
    Flatpak {
        #[cfg_attr(feature = "serde", serde(rename = "appId"))]
        app_id: String,
        command: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ClientInstallation {
    pub id: InstallationId,
    pub provider_id: ProviderId,
    pub variant_id: Option<VariantId>,
    pub display_name: String,
    pub source: DiscoverySource,
    pub launch_target: LaunchTarget,
    pub capabilities: ClientCapabilities,
    pub validation: ValidationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", rename_all = "camelCase"))]
pub enum LaunchSelector {
    #[default]
    Auto,
    Provider {
        #[cfg_attr(feature = "serde", serde(rename = "providerId"))]
        provider_id: ProviderId,
        #[cfg_attr(feature = "serde", serde(rename = "variantId"))]
        variant_id: Option<VariantId>,
    },
    Installation {
        #[cfg_attr(feature = "serde", serde(rename = "installationId"))]
        installation_id: InstallationId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum SessionOwnership {
    Managed,
    ExternalAttached,
    AmbiguousExternal,
    #[default]
    Unknown,
}

/// Which desktop client Helper should extract from or attach to.
///
/// This is not a Discord release channel. Vesktop hosts discord.com; official
/// Discord still uses Stable / PTB / Canary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesktopClientPreference {
    #[default]
    Auto,
    Official,
    Vesktop,
}

impl DesktopClientPreference {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Official => "official",
            Self::Vesktop => "vesktop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdpPortOwner {
    None,
    Official,
    Vesktop,
    Other,
}

impl CdpPortOwner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Official => "official",
            Self::Vesktop => "vesktop",
            Self::Other => "other",
        }
    }
}

/// Manual proxy endpoints exposed by a Linux desktop session.
///
/// The data model is platform-independent so consumers can test proxy client
/// construction without requiring a running Linux desktop. Discovery remains
/// Linux-only.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct LinuxDesktopProxySettings {
    pub http: Option<String>,
    pub https: Option<String>,
    pub all: Option<String>,
    pub no_proxy: Option<String>,
}

#[cfg(target_os = "linux")]
impl LinuxDesktopProxySettings {
    pub(crate) fn has_proxy(&self) -> bool {
        self.http.is_some() || self.https.is_some() || self.all.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordInstall {
    pub channel: DiscordChannel,
    pub executable_path: PathBuf,
    pub working_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscordLaunchMode {
    Normal,
    Cdp { port: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RunningCdpSession {
    pub channel: DiscordChannel,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreFailure {
    pub channel: DiscordChannel,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreResult {
    pub restored: Vec<DiscordChannel>,
    pub failures: Vec<RestoreFailure>,
}

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub port: u16,
    pub channel: Option<DiscordChannel>,
    pub client: DesktopClientPreference,
    /// Exact provider installation selected by the caller. Legacy callers can
    /// leave this empty and continue using `client` + `channel`.
    pub installation: Option<ClientInstallation>,
    pub restart_existing: bool,
    pub wait_for_cdp: bool,
    pub shutdown_timeout: Duration,
    pub readiness_timeout: Duration,
    pub poll_interval: Duration,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            port: DEFAULT_CDP_PORT,
            channel: None,
            client: DesktopClientPreference::Auto,
            installation: None,
            restart_existing: false,
            wait_for_cdp: true,
            shutdown_timeout: Duration::from_secs(8),
            readiness_timeout: Duration::from_secs(15),
            poll_interval: Duration::from_millis(500),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchOutcome {
    AlreadyAvailable,
    Spawned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchResult {
    pub outcome: LaunchOutcome,
    pub launched_path: PathBuf,
    pub channel: DiscordChannel,
    pub port: u16,
    pub pid: Option<u32>,
    pub cdp_connected: bool,
    pub provider_id: ProviderId,
    pub installation_id: Option<InstallationId>,
    pub variant_id: Option<VariantId>,
    pub ownership: SessionOwnership,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct DesktopCdpSession {
    pub provider_id: ProviderId,
    pub installation_id: Option<InstallationId>,
    pub variant_id: Option<VariantId>,
    pub port: u16,
    pub ownership: SessionOwnership,
    pub executable_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CdpProbeStatus {
    Unreachable,
    PortOccupied,
    CdpWithoutDiscordTarget,
    DiscordReady { target_title: Option<String> },
}

impl CdpProbeStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::PortOccupied => "occupiedNonCdp",
            Self::CdpWithoutDiscordTarget => "cdpWithoutDiscordTarget",
            Self::DiscordReady { .. } => "discordReady",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdpProbeObservation {
    pub status: CdpProbeStatus,
    pub target_count: Option<usize>,
    pub discord_target_count: Option<usize>,
    pub main_renderer_found: Option<bool>,
}

impl From<CdpProbeStatus> for CdpProbeObservation {
    fn from(status: CdpProbeStatus) -> Self {
        let main_renderer_found = matches!(status, CdpProbeStatus::DiscordReady { .. });
        Self {
            status,
            target_count: None,
            discord_target_count: None,
            main_renderer_found: Some(main_renderer_found),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdpTargetClassification {
    NotPage,
    AboutBlank,
    Updater,
    DiscordAuxiliary,
    DiscordMainRenderer,
    DiscordOtherRenderer,
    NonDiscordPage,
    MissingWebSocketDebuggerUrl,
}

impl CdpTargetClassification {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotPage => "notPage",
            Self::AboutBlank => "aboutBlank",
            Self::Updater => "updater",
            Self::DiscordAuxiliary => "discordAuxiliary",
            Self::DiscordMainRenderer => "discordMainRenderer",
            Self::DiscordOtherRenderer => "discordOtherRenderer",
            Self::NonDiscordPage => "nonDiscordPage",
            Self::MissingWebSocketDebuggerUrl => "missingWebSocketDebuggerUrl",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdpDiagnosticTarget {
    pub id: String,
    pub target_type: String,
    pub title: String,
    pub url: String,
    pub has_web_socket_debugger_url: bool,
    pub is_discord_target: bool,
    pub is_auxiliary_window: bool,
    pub is_main_renderer: bool,
    pub classification: CdpTargetClassification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailedCdpProbeResult {
    pub status: CdpProbeStatus,
    pub port_listening: bool,
    pub http_reachable: bool,
    pub http_status: Option<u16>,
    pub response_parseable: bool,
    pub targets: Vec<CdpDiagnosticTarget>,
}

impl DetailedCdpProbeResult {
    pub fn observation(&self) -> CdpProbeObservation {
        CdpProbeObservation {
            status: self.status.clone(),
            target_count: Some(self.targets.len()),
            discord_target_count: Some(
                self.targets
                    .iter()
                    .filter(|target| target.is_discord_target)
                    .count(),
            ),
            main_renderer_found: Some(self.targets.iter().any(|target| target.is_main_renderer)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdpDiagnosticProcess {
    pub pid: u32,
    pub process_name: String,
    pub provider_id: Option<ProviderId>,
    pub installation_id: Option<InstallationId>,
    pub executable_path: Option<PathBuf>,
    pub is_selected_installation: bool,
    pub has_remote_debugging_port_arg: bool,
    pub remote_debugging_port: Option<u16>,
    pub start_time: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct CdpTarget {
    #[cfg_attr(feature = "serde", serde(default))]
    pub id: String,
    #[cfg_attr(feature = "serde", serde(default, rename = "type"))]
    pub target_type: String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub title: String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub url: String,
    #[cfg_attr(feature = "serde", serde(default, rename = "webSocketDebuggerUrl"))]
    pub web_socket_debugger_url: Option<String>,
}

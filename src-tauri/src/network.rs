mod credentials;

use credentials::{Credentials, Edit, NativeVault, Vault};
use hyper_util::client::proxy::matcher::Matcher;
use reqwest::{Client, ClientBuilder, Proxy, Url};
use serde::{Deserialize, Serialize};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    io::Write,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

const SYSTEM_REFRESH: Duration = Duration::from_secs(5);
pub(crate) const IP_ENDPOINT: &str = "https://my.ippure.com/v1/info";
type CommandResult<T> = Result<T, &'static str>;

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    System,
    Direct,
    Manual,
}

#[derive(Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Settings {
    mode: Mode,
    address: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
struct StoredSettings {
    #[serde(flatten)]
    settings: Settings,
    #[serde(skip_serializing_if = "Option::is_none")]
    credential_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    retired_credentials: Vec<String>,
}

impl StoredSettings {
    fn validated(mut self) -> CommandResult<Self> {
        self.settings = self.settings.validated()?;
        if self.settings.mode != Mode::Manual {
            self.retired_credentials.extend(self.credential_ref.take());
        }
        self.retired_credentials
            .retain(|reference| Some(reference) != self.credential_ref.as_ref());
        if self
            .credential_ref
            .iter()
            .chain(&self.retired_credentials)
            .any(|reference| uuid::Uuid::parse_str(reference).is_err())
        {
            return Err("storage_error");
        }
        Ok(self)
    }
}

impl Settings {
    fn validate_credentials(&self, credentials: &Credentials) -> CommandResult<()> {
        let (scheme, _) = self
            .address
            .as_deref()
            .and_then(|url| url.split_once("://"))
            .ok_or("invalid_address")?;
        credentials.validate(scheme)
    }

    fn validated(mut self) -> CommandResult<Self> {
        if self.mode != Mode::Manual {
            self.address = None;
            return Ok(self);
        }

        let raw = self.address.as_deref().unwrap_or("").trim();
        let mut address = Url::parse(raw).map_err(|_| "invalid_address")?;
        let authority = raw.split_once("://").map(|(_, rest)| rest).unwrap_or("");
        let authority = authority.strip_suffix('/').unwrap_or(authority);
        if !matches!(address.scheme(), "http" | "https" | "socks5" | "socks5h")
            || address.host().is_none()
            || proxy_port(&address).is_none_or(|port| port == 0)
            || authority.is_empty()
            || authority.contains(['@', '/', '\\'])
            || authority.chars().any(char::is_whitespace)
            || !address.username().is_empty()
            || address.password().is_some()
            || !matches!(address.path(), "" | "/")
            || address.query().is_some()
            || address.fragment().is_some()
        {
            return Err("invalid_address");
        }

        if matches!(address.scheme(), "socks5" | "socks5h") {
            address
                .set_port(proxy_port(&address))
                .map_err(|_| "invalid_address")?;
            address.set_path("");
        }
        self.address = Some(address.to_string());
        Ok(self)
    }
}

#[derive(Clone, Default, Hash)]
struct Environment {
    http: Option<String>,
    https: Option<String>,
    all: Option<String>,
    no: Option<String>,
}

impl Environment {
    fn current() -> Self {
        let mut environment = Self::from_lookup(|name| std::env::var(name).ok());
        if std::env::var_os("REQUEST_METHOD").is_some() {
            environment.http = None;
        }
        environment
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let read = |upper, lower| {
            [upper, lower].into_iter().find_map(|name| {
                lookup(name)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
        };
        Self {
            http: read("HTTP_PROXY", "http_proxy"),
            https: read("HTTPS_PROXY", "https_proxy"),
            all: read("ALL_PROXY", "all_proxy"),
            no: read("NO_PROXY", "no_proxy"),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Source {
    Direct,
    Environment,
    System,
    Manual,
}

#[derive(Clone, Serialize)]
struct RouteStatus {
    source: Source,
    endpoint: Option<String>,
}

#[derive(Clone)]
struct SystemProxy {
    fingerprint: u64,
    // Explicit combined rules are needed only on Linux when GNOME provides a proxy.
    explicit: Option<Environment>,
    discord: RouteStatus,
    pac_configured: bool,
}

fn desktop_rules(
    environment: &Environment,
    desktop: &discord_cdp_launch_core::LinuxDesktopProxySettings,
) -> Environment {
    let select = |candidates: [Option<&String>; 4]| {
        let probe = "http://discord.com".parse().expect("static proxy probe");
        candidates
            .into_iter()
            .flatten()
            .find(|address| {
                Proxy::http(address.as_str()).is_ok()
                    && Matcher::builder()
                        .http(address.as_str())
                        .build()
                        .intercept(&probe)
                        .is_some()
            })
            .cloned()
    };
    Environment {
        http: select([
            environment.http.as_ref(),
            environment.all.as_ref(),
            desktop.http.as_ref(),
            desktop.all.as_ref(),
        ]),
        https: select([
            environment.https.as_ref(),
            environment.all.as_ref(),
            desktop.https.as_ref(),
            desktop.all.as_ref(),
        ]),
        all: None,
        no: environment.no.clone().or_else(|| desktop.no_proxy.clone()),
    }
}

impl SystemProxy {
    fn detect() -> CommandResult<Self> {
        let environment = Environment::current();
        #[cfg(target_os = "linux")]
        let (desktop, pac_configured) =
            discord_cdp_launch_core::linux_desktop_proxy_config().map_err(|_| "client_error")?;
        #[cfg(not(target_os = "linux"))]
        let desktop: Option<discord_cdp_launch_core::LinuxDesktopProxySettings> = None;

        let mut hasher = DefaultHasher::new();
        environment.hash(&mut hasher);
        desktop.hash(&mut hasher);
        #[cfg(not(target_os = "linux"))]
        let pac_configured = platform_fingerprint(&mut hasher)?;
        let explicit = desktop
            .as_ref()
            .map(|desktop| desktop_rules(&environment, desktop));
        let matcher = if let Some(rules) = &explicit {
            Matcher::builder()
                .http(rules.http.as_deref().unwrap_or(""))
                .https(rules.https.as_deref().unwrap_or(""))
                .no(rules.no.as_deref().unwrap_or(""))
                .build()
        } else {
            Matcher::from_system()
        };
        let environment_matcher = Matcher::from_env();
        let target = "https://discord.com".parse().expect("static Discord URI");
        let route = matcher.intercept(&target);
        let discord = match route {
            Some(proxy) => RouteStatus {
                source: if environment_matcher
                    .intercept(&target)
                    .is_some_and(|environment_proxy| environment_proxy.uri() == proxy.uri())
                {
                    Source::Environment
                } else {
                    Source::System
                },
                endpoint: display_endpoint(&proxy.uri().to_string()),
            },
            None => RouteStatus {
                source: Source::Direct,
                endpoint: None,
            },
        };

        for destination in ["http://discord.com", "https://discord.com"] {
            let target = destination.parse().expect("static Discord URI");
            matcher
                .intercept(&target)
                .map(|proxy| proxy.uri().to_string())
                .hash(&mut hasher);
        }
        pac_configured.hash(&mut hasher);
        Ok(Self {
            fingerprint: hasher.finish(),
            explicit,
            discord,
            pac_configured,
        })
    }
}

#[cfg(windows)]
fn platform_fingerprint(hasher: &mut DefaultHasher) -> CommandResult<bool> {
    let Ok(settings) = windows_registry::CURRENT_USER
        .open("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
    else {
        return Ok(false);
    };
    for key in ["ProxyServer", "ProxyOverride", "AutoConfigURL"] {
        settings.get_string(key).ok().hash(hasher);
    }
    for key in ["ProxyEnable", "AutoDetect"] {
        settings.get_u32(key).ok().hash(hasher);
    }
    Ok(settings
        .get_string("AutoConfigURL")
        .is_ok_and(|url| !url.trim().is_empty())
        || settings.get_u32("AutoDetect").unwrap_or(0) == 1)
}

#[cfg(target_os = "macos")]
fn platform_fingerprint(hasher: &mut DefaultHasher) -> CommandResult<bool> {
    let output = discord_cdp_launch_core::proxy_command_output(
        std::process::Command::new("/usr/sbin/scutil").arg("--proxy"),
    )
    .map_err(|_| "client_error")?;
    output.hash(hasher);
    Ok(output.as_deref().is_some_and(|bytes| {
        String::from_utf8_lossy(bytes)
            .lines()
            .filter_map(|line| line.split_once(':'))
            .any(|(name, value)| {
                matches!(
                    name.trim(),
                    "ProxyAutoConfigEnable" | "ProxyAutoDiscoveryEnable"
                ) && value.trim() == "1"
            })
    }))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn platform_fingerprint(_: &mut DefaultHasher) -> CommandResult<bool> {
    Ok(false)
}

#[derive(Clone)]
struct Route {
    settings: Settings,
    credentials: Option<Arc<Credentials>>,
    system: Option<SystemProxy>,
    revision: u64,
}

impl Route {
    fn apply(&self, builder: ClientBuilder) -> CommandResult<ClientBuilder> {
        match self.settings.mode {
            Mode::Direct => Ok(builder.no_proxy()),
            Mode::Manual => {
                let address = self.settings.address.as_deref().ok_or("invalid_address")?;
                let proxy = Proxy::all(address).map_err(|_| "invalid_address")?;
                let proxy = match &self.credentials {
                    Some(credentials) => credentials.apply(proxy),
                    None => proxy,
                };
                Ok(builder.proxy(proxy))
            }
            Mode::System => {
                let Some(rules) = self
                    .system
                    .as_ref()
                    .and_then(|system| system.explicit.as_ref())
                else {
                    return Ok(builder);
                };
                let no_proxy = rules.no.as_deref().and_then(reqwest::NoProxy::from_string);
                let mut builder = builder.no_proxy();
                if let Some(address) = &rules.http {
                    builder = builder.proxy(
                        Proxy::http(address)
                            .map_err(|_| "client_error")?
                            .no_proxy(no_proxy.clone()),
                    );
                }
                if let Some(address) = &rules.https {
                    builder = builder.proxy(
                        Proxy::https(address)
                            .map_err(|_| "client_error")?
                            .no_proxy(no_proxy),
                    );
                }
                Ok(builder)
            }
        }
    }

    fn build(&self, builder: ClientBuilder) -> CommandResult<Client> {
        self.apply(builder)?.build().map_err(|_| "client_error")
    }
}

#[derive(Clone)]
struct NetworkState {
    path: Option<PathBuf>,
    settings: Option<StoredSettings>,
    credentials: Option<Arc<Credentials>>,
    system: Option<(Instant, SystemProxy)>,
    revision: u64,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            path: None,
            settings: Some(StoredSettings::default()),
            credentials: None,
            system: None,
            revision: 0,
        }
    }
}

impl NetworkState {
    fn load(&mut self, path: PathBuf) {
        self.settings = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<StoredSettings>(&bytes)
                .ok()
                .and_then(|settings| settings.validated().ok()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Some(StoredSettings::default())
            }
            Err(_) => None,
        };
        self.path = Some(path);
        self.credentials = None;
        self.system = None;
        self.revision += 1;
    }

    fn retained_reference(&self, settings: &Settings) -> CommandResult<Option<&str>> {
        let Some(stored) = &self.settings else {
            return Ok(None);
        };
        let reference = stored.credential_ref.as_deref();
        if reference.is_some() && &stored.settings != settings {
            return Err("credential_endpoint_changed");
        }
        Ok(reference)
    }

    fn saved_credentials(
        &mut self,
        settings: &Settings,
        vault: &impl Vault,
    ) -> CommandResult<Option<Arc<Credentials>>> {
        let Some(reference) = self.retained_reference(settings)? else {
            return Ok(None);
        };
        if let Some(credentials) = &self.credentials {
            return Ok(Some(credentials.clone()));
        }
        let credentials = Arc::new(vault.read(reference)?);
        settings
            .validate_credentials(&credentials)
            .map_err(|_| "credential_store_error")?;
        self.credentials = Some(credentials.clone());
        Ok(Some(credentials))
    }

    fn save(&mut self, settings: Settings, edit: Edit, vault: &impl Vault) -> CommandResult<()> {
        let settings = settings.validated()?;
        let path = self.path.as_deref().ok_or("storage_error")?;
        let previous_ref = self
            .settings
            .as_ref()
            .and_then(|record| record.credential_ref.as_deref())
            .map(str::to_owned);
        let edit = if settings.mode == Mode::Manual {
            edit
        } else {
            Edit::Remove
        };
        let (credential_ref, replacement, reservation) = match edit {
            Edit::Keep => (
                self.retained_reference(&settings)?.map(str::to_owned),
                None,
                None,
            ),
            Edit::Remove => (None, None, None),
            Edit::Replace { credentials } => {
                settings.validate_credentials(&credentials)?;
                let reference = uuid::Uuid::new_v4().to_string();
                // Record the reference before creating a secret. A failed save or a
                // crash can then be recovered without inspecting the credential store.
                let reservation =
                    reserve_credential(path, &reference).map_err(|_| "storage_error")?;
                if let Err(error) = vault.write(&reference, &credentials) {
                    drop(reservation);
                    cleanup_reservation(path, &reference, false, vault);
                    return Err(error);
                }
                (
                    Some(reference),
                    Some(Arc::new(credentials)),
                    Some(reservation),
                )
            }
        };
        let changed = previous_ref != credential_ref;
        let mut retired_credentials = self
            .settings
            .as_ref()
            .map(|stored| stored.retired_credentials.clone())
            .unwrap_or_default();
        if changed {
            retired_credentials.extend(previous_ref);
        }
        let next = StoredSettings {
            settings,
            credential_ref,
            retired_credentials,
        };
        let written = write_settings(path, &next);
        drop(reservation);
        if written.is_err() {
            if let (Some(_), Some(reference)) = (&replacement, &next.credential_ref) {
                cleanup_reservation(path, reference, false, vault);
            }
            return Err("storage_error");
        }
        if self.settings.as_ref() != Some(&next) {
            if changed {
                self.credentials = replacement;
            }
            self.system = None;
            self.revision += 1;
            self.settings = Some(next);
        }
        Ok(())
    }

    fn finish_credential_cleanup(&mut self, vault: &impl Vault) {
        let (Some(path), Some(stored)) = (&self.path, &self.settings) else {
            return;
        };
        for reference in credential_reservations(path) {
            let active = stored.credential_ref.as_deref() == Some(reference.as_str());
            cleanup_reservation(path, &reference, active, vault);
        }
        let mut updated = stored.clone();
        updated
            .retired_credentials
            .retain(|reference| vault.remove(reference).is_err());
        if updated.retired_credentials != stored.retired_credentials
            && write_settings(path, &updated).is_ok()
        {
            self.settings = Some(updated);
        }
    }

    fn draft_route(
        &mut self,
        settings: Settings,
        edit: Edit,
        vault: &impl Vault,
    ) -> CommandResult<Route> {
        let settings = settings.validated()?;
        let credentials = if settings.mode != Mode::Manual {
            None
        } else {
            match edit {
                Edit::Keep => self.saved_credentials(&settings, vault)?,
                Edit::Remove => None,
                Edit::Replace { credentials } => {
                    settings.validate_credentials(&credentials)?;
                    Some(Arc::new(credentials))
                }
            }
        };
        self.route_for(settings, credentials)
    }

    fn route_for(
        &mut self,
        settings: Settings,
        credentials: Option<Arc<Credentials>>,
    ) -> CommandResult<Route> {
        let system = (settings.mode == Mode::System)
            .then(|| self.system_proxy(Instant::now(), SystemProxy::detect))
            .transpose()?;
        Ok(Route {
            settings,
            credentials,
            system,
            revision: self.revision,
        })
    }

    fn system_proxy(
        &mut self,
        now: Instant,
        detect: impl FnOnce() -> CommandResult<SystemProxy>,
    ) -> CommandResult<SystemProxy> {
        if let Some((checked_at, system)) = &self.system {
            if now.saturating_duration_since(*checked_at) < SYSTEM_REFRESH {
                return Ok(system.clone());
            }
        }
        let system = match detect() {
            Ok(system) => system,
            Err(error) => self
                .system
                .as_ref()
                .map(|(_, system)| system.clone())
                .ok_or(error)?,
        };
        if self
            .system
            .as_ref()
            .is_some_and(|(_, previous)| previous.fingerprint != system.fingerprint)
        {
            self.revision += 1;
        }
        self.system = Some((now, system.clone()));
        Ok(system)
    }

    fn route(&mut self) -> CommandResult<Route> {
        let settings = self
            .settings
            .as_ref()
            .ok_or("storage_error")?
            .settings
            .clone();
        let credentials = self.saved_credentials(&settings, &NativeVault)?;
        self.route_for(settings, credentials)
    }

    fn status(&self) -> Status {
        let stored = self.settings.clone().unwrap_or_default();
        let settings = stored.settings;
        let system = self.system.as_ref().map(|(_, system)| system);
        let discord = match settings.mode {
            Mode::System => system.as_ref().map(|system| system.discord.clone()),
            Mode::Direct => Some(RouteStatus {
                source: Source::Direct,
                endpoint: None,
            }),
            Mode::Manual => Some(RouteStatus {
                source: Source::Manual,
                endpoint: settings.address.as_deref().and_then(display_endpoint),
            }),
        };
        Status {
            settings,
            discord,
            pac_configured: system.is_some_and(|system| system.pac_configured),
            storage_invalid: self.settings.is_none(),
            has_credentials: stored.credential_ref.is_some(),
            credential_cleanup_pending: !stored.retired_credentials.is_empty()
                || self.path.as_ref().is_some_and(|path| {
                    credential_reservations(path).any(|reference| {
                        stored.credential_ref.as_deref() != Some(reference.as_str())
                    })
                }),
        }
    }
}

fn proxy_port(url: &Url) -> Option<u16> {
    url.port_or_known_default()
        .or_else(|| matches!(url.scheme(), "socks5" | "socks5h").then_some(1080))
}

fn display_endpoint(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    Some(format!("{}:{}", url.host()?, proxy_port(&url)?))
}

fn write_settings(path: &Path, settings: &StoredSettings) -> std::io::Result<()> {
    let parent = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec_pretty(settings)?)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

const CREDENTIAL_RESERVATION_PREFIX: &str = "network-credential-";
const CREDENTIAL_RESERVATION_SUFFIX: &str = ".pending";

fn reservation_path(path: &Path, reference: &str) -> PathBuf {
    path.with_file_name(format!(
        "{CREDENTIAL_RESERVATION_PREFIX}{reference}{CREDENTIAL_RESERVATION_SUFFIX}"
    ))
}

fn reserve_credential(path: &Path, reference: &str) -> std::io::Result<std::fs::File> {
    let parent = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    std::fs::create_dir_all(parent)?;
    let file = tempfile::NamedTempFile::new_in(parent)?;
    file.as_file().lock()?;
    file.as_file().sync_all()?;
    Ok(file.persist_noclobber(reservation_path(path, reference))?)
}

fn credential_reservations(path: &Path) -> impl Iterator<Item = String> {
    path.parent()
        .and_then(|parent| std::fs::read_dir(parent).ok())
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.ok()?.file_name();
            let reference = name
                .to_str()?
                .strip_prefix(CREDENTIAL_RESERVATION_PREFIX)?
                .strip_suffix(CREDENTIAL_RESERVATION_SUFFIX)?;
            uuid::Uuid::parse_str(reference).ok()?;
            Some(reference.to_owned())
        })
}

fn cleanup_reservation(path: &Path, reference: &str, active: bool, vault: &impl Vault) {
    let reservation = reservation_path(path, reference);
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&reservation)
    else {
        return;
    };
    if file.try_lock().is_err() {
        return;
    }
    // Another instance may have saved the reference after this instance loaded its settings.
    let active_on_disk = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StoredSettings>(&bytes).ok())
        .is_some_and(|stored| stored.credential_ref.as_deref() == Some(reference));
    if active || active_on_disk || vault.remove(reference).is_ok() {
        drop(file);
        let _ = std::fs::remove_file(&reservation);
    }
}

static STATE: LazyLock<Mutex<NetworkState>> = LazyLock::new(|| Mutex::new(NetworkState::default()));

pub(crate) fn initialize(path: PathBuf) {
    STATE.lock().unwrap().load(path);
}

static SYSTEM_PROBE: Mutex<()> = Mutex::new(());
static CREDENTIAL_READ: Mutex<()> = Mutex::new(());
static SETTINGS_SAVE: Mutex<()> = Mutex::new(());

impl NetworkState {
    fn mode(&self) -> Option<Mode> {
        self.settings.as_ref().map(|stored| stored.settings.mode)
    }

    fn cached_route(&self) -> CommandResult<Option<Route>> {
        let stored = self.settings.as_ref().ok_or("storage_error")?;
        let system = if stored.settings.mode == Mode::System {
            let Some((updated, system)) = &self.system else {
                return Ok(None);
            };
            if updated.elapsed() >= SYSTEM_REFRESH {
                return Ok(None);
            }
            Some(system.clone())
        } else {
            None
        };
        if stored.credential_ref.is_some() && self.credentials.is_none() {
            return Ok(None);
        }
        Ok(Some(Route {
            settings: stored.settings.clone(),
            credentials: self.credentials.clone(),
            system,
            revision: self.revision,
        }))
    }
}

fn refresh_route(status_only: bool) -> CommandResult<Option<Route>> {
    loop {
        let mode = STATE.lock().unwrap().mode();
        if status_only && mode != Some(Mode::System) {
            return Ok(None);
        }
        let _probe = match mode {
            Some(Mode::System) => Some(SYSTEM_PROBE.lock().unwrap()),
            Some(Mode::Manual) => Some(CREDENTIAL_READ.lock().unwrap()),
            _ => None,
        };
        let mut snapshot = STATE.lock().unwrap().clone();
        if snapshot.mode() != mode {
            continue;
        }
        let revision = snapshot.revision;
        let route = if status_only {
            snapshot
                .system_proxy(Instant::now(), SystemProxy::detect)
                .map(|_| None)
        } else {
            match snapshot.cached_route()? {
                Some(route) => Ok(Some(route)),
                None => snapshot.route().map(Some),
            }
        };
        let mut state = STATE.lock().unwrap();
        if state.revision != revision {
            continue;
        }
        state.system = snapshot.system;
        state.credentials = snapshot.credentials;
        state.revision = snapshot.revision;
        return route;
    }
}

async fn active_route() -> CommandResult<Route> {
    if let Ok(state) = STATE.try_lock() {
        if let Some(route) = state.cached_route()? {
            return Ok(route);
        }
    }
    tauri::async_runtime::spawn_blocking(|| refresh_route(false))
        .await
        .map_err(|_| "client_error")??
        .ok_or("client_error")
}

#[derive(Default)]
pub(crate) struct HttpClient(Mutex<Option<(u64, Client)>>);

impl HttpClient {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub(crate) async fn get(
        &self,
        configure: impl FnOnce(ClientBuilder) -> ClientBuilder + Send + 'static,
    ) -> CommandResult<Client> {
        self.client(active_route().await?, configure, false).await
    }

    pub(crate) async fn reconnect(
        &self,
        configure: impl FnOnce(ClientBuilder) -> ClientBuilder + Send + 'static,
    ) -> CommandResult<Client> {
        self.client(active_route().await?, configure, true).await
    }

    async fn client(
        &self,
        route: Route,
        configure: impl FnOnce(ClientBuilder) -> ClientBuilder + Send + 'static,
        reconnect: bool,
    ) -> CommandResult<Client> {
        let revision = route.revision;
        if let Some((cached_revision, client)) = &*self.0.lock().unwrap() {
            if *cached_revision > revision || (!reconnect && *cached_revision == revision) {
                return Ok(client.clone());
            }
        }
        let client =
            tauri::async_runtime::spawn_blocking(move || route.build(configure(Client::builder())))
                .await
                .map_err(|_| "client_error")??;
        let mut cached = self.0.lock().unwrap();
        if let Some((cached_revision, current)) = &*cached {
            if *cached_revision > revision || (!reconnect && *cached_revision == revision) {
                return Ok(current.clone());
            }
        }
        *cached = Some((revision, client.clone()));
        Ok(client)
    }
}

#[derive(Serialize)]
pub(crate) struct Status {
    settings: Settings,
    discord: Option<RouteStatus>,
    pac_configured: bool,
    storage_invalid: bool,
    has_credentials: bool,
    credential_cleanup_pending: bool,
}

#[tauri::command]
pub(crate) async fn get_network_settings() -> CommandResult<Status> {
    tauri::async_runtime::spawn_blocking(|| {
        let _ = refresh_route(true);
        let snapshot = STATE.lock().unwrap().clone();
        Ok(snapshot.status())
    })
    .await
    .map_err(|_| "storage_error")?
}

#[tauri::command]
pub(crate) async fn save_network_settings(
    settings: Settings,
    credentials: Option<Edit>,
) -> CommandResult<Status> {
    tauri::async_runtime::spawn_blocking(move || {
        let _save = SETTINGS_SAVE.lock().unwrap();
        let mut snapshot = STATE.lock().unwrap().clone();
        let previous = snapshot.settings.clone();
        snapshot.save(settings, credentials.unwrap_or(Edit::Keep), &NativeVault)?;
        {
            let mut state = STATE.lock().unwrap();
            if snapshot
                .settings
                .as_ref()
                .map(|saved| (&saved.settings, &saved.credential_ref))
                != previous
                    .as_ref()
                    .map(|saved| (&saved.settings, &saved.credential_ref))
            {
                state.revision += 1;
                state.system = None;
                state.credentials = snapshot.credentials.clone();
            }
            state.settings = snapshot.settings.clone();
        }
        snapshot.finish_credential_cleanup(&NativeVault);
        STATE.lock().unwrap().settings = snapshot.settings;
        let _ = refresh_route(true);
        let snapshot = STATE.lock().unwrap().clone();
        Ok(snapshot.status())
    })
    .await
    .map_err(|_| "storage_error")?
}

#[derive(Deserialize)]
struct IpResponse {
    ip: IpAddr,
}

#[derive(Serialize)]
pub(crate) struct ConnectionTest {
    ip: IpAddr,
    elapsed_ms: u64,
}

fn connection_error(error: reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() || error.is_request() {
        "connection_error"
    } else {
        "unexpected_response"
    }
}

#[tauri::command]
pub(crate) async fn test_network_connection(
    settings: Settings,
    credentials: Option<Edit>,
) -> CommandResult<ConnectionTest> {
    let route = tauri::async_runtime::spawn_blocking(move || {
        STATE
            .lock()
            .unwrap()
            .draft_route(settings, credentials.unwrap_or(Edit::Keep), &NativeVault)
    })
    .await
    .map_err(|_| "client_error")??;
    let client = route.build(
        Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none()),
    )?;
    let started = Instant::now();
    let response = client
        .get(IP_ENDPOINT)
        .send()
        .await
        .map_err(connection_error)?;
    if !response.status().is_success() {
        return Err("http_error");
    }
    let result = response
        .json::<IpResponse>()
        .await
        .map_err(connection_error)?;
    Ok(ConnectionTest {
        ip: result.ip,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QuestDataset {
    Quests,
    Regions,
}

#[tauri::command]
pub(crate) async fn fetch_global_quest_dataset(
    dataset: QuestDataset,
) -> CommandResult<serde_json::Value> {
    let url = match dataset {
        QuestDataset::Quests => "https://api.discordquest.com/api/quests",
        QuestDataset::Regions => "https://api.discordquest.com/api/regions",
    };
    fetch_public_json(url, "application/json").await
}

#[tauri::command]
pub(crate) async fn fetch_github_releases(prerelease: bool) -> CommandResult<serde_json::Value> {
    let url = if prerelease {
        "https://api.github.com/repos/Masterain98/discord-quest-helper/releases"
    } else {
        "https://api.github.com/repos/Masterain98/discord-quest-helper/releases/latest"
    };
    fetch_public_json(url, "application/vnd.github.v3+json").await
}

async fn fetch_public_json(url: &str, accept: &str) -> CommandResult<serde_json::Value> {
    static PUBLIC_API: HttpClient = HttpClient::new();
    let client = PUBLIC_API
        .get(|builder| {
            builder
                .timeout(Duration::from_secs(20))
                .user_agent("Discord-Quest-Helper")
        })
        .await?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, accept)
        .send()
        .await
        .map_err(connection_error)?;
    if !response.status().is_success() {
        return Err("http_error");
    }
    response.json().await.map_err(connection_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    fn settings(mode: Mode, address: Option<String>) -> Settings {
        Settings { mode, address }
    }

    fn manual(address: impl ToString) -> Settings {
        settings(Mode::Manual, Some(address.to_string()))
            .validated()
            .unwrap()
    }

    fn no_redirect(builder: ClientBuilder) -> ClientBuilder {
        builder
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
    }

    fn build(settings: Settings) -> Client {
        Route {
            settings,
            credentials: None,
            system: None,
            revision: 0,
        }
        .build(no_redirect(Client::builder()))
        .unwrap()
    }

    async fn read_headers(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            bytes.push(stream.read_u8().await.unwrap());
            assert!(bytes.len() <= 4096);
        }
        String::from_utf8(bytes).unwrap()
    }

    async fn respond(stream: &mut TcpStream, body: &str) {
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(format!("{headers}{body}").as_bytes())
            .await
            .unwrap();
    }

    #[test]
    fn validates_manual_protocols_ports_and_credentials() {
        for valid in [
            "http://localhost",
            "https://proxy:444",
            "socks5://[::1]:1081",
            "socks5h://proxy",
        ] {
            let config = manual(valid);
            assert!(display_endpoint(config.address.as_deref().unwrap()).is_some());
        }
        for invalid in [
            "localhost:7890",
            "http:localhost",
            "ftp://localhost:7890",
            "http://localhost:0",
            "http://localhost:65536",
            "http://path/home",
            "http://path/..",
            "http://proxy\\path",
            "http://proxy?x=1",
            "http://proxy#",
            "http://user:secret@proxy",
            "http://@proxy",
            "socks5://",
        ] {
            assert_eq!(
                settings(Mode::Manual, Some(invalid.into()))
                    .validated()
                    .err(),
                Some("invalid_address"),
                "{invalid}"
            );
        }
        assert_eq!(
            display_endpoint("http://user:secret@[::1]:8080").as_deref(),
            Some("[::1]:8080")
        );
    }

    #[test]
    fn saves_atomically_and_recovers_an_invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings/network.json");
        let mut state = NetworkState::default();
        state.load(path.clone());
        assert!(state.status().settings.mode == Mode::System);
        let manual = manual("http://127.0.0.1:7890/");
        state
            .save(manual.clone(), Edit::Remove, &NativeVault)
            .unwrap();
        let revision = state.revision;
        assert_eq!(
            state.save(
                settings(Mode::Manual, Some("bad".into())),
                Edit::Remove,
                &NativeVault
            ),
            Err("invalid_address")
        );
        assert_eq!(state.revision, revision);
        let mut relaunched = NetworkState::default();
        relaunched.load(path.clone());
        assert!(relaunched.settings.as_ref().map(|record| &record.settings) == Some(&manual));
        state
            .save(settings(Mode::Direct, None), Edit::Remove, &NativeVault)
            .unwrap();
        relaunched.load(path.clone());
        assert!(relaunched.status().settings.mode == Mode::Direct);
        std::fs::write(&path, b"invalid json").unwrap();
        relaunched.load(path.clone());
        assert!(relaunched.status().storage_invalid);
        assert_eq!(relaunched.route().err(), Some("storage_error"));
        relaunched
            .save(Settings::default(), Edit::Remove, &NativeVault)
            .unwrap();
        assert!(!relaunched.status().storage_invalid);
        relaunched.path = Some(path.join("cannot-write-here.json"));
        assert_eq!(
            relaunched.save(settings(Mode::Direct, None), Edit::Remove, &NativeVault),
            Err("storage_error")
        );
        assert!(relaunched.status().settings.mode == Mode::System);
    }

    #[derive(Default)]
    struct MemoryVault(
        std::cell::RefCell<std::collections::HashMap<String, (String, String)>>,
        std::cell::Cell<bool>,
    );

    impl Vault for MemoryVault {
        fn read(&self, reference: &str) -> CommandResult<Credentials> {
            self.0
                .borrow()
                .get(reference)
                .map(|(username, password)| Credentials {
                    username: username.clone(),
                    password: password.clone(),
                })
                .ok_or("credential_store_error")
        }

        fn write(&self, reference: &str, credentials: &Credentials) -> CommandResult<()> {
            self.0.borrow_mut().insert(
                reference.into(),
                (credentials.username.clone(), credentials.password.clone()),
            );
            Ok(())
        }

        fn remove(&self, reference: &str) -> CommandResult<()> {
            if self.1.get() {
                return Err("credential_store_error");
            }
            self.0.borrow_mut().remove(reference);
            Ok(())
        }
    }

    fn replace_credentials(username: &str, password: &str) -> Edit {
        Edit::Replace {
            credentials: Credentials {
                username: username.into(),
                password: password.into(),
            },
        }
    }

    #[test]
    fn credentials_stay_in_the_vault_and_are_bound_to_the_saved_endpoint() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("network.json");
        let settings = manual("http://127.0.0.1:7890");
        std::fs::write(
            &path,
            br#"{"mode":"manual","address":"http://127.0.0.1:7890"}"#,
        )
        .unwrap();
        let mut state = NetworkState::default();
        let vault = MemoryVault::default();
        state.load(path.clone());
        assert!(!state.status().has_credentials);
        let edit: Edit = serde_json::from_value(serde_json::json!({
            "action":"replace", "username":"dummy-user", "password":"dummy-password"
        }))
        .unwrap();
        state.save(settings.clone(), edit, &vault).unwrap();

        let file = std::fs::read_to_string(&path).unwrap();
        let status = serde_json::to_string(&state.status()).unwrap();
        for raw in [&file, &status] {
            assert!(!raw.contains("dummy-user") && !raw.contains("dummy-password"));
        }
        assert!(file.contains("credential_ref"));
        assert!(!status.contains("credential_ref"));
        assert!(state.status().has_credentials);
        let revision = state.revision;
        state
            .save(
                settings.clone(),
                replace_credentials("new-user", "new-password"),
                &vault,
            )
            .unwrap();
        assert!(state.revision > revision);
        state.finish_credential_cleanup(&vault);
        assert_eq!(vault.0.borrow().len(), 1);

        let mut relaunched = NetworkState::default();
        relaunched.load(path.clone());
        let route = relaunched
            .draft_route(settings.clone(), Edit::Keep, &vault)
            .unwrap();
        assert_eq!(route.credentials.as_ref().unwrap().password, "new-password");
        let changed = manual("socks5h://127.0.0.1:1080");
        assert_eq!(
            relaunched
                .draft_route(changed.clone(), Edit::Keep, &vault)
                .err(),
            Some("credential_endpoint_changed")
        );
        assert!(relaunched
            .draft_route(changed, Edit::Remove, &vault)
            .unwrap()
            .credentials
            .is_none());

        // Testing a replacement never writes it to the vault or activates it.
        let draft = relaunched
            .draft_route(
                settings.clone(),
                replace_credentials("temporary", "probe"),
                &vault,
            )
            .unwrap();
        assert_eq!(draft.credentials.as_ref().unwrap().password, "probe");
        assert_eq!(vault.0.borrow().len(), 1);
        assert_eq!(
            relaunched
                .saved_credentials(&settings, &vault)
                .unwrap()
                .unwrap()
                .password,
            "new-password"
        );

        relaunched.path = Some(path.join("cannot-write-here.json"));
        assert_eq!(
            relaunched.save(
                settings.clone(),
                replace_credentials("failed", "write"),
                &vault
            ),
            Err("storage_error")
        );
        assert_eq!(vault.0.borrow().len(), 1);
        assert_eq!(
            relaunched
                .saved_credentials(&settings, &vault)
                .unwrap()
                .unwrap()
                .password,
            "new-password"
        );

        relaunched.path = Some(path.clone());
        relaunched.save(settings, Edit::Remove, &vault).unwrap();
        relaunched.finish_credential_cleanup(&vault);
        assert!(vault.0.borrow().is_empty());
        assert!(!relaunched.status().has_credentials);
        assert!(!std::fs::read_to_string(path)
            .unwrap()
            .contains("credential_ref"));
        // A previously accepted request keeps its own immutable credential snapshot.
        assert_eq!(route.credentials.as_ref().unwrap().password, "new-password");
    }

    #[test]
    fn failed_credential_cleanup_survives_restart_without_changing_the_new_route() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("network.json");
        let vault = MemoryVault::default();
        let mut state = NetworkState::default();
        state.load(path.clone());
        state
            .save(
                manual("http://127.0.0.1:7890"),
                replace_credentials("dummy", "secret"),
                &vault,
            )
            .unwrap();

        vault.1.set(true);
        state
            .save(settings(Mode::Direct, None), Edit::Remove, &vault)
            .unwrap();
        state.finish_credential_cleanup(&vault);
        assert_eq!(vault.0.borrow().len(), 1);
        assert!(state.status().credential_cleanup_pending);
        assert!(state.status().settings.mode == Mode::Direct);

        let mut reloaded = NetworkState::default();
        reloaded.load(path);
        assert!(!reloaded.status().has_credentials);
        assert!(reloaded.status().credential_cleanup_pending);
        vault.1.set(false);
        reloaded.finish_credential_cleanup(&vault);
        assert!(vault.0.borrow().is_empty());
        assert!(!reloaded.status().credential_cleanup_pending);
    }

    #[test]
    fn failed_save_and_failed_rollback_leave_a_retryable_reservation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("network.json");
        std::fs::create_dir(&path).unwrap();
        let vault = MemoryVault::default();
        vault.1.set(true);
        let mut state = NetworkState::default();
        state.load(path.clone());
        assert_eq!(
            state.save(
                manual("http://127.0.0.1:7890"),
                replace_credentials("dummy", "secret"),
                &vault
            ),
            Err("storage_error")
        );
        assert!(state.status().credential_cleanup_pending);
        assert_eq!(vault.0.borrow().len(), 1);

        std::fs::remove_dir(&path).unwrap();
        let mut reloaded = NetworkState::default();
        reloaded.load(path);
        assert!(reloaded.status().credential_cleanup_pending);
        vault.1.set(false);
        reloaded.finish_credential_cleanup(&vault);
        assert!(vault.0.borrow().is_empty());
        assert!(!reloaded.status().credential_cleanup_pending);
    }

    #[test]
    fn equivalent_reentered_proxy_addresses_keep_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let vault = MemoryVault::default();
        let mut state = NetworkState::default();
        state.load(directory.path().join("network.json"));
        for (stored, draft) in [
            ("http://127.0.0.1:7890/", "http://127.0.0.1:7890"),
            ("socks5h://proxy:1080", "socks5h://proxy/"),
        ] {
            state
                .save(
                    manual(stored),
                    replace_credentials("dummy", "secret"),
                    &vault,
                )
                .unwrap();
            state.save(manual(draft), Edit::Keep, &vault).unwrap();
            assert!(state.status().has_credentials);
            assert!(state
                .draft_route(manual(draft), Edit::Keep, &vault)
                .unwrap()
                .credentials
                .is_some());
        }
    }

    #[test]
    fn malformed_environment_proxy_falls_back_to_valid_desktop_rules() {
        let desktop = discord_cdp_launch_core::LinuxDesktopProxySettings {
            https: Some("http://127.0.0.1:7890".into()),
            ..Default::default()
        };
        let rules = desktop_rules(
            &Environment {
                https: Some("http://".into()),
                all: Some("ftp://invalid".into()),
                ..Default::default()
            },
            &desktop,
        );
        assert_eq!(rules.https, desktop.https);
        assert!(rules.http.is_none());
    }

    #[test]
    fn credentials_are_validated_per_proxy_protocol() {
        let mut state = NetworkState::default();
        let vault = MemoryVault::default();
        for (scheme, username, password, error) in [
            (
                "http",
                "user:name",
                "pass",
                Some("invalid_http_credentials"),
            ),
            ("https", "user", "pa\nss", Some("invalid_http_credentials")),
            ("http", "user", "", None),
            ("http", "", "pass", None),
            ("socks5", "user", "", Some("invalid_socks_credentials")),
            ("socks5h", "", "pass", Some("invalid_socks_credentials")),
            ("socks5h", "user", "pass", None),
        ] {
            let route = state.draft_route(
                manual(format!("{scheme}://127.0.0.1:8080")),
                replace_credentials(username, password),
                &vault,
            );
            assert_eq!(route.err(), error);
        }
        let error = state
            .draft_route(
                manual("socks5://127.0.0.1:1080"),
                replace_credentials("ç".repeat(128).as_str(), "pw"),
                &vault,
            )
            .err();
        assert_eq!(error, Some("invalid_socks_credentials"));
    }

    #[tokio::test]
    async fn http_connect_authenticates_with_exact_proxy_credentials() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut state = NetworkState::default();
        let username = "proxy%2f-user";
        let password = "p!@#:%25";
        let route = state
            .draft_route(
                manual(format!("http://{}", listener.local_addr().unwrap())),
                replace_credentials(username, password),
                &MemoryVault::default(),
            )
            .unwrap();
        let client = route.build(no_redirect(Client::builder())).unwrap();
        let (response, observed) =
            tokio::join!(client.get("https://example.invalid").send(), async {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(3), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let headers = read_headers(&mut stream).await;
                stream
                    .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                    .await
                    .unwrap();
                headers
            });
        assert!(response.is_err());
        assert!(observed.starts_with("CONNECT example.invalid:443 HTTP/1.1"));
        let expected = format!(
            "Basic {}",
            STANDARD.encode(format!("{username}:{password}"))
        );
        let authentication = observed
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find_map(|(key, value)| {
                key.eq_ignore_ascii_case("Proxy-Authorization")
                    .then(|| value.trim())
            });
        assert_eq!(authentication, Some(expected.as_str()));
    }

    #[tokio::test]
    async fn authenticated_socks5_and_socks5h_send_exact_credentials() {
        let username = "proxy%2f:user";
        let password = "p@ss#%25";
        let mut state = NetworkState::default();
        for (scheme, target, address_kind) in [
            ("socks5", "127.0.0.1", 1),
            ("socks5h", "example.invalid", 3),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let route = state
                .draft_route(
                    manual(format!("{scheme}://{}", listener.local_addr().unwrap())),
                    replace_credentials(username, password),
                    &MemoryVault::default(),
                )
                .unwrap();
            let client = route.build(no_redirect(Client::builder())).unwrap();
            let url = format!("http://{target}:8080/test");
            let (response, _peer) = tokio::join!(
                async { client.get(&url).send().await?.text().await },
                async {
                    let (mut stream, _) =
                        tokio::time::timeout(Duration::from_secs(3), listener.accept())
                            .await
                            .unwrap()
                            .unwrap();
                    assert_eq!(stream.read_u8().await.unwrap(), 5);
                    let mut methods = vec![0; stream.read_u8().await.unwrap().into()];
                    stream.read_exact(&mut methods).await.unwrap();
                    assert!(methods.contains(&2));
                    stream.write_all(&[5, 2]).await.unwrap();
                    assert_eq!(stream.read_u8().await.unwrap(), 1);
                    for expected in [username, password] {
                        let mut field = vec![0; stream.read_u8().await.unwrap().into()];
                        stream.read_exact(&mut field).await.unwrap();
                        assert_eq!(field, expected.as_bytes());
                    }
                    stream.write_all(&[1, 0]).await.unwrap();
                    let mut request = [0; 4];
                    stream.read_exact(&mut request).await.unwrap();
                    assert_eq!(request, [5, 1, 0, address_kind]);
                    let size = if address_kind == 3 {
                        stream.read_u8().await.unwrap().into()
                    } else {
                        4
                    };
                    let mut address = vec![0; size];
                    stream.read_exact(&mut address).await.unwrap();
                    let expected: &[u8] = if address_kind == 3 {
                        target.as_bytes()
                    } else {
                        &[127, 0, 0, 1]
                    };
                    assert_eq!(address, expected);
                    assert_eq!(stream.read_u16().await.unwrap(), 8080);
                    stream
                        .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                        .await
                        .unwrap();
                    let headers = read_headers(&mut stream).await;
                    assert!(headers.starts_with("GET /test HTTP/1.1"));
                    assert!(!headers
                        .to_ascii_lowercase()
                        .contains("proxy-authorization:"));
                    respond(&mut stream, "authenticated").await;
                    stream
                },
            );
            assert_eq!(response.unwrap(), "authenticated");
        }
    }

    #[test]
    fn system_detection_updates_the_revision_after_the_refresh_interval() {
        let first = Instant::now();
        let detector = |fingerprint| SystemProxy {
            fingerprint,
            explicit: None,
            discord: RouteStatus {
                source: Source::Direct,
                endpoint: None,
            },
            pac_configured: false,
        };
        let mut state = NetworkState::default();
        assert_eq!(
            state
                .system_proxy(first, || Ok(detector(1)))
                .unwrap()
                .fingerprint,
            1
        );
        assert_eq!(
            state
                .system_proxy(first + Duration::from_secs(1), || panic!("cached"))
                .unwrap()
                .fingerprint,
            1
        );
        assert_eq!(state.revision, 0);
        assert_eq!(
            state
                .system_proxy(first + SYSTEM_REFRESH, || Ok(detector(2)))
                .unwrap()
                .fingerprint,
            2
        );
        assert_eq!(state.revision, 1);
    }

    #[test]
    fn failed_system_detection_preserves_the_last_known_route() {
        let now = Instant::now();
        let mut state = NetworkState::default();
        assert!(state.system_proxy(now, || Err("client_error")).is_err());
        let known = SystemProxy {
            fingerprint: 12,
            explicit: None,
            discord: RouteStatus {
                source: Source::System,
                endpoint: Some("proxy:7890".into()),
            },
            pac_configured: false,
        };
        state.system_proxy(now, || Ok(known)).unwrap();
        let retained = state
            .system_proxy(now + SYSTEM_REFRESH, || Err("client_error"))
            .unwrap();
        assert_eq!(retained.fingerprint, 12);
        assert_eq!(state.revision, 0);
    }

    #[tokio::test]
    async fn mixed_environment_and_desktop_proxies_apply_per_scheme_and_respect_bypass() {
        let desktop_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let environment = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let env_address = format!("http://{}", environment.local_addr().unwrap());
        let desktop_address = format!("http://{}", desktop_listener.local_addr().unwrap());
        let env = Environment::from_lookup(|name| match name {
            "HTTPS_PROXY" => Some(env_address.clone()),
            "https_proxy" => Some("http://unused.invalid".into()),
            _ => None,
        });
        let desktop = discord_cdp_launch_core::LinuxDesktopProxySettings {
            http: Some(desktop_address),
            https: Some("http://unused.invalid".into()),
            no_proxy: Some("localhost,127.0.0.1".into()),
            ..Default::default()
        };
        let route = Route {
            settings: Settings::default(),
            credentials: None,
            system: Some(SystemProxy {
                fingerprint: 0,
                explicit: Some(desktop_rules(&env, &desktop)),
                discord: RouteStatus {
                    source: Source::Environment,
                    endpoint: None,
                },
                pac_configured: false,
            }),
            revision: 0,
        };
        let client = route.build(no_redirect(Client::builder())).unwrap();
        let (http, (observed, _peer)) = tokio::join!(
            async {
                client
                    .get("http://example.invalid/path")
                    .send()
                    .await?
                    .text()
                    .await
            },
            async {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(3), desktop_listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let headers = read_headers(&mut stream).await;
                respond(&mut stream, "desktop").await;
                (headers, stream)
            }
        );
        assert!(observed.starts_with("GET http://example.invalid/path HTTP/1.1"));
        assert_eq!(http.unwrap(), "desktop");

        let (https, observed) =
            tokio::join!(client.get("https://example.invalid/path").send(), async {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(3), environment.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let headers = read_headers(&mut stream).await;
                stream
                    .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                    .await
                    .unwrap();
                headers
            });
        assert!(observed.starts_with("CONNECT example.invalid:443 HTTP/1.1"));
        assert!(https.is_err());

        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = format!("http://{}/bypass", origin.local_addr().unwrap());
        let (bypass, (observed, _peer)) = tokio::join!(
            async { client.get(target).send().await?.text().await },
            async {
                let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), origin.accept())
                    .await
                    .unwrap()
                    .unwrap();
                let headers = read_headers(&mut stream).await;
                respond(&mut stream, "bypass").await;
                (headers, stream)
            }
        );
        assert!(observed.starts_with("GET /bypass HTTP/1.1"));
        assert_eq!(bypass.unwrap(), "bypass");
    }

    #[tokio::test]
    async fn changing_the_mode_replaces_cached_clients_and_a_dead_proxy_does_not_fall_back() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = NetworkState::default();
        state.load(directory.path().join("network.json"));
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = format!("http://{}/test", origin.local_addr().unwrap());
        let proxy_address = format!("http://{}", proxy.local_addr().unwrap());
        state
            .save(manual(&proxy_address), Edit::Remove, &NativeVault)
            .unwrap();
        let state = Mutex::new(state);
        let pool = HttpClient::new();

        let proxied = pool
            .client(state.lock().unwrap().route().unwrap(), no_redirect, false)
            .await
            .unwrap();
        let (response, (observed, _peer)) = tokio::join!(
            async { proxied.get(&target).send().await?.text().await },
            async {
                let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), proxy.accept())
                    .await
                    .unwrap()
                    .unwrap();
                let headers = read_headers(&mut stream).await;
                respond(&mut stream, "proxy").await;
                (headers, stream)
            }
        );
        assert!(observed.starts_with(&format!("GET {target} HTTP/1.1")));
        assert_eq!(response.unwrap(), "proxy");

        // An unreachable configured proxy must leave the origin untouched.
        drop(proxy);
        assert!(pool
            .client(state.lock().unwrap().route().unwrap(), no_redirect, true)
            .await
            .unwrap()
            .get(&target)
            .send()
            .await
            .is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(75), origin.accept())
                .await
                .is_err()
        );

        state
            .lock()
            .unwrap()
            .save(settings(Mode::Direct, None), Edit::Remove, &NativeVault)
            .unwrap();
        let direct = pool
            .client(
                state.lock().unwrap().route().unwrap(),
                move |builder| no_redirect(builder).proxy(Proxy::all(proxy_address).unwrap()),
                false,
            )
            .await
            .unwrap();
        let (response, (observed, _peer)) = tokio::join!(
            async { direct.get(&target).send().await?.text().await },
            async {
                let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), origin.accept())
                    .await
                    .unwrap()
                    .unwrap();
                let headers = read_headers(&mut stream).await;
                respond(&mut stream, "direct").await;
                (headers, stream)
            }
        );
        assert!(observed.starts_with("GET /test HTTP/1.1"));
        assert_eq!(response.unwrap(), "direct");
    }

    #[tokio::test]
    async fn socks5_uses_a_local_address_and_socks5h_sends_the_hostname_to_the_proxy() {
        for remote_dns in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let scheme = if remote_dns { "socks5h" } else { "socks5" };
            let target = if remote_dns {
                "proxy-resolution.invalid"
            } else {
                "127.0.0.1"
            };
            let client = build(manual(format!(
                "{scheme}://{}",
                listener.local_addr().unwrap()
            )));
            let url = format!("http://{target}:8080/test");
            let (response, (headers, _peer)) = tokio::join!(
                async { client.get(&url).send().await?.text().await },
                async {
                    let (mut stream, _) =
                        tokio::time::timeout(Duration::from_secs(3), listener.accept())
                            .await
                            .unwrap()
                            .unwrap();
                    let mut greeting = [0; 3];
                    stream.read_exact(&mut greeting).await.unwrap();
                    assert_eq!(greeting, [5, 1, 0]);
                    stream.write_all(&[5, 0]).await.unwrap();
                    let mut request = [0; 4];
                    stream.read_exact(&mut request).await.unwrap();
                    assert_eq!(request, [5, 1, 0, if remote_dns { 3 } else { 1 }]);
                    if remote_dns {
                        let length = stream.read_u8().await.unwrap();
                        let mut domain = vec![0; length.into()];
                        stream.read_exact(&mut domain).await.unwrap();
                        assert_eq!(domain, target.as_bytes());
                    } else {
                        let mut address = [0; 4];
                        stream.read_exact(&mut address).await.unwrap();
                        assert_eq!(address, [127, 0, 0, 1]);
                    }
                    assert_eq!(stream.read_u16().await.unwrap(), 8080);
                    stream
                        .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                        .await
                        .unwrap();
                    let headers = read_headers(&mut stream).await;
                    respond(&mut stream, "socks").await;
                    (headers, stream)
                }
            );
            assert!(headers.starts_with("GET /test HTTP/1.1"));
            assert_eq!(response.unwrap(), "socks");
        }
    }
}

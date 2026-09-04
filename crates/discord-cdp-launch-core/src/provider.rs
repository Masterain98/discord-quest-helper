use crate::vesktop::{find_vesktop_install, vesktop_install_from_executable, VesktopInstall};
use crate::{
    find_discord_installs, ClientCapabilities, ClientInstallation, DiscordInstall, DiscoverySource,
    InstallationId, LaunchError, LaunchTarget, ProviderId, ValidationState, VariantId,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Compile-time extension point for supported Discord desktop clients.
///
/// Providers are deliberately code-shipped rather than dynamically loaded: a
/// provider can discover installations and translate them to the common launch
/// model, while process supervision and CDP verification stay centralized.
pub trait DesktopClientProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn display_name(&self) -> &'static str;
    fn discover(&self) -> Result<Vec<ClientInstallation>, LaunchError>;
    fn validate_target(&self, target: &LaunchTarget) -> ValidationState;

    fn process_matches(
        &self,
        process_name: &str,
        executable_path: Option<&Path>,
        installation: &ClientInstallation,
    ) -> bool {
        let name_matches = if self.id() == ProviderId::vesktop() {
            crate::is_vesktop_process_name(process_name)
        } else {
            process_name.to_ascii_lowercase().starts_with("discord")
        };
        name_matches
            && executable_path.is_some_and(|path| {
                target_path(&installation.launch_target)
                    .is_some_and(|target| normalized_path_key(path) == normalized_path_key(target))
            })
    }

    fn build_launch_target(
        &self,
        installation: &ClientInstallation,
    ) -> Result<LaunchTarget, LaunchError> {
        if installation.provider_id != self.id() {
            return Err(LaunchError::InvalidInstallation {
                details: "The installation belongs to a different provider.".into(),
            });
        }
        if self.validate_target(&installation.launch_target) != ValidationState::Valid {
            return Err(LaunchError::InvalidInstallation {
                details: "The provider rejected the installation launch target.".into(),
            });
        }
        Ok(installation.launch_target.clone())
    }
}

pub struct OfficialDiscordProvider;
pub struct VesktopProvider;

impl DesktopClientProvider for OfficialDiscordProvider {
    fn id(&self) -> ProviderId {
        ProviderId::official_discord()
    }

    fn display_name(&self) -> &'static str {
        "Discord"
    }

    fn discover(&self) -> Result<Vec<ClientInstallation>, LaunchError> {
        Ok(find_discord_installs()?
            .into_iter()
            .map(official_installation)
            .collect())
    }

    fn validate_target(&self, target: &LaunchTarget) -> ValidationState {
        validate_named_executable_target(
            target,
            &[
                "discord",
                "discord.exe",
                "discordptb",
                "discordptb.exe",
                "discordcanary",
                "discordcanary.exe",
            ],
        )
    }
}

impl DesktopClientProvider for VesktopProvider {
    fn id(&self) -> ProviderId {
        ProviderId::vesktop()
    }

    fn display_name(&self) -> &'static str {
        "Vesktop"
    }

    fn discover(&self) -> Result<Vec<ClientInstallation>, LaunchError> {
        let mut installs = Vec::new();
        let mut seen = HashSet::new();
        for install in crate::running_vesktop_installs() {
            if seen.insert(normalized_path_key(&install.executable_path)) {
                installs.push(vesktop_installation(
                    install,
                    DiscoverySource::RunningProcess,
                ));
            }
        }
        #[cfg(target_os = "linux")]
        for install in linux_desktop_entry_vesktop_installs() {
            if seen.insert(normalized_path_key(&install.executable_path)) {
                installs.push(vesktop_installation(install, DiscoverySource::OsMetadata));
            }
        }
        if let Some(install) = find_vesktop_install() {
            if seen.insert(normalized_path_key(&install.executable_path)) {
                installs.push(vesktop_installation(install, DiscoverySource::StandardPath));
            }
        }
        #[cfg(target_os = "linux")]
        if flatpak_is_installed("dev.vencord.Vesktop") {
            installs.push(flatpak_vesktop_installation("dev.vencord.Vesktop"));
        }
        Ok(installs)
    }

    fn validate_target(&self, target: &LaunchTarget) -> ValidationState {
        validate_named_executable_target(target, &["vesktop", "vesktop.exe"])
    }
}

static OFFICIAL_PROVIDER: OfficialDiscordProvider = OfficialDiscordProvider;
static VESKTOP_PROVIDER: VesktopProvider = VesktopProvider;

pub fn provider_registry() -> [&'static dyn DesktopClientProvider; 2] {
    [&OFFICIAL_PROVIDER, &VESKTOP_PROVIDER]
}

pub fn discover_client_installations() -> (Vec<ClientInstallation>, Vec<String>) {
    let mut installations = Vec::new();
    let mut issues = Vec::new();
    for provider in provider_registry() {
        match provider.discover() {
            Ok(found) => installations.extend(found),
            Err(error) => issues.push(format!("{}: {error}", provider.display_name())),
        }
    }
    (installations, issues)
}

pub fn custom_executable_installation(
    provider_id: &ProviderId,
    executable_path: PathBuf,
) -> Result<ClientInstallation, LaunchError> {
    let provider = provider_registry()
        .into_iter()
        .find(|provider| provider.id() == *provider_id)
        .ok_or_else(|| LaunchError::UnsupportedClient(provider_id.0.clone()))?;
    let target = custom_launch_target(provider_id, executable_path)?;
    let validation = provider.validate_target(&target);
    if validation == ValidationState::Invalid {
        return Err(LaunchError::InvalidInstallation {
            details: format!(
                "The selected file is not a valid {} executable.",
                provider.display_name()
            ),
        });
    }
    let display_name = provider.display_name().to_string();
    Ok(ClientInstallation {
        id: installation_id(
            provider_id,
            target_path(&target).unwrap_or_else(|| Path::new("")),
        ),
        provider_id: provider_id.clone(),
        variant_id: None,
        display_name,
        source: DiscoverySource::User,
        launch_target: target,
        capabilities: capabilities(provider_id),
        validation,
    })
}

pub fn refresh_installation_validation(mut install: ClientInstallation) -> ClientInstallation {
    install.validation = provider_registry()
        .into_iter()
        .find(|provider| provider.id() == install.provider_id)
        .map_or(ValidationState::Invalid, |provider| {
            provider.validate_target(&install.launch_target)
        });
    install
}

pub fn official_installation(install: DiscordInstall) -> ClientInstallation {
    let provider_id = ProviderId::official_discord();
    let variant_id = VariantId(install.channel.as_str().to_string());
    ClientInstallation {
        id: installation_id(&provider_id, &install.executable_path),
        provider_id,
        variant_id: Some(variant_id),
        display_name: format!("Discord {}", install.channel.display_name()),
        source: DiscoverySource::StandardPath,
        launch_target: LaunchTarget::Executable {
            path: install.executable_path,
            working_dir: install.working_dir,
            prefix_args: Vec::new(),
        },
        capabilities: ClientCapabilities {
            cdp: true,
            local_token: true,
            restore_normal: true,
        },
        validation: ValidationState::Valid,
    }
}

pub fn vesktop_installation(
    install: VesktopInstall,
    source: DiscoverySource,
) -> ClientInstallation {
    let provider_id = ProviderId::vesktop();
    ClientInstallation {
        id: installation_id(&provider_id, &install.executable_path),
        provider_id,
        variant_id: None,
        display_name: "Vesktop".into(),
        source,
        launch_target: LaunchTarget::Executable {
            path: install.executable_path,
            working_dir: install.working_dir,
            prefix_args: Vec::new(),
        },
        capabilities: ClientCapabilities {
            cdp: true,
            local_token: false,
            restore_normal: true,
        },
        validation: ValidationState::Valid,
    }
}

#[cfg(target_os = "linux")]
pub fn flatpak_vesktop_installation(app_id: &str) -> ClientInstallation {
    let provider_id = ProviderId::vesktop();
    let target = LaunchTarget::Flatpak {
        app_id: app_id.to_string(),
        command: Some("flatpak".into()),
    };
    ClientInstallation {
        id: installation_id_for_key(&provider_id, app_id),
        provider_id,
        variant_id: Some(VariantId("flatpak".into())),
        display_name: "Vesktop (Flatpak)".into(),
        source: DiscoverySource::OsMetadata,
        launch_target: target,
        capabilities: ClientCapabilities {
            cdp: true,
            local_token: false,
            restore_normal: true,
        },
        validation: ValidationState::Valid,
    }
}

pub fn installation_as_vesktop(install: &ClientInstallation) -> Option<VesktopInstall> {
    if install.provider_id != ProviderId::vesktop() || install.validation != ValidationState::Valid
    {
        return None;
    }
    match &install.launch_target {
        LaunchTarget::Executable {
            path, working_dir, ..
        } => Some(VesktopInstall {
            executable_path: path.clone(),
            working_dir: working_dir.clone(),
        }),
        LaunchTarget::MacBundle {
            executable_path, ..
        } => vesktop_install_from_executable(executable_path.clone()),
        LaunchTarget::Flatpak { .. } => None,
    }
}

pub fn installation_as_official(install: &ClientInstallation) -> Option<DiscordInstall> {
    if install.provider_id != ProviderId::official_discord()
        || install.validation != ValidationState::Valid
    {
        return None;
    }
    let channel = install
        .variant_id
        .as_ref()
        .and_then(|variant| crate::parse_discord_channel(Some(&variant.0)).ok())??;
    match &install.launch_target {
        LaunchTarget::Executable {
            path, working_dir, ..
        } => Some(DiscordInstall {
            channel,
            executable_path: path.clone(),
            working_dir: working_dir.clone(),
        }),
        LaunchTarget::MacBundle {
            executable_path, ..
        } => Some(DiscordInstall {
            channel,
            executable_path: executable_path.clone(),
            working_dir: executable_path.parent()?.to_path_buf(),
        }),
        LaunchTarget::Flatpak { .. } => None,
    }
}

fn validate_executable_target(target: &LaunchTarget) -> ValidationState {
    match target {
        LaunchTarget::Executable { path, .. }
        | LaunchTarget::MacBundle {
            executable_path: path,
            ..
        } => {
            if path.is_file() {
                ValidationState::Valid
            } else {
                ValidationState::Missing
            }
        }
        LaunchTarget::Flatpak { app_id, .. } => {
            if app_id.trim().is_empty() {
                ValidationState::Invalid
            } else {
                ValidationState::Valid
            }
        }
    }
}

fn validate_named_executable_target(target: &LaunchTarget, names: &[&str]) -> ValidationState {
    let state = validate_executable_target(target);
    if state != ValidationState::Valid {
        return state;
    }
    let executable = match target {
        LaunchTarget::Executable { path, .. } => path,
        LaunchTarget::MacBundle {
            executable_path, ..
        } => executable_path,
        LaunchTarget::Flatpak { .. } => return state,
    };
    let Some(name) = executable.file_name().and_then(|name| name.to_str()) else {
        return ValidationState::Invalid;
    };
    let lowered = name.to_ascii_lowercase();
    let has_supported_name = names
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
        || lowered.starts_with("vesktop") && lowered.ends_with(".appimage");
    if has_supported_name {
        ValidationState::Valid
    } else {
        ValidationState::Invalid
    }
}

fn capabilities(provider_id: &ProviderId) -> ClientCapabilities {
    ClientCapabilities {
        cdp: true,
        local_token: provider_id == &ProviderId::official_discord(),
        restore_normal: true,
    }
}

fn target_path(target: &LaunchTarget) -> Option<&Path> {
    match target {
        LaunchTarget::Executable { path, .. } => Some(path),
        LaunchTarget::MacBundle { bundle_path, .. } => Some(bundle_path),
        LaunchTarget::Flatpak { .. } => None,
    }
}

fn custom_launch_target(
    _provider_id: &ProviderId,
    selected_path: PathBuf,
) -> Result<LaunchTarget, LaunchError> {
    #[cfg(target_os = "macos")]
    if selected_path
        .extension()
        .is_some_and(|extension| extension == "app")
    {
        let info = selected_path.join("Contents").join("Info.plist");
        let contents =
            std::fs::read_to_string(&info).map_err(|_| LaunchError::InvalidInstallation {
                details: "The selected app bundle has no readable Info.plist.".into(),
            })?;
        let expected = if _provider_id == &ProviderId::vesktop() {
            "dev.vencord.Vesktop"
        } else {
            "com.hnc.Discord"
        };
        if !contents.contains(expected) && !contents.to_ascii_lowercase().contains("discord") {
            return Err(LaunchError::InvalidInstallation {
                details: "The selected app bundle does not identify a supported Discord client."
                    .into(),
            });
        }
        let macos = selected_path.join("Contents").join("MacOS");
        let names: &[&str] = if _provider_id == &ProviderId::vesktop() {
            &["Vesktop", "vesktop"]
        } else {
            &["Discord", "Discord PTB", "Discord Canary"]
        };
        let executable_path = names
            .iter()
            .map(|name| macos.join(name))
            .find(|path| path.is_file())
            .ok_or_else(|| LaunchError::InvalidInstallation {
                details: "The app bundle's internal executable could not be found.".into(),
            })?;
        return Ok(LaunchTarget::MacBundle {
            bundle_path: selected_path,
            executable_path,
        });
    }

    let working_dir = selected_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| LaunchError::InvalidInstallation {
            details: "The selected executable has no parent directory.".into(),
        })?;
    Ok(LaunchTarget::Executable {
        path: selected_path,
        working_dir,
        prefix_args: Vec::new(),
    })
}

fn installation_id(provider_id: &ProviderId, path: &Path) -> InstallationId {
    installation_id_for_key(provider_id, &normalized_path_key(path))
}

fn installation_id_for_key(provider_id: &ProviderId, key: &str) -> InstallationId {
    let value = format!("{}\0{key}", provider_id.as_str());
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    InstallationId(format!("{}:{hash:016x}", provider_id.as_str()))
}

#[cfg(target_os = "linux")]
fn flatpak_is_installed(app_id: &str) -> bool {
    std::process::Command::new("flatpak")
        .args(["info", app_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "linux")]
fn linux_desktop_entry_vesktop_installs() -> Vec<VesktopInstall> {
    let mut roots = Vec::new();
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data_home).join("applications"));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/applications"));
    }
    roots.push(PathBuf::from("/usr/local/share/applications"));
    roots.push(PathBuf::from("/usr/share/applications"));
    let mut installs = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(contents) = std::fs::read_to_string(path) else {
                continue;
            };
            if !contents.to_ascii_lowercase().contains("vesktop") {
                continue;
            }
            let Some(exec) = contents.lines().find_map(|line| line.strip_prefix("Exec=")) else {
                continue;
            };
            let command = desktop_exec_command(exec);
            let Some(command) = command else {
                continue;
            };
            let candidate = PathBuf::from(&command);
            if candidate.is_absolute() {
                if let Some(install) = vesktop_install_from_executable(candidate) {
                    installs.push(install);
                }
                continue;
            }
            if let Some(path) = std::env::var_os("PATH") {
                if let Some(install) = std::env::split_paths(&path)
                    .find_map(|root| vesktop_install_from_executable(root.join(&command)))
                {
                    installs.push(install);
                }
            }
        }
    }
    installs
}

#[cfg(target_os = "linux")]
fn desktop_exec_command(exec: &str) -> Option<String> {
    let exec = exec.trim();
    if let Some(quoted) = exec.strip_prefix('"') {
        return quoted
            .split_once('"')
            .map(|(command, _)| command.replace("\\ ", " "));
    }
    exec.split_whitespace()
        .next()
        .map(|command| command.replace("\\ ", " "))
}

fn normalized_path_key(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = path.to_string_lossy().into_owned();
    if cfg!(target_os = "windows") {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_ids_are_provider_scoped_and_stable() {
        let path = Path::new("C:\\Clients\\Vesktop.exe");
        assert_eq!(
            installation_id(&ProviderId::vesktop(), path),
            installation_id(&ProviderId::vesktop(), path)
        );
        assert_ne!(
            installation_id(&ProviderId::vesktop(), path),
            installation_id(&ProviderId::official_discord(), path)
        );
    }

    #[test]
    fn accepts_a_unicode_portable_vesktop_executable_and_uses_its_parent() {
        let root = std::env::temp_dir().join(format!(
            "dqh-vesktop-便携-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let directory = root.join("Portable Apps").join("Vesktop");
        std::fs::create_dir_all(&directory).unwrap();
        let executable = directory.join(if cfg!(target_os = "windows") {
            "vesktop.exe"
        } else {
            "vesktop"
        });
        std::fs::write(&executable, []).unwrap();

        let install = custom_executable_installation(&ProviderId::vesktop(), executable.clone())
            .expect("portable Vesktop should validate");
        assert_eq!(install.validation, ValidationState::Valid);
        assert_eq!(install.source, DiscoverySource::User);
        assert!(matches!(
            install.launch_target,
            LaunchTarget::Executable { path, working_dir, .. }
                if path == executable && working_dir == directory
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_an_unrelated_executable_as_vesktop() {
        let root = std::env::temp_dir().join(format!("dqh-wrong-client-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join(if cfg!(target_os = "windows") {
            "chrome.exe"
        } else {
            "chrome"
        });
        std::fs::write(&executable, []).unwrap();
        assert!(matches!(
            custom_executable_installation(&ProviderId::vesktop(), executable),
            Err(LaunchError::InvalidInstallation { .. })
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}

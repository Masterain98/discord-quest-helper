//! Reconcile versioned Windows launch paths before snapshots and launch commands
//! consume the saved configuration. User-supplied paths remain exact targets.
use super::*;
use cdp_launch::{ClientInstallation, DiscoverySource, LaunchSelector};

pub(super) fn same_executable_path(left: &Path, right: &Path) -> bool {
    fn key(path: &Path) -> String {
        let key = path
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        key.strip_prefix(r"\\?\unc\")
            .map(|suffix| format!(r"\\{suffix}"))
            .unwrap_or_else(|| key.strip_prefix(r"\\?\").unwrap_or(&key).to_string())
    }
    key(left) == key(right)
}

pub(super) fn resolve_session_id(
    sessions: &[cdp_launch::DesktopCdpSession],
    requested: cdp_launch::InstallationId,
) -> cdp_launch::InstallationId {
    let mut matches = sessions.iter().filter(|session| {
        session.provider_id == cdp_launch::ProviderId::official_discord()
            && session.executable_path.as_ref().is_some_and(|path| {
                cdp_launch::windows_legacy_installation_id_matches(path, &requested)
            })
    });
    if let Some(session) = matches.next() {
        if matches.next().is_none() {
            return session.installation_id.clone().unwrap_or(requested);
        }
    }
    requested
}

pub(super) fn reconcile_config(
    mut config: DesktopClientsConfig,
    refresh: impl Fn(&ClientInstallation) -> std::io::Result<Option<ClientInstallation>>,
) -> DesktopClientsConfig {
    let mut refreshed = Vec::new();
    let mut aliases = HashMap::new();
    let mut missing = HashSet::new();
    for install in &config.installations {
        match refresh(install) {
            Ok(Some(current)) => {
                aliases.insert(install.id.clone(), current.id.clone());
                refreshed.push(current);
            }
            Ok(None) => {
                missing.insert(install.id.clone());
            }
            Err(error) => {
                // A failed scan cannot establish which saved installations are gone.
                // Leave the entire persistent config untouched and try on next refresh.
                config.discovery_issues.push(DiscoveryIssueDto {
                    provider_id: Some(install.provider_id.clone()),
                    code: "scan_failed",
                    message: format!("Could not refresh a saved Discord installation ({:?}); its configuration has been preserved.", error.kind()),
                });
                return config;
            }
        }
    }
    if let LaunchSelector::Installation { installation_id } = &mut config.selection {
        if missing.contains(installation_id) {
            config.discovery_issues.push(DiscoveryIssueDto {
                provider_id: Some(cdp_launch::ProviderId::official_discord()),
                code: "installation_missing",
                message: "The previously selected Discord installation is no longer available. Choose a desktop client again.".into(),
            });
            config.selection = LaunchSelector::Auto;
        } else if let Some(current) = aliases.get(installation_id) {
            *installation_id = current.clone();
        }
    }
    let mut seen = HashSet::new();
    config.installations = refreshed.into_iter().filter(|install| {
        let selected = matches!(&config.selection, LaunchSelector::Installation { installation_id } if installation_id == &install.id);
        (install.source == DiscoverySource::User || selected) && seen.insert(install.id.clone())
    }).collect();
    config.installation_aliases = aliases;
    config
}

pub(super) fn migrate_session_ids(
    sessions: &mut [cdp_launch::DesktopCdpSession],
    config: &DesktopClientsConfig,
    discovered: &[ClientInstallation],
) -> bool {
    let automatic_ids: HashSet<_> = config
        .installations
        .iter()
        .chain(discovered)
        .filter(|install| install.source == DiscoverySource::StandardPath)
        .map(|install| &install.id)
        .collect();
    let manual_ids: HashSet<_> = config
        .installations
        .iter()
        .filter(|install| install.source == DiscoverySource::User)
        .map(|install| &install.id)
        .collect();
    let mut changed = false;
    for session in sessions {
        if session.provider_id != cdp_launch::ProviderId::official_discord() {
            continue;
        }
        let Some(old_id) = &session.installation_id else {
            continue;
        };
        if manual_ids.contains(old_id) {
            continue;
        }
        let Some(path) = &session.executable_path else {
            continue;
        };
        let Some(channel) = session.variant_id.as_ref().and_then(|variant| {
            cdp_launch::parse_discord_channel(Some(&variant.0))
                .ok()
                .flatten()
        }) else {
            continue;
        };
        let Some(current_id) = cdp_launch::windows_official_installation_id(path, channel) else {
            continue;
        };
        let known_alias = config.installation_aliases.get(old_id) == Some(&current_id);
        // A journal has no discovery source. Infer an old automatic ID only when
        // both its old path hash and a currently known automatic installation agree.
        let known_legacy = automatic_ids.contains(&current_id)
            && cdp_launch::windows_legacy_installation_id_matches(path, old_id);
        if current_id != *old_id && (known_alias || known_legacy) {
            session.installation_id = Some(current_id);
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdp_launch::{DiscordChannel, LaunchTarget, ProviderId, SessionOwnership};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                    "dqh-inventory-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                )))
        }
        fn path(&self, folder: &str, version: &str, exe: &str) -> PathBuf {
            self.0.join(folder).join(format!("app-{version}")).join(exe)
        }
        fn legacy(&self, folder: &str, version: &str, exe: &str) -> ClientInstallation {
            let path = self.path(folder, version, exe);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, []).unwrap();
            // Generate the real legacy canonical-path ID while the executable exists.
            let mut install = cdp_launch::custom_executable_installation(
                &ProviderId::official_discord(),
                path.clone(),
            )
            .unwrap();
            install.source = DiscoverySource::StandardPath;
            std::fs::remove_file(path).unwrap();
            install
        }
        fn current(&self, folder: &str, version: &str, exe: &str) -> PathBuf {
            let path = self.path(folder, version, exe);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, []).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn legacy_history_merges_and_selected_client_tracks_subsequent_updates() {
        let fixture = Fixture::new();
        let mut history = Vec::new();
        for version in ["1.0.9256", "1.0.9258", "1.0.9259", "1.0.9260", "1.0.9261"] {
            history.push(fixture.legacy("Discord", version, "Discord.exe"));
        }
        history.push(fixture.legacy("DiscordPTB", "1.0.1224", "DiscordPTB.exe"));
        history.push(fixture.legacy("DiscordCanary", "1.0.1131", "DiscordCanary.exe"));
        let old_selected = history.last().unwrap().id.clone();
        fixture.current("Discord", "1.0.9264", "Discord.exe");
        fixture.current("DiscordPTB", "1.0.1226", "DiscordPTB.exe");
        let current = fixture.current("DiscordCanary", "1.0.1259", "DiscordCanary.exe");
        let mut manual = fixture.legacy("Portable", "1.0.1", "Discord.exe");
        manual.source = DiscoverySource::User;
        history.push(manual.clone());
        let config = DesktopClientsConfig {
            version: 1,
            installations: history,
            selection: LaunchSelector::Installation {
                installation_id: old_selected.clone(),
            },
            ..Default::default()
        };
        let config = reconcile_config(config, cdp_launch::refresh_windows_official_installation);
        let id =
            cdp_launch::windows_official_installation_id(&current, DiscordChannel::Canary).unwrap();
        assert_eq!(
            config.selection,
            LaunchSelector::Installation {
                installation_id: id.clone()
            }
        );
        assert_eq!(config.installations.len(), 2);
        assert!(config
            .installations
            .iter()
            .any(|install| install.id == manual.id));
        let options = options_for_selector(9223, &config.selection, &config, false).unwrap();
        assert_eq!(
            installation_executable_path(options.installation.as_ref().unwrap()),
            Some(current.as_path())
        );
        assert_eq!(config.installation_aliases.get(&old_selected), Some(&id));
        let next = fixture.current("DiscordCanary", "1.0.1260", "DiscordCanary.exe");
        // Simulate restart: transient aliases and issues do not enter the wire format.
        let saved = serde_json::to_vec(&config).unwrap();
        let config = reconcile_config(
            serde_json::from_slice(&saved).unwrap(),
            cdp_launch::refresh_windows_official_installation,
        );
        let selected = config
            .installations
            .iter()
            .find(|install| install.id == id)
            .unwrap();
        assert_eq!(installation_executable_path(selected), Some(next.as_path()));
        assert_eq!(config.installations.len(), 2);
        let saved = serde_json::to_vec(&config).unwrap();
        let again = reconcile_config(config, cdp_launch::refresh_windows_official_installation);
        assert_eq!(saved, serde_json::to_vec(&again).unwrap());
    }

    #[test]
    fn uninstall_resets_automatic_selection_but_io_failure_preserves_config() {
        let fixture = Fixture::new();
        let old = fixture.legacy("Discord", "1.0.1", "Discord.exe");
        let config = DesktopClientsConfig {
            version: 1,
            selection: LaunchSelector::Installation {
                installation_id: old.id.clone(),
            },
            installations: vec![old],
            ..Default::default()
        };
        let before = serde_json::to_vec(&config).unwrap();
        let failed = reconcile_config(config.clone(), |_| {
            Err(std::io::ErrorKind::PermissionDenied.into())
        });
        assert_eq!(before, serde_json::to_vec(&failed).unwrap());
        assert_eq!(failed.discovery_issues[0].code, "scan_failed");
        let removed = reconcile_config(config, cdp_launch::refresh_windows_official_installation);
        assert!(removed.installations.is_empty());
        assert_eq!(removed.selection, LaunchSelector::Auto);
        assert_eq!(removed.discovery_issues[0].code, "installation_missing");
    }

    #[test]
    fn session_migration_keeps_exact_path_and_ownership_and_excludes_manual_ids() {
        let fixture = Fixture::new();
        let old = fixture.legacy("Discord", "1.0.1", "Discord.exe");
        let path = installation_executable_path(&old).unwrap().to_path_buf();
        let current_path = fixture.current("Discord", "1.0.2", "Discord.exe");
        let config = DesktopClientsConfig {
            version: 1,
            selection: LaunchSelector::Installation {
                installation_id: old.id.clone(),
            },
            installations: vec![old.clone()],
            ..Default::default()
        };
        let current = reconcile_config(config, cdp_launch::refresh_windows_official_installation);
        let mut sessions = vec![cdp_launch::DesktopCdpSession {
            provider_id: old.provider_id.clone(),
            installation_id: Some(old.id.clone()),
            variant_id: old.variant_id.clone(),
            port: 9223,
            ownership: SessionOwnership::Managed,
            executable_path: Some(path.clone()),
        }];
        let before = sessions[0].clone();
        // Migration must still work after config migration has already been persisted.
        let saved: DesktopClientsConfig =
            serde_json::from_value(serde_json::to_value(&current).unwrap()).unwrap();
        assert!(migrate_session_ids(&mut sessions, &saved, &[]));
        assert_eq!(sessions[0].executable_path, before.executable_path);
        assert_eq!(sessions[0].ownership, before.ownership);
        assert_eq!(
            sessions[0].installation_id,
            Some(current.installations[0].id.clone())
        );
        assert_eq!(
            resolve_session_id(&sessions, old.id.clone()),
            current.installations[0].id
        );
        let mut detected = sessions[0].clone();
        detected.executable_path = Some(current_path);
        assert!(session_key_matches(&sessions[0], &detected));
        assert!(!managed_session_path_matches(&sessions[0], &detected));
        assert!(find_managed_journal_session(&sessions, &detected)
            .unwrap()
            .is_none());
        let journal = [sessions[0].clone(), detected.clone()];
        assert_eq!(
            find_managed_journal_session(&journal, &detected).unwrap(),
            Some(&journal[1])
        );
        assert!(find_live_sessions_for_managed(&[detected.clone()], &journal[0]).is_empty());
        assert_eq!(
            find_live_sessions_for_managed(&[detected.clone()], &journal[1]),
            vec![detected.clone()]
        );
        detected.installation_id = None;
        assert!(find_managed_journal_session(&sessions, &detected)
            .unwrap()
            .is_none());
        assert!(find_live_sessions_for_managed(&[detected], &sessions[0]).is_empty());
        let mut manual = old;
        manual.source = DiscoverySource::User;
        let manual_config = DesktopClientsConfig {
            installations: vec![manual],
            ..Default::default()
        };
        sessions[0] = before;
        assert!(!migrate_session_ids(
            &mut sessions,
            &manual_config,
            &current.installations
        ));
        assert!(matches!(
            current.installations[0].launch_target,
            LaunchTarget::Executable { .. }
        ));
    }
}

use crate::{
    CdpPortOwner, DesktopClientPreference, DiscordChannel, DiscordLaunchMode, LaunchError,
};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn find_vesktop_install() -> Option<VesktopInstall> {
    #[cfg(target_os = "windows")]
    {
        let local_appdata = std::env::var_os("LOCALAPPDATA")?;
        let mut extras = Vec::new();
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            extras.push(PathBuf::from(program_files));
        }
        if let Some(program_files) = std::env::var_os("ProgramFiles(x86)") {
            extras.push(PathBuf::from(program_files));
        }
        discover_windows_vesktop_install_in(Path::new(&local_appdata), &extras)
    }
    #[cfg(target_os = "macos")]
    {
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
        discover_macos_vesktop_install_in(&roots)
    }
    #[cfg(target_os = "linux")]
    {
        let mut roots = vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            roots.push(home.join(".local").join("bin"));
            roots.push(home.join(".local").join("share"));
        }
        if let Some(path) = std::env::var_os("PATH") {
            roots.extend(std::env::split_paths(&path));
        }
        if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
            roots.push(PathBuf::from(data_home).join("applications"));
        }
        if let Some(data_dirs) = std::env::var_os("XDG_DATA_DIRS") {
            roots.extend(std::env::split_paths(&data_dirs).map(|root| root.join("applications")));
        }
        discover_linux_vesktop_install_in(&roots)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

pub fn is_vesktop_running() -> Result<bool, LaunchError> {
    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::is_vesktop_running()
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::is_vesktop_running()
    }
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::is_vesktop_running()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Ok(false)
    }
}

pub fn spawn_vesktop(
    install: &VesktopInstall,
    mode: DiscordLaunchMode,
) -> Result<u32, LaunchError> {
    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::spawn_vesktop(install, mode)
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::spawn_vesktop(install, mode)
    }
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::spawn_vesktop(install, mode)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (install, mode);
        Err(LaunchError::UnsupportedPlatform)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VesktopInstall {
    pub executable_path: PathBuf,
    pub working_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VesktopLaunchPlan {
    UseOfficialPath,
    LaunchVesktop,
}

pub fn parse_desktop_client_preference(
    value: Option<&str>,
) -> Result<DesktopClientPreference, LaunchError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("auto") => Ok(DesktopClientPreference::Auto),
        Some("official" | "discord") => Ok(DesktopClientPreference::Official),
        Some("vesktop") => Ok(DesktopClientPreference::Vesktop),
        Some(value) => Err(LaunchError::UnsupportedClient(value.to_string())),
    }
}

pub fn cdp_ready_matches_preference(
    preference: DesktopClientPreference,
    owner: CdpPortOwner,
) -> bool {
    match preference {
        DesktopClientPreference::Auto => true,
        DesktopClientPreference::Official => matches!(owner, CdpPortOwner::Official),
        DesktopClientPreference::Vesktop => matches!(owner, CdpPortOwner::Vesktop),
    }
}

pub fn vesktop_launch_plan(
    channel: Option<DiscordChannel>,
    preference: DesktopClientPreference,
    official_running: bool,
    vesktop_running: bool,
    official_install_found: bool,
    vesktop_install_found: bool,
) -> VesktopLaunchPlan {
    if channel.is_some() || preference == DesktopClientPreference::Official {
        return VesktopLaunchPlan::UseOfficialPath;
    }
    if preference == DesktopClientPreference::Vesktop {
        return VesktopLaunchPlan::LaunchVesktop;
    }
    if vesktop_running && !official_running {
        return VesktopLaunchPlan::LaunchVesktop;
    }
    if !official_install_found && vesktop_install_found {
        return VesktopLaunchPlan::LaunchVesktop;
    }
    VesktopLaunchPlan::UseOfficialPath
}

pub fn is_vesktop_process_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "vesktop" | "vesktop.exe"
    )
}

pub fn vesktop_cdp_args(port: u16) -> Vec<OsString> {
    crate::build_launch_args(DiscordLaunchMode::Cdp { port })
}

pub fn vesktop_install_from_executable(executable_path: PathBuf) -> Option<VesktopInstall> {
    if !executable_path.is_file() {
        return None;
    }
    #[cfg(unix)]
    if std::os::unix::fs::MetadataExt::mode(&std::fs::metadata(&executable_path).ok()?) & 0o111 == 0
    {
        return None;
    }
    let working_dir = executable_path.parent()?.to_path_buf();
    Some(VesktopInstall {
        executable_path,
        working_dir,
    })
}

pub fn discover_windows_vesktop_install_in(
    local_appdata: &Path,
    extra_roots: &[PathBuf],
) -> Option<VesktopInstall> {
    let mut candidates = vec![
        local_appdata.join("vesktop").join("vesktop.exe"),
        local_appdata.join("Vesktop").join("vesktop.exe"),
    ];
    for root in extra_roots {
        candidates.push(root.join("Vesktop").join("vesktop.exe"));
        candidates.push(root.join("vesktop").join("vesktop.exe"));
    }
    candidates
        .into_iter()
        .find_map(vesktop_install_from_executable)
}

pub fn discover_macos_vesktop_install_in(roots: &[PathBuf]) -> Option<VesktopInstall> {
    for root in roots {
        let macos_dir = root.join("Vesktop.app").join("Contents").join("MacOS");
        for name in ["Vesktop", "vesktop"] {
            if let Some(install) = vesktop_install_from_executable(macos_dir.join(name)) {
                return Some(install);
            }
        }
    }
    None
}

pub fn discover_linux_vesktop_install_in(roots: &[PathBuf]) -> Option<VesktopInstall> {
    let relative = [
        PathBuf::from("vesktop"),
        PathBuf::from("Vesktop"),
        PathBuf::from("Vesktop").join("vesktop"),
        PathBuf::from("vesktop").join("vesktop"),
    ];
    for root in roots {
        for rel in &relative {
            if let Some(install) = vesktop_install_from_executable(root.join(rel)) {
                return Some(install);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_official_channel_never_selects_vesktop() {
        assert_eq!(
            vesktop_launch_plan(
                Some(DiscordChannel::Stable),
                DesktopClientPreference::Auto,
                false,
                true,
                false,
                true,
            ),
            VesktopLaunchPlan::UseOfficialPath
        );
    }

    #[test]
    fn running_vesktop_without_official_discord_selects_vesktop() {
        assert_eq!(
            vesktop_launch_plan(None, DesktopClientPreference::Auto, false, true, true, true,),
            VesktopLaunchPlan::LaunchVesktop
        );
    }

    #[test]
    fn official_discord_still_wins_when_it_is_running() {
        assert_eq!(
            vesktop_launch_plan(None, DesktopClientPreference::Auto, true, true, true, true,),
            VesktopLaunchPlan::UseOfficialPath
        );
    }

    #[test]
    fn vesktop_is_the_fallback_when_no_official_install_exists() {
        assert_eq!(
            vesktop_launch_plan(
                None,
                DesktopClientPreference::Auto,
                false,
                false,
                false,
                true,
            ),
            VesktopLaunchPlan::LaunchVesktop
        );
        assert_eq!(
            vesktop_launch_plan(
                None,
                DesktopClientPreference::Auto,
                false,
                false,
                true,
                true,
            ),
            VesktopLaunchPlan::UseOfficialPath
        );
    }

    #[test]
    fn explicit_vesktop_preference_wins_even_when_official_discord_is_present() {
        assert_eq!(
            vesktop_launch_plan(
                None,
                DesktopClientPreference::Vesktop,
                true,
                false,
                true,
                true,
            ),
            VesktopLaunchPlan::LaunchVesktop
        );
    }

    #[test]
    fn explicit_official_preference_never_selects_vesktop() {
        assert_eq!(
            vesktop_launch_plan(
                None,
                DesktopClientPreference::Official,
                false,
                true,
                true,
                true,
            ),
            VesktopLaunchPlan::UseOfficialPath
        );
    }

    #[test]
    fn explicit_official_channel_overrides_vesktop_preference() {
        assert_eq!(
            vesktop_launch_plan(
                Some(DiscordChannel::Ptb),
                DesktopClientPreference::Vesktop,
                false,
                true,
                true,
                true,
            ),
            VesktopLaunchPlan::UseOfficialPath
        );
    }

    #[test]
    fn parses_desktop_client_preference_aliases() {
        assert_eq!(
            parse_desktop_client_preference(None).unwrap(),
            DesktopClientPreference::Auto
        );
        assert_eq!(
            parse_desktop_client_preference(Some("discord")).unwrap(),
            DesktopClientPreference::Official
        );
        assert_eq!(
            parse_desktop_client_preference(Some("vesktop")).unwrap(),
            DesktopClientPreference::Vesktop
        );
        assert!(parse_desktop_client_preference(Some("chrome")).is_err());
    }

    #[test]
    fn ready_cdp_is_not_reused_for_the_other_client() {
        assert!(cdp_ready_matches_preference(
            DesktopClientPreference::Auto,
            CdpPortOwner::Official
        ));
        assert!(!cdp_ready_matches_preference(
            DesktopClientPreference::Official,
            CdpPortOwner::None
        ));
        assert!(!cdp_ready_matches_preference(
            DesktopClientPreference::Official,
            CdpPortOwner::Other
        ));
        assert!(!cdp_ready_matches_preference(
            DesktopClientPreference::Official,
            CdpPortOwner::Vesktop
        ));
        assert!(!cdp_ready_matches_preference(
            DesktopClientPreference::Vesktop,
            CdpPortOwner::Official
        ));
    }

    #[test]
    fn process_name_matching_is_case_insensitive_and_ignores_discord() {
        assert!(is_vesktop_process_name("vesktop.exe"));
        assert!(is_vesktop_process_name("Vesktop"));
        assert!(!is_vesktop_process_name("Discord.exe"));
        assert!(!is_vesktop_process_name("chrome.exe"));
    }

    #[test]
    fn cdp_spawn_args_match_official_discord() {
        assert_eq!(
            vesktop_cdp_args(9223),
            vec![OsString::from("--remote-debugging-port=9223")]
        );
    }

    fn unique_temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dqh-vesktop-install-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn windows_discovery_requires_a_real_executable() {
        let root = unique_temp_root("win");
        let vesktop_dir = root.join("vesktop");
        std::fs::create_dir_all(&vesktop_dir).unwrap();
        assert!(discover_windows_vesktop_install_in(&root, &[]).is_none());

        std::fs::write(vesktop_dir.join("vesktop.exe"), []).unwrap();
        let install = discover_windows_vesktop_install_in(&root, &[]).unwrap();
        assert_eq!(install.executable_path, vesktop_dir.join("vesktop.exe"));
        assert_eq!(install.working_dir, vesktop_dir);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_vesktop_installs_are_ignored() {
        let root = unique_temp_root("missing");
        std::fs::create_dir_all(&root).unwrap();
        assert!(discover_windows_vesktop_install_in(&root, &[]).is_none());
        assert!(discover_macos_vesktop_install_in(std::slice::from_ref(&root)).is_none());
        assert!(discover_linux_vesktop_install_in(std::slice::from_ref(&root)).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires a live Discord-ready CDP session on the default debugging port"]
    fn live_ready_cdp_port_is_already_available() {
        let result = crate::launch_discord_with_cdp(crate::LaunchOptions {
            port: crate::DEFAULT_CDP_PORT,
            channel: None,
            restart_existing: false,
            ..Default::default()
        })
        .expect("a Discord-ready CDP port should be treated as already available");
        assert!(result.cdp_connected);
        assert_eq!(result.outcome, crate::LaunchOutcome::AlreadyAvailable);
        println!(
            "live launch already-available path_len={} channel={:?}",
            result.launched_path.as_os_str().len(),
            result.channel
        );
    }

    #[test]
    #[ignore = "inspects live Vesktop/Discord processes"]
    fn live_vesktop_is_not_listed_as_official_cdp_session() {
        assert!(
            is_vesktop_running().expect("Vesktop process scan"),
            "this check requires Vesktop to be running"
        );
        let official_running = crate::is_discord_running(None).expect("official process scan");
        let sessions = crate::list_running_discord_cdp_sessions().expect("official CDP scan");
        println!(
            "live process check official_running={} official_cdp_sessions={}",
            official_running,
            sessions.len()
        );
        assert!(
            sessions
                .iter()
                .all(|session| session.port != crate::DEFAULT_CDP_PORT),
            "Vesktop's CDP port must not be claimed as an official Discord channel session"
        );
    }

    #[test]
    #[ignore = "binds a disposable localhost port"]
    fn live_non_discord_listener_is_port_occupied() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let _hold = listener;
            std::thread::sleep(std::time::Duration::from_secs(2));
        });
        let error = crate::launch_discord_with_cdp(crate::LaunchOptions {
            port,
            channel: None,
            restart_existing: false,
            ..Default::default()
        })
        .expect_err("a non-Discord TCP listener must not be launched over");
        assert!(
            matches!(error, crate::LaunchError::PortOccupied { port: occupied } if occupied == port)
        );
        println!("live port-occupied on disposable port {port}");
    }

    #[test]
    #[ignore = "requires official Discord stopped and Vesktop running without CDP"]
    fn live_vesktop_without_cdp_reports_already_running() {
        assert!(
            !crate::is_discord_running(None).expect("official process scan"),
            "official Discord must be fully quit for this Vesktop-only check"
        );
        assert!(
            is_vesktop_running().expect("Vesktop process scan"),
            "Vesktop must be running"
        );
        assert!(
            !crate::is_cdp_available(crate::DEFAULT_CDP_PORT),
            "this check requires Vesktop to be running without CDP"
        );
        let error = crate::launch_discord_with_cdp(crate::LaunchOptions {
            port: crate::DEFAULT_CDP_PORT,
            channel: None,
            restart_existing: false,
            ..Default::default()
        })
        .expect_err("running Vesktop without CDP should not be overwritten");
        assert!(matches!(
            error,
            crate::LaunchError::DiscordAlreadyRunning { .. }
        ));
        println!("live Vesktop-without-CDP correctly refused a non-restart launch");
    }

    #[test]
    #[ignore = "restarts Vesktop with CDP; official Discord must be stopped"]
    fn live_restart_vesktop_with_cdp_when_official_is_down() {
        assert!(
            !crate::is_discord_running(None).expect("official process scan"),
            "official Discord must be fully quit for this Vesktop-only launch"
        );
        assert!(
            find_vesktop_install().is_some(),
            "Vesktop must be installed"
        );
        let result = crate::launch_discord_with_cdp(crate::LaunchOptions {
            port: crate::DEFAULT_CDP_PORT,
            channel: None,
            restart_existing: true,
            readiness_timeout: std::time::Duration::from_secs(45),
            shutdown_timeout: std::time::Duration::from_secs(20),
            ..Default::default()
        })
        .expect("Vesktop should launch or restart with CDP");
        assert!(result.cdp_connected);
        println!(
            "live Vesktop relaunch outcome={:?} path_len={}",
            result.outcome,
            result.launched_path.as_os_str().len()
        );
    }
}

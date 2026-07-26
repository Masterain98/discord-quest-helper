use discord_cdp_launch_core::{
    build_launch_args, launch_with_backends, select_preferred_install, CdpProbe, CdpProbeStatus,
    DiscordChannel, DiscordInstall, LaunchError, LaunchOptions, LaunchOutcome, PlatformBackend,
};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

struct FakePlatform {
    installs: Vec<DiscordInstall>,
    running: Mutex<VecDeque<bool>>,
    terminate_count: AtomicUsize,
    spawn_count: AtomicUsize,
}

impl FakePlatform {
    fn new(installs: Vec<DiscordInstall>, running: &[bool]) -> Self {
        Self {
            installs,
            running: Mutex::new(running.iter().copied().collect()),
            terminate_count: AtomicUsize::new(0),
            spawn_count: AtomicUsize::new(0),
        }
    }
}

impl PlatformBackend for FakePlatform {
    fn find_installs(&self) -> Result<Vec<DiscordInstall>, LaunchError> {
        Ok(self.installs.clone())
    }

    fn is_running(&self, _channel: Option<DiscordChannel>) -> Result<bool, LaunchError> {
        let mut values = self.running.lock().unwrap();
        let value = values.front().copied().unwrap_or(false);
        if values.len() > 1 {
            values.pop_front();
        }
        Ok(value)
    }

    fn terminate(&self, _channel: Option<DiscordChannel>) -> Result<(), LaunchError> {
        self.terminate_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn spawn(
        &self,
        _install: &DiscordInstall,
        _port: u16,
        _allow_origins: bool,
    ) -> Result<u32, LaunchError> {
        self.spawn_count.fetch_add(1, Ordering::SeqCst);
        Ok(4242)
    }
}

struct FakeProbe {
    statuses: Mutex<VecDeque<CdpProbeStatus>>,
}

impl FakeProbe {
    fn new(statuses: Vec<CdpProbeStatus>) -> Self {
        Self {
            statuses: Mutex::new(statuses.into()),
        }
    }
}

impl CdpProbe for FakeProbe {
    fn probe(&self, _port: u16) -> CdpProbeStatus {
        let mut statuses = self.statuses.lock().unwrap();
        let status = statuses
            .front()
            .cloned()
            .unwrap_or(CdpProbeStatus::Unreachable);
        if statuses.len() > 1 {
            statuses.pop_front();
        }
        status
    }
}

fn install(channel: DiscordChannel) -> DiscordInstall {
    DiscordInstall {
        channel,
        executable_path: PathBuf::from(format!("C:\\Discord\\{}.exe", channel.as_str())),
        working_dir: PathBuf::from("C:\\Discord"),
    }
}

/// Options for tests that assert a *timeout* is reached: the budget must be
/// small so the test is fast.
fn fast_options() -> LaunchOptions {
    LaunchOptions {
        shutdown_timeout: Duration::from_millis(10),
        readiness_timeout: Duration::from_millis(10),
        poll_interval: Duration::from_millis(1),
        ..Default::default()
    }
}

/// Options for tests that assert *eventual success* after several polls.
///
/// `fast_options`' 10ms readiness budget is wall-clock, and a 1ms sleep can
/// overshoot by an order of magnitude on a loaded CI runner, so a multi-poll
/// success path spuriously times out there. The generous ceiling is never
/// actually reached: the fake probe reports readiness after a fixed number of
/// polls, so these tests still finish in milliseconds.
fn patient_options() -> LaunchOptions {
    LaunchOptions {
        shutdown_timeout: Duration::from_secs(30),
        readiness_timeout: Duration::from_secs(30),
        ..fast_options()
    }
}

#[test]
fn already_available_does_not_spawn() {
    let platform = FakePlatform::new(vec![install(DiscordChannel::Stable)], &[false]);
    let probe = FakeProbe::new(vec![CdpProbeStatus::DiscordReady {
        target_title: Some("Discord".to_string()),
    }]);
    let result = launch_with_backends(fast_options(), &platform, &probe).unwrap();
    assert_eq!(result.outcome, LaunchOutcome::AlreadyAvailable);
    assert_eq!(platform.spawn_count.load(Ordering::SeqCst), 0);
}

#[test]
fn running_without_restart_is_rejected() {
    let platform = FakePlatform::new(vec![install(DiscordChannel::Stable)], &[true]);
    let probe = FakeProbe::new(vec![CdpProbeStatus::Unreachable]);
    assert!(matches!(
        launch_with_backends(fast_options(), &platform, &probe),
        Err(LaunchError::DiscordAlreadyRunning { .. })
    ));
}

#[test]
fn restart_terminates_before_spawning() {
    let platform = FakePlatform::new(vec![install(DiscordChannel::Stable)], &[true, false]);
    let probe = FakeProbe::new(vec![
        CdpProbeStatus::Unreachable,
        CdpProbeStatus::Unreachable,
    ]);
    let options = LaunchOptions {
        restart_existing: true,
        wait_for_cdp: false,
        ..patient_options()
    };
    let result = launch_with_backends(options, &platform, &probe).unwrap();
    assert_eq!(result.outcome, LaunchOutcome::Spawned);
    assert_eq!(platform.terminate_count.load(Ordering::SeqCst), 1);
    assert_eq!(platform.spawn_count.load(Ordering::SeqCst), 1);
}

#[test]
fn persistent_process_hits_shutdown_timeout() {
    let platform = FakePlatform::new(vec![install(DiscordChannel::Stable)], &[true]);
    let probe = FakeProbe::new(vec![CdpProbeStatus::Unreachable]);
    let options = LaunchOptions {
        restart_existing: true,
        ..fast_options()
    };
    assert!(matches!(
        launch_with_backends(options, &platform, &probe),
        Err(LaunchError::ShutdownTimeout { .. })
    ));
}

#[test]
fn non_cdp_service_is_reported_as_port_occupied() {
    let platform = FakePlatform::new(vec![install(DiscordChannel::Stable)], &[false]);
    let probe = FakeProbe::new(vec![CdpProbeStatus::PortOccupied]);
    assert!(matches!(
        launch_with_backends(fast_options(), &platform, &probe),
        Err(LaunchError::PortOccupied { port: 9223 })
    ));
}

#[test]
fn non_discord_cdp_target_is_rejected_before_spawn() {
    let platform = FakePlatform::new(vec![install(DiscordChannel::Stable)], &[false]);
    let probe = FakeProbe::new(vec![
        CdpProbeStatus::CdpWithoutDiscordTarget,
        CdpProbeStatus::CdpWithoutDiscordTarget,
    ]);
    assert!(matches!(
        launch_with_backends(fast_options(), &platform, &probe),
        Err(LaunchError::NonDiscordCdpTarget { port: 9223 })
    ));
    assert_eq!(platform.spawn_count.load(Ordering::SeqCst), 0);
}

#[test]
fn zero_port_is_rejected() {
    let platform = FakePlatform::new(vec![install(DiscordChannel::Stable)], &[false]);
    let probe = FakeProbe::new(vec![CdpProbeStatus::Unreachable]);
    let options = LaunchOptions {
        port: 0,
        ..fast_options()
    };
    assert!(matches!(
        launch_with_backends(options, &platform, &probe),
        Err(LaunchError::InvalidPort(0))
    ));
}

#[test]
fn spawn_waits_until_discord_target_is_ready() {
    let platform = FakePlatform::new(vec![install(DiscordChannel::Stable)], &[false]);
    let probe = FakeProbe::new(vec![
        CdpProbeStatus::Unreachable,
        CdpProbeStatus::Unreachable,
        CdpProbeStatus::CdpWithoutDiscordTarget,
        CdpProbeStatus::DiscordReady {
            target_title: Some("Discord".to_string()),
        },
    ]);
    let result = launch_with_backends(patient_options(), &platform, &probe).unwrap();
    assert!(result.cdp_connected);
    assert_eq!(result.pid, Some(4242));
}

#[test]
fn spawn_readiness_timeout_is_typed() {
    let platform = FakePlatform::new(vec![install(DiscordChannel::Stable)], &[false]);
    let probe = FakeProbe::new(vec![CdpProbeStatus::Unreachable]);
    assert!(matches!(
        launch_with_backends(fast_options(), &platform, &probe),
        Err(LaunchError::ReadinessTimeout { port: 9223, .. })
    ));
}

#[test]
fn missing_requested_install_is_typed() {
    let platform = FakePlatform::new(vec![install(DiscordChannel::Stable)], &[false]);
    let probe = FakeProbe::new(vec![CdpProbeStatus::Unreachable]);
    let options = LaunchOptions {
        channel: Some(DiscordChannel::Ptb),
        ..fast_options()
    };
    assert!(matches!(
        launch_with_backends(options, &platform, &probe),
        Err(LaunchError::InstallNotFound {
            channel: Some(DiscordChannel::Ptb)
        })
    ));
}

#[test]
fn auto_selection_is_stable_then_ptb_then_canary() {
    let installs = vec![
        install(DiscordChannel::Canary),
        install(DiscordChannel::Ptb),
        install(DiscordChannel::Stable),
    ];
    assert_eq!(
        select_preferred_install(&installs, None).unwrap().channel,
        DiscordChannel::Stable
    );
    assert_eq!(
        select_preferred_install(&installs[0..2], None)
            .unwrap()
            .channel,
        DiscordChannel::Ptb
    );
    assert!(select_preferred_install(&[], None).is_err());
}

#[test]
fn launch_arguments_include_optional_allow_origins() {
    let with_origins = build_launch_args(9223, true);
    assert_eq!(with_origins[0], "--remote-debugging-port=9223");
    assert_eq!(with_origins[1], "--remote-allow-origins=*");
    assert_eq!(
        build_launch_args(9223, false),
        vec![std::ffi::OsString::from("--remote-debugging-port=9223")]
    );
}

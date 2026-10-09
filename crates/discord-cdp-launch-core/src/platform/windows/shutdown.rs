//! Windows shutdown shared by channel, exact-installation and restore paths.
//! Capture the old Electron tray identities before their windows disappear;
//! never refresh Explorer globally or move the user's pointer.
use crate::{CdpTarget, LaunchError};
use std::collections::HashMap;
use std::mem::size_of;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, FILETIME, HANDLE, HWND, LPARAM, RECT, WAIT_OBJECT_0,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::Networking::WinSock::AF_INET;
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconGetRect, Shell_NotifyIconW, NIF_GUID, NIM_DELETE, NOTIFYICONDATAW,
    NOTIFYICONIDENTIFIER,
};

mod tray_snapshot;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsWindow,
};

const REQUEST_BUDGET: Duration = Duration::from_secs(1);
const GRACE_PERIOD: Duration = Duration::from_secs(2);
const EXIT_BUDGET: Duration = Duration::from_secs(2);
const CAPTURE_BUDGET: Duration = Duration::from_secs(1);
// Electron allocates monotonically increasing IDs starting at 3. Probe a
// bounded range rather than assuming the first icon still has ID 3 after a
// settings change. Check each candidate through the documented Shell API.
const MAX_ICON_ID: u32 = 1024;
const TRAY_WINDOW_CLASS: &str = "Electron_NotifyIconHostWindow";

struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: this handle was opened by us and is closed exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

impl ProcessHandle {
    fn open(pid: u32, expected_start: u64) -> Option<Self> {
        // Holding a native handle lets cleanup wait for the original process,
        // even if Windows has already reused its PID for another application.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return None;
        }
        let handle = Self(handle);
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let success =
            unsafe { GetProcessTimes(handle.0, &mut creation, &mut exit, &mut kernel, &mut user) };
        (success != 0 && filetime_unix_seconds(creation) == Some(expected_start)).then_some(handle)
    }

    fn exited(&self) -> bool {
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
    }
}

fn filetime_unix_seconds(time: FILETIME) -> Option<u64> {
    let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    (ticks / 10_000_000).checked_sub(11_644_473_600)
}

struct CapturedProcess {
    pid: u32,
    handle: ProcessHandle,
    cdp_ports: Vec<u16>,
    executable: Option<PathBuf>,
    start_time: u64,
}

#[derive(Clone, Copy)]
struct TrayIcon {
    window: HWND,
    id: u32,
    owner_pid: u32,
    guid: Option<windows_sys::core::GUID>,
}

impl TrayIcon {
    fn identity_key(self) -> (usize, u32, Option<u128>) {
        let guid = self.guid.map(|guid| {
            windows_core::GUID::from_values(guid.data1, guid.data2, guid.data3, guid.data4)
                .to_u128()
        });
        (self.window as usize, self.id, guid)
    }

    fn identifier(self) -> NOTIFYICONIDENTIFIER {
        NOTIFYICONIDENTIFIER {
            cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: self.window,
            uID: self.id,
            guidItem: self.guid.unwrap_or(windows_sys::core::GUID::from_u128(0)),
        }
    }

    fn exists(self) -> bool {
        let mut rect = RECT::default();
        // This also works for icons in the collapsed overflow area; we do not
        // require visible screen coordinates or any Explorer toolbar layout.
        unsafe { Shell_NotifyIconGetRect(&self.identifier(), &mut rect) >= 0 }
    }

    fn remove_after_exit(self) {
        // A live HWND here belongs to a replacement window. Never delete its
        // icon using a handle recycled since our pre-shutdown snapshot.
        if unsafe { IsWindow(self.window) } != 0 {
            eprintln!("[cdp-shutdown] skipped tray cleanup for a reused window handle");
            return;
        }
        let data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.window,
            uID: self.id,
            uFlags: if self.guid.is_some() { NIF_GUID } else { 0 },
            guidItem: self.guid.unwrap_or(windows_sys::core::GUID::from_u128(0)),
            ..Default::default()
        };
        // An already removed icon or an unavailable Explorer is harmless.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
        if self.exists() {
            eprintln!("[cdp-shutdown] old tray icon could not be removed");
        }
    }
}

pub(crate) struct WindowsShutdown {
    processes: Vec<CapturedProcess>,
    icons: Vec<TrayIcon>,
}

impl WindowsShutdown {
    pub(crate) fn prepare(
        system: &System,
        identities: &HashMap<Pid, (u64, Option<PathBuf>)>,
    ) -> Self {
        let processes: Vec<_> = identities
            .iter()
            .filter_map(|(pid, (start, _))| {
                let handle = ProcessHandle::open(pid.as_u32(), *start)?;
                let cdp_ports = system
                    .process(*pid)
                    .into_iter()
                    .flat_map(|process| process.cmd())
                    .filter_map(|argument| crate::processes::parse_cdp_port(argument))
                    .collect();
                Some(CapturedProcess {
                    pid: pid.as_u32(),
                    handle,
                    cdp_ports,
                    start_time: *start,
                    executable: system
                        .process(*pid)
                        .and_then(|process| process.exe())
                        .map(PathBuf::from),
                })
            })
            .collect();
        let icons = Self::capture_icons(&processes);
        Self { processes, icons }
    }

    fn capture_icons(processes: &[CapturedProcess]) -> Vec<TrayIcon> {
        let windows = electron_tray_windows(processes);
        // Explorer's notification callback gives exact IDs and GUIDs, including
        // hidden icons and IDs beyond our fallback probe range. The same
        // interface is used by Chromium's StatusTrayStateChangerWin.
        if let Some(icons) = tray_snapshot::capture(&windows, processes) {
            return icons;
        }
        eprintln!("[cdp-shutdown] tray enumeration unavailable; probing window/ID icons");
        let deadline = Instant::now() + CAPTURE_BUDGET;
        let mut icons = Vec::new();
        for (window, owner_pid) in windows {
            for id in 1..=MAX_ICON_ID {
                if Instant::now() >= deadline {
                    eprintln!("[cdp-shutdown] tray discovery reached its time budget");
                    return icons;
                }
                let icon = TrayIcon {
                    window,
                    id,
                    owner_pid,
                    guid: None,
                };
                let mut current_pid = 0;
                unsafe { GetWindowThreadProcessId(window, &mut current_pid) };
                if current_pid != owner_pid {
                    break;
                }
                if icon.exists() {
                    icons.push(icon);
                }
            }
        }
        icons
    }

    pub(crate) fn refresh_tray_icons(&mut self) {
        // A vetoed/slow normal quit may recreate the tray while we wait. Include
        // those new identities in the snapshot immediately before the kill.
        for icon in Self::capture_icons(&self.processes) {
            if !self
                .icons
                .iter()
                .any(|old| old.identity_key() == icon.identity_key())
            {
                self.icons.push(icon);
            }
        }
    }

    pub(crate) fn request_graceful_exit(&self) {
        let deadline = Instant::now() + REQUEST_BUDGET;
        let mut attempted = false;
        for process in &self.processes {
            for &port in &process.cdp_ports {
                if Instant::now() >= deadline || process.handle.exited() {
                    continue;
                }
                // A command-line flag alone does not establish port ownership.
                // Never execute a quit command against some other local client.
                if !port_owned_by(port, process.pid) {
                    continue;
                }
                attempted |= request_native_quit(port, deadline, || {
                    !process.handle.exited() && port_owned_by(port, process.pid)
                });
            }
        }
        if attempted {
            let deadline = Instant::now() + GRACE_PERIOD;
            while Instant::now() < deadline {
                if self.processes.iter().all(|process| process.handle.exited()) {
                    eprintln!("[cdp-shutdown] client exited normally");
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            eprintln!("[cdp-shutdown] native quit timed out; falling back to process termination");
        }
    }

    pub(crate) fn finish(self) -> Result<(), LaunchError> {
        let deadline = Instant::now() + EXIT_BUDGET;
        for process in &self.processes {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let milliseconds = remaining.as_millis().min(u128::from(u32::MAX - 1)) as u32;
            if unsafe { WaitForSingleObject(process.handle.0, milliseconds) } != WAIT_OBJECT_0 {
                return Err(LaunchError::ProcessTermination {
                    process: process.pid.to_string(),
                    details: "the original Windows process did not finish exiting".into(),
                });
            }
        }
        eprintln!(
            "[cdp-shutdown] checked {} old tray icon(s)",
            self.icons.len()
        );
        Ok(())
    }
}

impl Drop for WindowsShutdown {
    fn drop(&mut self) {
        // Also clean icons of already terminated targets if another member of
        // the tree refused termination. Leave every still-running owner alone.
        for icon in &self.icons {
            if let Some(process) = self
                .processes
                .iter()
                .find(|process| process.pid == icon.owner_pid)
            {
                if !process.handle.exited() {
                    continue;
                }
                // A GUID can be reused by an externally relaunched instance.
                // Preserve that instance even if its callback HWND is different.
                if icon.guid.is_some()
                    && process
                        .executable
                        .as_deref()
                        .is_none_or(crate::is_installation_running)
                {
                    eprintln!(
                        "[cdp-shutdown] skipped GUID cleanup while a replacement client is running"
                    );
                    continue;
                }
                icon.remove_after_exit();
            }
        }
    }
}

fn electron_tray_windows(processes: &[CapturedProcess]) -> Vec<(HWND, u32)> {
    struct Context<'a> {
        processes: &'a [CapturedProcess],
        windows: Vec<(HWND, u32)>,
    }
    unsafe extern "system" fn visit(window: HWND, context: LPARAM) -> i32 {
        // SAFETY: EnumWindows is synchronous; this context lives for the call.
        let context = unsafe { &mut *(context as *mut Context<'_>) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if !context
            .processes
            .iter()
            .any(|process| process.pid == pid && !process.handle.exited())
        {
            return 1;
        }
        let mut name = [0u16; 128];
        let length = unsafe { GetClassNameW(window, name.as_mut_ptr(), name.len() as i32) };
        if length > 0 && String::from_utf16_lossy(&name[..length as usize]) == TRAY_WINDOW_CLASS {
            context.windows.push((window, pid));
        }
        1
    }
    let mut context = Context {
        processes,
        windows: Vec::new(),
    };
    unsafe { EnumWindows(Some(visit), &mut context as *mut _ as LPARAM) };
    context.windows
}

fn request_native_quit(port: u16, deadline: Instant, still_owned: impl Fn() -> bool) -> bool {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return false;
    }
    let Ok(targets) = crate::list_cdp_targets_with_timeouts(port, remaining / 2, remaining / 2)
    else {
        return false;
    };
    for target in targets.iter().filter(|target| quit_target(target)) {
        if Instant::now() >= deadline || !still_owned() {
            break;
        }
        if crate::runtime::close_browser(port, target, deadline) {
            return true;
        }
    }
    false
}

fn quit_target(target: &CdpTarget) -> bool {
    crate::is_discord_target(target)
        && !crate::is_discord_auxiliary_window(target)
        && target.web_socket_debugger_url.is_some()
}

fn port_owned_by(port: u16, pid: u32) -> bool {
    let mut bytes = 0;
    let result = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut bytes,
            0,
            u32::from(AF_INET),
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if result != ERROR_INSUFFICIENT_BUFFER {
        return false;
    }
    // u32 storage provides the table's alignment, unlike a Vec<u8> cast.
    for _ in 0..3 {
        if bytes > 16 * 1024 * 1024 {
            return false;
        }
        let mut storage = vec![0u32; (bytes as usize).div_ceil(size_of::<u32>())];
        let result = unsafe {
            GetExtendedTcpTable(
                storage.as_mut_ptr().cast(),
                &mut bytes,
                0,
                u32::from(AF_INET),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if result == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if result != 0 || bytes < size_of::<u32>() as u32 {
            return false;
        }
        let count = storage[0] as usize;
        let Some(required) = count
            .checked_mul(size_of::<MIB_TCPROW_OWNER_PID>())
            .and_then(|size| size.checked_add(size_of::<u32>()))
        else {
            return false;
        };
        if required > bytes as usize || required > storage.len() * size_of::<u32>() {
            return false;
        }
        let rows = unsafe {
            std::slice::from_raw_parts(
                storage.as_ptr().add(1).cast::<MIB_TCPROW_OWNER_PID>(),
                count,
            )
        };
        return rows
            .iter()
            .any(|row| row.dwOwningPid == pid && u16::from_be(row.dwLocalPort as u16) == port);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};

    #[test]
    fn listener_ownership_is_checked_against_the_operating_system() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_owned_by(port, std::process::id()));
        assert!(!port_owned_by(port, u32::MAX));
    }

    #[test]
    fn original_process_handle_rejects_a_mismatched_creation_time() {
        assert!(ProcessHandle::open(std::process::id(), 0).is_none());
    }

    #[test]
    fn creation_time_conversion_handles_the_windows_epoch() {
        let ticks = (11_644_473_600 + 123) * 10_000_000u64;
        assert_eq!(
            filetime_unix_seconds(FILETIME {
                dwLowDateTime: ticks as u32,
                dwHighDateTime: (ticks >> 32) as u32,
            }),
            Some(123)
        );
        assert_eq!(filetime_unix_seconds(FILETIME::default()), None);
    }

    #[test]
    fn quit_is_not_sent_when_ownership_changes_after_discovery() {
        use std::io::{Read, Write};
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer).unwrap();
            let body = serde_json::json!([{
                "id":"a", "type":"page", "title":"Discord",
                "url":"https://discord.com/app",
                "webSocketDebuggerUrl":format!("ws://127.0.0.1:{port}/devtools/page/a")
            }])
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            listener
        });
        assert!(!request_native_quit(
            port,
            Instant::now() + Duration::from_secs(2),
            || false
        ));
        let listener = server.join().unwrap();
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    struct Fixture {
        child: Child,
        executable: PathBuf,
        icon: TrayIcon,
        guid: bool,
    }

    impl Fixture {
        fn start(arguments: &[String], guid: bool) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let directory = std::env::temp_dir().join(format!(
                "dqh-tray-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            let source = std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("examples/windows_tray_fixture.exe");
            let executable = directory.join("fixture.exe");
            std::fs::copy(&source, &executable).expect(
                "first run: cargo build -p discord-cdp-launch-core --example windows_tray_fixture",
            );
            let mut child = Command::new(&executable)
                .args(arguments)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut line = String::new();
                let _ = BufReader::new(stdout).read_line(&mut line);
                let _ = sender.send(line);
            });
            let icon = TrayIcon {
                window: std::ptr::null_mut(),
                id: 0,
                owner_pid: child.id(),
                guid: None,
            };
            let mut fixture = Self {
                child,
                executable,
                icon,
                guid,
            };
            let line = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
            let info: Value =
                serde_json::from_str(&line).expect("native tray fixture did not become ready");
            fixture.icon.window = info["window"].as_u64().unwrap() as usize as HWND;
            fixture.icon.id = info["id"].as_u64().unwrap() as u32;
            if guid {
                fixture.icon.guid = Some(windows_sys::core::GUID::from_u128(
                    0xc9b70755_8da9_47b3_a8cb_001000000000 | u128::from(fixture.child.id()),
                ));
            }
            if let Some(port) = arguments
                .iter()
                .find_map(|argument| crate::processes::parse_cdp_port(argument.as_ref()))
            {
                // Window creation alone does not prove the CDP worker is ready.
                // Wait for a complete discovery response before timing shutdown.
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    if crate::list_cdp_targets_with_timeouts(
                        port,
                        Duration::from_millis(100),
                        Duration::from_millis(100),
                    )
                    .is_ok_and(|targets| targets.iter().any(quit_target))
                    {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "fixture CDP worker was not ready"
                    );
                    assert!(fixture.child.try_wait().unwrap().is_none());
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            fixture
        }

        fn snapshot(&self) -> WindowsShutdown {
            let mut system = System::new();
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing()
                    .with_exe(UpdateKind::OnlyIfNotSet)
                    .with_cmd(UpdateKind::OnlyIfNotSet)
                    .without_tasks(),
            );
            let pid = Pid::from_u32(self.child.id());
            let process = system.process(pid).unwrap();
            WindowsShutdown::prepare(
                &system,
                &HashMap::from([(
                    pid,
                    (process.start_time(), process.exe().map(PathBuf::from)),
                )]),
            )
        }

        fn second_guid_icon(&self) -> TrayIcon {
            TrayIcon {
                guid: Some(windows_sys::core::GUID::from_u128(
                    0xc9b70756_8da9_47b3_a8cb_001000000000 | u128::from(self.child.id()),
                )),
                ..self.icon
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // This fixture owns its exact ID/GUID, including after a failing test.
            let mut data = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.icon.window,
                uID: self.icon.id,
                ..Default::default()
            };
            if self.guid {
                data.uFlags = NIF_GUID;
                data.guidItem = windows_sys::core::GUID::from_u128(
                    0xc9b70755_8da9_47b3_a8cb_001000000000 | u128::from(self.child.id()),
                );
            }
            unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
            if self.guid {
                data.guidItem = self.second_guid_icon().guid.unwrap();
                unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.executable);
            let _ = std::fs::remove_dir(self.executable.parent().unwrap());
        }
    }

    #[test]
    #[ignore = "requires Explorer; build the windows_tray_fixture example first"]
    fn native_forced_exit_cleans_only_the_selected_installation() {
        let mut target = Fixture::start(&["--icon-id=17".into()], false);
        let mut unrelated = Fixture::start(&[], false);
        assert!(target.icon.exists());
        assert!(unrelated.icon.exists());
        let snapshot = target.snapshot();
        assert_eq!(snapshot.icons.len(), 1);
        drop(snapshot); // dropping preparation while the owner is alive is harmless
        assert!(target.icon.exists());
        crate::terminate_installation_process_tree(&target.executable).unwrap();
        assert!(!target.child.wait().unwrap().success());
        assert!(!target.icon.exists());
        assert!(unrelated.child.try_wait().unwrap().is_none());
        assert!(unrelated.icon.exists());
    }

    #[test]
    #[ignore = "requires Explorer; build the windows_tray_fixture example first"]
    fn native_guid_icon_is_captured_and_removed_after_forced_exit() {
        let mut target = Fixture::start(&["--guid".into()], true);
        assert!(target.icon.exists());
        assert_eq!(target.snapshot().icons.len(), 1);
        crate::terminate_installation_process_tree(&target.executable).unwrap();
        assert!(!target.child.wait().unwrap().success());
        assert!(!target.icon.exists());
    }

    #[test]
    #[ignore = "requires Explorer; build the windows_tray_fixture example first"]
    fn native_distinct_and_recreated_guids_survive_snapshot_deduplication() {
        for recreate in [false, true] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            drop(listener);
            let mut arguments = vec!["--guid".into()];
            if recreate {
                arguments.extend([
                    format!("--remote-debugging-port={port}"),
                    "--ignore-close".into(),
                    "--recreate-guid-on-close".into(),
                ]);
            } else {
                arguments.push("--second-guid".into());
            }
            let mut target = Fixture::start(&arguments, true);
            let second = target.second_guid_icon();
            assert!(target.icon.exists());
            assert_eq!(second.exists(), !recreate);
            let mut snapshot = target.snapshot();
            assert_eq!(snapshot.icons.len(), if recreate { 1 } else { 2 });
            if recreate {
                assert_eq!(snapshot.processes.len(), 1);
                assert_eq!(snapshot.processes[0].cdp_ports, vec![port]);
                assert!(port_owned_by(port, target.child.id()));
                snapshot.request_graceful_exit();
                assert!(!target.icon.exists());
                assert!(second.exists());
            }
            snapshot.refresh_tray_icons();
            assert_eq!(snapshot.icons.len(), 2);
            target.child.kill().unwrap();
            target.child.wait().unwrap();
            snapshot.finish().unwrap();
            assert!(!target.icon.exists());
            assert!(!second.exists());
        }
    }

    #[test]
    #[ignore = "requires Explorer; build the windows_tray_fixture example first"]
    fn native_high_icon_id_is_captured_without_assuming_a_fresh_electron_process() {
        let mut target = Fixture::start(&["--icon-id=5000".into()], false);
        assert_eq!(target.snapshot().icons.len(), 1);
        crate::terminate_installation_process_tree(&target.executable).unwrap();
        target.child.wait().unwrap();
        assert!(!target.icon.exists());
    }

    #[test]
    #[ignore = "requires Explorer; build the windows_tray_fixture example first"]
    fn native_live_owner_and_window_are_preserved() {
        let mut target = Fixture::start(&[], false);
        assert!(target.snapshot().finish().is_err());
        assert!(target.icon.exists());
        target.icon.remove_after_exit(); // the live-window guard also skips this original HWND
        assert!(target.icon.exists());
        assert!(target.child.try_wait().unwrap().is_none());
    }

    #[test]
    #[ignore = "requires Explorer; build the windows_tray_fixture example first"]
    fn native_cdp_exit_is_graceful_and_a_hung_client_falls_back_to_kill() {
        for ignore_close in [false, true] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            drop(listener);
            let mut arguments = vec![format!("--remote-debugging-port={port}")];
            if ignore_close {
                arguments.push("--ignore-close".into());
            }
            let mut target = Fixture::start(&arguments, false);
            assert!(target.icon.exists());
            let start = Instant::now();
            crate::terminate_installation_process_tree(&target.executable).unwrap();
            assert_eq!(target.child.wait().unwrap().success(), !ignore_close);
            assert!(!target.icon.exists());
            assert!(start.elapsed() < Duration::from_secs(6));
        }
    }
}

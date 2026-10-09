//! Read-only, short-lived Explorer notification enumeration. This versioned
//! COM interface is also used by Chromium's status_tray_state_changer_win.cc.
//! It is undocumented: failure falls back to the documented window/ID probe.
use super::{CapturedProcess, ProcessHandle, TrayIcon, CAPTURE_BUDGET};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use windows_core::{implement, interface, IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

#[repr(C)]
struct NotifyItem {
    executable: *const u16,
    tooltip: *const u16,
    icon: usize,
    window: usize,
    preference: u32,
    id: u32,
    guid: GUID,
}

#[interface("D782CCBA-AFB0-43F1-94DB-FDA3779EACCB")]
unsafe trait INotificationCallback: IUnknown {
    fn notify(&self, event: u32, item: *const NotifyItem) -> HRESULT;
}

#[interface("D133CE13-3537-48BA-93A7-AFCD5D2053B4")]
unsafe trait ITrayNotify: IUnknown {
    fn register_callback(&self, callback: *mut c_void, registration: *mut u32) -> HRESULT;
    fn unregister_callback(&self, registration: *mut u32) -> HRESULT;
}

#[derive(Clone)]
struct Record {
    window: usize,
    owner_pid: u32,
    id: u32,
    guid: GUID,
}

#[implement(INotificationCallback)]
struct Callback {
    windows: HashMap<usize, (u32, u64)>,
    records: Arc<Mutex<Vec<Record>>>,
}

impl INotificationCallback_Impl for Callback_Impl {
    unsafe fn notify(&self, _event: u32, item: *const NotifyItem) -> HRESULT {
        if item.is_null() {
            return HRESULT(0);
        }
        // SAFETY: the COM callback supplies a marshalled NotifyItem for this call.
        // Copy only its scalar identity fields, never executable/tooltip strings.
        let item = unsafe { &*item };
        let Some(&(owner_pid, start_time)) = self.windows.get(&item.window) else {
            return HRESULT(0);
        };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(item.window as _, &mut pid) };
        if pid != owner_pid || ProcessHandle::open(pid, start_time).is_none() {
            return HRESULT(0);
        }
        self.records
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(Record {
                window: item.window,
                owner_pid,
                id: item.id,
                guid: item.guid,
            });
        HRESULT(0)
    }
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn enumerate(windows: HashMap<usize, (u32, u64)>) -> Option<Vec<Record>> {
    // Use a dedicated apartment so Tauri worker threads' COM initialization
    // cannot conflict with this callback's synchronous registration.
    if unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) } < 0 {
        return None;
    }
    let _apartment = Apartment;
    let class = windows_sys::core::GUID::from_u128(0x25dead04_1eac_4911_9e3a_ad0a4ab560fd);
    let mut raw = std::ptr::null_mut();
    let iid = windows_sys::core::GUID::from_u128(0xd133ce13_3537_48ba_93a7_afcd5d2053b4);
    if unsafe {
        CoCreateInstance(
            &class,
            std::ptr::null_mut(),
            CLSCTX_LOCAL_SERVER,
            &iid,
            &mut raw,
        )
    } < 0
    {
        return None;
    }
    // SAFETY: CoCreateInstance returned exactly ITrayNotify with an owned reference.
    let notifier = unsafe { ITrayNotify::from_raw(raw) };
    let records = Arc::new(Mutex::new(Vec::new()));
    let callback: INotificationCallback = Callback {
        windows,
        records: records.clone(),
    }
    .into();
    let mut registration = 0;
    let registered = unsafe { notifier.register_callback(callback.as_raw(), &mut registration) };
    // Explorer enumerates synchronously during RegisterCallback. Always unregister;
    // COM owns any outstanding callback references if this fails during shutdown.
    let unregistered = unsafe { notifier.unregister_callback(&mut registration) };
    if registered.is_err() || unregistered.is_err() {
        return None;
    }
    let result = records
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    Some(result)
}

pub(super) fn capture(
    windows: &[(windows_sys::Win32::Foundation::HWND, u32)],
    processes: &[CapturedProcess],
) -> Option<Vec<TrayIcon>> {
    if windows.is_empty() {
        return Some(Vec::new());
    }
    // A timed-out COM call may still be waiting for Explorer. Keep at most one
    // such worker, rather than leaking a new thread on every restart attempt.
    static BUSY: AtomicBool = AtomicBool::new(false);
    struct Permit;
    impl Drop for Permit {
        fn drop(&mut self) {
            BUSY.store(false, Ordering::Release);
        }
    }
    BUSY.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .ok()?;
    let permit = Permit;
    let windows = windows
        .iter()
        .filter_map(|&(window, pid)| {
            processes
                .iter()
                .find(|process| process.pid == pid)
                .map(|process| (window as usize, (pid, process.start_time)))
        })
        .collect();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("tray-snapshot".into())
        .spawn(move || {
            let _permit = permit;
            let _ = sender.send(enumerate(windows));
        })
        .ok()?;
    // A hung Explorer must not indefinitely delay restarting the client. A
    // late callback owns all its state and unregisters when the call returns.
    let records = receiver.recv_timeout(CAPTURE_BUDGET).ok()??;
    let mut icons: Vec<_> = records
        .into_iter()
        .map(|record| TrayIcon {
            window: record.window as _,
            id: record.id,
            owner_pid: record.owner_pid,
            guid: (record.guid != GUID::zeroed()).then_some(windows_sys::core::GUID {
                data1: record.guid.data1,
                data2: record.guid.data2,
                data3: record.guid.data3,
                data4: record.guid.data4,
            }),
        })
        .collect();
    // GUID identifies a separate Shell registration even when HWND/ID match.
    // Use the same complete identity when merging a later shutdown snapshot.
    icons.sort_by_key(|icon| icon.identity_key());
    icons.dedup_by_key(|icon| icon.identity_key());
    Some(icons)
}

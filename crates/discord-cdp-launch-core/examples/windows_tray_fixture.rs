//! Disposable native tray process for the opt-in interactive Windows tests.
//! No Discord installation, login, network API or user data is used.
#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
fn main() {
    fixture::run();
}

#[cfg(target_os = "windows")]
mod fixture {
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_GUID, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
        NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, LoadIconW, PeekMessageW,
        RegisterClassW, TranslateMessage, IDI_APPLICATION, MSG, PM_REMOVE, WNDCLASSW, WS_POPUP,
    };

    pub fn run() {
        let args: Vec<_> = std::env::args().collect();
        let ignore_close = args.iter().any(|arg| arg == "--ignore-close");
        let use_guid = args.iter().any(|arg| arg == "--guid");
        let port = args.iter().find_map(|arg| {
            arg.strip_prefix("--remote-debugging-port=")
                .and_then(|port| port.parse::<u16>().ok())
        });
        let id = args
            .iter()
            .find_map(|arg| {
                arg.strip_prefix("--icon-id=")
                    .and_then(|id| id.parse::<u32>().ok())
            })
            .unwrap_or(3);
        let quit = Arc::new(AtomicBool::new(false));
        if let Some(port) = port {
            let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
            let quit = quit.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else {
                        break;
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut header = [0u8; 4096];
                    let length = stream.peek(&mut header).unwrap();
                    if header[..length].starts_with(b"GET /json ") {
                        let _ = stream.read(&mut header);
                        let body = json!([{
                            "id": "fixture", "type": "page", "title": "Shutdown fixture",
                            "url": "https://discord.com/shutdown-fixture",
                            "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/fixture")
                        }]).to_string();
                        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                        continue;
                    }
                    let mut socket = tungstenite::accept(stream).unwrap();
                    let request = socket.read().unwrap().into_text().unwrap();
                    let request: serde_json::Value = serde_json::from_str(&request).unwrap();
                    assert_eq!(request["method"], "Browser.close");
                    if ignore_close {
                        let _ = socket.send(tungstenite::Message::Text(
                            json!({"id": request["id"], "result": {}})
                                .to_string()
                                .into(),
                        ));
                    } else {
                        // Match Electron: quit without sending an acknowledgment.
                        quit.store(true, Ordering::SeqCst);
                        let _ = socket.close(None);
                    }
                }
            });
        }
        let class: Vec<u16> = "Electron_NotifyIconHostWindow\0".encode_utf16().collect();
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassW(&window_class) }, 0);
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        assert!(!window.is_null());
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: id,
            uFlags: NIF_ICON | NIF_TIP | NIF_MESSAGE,
            uCallbackMessage: 0x8001,
            hIcon: unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) },
            ..Default::default()
        };
        for (slot, character) in data
            .szTip
            .iter_mut()
            .zip("DQH shutdown fixture".encode_utf16())
        {
            *slot = character;
        }
        if use_guid {
            data.uFlags |= NIF_GUID;
            data.guidItem = windows_sys::core::GUID::from_u128(
                0xc9b70755_8da9_47b3_a8cb_001000000000 | u128::from(std::process::id()),
            );
        }
        assert_ne!(
            unsafe { Shell_NotifyIconW(NIM_ADD, &data) },
            0,
            "Explorer must be running"
        );
        println!("{}", json!({"window": window as usize, "id": id}));
        std::io::stdout().flush().unwrap();
        let mut message = MSG::default();
        while !quit.load(Ordering::SeqCst) {
            while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0
            {
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_ne!(unsafe { Shell_NotifyIconW(NIM_DELETE, &data) }, 0);
        unsafe { DestroyWindow(window) };
    }
}

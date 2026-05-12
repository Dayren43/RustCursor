//! Monitor-layout hot-reload via `WM_DISPLAYCHANGE`. We register a hidden
//! top-level window so the broadcast (which Windows only sends to top-level
//! windows, not message-only ones) reaches us. The window lives on the main
//! thread and its messages flow through the tray's existing `GetMessageW`
//! pump, with no extra thread and no polling.

use std::collections::HashMap;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, PostMessageW, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_DISPLAYCHANGE, WNDCLASSW,
};
use windows::core::{PCWSTR, w};

use rust_cursor::core::Monitor;

use super::monitors::build_monitor_map;

static MONITORS: OnceLock<Arc<RwLock<HashMap<String, Monitor>>>> = OnceLock::new();

/// HWND of the listener window, published once `CreateWindowExW` succeeds.
/// `trigger_monitor_rebuild` reads this so the GUI thread can post a
/// `WM_DISPLAYCHANGE` to the listener after writing config.
static LISTENER_HWND: AtomicIsize = AtomicIsize::new(0);

/// Register a hidden top-level window that rebuilds the shared monitor map on
/// every `WM_DISPLAYCHANGE`. Must be called on the thread that will pump
/// Win32 messages (the main thread, where `run_tray_loop` lives).
pub fn register_display_listener(monitors: Arc<RwLock<HashMap<String, Monitor>>>) {
    if MONITORS.set(monitors).is_err() {
        return; // already registered; only one listener per process
    }

    unsafe {
        let h_instance = GetModuleHandleW(PCWSTR::null()).expect("GetModuleHandleW");
        let class_name = w!("RustCursorDisplayListener");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: HINSTANCE(h_instance.0),
            lpszClassName: class_name,
            ..Default::default()
        };
        // RegisterClassW returns 0 on failure; ignore. If registration fails because
        // the class already exists (unlikely for our private name), CreateWindowExW
        // will still succeed using the existing class.
        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("RustCursorDisplayListener"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(HINSTANCE(h_instance.0)),
            None,
        )
        .expect("CreateWindowExW (display listener)");
        LISTENER_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    }
}

/// Post a `WM_DISPLAYCHANGE` to the listener window so the monitor map gets
/// rebuilt with the currently-installed sizes/positions. Used by the GUI's
/// save path to surface config edits without a restart. Safe to call from
/// any thread because `PostMessageW` is cross-thread.
pub fn trigger_monitor_rebuild() {
    let raw = LISTENER_HWND.load(Ordering::SeqCst);
    if raw == 0 {
        return;
    }
    unsafe {
        let _ = PostMessageW(
            Some(HWND(raw as *mut _)),
            WM_DISPLAYCHANGE,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_DISPLAYCHANGE {
        if let Some(monitors) = MONITORS.get() {
            let fresh = build_monitor_map();
            *monitors.write().unwrap() = fresh;
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

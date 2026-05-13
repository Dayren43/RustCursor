//! Monitor-layout hot-reload via `WM_DISPLAYCHANGE`, plus IPC reload from
//! the Settings subprocess via `WM_RUSTCURSOR_RELOAD`. We register a hidden
//! top-level window so the broadcast (which Windows only sends to top-level
//! windows, not message-only ones) reaches us. The window lives on the main
//! thread and its messages flow through the tray's existing `GetMessageW`
//! pump, with no extra thread and no polling.
//!
//! The window class name `RustCursorDisplayListener` is the well-known hook
//! the Settings subprocess uses to find this window via `FindWindowExW` and
//! post `WM_RUSTCURSOR_RELOAD` after writing config.toml.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_DISPLAYCHANGE, WNDCLASSW,
};
use windows::core::{PCWSTR, w};

use rust_cursor::core::Monitor;

use super::monitors::{build_monitor_map, enumerate_hwids};

/// Private message posted by the Settings subprocess after it writes
/// `config.toml`. Handler re-reads the file, refreshes the runtime
/// `SIZES` + `BYPASS` lookups, and rebuilds the monitor map so the
/// changes apply without a parent restart.
const WM_RUSTCURSOR_RELOAD: u32 = WM_APP + 1;

static MONITORS: OnceLock<Arc<RwLock<HashMap<String, Monitor>>>> = OnceLock::new();

/// Register a hidden top-level window that rebuilds the shared monitor map on
/// every `WM_DISPLAYCHANGE` (display plug/unplug) and on
/// `WM_RUSTCURSOR_RELOAD` (Settings subprocess saved config). Must be called
/// on the thread that will pump Win32 messages (the main thread, where
/// `run_tray_loop` lives).
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

        let _ = CreateWindowExW(
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
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_DISPLAYCHANGE => {
            if let Some(monitors) = MONITORS.get() {
                let fresh = build_monitor_map();
                *monitors.write().unwrap() = fresh;
            }
            LRESULT(0)
        }
        WM_RUSTCURSOR_RELOAD => {
            // Settings subprocess wrote config.toml and posted this. Re-load
            // config, re-resolve the active profile for currently-connected
            // HWIDs, hot-swap the runtime SIZES/BYPASS lookups, then rebuild
            // the monitor map so cursor crossings reflect the new layout.
            let cfg = rust_cursor::config::Config::load();
            let hwids = enumerate_hwids();
            let profile_monitors = cfg
                .active_profile(&hwids)
                .map(|p| p.monitors.clone())
                .unwrap_or_default();
            rust_cursor::config::install_active_profile(profile_monitors, cfg.default_size_in);
            rust_cursor::config::install_bypass_processes(cfg.bypass_processes);
            if let Some(monitors) = MONITORS.get() {
                let fresh = build_monitor_map();
                *monitors.write().unwrap() = fresh;
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

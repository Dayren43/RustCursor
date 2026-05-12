//! Settings GUI. Hidden by default; opened from the tray "Settings…" menu item.
//!
//! winit allows only one `EventLoop` per process, so we can't spawn a fresh
//! window per click. The GUI thread is started on the first click and runs
//! for the lifetime of the process. Show/hide is driven through the raw
//! HWND via Win32 `ShowWindow`, which is more reliable than egui's
//! `ViewportCommand::Visible` for a window that has been hidden away from
//! the event loop.

mod app;
mod autostart;
mod config_io;
mod tabs;

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    SW_HIDE, SW_SHOW, SetForegroundWindow, ShowWindow,
};

static THREAD_SPAWNED: AtomicBool = AtomicBool::new(false);

/// Settings-window HWND as `isize`. `0` means the window hasn't reached its
/// first paint yet, in which case clicking Settings… is a brief no-op.
static HWND_PTR: AtomicIsize = AtomicIsize::new(0);

/// Called from the eframe app's first frame to publish the HWND so the tray
/// thread can show/hide us without going through egui.
pub(crate) fn set_hwnd(hwnd: isize) {
    HWND_PTR.store(hwnd, Ordering::SeqCst);
}

pub fn open_settings_window() {
    let hwnd = HWND_PTR.load(Ordering::SeqCst);
    if hwnd != 0 {
        unsafe {
            let h = HWND(hwnd as *mut _);
            let _ = ShowWindow(h, SW_SHOW);
            let _ = SetForegroundWindow(h);
        }
        return;
    }
    if THREAD_SPAWNED.swap(true, Ordering::SeqCst) {
        // Spawn is in flight; the HWND will appear on first paint.
        return;
    }
    std::thread::spawn(|| {
        app::run();
    });
}

/// Hide the settings window. Called from the close-button intercept inside
/// the eframe app to keep the EventLoop alive while making the window
/// disappear from screen and taskbar.
pub(crate) fn hide_window() {
    let hwnd = HWND_PTR.load(Ordering::SeqCst);
    if hwnd != 0 {
        unsafe {
            let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
        }
    }
}

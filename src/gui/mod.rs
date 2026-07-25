//! Settings GUI plumbing. The actual eframe app lives in `app`; this module
//! contains:
//!  - the IPC signal the subprocess uses to tell the parent its config was
//!    updated (`signal_parent_reload`),
//!  - the spawn helper the tray uses to launch the subprocess
//!    (`spawn_settings_subprocess`).
//!
//! Architecture rationale: opening a GPU-accelerated eframe window in the
//! main process loads OpenGL drivers whose background threads stay alive
//! for the process lifetime and consume ~6% CPU on every mouse-move event,
//! even after the window is hidden. winit only allows one `EventLoop` per
//! process, so we can't recreate the GUI cleanly. Running Settings as a
//! separate child process means the cost is paid only while the window is
//! open, and `Drop` reclaims everything on close.

pub(crate) mod app;
mod autostart;
mod config_io;
mod tabs;

use std::os::windows::ffi::OsStrExt;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, PostMessageW, WM_APP, WM_CLOSE};
use windows::core::PCWSTR;

/// Title of the Settings window. `app.rs` sets it on the viewport;
/// `close_settings_windows` finds windows by it when the parent quits.
pub(crate) const SETTINGS_WINDOW_TITLE: &str = "RustCursor Settings";

/// Mirror of the parent's `WM_RUSTCURSOR_RELOAD` constant. Kept here as well
/// so the subprocess (which doesn't link the platform/windows/layout.rs
/// listener) has its own copy. The numeric value MUST stay in sync with
/// `platform::windows::layout`.
const WM_RUSTCURSOR_RELOAD: u32 = WM_APP + 1;

/// Class name of the parent's hidden listener window. Mirrors the literal
/// passed to `RegisterClassW` / `CreateWindowExW` in `platform::windows::layout`.
const LISTENER_CLASS: &str = "RustCursorDisplayListener";

/// Called from tab save paths after a successful `config.toml` write. Does
/// the subprocess-local equivalent of what the in-process GUI used to do:
///  1. Re-reads `config.toml` from disk.
///  2. Resolves the active profile against currently-connected HWIDs.
///  3. Re-installs the subprocess's own `SIZES` and `BYPASS` lookups so the
///     Monitors tab's subsequent `build_monitor_map` calls see the new values.
///  4. Signals the parent process to do the same (so cursor remapping reflects
///     the edit without waiting for the user to close Settings).
pub(crate) fn reload_active_profile() {
    let cfg = rust_cursor::config::Config::load();
    let hwids = crate::platform::windows::enumerate_hwids();
    let profile_monitors = cfg
        .active_profile(&hwids)
        .map(|p| p.monitors.clone())
        .unwrap_or_default();
    rust_cursor::config::install_active_profile(profile_monitors, cfg.default_size_in);
    rust_cursor::config::install_bypass_processes(cfg.bypass_processes);
    signal_parent_reload();
}

/// Finds the parent's display-listener window by its registered class name
/// and posts `WM_RUSTCURSOR_RELOAD`. A null result (parent not running, or
/// this is a standalone launch) is silently ignored: the on-disk config is
/// the source of truth and the change will surface on the next parent start.
fn signal_parent_reload() {
    let class_wide: Vec<u16> = std::ffi::OsStr::new(LISTENER_CLASS)
        .encode_wide()
        .chain([0])
        .collect();
    unsafe {
        let hwnd = FindWindowExW(None, None, PCWSTR(class_wide.as_ptr()), PCWSTR::null());
        let Ok(hwnd) = hwnd else { return };
        if hwnd.0.is_null() {
            return;
        }
        let _ = PostMessageW(Some(hwnd), WM_RUSTCURSOR_RELOAD, WPARAM(0), LPARAM(0));
    }
}

/// Called by the parent after the tray pump exits (Quit selected). Settings
/// subprocesses are independent processes, so quitting the parent would
/// otherwise leave their windows orphaned. Posts `WM_CLOSE` to every
/// top-level window with the Settings title, the graceful equivalent of
/// clicking X, so eframe unwinds and each subprocess exits on its own. A
/// subprocess whose window isn't created yet is missed; that race is a few
/// hundred ms wide and the leftover window is still individually closable.
pub fn close_settings_windows() {
    let title_wide: Vec<u16> = std::ffi::OsStr::new(SETTINGS_WINDOW_TITLE)
        .encode_wide()
        .chain([0])
        .collect();
    unsafe {
        let mut after: Option<HWND> = None;
        loop {
            let found = FindWindowExW(None, after, PCWSTR::null(), PCWSTR(title_wide.as_ptr()));
            let Ok(hwnd) = found else { break };
            if hwnd.0.is_null() {
                break;
            }
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            after = Some(hwnd);
        }
    }
}

/// Called from the tray's "Settings…" handler. Launches a new
/// `RustCursor.exe --settings` process via `Command::spawn`, fire-and-forget.
/// Errors are silently swallowed (no way to surface them from a tray click
/// without an extra dialog dependency, and the only realistic failure is
/// `CreateProcess` denied which the user would notice immediately).
///
/// Each click spawns a fresh process; multiple Settings windows are
/// theoretically possible if the user double-clicks rapidly, but the
/// subprocess startup time (~hundreds of ms) makes that rare in practice
/// and the windows don't interfere with each other (each has its own
/// `EventLoop`, both write through the same `ConfigDoc` save path).
pub fn spawn_settings_subprocess() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = std::process::Command::new(exe).arg("--settings").spawn();
}

/// Entry point for the `RustCursor.exe --settings` subprocess. Loads config,
/// installs the active profile into the subprocess's own `SIZES` lookup so
/// the Monitors tab's `build_monitor_map` reads match what the parent uses,
/// then runs the eframe app until the user closes the window.
pub fn run_settings_subprocess() {
    crate::platform::windows::setup_dpi_awareness();
    let cfg = rust_cursor::config::Config::load();
    let hwids = crate::platform::windows::enumerate_hwids();
    let profile_monitors = cfg
        .active_profile(&hwids)
        .map(|p| p.monitors.clone())
        .unwrap_or_default();
    rust_cursor::config::install_active_profile(profile_monitors, cfg.default_size_in);
    app::run();
}

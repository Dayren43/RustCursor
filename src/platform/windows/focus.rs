//! Foreground-focus checks that decide whether to pause cursor remapping.
//!
//! Two signals drive the decision:
//!  1. `SHQueryUserNotificationState() == QUNS_RUNNING_D3D_FULL_SCREEN`,
//!     Windows' canonical "user is running a fullscreen DX/Vulkan/OpenGL app"
//!     query. Catches most fullscreen games, including DXGI flip-discard
//!     borderless-fullscreen titles. Does not catch F11 browsers, video
//!     players, or PowerPoint slideshows.
//!  2. A user-supplied bypass list of process basenames (config.toml). The
//!     foreground window's owning process is resolved per stroke (cached on
//!     HWND) and matched case-insensitively.

use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, OpenInputDesktop,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::PWSTR;

/// How long a `should_skip_remap` decision is cached before re-running the
/// underlying syscalls. The hook callback fires on every mouse event (up to
/// the polling rate, ~1000 Hz), and the three syscalls inside the slow path
/// (OpenInputDesktop, GetForegroundWindow, SHQueryUserNotificationState) cost
/// far more than the remap math itself. 50 ms means at most ~20 slow paths
/// per second instead of ~1000; the foreground app and UAC state cannot
/// change meaningfully faster than that for our purposes.
const SKIP_CACHE_TTL: Duration = Duration::from_millis(50);

pub struct FocusGuard {
    last_hwnd: isize,
    last_basename: Option<String>,
    cached_skip: Option<(Instant, bool)>,
}

impl FocusGuard {
    pub fn new() -> Self {
        Self {
            last_hwnd: 0,
            last_basename: None,
            cached_skip: None,
        }
    }

    /// Returns `true` when the current stroke should be forwarded unchanged
    /// (i.e. the user is in a fullscreen game or a whitelisted process is in
    /// the foreground). The bypass list is read from the global
    /// `config::is_bypassed` lookup each call so the GUI's Bypass tab can
    /// swap it live without a RustCursor restart.
    ///
    /// Cached for `SKIP_CACHE_TTL` to keep the per-event cost low; see the
    /// constant doc for the trade-off.
    pub fn should_skip_remap(&mut self) -> bool {
        if let Some((when, decision)) = self.cached_skip
            && when.elapsed() < SKIP_CACHE_TTL
        {
            return decision;
        }
        let decision = self.compute_skip_remap();
        self.cached_skip = Some((Instant::now(), decision));
        decision
    }

    fn compute_skip_remap(&mut self) -> bool {
        // UAC's consent prompt runs on a separate "secure desktop" our process
        // can't reach. While it owns input, OpenInputDesktop fails for
        // non-winlogon callers. Use that as the signal to step fully aside,
        // otherwise our SetCursorPos and zeroed deltas leak into the secure
        // desktop's input and the consent prompt's cursor stops moving freely.
        unsafe {
            match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) {
                Ok(h) => {
                    let _ = CloseDesktop(h);
                }
                Err(_) => return true,
            }
        }

        self.refresh_foreground();

        if let Ok(state) = unsafe { SHQueryUserNotificationState() }
            && state == QUNS_RUNNING_D3D_FULL_SCREEN
        {
            return true;
        }

        if rust_cursor::config::bypass_is_empty() {
            return false;
        }
        match &self.last_basename {
            Some(name) => rust_cursor::config::is_bypassed(name),
            None => false,
        }
    }

    /// Cached basename of the foreground window's process. Populated by
    /// `should_skip_remap`; used by the event loop for diagnostic logging
    /// so a misbehaving app can be added to `bypass_processes` by name.
    /// Only consumed when the `log` feature is enabled.
    #[cfg(feature = "log")]
    pub fn current_basename(&self) -> Option<&str> {
        self.last_basename.as_deref()
    }

    fn refresh_foreground(&mut self) {
        let hwnd = unsafe { GetForegroundWindow() };
        let raw = hwnd.0 as isize;
        if raw == 0 || raw == self.last_hwnd {
            return;
        }
        self.last_hwnd = raw;
        self.last_basename = process_basename_for_hwnd(hwnd);
    }
}

fn process_basename_for_hwnd(hwnd: HWND) -> Option<String> {
    let mut pid: u32 = 0;
    let _tid = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };

    let mut buf = [0u16; MAX_PATH as usize];
    let mut size = buf.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }
    if result.is_err() {
        return None;
    }

    let path = String::from_utf16_lossy(&buf[..size as usize]);
    Path::new(&path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
}

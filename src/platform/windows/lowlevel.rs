//! Userspace mouse backend using `WH_MOUSE_LL`. Anti-cheat-compatible
//! alternative to the Interception kernel-driver backend, at the cost of a
//! 1-frame snap on monitor crossings.
//!
//! The Win32 hook callback must be a free `extern "system" fn` with no captured
//! state, so backend state lives in a process-wide `OnceLock<Mutex<State>>`.
//! All hook invocations happen on the thread that installed the hook (i.e. the
//! worker thread running `run_lowlevel_loop`), so the mutex is uncontested in
//! steady state; we keep it for safety regardless.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, HC_ACTION, HHOOK, LLMHF_INJECTED,
    MSG, MSLLHOOKSTRUCT, SetCursorPos, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_MOUSE_LL, WM_MOUSEMOVE,
};

use rust_cursor::core::Monitor;
use rust_cursor::remapper::{monitor_at_pixel, remap_transition};

use super::focus::FocusGuard;

struct State {
    monitors: Arc<RwLock<HashMap<String, Monitor>>>,
    focus: FocusGuard,
    log: std::fs::File,
    prev_pt: POINT,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

pub fn run_lowlevel_loop(monitors: Arc<RwLock<HashMap<String, Monitor>>>) {
    let log = open_log();
    let mut prev_pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut prev_pt);
    }

    let state = State {
        monitors,
        focus: FocusGuard::new(),
        log,
        prev_pt,
    };

    if STATE.set(Mutex::new(state)).is_err() {
        // Already initialized; only allowed once per process.
        return;
    }

    write_session_header();

    let hook = unsafe {
        SetWindowsHookExW(WH_MOUSE_LL, Some(ll_callback), None, 0)
            .expect("install WH_MOUSE_LL hook")
    };

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
}

unsafe extern "system" fn ll_callback(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    let pass_through =
        || unsafe { CallNextHookEx(Some(HHOOK::default()), n_code, w_param, l_param) };

    if n_code != HC_ACTION as i32 {
        return pass_through();
    }
    if w_param.0 as u32 != WM_MOUSEMOVE {
        return pass_through();
    }

    let info = unsafe { &*(l_param.0 as *const MSLLHOOKSTRUCT) };

    // Ignore injected events: our own `SetCursorPos` will produce one, and
    // we'd otherwise recurse on it. Also skips other input-injecting tools.
    if info.flags & LLMHF_INJECTED != 0 {
        return pass_through();
    }

    let new_pt = info.pt;
    let Some(mutex) = STATE.get() else {
        return pass_through();
    };
    let mut state = mutex.lock().unwrap();

    let old_pt = state.prev_pt;
    state.prev_pt = new_pt;

    if state.focus.should_skip_remap() {
        return pass_through();
    }

    let monitors_arc = state.monitors.clone();
    let map = monitors_arc.read().unwrap();
    let remap_result = remap_transition(old_pt.x, old_pt.y, new_pt.x, new_pt.y, &map);
    let Some((cx, cy)) = remap_result else {
        drop(map);
        return pass_through();
    };

    let tag = {
        let src = monitor_at_pixel(old_pt.x, old_pt.y, &map).map(|m| &m.identifier);
        let dst = monitor_at_pixel(cx, cy, &map).map(|m| &m.identifier);
        if src != dst { "REMAP" } else { "BLOCK" }
    };
    drop(map);
    let process = state.focus.current_basename().unwrap_or("?").to_owned();

    let _ = writeln!(
        state.log,
        "{}  ({},{}) → ({},{})  [raw ({},{})]  [{}]",
        tag, old_pt.x, old_pt.y, cx, cy, new_pt.x, new_pt.y, process
    );

    // Remember the corrected position so the next stroke's old_pt reflects
    // where we actually put the cursor, not the OS's pre-correction reading.
    state.prev_pt = POINT { x: cx, y: cy };
    drop(state);

    unsafe {
        let _ = SetCursorPos(cx, cy);
    }
    LRESULT(1) // suppress original event
}

fn write_session_header() {
    let Some(mutex) = STATE.get() else { return };
    let mut s = mutex.lock().unwrap();
    let _ = writeln!(s.log, "=== session start (lowlevel backend) ===");
    let _ = writeln!(s.log, "--- monitor layout ---");
    let lines: Vec<String> = {
        let map = s.monitors.read().unwrap();
        map.values()
            .map(|m| {
                format!(
                    "  {} : x=[{}, {}]  y=[{}, {}]  ({}x{})  {:.0}dpi",
                    m.identifier,
                    m.bounds.x as i32,
                    (m.bounds.x + m.bounds.w) as i32,
                    m.bounds.y as i32,
                    (m.bounds.y + m.bounds.h) as i32,
                    m.resolution.0,
                    m.resolution.1,
                    m.dpi,
                )
            })
            .collect()
    };
    for line in lines {
        let _ = writeln!(s.log, "{}", line);
    }
    let _ = writeln!(s.log, "----------------------");
    let (sx, sy) = (s.prev_pt.x, s.prev_pt.y);
    let _ = writeln!(s.log, "start ({}, {})", sx, sy);
}

fn open_log() -> std::fs::File {
    let log_dir = std::env::var_os("LOCALAPPDATA")
        .map(|s| PathBuf::from(s).join("RustCursor"))
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&log_dir);
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_dir.join("cursor_log.txt"))
        .expect("Could not open cursor_log.txt")
}

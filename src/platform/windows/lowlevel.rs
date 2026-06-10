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
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, HC_ACTION, HHOOK, LLMHF_INJECTED,
    MSG, MSLLHOOKSTRUCT, SetCursorPos, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_MOUSE_LL, WM_MOUSEMOVE,
};

use rust_cursor::core::Monitor;
#[cfg(feature = "log")]
use rust_cursor::remapper::monitor_at_pixel;
use rust_cursor::remapper::remap_transition;

use super::focus::FocusGuard;

struct State {
    monitors: Arc<RwLock<HashMap<String, Monitor>>>,
    focus: FocusGuard,
    /// Channel into the writer thread that owns `cursor_log.txt`. The hook
    /// callback must never do file I/O itself: synchronous writes count
    /// against `LowLevelHooksTimeout` (~300 ms) and a slow flush (AV scan,
    /// disk stall) would get the hook silently evicted.
    #[cfg(feature = "log")]
    log_tx: std::sync::mpsc::Sender<String>,
    prev_pt: POINT,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

pub fn run_lowlevel_loop(monitors: Arc<RwLock<HashMap<String, Monitor>>>) {
    let mut prev_pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut prev_pt);
    }

    let state = State {
        monitors,
        focus: FocusGuard::new(),
        #[cfg(feature = "log")]
        log_tx: spawn_log_writer(),
        prev_pt,
    };

    if STATE.set(Mutex::new(state)).is_err() {
        // Already initialized; only allowed once per process.
        return;
    }

    #[cfg(feature = "log")]
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

    #[cfg(feature = "log")]
    {
        // BLOCK lines (cursor pinned within its source monitor) are noisy edge-
        // glide spam at the input polling rate; only log genuine REMAPs.
        let is_remap = {
            let src = monitor_at_pixel(old_pt.x, old_pt.y, &map).map(|m| &m.identifier);
            let dst = monitor_at_pixel(cx, cy, &map).map(|m| &m.identifier);
            src != dst
        };
        if is_remap {
            // Split-borrow through the guard so sending on `log_tx` and
            // reading `focus.current_basename()` (which borrows `focus`)
            // don't alias. The send never blocks (unbounded channel); the
            // writer thread does the actual file I/O.
            let s = &mut *state;
            let process = s.focus.current_basename().unwrap_or("?");
            let _ = s.log_tx.send(format!(
                "REMAP  ({},{}) → ({},{})  [raw ({},{})]  [{}]",
                old_pt.x, old_pt.y, cx, cy, new_pt.x, new_pt.y, process
            ));
        }
    }
    drop(map);

    // Remember the corrected position so the next stroke's old_pt reflects
    // where we actually put the cursor, not the OS's pre-correction reading.
    state.prev_pt = POINT { x: cx, y: cy };
    drop(state);

    unsafe {
        let _ = SetCursorPos(cx, cy);
    }
    LRESULT(1) // suppress original event
}

#[cfg(feature = "log")]
fn write_session_header() {
    let Some(mutex) = STATE.get() else { return };
    let s = mutex.lock().unwrap();
    let send = |line: String| {
        let _ = s.log_tx.send(line);
    };
    send("=== session start (lowlevel backend) ===".into());
    for w in rust_cursor::config::load_warnings() {
        send(format!("config warning: {w}"));
    }
    send("--- monitor layout ---".into());
    {
        let map = s.monitors.read().unwrap();
        for m in map.values() {
            send(format!(
                "  {} : x=[{}, {}]  y=[{}, {}]  ({}x{})  {:.0}dpi",
                m.identifier,
                m.bounds.x as i32,
                (m.bounds.x + m.bounds.w) as i32,
                m.bounds.y as i32,
                (m.bounds.y + m.bounds.h) as i32,
                m.resolution.0,
                m.resolution.1,
                m.dpi,
            ));
        }
    }
    send("----------------------".into());
    send(format!("start ({}, {})", s.prev_pt.x, s.prev_pt.y));
}

/// Spawn the thread that owns `cursor_log.txt` and drain log lines to it.
/// Batches whatever has queued up before each flush, so the Settings GUI's
/// live tail still sees lines promptly without a syscall per line. If the
/// file can't be opened the thread exits and subsequent sends fail silently;
/// logging is best-effort and must never take down remapping.
#[cfg(feature = "log")]
fn spawn_log_writer() -> std::sync::mpsc::Sender<String> {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::PathBuf;

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let log_dir = std::env::var_os("LOCALAPPDATA")
            .map(|s| PathBuf::from(s).join("RustCursor"))
            .unwrap_or_else(|| PathBuf::from("."));
        let _ = std::fs::create_dir_all(&log_dir);
        let Ok(file) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(log_dir.join("cursor_log.txt"))
        else {
            return;
        };
        let mut log = std::io::BufWriter::new(file);
        while let Ok(line) = rx.recv() {
            let _ = writeln!(log, "{line}");
            while let Ok(more) = rx.try_recv() {
                let _ = writeln!(log, "{more}");
            }
            let _ = log.flush();
        }
    });
    tx
}

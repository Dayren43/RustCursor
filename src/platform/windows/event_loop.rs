use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use interception::{Filter, Interception, MouseFlags, MouseState, Stroke, is_mouse};

use windows::Win32::{
    Foundation::POINT,
    UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos},
};

use rust_cursor::core::Monitor;
use rust_cursor::remapper::{monitor_at_pixel, remap_transition};

use super::focus::FocusGuard;

pub fn run_event_loop(
    monitors: Arc<RwLock<HashMap<String, Monitor>>>,
    bypass_processes: Vec<String>,
) {
    let mut focus = FocusGuard::new(bypass_processes);
    let ic = Interception::new()
        .expect("Failed to create Interception context — is the driver installed?");

    ic.set_filter(is_mouse, Filter::MouseFilter(MouseState::MOVE));

    // Log under %LOCALAPPDATA%\RustCursor\ so the path is stable regardless of CWD —
    // Task Scheduler launches with CWD=System32 by default, where a relative path lands.
    let log_dir = std::env::var_os("LOCALAPPDATA")
        .map(|s| PathBuf::from(s).join("RustCursor"))
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("cursor_log.txt");

    let mut log = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .expect("Could not open cursor_log.txt");

    writeln!(log, "=== session start ===").ok();
    writeln!(log, "--- monitor layout ---").ok();
    {
        let map = monitors.read().unwrap();
        for m in map.values() {
            writeln!(
                log,
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
            .ok();
        }
    }
    writeln!(log, "----------------------").ok();

    {
        let (sx, sy) = unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            (pt.x, pt.y)
        };
        writeln!(log, "start ({}, {})", sx, sy).ok();
    }

    loop {
        let device = ic.wait();

        let mut stroke = Stroke::Mouse {
            state: MouseState::empty(),
            flags: MouseFlags::empty(),
            rolling: 0,
            x: 0,
            y: 0,
            information: 0,
        };
        let received = ic.receive(device, std::slice::from_mut(&mut stroke));
        if received <= 0 {
            continue;
        }

        // Read actual cursor position before win32k processes this event to avoid
        // drift from pointer-speed/acceleration scaling raw hardware deltas.
        let (old_x, old_y) = unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            (pt.x, pt.y)
        };

        if let Stroke::Mouse {
            ref flags,
            ref mut x,
            ref mut y,
            ..
        } = stroke
            && !flags.contains(MouseFlags::MOVE_ABSOLUTE)
            && !focus.should_skip_remap()
        {
            let new_x = old_x + *x;
            let new_y = old_y + *y;

            let map = monitors.read().unwrap();
            if let Some((cx, cy)) = remap_transition(old_x, old_y, new_x, new_y, &map) {
                let tag = {
                    let src = monitor_at_pixel(old_x, old_y, &map).map(|m| &m.identifier);
                    let dst = monitor_at_pixel(cx, cy, &map).map(|m| &m.identifier);
                    if src != dst { "REMAP" } else { "BLOCK" }
                };
                let process = focus.current_basename().unwrap_or("?").to_owned();
                drop(map);
                writeln!(
                    log,
                    "{}  ({},{}) → ({},{})  [raw ({},{})]  [{}]",
                    tag, old_x, old_y, cx, cy, new_x, new_y, process
                )
                .ok();

                *x = 0;
                *y = 0;
                unsafe {
                    let _ = SetCursorPos(cx, cy);
                }
            }
        }

        ic.send(device, std::slice::from_ref(&stroke));
    }
}

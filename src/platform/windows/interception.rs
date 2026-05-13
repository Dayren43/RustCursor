use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use interception::{Filter, Interception, MouseFlags, MouseState, Stroke, is_mouse};

use windows::Win32::{
    Foundation::POINT,
    UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos},
};

use rust_cursor::core::Monitor;
#[cfg(feature = "log")]
use rust_cursor::remapper::monitor_at_pixel;
use rust_cursor::remapper::remap_transition;

use super::focus::FocusGuard;

pub fn run_interception_loop(monitors: Arc<RwLock<HashMap<String, Monitor>>>) {
    let mut focus = FocusGuard::new();
    let ic = Interception::new()
        .expect("Failed to create Interception context. Is the driver installed?");

    ic.set_filter(is_mouse, Filter::MouseFilter(MouseState::MOVE));

    #[cfg(feature = "log")]
    let mut log = open_session_log(&monitors);

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
                #[cfg(feature = "log")]
                {
                    // BLOCK lines are edge-glide spam at the input polling rate;
                    // only log genuine cross-monitor REMAPs.
                    let is_remap = {
                        let src = monitor_at_pixel(old_x, old_y, &map).map(|m| &m.identifier);
                        let dst = monitor_at_pixel(cx, cy, &map).map(|m| &m.identifier);
                        src != dst
                    };
                    if is_remap {
                        use std::io::Write;
                        let process = focus.current_basename().unwrap_or("?");
                        let _ = writeln!(
                            log,
                            "REMAP  ({},{}) → ({},{})  [raw ({},{})]  [{}]",
                            old_x, old_y, cx, cy, new_x, new_y, process
                        );
                        let _ = log.flush();
                    }
                }
                drop(map);

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

#[cfg(feature = "log")]
fn open_session_log(
    monitors: &Arc<RwLock<HashMap<String, Monitor>>>,
) -> std::io::BufWriter<std::fs::File> {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::PathBuf;

    // Log under %LOCALAPPDATA%\RustCursor\ so the path is stable regardless of CWD.
    // Task Scheduler launches with CWD=System32 by default, where a relative path lands.
    let log_dir = std::env::var_os("LOCALAPPDATA")
        .map(|s| PathBuf::from(s).join("RustCursor"))
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("cursor_log.txt");

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .expect("Could not open cursor_log.txt");
    let mut log = std::io::BufWriter::new(file);

    let _ = writeln!(log, "=== session start ===");
    let _ = writeln!(log, "--- monitor layout ---");
    {
        let map = monitors.read().unwrap();
        for m in map.values() {
            let _ = writeln!(
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
            );
        }
    }
    let _ = writeln!(log, "----------------------");

    let (sx, sy) = unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        (pt.x, pt.y)
    };
    let _ = writeln!(log, "start ({}, {})", sx, sy);
    let _ = log.flush();
    log
}

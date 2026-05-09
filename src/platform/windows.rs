mod monitors;
mod event_loop;

pub use monitors::{build_monitor_map, setup_dpi_awareness};
pub use event_loop::run_event_loop;

/// Spawn a background thread that calls `std::process::exit` when ESC is held.
pub fn spawn_exit_on_escape() {
    use std::{thread, time::Duration};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
    thread::spawn(|| loop {
        if unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) as i32 & 0x8000 } != 0 {
            println!("ESC — exiting.");
            std::process::exit(0);
        }
        thread::sleep(Duration::from_millis(50));
    });
}

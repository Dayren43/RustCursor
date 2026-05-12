//! Settings GUI. Hidden by default; opened from the tray "Settings…" menu item.
//!
//! The window runs on its own thread with eframe owning a winit event loop.
//! Spawning is one-shot: when the window closes, the thread exits. Re-opening
//! from the tray spawns a fresh thread. A single-instance guard prevents two
//! windows from being open at once.

mod app;

use std::sync::atomic::{AtomicBool, Ordering};

static OPEN: AtomicBool = AtomicBool::new(false);

/// Open the settings window if not already open. Returns immediately; the
/// window runs on a background thread.
pub fn open_settings_window() {
    if OPEN.swap(true, Ordering::SeqCst) {
        // Already open. A future iteration can focus the existing window;
        // for now the click is a no-op so we don't stack instances.
        return;
    }
    std::thread::spawn(|| {
        app::run();
        OPEN.store(false, Ordering::SeqCst);
    });
}

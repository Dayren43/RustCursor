//! Cursor monitor-transition remapper.
//!
//! When the cursor crosses between monitors, its position is carried through a
//! shared physical-millimetre space, so it keeps its physical height (or
//! physical x, for vertical crossings) whatever each monitor's resolution,
//! diagonal or mounting offset. Crossings into places where the destination
//! has no panel are blocked, and the cursor slides along the source's edge.
//!
//! Architecture:
//!  - `platform::windows` owns the Windows runtime: monitor enumeration, DPI setup,
//!    the input backends (the `WH_MOUSE_LL` hook by default, Interception behind
//!    the `interception-backend` feature), and the system-tray UI. A future
//!    `platform::linux` would implement the same surface.
//!  - `gui` is the Settings window, run as a `--settings` subprocess.
//!  - `rust_cursor::remapper` contains the platform-agnostic crossing logic.
//!  - `rust_cursor::core` contains the Monitor struct and physical↔pixel mapping math.
//!  - `rust_cursor::config` loads `config.toml` and holds the live lookups.
//!
//! See the README for build features and the Interception driver setup.

#![windows_subsystem = "windows"]

mod gui;
mod platform;

use std::sync::{Arc, RwLock};

fn main() {
    // Settings subprocess path: `RustCursor.exe --settings`. Launched by the
    // tray to run eframe in isolation. No input hook, no tray, no display
    // listener: the parent owns those. On window close, this process exits
    // and all its memory + GL driver threads are reclaimed.
    if std::env::args().any(|a| a == "--settings") {
        gui::run_settings_subprocess();
        return;
    }

    platform::windows::setup_dpi_awareness();

    let config = rust_cursor::config::Config::load();

    // Resolve the active profile from currently-connected HWIDs. If a
    // matching `[[profile]]` exists, its per-HWID size entries take precedence;
    // otherwise the lookup falls back to `default_size_in`.
    // This must run before any `build_monitor_map` call because that call
    // consults `size_for`.
    platform::windows::install_matching_profile(&config);

    let monitors = Arc::new(RwLock::new(platform::windows::build_monitor_map()));

    // Hot-reload monitor layout on display changes (plug/unplug, rearrange).
    // Must be registered on the main thread before run_tray_loop's message pump.
    platform::windows::register_display_listener(monitors.clone());

    rust_cursor::config::install_bypass_processes(config.bypass_processes);
    let backend = config.backend;
    let backend_monitors = monitors.clone();
    std::thread::spawn(move || match backend {
        #[cfg(feature = "interception-backend")]
        rust_cursor::config::Backend::Interception => {
            platform::windows::run_interception_loop(backend_monitors);
        }
        rust_cursor::config::Backend::Lowlevel => {
            platform::windows::run_lowlevel_loop(backend_monitors);
        }
    });

    platform::windows::run_tray_loop(backend);

    // Tray Quit ends the pump above; close any Settings subprocess windows
    // on the way out so quitting the app doesn't leave them orphaned.
    gui::close_settings_windows();
}

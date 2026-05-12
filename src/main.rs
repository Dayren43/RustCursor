//! Cursor monitor-transition remapper using the Interception kernel driver.
//!
//! When the cursor crosses between monitors of different resolutions, the Y position
//! is remapped to the equivalent physical height on the destination monitor. Both
//! monitors are assumed to be the same physical diagonal size but may differ in
//! resolution.
//!
//! Architecture:
//!  - `platform::windows` owns the Windows runtime: monitor enumeration, DPI setup,
//!    the Interception event loop, and the system-tray UI. A future
//!    `platform::linux` would implement the same surface.
//!  - `RustCursor::remapper` contains the platform-agnostic crossing logic.
//!  - `RustCursor::core` contains the Monitor struct and physical↔pixel mapping math.
//!
//! Prerequisites:
//!   Install the Interception driver (run as administrator, then reboot):
//!     install-interception.exe /install
//!   Download: https://github.com/oblitum/Interception/releases

#![windows_subsystem = "windows"]

mod gui;
mod platform;

use std::sync::{Arc, RwLock};

fn main() {
    platform::windows::setup_dpi_awareness();

    let config = rust_cursor::config::Config::load();

    // Resolve the active profile from currently-connected HWIDs. If a
    // matching `[[profile]]` exists, its per-HWID size entries take precedence;
    // otherwise the lookup falls back to legacy `[[monitor]]` entries or the
    // global default. `install_active_sizes` must run before any
    // `build_monitor_map` call because that call consults `size_for`.
    let connected_hwids = platform::windows::enumerate_hwids();
    let profile_monitors = config
        .active_profile(&connected_hwids)
        .map(|p| p.monitors.clone())
        .unwrap_or_default();
    rust_cursor::config::install_active_sizes(
        profile_monitors,
        config.legacy_monitors.clone(),
        config.default_size_in,
    );

    let monitors = Arc::new(RwLock::new(platform::windows::build_monitor_map()));

    // Hot-reload monitor layout on display changes (plug/unplug, rearrange).
    // Must be registered on the main thread before run_tray_loop's message pump.
    platform::windows::register_display_listener(monitors.clone());

    let bypass = config.bypass_processes;
    let backend = config.backend;
    let backend_monitors = monitors.clone();
    std::thread::spawn(move || match backend {
        rust_cursor::config::Backend::Interception => {
            platform::windows::run_interception_loop(backend_monitors, bypass);
        }
        rust_cursor::config::Backend::Lowlevel => {
            platform::windows::run_lowlevel_loop(backend_monitors, bypass);
        }
    });

    platform::windows::run_tray_loop(backend);
}

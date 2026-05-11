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
//!     install-interception.exe /install  — https://github.com/oblitum/Interception/releases

#![windows_subsystem = "windows"]

mod platform;

fn main() {
    platform::windows::setup_dpi_awareness();
    let monitors = platform::windows::build_monitor_map();
    let config = rust_cursor::config::Config::load();

    let bypass = config.bypass_processes;
    let backend = config.backend;
    std::thread::spawn(move || match backend {
        rust_cursor::config::Backend::Interception => {
            platform::windows::run_event_loop(monitors, bypass);
        }
        rust_cursor::config::Backend::Lowlevel => {
            platform::windows::run_lowlevel_loop(monitors, bypass);
        }
    });

    platform::windows::run_tray_loop(backend);
}

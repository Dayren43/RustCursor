//! Cursor monitor-transition remapper using the Interception kernel driver.
//!
//! When the cursor crosses between monitors of different resolutions, the Y position is
//! remapped to the equivalent physical height on the destination monitor. Both monitors
//! are assumed to be the same physical diagonal size but may differ in resolution.
//!
//! Architecture:
//!  - `platform::windows` handles monitor enumeration, DPI setup, and the Interception
//!    event loop. A future `platform::linux` would implement the same surface.
//!  - `RustCursor::remapper` contains the platform-agnostic crossing logic.
//!  - `RustCursor::core` contains the Monitor struct and physical↔pixel mapping math.
//!
//! Prerequisites:
//!   Install the Interception driver (run as administrator, then reboot):
//!     install-interception.exe /install  — https://github.com/oblitum/Interception/releases

mod platform;

fn main() {
    platform::windows::setup_dpi_awareness();

    let monitors = platform::windows::build_monitor_map();
    println!("Monitors:");
    for m in monitors.values() {
        println!(
            "  {} : ({}, {})–({}, {})  {}  {:.0} dpi  {}\" diagonal",
            m.identifier,
            m.bounds.x as i32,
            m.bounds.y as i32,
            (m.bounds.x + m.bounds.w) as i32,
            (m.bounds.y + m.bounds.h) as i32,
            m.pretty_resolution(),
            m.dpi,
            m.physical_size_in,
        );
    }

    platform::windows::spawn_exit_on_escape();

    println!("Running. Press ESC to exit.");
    platform::windows::run_event_loop(monitors);
}

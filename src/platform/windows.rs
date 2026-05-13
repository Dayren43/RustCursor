//! Windows-specific platform surface. Each submodule owns one concern of the
//! Windows runtime; `main.rs` calls only the items re-exported here.

mod focus;
mod interception;
mod layout;
mod lowlevel;
mod monitors;
mod tray;

pub use interception::run_interception_loop;
pub use layout::register_display_listener;
pub use lowlevel::run_lowlevel_loop;
pub use monitors::{build_monitor_map, enumerate_hwids, setup_dpi_awareness};
pub use tray::run_tray_loop;

//! Windows-specific platform surface. Each submodule owns one concern of the
//! Windows runtime; `main.rs` calls only the items re-exported here.

mod event_loop;
mod focus;
mod lowlevel;
mod monitors;
mod tray;

pub use event_loop::run_event_loop;
pub use lowlevel::run_lowlevel_loop;
pub use monitors::{build_monitor_map, setup_dpi_awareness};
pub use tray::run_tray_loop;

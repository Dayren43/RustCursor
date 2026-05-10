//! Windows-specific platform surface. Each submodule owns one concern of the
//! Windows runtime; `main.rs` calls only the items re-exported here.

mod monitors;
mod event_loop;
mod tray;

pub use monitors::{build_monitor_map, setup_dpi_awareness};
pub use event_loop::run_event_loop;
pub use tray::run_tray_loop;

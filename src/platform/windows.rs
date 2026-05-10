mod monitors;
mod event_loop;

pub use monitors::{build_monitor_map, setup_dpi_awareness};
pub use event_loop::run_event_loop;

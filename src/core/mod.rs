// Monitor and cursor mapping utilities.
// Fixed mapping bugs and made mapping functions robust to edge cases:
// - Prevent division by zero when monitor bounds width/height are zero.
// - Allow points outside monitor bounds (relative coords can be <0 or >1).
// - Use a stable pretty_monitor_name implementation that strips the common "\\.\\"
//   prefix correctly.
// - Added helper clamp option (currently not clamping, but left as note).
// - Use consistent numeric types and avoid accidental integer math.

pub mod geometry;

use crate::core::geometry::Rect;

#[derive(Debug, Clone)]
pub struct Monitor {
    pub identifier: String,
    pub bounds: Rect,
    pub aspect_ratio: f32,
    pub resolution: (u32, u32),
    pub dpi: f64,
    pub physical_size_in: f32,
}

impl Monitor {
    /// Compute width and height in millimeters from diagonal size and aspect ratio.
    /// aspect_ratio is width/height.
    pub fn physical_size_mm(&self) -> (f32, f32) {
        let diag_mm = self.physical_size_in * 25.4_f32;
        let aspect = self.aspect_ratio;
        // guard against degenerate aspect ratio
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 16.0 / 9.0 };
        let height_mm = diag_mm / ((aspect * aspect + 1.0).sqrt());
        let width_mm = height_mm * aspect;
        (width_mm, height_mm)
    }

    pub fn default() -> Self {
        Self {
            identifier: "Default".into(),
            bounds: Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 },
            aspect_ratio: 16.0 / 9.0,
            resolution: (1920, 1080),
            dpi: 96.0,
            physical_size_in: 27.0,
        }
    }

    pub fn pretty_resolution(&self) -> String {
        format!("{}x{}", self.resolution.0, self.resolution.1)
    }

    /// Returns a nicer monitor name by stripping the common Windows device prefix
    /// "\\.\\" (which in a Rust string literal is "\\\\\\\\.\\\\").
    /// If the identifier doesn't start with that prefix the original identifier is returned.
    pub fn pretty_monitor_name(&self) -> String {
        // Windows device names often start with "\\.\DISPLAYx" or similar.
        let prefix = r"\\.\";
        if let Some(stripped) = self.identifier.strip_prefix(prefix) {
            stripped.to_string()
        } else {
            self.identifier.clone()
        }
    }
}

impl PartialEq for Monitor {
    fn eq(&self, other: &Self) -> bool {
        self.identifier == other.identifier
    }
}

pub mod cursor_mapper {
    use crate::core::geometry::Point;
    use crate::core::Monitor;

    /// Map a cursor point in global (OS) coordinates to physical space (mm) using a given monitor.
    /// Returns physical coordinates in millimeters relative to the top-left corner of the monitor.
    /// If monitor bounds width/height are zero, returns (0,0).
    pub fn to_physical(point: Point, source: &Monitor) -> Point {
        // Avoid division by zero
        let w = if source.bounds.w.abs() < f32::EPSILON {
            0.0_f32
        } else {
            source.bounds.w
        };
        let h = if source.bounds.h.abs() < f32::EPSILON {
            0.0_f32
        } else {
            source.bounds.h
        };

        if w == 0.0 || h == 0.0 {
            return Point { x: 0.0, y: 0.0 };
        }

        // relative coordinates (can be outside 0.0..1.0 if point sits outside monitor)
        let x_rel = (point.x - source.bounds.x) / w;
        let y_rel = (point.y - source.bounds.y) / h;

        let (width_mm, height_mm) = source.physical_size_mm();

        Point {
            x: x_rel * width_mm,
            y: y_rel * height_mm,
        }
    }

    /// Map a physical-space point (mm) back to OS coordinates on a target monitor.
    /// physical is in millimeters relative to the top-left corner of the monitor.
    /// If target physical width/height are zero, returns the monitor top-left.
    pub fn to_os_pos(physical: Point, target: &Monitor) -> Point {
        let (width_mm, height_mm) = target.physical_size_mm();

        if width_mm.abs() < f32::EPSILON || height_mm.abs() < f32::EPSILON {
            return Point {
                x: target.bounds.x,
                y: target.bounds.y,
            };
        }

        let x_rel = physical.x / width_mm;
        let y_rel = physical.y / height_mm;

        Point {
            x: target.bounds.x + x_rel * target.bounds.w,
            y: target.bounds.y + y_rel * target.bounds.h,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cursor_mapper::*;
    use crate::core::{Monitor, geometry::Point, geometry::Rect};

    fn make_test_monitor(x: f32, y: f32, w: f32, h: f32, physical_in: f32) -> Monitor {
        Monitor {
            identifier: format!("Test {}x{}", w as u32, h as u32),
            bounds: Rect { x, y, w, h },
            aspect_ratio: if h.abs() < f32::EPSILON { 16.0 / 9.0 } else { w / h },
            resolution: (w as u32, h as u32),
            dpi: (((w as f64).powi(2) + (h as f64).powi(2)).sqrt()) as f64 / physical_in as f64,
            physical_size_in: physical_in,
        }
    }

    #[test]
    fn test_to_physical_center() {
        let monitor = make_test_monitor(0.0, 0.0, 1920.0, 1080.0, 27.0);
        let point = Point { x: 960.0, y: 540.0 };
        let physical = to_physical(point, &monitor);
        let (width_mm, height_mm) = monitor.physical_size_mm();

        assert!((physical.x - width_mm / 2.0).abs() < 1e-3);
        assert!((physical.y - height_mm / 2.0).abs() < 1e-3);
    }

    #[test]
    fn test_to_screen_center() {
        let monitor = make_test_monitor(0.0, 0.0, 1920.0, 1080.0, 27.0);
        let physical_center = {
            let (w_mm, h_mm) = monitor.physical_size_mm();
            Point { x: w_mm / 2.0, y: h_mm / 2.0 }
        };
        let screen_point = to_os_pos(physical_center, &monitor);

        assert!((screen_point.x - 960.0).abs() < 1e-3);
        assert!((screen_point.y - 540.0).abs() < 1e-3);
    }

    #[test]
    fn test_round_trip() {
        let monitor = make_test_monitor(100.0, 50.0, 1920.0, 1080.0, 27.0);
        let point = Point { x: 1000.0, y: 600.0 };
        let physical = to_physical(point.clone(), &monitor);
        let screen_point = to_os_pos(physical, &monitor);

        assert!((screen_point.x - point.x).abs() < 1e-3);
        assert!((screen_point.y - point.y).abs() < 1e-3);
    }

    #[test]
    fn test_negative_offset_monitor() {
        let monitor = make_test_monitor(-1920.0, 0.0, 1920.0, 1080.0, 27.0);
        let point = Point { x: -960.0, y: 540.0 }; // center of this monitor
        let physical = to_physical(point.clone(), &monitor);
        let screen_point = to_os_pos(physical, &monitor);

        assert!((screen_point.x - point.x).abs() < 1e-3);
        assert!((screen_point.y - point.y).abs() < 1e-3);
    }

    #[test]
    fn test_cross_monitor_mapping() {
        let source = make_test_monitor(0.0, 0.0, 1920.0, 1080.0, 27.0);
        let target = make_test_monitor(1920.0, 0.0, 2560.0, 1440.0, 27.0);

        let point = Point { x: 960.0, y: 540.0 }; // center of source
        let physical = to_physical(point, &source);
        let mapped = to_os_pos(physical, &target);

        assert!((mapped.x - (target.bounds.x + target.bounds.w / 2.0)).abs() < 1e-3);
        assert!((mapped.y - (target.bounds.y + target.bounds.h / 2.0)).abs() < 1e-3);
    }
}
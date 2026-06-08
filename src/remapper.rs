use std::collections::HashMap;

use crate::core::{Monitor, cursor_mapper, geometry::Point};

/// Returns the monitor whose pixel rect contains (x, y), or None.
pub fn monitor_at_pixel(x: i32, y: i32, monitors: &HashMap<String, Monitor>) -> Option<&Monitor> {
    monitors.values().find(|m| {
        x >= m.bounds.x as i32
            && x < (m.bounds.x + m.bounds.w) as i32
            && y >= m.bounds.y as i32
            && y < (m.bounds.y + m.bounds.h) as i32
    })
}

/// Find a monitor whose X-span includes `x`, regardless of Y.
/// Used for gap-zone crossings where the raw destination y is outside the
/// target monitor's OS y-range due to different per-monitor y-offsets.
pub fn monitor_for_x(x: i32, monitors: &HashMap<String, Monitor>) -> Option<&Monitor> {
    monitors
        .values()
        .find(|m| x >= m.bounds.x as i32 && x < (m.bounds.x + m.bounds.w) as i32)
}

/// Pin a cursor position to source monitor bounds in OS pixel space. Used to
/// "block" a crossing when the user-defined physical layout doesn't actually
/// share a continuous edge at the cursor's height (or width, for vertical
/// crossings): the cursor slides along the source's edge and only crosses
/// where the destination physically exists.
fn pin_to_source(old_mon: &Monitor, new_x: i32, new_y: i32) -> Option<(i32, i32)> {
    let pinned_x = new_x.clamp(
        old_mon.bounds.x as i32,
        (old_mon.bounds.x + old_mon.bounds.w - 1.0) as i32,
    );
    let pinned_y = new_y.clamp(
        old_mon.bounds.y as i32,
        (old_mon.bounds.y + old_mon.bounds.h - 1.0) as i32,
    );
    if pinned_x == new_x && pinned_y == new_y {
        None
    } else {
        Some((pinned_x, pinned_y))
    }
}

/// Compute the corrected cursor position when crossing a monitor boundary.
///
/// Returns `Some((x, y))` when a correction is needed, `None` when the raw
/// position is already correct (same monitor, or negligible delta).
pub fn remap_transition(
    old_x: i32,
    old_y: i32,
    new_x: i32,
    new_y: i32,
    monitors: &HashMap<String, Monitor>,
) -> Option<(i32, i32)> {
    let old_mon = monitor_at_pixel(old_x, old_y, monitors)?;
    let new_mon = monitor_at_pixel(new_x, new_y, monitors);

    match new_mon {
        // ── Gap crossing: no monitor at raw destination ──────────────────
        None => {
            // If new_x falls in a *different* monitor's x-span, this is a gap-zone
            // crossing (the monitors have different OS y-offsets). Use physical-space
            // y to find the correct landing row instead of clamping the raw pixel.
            if let Some(dest_mon) =
                monitor_for_x(new_x, monitors).filter(|m| m.identifier != old_mon.identifier)
            {
                let old_local_y = cursor_mapper::to_physical(
                    Point {
                        x: old_x as f32,
                        y: old_y as f32,
                    },
                    old_mon,
                )
                .y;
                // Map source-local mm -> shared world mm -> destination-local mm
                // so the cursor lands at the same world height regardless of the
                // two monitors' physical y-offsets.
                let world_y = old_mon.position_mm.y + old_local_y;
                let (_, dest_h_mm) = dest_mon.physical_size_mm();
                let dest_world_top = dest_mon.position_mm.y;
                let dest_world_bottom = dest_world_top + dest_h_mm;
                if world_y < dest_world_top || world_y > dest_world_bottom {
                    // Destination doesn't physically exist at this world height
                    // (user-defined layout has the source extending above or
                    // below the destination). Block the crossing.
                    return pin_to_source(old_mon, new_x, new_y);
                }
                let target_local_y = world_y - dest_mon.position_mm.y;
                let target_os_y = cursor_mapper::to_os_pos(
                    Point {
                        x: 0.0,
                        y: target_local_y,
                    },
                    dest_mon,
                )
                .y;
                let tx = new_x.clamp(
                    dest_mon.bounds.x as i32,
                    (dest_mon.bounds.x + dest_mon.bounds.w - 1.0) as i32,
                );
                let ty = (target_os_y as i32).clamp(
                    dest_mon.bounds.y as i32,
                    (dest_mon.bounds.y + dest_mon.bounds.h - 1.0) as i32,
                );
                return Some((tx, ty));
            }

            // Same monitor x-span or no monitor at all: block inside source bounds.
            pin_to_source(old_mon, new_x, new_y)
        }

        // ── Normal crossing: both monitors known ─────────────────────────
        Some(new_mon) => {
            if old_mon.identifier == new_mon.identifier {
                return None;
            }

            // Determine crossing direction via monitor centres.
            let old_cx = old_mon.bounds.x + old_mon.bounds.w / 2.0;
            let old_cy = old_mon.bounds.y + old_mon.bounds.h / 2.0;
            let new_cx = new_mon.bounds.x + new_mon.bounds.w / 2.0;
            let new_cy = new_mon.bounds.y + new_mon.bounds.h / 2.0;

            let horizontal_crossing = (new_cx - old_cx).abs() >= (new_cy - old_cy).abs();

            let old_local = cursor_mapper::to_physical(
                Point {
                    x: old_x as f32,
                    y: old_y as f32,
                },
                old_mon,
            );
            let new_local = cursor_mapper::to_physical(
                Point {
                    x: new_x as f32,
                    y: new_y as f32,
                },
                new_mon,
            );

            // Translate both endpoints into shared world mm so the layout's
            // per-monitor `position_mm` offsets factor into the crossing.
            let old_world = Point {
                x: old_mon.position_mm.x + old_local.x,
                y: old_mon.position_mm.y + old_local.y,
            };
            let new_world = Point {
                x: new_mon.position_mm.x + new_local.x,
                y: new_mon.position_mm.y + new_local.y,
            };

            // Preserve world height for horizontal crossings, world x for vertical.
            let target_world = if horizontal_crossing {
                Point {
                    x: new_world.x,
                    y: old_world.y,
                }
            } else {
                Point {
                    x: old_world.x,
                    y: new_world.y,
                }
            };

            // If the preserved world axis falls outside the destination's
            // physical extent, the user's layout has no shared edge here.
            // Block the crossing so the cursor slides along the source's edge.
            let (dest_w_mm, dest_h_mm) = new_mon.physical_size_mm();
            let dest_world_left = new_mon.position_mm.x;
            let dest_world_right = dest_world_left + dest_w_mm;
            let dest_world_top = new_mon.position_mm.y;
            let dest_world_bottom = dest_world_top + dest_h_mm;
            let preserved_in_bounds = if horizontal_crossing {
                target_world.y >= dest_world_top && target_world.y <= dest_world_bottom
            } else {
                target_world.x >= dest_world_left && target_world.x <= dest_world_right
            };
            if !preserved_in_bounds {
                return pin_to_source(old_mon, new_x, new_y);
            }

            let target_local = Point {
                x: target_world.x - new_mon.position_mm.x,
                y: target_world.y - new_mon.position_mm.y,
            };
            let target_os = cursor_mapper::to_os_pos(target_local, new_mon);
            let tx = (target_os.x as i32).clamp(
                new_mon.bounds.x as i32,
                (new_mon.bounds.x + new_mon.bounds.w - 1.0) as i32,
            );
            let ty = (target_os.y as i32).clamp(
                new_mon.bounds.y as i32,
                (new_mon.bounds.y + new_mon.bounds.h - 1.0) as i32,
            );

            if (tx - new_x).abs() < 1 && (ty - new_y).abs() < 1 {
                return None;
            }

            Some((tx, ty))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geometry::Rect;
    use std::collections::HashMap;

    fn make_monitor(id: &str, x: f32, y: f32, w: f32, h: f32, dpi: f64) -> Monitor {
        Monitor {
            identifier: id.to_string(),
            hwid: None,
            bounds: Rect { x, y, w, h },
            position_mm: Point { x: 0.0, y: 0.0 },
            aspect_ratio: w / h,
            resolution: (w as u32, h as u32),
            dpi,
            physical_size_in: (((w as f64).powi(2) + (h as f64).powi(2)).sqrt() / dpi) as f32,
        }
    }

    /// Crossing from the source's vertical centre should land at the
    /// destination's vertical centre when both panels are the same physical
    /// size, even though the destination has more vertical pixels. A raw pixel
    /// copy would keep y=540 (above B's centre); physical remapping yields B's
    /// centre at ~720.
    #[test]
    fn horizontal_crossing_preserves_physical_y() {
        let mut monitors = HashMap::new();
        monitors.insert(
            "A".into(),
            make_monitor("A", 0.0, 0.0, 1920.0, 1080.0, 81.59),
        );
        monitors.insert(
            "B".into(),
            make_monitor("B", 1920.0, 0.0, 2560.0, 1440.0, 108.84),
        );

        let (_, cy) = remap_transition(1919, 540, 1920, 540, &monitors)
            .expect("expected a correction crossing into a taller-pixel monitor");
        assert!(
            (cy - 720).abs() <= 1,
            "expected landing at destination vertical centre (~720), got y={cy}"
        );
    }

    #[test]
    fn no_correction_on_same_monitor() {
        let mut monitors = HashMap::new();
        monitors.insert(
            "A".into(),
            make_monitor("A", 0.0, 0.0, 1920.0, 1080.0, 81.59),
        );

        let result = remap_transition(100, 100, 200, 200, &monitors);
        assert!(
            result.is_none(),
            "No correction expected within same monitor"
        );
    }

    /// With the destination monitor pinned 50 mm lower in shared world space
    /// (its top edge below the source's), a horizontal crossing from the
    /// source's vertical centre preserves world-y, which on the destination
    /// is 50 mm above its own centre, i.e. a smaller pixel y.
    #[test]
    fn position_mm_offset_shifts_landing() {
        let mut monitors = HashMap::new();
        let src = make_monitor("src", 0.0, 0.0, 1920.0, 1080.0, 81.59);
        let mut dst = make_monitor("dst", 1920.0, 0.0, 1920.0, 1080.0, 81.59);
        dst.position_mm = Point { x: 597.5, y: 50.0 };
        monitors.insert("src".into(), src);
        monitors.insert("dst".into(), dst);

        let (_, ty) = remap_transition(1919, 540, 1920, 540, &monitors).expect("crossing");
        assert!(
            ty < 540,
            "expected landing above destination centre, got y={ty}"
        );
    }

    /// Tall portrait monitor at world (0, 0) -> ~600 mm tall; short landscape
    /// to its right mounted 100 mm down in world coords so it doesn't cover
    /// the portrait's top region. Crossing right from the portrait's top
    /// should be blocked (cursor pinned to portrait's right edge) because the
    /// landscape doesn't physically exist at that world height.
    #[test]
    fn crossing_blocked_when_dest_missing_at_world_y() {
        let mut monitors = HashMap::new();
        let mut portrait = make_monitor("portrait", 0.0, 0.0, 1080.0, 1920.0, 81.59);
        portrait.position_mm = Point { x: 0.0, y: 0.0 };
        let mut landscape = make_monitor("landscape", 1080.0, 0.0, 1920.0, 1080.0, 81.59);
        // Touching horizontally at portrait's right (336 mm), 100 mm down.
        landscape.position_mm = Point { x: 336.3, y: 100.0 };
        monitors.insert("portrait".into(), portrait);
        monitors.insert("landscape".into(), landscape);

        let (tx, _ty) =
            remap_transition(1079, 5, 1080, 5, &monitors).expect("expected pin to source");
        assert!(
            tx < 1080,
            "expected cursor pinned to portrait's right edge, got x={tx}"
        );
    }

    /// Same layout but cursor at a y where the landscape DOES exist in world
    /// coords; the crossing should be allowed (cursor lands on the landscape).
    #[test]
    fn crossing_allowed_when_dest_covers_world_y() {
        let mut monitors = HashMap::new();
        let mut portrait = make_monitor("portrait", 0.0, 0.0, 1080.0, 1920.0, 81.59);
        portrait.position_mm = Point { x: 0.0, y: 0.0 };
        let mut landscape = make_monitor("landscape", 1080.0, 0.0, 1920.0, 1080.0, 81.59);
        landscape.position_mm = Point { x: 336.3, y: 100.0 };
        monitors.insert("portrait".into(), portrait);
        monitors.insert("landscape".into(), landscape);

        // Portrait y=600 -> world_y ~= 600/1920 * 598 = 186.9 mm, inside
        // landscape's world-y range [100, 436].
        let (tx, _ty) =
            remap_transition(1079, 600, 1080, 600, &monitors).expect("expected crossing");
        assert!(tx >= 1080, "expected cursor on landscape, got x={tx}");
    }

    #[test]
    fn gap_zone_crossing_uses_physical_y() {
        // 1080p left at y-offset 165, 1440p right at y=0. Matches the real layout.
        let mut monitors = HashMap::new();
        monitors.insert(
            "left".into(),
            make_monitor("left", -1920.0, 165.0, 1920.0, 1080.0, 81.59),
        );
        monitors.insert(
            "right".into(),
            make_monitor("right", 0.0, 0.0, 2560.0, 1440.0, 108.79),
        );

        // Cursor at top of 1440p (y=5), moving left into 1080p gap zone (y=5 < 165).
        // Physical mapping should land ~y=170 on the 1080p, not snapped to y=165.
        let result = remap_transition(0, 5, -1, 5, &monitors);
        assert!(
            result.is_some(),
            "Gap zone crossing should produce a correction"
        );
        let (_, ty) = result.unwrap();
        assert!(
            ty > 165,
            "Gap zone crossing should map above 1080p floor, got y={}",
            ty
        );
    }
}

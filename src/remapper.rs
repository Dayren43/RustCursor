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

/// The source edge a raw destination coordinate fell outside of, on one axis.
struct Exit {
    /// The right (or bottom) edge when true, the left (or top) edge when false.
    forward: bool,
    /// Pixels past that edge, always >= 1.
    px: i32,
}

/// Which of the source's vertical edges `x` fell outside of. `None` when `x` is
/// still inside the source's x-span, meaning the cursor did not leave through a
/// vertical edge and x is not the axis being crossed.
fn exit_x(x: i32, src: &Monitor) -> Option<Exit> {
    let left = src.bounds.x as i32;
    let right = (src.bounds.x + src.bounds.w) as i32;
    if x >= right {
        Some(Exit {
            forward: true,
            px: x - right + 1,
        })
    } else if x < left {
        Some(Exit {
            forward: false,
            px: left - x,
        })
    } else {
        None
    }
}

/// Horizontal twin of [`exit_x`]: which of the source's horizontal edges `y`
/// fell outside of.
fn exit_y(y: i32, src: &Monitor) -> Option<Exit> {
    let top = src.bounds.y as i32;
    let bottom = (src.bounds.y + src.bounds.h) as i32;
    if y >= bottom {
        Some(Exit {
            forward: true,
            px: y - bottom + 1,
        })
    } else if y < top {
        Some(Exit {
            forward: false,
            px: top - y,
        })
    } else {
        None
    }
}

/// The monitor a horizontal gap-zone crossing lands on: its OS x-span contains
/// `x`, and it sits beyond the source edge the cursor left through. That side
/// check is what keeps a monitor merely *overlapping* the source in x (a
/// vertical neighbour) from being treated as a horizontal destination.
///
/// Nearest edge wins and `identifier` breaks ties, so the pick is the same on
/// every run: `HashMap` iteration order is not stable, and a bare `find` could
/// send the cursor to a different monitor each time the binary starts.
fn dest_across_x<'a>(
    x: i32,
    exit: &Exit,
    src: &Monitor,
    monitors: &'a HashMap<String, Monitor>,
) -> Option<&'a Monitor> {
    let src_left = src.bounds.x as i32;
    let src_right = (src.bounds.x + src.bounds.w) as i32;
    let gap = |m: &Monitor| {
        if exit.forward {
            m.bounds.x as i32 - src_right
        } else {
            src_left - (m.bounds.x + m.bounds.w) as i32
        }
    };
    monitors
        .values()
        .filter(|m| m.identifier != src.identifier)
        .filter(|m| x >= m.bounds.x as i32 && x < (m.bounds.x + m.bounds.w) as i32)
        .filter(|m| gap(m) >= 0)
        .min_by(|a, b| {
            gap(a)
                .cmp(&gap(b))
                .then_with(|| a.identifier.cmp(&b.identifier))
        })
}

/// Vertical twin of [`dest_across_x`], for crossings between stacked monitors
/// whose OS x-offsets differ.
fn dest_across_y<'a>(
    y: i32,
    exit: &Exit,
    src: &Monitor,
    monitors: &'a HashMap<String, Monitor>,
) -> Option<&'a Monitor> {
    let src_top = src.bounds.y as i32;
    let src_bottom = (src.bounds.y + src.bounds.h) as i32;
    let gap = |m: &Monitor| {
        if exit.forward {
            m.bounds.y as i32 - src_bottom
        } else {
            src_top - (m.bounds.y + m.bounds.h) as i32
        }
    };
    monitors
        .values()
        .filter(|m| m.identifier != src.identifier)
        .filter(|m| y >= m.bounds.y as i32 && y < (m.bounds.y + m.bounds.h) as i32)
        .filter(|m| gap(m) >= 0)
        .min_by(|a, b| {
            gap(a)
                .cmp(&gap(b))
                .then_with(|| a.identifier.cmp(&b.identifier))
        })
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

/// Where a gap-zone crossing puts the cursor.
enum Landing {
    /// Remap to this OS pixel position on the destination.
    At(i32, i32),
    /// The destination has no panel at the source's world coordinate, so the
    /// user's layout shares no edge here.
    Blocked,
}

/// Land a horizontal gap-zone crossing. Source-local mm -> shared world mm ->
/// destination-local mm, so the cursor keeps its world height regardless of the
/// two monitors' physical y-offsets.
fn land_across_x(
    old_x: i32,
    old_y: i32,
    new_x: i32,
    old_mon: &Monitor,
    dest_mon: &Monitor,
) -> Landing {
    let old_local_y = cursor_mapper::to_physical(
        Point {
            x: old_x as f32,
            y: old_y as f32,
        },
        old_mon,
    )
    .y;
    let world_y = old_mon.position_mm.y + old_local_y;
    let (_, dest_h_mm) = dest_mon.physical_size_mm();
    let dest_world_top = dest_mon.position_mm.y;
    if world_y < dest_world_top || world_y > dest_world_top + dest_h_mm {
        return Landing::Blocked;
    }
    let target_os_y = cursor_mapper::to_os_pos(
        Point {
            x: 0.0,
            y: world_y - dest_world_top,
        },
        dest_mon,
    )
    .y;
    Landing::At(
        new_x.clamp(
            dest_mon.bounds.x as i32,
            (dest_mon.bounds.x + dest_mon.bounds.w - 1.0) as i32,
        ),
        (target_os_y as i32).clamp(
            dest_mon.bounds.y as i32,
            (dest_mon.bounds.y + dest_mon.bounds.h - 1.0) as i32,
        ),
    )
}

/// Vertical twin of [`land_across_x`]: preserves world x instead of world y.
fn land_across_y(
    old_x: i32,
    old_y: i32,
    new_y: i32,
    old_mon: &Monitor,
    dest_mon: &Monitor,
) -> Landing {
    let old_local_x = cursor_mapper::to_physical(
        Point {
            x: old_x as f32,
            y: old_y as f32,
        },
        old_mon,
    )
    .x;
    let world_x = old_mon.position_mm.x + old_local_x;
    let (dest_w_mm, _) = dest_mon.physical_size_mm();
    let dest_world_left = dest_mon.position_mm.x;
    if world_x < dest_world_left || world_x > dest_world_left + dest_w_mm {
        return Landing::Blocked;
    }
    let target_os_x = cursor_mapper::to_os_pos(
        Point {
            x: world_x - dest_world_left,
            y: 0.0,
        },
        dest_mon,
    )
    .x;
    Landing::At(
        (target_os_x as i32).clamp(
            dest_mon.bounds.x as i32,
            (dest_mon.bounds.x + dest_mon.bounds.w - 1.0) as i32,
        ),
        new_y.clamp(
            dest_mon.bounds.y as i32,
            (dest_mon.bounds.y + dest_mon.bounds.h - 1.0) as i32,
        ),
    )
}

/// Shared exit for every corrected position: a target the raw event already
/// lands on is not worth swallowing the event and injecting a `SetCursorPos`
/// for. These are whole pixels, so this is equality, not a tolerance.
fn correction(new_x: i32, new_y: i32, tx: i32, ty: i32) -> Option<(i32, i32)> {
    if tx == new_x && ty == new_y {
        None
    } else {
        Some((tx, ty))
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
            // The raw destination is in no monitor's rect, so the cursor left
            // through one of the source's edges. Find the panel the OS
            // arrangement puts beyond *that* edge and land on it in world
            // coordinates, instead of clamping the raw pixel.
            let x_exit = exit_x(new_x, old_mon);
            let y_exit = exit_y(new_y, old_mon);
            // A diagonal exit can have a candidate on both axes. The edge the
            // cursor travelled furthest past is the one it actually crossed.
            let prefer_x = match (&x_exit, &y_exit) {
                (Some(x), Some(y)) => x.px >= y.px,
                _ => true,
            };
            let across_x = x_exit
                .as_ref()
                .and_then(|e| dest_across_x(new_x, e, old_mon, monitors))
                .map(|dest| land_across_x(old_x, old_y, new_x, old_mon, dest));
            let across_y = y_exit
                .as_ref()
                .and_then(|e| dest_across_y(new_y, e, old_mon, monitors))
                .map(|dest| land_across_y(old_x, old_y, new_y, old_mon, dest));

            // Not a fallback chain: `or` short-circuits on `Some(Blocked)` too,
            // so a preferred axis that found a destination and then rejected it
            // on world coordinates deliberately wins over a valid landing on
            // the other axis. Falling through would send the cursor to the
            // neighbour it was not heading for.
            match if prefer_x {
                across_x.or(across_y)
            } else {
                across_y.or(across_x)
            } {
                Some(Landing::At(tx, ty)) => correction(new_x, new_y, tx, ty),
                // Nothing beyond that edge, or the layout puts no panel at this
                // world coordinate: block inside the source bounds so the cursor
                // slides along its edge instead of jumping somewhere arbitrary.
                Some(Landing::Blocked) | None => pin_to_source(old_mon, new_x, new_y),
            }
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

            correction(new_x, new_y, tx, ty)
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

    /// Vertical twin of `horizontal_crossing_preserves_physical_y`: crossing
    /// down from the source's horizontal centre lands at the destination's
    /// horizontal centre when both panels are the same physical size, despite
    /// the destination having more horizontal pixels. A raw pixel copy keeps
    /// x=960; physical remapping yields B's centre at ~1280.
    #[test]
    fn vertical_crossing_preserves_physical_x() {
        let mut monitors = HashMap::new();
        monitors.insert(
            "A".into(),
            make_monitor("A", 0.0, -1080.0, 1920.0, 1080.0, 81.59),
        );
        monitors.insert(
            "B".into(),
            make_monitor("B", 0.0, 0.0, 2560.0, 1440.0, 108.84),
        );

        let (cx, _) = remap_transition(960, -1, 960, 0, &monitors)
            .expect("expected a correction crossing into a wider-pixel monitor");
        assert!(
            (cx - 1280).abs() <= 1,
            "expected landing at destination horizontal centre (~1280), got x={cx}"
        );
    }

    /// Vertical gap-zone twin of `gap_zone_crossing_uses_physical_y`. Top 1080p
    /// offset right by 165 px, bottom 1440p at x=0. Cursor at the left of the
    /// 1440p moving up into the 1080p's gap zone (x=5 < 165). Physical mapping
    /// should land ~x=168 on the 1080p, not snapped to its x=165 left edge.
    #[test]
    fn vertical_gap_zone_crossing_uses_physical_x() {
        let mut monitors = HashMap::new();
        monitors.insert(
            "top".into(),
            make_monitor("top", 165.0, -1080.0, 1920.0, 1080.0, 81.59),
        );
        monitors.insert(
            "bottom".into(),
            make_monitor("bottom", 0.0, 0.0, 2560.0, 1440.0, 108.79),
        );

        let result = remap_transition(5, 0, 5, -1, &monitors);
        assert!(
            result.is_some(),
            "vertical gap-zone crossing should produce a correction"
        );
        let (tx, _) = result.unwrap();
        assert!(
            tx > 165,
            "vertical gap-zone crossing should map right of the 1080p left edge, got x={tx}"
        );
    }

    /// Leaving the *bottom* of a monitor must not land on one beside it. The
    /// right-hand panel is taller in pixels, so its OS y-span covers the row
    /// just below the source's bottom edge: a y-span match alone used to select
    /// it and teleport the cursor sideways.
    #[test]
    fn bottom_exit_ignores_a_monitor_beside_the_source() {
        let mut monitors = HashMap::new();
        monitors.insert(
            "left".into(),
            make_monitor("left", 0.0, 0.0, 1920.0, 1080.0, 81.59),
        );
        monitors.insert(
            "tall_right".into(),
            make_monitor("tall_right", 1920.0, 0.0, 2560.0, 1440.0, 108.79),
        );

        // Straight down off the left panel, well inside its x-span.
        let (tx, ty) = remap_transition(500, 1079, 500, 1080, &monitors)
            .expect("expected the crossing to be blocked at the source's bottom edge");
        assert_eq!(
            (tx, ty),
            (500, 1079),
            "expected a pin to the source's bottom edge, not a jump onto tall_right"
        );
    }

    /// A purely vertical exit must not be treated as a horizontal crossing just
    /// because some other monitor's x-span happens to contain the raw x. Here
    /// the panel above is offset right and stops 500 px short of the source, so
    /// the raw destination sits in dead space that neither panel covers.
    #[test]
    fn vertical_exit_is_not_treated_as_horizontal() {
        let mut monitors = HashMap::new();
        monitors.insert(
            "source".into(),
            make_monitor("source", 0.0, 0.0, 1920.0, 1080.0, 81.59),
        );
        monitors.insert(
            "above".into(),
            make_monitor("above", 500.0, -1080.0, 1920.0, 580.0, 81.59),
        );

        // x=600 is inside `above`'s x-span, but the cursor left through the
        // source's top edge, and `above` does not reach down to y=-1.
        let (tx, ty) = remap_transition(600, 0, 600, -1, &monitors)
            .expect("expected the crossing to be blocked at the source's top edge");
        assert_eq!(
            (tx, ty),
            (600, 0),
            "expected a pin to the source's top edge, got ({tx}, {ty})"
        );
    }

    /// Two stacked monitors to the right both span the raw destination x. The
    /// nearer one wins, and the pick must not depend on `HashMap` iteration
    /// order, so building the same layout in either insertion order agrees.
    #[test]
    fn gap_zone_destination_is_nearest_and_order_independent() {
        // `near` starts 80 px past the source's right edge, `far` 480 px past.
        // Neither covers y=500, so the raw destination is a genuine gap.
        let source = make_monitor("source", 0.0, 0.0, 1920.0, 1080.0, 81.59);
        let near = make_monitor("near", 2000.0, -1080.0, 1920.0, 1080.0, 81.59);
        let far = make_monitor("far", 2400.0, 2000.0, 1920.0, 1080.0, 81.59);

        let mut forward = HashMap::new();
        forward.insert("source".into(), source.clone());
        forward.insert("near".into(), near.clone());
        forward.insert("far".into(), far.clone());

        let mut reverse = HashMap::new();
        reverse.insert("far".into(), far);
        reverse.insert("near".into(), near);
        reverse.insert("source".into(), source);

        let a = remap_transition(1919, 500, 2500, 500, &forward).expect("crossing");
        let b = remap_transition(1919, 500, 2500, 500, &reverse).expect("crossing");
        assert_eq!(a, b, "destination must not depend on insertion order");
        assert!(
            a.1 < 0,
            "expected a landing on `near` (OS y in -1080..0), got y={}",
            a.1
        );
    }
}

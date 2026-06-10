//! Monitors tab: visual layout canvas plus a per-monitor diagonal-size editor.
//!
//! The canvas at the top draws each connected monitor as a rectangle whose
//! aspect ratio matches its physical mm dimensions. Dragging a rectangle
//! updates that monitor's `position_mm` locally so the user sees the
//! preview move. Disk write + IPC reload of the parent's cursor remap fires
//! once when the drag ends (live during-drag updates aren't viable across
//! the subprocess boundary); the write pins every HWID-bearing monitor's
//! position, not just the dragged one, so saved profiles never mix pinned
//! and default-seeded coordinate bases.
//!
//! Snap-to-edge is applied during drag: the dragged monitor's edges snap to
//! any other monitor's edges within 10 mm (touching candidates: left-to-right
//! and right-to-left; alignment candidates: matching lefts/rights/tops/
//! bottoms). Holding **Alt** during a drag disables snap.
//!
//! The canvas also validates the arrangement against the Windows pixel
//! layout every frame: pairs that are adjacent in Windows but have no
//! cross-axis overlap here would have all their crossings blocked by the
//! remapper, so they get a red outline and an explanation (see
//! [`layout_issues`]).
//!
//! Sizes come from a fresh `Config::load()` -> active profile lookup via
//! `build_monitor_map`, so the displayed values match what the running
//! remapper uses. Edits auto-save on `DragValue::drag_stopped` /
//! `lost_focus` for the numeric field, and on drag end for the canvas.
//! Both paths call `gui::reload_active_profile` so changes apply live.
//!
//! Writes go to `[[profile.monitor]]` entries keyed by stable HWID. Rows for
//! monitors that don't expose a HWID can't be persisted (size or position);
//! the canvas drag path skips them and the size editor shows a "no hardware
//! ID" note in place of the HWID line.

use eframe::egui;

use crate::gui::config_io::ConfigDoc;

/// Snap range in millimetres. Edges within this distance attract.
const SNAP_MM: f32 = 10.0;
/// Canvas height in pixels. The width fills the available area.
const CANVAS_PX_H: f32 = 220.0;
/// Minimum cross-axis overlap in millimetres for a Windows-adjacent pair to
/// count as having a usable shared physical edge. Below this every crossing
/// at the pair's pixel boundary pins to the source monitor.
const MIN_SHARED_EDGE_MM: f32 = 5.0;

struct MonitorRow {
    device: String,
    hwid: Option<String>,
    resolution: (u32, u32),
    os_x: i32,
    os_y: i32,
    size_in: f32,
    initial_size_in: f32,
    position_mm: (f32, f32),
    initial_position_mm: (f32, f32),
}

impl MonitorRow {
    fn size_mm(&self) -> (f32, f32) {
        let aspect = if self.resolution.1 == 0 {
            16.0 / 9.0
        } else {
            self.resolution.0 as f32 / self.resolution.1 as f32
        };
        let diag_mm = self.size_in * 25.4;
        let h_mm = diag_mm / ((aspect * aspect + 1.0).sqrt());
        let w_mm = h_mm * aspect;
        (w_mm, h_mm)
    }
}

pub struct MonitorsTab {
    rows: Vec<MonitorRow>,
    default_size_in: f32,
    last_error: Option<String>,
}

impl MonitorsTab {
    pub fn new() -> Self {
        let mut tab = Self {
            rows: Vec::new(),
            default_size_in: 27.0,
            last_error: None,
        };
        tab.refresh();
        tab
    }

    fn refresh(&mut self) {
        let cfg = rust_cursor::config::Config::load();
        self.default_size_in = cfg.default_size_in;

        // build_monitor_map already applies the active profile (via SIZES) for
        // both size and position. Read it and treat its values as the source
        // of truth so the rows match what the running remapper sees.
        let mut rows: Vec<MonitorRow> = crate::platform::windows::build_monitor_map()
            .into_values()
            .map(|m| {
                let pos = (m.position_mm.x, m.position_mm.y);
                MonitorRow {
                    device: m.identifier.clone(),
                    hwid: m.hwid,
                    resolution: m.resolution,
                    os_x: m.bounds.x as i32,
                    os_y: m.bounds.y as i32,
                    size_in: m.physical_size_in,
                    initial_size_in: m.physical_size_in,
                    position_mm: pos,
                    initial_position_mm: pos,
                }
            })
            .collect();
        rows.sort_by_key(|r| r.os_x);
        self.rows = rows;
        self.last_error = None;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Monitors");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Refresh").clicked() {
                    self.refresh();
                }
            });
        });
        ui.add_space(8.0);

        if self.rows.is_empty() {
            ui.label(egui::RichText::new("No monitors detected.").small().weak());
            return;
        }

        // Recomputed every frame so the highlight tracks the drag live and
        // clears the moment the user fixes the arrangement.
        let issues = layout_issues(&self.rows);
        let mut flagged = vec![false; self.rows.len()];
        for issue in &issues {
            flagged[issue.a] = true;
            flagged[issue.b] = true;
        }

        let drag_stopped_idx = self.show_layout_canvas(ui, &flagged);
        if let Some(idx) = drag_stopped_idx {
            self.save_position(idx);
        }

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Drag to reposition. Edges snap to other monitors within 10 mm; hold Alt to disable.")
                .small()
                .weak(),
        );
        if !issues.is_empty() {
            ui.add_space(4.0);
            for issue in &issues {
                ui.colored_label(egui::Color32::from_rgb(220, 90, 90), &issue.message);
            }
            ui.label(
                egui::RichText::new(
                    "Arrange the rectangles to match how the monitors are laid out in Windows Display Settings.",
                )
                .small()
                .weak(),
            );
        }
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        self.show_rows(ui);

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(format!(
                "Default ({:.1} in) is used for any monitor without an override.",
                self.default_size_in
            ))
            .small()
            .weak(),
        );

        if let Some(err) = &self.last_error {
            ui.add_space(8.0);
            ui.colored_label(egui::Color32::from_rgb(220, 90, 90), err);
        }
    }

    /// Returns the row index whose drag just ended (if any), so the caller
    /// can write the position to disk. Rows marked in `flagged` are part of
    /// a layout issue and get a red outline.
    fn show_layout_canvas(&mut self, ui: &mut egui::Ui, flagged: &[bool]) -> Option<usize> {
        let canvas_w = ui.available_width();
        let (canvas_rect, _) =
            ui.allocate_exact_size(egui::vec2(canvas_w, CANVAS_PX_H), egui::Sense::hover());

        // Compute world bounds across all monitor rectangles.
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for r in &self.rows {
            let (w, h) = r.size_mm();
            min_x = min_x.min(r.position_mm.0);
            min_y = min_y.min(r.position_mm.1);
            max_x = max_x.max(r.position_mm.0 + w);
            max_y = max_y.max(r.position_mm.1 + h);
        }
        let world_w = (max_x - min_x).max(1.0);
        let world_h = (max_y - min_y).max(1.0);

        let pad = 16.0;
        let scale_x = (canvas_rect.width() - 2.0 * pad) / world_w;
        let scale_y = (canvas_rect.height() - 2.0 * pad) / world_h;
        let scale = scale_x.min(scale_y).max(0.001);

        let centre = canvas_rect.center();
        let half_world = egui::vec2(world_w * scale * 0.5, world_h * scale * 0.5);
        let world_origin = centre - half_world - egui::vec2(min_x * scale, min_y * scale);
        let world_to_screen =
            |wx: f32, wy: f32| -> egui::Pos2 { world_origin + egui::vec2(wx * scale, wy * scale) };

        let painter = ui.painter_at(canvas_rect);
        painter.rect_filled(canvas_rect, 4.0, ui.style().visuals.extreme_bg_color);

        let alt_held = ui.input(|i| i.modifiers.alt);
        let mut drag_deltas: Vec<(usize, egui::Vec2)> = Vec::new();
        let mut stopped: Option<usize> = None;

        for (i, r) in self.rows.iter().enumerate() {
            let (w_mm, h_mm) = r.size_mm();
            let p0 = world_to_screen(r.position_mm.0, r.position_mm.1);
            let p1 = world_to_screen(r.position_mm.0 + w_mm, r.position_mm.1 + h_mm);
            let screen_rect = egui::Rect::from_two_pos(p0, p1);

            let id = ui.id().with(("layout_monitor", i));
            let resp = ui.interact(screen_rect, id, egui::Sense::drag());

            let fill = if resp.dragged() {
                egui::Color32::from_rgb(80, 110, 150)
            } else if resp.hovered() {
                egui::Color32::from_rgb(60, 80, 115)
            } else {
                egui::Color32::from_rgb(45, 60, 90)
            };
            painter.rect_filled(screen_rect, 4.0, fill);
            let stroke = if flagged.get(i).copied().unwrap_or(false) {
                egui::Stroke::new(2.0, egui::Color32::from_rgb(220, 90, 90))
            } else {
                egui::Stroke::new(1.0, egui::Color32::from_gray(220))
            };
            painter.rect_stroke(screen_rect, 4.0, stroke);
            painter.text(
                screen_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{}\n{:.1}\"", r.device, r.size_in),
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );

            if resp.dragged() {
                let dp = resp.drag_delta();
                drag_deltas.push((i, egui::vec2(dp.x / scale, dp.y / scale)));
            }
            if resp.drag_stopped() {
                stopped = Some(i);
            }
        }

        for (idx, delta_mm) in drag_deltas {
            self.rows[idx].position_mm.0 += delta_mm.x;
            self.rows[idx].position_mm.1 += delta_mm.y;
            if !alt_held {
                apply_snap(&mut self.rows, idx);
            }
            // No in-drag cross-process push: the parent's runtime SIZES are
            // updated on drag-end via `save_position` → ConfigDoc write →
            // `gui::reload_active_profile`, which posts the IPC reload signal.
            // The canvas's drag preview is purely local (paints from
            // `row.position_mm`), so the user still sees the rectangle move.
        }

        stopped
    }

    fn show_rows(&mut self, ui: &mut egui::Ui) {
        let connected_hwids: Vec<String> =
            self.rows.iter().filter_map(|r| r.hwid.clone()).collect();
        let description = self
            .rows
            .iter()
            .map(|r| r.device.as_str())
            .collect::<Vec<_>>()
            .join(" + ");

        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut size_saves: Vec<(String, Option<String>, f32)> = Vec::new();

            for row in self.rows.iter_mut() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new(&row.device).strong());
                    ui.label(
                        egui::RichText::new(format!(
                            "{} x {}  @  ({}, {})",
                            row.resolution.0, row.resolution.1, row.os_x, row.os_y
                        ))
                        .small()
                        .weak(),
                    );
                    if let Some(h) = &row.hwid {
                        ui.label(egui::RichText::new(h).small().weak());
                    } else {
                        ui.label(
                            egui::RichText::new("(no hardware ID; edits can't be persisted)")
                                .small()
                                .weak(),
                        );
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Diagonal");
                        let resp = ui.add(
                            egui::DragValue::new(&mut row.size_in)
                                .range(10.0..=80.0)
                                .speed(0.1)
                                .suffix(" in"),
                        );
                        let done = resp.drag_stopped() || resp.lost_focus();
                        if done && (row.size_in - row.initial_size_in).abs() > f32::EPSILON {
                            size_saves.push((row.device.clone(), row.hwid.clone(), row.size_in));
                        }
                    });
                });
                ui.add_space(4.0);
            }

            for (device, hwid, size) in size_saves {
                if let Some(h) = hwid {
                    self.save_size(&connected_hwids, &description, &device, &h, size);
                }
            }
        });
    }

    fn save_size(
        &mut self,
        profile_hwids: &[String],
        description: &str,
        device: &str,
        hwid: &str,
        size_in: f32,
    ) {
        let result = (|| -> Result<(), String> {
            let mut doc = ConfigDoc::load()?;
            doc.upsert_profile_monitor(profile_hwids, hwid, size_in, None, description);
            doc.save()
        })();
        match result {
            Ok(()) => {
                self.last_error = None;
                crate::gui::reload_active_profile();
                for row in self.rows.iter_mut() {
                    if row.device == device {
                        row.initial_size_in = row.size_in;
                    }
                }
            }
            Err(e) => self.last_error = Some(e),
        }
    }

    /// Persist the layout after a drag ends. Writes every row with a HWID,
    /// not just the dragged one, so the profile is always a complete snapshot
    /// in one coordinate basis — pinning only the dragged monitor would leave
    /// the others default-seeded against a layout that no longer matches.
    fn save_position(&mut self, idx: usize) {
        if self.rows[idx].hwid.is_none() {
            // No HWID -> can't persist; row stays edited in-memory only.
            return;
        }

        let connected_hwids: Vec<String> =
            self.rows.iter().filter_map(|r| r.hwid.clone()).collect();
        let description = self
            .rows
            .iter()
            .map(|r| r.device.as_str())
            .collect::<Vec<_>>()
            .join(" + ");

        let result = (|| -> Result<(), String> {
            let mut doc = ConfigDoc::load()?;
            for row in self.rows.iter().filter(|r| r.hwid.is_some()) {
                let hwid = row.hwid.as_deref().expect("filtered on hwid presence");
                doc.upsert_profile_monitor(
                    &connected_hwids,
                    hwid,
                    row.size_in,
                    Some(row.position_mm),
                    &description,
                );
            }
            doc.save()
        })();

        match result {
            Ok(()) => {
                self.last_error = None;
                for row in self.rows.iter_mut() {
                    row.initial_position_mm = row.position_mm;
                }
                crate::gui::reload_active_profile();
            }
            Err(e) => self.last_error = Some(e),
        }
    }
}

/// Snap the row at `idx` to align with other rows' edges. Touching candidates
/// (right-to-left and left-to-right) plus alignment candidates (matching
/// lefts/rights/tops/bottoms). Picks the closest match per axis within
/// `SNAP_MM` and adjusts only that axis.
fn apply_snap(rows: &mut [MonitorRow], idx: usize) {
    let (my_w, my_h) = rows[idx].size_mm();
    let my_left = rows[idx].position_mm.0;
    let my_top = rows[idx].position_mm.1;
    let my_right = my_left + my_w;
    let my_bottom = my_top + my_h;

    let mut best_dx: Option<f32> = None;
    let mut best_dy: Option<f32> = None;

    for (j, other) in rows.iter().enumerate() {
        if j == idx {
            continue;
        }
        let (o_w, o_h) = other.size_mm();
        let o_left = other.position_mm.0;
        let o_right = o_left + o_w;
        let o_top = other.position_mm.1;
        let o_bottom = o_top + o_h;

        for &(mine, theirs) in &[
            (my_left, o_right),
            (my_right, o_left),
            (my_left, o_left),
            (my_right, o_right),
        ] {
            let delta = theirs - mine;
            if delta.abs() <= SNAP_MM && best_dx.is_none_or(|b| delta.abs() < b.abs()) {
                best_dx = Some(delta);
            }
        }
        for &(mine, theirs) in &[
            (my_top, o_bottom),
            (my_bottom, o_top),
            (my_top, o_top),
            (my_bottom, o_bottom),
        ] {
            let delta = theirs - mine;
            if delta.abs() <= SNAP_MM && best_dy.is_none_or(|b| delta.abs() < b.abs()) {
                best_dy = Some(delta);
            }
        }
    }

    if let Some(dx) = best_dx {
        rows[idx].position_mm.0 += dx;
    }
    if let Some(dy) = best_dy {
        rows[idx].position_mm.1 += dy;
    }
}

/// A monitor pair whose physical arrangement makes the remapper block every
/// crossing at their shared Windows edge.
struct LayoutIssue {
    a: usize,
    b: usize,
    message: String,
}

/// Detect physical layouts that can't work with the current Windows pixel
/// arrangement. Windows only lets the cursor cross where pixel rects are
/// adjacent, and at such a boundary the remapper preserves the cross-axis
/// world-mm coordinate, pinning to the source when the destination has no
/// panel there (see `remap_transition`). So a Windows-adjacent pair with no
/// cross-axis mm overlap is unreachable from each other: flag it.
///
/// Deliberately not flagged: in-plane order disagreeing with Windows (the
/// remapper never consults it on that axis) and partial overlaps (modelling
/// offset panels is the point of the app).
fn layout_issues(rows: &[MonitorRow]) -> Vec<LayoutIssue> {
    let mut issues = Vec::new();
    for i in 0..rows.len() {
        for j in (i + 1)..rows.len() {
            let (a, b) = (&rows[i], &rows[j]);

            let (ax0, ay0) = (a.os_x as i64, a.os_y as i64);
            let (ax1, ay1) = (ax0 + a.resolution.0 as i64, ay0 + a.resolution.1 as i64);
            let (bx0, by0) = (b.os_x as i64, b.os_y as i64);
            let (bx1, by1) = (bx0 + b.resolution.0 as i64, by0 + b.resolution.1 as i64);

            let px_x_overlap = ax1.min(bx1) - ax0.max(bx0);
            let px_y_overlap = ay1.min(by1) - ay0.max(by0);
            // Edge adjacency in the OS arrangement. Touching on one axis
            // implies zero overlap there, so the two cases are exclusive;
            // corner-only contact (both overlaps zero) is skipped as a
            // degenerate crossing nobody hits in practice.
            let side_by_side = (ax1 == bx0 || bx1 == ax0) && px_y_overlap > 0;
            let stacked = (ay1 == by0 || by1 == ay0) && px_x_overlap > 0;
            if !side_by_side && !stacked {
                continue;
            }

            let (a_w_mm, a_h_mm) = a.size_mm();
            let (b_w_mm, b_h_mm) = b.size_mm();
            let mm_x_overlap = (a.position_mm.0 + a_w_mm).min(b.position_mm.0 + b_w_mm)
                - a.position_mm.0.max(b.position_mm.0);
            let mm_y_overlap = (a.position_mm.1 + a_h_mm).min(b.position_mm.1 + b_h_mm)
                - a.position_mm.1.max(b.position_mm.1);

            let message = if side_by_side && mm_y_overlap < MIN_SHARED_EDGE_MM {
                format!(
                    "{} and {} are side by side in Windows but don't overlap vertically here; crossings between them will be blocked.",
                    a.device, b.device
                )
            } else if stacked && mm_x_overlap < MIN_SHARED_EDGE_MM {
                format!(
                    "{} and {} are stacked in Windows but don't overlap horizontally here; crossings between them will be blocked.",
                    a.device, b.device
                )
            } else {
                continue;
            };
            issues.push(LayoutIssue { a: i, b: j, message });
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 27" 16:9 panel: ~597.9 mm x ~336.3 mm.
    fn row(device: &str, os_x: i32, os_y: i32, res: (u32, u32), mm: (f32, f32)) -> MonitorRow {
        MonitorRow {
            device: device.to_string(),
            hwid: None,
            resolution: res,
            os_x,
            os_y,
            size_in: 27.0,
            initial_size_in: 27.0,
            position_mm: mm,
            initial_position_mm: mm,
        }
    }

    #[test]
    fn consistent_side_by_side_has_no_issues() {
        let rows = vec![
            row("A", 0, 0, (1920, 1080), (0.0, 0.0)),
            row("B", 1920, 0, (1920, 1080), (600.0, 0.0)),
        ];
        assert!(layout_issues(&rows).is_empty());
    }

    #[test]
    fn stacked_in_windows_but_side_by_side_here_is_blocked() {
        let rows = vec![
            row("A", 0, -1080, (1920, 1080), (0.0, 0.0)),
            row("B", 0, 0, (1920, 1080), (600.0, 0.0)),
        ];
        let issues = layout_issues(&rows);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("stacked"), "{}", issues[0].message);
    }

    #[test]
    fn side_by_side_in_windows_but_stacked_here_is_blocked() {
        let rows = vec![
            row("A", 0, 0, (1920, 1080), (0.0, 0.0)),
            row("B", 1920, 0, (1920, 1080), (0.0, 340.0)),
        ];
        let issues = layout_issues(&rows);
        assert_eq!(issues.len(), 1);
        assert!(
            issues[0].message.contains("side by side"),
            "{}",
            issues[0].message
        );
    }

    /// Offset panels with partial overlap are the app's normal use case and
    /// must not be flagged.
    #[test]
    fn partial_cross_axis_overlap_is_fine() {
        let rows = vec![
            row("A", 0, 0, (1920, 1080), (0.0, 0.0)),
            row("B", 1920, 0, (1920, 1080), (600.0, 200.0)),
        ];
        assert!(layout_issues(&rows).is_empty());
    }

    /// Pairs that aren't adjacent in the Windows arrangement can't cross
    /// directly, so their physical relationship is unconstrained.
    #[test]
    fn non_adjacent_pair_is_ignored() {
        let rows = vec![
            row("A", 0, 0, (1920, 1080), (0.0, 0.0)),
            row("B", 5000, 0, (1920, 1080), (0.0, 340.0)),
        ];
        assert!(layout_issues(&rows).is_empty());
    }

    /// Mirrors the real mixed-DPI layout: 1080p left with an OS y-offset,
    /// 1440p right at y=0. Adjacent with healthy vertical mm overlap.
    #[test]
    fn os_y_offset_pair_with_mm_overlap_is_fine() {
        let rows = vec![
            row("left", -1920, 165, (1920, 1080), (0.0, 0.0)),
            row("right", 0, 0, (2560, 1440), (600.0, 0.0)),
        ];
        assert!(layout_issues(&rows).is_empty());
    }
}

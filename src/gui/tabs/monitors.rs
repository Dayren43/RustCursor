//! Monitors tab: visual layout canvas plus a per-monitor diagonal-size editor.
//!
//! The canvas at the top draws each connected monitor as a rectangle whose
//! aspect ratio matches its physical mm dimensions. Dragging a rectangle
//! updates that monitor's `position_mm` and, on every drag frame, pushes the
//! new value into the runtime `SIZES` lookup plus triggers a monitor-map
//! rebuild so cursor crossings reflect the layout immediately. The disk
//! write happens once when the drag ends.
//!
//! Snap-to-edge is applied during drag: the dragged monitor's edges snap to
//! any other monitor's edges within 10 mm (touching candidates: left-to-right
//! and right-to-left; alignment candidates: matching lefts/rights/tops/
//! bottoms). Holding **Alt** during a drag disables snap.
//!
//! Sizes come from a fresh `Config::load()` -> active profile lookup via
//! `build_monitor_map`, so the displayed values match what the running
//! remapper uses. Edits auto-save on `DragValue::drag_stopped` /
//! `lost_focus` for the numeric field, and on drag end for the canvas.
//! Both paths call `gui::reload_active_profile` so changes apply live.
//!
//! Writes go to `[[profile.monitor]]` entries keyed by stable HWID. Rows for
//! monitors that don't expose a HWID fall back to legacy `[[monitor]]`
//! entries (size only) and are skipped in the canvas drag path.

use eframe::egui;

use crate::gui::config_io::ConfigDoc;

/// Snap range in millimetres. Edges within this distance attract.
const SNAP_MM: f32 = 10.0;
/// Canvas height in pixels. The width fills the available area.
const CANVAS_PX_H: f32 = 220.0;

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
            ui.label(
                egui::RichText::new("No monitors detected.")
                    .small()
                    .weak(),
            );
            return;
        }

        let drag_stopped_idx = self.show_layout_canvas(ui);
        if let Some(idx) = drag_stopped_idx {
            self.save_position(idx);
        }

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Drag to reposition. Edges snap to other monitors within 10 mm; hold Alt to disable.")
                .small()
                .weak(),
        );
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
    /// can write the position to disk.
    fn show_layout_canvas(&mut self, ui: &mut egui::Ui) -> Option<usize> {
        let canvas_w = ui.available_width();
        let (canvas_rect, _) = ui.allocate_exact_size(
            egui::vec2(canvas_w, CANVAS_PX_H),
            egui::Sense::hover(),
        );

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
        let world_to_screen = |wx: f32, wy: f32| -> egui::Pos2 {
            world_origin + egui::vec2(wx * scale, wy * scale)
        };

        let painter = ui.painter_at(canvas_rect);
        painter.rect_filled(
            canvas_rect,
            4.0,
            ui.style().visuals.extreme_bg_color,
        );

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
            painter.rect_stroke(
                screen_rect,
                4.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(220)),
            );
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
            // Live-push to the runtime so cursor crossings reflect the drag
            // immediately. Disk write happens once on drag end.
            let r = &self.rows[idx];
            if let Some(h) = r.hwid.as_deref() {
                rust_cursor::config::override_hwid(h, r.size_in, r.position_mm);
                crate::platform::windows::trigger_monitor_rebuild();
            }
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
                            egui::RichText::new("(no hardware ID; saved by OS slot)")
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
                self.save_size(&connected_hwids, &description, &device, hwid.as_deref(), size);
            }
        });
    }

    fn save_size(
        &mut self,
        profile_hwids: &[String],
        description: &str,
        device: &str,
        hwid: Option<&str>,
        size_in: f32,
    ) {
        let result = (|| -> Result<(), String> {
            let mut doc = ConfigDoc::load()?;
            match hwid {
                Some(h) => doc.upsert_profile_monitor(profile_hwids, h, size_in, None, description),
                None => doc.set_monitor_size(device, size_in),
            }
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

    fn save_position(&mut self, idx: usize) {
        let Some(hwid) = self.rows[idx].hwid.clone() else {
            // No HWID -> no profile entry; legacy entries are size-only.
            return;
        };
        let pos = self.rows[idx].position_mm;
        let size = self.rows[idx].size_in;

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
            doc.upsert_profile_monitor(&connected_hwids, &hwid, size, Some(pos), &description);
            doc.save()
        })();

        match result {
            Ok(()) => {
                self.last_error = None;
                self.rows[idx].initial_position_mm = pos;
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

    for j in 0..rows.len() {
        if j == idx {
            continue;
        }
        let (o_w, o_h) = rows[j].size_mm();
        let o_left = rows[j].position_mm.0;
        let o_right = o_left + o_w;
        let o_top = rows[j].position_mm.1;
        let o_bottom = o_top + o_h;

        for &(mine, theirs) in &[
            (my_left, o_right),
            (my_right, o_left),
            (my_left, o_left),
            (my_right, o_right),
        ] {
            let delta = theirs - mine;
            if delta.abs() <= SNAP_MM && best_dx.map_or(true, |b| delta.abs() < b.abs()) {
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
            if delta.abs() <= SNAP_MM && best_dy.map_or(true, |b| delta.abs() < b.abs()) {
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


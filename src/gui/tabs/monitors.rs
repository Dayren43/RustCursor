//! Monitors tab: live device list with per-monitor `size_in` overrides.
//!
//! Sizes come from a fresh `Config::load()` rather than the running process's
//! cached `SIZES`, so the values match what the next start-up would see. Edits
//! auto-save on drag-end / focus-loss to keep disk writes bounded during
//! `DragValue` interaction. A "Restart to apply" banner appears as soon as
//! any row's saved size diverges from the snapshot taken on tab construction
//! or last refresh.
//!
//! Writes go to `[[profile.monitor]]` entries keyed by stable HWID, scoped to
//! the profile whose `hwids` set equals the currently-connected monitor set.
//! Rows for monitors that don't expose a HWID fall back to legacy
//! `[[monitor]]` entries keyed by OS slot name.

use std::collections::HashMap;

use eframe::egui;

use rust_cursor::config::Config;

use crate::gui::config_io::ConfigDoc;

struct MonitorRow {
    device: String,
    hwid: Option<String>,
    resolution: (u32, u32),
    x: i32,
    y: i32,
    /// Snapshot at refresh time; the restart banner is driven by `live` differing from this.
    initial_size_in: f32,
    live_size_in: f32,
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
        let cfg = Config::load();
        self.default_size_in = cfg.default_size_in;

        // Build a HWID -> size lookup from the active profile (matched against
        // currently-connected HWIDs). Falls through to the legacy device-keyed
        // map for monitors that have no profile entry yet.
        let monitors = crate::platform::windows::build_monitor_map();
        let connected_hwids: Vec<String> = monitors
            .values()
            .filter_map(|m| m.hwid.clone())
            .collect();
        let profile = cfg.active_profile(&connected_hwids);
        let by_hwid: HashMap<String, f32> = profile
            .map(|p| {
                p.monitors
                    .iter()
                    .map(|m| (m.hwid.clone(), m.size_in))
                    .collect()
            })
            .unwrap_or_default();
        let by_device: HashMap<String, f32> = cfg
            .legacy_monitors
            .iter()
            .map(|m| (m.device.clone(), m.size_in))
            .collect();

        let mut rows: Vec<MonitorRow> = monitors
            .into_values()
            .map(|m| {
                let size = m
                    .hwid
                    .as_ref()
                    .and_then(|h| by_hwid.get(h).copied())
                    .or_else(|| by_device.get(&m.identifier).copied())
                    .unwrap_or(cfg.default_size_in);
                MonitorRow {
                    device: m.identifier.clone(),
                    hwid: m.hwid,
                    resolution: m.resolution,
                    x: m.bounds.x as i32,
                    y: m.bounds.y as i32,
                    initial_size_in: size,
                    live_size_in: size,
                }
            })
            .collect();
        rows.sort_by_key(|r| r.x);
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

        if self.needs_restart() {
            ui.colored_label(
                egui::Color32::from_rgb(220, 170, 80),
                "Restart RustCursor to apply changes.",
            );
            ui.add_space(8.0);
        }

        if self.rows.is_empty() {
            ui.label(
                egui::RichText::new("No monitors detected.")
                    .small()
                    .weak(),
            );
            return;
        }

        // Snapshot the HWID set the writer uses to key the profile. Doing this
        // up front keeps the per-row save call cheap and avoids re-collecting
        // per save.
        let connected_hwids: Vec<String> =
            self.rows.iter().filter_map(|r| r.hwid.clone()).collect();
        let description = self
            .rows
            .iter()
            .map(|r| r.device.as_str())
            .collect::<Vec<_>>()
            .join(" + ");

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Collect rows that need saving, then save outside the borrow so
            // the &mut self call doesn't conflict with the row iteration.
            let mut to_save: Vec<(String, Option<String>, f32)> = Vec::new();

            for row in self.rows.iter_mut() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new(&row.device).strong());
                    ui.label(
                        egui::RichText::new(format!(
                            "{} x {}  @  ({}, {})",
                            row.resolution.0, row.resolution.1, row.x, row.y
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
                        let response = ui.add(
                            egui::DragValue::new(&mut row.live_size_in)
                                .range(10.0..=80.0)
                                .speed(0.1)
                                .suffix(" in"),
                        );
                        let done = response.drag_stopped() || response.lost_focus();
                        if done && (row.live_size_in - row.initial_size_in).abs() > f32::EPSILON {
                            to_save.push((
                                row.device.clone(),
                                row.hwid.clone(),
                                row.live_size_in,
                            ));
                        }
                    });
                });
                ui.add_space(4.0);
            }

            for (device, hwid, size) in to_save {
                self.save_one(&connected_hwids, &description, &device, hwid.as_deref(), size);
            }
        });

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

    fn needs_restart(&self) -> bool {
        self.rows
            .iter()
            .any(|r| (r.live_size_in - r.initial_size_in).abs() > f32::EPSILON)
    }

    fn save_one(
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
                Some(h) => doc.upsert_profile_monitor(profile_hwids, h, size_in, description),
                None => doc.set_monitor_size(device, size_in),
            }
            doc.save()
        })();
        match result {
            Ok(()) => self.last_error = None,
            Err(e) => self.last_error = Some(e),
        }
    }
}

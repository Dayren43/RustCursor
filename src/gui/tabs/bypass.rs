//! Bypass tab: editor for the `bypass_processes` list. Foreground processes
//! whose focus pauses cursor remapping (e.g. windowed-fullscreen games that
//! aren't picked up by the auto-detect in [`focus`]). Entries are exe
//! basenames, matched case-insensitively at runtime.
//!
//! Each add/remove writes the full array to `config.toml` immediately and
//! also calls `config::install_bypass_processes` so the input loop's next
//! focus check sees the new list. No restart needed.

use eframe::egui;

use rust_cursor::config::Config;

use crate::gui::config_io::ConfigDoc;

pub struct BypassTab {
    entries: Vec<String>,
    new_entry: String,
    last_error: Option<String>,
}

impl BypassTab {
    pub fn new() -> Self {
        let cfg = Config::load();
        Self {
            entries: cfg.bypass_processes,
            new_entry: String::new(),
            last_error: None,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Bypass");
        ui.add_space(8.0);

        ui.label(
            egui::RichText::new(
                "Foreground processes whose focus pauses cursor remapping. Use exe basenames like \"csgo2.exe\"; case-insensitive. Fullscreen DX/Vk/GL games are auto-detected and don't need to be listed.",
            )
            .small()
            .weak(),
        );
        ui.add_space(8.0);

        let mut to_add: Option<String> = None;
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.new_entry)
                    .hint_text("exe basename, e.g. game.exe"),
            );
            let enter_submitted =
                resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Add").clicked() || enter_submitted {
                let trimmed = self.new_entry.trim().to_string();
                if !trimmed.is_empty() {
                    to_add = Some(trimmed);
                }
            }
        });
        if let Some(name) = to_add {
            let already = self.entries.iter().any(|e| e.eq_ignore_ascii_case(&name));
            if !already {
                self.entries.push(name);
                self.save();
            }
            self.new_entry.clear();
        }

        ui.add_space(8.0);

        if self.entries.is_empty() {
            ui.label(
                egui::RichText::new("No bypass processes configured.")
                    .small()
                    .weak(),
            );
        } else {
            let mut remove_idx: Option<usize> = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, entry) in self.entries.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(entry);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Remove").clicked() {
                                remove_idx = Some(i);
                            }
                        });
                    });
                    ui.separator();
                }
            });
            if let Some(i) = remove_idx {
                self.entries.remove(i);
                self.save();
            }
        }

        if let Some(err) = &self.last_error {
            ui.add_space(8.0);
            ui.colored_label(egui::Color32::from_rgb(220, 90, 90), err);
        }
    }

    fn save(&mut self) {
        let result = (|| -> Result<(), String> {
            let mut doc = ConfigDoc::load()?;
            doc.set_bypass_processes(&self.entries);
            doc.save()
        })();
        match result {
            Ok(()) => {
                self.last_error = None;
                // Re-install BYPASS locally and signal the parent process so
                // its input loop's next focus check sees the new list.
                crate::gui::reload_active_profile();
            }
            Err(e) => self.last_error = Some(e),
        }
    }
}

impl crate::gui::tabs::SettingsTab for BypassTab {
    fn title(&self) -> &'static str {
        "Bypass"
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        self.ui(ui);
    }
}

//! General tab: backend selection, default monitor size, and auto-start toggle.
//!
//! Edits to backend and default_size_in write through to `config.toml` on
//! change. The running RustCursor process keeps its in-memory config, so a
//! restart banner appears as soon as the saved value diverges from what was
//! loaded at startup.

use eframe::egui;

use rust_cursor::config::{Backend, Config};

use crate::gui::autostart;
use crate::gui::config_io::ConfigDoc;

pub struct GeneralTab {
    /// Snapshot of the backend at window open, used to flag the restart banner
    /// (the input-loop thread is spawned once at startup and can't be swapped
    /// live). `default_size_in` is also snapshotted for the save-and-clear
    /// flow, but doesn't drive the banner since it hot-reloads.
    initial_backend: Backend,
    initial_default_size_in: f32,

    /// Live values driven by the widgets.
    backend: Backend,
    default_size_in: f32,

    /// Auto-start state, refreshed from `schtasks /Query` on open.
    autostart_enabled: bool,

    /// Last error from a save or schtasks call, shown inline until cleared.
    last_error: Option<String>,

    /// Transient success message, replaced on each successful action.
    last_status: Option<String>,
}

impl GeneralTab {
    pub fn new() -> Self {
        let cfg = Config::load();
        Self {
            initial_backend: cfg.backend,
            initial_default_size_in: cfg.default_size_in,
            backend: cfg.backend,
            default_size_in: cfg.default_size_in,
            autostart_enabled: autostart::is_enabled(),
            last_error: None,
            last_status: None,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("General");
        ui.add_space(8.0);

        if self.needs_restart() {
            ui.colored_label(
                egui::Color32::from_rgb(220, 170, 80),
                "Restart RustCursor for the backend change to take effect.",
            );
            ui.add_space(8.0);
        }

        ui.label("Backend");
        let prev_backend = self.backend;
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.backend, Backend::Lowlevel, "lowlevel");
            #[cfg(feature = "interception-backend")]
            ui.radio_value(&mut self.backend, Backend::Interception, "interception");
        });
        ui.label(
            egui::RichText::new(match self.backend {
                Backend::Lowlevel => {
                    "User-mode WH_MOUSE_LL hook. AC-compatible, brief snap on crossings."
                }
                #[cfg(feature = "interception-backend")]
                Backend::Interception => {
                    "Kernel driver. No snap artifact, blocked by kernel anti-cheats."
                }
            })
            .small()
            .weak(),
        );
        if self.backend != prev_backend {
            self.save_backend();
        }

        ui.add_space(12.0);
        ui.label("Default monitor diagonal (inches)");
        let prev_size = self.default_size_in;
        ui.add(
            egui::DragValue::new(&mut self.default_size_in)
                .range(10.0..=80.0)
                .speed(0.1)
                .suffix(" in"),
        );
        ui.label(
            egui::RichText::new("Used when a monitor has no per-device override.")
                .small()
                .weak(),
        );
        if (self.default_size_in - prev_size).abs() > f32::EPSILON {
            self.save_default_size();
        }

        ui.add_space(12.0);
        let prev_autostart = self.autostart_enabled;
        ui.checkbox(&mut self.autostart_enabled, "Auto-start at login");
        ui.label(
            egui::RichText::new(
                "Registers a Task Scheduler entry that launches RustCursor elevated at logon.",
            )
            .small()
            .weak(),
        );
        if self.autostart_enabled != prev_autostart {
            self.apply_autostart(self.autostart_enabled);
        }

        if let Some(err) = &self.last_error {
            ui.add_space(12.0);
            ui.colored_label(egui::Color32::from_rgb(220, 90, 90), err);
        } else if let Some(status) = &self.last_status {
            ui.add_space(12.0);
            ui.colored_label(egui::Color32::from_rgb(120, 180, 120), status);
        }
    }

    fn needs_restart(&self) -> bool {
        // Only backend changes require a restart; size edits hot-reload via
        // `reload_active_profile` in `save_default_size`.
        self.backend != self.initial_backend
    }

    fn save_backend(&mut self) {
        let backend = self.backend;
        if let Err(e) = self.write_with(|doc| doc.set_backend(backend)) {
            self.last_error = Some(e);
        }
    }

    fn save_default_size(&mut self) {
        let inches = self.default_size_in;
        match self.write_with(|doc| doc.set_default_size_in(inches)) {
            Ok(()) => {
                // Default size affects any monitor without an override; hot-swap
                // the SIZES lookup and rebuild the monitor map so the change
                // surfaces without a restart. Reset the banner baseline so it
                // disappears once the running process matches the saved value.
                crate::gui::reload_active_profile();
                self.initial_default_size_in = self.default_size_in;
            }
            Err(e) => self.last_error = Some(e),
        }
    }

    fn write_with<F: FnOnce(&mut ConfigDoc)>(&mut self, f: F) -> Result<(), String> {
        let mut doc = ConfigDoc::load()?;
        f(&mut doc);
        doc.save()?;
        self.last_error = None;
        Ok(())
    }

    fn apply_autostart(&mut self, enable: bool) {
        let result = if enable {
            autostart::enable()
        } else {
            autostart::disable()
        };
        match result {
            Ok(()) => {
                self.last_error = None;
                self.last_status = Some(if enable {
                    "Auto-start enabled.".into()
                } else {
                    "Auto-start disabled.".into()
                });
            }
            Err(e) => {
                // Revert the toggle so the UI reflects the actual on-disk state.
                self.autostart_enabled = !enable;
                self.last_status = None;
                self.last_error = Some(e);
            }
        }
    }
}

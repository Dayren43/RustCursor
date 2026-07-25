//! Log tab: live tail of `%LOCALAPPDATA%\RustCursor\cursor_log.txt`.
//!
//! Reads the whole file on each refresh, trims to the last `MAX_LINES` so a
//! long-running session doesn't blow up the GUI's memory, and renders into a
//! read-only monospace `TextEdit`. `ScrollArea::stick_to_bottom` keeps the
//! view pinned to the newest line unless the user has scrolled up, matching
//! the "live tail" convention.
//!
//! Auto-refresh schedules a repaint every 500 ms via
//! `ctx.request_repaint_after`, so cursor crossings show up without manually
//! clicking Refresh. Open-in-editor shells out via the same `ShellExecuteW`
//! pattern the tray uses for `config.toml`.

use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::Duration;

use eframe::egui;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

const MAX_LINES: usize = 2000;

pub struct LogTab {
    content: String,
    auto_refresh: bool,
    last_error: Option<String>,
}

impl LogTab {
    pub fn new() -> Self {
        let mut tab = Self {
            content: String::new(),
            auto_refresh: false,
            last_error: None,
        };
        tab.refresh();
        tab
    }

    fn refresh(&mut self) {
        let Some(path) = log_path() else {
            self.last_error = Some("LOCALAPPDATA is not set".to_string());
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => {
                let lines: Vec<&str> = s.lines().collect();
                let start = lines.len().saturating_sub(MAX_LINES);
                self.content = lines[start..].join("\n");
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("read {}: {}", path.display(), e));
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Log");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Open in editor").clicked() {
                    open_log_in_editor();
                }
                if ui.button("Refresh").clicked() {
                    self.refresh();
                }
                ui.checkbox(&mut self.auto_refresh, "Auto-refresh");
            });
        });

        if let Some(p) = log_path() {
            ui.label(egui::RichText::new(p.display().to_string()).small().weak());
        }
        ui.add_space(6.0);

        if let Some(err) = &self.last_error {
            ui.colored_label(egui::Color32::from_rgb(220, 90, 90), err);
            return;
        }

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.content)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
            });

        if self.auto_refresh {
            self.refresh();
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
    }
}

fn log_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|s| PathBuf::from(s).join("RustCursor").join("cursor_log.txt"))
}

fn open_log_in_editor() {
    let Some(path) = log_path() else {
        return;
    };
    let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(path_wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

// No `#[cfg(feature = "log")]` needed: this whole module is already gated on it
// in `tabs/mod.rs`, which is the point of keeping the impl here.
impl crate::gui::tabs::SettingsTab for LogTab {
    fn title(&self) -> &'static str {
        "Log"
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        self.ui(ui);
    }
}

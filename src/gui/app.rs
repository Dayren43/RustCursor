//! eframe application running in the Settings subprocess. The subprocess
//! exists only while the window does — closing the X ends `run_native`, the
//! process exits, and all its memory + driver threads are reclaimed.

use eframe::egui;

use crate::gui::tabs::bypass::BypassTab;
use crate::gui::tabs::general::GeneralTab;
use crate::gui::tabs::log::LogTab;
use crate::gui::tabs::monitors::MonitorsTab;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Tab {
    #[default]
    General,
    Monitors,
    Bypass,
    Log,
}

struct SettingsApp {
    tab: Tab,
    general: GeneralTab,
    monitors: MonitorsTab,
    bypass: BypassTab,
    log: LogTab,
}

impl Default for SettingsApp {
    fn default() -> Self {
        Self {
            tab: Tab::default(),
            general: GeneralTab::new(),
            monitors: MonitorsTab::new(),
            bypass: BypassTab::new(),
            log: LogTab::new(),
        }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Config-load warnings (salvaged fields, unreadable file). Refreshed
        // by every `Config::load`, so the banner clears once a save or tab
        // refresh observes a clean parse.
        let warnings = rust_cursor::config::load_warnings();
        if !warnings.is_empty() {
            egui::TopBottomPanel::top("config_warnings").show(ctx, |ui| {
                ui.add_space(4.0);
                for w in &warnings {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 170, 80),
                        format!("config.toml: {w}"),
                    );
                }
                ui.label(
                    egui::RichText::new(
                        "Affected settings fall back to defaults until the value is fixed here or in config.toml.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(4.0);
            });
        }

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::General, "General");
                ui.selectable_value(&mut self.tab, Tab::Monitors, "Monitors");
                ui.selectable_value(&mut self.tab, Tab::Bypass, "Bypass");
                ui.selectable_value(&mut self.tab, Tab::Log, "Log");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::General => self.general.ui(ui),
            Tab::Monitors => self.monitors.ui(ui),
            Tab::Bypass => self.bypass.ui(ui),
            Tab::Log => self.log.ui(ui),
        });
    }
}

pub fn run() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 420.0])
            .with_min_inner_size([400.0, 300.0])
            .with_title(crate::gui::SETTINGS_WINDOW_TITLE)
            .with_icon(load_window_icon()),
        ..Default::default()
    };
    let _ = eframe::run_native(
        crate::gui::SETTINGS_WINDOW_TITLE,
        options,
        Box::new(|_cc| Ok(Box::<SettingsApp>::default())),
    );
}

/// Decode the bundled PNG into the icon shown in the window's title bar and
/// Windows taskbar entry. Same asset the tray icon uses, embedded at compile
/// time so the exe is self-contained.
fn load_window_icon() -> egui::IconData {
    const ICON_PNG: &[u8] = include_bytes!("../../assets/rustcursor-icon.png");
    let img = image::load_from_memory(ICON_PNG)
        .expect("decode window icon PNG")
        .into_rgba8();
    let (w, h) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    }
}

//! eframe application: tab dispatch and the per-frame UI. Tabs are stubs for
//! now; each one will move into its own module under `gui/tabs/` as they grow.

use eframe::egui;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Tab {
    #[default]
    General,
    Monitors,
    Bypass,
    Log,
}

#[derive(Default)]
struct SettingsApp {
    tab: Tab,
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::General, "General");
                ui.selectable_value(&mut self.tab, Tab::Monitors, "Monitors");
                ui.selectable_value(&mut self.tab, Tab::Bypass, "Bypass");
                ui.selectable_value(&mut self.tab, Tab::Log, "Log");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::General => {
                ui.heading("General");
                ui.label("Backend, auto-start, and physical-size defaults go here.");
            }
            Tab::Monitors => {
                ui.heading("Monitors");
                ui.label("Live monitor list with per-device size_in overrides.");
            }
            Tab::Bypass => {
                ui.heading("Bypass");
                ui.label("Foreground processes that pause remapping while focused.");
            }
            Tab::Log => {
                ui.heading("Log");
                ui.label("Tail of %LOCALAPPDATA%\\RustCursor\\cursor_log.txt.");
            }
        });
    }
}

pub fn run() {
    use winit::platform::windows::EventLoopBuilderExtWindows;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 420.0])
            .with_min_inner_size([400.0, 300.0])
            .with_title("RustCursor Settings"),
        // winit defaults to main-thread-only; we run the window on a worker
        // thread spawned from the tray, so opt into any-thread construction.
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "RustCursor Settings",
        options,
        Box::new(|_cc| Ok(Box::<SettingsApp>::default())),
    );
}

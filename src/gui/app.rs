//! eframe application: HWND publication, close intercept, and tab dispatch.

use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::gui::tabs::bypass::BypassTab;
use crate::gui::tabs::general::GeneralTab;
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
    /// True once we've published the HWND to `gui::HWND_PTR`. Done on the
    /// first frame because `eframe::Frame` only exposes a window handle
    /// after the OS window has been created.
    hwnd_published: bool,
}

impl Default for SettingsApp {
    fn default() -> Self {
        Self {
            tab: Tab::default(),
            general: GeneralTab::new(),
            monitors: MonitorsTab::new(),
            bypass: BypassTab::new(),
            hwnd_published: false,
        }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.hwnd_published {
            if let Ok(wh) = frame.window_handle() {
                if let RawWindowHandle::Win32(w32) = wh.as_raw() {
                    crate::gui::set_hwnd(w32.hwnd.get());
                    self.hwnd_published = true;
                }
            }
        }

        // Intercept the close button: cancel the close so winit's EventLoop
        // stays alive, then hide via Win32. Re-opening from the tray uses
        // ShowWindow(SW_SHOW) on the same HWND.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            crate::gui::hide_window();
            return;
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
            .with_title("RustCursor Settings")
            .with_icon(load_window_icon()),
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

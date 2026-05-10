//! Cursor monitor-transition remapper using the Interception kernel driver.
//!
//! When the cursor crosses between monitors of different resolutions, the Y position is
//! remapped to the equivalent physical height on the destination monitor. Both monitors
//! are assumed to be the same physical diagonal size but may differ in resolution.
//!
//! Architecture:
//!  - `platform::windows` handles monitor enumeration, DPI setup, and the Interception
//!    event loop. A future `platform::linux` would implement the same surface.
//!  - `RustCursor::remapper` contains the platform-agnostic crossing logic.
//!  - `RustCursor::core` contains the Monitor struct and physical↔pixel mapping math.
//!
//! Prerequisites:
//!   Install the Interception driver (run as administrator, then reboot):
//!     install-interception.exe /install  — https://github.com/oblitum/Interception/releases

#![windows_subsystem = "windows"]

mod platform;

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, TranslateMessage,
};

fn main() {
    platform::windows::setup_dpi_awareness();
    let monitors = platform::windows::build_monitor_map();

    std::thread::spawn(move || {
        platform::windows::run_event_loop(monitors);
    });

    let menu = Menu::new();
    let quit_item = MenuItem::new("Quit RustCursor", true, None);
    menu.append(&quit_item).expect("append quit menu item");

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("RustCursor")
        .with_icon(build_tray_icon())
        .build()
        .expect("build tray icon");

    let quit_id = quit_item.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            unsafe { PostQuitMessage(0) };
        }
    }));

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Decode the bundled PNG asset into a tray icon. The PNG is embedded at compile
/// time so the exe is self-contained — no asset path lookup at runtime.
fn build_tray_icon() -> Icon {
    const ICON_PNG: &[u8] = include_bytes!("../assets/rustcursor-icon.png");
    let img = image::load_from_memory(ICON_PNG)
        .expect("decode tray icon PNG")
        .into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).expect("icon from rgba")
}

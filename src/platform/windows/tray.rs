//! System-tray UI: Settings and Quit menu items plus the Win32 message pump
//! that drives them. The tray icon's lifetime equals the duration of
//! `run_tray_loop`: dropping the binding hides the icon, so we hold it for as
//! long as the pump runs.

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, TranslateMessage,
};

/// Build the tray icon, register menu handlers, and pump messages until Quit is
/// selected. Must run on the thread that owns the process message queue
/// (the main thread). `backend` is shown as an inert status line so the
/// user can tell at a glance which backend is active.
pub fn run_tray_loop(backend: rust_cursor::config::Backend) {
    let backend_label = match backend {
        #[cfg(feature = "interception-backend")]
        rust_cursor::config::Backend::Interception => "Backend: interception",
        rust_cursor::config::Backend::Lowlevel => "Backend: lowlevel",
    };
    let menu = Menu::new();
    let backend_item = MenuItem::new(backend_label, false, None);
    let settings_item = MenuItem::new("Settings…", true, None);
    let quit_item = MenuItem::new("Quit RustCursor", true, None);
    menu.append(&backend_item).expect("append backend status");
    menu.append(&PredefinedMenuItem::separator())
        .expect("append separator");
    menu.append(&settings_item).expect("append settings item");
    menu.append(&PredefinedMenuItem::separator())
        .expect("append separator");
    menu.append(&quit_item).expect("append quit menu item");

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("RustCursor")
        .with_icon(build_icon())
        .build()
        .expect("build tray icon");

    let settings_id = settings_item.id().clone();
    let quit_id = quit_item.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            unsafe { PostQuitMessage(0) };
        } else if event.id == settings_id {
            crate::gui::spawn_settings_subprocess();
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
/// time so the exe is self-contained; no asset path lookup at runtime.
fn build_icon() -> Icon {
    const ICON_PNG: &[u8] = include_bytes!("../../../assets/rustcursor-icon.png");
    let img = image::load_from_memory(ICON_PNG)
        .expect("decode tray icon PNG")
        .into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).expect("icon from rgba")
}

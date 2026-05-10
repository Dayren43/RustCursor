//! System-tray UI: a single Quit menu item plus the Win32 message pump that
//! drives it. The tray icon's lifetime equals the duration of `run_tray_loop` —
//! dropping the binding hides the icon, so we hold it for as long as the pump runs.

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, TranslateMessage,
};

/// Build the tray icon, register the Quit handler, and pump messages until Quit
/// is selected. Must run on the thread that owns the process message queue —
/// i.e. the main thread.
pub fn run_tray_loop() {
    let menu = Menu::new();
    let quit_item = MenuItem::new("Quit RustCursor", true, None);
    menu.append(&quit_item).expect("append quit menu item");

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("RustCursor")
        .with_icon(build_icon())
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
fn build_icon() -> Icon {
    const ICON_PNG: &[u8] = include_bytes!("../../../assets/rustcursor-icon.png");
    let img = image::load_from_memory(ICON_PNG)
        .expect("decode tray icon PNG")
        .into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).expect("icon from rgba")
}

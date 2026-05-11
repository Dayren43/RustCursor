//! System-tray UI: Edit-bypass and Quit menu items plus the Win32 message pump
//! that drives them. The tray icon's lifetime equals the duration of
//! `run_tray_loop`: dropping the binding hides the icon, so we hold it for as
//! long as the pump runs.

use std::os::windows::ffi::OsStrExt;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, SW_SHOWNORMAL, TranslateMessage,
};
use windows::core::{PCWSTR, w};

/// Build the tray icon, register menu handlers, and pump messages until Quit is
/// selected. Must run on the thread that owns the process message queue
/// (the main thread). `backend` is shown as an inert status line so the
/// user can tell at a glance which backend is active.
pub fn run_tray_loop(backend: rust_cursor::config::Backend) {
    let backend_label = match backend {
        rust_cursor::config::Backend::Interception => "Backend: interception",
        rust_cursor::config::Backend::Lowlevel => "Backend: lowlevel",
    };
    let menu = Menu::new();
    let backend_item = MenuItem::new(backend_label, false, None);
    let edit_item = MenuItem::new("Edit bypass list…", true, None);
    let quit_item = MenuItem::new("Quit RustCursor", true, None);
    menu.append(&backend_item).expect("append backend status");
    menu.append(&PredefinedMenuItem::separator())
        .expect("append separator");
    menu.append(&edit_item).expect("append edit menu item");
    menu.append(&PredefinedMenuItem::separator())
        .expect("append separator");
    menu.append(&quit_item).expect("append quit menu item");

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("RustCursor")
        .with_icon(build_icon())
        .build()
        .expect("build tray icon");

    let edit_id = edit_item.id().clone();
    let quit_id = quit_item.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            unsafe { PostQuitMessage(0) };
        } else if event.id == edit_id {
            open_config_in_editor();
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

/// Open `config.toml` in the system's default associated editor for `.toml`
/// files (or Notepad if no association is set). Errors are swallowed; the
/// menu click is best-effort.
fn open_config_in_editor() {
    let Some(path) = rust_cursor::config::path() else {
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

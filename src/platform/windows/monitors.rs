use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows::{
    Win32::{
        Foundation::{LPARAM, RECT},
        Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, MONITORINFOEXW},
    },
    core::BOOL,
};

use rust_cursor::core::{Monitor, geometry::Rect};

struct MonitorInfo {
    name: String,
    rect: RECT,
}

unsafe extern "system" fn monitor_enum_proc(
    hmonitor: windows::Win32::Graphics::Gdi::HMONITOR,
    _hdc: HDC,
    _lprect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    unsafe {
        let monitors = &mut *(lparam.0 as *mut Vec<MonitorInfo>);

        let mut mi_ex = MONITORINFOEXW::default();
        mi_ex.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

        if GetMonitorInfoW(hmonitor, &mut mi_ex as *mut _ as *mut _) == false {
            return true.into();
        }

        let len = mi_ex
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(mi_ex.szDevice.len());

        let name = OsString::from_wide(&mi_ex.szDevice[..len])
            .to_string_lossy()
            .into_owned();

        monitors.push(MonitorInfo {
            name,
            rect: mi_ex.monitorInfo.rcMonitor,
        });
        true.into()
    }
}

fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mut monitors: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            Some(HDC::default()),
            None,
            Some(monitor_enum_proc),
            LPARAM(&mut monitors as *mut _ as isize),
        );
    }
    monitors
}

/// Declare PER_MONITOR_AWARE_V2 so all coordinate APIs use physical pixels —
/// consistent with the raw device counts Interception delivers.
pub fn setup_dpi_awareness() {
    unsafe {
        use windows::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Enumerate all connected monitors and build the monitor map used by the remapper.
pub fn build_monitor_map() -> HashMap<String, Monitor> {
    let mut map = HashMap::new();
    for m in enumerate_monitors() {
        let physical_size_in: f64 = 27.0; // TODO: read from EDID or config
        let pixels_w = (m.rect.right - m.rect.left) as u32;
        let pixels_h = (m.rect.bottom - m.rect.top) as u32;
        let aspect_ratio = pixels_w as f32 / pixels_h as f32;
        let dpi = ((pixels_w.pow(2) + pixels_h.pow(2)) as f64).sqrt() / physical_size_in;
        map.insert(
            m.name.clone(),
            Monitor {
                identifier: m.name.clone(),
                bounds: Rect {
                    x: m.rect.left as f32,
                    y: m.rect.top as f32,
                    w: (m.rect.right - m.rect.left) as f32,
                    h: (m.rect.bottom - m.rect.top) as f32,
                },
                aspect_ratio,
                resolution: (pixels_w, pixels_h),
                dpi,
                physical_size_in: physical_size_in as f32,
            },
        );
    }
    map
}

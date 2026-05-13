use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows::{
    Win32::{
        Foundation::{LPARAM, RECT},
        Graphics::Gdi::{
            DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC,
            MONITORINFOEXW,
        },
    },
    core::{BOOL, PCWSTR},
};

use rust_cursor::core::{Monitor, geometry::Point, geometry::Rect};

struct MonitorInfo {
    /// Windows OS slot name, e.g. `\\.\DISPLAY1`.
    name: String,
    /// Stable hardware ID for this physical panel, e.g. `MONITOR\DEL41B7`.
    /// `None` when the slot is empty or the OS does not report a child device
    /// for the adapter.
    hwid: Option<String>,
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

        let hwid = monitor_hwid(&name);

        monitors.push(MonitorInfo {
            name,
            hwid,
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

/// Query the hardware ID of the panel attached to `adapter` (e.g.
/// `\\.\DISPLAY1`). Returns a prefix-stable identifier built from the EDID
/// manufacturer + product code, e.g. `MONITOR\DEL41B7`. This survives cable
/// swaps, port changes, and driver reinstalls, but two identical models
/// (same make + model number) share the same ID; see README for the planned
/// EDID-serial extension that would disambiguate them.
fn monitor_hwid(adapter: &str) -> Option<String> {
    let adapter_w: Vec<u16> = adapter.encode_utf16().chain([0]).collect();
    let mut child = DISPLAY_DEVICEW {
        cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    let ok = unsafe { EnumDisplayDevicesW(PCWSTR(adapter_w.as_ptr()), 0, &mut child, 0) };
    if !ok.as_bool() {
        return None;
    }
    let len = child
        .DeviceID
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(child.DeviceID.len());
    let id = OsString::from_wide(&child.DeviceID[..len])
        .to_string_lossy()
        .into_owned();
    parse_hwid_prefix(&id)
}

/// Extract `MONITOR\MMMPPPP` from a Windows device instance ID like
/// `MONITOR\DEL41B7\{4d36e96e-...}\0000`.
fn parse_hwid_prefix(device_id: &str) -> Option<String> {
    let mut parts = device_id.split('\\');
    let prefix = parts.next()?;
    let model = parts.next()?;
    if prefix.eq_ignore_ascii_case("MONITOR") && !model.is_empty() {
        Some(format!("{prefix}\\{model}"))
    } else {
        None
    }
}

/// Declare PER_MONITOR_AWARE_V2 so all coordinate APIs use physical pixels,
/// consistent with the raw device counts Interception delivers.
pub fn setup_dpi_awareness() {
    unsafe {
        use windows::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Connected HWIDs in stable sorted order. Used at startup to pick the
/// active `[[profile]]` from `config.toml`.
pub fn enumerate_hwids() -> Vec<String> {
    let mut hwids: Vec<String> = enumerate_monitors()
        .into_iter()
        .filter_map(|m| m.hwid)
        .collect();
    hwids.sort();
    hwids
}

/// Enumerate all connected monitors and build the monitor map used by the
/// remapper. Physical sizes come from `config::size_for(device, hwid)`: the
/// active profile's per-HWID entry takes precedence, falling back to a
/// legacy device-keyed `[[monitor]]` entry, then to `default_size_in`.
///
/// Default `position_mm` is seeded by walking monitors in OS-x order and
/// accumulating physical widths so they touch left-to-right with y=0. This
/// preserves today's behaviour (same physical y origin across monitors) for
/// any setup where the user hasn't pinned positions in the Settings GUI.
/// Profile overrides from `config::position_for` win when present.
pub fn build_monitor_map() -> HashMap<String, Monitor> {
    struct Tmp {
        info: MonitorInfo,
        size_in: f32,
        w_mm: f32,
    }

    let mut tmps: Vec<Tmp> = enumerate_monitors()
        .into_iter()
        .map(|m| {
            let size_in = rust_cursor::config::size_for(m.hwid.as_deref());
            let w_px = (m.rect.right - m.rect.left) as f32;
            let h_px = (m.rect.bottom - m.rect.top) as f32;
            let aspect = if h_px.abs() < f32::EPSILON {
                16.0 / 9.0
            } else {
                w_px / h_px
            };
            let diag_mm = size_in * 25.4;
            let h_mm = diag_mm / ((aspect * aspect + 1.0).sqrt());
            let w_mm = h_mm * aspect;
            Tmp {
                info: m,
                size_in,
                w_mm,
            }
        })
        .collect();
    tmps.sort_by_key(|t| t.info.rect.left);

    let mut defaults: HashMap<String, (f32, f32)> = HashMap::new();
    let mut cum_x_mm = 0.0_f32;
    for t in &tmps {
        defaults.insert(t.info.name.clone(), (cum_x_mm, 0.0));
        cum_x_mm += t.w_mm;
    }

    let mut map = HashMap::new();
    for t in tmps {
        let pos = rust_cursor::config::position_for(t.info.hwid.as_deref())
            .unwrap_or_else(|| defaults[&t.info.name]);
        let pixels_w = (t.info.rect.right - t.info.rect.left) as u32;
        let pixels_h = (t.info.rect.bottom - t.info.rect.top) as u32;
        let aspect_ratio = pixels_w as f32 / pixels_h as f32;
        let dpi = ((pixels_w.pow(2) + pixels_h.pow(2)) as f64).sqrt() / t.size_in as f64;
        map.insert(
            t.info.name.clone(),
            Monitor {
                identifier: t.info.name.clone(),
                hwid: t.info.hwid,
                bounds: Rect {
                    x: t.info.rect.left as f32,
                    y: t.info.rect.top as f32,
                    w: (t.info.rect.right - t.info.rect.left) as f32,
                    h: (t.info.rect.bottom - t.info.rect.top) as f32,
                },
                position_mm: Point { x: pos.0, y: pos.1 },
                aspect_ratio,
                resolution: (pixels_w, pixels_h),
                dpi,
                physical_size_in: t.size_in,
            },
        );
    }
    map
}

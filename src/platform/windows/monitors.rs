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

/// Returns true when the OS arrangement is predominantly a vertical stack:
/// more monitor pairs are separated along y (disjoint y-projections) than
/// along x. A side-by-side pair separates along x, a stacked pair along y;
/// a diagonal pair separates along both and contributes to neither side of
/// the comparison. Ties (single monitor, L-shapes, grids) report false so
/// those layouts keep the historical horizontal seeding.
fn is_vertical_stack(rects: &[RECT]) -> bool {
    let mut horizontal_pairs = 0u32;
    let mut vertical_pairs = 0u32;
    for (i, a) in rects.iter().enumerate() {
        for b in &rects[i + 1..] {
            if a.right <= b.left || b.right <= a.left {
                horizontal_pairs += 1;
            }
            if a.bottom <= b.top || b.bottom <= a.top {
                vertical_pairs += 1;
            }
        }
    }
    vertical_pairs > horizontal_pairs
}

/// Enumerate all connected monitors and build the monitor map used by the
/// remapper. Physical sizes come from `config::size_for(device, hwid)`: the
/// active profile's per-HWID entry takes precedence, falling back to a
/// legacy device-keyed `[[monitor]]` entry, then to `default_size_in`.
///
/// Default `position_mm` is seeded by a cumulative walk along the dominant
/// axis of the OS arrangement: side-by-side layouts walk OS-x order
/// accumulating physical widths (touching left-to-right, y=0), vertical
/// stacks walk OS-y order accumulating physical heights (touching
/// top-to-bottom, x=0). Either way both monitors share the cross-axis
/// origin until the user pins real positions in the Settings GUI.
/// Profile overrides from `config::position_for` win when present.
pub fn build_monitor_map() -> HashMap<String, Monitor> {
    struct Tmp {
        info: MonitorInfo,
        size_in: f32,
        w_mm: f32,
        h_mm: f32,
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
                h_mm,
            }
        })
        .collect();

    let rects: Vec<RECT> = tmps.iter().map(|t| t.info.rect).collect();
    let mut defaults: HashMap<String, (f32, f32)> = HashMap::new();
    if is_vertical_stack(&rects) {
        tmps.sort_by_key(|t| t.info.rect.top);
        let mut cum_y_mm = 0.0_f32;
        for t in &tmps {
            defaults.insert(t.info.name.clone(), (0.0, cum_y_mm));
            cum_y_mm += t.h_mm;
        }
    } else {
        tmps.sort_by_key(|t| t.info.rect.left);
        let mut cum_x_mm = 0.0_f32;
        for t in &tmps {
            defaults.insert(t.info.name.clone(), (cum_x_mm, 0.0));
            cum_x_mm += t.w_mm;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn single_monitor_is_not_a_stack() {
        assert!(!is_vertical_stack(&[rect(0, 0, 2560, 1440)]));
    }

    #[test]
    fn side_by_side_pair_is_not_a_stack() {
        assert!(!is_vertical_stack(&[
            rect(0, 0, 2560, 1440),
            rect(2560, 120, 4480, 1200),
        ]));
    }

    #[test]
    fn stacked_pair_is_a_stack() {
        assert!(is_vertical_stack(&[
            rect(0, 0, 2560, 1440),
            rect(320, 1440, 2240, 2520),
        ]));
    }

    #[test]
    fn three_monitor_stack_is_a_stack() {
        assert!(is_vertical_stack(&[
            rect(0, 0, 2560, 1440),
            rect(0, 1440, 2560, 2880),
            rect(0, 2880, 1920, 3960),
        ]));
    }

    #[test]
    fn diagonal_pair_ties_to_horizontal() {
        // Disjoint along both axes: counts as evidence for both, so the
        // tie keeps the horizontal default.
        assert!(!is_vertical_stack(&[
            rect(0, 0, 2560, 1440),
            rect(2560, 1440, 4480, 2520),
        ]));
    }

    #[test]
    fn l_shape_ties_to_horizontal() {
        // Two side-by-side plus one above the left. The top-left vs
        // bottom-right pair is disjoint on both axes and scores on both
        // sides, leaving a 2-2 tie that keeps the horizontal default.
        assert!(!is_vertical_stack(&[
            rect(0, 0, 2560, 1440),
            rect(2560, 0, 5120, 1440),
            rect(0, -1440, 2560, 0),
        ]));
    }
}

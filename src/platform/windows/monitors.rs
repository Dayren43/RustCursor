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

/// One monitor's OS rect paired with its physical size in millimetres: the
/// input to default position seeding.
struct Panel {
    rect: RECT,
    w_mm: f32,
    h_mm: f32,
}

impl Panel {
    fn mm_per_px_x(&self) -> f32 {
        // A whole pixel count, so this is the degenerate zero-width case, not
        // a float-comparison epsilon.
        let px = (self.rect.right - self.rect.left) as f32;
        if px == 0.0 { 0.0 } else { self.w_mm / px }
    }

    fn mm_per_px_y(&self) -> f32 {
        let px = (self.rect.bottom - self.rect.top) as f32;
        if px == 0.0 { 0.0 } else { self.h_mm / px }
    }
}

/// Seed a default `position_mm` per panel from the OS arrangement, returned
/// in input order.
///
/// The dominant axis (x for side-by-side layouts, y for vertical stacks) is a
/// cumulative walk in OS order accumulating physical widths or heights, so the
/// seeded monitors touch regardless of any OS-space gap or overlap. The cross
/// axis keeps the OS offset the user already arranged, converted to
/// millimetres through each panel's own px/mm ratio and measured from the
/// arrangement's top or left edge. A narrower panel centred under a wider one
/// therefore stays centred instead of collapsing to a shared origin.
fn seed_positions(panels: &[Panel]) -> Vec<(f32, f32)> {
    let rects: Vec<RECT> = panels.iter().map(|p| p.rect).collect();
    let mut order: Vec<usize> = (0..panels.len()).collect();
    let mut out = vec![(0.0_f32, 0.0_f32); panels.len()];
    if is_vertical_stack(&rects) {
        order.sort_by_key(|&i| panels[i].rect.top);
        let origin_x = rects.iter().map(|r| r.left).min().unwrap_or(0);
        let mut cum_y_mm = 0.0_f32;
        for &i in &order {
            let p = &panels[i];
            out[i] = ((p.rect.left - origin_x) as f32 * p.mm_per_px_x(), cum_y_mm);
            cum_y_mm += p.h_mm;
        }
    } else {
        order.sort_by_key(|&i| panels[i].rect.left);
        let origin_y = rects.iter().map(|r| r.top).min().unwrap_or(0);
        let mut cum_x_mm = 0.0_f32;
        for &i in &order {
            let p = &panels[i];
            out[i] = (cum_x_mm, (p.rect.top - origin_y) as f32 * p.mm_per_px_y());
            cum_x_mm += p.w_mm;
        }
    }
    out
}

/// Enumerate all connected monitors and build the monitor map used by the
/// remapper. Physical sizes come from `config::size_for(device, hwid)`: the
/// active profile's per-HWID entry takes precedence, falling back to a
/// legacy device-keyed `[[monitor]]` entry, then to `default_size_in`.
///
/// Default `position_mm` comes from [`seed_positions`]: the monitors are made
/// to touch along the arrangement's dominant axis and keep their OS offset on
/// the cross axis, until the user pins real positions in the Settings GUI.
/// Profile overrides from `config::position_for` win when present.
pub fn build_monitor_map() -> HashMap<String, Monitor> {
    struct Tmp {
        info: MonitorInfo,
        size_in: f32,
        w_mm: f32,
        h_mm: f32,
    }

    let tmps: Vec<Tmp> = enumerate_monitors()
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

    let panels: Vec<Panel> = tmps
        .iter()
        .map(|t| Panel {
            rect: t.info.rect,
            w_mm: t.w_mm,
            h_mm: t.h_mm,
        })
        .collect();
    let defaults = seed_positions(&panels);

    let mut map = HashMap::new();
    for (i, t) in tmps.into_iter().enumerate() {
        let pos = rust_cursor::config::position_for(t.info.hwid.as_deref()).unwrap_or(defaults[i]);
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

    fn panel(rect: RECT, w_mm: f32, h_mm: f32) -> Panel {
        Panel { rect, w_mm, h_mm }
    }

    #[test]
    fn lone_monitor_seeds_at_the_origin() {
        let seeded = seed_positions(&[panel(rect(0, 0, 2560, 1440), 600.0, 337.5)]);
        assert_eq!(seeded, vec![(0.0, 0.0)]);
    }

    #[test]
    fn stack_keeps_the_os_horizontal_offset() {
        // 1920-wide panel centred under a 2560-wide one: 320 px of OS offset
        // at 0.25 mm/px is 80 mm of physical offset, not a shared origin.
        let seeded = seed_positions(&[
            panel(rect(0, 0, 2560, 1440), 600.0, 337.5),
            panel(rect(320, 1440, 2240, 2520), 480.0, 270.0),
        ]);
        assert_eq!(seeded, vec![(0.0, 0.0), (80.0, 337.5)]);
    }

    #[test]
    fn side_by_side_keeps_the_os_vertical_offset() {
        let seeded = seed_positions(&[
            panel(rect(0, 0, 2560, 1440), 600.0, 337.5),
            panel(rect(2560, 120, 4480, 1200), 480.0, 270.0),
        ]);
        assert_eq!(seeded, vec![(0.0, 0.0), (600.0, 30.0)]);
    }

    #[test]
    fn seeding_is_independent_of_input_order_and_sign() {
        // Bottom panel listed first, arrangement extending left of x=0: the
        // cross axis is measured from the leftmost edge, and results come
        // back in input order.
        let seeded = seed_positions(&[
            panel(rect(-320, 1440, 1600, 2520), 480.0, 270.0),
            panel(rect(0, 0, 2560, 1440), 600.0, 337.5),
        ]);
        assert_eq!(seeded, vec![(0.0, 337.5), (75.0, 0.0)]);
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

# RustCursor

Seamlessly moves the cursor between monitors of different resolutions, physical sizes, or mount positions by remapping through a shared physical-millimetre coordinate space. Built for a 1080p + 1440p pair of 27" displays side by side, and tunable to any layout from the Settings GUI.

## The problem

Windows places the cursor at the raw pixel position when crossing monitor boundaries. Different pixel densities, OS y-offsets, and physical mount positions all conspire to make the cursor jump unpredictably. At the vertical edges where one monitor is taller in pixels than the other, Windows creates "gap zones" where the cursor can get trapped or snap erratically.

## How it works

- Intercepts mouse strokes before they reach the foreground window. Two strategies are available; see [Backends](#backends) below.
- Translates the source pixel position into a shared physical millimetre coordinate space using the monitor's diagonal, aspect ratio, and its `position_mm` (the physical top-left in shared world coords).
- Maps the cursor's world position onto the destination monitor so crossings preserve physical height (or physical x, for vertical crossings) regardless of pixel density.
- Blocks crossings when the destination doesn't physically exist at the source's world position (e.g. portrait next to landscape, where the landscape doesn't cover the portrait's top). The cursor slides along the source's edge instead of jumping to the nearest valid pixel.

## Prerequisites

Rust toolchain (stable, MSVC target):
```
rustup target add x86_64-pc-windows-msvc
```

The default `lowlevel` backend requires no external dependencies. For the `interception` backend, additionally install the [Interception driver](https://github.com/oblitum/Interception/releases) (run as administrator, then reboot):
```
install-interception.exe /install
```

## Build and run

Release build, and what the published exe is built with (lowlevel backend only, no external dependencies, no per-stroke logging):
```
cargo build --release --no-default-features
.\target\release\RustCursor.exe
```

Development build. The default `log` feature adds per-stroke diagnostics to `%LOCALAPPDATA%\RustCursor\cursor_log.txt` and the Settings **Log** tab that tails it, at the cost of file I/O and process-basename resolution on the input hot path:
```
cargo build --release
.\target\release\RustCursor.exe
```

To include the interception backend (requires the driver and DLL, see [Backends](#backends)):
```
cargo build --release --no-default-features --features interception-backend
.\target\release\RustCursor.exe
```

The app runs silently with no console window. A tray icon appears in the notification area; right-click it for **Settings…** and **Quit RustCursor**.

When building with `--features interception-backend`, `interception.dll` must sit next to the exe at runtime. The DLL is not redistributed by the `interception-sys` crate or installed by the driver; it ships in the Interception release ZIP under `library/x64/`. `build.rs` copies it next to the built exe automatically; provide it via either of:

- A copy at `vendor/interception.dll` in the repo root (gitignored), or
- The `INTERCEPTION_DLL` environment variable pointing at the absolute path.

## Settings GUI

Right-clicking the tray and choosing **Settings…** opens a tabbed configuration window. Each click launches a separate short-lived `RustCursor.exe --settings` subprocess, so the GPU-accelerated window's driver overhead is only paid while Settings is open; closing the window exits that subprocess while the tray process keeps running. Edits are signalled back to the parent over a window message, which is why they hot-reload without a restart. Quitting from the tray closes any Settings windows still open.

- **General**: default monitor diagonal, and an auto-start-at-login toggle that wraps `schtasks` (prompts UAC when the running process isn't already elevated). Builds with `--features interception-backend` also get an input backend selector (`lowlevel` / `interception`); with only one backend compiled in there is nothing to select, so it is hidden.
- **Monitors**: drag-to-arrange layout canvas at the top, each monitor drawn at its physical aspect ratio. Edges snap to neighbours within 10 mm; hold **Alt** during a drag to disable snap. A per-monitor numeric diagonal editor sits underneath for typed precision. Size and position edits hot-reload, so cursor crossings reflect changes in real time without a restart.
- **Bypass**: add/remove process basenames whose foreground focus pauses cursor remapping (case-insensitive). Hot-reloads on every edit.
- **Log**: live tail of `cursor_log.txt` with an Auto-refresh toggle (500 ms tick) and an Open-in-editor button. Only present in builds with the `log` feature, since nothing writes the file otherwise.

Backend changes still require a restart because the input loop thread captures the backend at spawn.

## Pausing for fullscreen apps

While focused on a fullscreen DirectX/Vulkan/OpenGL app, RustCursor automatically forwards strokes unchanged so games can keep cursor capture. Detection uses Windows' own `SHQueryUserNotificationState`, the same signal used to suppress toast notifications during gameplay, so F11 browsers, fullscreen video, and PowerPoint slideshows are *not* paused.

For windowed-fullscreen titles that aren't auto-detected, add their executable basename to the bypass list via the Settings GUI's **Bypass** tab. The underlying file is `%LOCALAPPDATA%\RustCursor\config.toml` (created with comments on first run); hand-edits hot-reload, no restart required.

## Backends

Set `backend` in `config.toml`:

- **`lowlevel`** *(default)*: user-mode `WH_MOUSE_LL` hook. No driver needed, compatible with kernel anti-cheats (Vanguard, Javelin, kernel-mode EAC). A brief snap is visible at each monitor crossing.
- **`interception`**: kernel-driver path via [Interception](https://github.com/oblitum/Interception). No snap artifact, but flagged by kernel anti-cheats. Requires the Interception driver to be installed and a build with `--features interception-backend`. The pre-built release exe uses lowlevel only.

Switching backends requires restarting RustCursor. The currently active backend is shown as the first item in the tray menu and on the General tab of the Settings window.

## Monitor physical sizes and layout

The remap math needs each monitor's physical diagonal size to convert pixels to millimetres, plus a `position_mm` for each monitor (the physical top-left in shared world coordinates). A `default_size_in` (inches) applies to every monitor without an explicit override. `position_mm` defaults to a cumulative walk along the dominant axis of the Windows arrangement: side-by-side layouts touch left-to-right with y=0, vertically stacked layouts touch top-to-bottom with x=0. Mixed grids and L-shapes default to the horizontal walk; arrange those (and any layout where the defaults don't match physical reality) in the Monitors tab.

> **Important: the layout must match Windows Display Settings.** RustCursor only
> corrects *where the cursor lands* when it crosses an edge that **Windows already
> has** between your monitors; it does not create crossing edges. Which monitors
> are adjacent, and therefore whether you cross left/right or top/bottom, is set
> entirely by Windows Display Settings. The Monitors-tab canvas (and `position_mm`)
> only describes the *physical* arrangement for the remap math, it does not move
> your monitors in Windows. So to get **vertical (top/bottom) crossings you must
> first stack the monitors vertically in Windows Display Settings**, then mirror
> that arrangement in the Monitors tab. Arranging them vertically only in
> RustCursor while Windows still has them side by side makes the two panels'
> physical heights non-overlapping, so every left/right crossing is blocked and
> the cursor sticks at the edge (and vice-versa for a horizontal `position_mm` on
> a vertically-stacked Windows layout). Keep the two arrangements consistent.

Overrides are stored per **display set** under `[[profile]]`. A profile matches when its `hwids` field (a set of stable monitor IDs) equals the set of monitors currently plugged in, so docking/undocking a laptop or rearranging cables picks the right layout automatically. The Settings GUI's Monitors tab writes profiles for you; the hand-edited form looks like:

```toml
default_size_in = 27.0

[[profile]]
hwids       = ["MONITOR\\DEL41B7", "MONITOR\\GSM5BAF"]
description = "Dell 27 + LG 27"

[[profile.monitor]]
hwid        = "MONITOR\\DEL41B7"
size_in     = 27.0
position_mm = [0.0, 0.0]

[[profile.monitor]]
hwid        = "MONITOR\\GSM5BAF"
size_in     = 24.0
position_mm = [597.5, 30.0]
```

HWIDs are read from the EDID manufacturer + product code via `EnumDisplayDevices`; find yours in the Settings GUI's Monitors tab (each row prints its HWID) or look up the device instance ID under `HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY` in the registry. GUI edits hot-reload; hand-edits to `config.toml` still need a RustCursor restart.

### Identical-model limitation

Two monitors of the exact same make and model share an HWID prefix (e.g. both report `MONITOR\DEL41B7`), so a setup with two identical panels cannot tell them apart by HWID alone. The planned fix is to additionally parse the EDID serial number from `HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY\<MMMPPPP>\<instance>\Device Parameters\EDID` and append it to the HWID string. The on-disk schema does not change when this lands; HWID just becomes more specific.

## Run elevated

RustCursor must run with administrator privileges. Without elevation, `SetCursorPos` calls are silently blocked by Windows UIPI when the target lies over a higher-integrity window (Task Manager, UAC dialogs, some installers), which freezes screen-crossing while those windows have focus. Right-click the exe → **Run as administrator** for ad-hoc runs.

## Auto-start at login

The Settings GUI's General tab has an **Auto-start at login** checkbox that wraps the same Task Scheduler entry shown below; toggling it from a non-elevated session triggers a UAC prompt for the `schtasks` call alone, so you never see UAC at *startup* itself.

For the equivalent manual setup from an elevated PowerShell:

```
schtasks /Create /TN "RustCursor" /SC ONLOGON /RL HIGHEST /TR "<absolute path to RustCursor.exe>" /F
```

Smoke-test without rebooting: `schtasks /Run /TN "RustCursor"`. Remove with `schtasks /Delete /TN "RustCursor" /F` (or untick the checkbox).

## Module layout

```
build.rs                    copies interception.dll next to the built exe (interception-backend feature only)
assets/
  rustcursor-icon.png       embedded in the exe; used for tray + Settings window
  rustcursor-icon.svg       source vector
src/
  main.rs                   entry: --settings dispatch, DPI setup, profile resolution,
                            loop spawn, tray
  lib.rs                    exports config, core, and remapper
  config.rs                 TOML config types, active-profile resolution,
                            runtime SIZES + BYPASS lookups (RwLock for live-reload)
  remapper.rs               platform-agnostic crossing logic, world-coord math, tests
  core/
    mod.rs                  Monitor struct (incl. position_mm) and pixel<->mm mapping
    geometry.rs             Point and Rect types
  gui/
    mod.rs                  settings-subprocess spawn + entry, parent-reload IPC,
                            close_settings_windows (tray-quit cleanup)
    app.rs                  eframe App, tab dispatch, window icon
    autostart.rs            schtasks Create/Delete via ShellExecuteEx + runas
    config_io.rs            toml_edit writer that preserves doc comments
    tabs/
      general.rs            backend radio, default diagonal, auto-start toggle
      monitors.rs           drag-to-arrange canvas + numeric diagonal editor
      bypass.rs             process-list editor (live-reload)
      log.rs                tail of cursor_log.txt with auto-refresh
  platform/
    mod.rs                  cfg-gated platform selection
    windows.rs              Windows platform surface (re-exports)
    windows/
      monitors.rs           enumeration, HWID query, build_monitor_map (+ position seeding)
      interception.rs       Interception kernel-driver backend
      lowlevel.rs           WH_MOUSE_LL userspace backend
      focus.rs              fullscreen-game detect + global BYPASS lookup
      layout.rs             WM_DISPLAYCHANGE listener + trigger_monitor_rebuild
      tray.rs               tray icon, menu (backend status/Settings.../Quit), pump
```

Adding Linux support requires only a new `platform/linux/` implementation of the same public surface re-exported from `platform/windows.rs`. The `gui/` tree is already cross-platform (egui + eframe); only the subprocess-to-parent reload IPC (`FindWindowExW` + `PostMessageW` in `gui/mod.rs`) and the schtasks autostart helper need Windows-specific replacements.

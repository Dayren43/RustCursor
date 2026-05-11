# RustCursor

Seamlessly moves the cursor between two monitors of different resolutions by remapping to the same **physical height** on the destination monitor. Built for a 1080p + 1440p pair of 27" displays side by side, but works for any two monitors with the same physical diagonal.

## The problem

Windows places the cursor at the raw pixel position when crossing monitor boundaries. A 1080p and a 1440p monitor have different pixel densities, so the cursor appears to jump up or down when crossing. At the vertical edges (where one monitor is taller in pixels than the other), Windows creates "gap zones" where the cursor can get trapped or snap erratically.

## How it works

- Intercepts mouse strokes at kernel level using the [Interception driver](https://github.com/oblitum/Interception) — no snap-back race condition possible
- Reads the actual cursor position via `GetCursorPos` each stroke (avoids drift from Windows pointer-speed scaling)
- Converts the source pixel position to physical millimetres using the monitor's diagonal and aspect ratio, then maps to the equivalent mm position on the destination monitor
- Gap zones (OS y-ranges that exist on one monitor but not the other due to y-offset in display settings) are handled by the same physical-space mapping — no snapping to the nearest edge

## Prerequisites

Rust toolchain (stable, MSVC target):
```
rustup target add x86_64-pc-windows-msvc
```

The default `lowlevel` backend requires no external dependencies. For the `interception` backend, additionally install the [Interception driver](https://github.com/oblitum/Interception/releases) — run as administrator, then reboot:
```
install-interception.exe /install
```

## Build and run

```
cargo build --release
.\target\release\RustCursor.exe
```

The app runs silently with no console window. A tray icon appears in the notification area — right-click it and choose **Quit RustCursor** to exit. Per-stroke diagnostics are written to `%LOCALAPPDATA%\RustCursor\cursor_log.txt`.

For the `interception` backend, `interception.dll` must sit next to the exe at runtime. The DLL is not redistributed by the `interception-sys` crate or installed by the driver — it ships in the Interception release ZIP under `library/x64/`. `build.rs` copies it next to the built exe automatically; provide it via either of:

- A copy at `vendor/interception.dll` in the repo root (gitignored), or
- The `INTERCEPTION_DLL` environment variable pointing at the absolute path.

## Pausing for fullscreen apps

While focused on a fullscreen DirectX/Vulkan/OpenGL app, RustCursor automatically forwards strokes unchanged so games can keep cursor capture. Detection uses Windows' own `SHQueryUserNotificationState`, the same signal used to suppress toast notifications during gameplay — so F11 browsers, fullscreen video, and PowerPoint slideshows are *not* paused.

For windowed-fullscreen titles that aren't auto-detected, add their executable basename to the bypass list in `%LOCALAPPDATA%\RustCursor\config.toml` (created with comments on first run). Restart RustCursor to pick up changes. The tray menu's **Edit bypass list…** item opens the file in the default editor.

## Backends

Set `backend` in `config.toml`:

- **`lowlevel`** *(default)* — user-mode `WH_MOUSE_LL` hook (LBM-style). No driver needed, compatible with kernel anti-cheats (Vanguard, Javelin, kernel-mode EAC). A brief snap is visible at each monitor crossing.
- **`interception`** — kernel-driver path via [Interception](https://github.com/oblitum/Interception). No snap artifact, but flagged by kernel anti-cheats. Requires the Interception driver to be installed.

Switching backends requires restarting RustCursor. The currently active backend is shown as the first item in the tray menu.

## Run elevated

RustCursor must run with administrator privileges. Without elevation, `SetCursorPos` calls are silently blocked by Windows UIPI when the target lies over a higher-integrity window (Task Manager, UAC dialogs, some installers), which freezes screen-crossing while those windows have focus. Right-click the exe → **Run as administrator** for ad-hoc runs.

## Auto-start at login

Register a Task Scheduler entry that runs the exe with highest privileges at logon — no UAC prompt at startup. From an elevated PowerShell:

```
schtasks /Create /TN "RustCursor" /SC ONLOGON /RL HIGHEST /TR "<absolute path to RustCursor.exe>" /F
```

Smoke-test without rebooting: `schtasks /Run /TN "RustCursor"`. Remove with `schtasks /Delete /TN "RustCursor" /F`.

## Module layout

```
build.rs                    copies interception.dll next to the built exe
assets/                     bundled tray icon (PNG embedded via include_bytes!)
src/
  main.rs                   entry — DPI setup → monitors → event loop → tray
  lib.rs                    exports config, core, and remapper
  config.rs                 user-editable TOML config (bypass list)
  remapper.rs               platform-agnostic crossing logic and tests
  core/
    mod.rs                  Monitor struct and physical↔pixel mapping
    geometry.rs             Point and Rect types
  platform/
    mod.rs                  cfg-gated platform selection
    windows.rs              Windows platform surface (re-exports)
    windows/
      monitors.rs           monitor enumeration, DPI setup, build_monitor_map
      event_loop.rs         Interception event loop
      lowlevel.rs           WH_MOUSE_LL userspace backend
      focus.rs              fullscreen-game and process-bypass detection
      layout.rs             hot-reload watcher for display changes
      tray.rs               system-tray icon and Win32 message pump
```

Adding Linux support requires only a new `platform/linux/` implementation of the same public surface (`setup_dpi_awareness`, `build_monitor_map`, `run_event_loop`). The remapper and core are unchanged.

## Known limitations / TODO

- [ ] Physical monitor size is hardcoded at **27"** for all monitors — should be read from EDID or a config file.

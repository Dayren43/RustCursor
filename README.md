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

1. Install the Interception driver (run as administrator, then reboot):
   ```
   install-interception.exe /install
   ```
   Download: https://github.com/oblitum/Interception/releases

2. Rust toolchain (stable, MSVC target):
   ```
   rustup target add x86_64-pc-windows-msvc
   ```

## Build and run

```
cargo build --release
.\target\release\RustCursor.exe
```

The `interception.dll` runtime library is not redistributed by the `interception-sys` crate or installed by the driver — it ships in the Interception release ZIP under `library/x64/`. `build.rs` copies it next to the built exe on every build. Provide it in one of two ways:

- Drop a copy at `vendor/interception.dll` in the repo root (gitignored), or
- Set the `INTERCEPTION_DLL` environment variable to the absolute path of `interception.dll`.

The app runs silently with no console window. A two-tone tray icon appears in the notification area — right-click it and choose **Quit RustCursor** to exit. Per-stroke diagnostics are written to `cursor_log.txt` in the working directory.

## Module layout

```
src/
  main.rs                   entry point — wires platform + config
  lib.rs                    exports core and remapper
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
```

Adding Linux support requires only a new `platform/linux/` implementation of the same public surface (`setup_dpi_awareness`, `build_monitor_map`, `run_event_loop`, `spawn_exit_on_escape`). The remapper and core are unchanged.

## Known limitations / TODO

- [ ] Physical monitor size is hardcoded at **27"** for all monitors — should be read from EDID or a config file.
- [ ] Rust 2024 edition warnings in `monitor_enum_proc` — raw pointer dereference and `GetMonitorInfoW` need explicit `unsafe {}` blocks.
- [ ] Monitor layout changes (plugging/unplugging, repositioning in display settings) require a restart.
- [ ] Windows display scaling (e.g. 150%) is expected to work with `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2` but has not been explicitly tested.
- [ ] Fullscreen and fullscreen-windowed games/apps are untested — verify Interception does not interfere with raw input or exclusive mode.
- [ ] Auto-start on login (Task Scheduler or `HKCU\Run`) for a fully hands-off setup.

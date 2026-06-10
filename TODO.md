# RustCursor — deferred review items

Items surfaced in the code review that need a design decision before acting.
The pure-cleanup wins (weak test, dead code, pin_to_source dedup) are already done.
Resolved since: #1 (field-level config parse + warning banner), #4 (log writes
moved to a writer thread), #8 (drag-end saves all monitors' positions).
Vertical gap-zone crossings (commit 2845025) validated on real stacked
hardware 2026-06-10: crossings land at the right physical column, no sticking.

## 2. README is stale re: the Settings window model
Docs still describe hide-to-tray + HWND show/hide; the code spawns a fresh
`--settings` subprocess per click and exits the process on close.
- `README.md:51` (close button "hides the window to the tray")
- `README.md:138-141` (module layout: `gui/mod.rs` "HWND show/hide",
  `app.rs` "HWND capture")
- Fix: rewrite to describe the subprocess-per-click design.

## 10. Default position seeding assumes a horizontal arrangement
`build_monitor_map` seeds every monitor at `y=0` with cumulative-width x, so a
fresh vertical stack is modeled side-by-side until arranged in the GUI. The
vertical gap-zone remap (added) and normal vertical crossings both rely on
correct `position_mm`, so vertical stacks need GUI setup before they behave
physically. Consider seeding y from the OS arrangement too, or detecting
stacked layouts.
- `src/platform/windows/monitors.rs:180-185`

## 9. Foreground-basename cache keyed only on raw HWND value
If a window is destroyed and its HWND value is reused by an unrelated window,
the cached bypass basename is stale until the value changes again. Bypass is
best-effort, so low severity; a PID check alongside the HWND would harden it.
- `src/platform/windows/focus.rs:111-119`

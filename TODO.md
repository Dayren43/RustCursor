# TODO

Open items from the code review of the `develop` branch (2026-07-24). Most of
that review is now addressed; what is left is parked here.

## Cleanup

### Per-frame allocation in the warnings banner
`src/gui/app.rs:46`

`load_warnings()` takes the `RwLock` and deep-clones a `Vec<String>` on every
repaint just to test whether it is empty, plus a `format!` per warning per
frame when warnings exist. The Log tab schedules `request_repaint_after(500ms)`
so repaints never stop.

Fix: a `config::has_warnings() -> bool` fast path, or snapshot into
`SettingsApp` and refresh only where `Config::load` is called.
`layout_issues(&self.rows)` at `src/gui/tabs/monitors.rs:137` has the same
shape: an O(n^2) scan plus fresh message strings on every frame the Monitors
tab is visible.

## Resolved, with one correction

- Gap-zone destination is now direction-aware and deterministic
  (`dest_across_x` / `dest_across_y` in `src/remapper.rs`).
- `save_position` writes committed diagonals only.
- Log tab is `#[cfg(feature = "log")]`; README documents both builds.
- CI runs clippy + tests across all three feature configurations.
- `release.yml` builds before it tags, and pushes the tag last.
- Em dashes removed.

The review's second correctness finding, "gap-zone branches skip the *already
correct* check", did not hold up. The delta check was hoisted into a shared
`correction()` anyway, but it can never fire on a gap-zone path: both branches
clamp the target inside the destination's bounds, while the raw destination is
by definition inside no monitor's rect, so the two can never be equal. The
per-event injection that finding described comes from `pin_to_source`, where it
is intended: an event pushing at a blocked edge has to be swallowed every time
or the cursor escapes into the gap.

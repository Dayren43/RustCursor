# Windows test plan: `fix/crossing-direction-and-hotplug`

Instructions for a Claude Code session on Windows, testing this branch with the
user. Work through the sections in order. The user does the physical actions
(moving the mouse, unplugging monitors); you build, run commands, read
`%LOCALAPPDATA%\RustCursor\cursor_log.txt`, and report. Stop and ask before
changing Windows display settings, and put them back afterwards.

**This file is for testing only. Drop its commit before merging the branch.**

## What changed and why

The branch was written on Linux. It was linted with clippy against
`x86_64-pc-windows-msvc` for all three CI feature sets, and the
platform-independent tests (`config`, `core`, `remapper`) passed there. The exe
has never been built or run, and the binary crate's tests have never run.

| Commit | Change | How it was checked on Linux |
|---|---|---|
| `fix: pick crossing direction from the exit edge…` | `src/remapper.rs`: decide horizontal vs vertical from the edge the cursor left through, not by comparing monitor centres. Also round float pixel targets instead of truncating them. | New test `offset_stack_crosses_vertically_both_ways`. It fails without the fix: a downward move was blocked. |
| `fix: re-resolve the active profile on display changes` | `layout.rs`: `WM_DISPLAYCHANGE` now loads the profile matching the newly connected monitors. A new helper, `install_matching_profile`, replaces three copies of that logic. | Code review only. |
| `fix: track injected cursor moves in the lowlevel hook` | `lowlevel.rs`: record `prev_pt` before skipping injected events, so a pen tablet or remote desktop moving the cursor doesn't leave a stale last position. | Code review only. |
| `docs: …` | README hand-edit claim, `main.rs` header comment. | n/a |
| `fix: save config.toml atomically…` | `gui/config_io.rs`: write to a temp file then rename it; widen f32 values without float noise (`23.8`, not `23.799999237060547`). | The float output was checked through `toml_edit`. The new tests in `config_io.rs` have not run. |

## 1. Setup

1. Confirm `cargo`, `rustup` and the `x86_64-pc-windows-msvc` toolchain are installed.
2. `git fetch` then `git switch fix/crossing-direction-and-hotplug`.
3. Ask the user to **quit any running RustCursor** from the tray: it can be
   running from the scheduled task. Two hooks at once give meaningless results,
   and a running exe can't be overwritten by the build.

## 2. Automated checks (what CI runs)

Run each of these, then report any failures with their output:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo clippy --all-targets --features interception-backend -- -D warnings
cargo test
cargo test --no-default-features
cargo test --features interception-backend
```

The `interception-backend` runs need the Interception import library, which
`interception-sys` provides. A missing `interception.dll` warning from
`build.rs` is expected and harmless for these checks. If those two runs fail
only because the driver or DLL is missing, say so and carry on.

Check that these new or changed tests ran and passed:
- `remapper::tests::offset_stack_crosses_vertically_both_ways`
- `gui::config_io::tests::tidy_keeps_the_typed_decimal`
- `gui::config_io::tests::written_sizes_have_no_float_noise`

## 3. Build the test exe

Use the default features (with the `log` feature) so `cursor_log.txt` records
every crossing:

```
cargo build --release
```

The app must run **elevated** (see the README, "Run elevated"). Ask the user to
start `target\release\RustCursor.exe` with **Run as administrator**, or start it
from an elevated shell. The tray icon should appear, and the log should start
with `=== session start (lowlevel backend) ===` and the monitor layout.

## 4. Manual tests

For each test, tell the user what to do and what to expect, then read the
log's `REMAP` lines (`REMAP (old) → (corrected) [raw (…)] [process]`) to
confirm. If behaviour differs from what's expected, collect the log lines and
the layout header before going further.

### 4.1 Regression: normal crossings (always run)

The user's setup is a 1080p and a 1440p monitor side by side.

1. Move the cursor slowly across the shared edge in both directions, near the
   top, the middle and the bottom.
2. Expected: crossings feel the same as on v0.6.0. The cursor keeps its
   physical height, and there's no stickiness at the edge except where the
   layout really has no panel.
3. Also check the top and bottom corners where the 1440p is taller than the
   1080p (the gap zones). The cursor should slide along the edge, not jump.

### 4.2 Fix #1: offset vertical stack (optional; changes Windows display settings)

This only reproduces when monitors are stacked vertically with a large
sideways offset. Ask the user first, and note the current arrangement so it
can be restored.

1. In Windows Display Settings, put the 1080p **below** the 1440p, slid right
   so their left edges are about 1800 px apart (the bottom monitor's x is
   around 1800). The two panels still share a 760 px stretch of edge. Apply.
2. In RustCursor Settings → Monitors, arrange the rectangles the same way:
   1080p below, right-aligned roughly the same, edges touching. Then close
   Settings.
3. Move the cursor **down** from the 1440p into the 1080p across the shared
   stretch, then back **up**.
4. Expected: both directions cross, and the x position follows physical
   position (the landing x is shifted because pixel densities differ). On
   v0.6.0, moving down here pinned the cursor to the 1440p's bottom edge.
5. Restore the original arrangement in Display Settings and in the Monitors
   tab, and confirm 4.1 still behaves.

### 4.3 Fix #2: profile follows monitor plug and unplug

1. In Settings → Monitors, move the 1080p rectangle **down by about 100 mm**
   (hold Alt to disable snapping) so it's clearly offset. Close Settings.
   Crossings near the top of the 1440p should now be blocked or land lower,
   which makes the profile's effect easy to see. Note the behaviour.
2. Disconnect one monitor while RustCursor keeps running: unplug its cable,
   or use Display Settings → "Disconnect this display". Then reconnect it.
3. Cross at the same spots as in step 1.
4. Expected: same behaviour as step 1, so the saved profile was re-applied.
   Before the fix, the reconnected layout fell back to default positions (no
   100 mm offset) until RustCursor was restarted.
5. Put the 1080p rectangle back where it was (or restore the profile's
   `position_mm` values in `config.toml` from a copy you took before step 1).

### 4.4 Fix #3: injected cursor moves (optional)

This needs input that Windows marks as injected. A pen tablet or a remote
desktop session works. Without either, use this PowerShell snippet, which
moves the cursor with `SendInput` (flagged `LLMHF_INJECTED`):

```powershell
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Inj {
  [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public MOUSEINPUT mi; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int size);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
  public static void MoveTo(int x, int y) {
    int vx = GetSystemMetrics(76), vy = GetSystemMetrics(77), vw = GetSystemMetrics(78), vh = GetSystemMetrics(79);
    var inp = new INPUT { type = 0 };
    inp.mi.dx = (int)((x - vx) * 65535L / (vw - 1));
    inp.mi.dy = (int)((y - vy) * 65535L / (vh - 1));
    inp.mi.flags = 0x0001 | 0x8000 | 0x4000; // MOVE | ABSOLUTE | VIRTUALDESK
    SendInput(1, new[] { inp }, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
Start-Sleep 3; [Inj]::MoveTo(<x>, <y>)
```

Replace `<x>, <y>` with a point in the middle of the monitor the cursor is
**not** on; use the layout header in the log for coordinates. Display scaling
can make the landing point inexact, which is fine as long as it lands on the
other monitor.

1. The user parks the cursor on one monitor, you run the snippet, and within
   3 s the cursor moves to the other monitor.
2. The user then nudges the real mouse a little.
3. Expected: the cursor carries on from where the injected move put it. Before
   the fix, the first real move was treated as a crossing from the old
   monitor, and the cursor jumped back to the old monitor's edge or snapped to
   a different height.

### 4.5 Config writer

1. In Settings → Monitors, set a diagonal with a decimal (e.g. 23.8), let the
   field lose focus, then drag a rectangle a little.
2. Open `config.toml`. Expected: `size_in = 23.8` and short `position_mm`
   values, with no `23.799999237060547`-style noise, and the file's comments
   still intact. No `config.toml.tmp` should be left behind.
3. Put the diagonal back to its real value.

## 5. Report back

Summarise for the user: which checks passed, which failed (with log excerpts
or command output), and which were skipped and why. If everything passed,
they can push the branch and open a PR, after dropping this file's commit.

//! User-editable runtime config loaded from `%LOCALAPPDATA%\RustCursor\config.toml`.
//! The file is created with documentation comments on first run.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;

/// Mouse-input backend selection.
///
/// - `Interception`: kernel-driver path (`oblitum/Interception`). No snap
///   artifact at monitor crossings, but flagged by kernel anti-cheats
///   (Vanguard, Javelin, EAC kernel mode). The driver must be installed.
/// - `Lowlevel`: user-mode `WH_MOUSE_LL` hook (LBM-style). No driver needed,
///   AC-compatible, but a brief snap is visible on every monitor crossing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Interception,
    #[default]
    #[serde(alias = "lowlevel_hook", alias = "ll")]
    Lowlevel,
}

/// Per-monitor override entry from the `[[monitor]]` array of tables.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct MonitorEntry {
    /// Windows device name, e.g. `\\.\DISPLAY1`. Find yours in
    /// `%LOCALAPPDATA%\RustCursor\cursor_log.txt`.
    pub device: String,
    /// Physical diagonal size in inches.
    pub size_in: f32,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    /// Which mouse-input backend to use.
    #[serde(default)]
    pub backend: Backend,

    /// Foreground process basenames (e.g. "csgo2.exe") whose focus should pause
    /// cursor remapping. Case-insensitive.
    #[serde(default)]
    pub bypass_processes: Vec<String>,

    /// Diagonal size in inches used when a monitor has no `[[monitor]]` override.
    #[serde(default = "default_size_in")]
    pub default_size_in: f32,

    /// Per-monitor diagonal-size overrides. Serialised as `[[monitor]]` tables.
    #[serde(default, rename = "monitor")]
    pub monitors: Vec<MonitorEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            bypass_processes: Vec::new(),
            default_size_in: default_size_in(),
            monitors: Vec::new(),
        }
    }
}

fn default_size_in() -> f32 {
    27.0
}

impl Config {
    /// Load config from the standard location, creating a default file with
    /// documentation if it does not yet exist. Falls back to `Config::default()`
    /// on any I/O or parse error so a malformed config never wedges startup.
    pub fn load() -> Self {
        let Some(path) = path() else {
            return Self::default();
        };
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, DEFAULT_CONFIG);
        }
        match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }
}

/// Path to the config file: `%LOCALAPPDATA%\RustCursor\config.toml`.
pub fn path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|s| PathBuf::from(s).join("RustCursor").join("config.toml"))
}

// ── Per-monitor physical size lookup ────────────────────────────────────────
// Installed once at startup; read by `build_monitor_map` (initial enumeration)
// and by the WM_DISPLAYCHANGE handler (rebuild on layout change).

struct SizeMap {
    overrides: HashMap<String, f32>,
    default: f32,
}

static SIZES: OnceLock<SizeMap> = OnceLock::new();

/// Install the per-monitor size lookup from a loaded `Config`. Call once at
/// startup before any `build_monitor_map` invocation. Calling more than once
/// is a no-op.
pub fn install_monitor_sizes(monitors: Vec<MonitorEntry>, default: f32) {
    let overrides = monitors
        .into_iter()
        .map(|e| (e.device, e.size_in))
        .collect();
    let _ = SIZES.set(SizeMap { overrides, default });
}

/// Diagonal size in inches for the given Windows device name. Returns the
/// configured default (or a hard fallback of 27.0 if `install_monitor_sizes`
/// was never called) when no override matches.
pub fn size_for(device: &str) -> f32 {
    SIZES
        .get()
        .map(|s| s.overrides.get(device).copied().unwrap_or(s.default))
        .unwrap_or(27.0)
}

const DEFAULT_CONFIG: &str = "\
# RustCursor config — %LOCALAPPDATA%\\RustCursor\\config.toml
# Restart RustCursor after editing.

# Mouse-input backend.
#   \"lowlevel\"     — user-mode WH_MOUSE_LL hook. AC-compatible, no driver
#                    needed, brief snap visible on monitor crossings. (default)
#   \"interception\" — kernel driver, no snap artifact, but blocked by kernel
#                    anti-cheats (Vanguard, Javelin, kernel-mode EAC). Requires
#                    the Interception driver to be installed.
backend = \"lowlevel\"

# Foreground processes that pause cursor remapping while focused.
# Use executable basenames (with .exe), case-insensitive.
#
# Fullscreen DirectX/Vulkan/OpenGL games are auto-detected via
# SHQueryUserNotificationState — only list a game here if it isn't
# being detected automatically (e.g. windowed-fullscreen titles).
bypass_processes = []

# Default physical monitor diagonal size in inches. Used when a monitor is
# not listed in [[monitor]] below.
default_size_in = 27.0

# Per-monitor physical size overrides. The device name is Windows'
# \\\\.\\DISPLAYx; look at %LOCALAPPDATA%\\RustCursor\\cursor_log.txt to see
# the names assigned to your current monitors.
#
# [[monitor]]
# device = '\\\\.\\DISPLAY1'
# size_in = 27.0
#
# [[monitor]]
# device = '\\\\.\\DISPLAY2'
# size_in = 24.0
";

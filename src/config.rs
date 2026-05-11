//! User-editable runtime config loaded from `%LOCALAPPDATA%\RustCursor\config.toml`.
//! The file is created with documentation comments on first run.

use std::path::PathBuf;

use serde::Deserialize;

/// Mouse-input backend selection.
///
/// - `Interception`: kernel-driver path (`oblitum/Interception`). No snap
///   artifact at monitor crossings, but flagged by kernel anti-cheats
///   (Vanguard, Javelin, EAC kernel mode). The driver must be installed.
/// - `LowLevel`: user-mode `WH_MOUSE_LL` hook (LBM-style). No driver needed,
///   AC-compatible, but a brief snap is visible on every monitor crossing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Interception,
    #[default]
    #[serde(alias = "lowlevel_hook", alias = "ll")]
    Lowlevel,
}

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// Which mouse-input backend to use.
    #[serde(default)]
    pub backend: Backend,

    /// Foreground process basenames (e.g. "csgo2.exe") whose focus should pause
    /// cursor remapping. Case-insensitive.
    #[serde(default)]
    pub bypass_processes: Vec<String>,
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
";

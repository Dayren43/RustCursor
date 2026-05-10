//! User-editable runtime config loaded from `%LOCALAPPDATA%\RustCursor\config.toml`.
//! The file is created with documentation comments on first run.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
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

# Foreground processes that pause cursor remapping while focused.
# Use executable basenames (with .exe), case-insensitive.
#
# Fullscreen DirectX/Vulkan/OpenGL games are auto-detected via
# SHQueryUserNotificationState — only list a game here if it isn't
# being detected automatically (e.g. windowed-fullscreen titles).
bypass_processes = []
";

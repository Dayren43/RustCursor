//! User-editable runtime config loaded from `%LOCALAPPDATA%\RustCursor\config.toml`.
//! The file is created with documentation comments on first run.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::RwLock;

use serde::Deserialize;

/// Mouse-input backend selection.
///
/// - `Interception`: kernel-driver path (`oblitum/Interception`). No snap
///   artifact at monitor crossings, but flagged by kernel anti-cheats
///   (Vanguard, Javelin, EAC kernel mode). The driver must be installed.
/// - `Lowlevel`: user-mode `WH_MOUSE_LL` hook. No driver needed,
///   AC-compatible, but a brief snap is visible on every monitor crossing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[cfg(feature = "interception-backend")]
    Interception,
    #[default]
    #[serde(alias = "lowlevel_hook", alias = "ll")]
    Lowlevel,
}

/// Per-monitor entry inside a `[[profile.monitor]]` array. Keyed by stable
/// HWID rather than OS slot so cable swaps and port changes don't re-trigger
/// first-time setup.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct ProfileMonitor {
    pub hwid: String,
    pub size_in: f32,
    /// Physical position of the monitor's top-left corner in the profile's
    /// shared millimetre coordinate space. Consumed by the remapper from
    /// the math-rewrite commit onward; absent on entries created before that.
    #[serde(default)]
    pub position_mm: Option<[f32; 2]>,
}

/// Layout for one specific set of connected monitors. The `hwids` field is
/// matched (order-independent) against the currently-connected HWIDs to pick
/// the active profile at startup.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub hwids: Vec<String>,
    /// Human description, regenerated when the GUI writes the profile.
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "monitor")]
    pub monitors: Vec<ProfileMonitor>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub backend: Backend,

    /// Foreground process basenames (e.g. "csgo2.exe") whose focus should pause
    /// cursor remapping. Case-insensitive.
    #[serde(default)]
    pub bypass_processes: Vec<String>,

    /// Diagonal size in inches used when no profile or legacy entry matches.
    #[serde(default = "default_size_in")]
    pub default_size_in: f32,

    /// HWID-keyed layouts. The profile whose `hwids` set equals the currently
    /// connected HWID set is selected at startup.
    #[serde(default, rename = "profile")]
    pub profiles: Vec<Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            bypass_processes: Vec::new(),
            default_size_in: default_size_in(),
            profiles: Vec::new(),
        }
    }
}

fn default_size_in() -> f32 {
    27.0
}

impl Config {
    /// Load config from the standard location, creating a default file with
    /// documentation if it does not yet exist. Parsing is resilient per
    /// top-level field: an invalid value is dropped with a warning instead of
    /// discarding the whole document, so one bad field can't silently wipe
    /// the profiles' calibration or the bypass list. Warnings from the most
    /// recent load are retrievable via [`load_warnings`]; only an unreadable
    /// or syntactically broken file falls back to `Config::default()`
    /// entirely. Exception: in single-backend builds an invalid `backend` is
    /// ignored without a warning, since the build runs lowlevel regardless.
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
        let (cfg, warnings) = match std::fs::read_to_string(&path) {
            Ok(s) => Self::parse_resilient(&s),
            Err(e) => (
                Self::default(),
                vec![format!("could not read {}: {e}", path.display())],
            ),
        };
        *LOAD_WARNINGS.write().expect("LOAD_WARNINGS poisoned") = warnings;
        cfg
    }

    /// Parse `s`, salvaging whatever deserializes cleanly. Fast path: the
    /// whole document parses and there are no warnings. On failure each
    /// top-level field is deserialized independently against the syntax tree;
    /// fields that fail keep their default and produce a warning naming them.
    fn parse_resilient(s: &str) -> (Self, Vec<String>) {
        match toml::from_str::<Self>(s) {
            Ok(cfg) => (cfg, Vec::new()),
            Err(_) => match s.parse::<toml::Table>() {
                Ok(table) => {
                    let mut warnings = Vec::new();
                    let mut cfg = Self::default();
                    // `backend` only selects behaviour when more than one
                    // backend is compiled in. A lowlevel-only build runs
                    // lowlevel whatever the field says (and has no GUI to
                    // rewrite it), so an invalid value there is ignored
                    // without a warning rather than nagging unactionably.
                    #[cfg(feature = "interception-backend")]
                    if let Some(v) = field::<Backend>(&table, "backend", &mut warnings) {
                        cfg.backend = v;
                    }
                    if let Some(v) =
                        field::<Vec<String>>(&table, "bypass_processes", &mut warnings)
                    {
                        cfg.bypass_processes = v;
                    }
                    if let Some(v) = field::<f32>(&table, "default_size_in", &mut warnings) {
                        cfg.default_size_in = v;
                    }
                    if let Some(v) = field::<Vec<Profile>>(&table, "profile", &mut warnings) {
                        cfg.profiles = v;
                    }
                    (cfg, warnings)
                }
                Err(e) => (
                    Self::default(),
                    vec![format!("not valid TOML, all defaults in effect: {}", e.message())],
                ),
            },
        }
    }

    /// Find the profile whose HWID set equals `connected` (order-independent).
    /// Returns `None` when no profile matches; callers should treat that as
    /// "fresh layout" and either synthesize defaults at runtime or prompt the
    /// user to create one via the GUI.
    pub fn active_profile(&self, connected: &[String]) -> Option<&Profile> {
        let mut want: Vec<&str> = connected.iter().map(String::as_str).collect();
        want.sort();
        self.profiles.iter().find(|p| {
            let mut have: Vec<&str> = p.hwids.iter().map(String::as_str).collect();
            have.sort();
            have == want
        })
    }
}

/// Deserialize one top-level field out of the parsed syntax tree, pushing a
/// warning and returning `None` (caller keeps the default) when invalid.
fn field<T: serde::de::DeserializeOwned>(
    table: &toml::Table,
    key: &str,
    warnings: &mut Vec<String>,
) -> Option<T> {
    let v = table.get(key)?;
    match v.clone().try_into::<T>() {
        Ok(t) => Some(t),
        Err(e) => {
            warnings.push(format!("`{key}` ignored ({}), default in effect", e.message()));
            None
        }
    }
}

/// Path to the config file: `%LOCALAPPDATA%\RustCursor\config.toml`.
pub fn path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|s| PathBuf::from(s).join("RustCursor").join("config.toml"))
}

// ── Config-load warnings ───────────────────────────────────────────────────
// Populated by every `Config::load`; surfaced as a banner in the Settings GUI
// and in the session header of cursor_log.txt (log builds). Without this a
// salvaged field would fall back to its default with no visible trace —
// `windows_subsystem = "windows"` means stderr goes nowhere.

static LOAD_WARNINGS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Warnings recorded by the most recent `Config::load` in this process.
/// Empty when the config parsed cleanly.
pub fn load_warnings() -> Vec<String> {
    LOAD_WARNINGS.read().expect("LOAD_WARNINGS poisoned").clone()
}

// ── Active-sizes lookup ────────────────────────────────────────────────────
// Installed once at startup; consulted by `build_monitor_map` (initial
// enumeration) and by the WM_DISPLAYCHANGE handler (rebuild on layout change).
// Keyed by stable HWID; `default` covers any connected monitor not in the
// active profile (or any monitor whose adapter doesn't expose a HWID).

struct HwidEntry {
    size_in: f32,
    position_mm: Option<(f32, f32)>,
}

struct ActiveSizes {
    by_hwid: HashMap<String, HwidEntry>,
    default: f32,
}

static SIZES: RwLock<Option<ActiveSizes>> = RwLock::new(None);

/// Install (or replace) the per-monitor size and position lookup from a
/// resolved active profile. Called once at startup and again from the GUI
/// after every successful save so live edits show up without a restart.
pub fn install_active_profile(profile_monitors: Vec<ProfileMonitor>, default: f32) {
    let by_hwid = profile_monitors
        .into_iter()
        .map(|m| {
            (
                m.hwid,
                HwidEntry {
                    size_in: m.size_in,
                    position_mm: m.position_mm.map(|p| (p[0], p[1])),
                },
            )
        })
        .collect();
    *SIZES.write().expect("SIZES poisoned") = Some(ActiveSizes { by_hwid, default });
}

/// Diagonal size in inches for a monitor identified by stable HWID. Returns
/// `default_size_in` (or the hard 27.0 fallback if `install_active_profile`
/// was never called) when no HWID match exists.
pub fn size_for(hwid: Option<&str>) -> f32 {
    let guard = SIZES.read().expect("SIZES poisoned");
    let Some(sizes) = guard.as_ref() else {
        return 27.0;
    };
    if let Some(h) = hwid
        && let Some(e) = sizes.by_hwid.get(h)
    {
        return e.size_in;
    }
    sizes.default
}

/// Physical top-left in shared millimetre coordinates for a monitor, if the
/// active profile pins one. `None` means the caller should fall back to the
/// runtime default (left-to-right cumulative-width seeding).
pub fn position_for(hwid: Option<&str>) -> Option<(f32, f32)> {
    let guard = SIZES.read().expect("SIZES poisoned");
    let sizes = guard.as_ref()?;
    let h = hwid?;
    sizes.by_hwid.get(h)?.position_mm
}

// ── Active bypass-process lookup ───────────────────────────────────────────
// Consulted by `focus::FocusGuard::should_skip_remap` on every stroke. Stored
// behind a RwLock so the GUI's Bypass tab can swap it after add/remove without
// requiring a RustCursor restart. Stored lowercased for case-insensitive match.

static BYPASS: RwLock<Option<HashSet<String>>> = RwLock::new(None);

/// Install (or replace) the bypass-process lookup. Called once at startup
/// from the loaded config and again from the GUI after every add/remove.
pub fn install_bypass_processes(processes: Vec<String>) {
    let set: HashSet<String> = processes.into_iter().map(|s| s.to_lowercase()).collect();
    *BYPASS.write().expect("BYPASS poisoned") = Some(set);
}

/// True when the bypass list is empty (or never installed). Lets the caller
/// skip the basename lookup entirely in the common no-bypass case.
pub fn bypass_is_empty() -> bool {
    let guard = BYPASS.read().expect("BYPASS poisoned");
    guard.as_ref().map(|b| b.is_empty()).unwrap_or(true)
}

/// True when `basename` (already lowercased) is in the bypass list.
pub fn is_bypassed(basename: &str) -> bool {
    let guard = BYPASS.read().expect("BYPASS poisoned");
    let Some(b) = guard.as_ref() else {
        return false;
    };
    b.contains(basename)
}

const DEFAULT_CONFIG: &str = indoc::indoc! {r#"
    # RustCursor config: %LOCALAPPDATA%\RustCursor\config.toml
    # Restart RustCursor after editing.

    # Mouse-input backend.
    #   "lowlevel"     : user-mode WH_MOUSE_LL hook. AC-compatible, no driver
    #                    needed, brief snap visible on monitor crossings. (default)
    #   "interception" : kernel driver, no snap artifact, but blocked by kernel
    #                    anti-cheats (Vanguard, Javelin, kernel-mode EAC). Requires
    #                    the Interception driver to be installed.
    backend = "lowlevel"

    # Foreground processes that pause cursor remapping while focused.
    # Use executable basenames (with .exe), case-insensitive.
    #
    # Fullscreen DirectX/Vulkan/OpenGL games are auto-detected via
    # SHQueryUserNotificationState; only list a game here if it isn't
    # being detected automatically (e.g. windowed-fullscreen titles).
    bypass_processes = []

    # Default physical monitor diagonal size in inches. Used when no [[profile]]
    # matches the currently connected monitors, or a connected monitor has no
    # per-HWID entry.
    default_size_in = 27.0

    # Per-display-set layouts. A profile applies when its `hwids` set equals the
    # currently connected monitors' set (order-independent). The Settings GUI
    # writes profiles automatically; you usually do not need to edit by hand.
    #
    # [[profile]]
    # hwids = ["MONITOR\\DEL41B7", "MONITOR\\GSM5BAF"]
    # description = "Dell 27 + LG 27"
    #
    # [[profile.monitor]]
    # hwid        = "MONITOR\\DEL41B7"
    # size_in     = 27.0
    # position_mm = [0.0, 0.0]
    #
    # [[profile.monitor]]
    # hwid        = "MONITOR\\GSM5BAF"
    # size_in     = 27.0
    # position_mm = [600.0, 0.0]
"#};

#[cfg(test)]
mod tests {
    use super::*;

    /// Valid calibration data that must survive any single bad sibling field.
    const CALIBRATION: &str = r#"
        bypass_processes = ["game.exe"]
        default_size_in = 24.0

        [[profile]]
        hwids = ["MONITOR\\AAA1111"]
        description = "test"

        [[profile.monitor]]
        hwid = "MONITOR\\AAA1111"
        size_in = 31.5
        position_mm = [10.0, 20.0]
    "#;

    #[test]
    fn clean_document_parses_without_warnings() {
        let (cfg, warnings) = Config::parse_resilient(CALIBRATION);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert_eq!(cfg.profiles.len(), 1);
        assert_eq!(cfg.profiles[0].monitors[0].position_mm, Some([10.0, 20.0]));
    }

    #[test]
    fn default_config_template_parses_without_warnings() {
        let (_, warnings) = Config::parse_resilient(DEFAULT_CONFIG);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn unknown_backend_keeps_calibration() {
        let doc = format!("backend = \"bogus\"\n{CALIBRATION}");
        let (cfg, warnings) = Config::parse_resilient(&doc);
        assert_eq!(cfg.backend, Backend::Lowlevel);
        assert_eq!(cfg.bypass_processes, vec!["game.exe".to_string()]);
        assert_eq!(cfg.default_size_in, 24.0);
        assert_eq!(cfg.profiles.len(), 1);
        assert_eq!(cfg.profiles[0].monitors[0].size_in, 31.5);
        assert_eq!(cfg.profiles[0].monitors[0].position_mm, Some([10.0, 20.0]));
        // A bad backend only warrants a warning when the build actually has
        // a backend choice; single-backend builds ignore the field silently.
        #[cfg(feature = "interception-backend")]
        {
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("backend"), "warning: {}", warnings[0]);
        }
        #[cfg(not(feature = "interception-backend"))]
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn bad_profile_field_keeps_other_fields_and_warns() {
        let doc = "default_size_in = 24.0\nbypass_processes = [\"game.exe\"]\nprofile = 3\n";
        let (cfg, warnings) = Config::parse_resilient(doc);
        assert_eq!(cfg.default_size_in, 24.0);
        assert_eq!(cfg.bypass_processes, vec!["game.exe".to_string()]);
        assert!(cfg.profiles.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("profile"), "warning: {}", warnings[0]);
    }

    #[test]
    fn syntax_error_falls_back_to_defaults_with_warning() {
        let (cfg, warnings) = Config::parse_resilient("backend = [unterminated");
        assert_eq!(cfg.default_size_in, 27.0);
        assert!(cfg.profiles.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}

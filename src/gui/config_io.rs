//! Read/write `config.toml` while preserving the documentation comments that
//! ship in the default file. `toml_edit` keeps the original tree intact and
//! only rewrites the values we touch.

use std::path::PathBuf;

use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

#[cfg(feature = "interception-backend")]
use rust_cursor::config::Backend;
use rust_cursor::config::path;

pub struct ConfigDoc {
    path: PathBuf,
    doc: DocumentMut,
}

impl ConfigDoc {
    /// Load the file from disk. The file is guaranteed to exist after the
    /// first `Config::load()` at app startup, so a missing file here is an
    /// unexpected state we surface as an error.
    pub fn load() -> Result<Self, String> {
        let path = path().ok_or_else(|| "LOCALAPPDATA is not set".to_string())?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("read {}: {}", path.display(), e))?;
        let doc: DocumentMut = text
            .parse()
            .map_err(|e| format!("parse {}: {}", path.display(), e))?;
        Ok(Self { path, doc })
    }

    /// Write to a sibling temp file, then rename it over `config.toml`, so a
    /// crash or full disk mid-write leaves the old file intact instead of a
    /// truncated one that would load as all defaults. `rename` replaces an
    /// existing file on Windows (`MOVEFILE_REPLACE_EXISTING`).
    pub fn save(&self) -> Result<(), String> {
        let tmp = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp, self.doc.to_string())
            .map_err(|e| format!("write {}: {}", tmp.display(), e))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("replace {}: {}", self.path.display(), e)
        })
    }

    /// Only exists alongside the General tab's backend section: a build with
    /// a single compiled-in backend has no UI that writes this field.
    #[cfg(feature = "interception-backend")]
    pub fn set_backend(&mut self, backend: Backend) {
        let s = match backend {
            Backend::Lowlevel => "lowlevel",
            Backend::Interception => "interception",
        };
        self.doc["backend"] = value(s);
    }

    pub fn set_default_size_in(&mut self, inches: f32) {
        self.doc["default_size_in"] = value(tidy(inches));
    }

    /// Overwrite the `bypass_processes` array with the given list. The order
    /// is preserved so the GUI's row order matches the file.
    pub fn set_bypass_processes(&mut self, processes: &[String]) {
        let mut arr = Array::new();
        for p in processes {
            arr.push(p.as_str());
        }
        self.doc["bypass_processes"] = value(arr);
    }

    /// Upsert a `[[profile.monitor]]` entry inside the profile keyed by
    /// `profile_hwids`. Creates the profile if no existing one has a matching
    /// HWID set (order-independent). `description` is written/refreshed on
    /// every call so the human label tracks whatever the GUI shows.
    /// `position_mm` is optional so size-only edits don't synthesise a
    /// position field for entries that previously had none.
    pub fn upsert_profile_monitor(
        &mut self,
        profile_hwids: &[String],
        hwid: &str,
        size_in: f32,
        position_mm: Option<(f32, f32)>,
        description: &str,
    ) {
        if !matches!(self.doc.get("profile"), Some(Item::ArrayOfTables(_))) {
            self.doc["profile"] = Item::ArrayOfTables(ArrayOfTables::new());
        }
        let profiles = self.doc["profile"]
            .as_array_of_tables_mut()
            .expect("profile key is array of tables");

        let mut want: Vec<&str> = profile_hwids.iter().map(String::as_str).collect();
        want.sort();

        let mut profile_idx: Option<usize> = None;
        for i in 0..profiles.len() {
            let p = profiles.get(i).expect("profile index in bounds");
            let mut have: Vec<&str> = p
                .get("hwids")
                .and_then(|it| it.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            have.sort();
            if have == want {
                profile_idx = Some(i);
                break;
            }
        }

        let idx = profile_idx.unwrap_or_else(|| {
            let mut tbl = Table::new();
            let mut arr = Array::new();
            for h in profile_hwids {
                arr.push(h.as_str());
            }
            tbl["hwids"] = value(arr);
            profiles.push(tbl);
            profiles.len() - 1
        });
        let profile = profiles
            .get_mut(idx)
            .expect("profile index in bounds after upsert");

        profile["description"] = value(description);

        if !matches!(profile.get("monitor"), Some(Item::ArrayOfTables(_))) {
            profile["monitor"] = Item::ArrayOfTables(ArrayOfTables::new());
        }
        let monitors = profile["monitor"]
            .as_array_of_tables_mut()
            .expect("profile.monitor key is array of tables");

        for i in 0..monitors.len() {
            let same = monitors
                .get(i)
                .and_then(|t| t.get("hwid"))
                .and_then(|it| it.as_str())
                == Some(hwid);
            if same {
                let tbl = monitors.get_mut(i).expect("monitor index in bounds");
                tbl["size_in"] = value(tidy(size_in));
                if let Some(pos) = position_mm {
                    tbl["position_mm"] = value(position_array(pos));
                }
                return;
            }
        }

        let mut tbl = Table::new();
        tbl["hwid"] = value(hwid);
        tbl["size_in"] = value(tidy(size_in));
        if let Some(pos) = position_mm {
            tbl["position_mm"] = value(position_array(pos));
        }
        monitors.push(tbl);
    }
}

fn position_array(pos: (f32, f32)) -> Array {
    let mut arr = Array::new();
    arr.push(tidy(pos.0));
    arr.push(tidy(pos.1));
    arr
}

/// Widen an `f32` to the `f64` with the same shortest decimal form. A plain
/// `as f64` keeps the f32's binary error, so a diagonal typed as 23.8 would be
/// written to the hand-editable file as `23.799999237060547`.
fn tidy(v: f32) -> f64 {
    v.to_string().parse().unwrap_or(v as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_keeps_the_typed_decimal() {
        assert_eq!(tidy(23.8), 23.8);
        assert_eq!(tidy(27.0), 27.0);
        assert_eq!(tidy(597.5), 597.5);
    }

    #[test]
    fn written_sizes_have_no_float_noise() {
        let mut doc = ConfigDoc {
            path: PathBuf::new(),
            doc: DocumentMut::new(),
        };
        doc.set_default_size_in(23.8);
        doc.upsert_profile_monitor(
            &["MONITOR\\AAA1111".to_string()],
            "MONITOR\\AAA1111",
            31.5,
            Some((597.9, 12.3)),
            "test",
        );
        let text = doc.doc.to_string();
        assert!(text.contains("default_size_in = 23.8\n"), "{text}");
        assert!(text.contains("position_mm = [597.9, 12.3]"), "{text}");
    }
}

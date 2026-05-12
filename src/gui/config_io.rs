//! Read/write `config.toml` while preserving the documentation comments that
//! ship in the default file. `toml_edit` keeps the original tree intact and
//! only rewrites the values we touch.

use std::path::PathBuf;

use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

use rust_cursor::config::{Backend, path};

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

    pub fn save(&self) -> Result<(), String> {
        std::fs::write(&self.path, self.doc.to_string())
            .map_err(|e| format!("write {}: {}", self.path.display(), e))
    }

    pub fn set_backend(&mut self, backend: Backend) {
        let s = match backend {
            Backend::Lowlevel => "lowlevel",
            Backend::Interception => "interception",
        };
        self.doc["backend"] = value(s);
    }

    pub fn set_default_size_in(&mut self, inches: f32) {
        self.doc["default_size_in"] = value(inches as f64);
    }

    /// Legacy: upsert a `[[monitor]]` entry for `device`. Kept as a fallback
    /// for monitors that report no HWID; the GUI prefers `upsert_profile_monitor`
    /// when one is available.
    pub fn set_monitor_size(&mut self, device: &str, size_in: f32) {
        if !matches!(self.doc.get("monitor"), Some(Item::ArrayOfTables(_))) {
            self.doc["monitor"] = Item::ArrayOfTables(ArrayOfTables::new());
        }
        let aot = self.doc["monitor"]
            .as_array_of_tables_mut()
            .expect("monitor key is array of tables");

        for tbl in aot.iter_mut() {
            if tbl.get("device").and_then(|i| i.as_str()) == Some(device) {
                tbl["size_in"] = value(size_in as f64);
                return;
            }
        }

        let mut tbl = Table::new();
        tbl["device"] = value(device);
        tbl["size_in"] = value(size_in as f64);
        aot.push(tbl);
    }

    /// Upsert a `[[profile.monitor]]` entry inside the profile keyed by
    /// `profile_hwids`. Creates the profile if no existing one has a matching
    /// HWID set (order-independent). `description` is written/refreshed on
    /// every call so the human label tracks whatever the GUI shows.
    pub fn upsert_profile_monitor(
        &mut self,
        profile_hwids: &[String],
        hwid: &str,
        size_in: f32,
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
                monitors.get_mut(i).expect("monitor index in bounds")["size_in"] =
                    value(size_in as f64);
                return;
            }
        }

        let mut tbl = Table::new();
        tbl["hwid"] = value(hwid);
        tbl["size_in"] = value(size_in as f64);
        monitors.push(tbl);
    }
}

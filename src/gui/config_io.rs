//! Read/write `config.toml` while preserving the documentation comments that
//! ship in the default file. `toml_edit` keeps the original tree intact and
//! only rewrites the values we touch.

use std::path::PathBuf;

use toml_edit::{DocumentMut, value};

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
}

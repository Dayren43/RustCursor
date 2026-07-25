//! Settings window tabs.
//!
//! Each tab implements [`SettingsTab`] in its own file and `app.rs` holds them
//! as a `Vec<Box<dyn SettingsTab>>`, so adding or removing one touches the
//! module list here plus the registry in `SettingsApp::default`, and nothing
//! else. That matters for the feature-gated tabs: a variant, a struct field, an
//! initialiser, a selector and a match arm would each have needed the same
//! `#[cfg]`, and any one of them could be missed.

use eframe::egui;

pub mod bypass;
pub mod general;
/// Only built with the `log` feature: without it nothing writes
/// `cursor_log.txt`, so the tab would have nothing to tail. The
/// `impl SettingsTab` lives in the module itself and so needs no `cfg` of its
/// own.
#[cfg(feature = "log")]
pub mod log;
pub mod monitors;

/// One tab of the Settings window.
pub trait SettingsTab {
    /// Label for the tab's selector button.
    fn title(&self) -> &'static str;
    /// Draw the tab's body into the central panel.
    fn show(&mut self, ui: &mut egui::Ui);
}

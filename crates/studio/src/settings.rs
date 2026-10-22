//! Remembered choices, stored as JSON in the per-user config folder
//! (`~/.config/legaia-studio/settings.json` on Linux,
//! `%APPDATA%\legaia-studio\` on Windows,
//! `~/Library/Application Support/legaia-studio/` on macOS).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The disc image or extracted folder opened last.
    pub last_source: Option<PathBuf>,
    /// The folder the last extraction wrote into.
    pub last_out_dir: Option<PathBuf>,
    /// The folder the last export wrote into.
    pub last_export_dir: Option<PathBuf>,
    pub skip_png: bool,
    pub skip_verify: bool,
    pub skip_xa: bool,
    pub skip_catalog: bool,
    pub skip_font: bool,
    /// Extract tab: also export every texture as PNG.
    pub all_textures: bool,
    /// The window's inner size (logical points) when it last closed.
    pub window_size: Option<[f32; 2]>,
    /// The window's outer top-left corner (physical pixels) when it last
    /// closed; restored only onto a connected monitor (`placement`).
    pub window_pos: Option<[i32; 2]>,
    /// The view (`browse`, `actors`, `media`, `extract`) and preview tab
    /// (`overview`, `textures`, ...) open when the app last closed.
    pub mode: Option<String>,
    pub tab: Option<String>,
    /// Disc images and extracted folders opened recently, newest first, at
    /// most [`RECENT_MAX`] ([`Settings::remember_recent`]).
    pub recent: Vec<PathBuf>,
    /// The colour theme.
    pub theme: Theme,
    /// The UI zoom factor (`Ctrl+=` / `Ctrl+-` / `Ctrl+0`); `None` is 1.0.
    pub zoom: Option<f32>,
}

/// How many recent sources the open menu keeps.
pub const RECENT_MAX: usize = 8;

/// The colour theme choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow the desktop's light / dark preference.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];

    pub fn name(self) -> &'static str {
        match self {
            Theme::System => "Follow system",
            Theme::Light => "Light",
            Theme::Dark => "Dark",
        }
    }
}

/// The zoom range the settings file accepts; anything else (a hand edit, a
/// corrupt file) falls back to 1.0.
pub fn sane_zoom(z: Option<f32>) -> f32 {
    z.filter(|z| z.is_finite() && (0.5..=3.0).contains(z))
        .unwrap_or(1.0)
}

impl Settings {
    /// Put `path` at the top of the recent list (moving it if already
    /// there), keeping at most [`RECENT_MAX`].
    pub fn remember_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT_MAX);
    }

    /// Drop recent entries whose file or folder no longer exists. `true`
    /// when the list changed.
    pub fn prune_recent(&mut self) -> bool {
        let before = self.recent.len();
        self.recent.retain(|p| p.exists());
        self.recent.len() != before
    }

    /// Remove `path` from the recent list.
    pub fn forget_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
    }

    /// The default settings file, or `None` when the platform has no config
    /// folder.
    pub fn default_path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("LEGAIA_STUDIO_SETTINGS") {
            return Some(PathBuf::from(p));
        }
        dirs::config_dir().map(|d| d.join("legaia-studio").join("settings.json"))
    }

    /// Load from `path`; a missing or unreadable file gives the defaults.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_missing_fields() {
        let dir =
            std::env::temp_dir().join(format!("legaia-studio-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let s = Settings {
            last_source: Some(PathBuf::from("/x/disc.bin")),
            skip_png: true,
            ..Default::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        std::fs::write(&path, r#"{"skip_verify": true}"#).unwrap();
        let partial = Settings::load(&path);
        assert!(partial.skip_verify && partial.last_source.is_none());
        assert_eq!(
            Settings::load(&dir.join("absent.json")),
            Settings::default()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_list_is_most_recent_first_capped_and_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        let paths: Vec<PathBuf> = (0..RECENT_MAX + 2)
            .map(|i| dir.path().join(format!("d{i}.bin")))
            .collect();
        for p in &paths {
            std::fs::write(p, b"").unwrap();
            s.remember_recent(p);
        }
        assert_eq!(s.recent.len(), RECENT_MAX);
        assert_eq!(s.recent[0], paths[RECENT_MAX + 1]);
        // Re-opening moves an entry to the top instead of duplicating it.
        s.remember_recent(&paths[5]);
        assert_eq!(s.recent[0], paths[5]);
        assert_eq!(s.recent.iter().filter(|p| **p == paths[5]).count(), 1);
        // A file that has gone is dropped; a removed entry stays removed.
        std::fs::remove_file(&paths[5]).unwrap();
        assert!(s.prune_recent());
        assert!(!s.recent.contains(&paths[5]));
        assert!(!s.prune_recent());
        let gone = s.recent[0].clone();
        s.forget_recent(&gone);
        assert!(!s.recent.contains(&gone));
    }

    #[test]
    fn theme_and_zoom_round_trip_and_bad_zoom_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = Settings {
            theme: Theme::Light,
            zoom: Some(1.3),
            ..Default::default()
        };
        s.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"light\""), "{text}");
        assert_eq!(Settings::load(&path), s);
        assert_eq!(sane_zoom(Some(1.3)), 1.3);
        assert_eq!(sane_zoom(Some(f32::NAN)), 1.0);
        assert_eq!(sane_zoom(Some(40.0)), 1.0);
        assert_eq!(sane_zoom(None), 1.0);
    }
}

//! Remembered choices, stored as JSON in the per-user config folder
//! (`~/.config/legaia-studio/settings.json` on Linux,
//! `%APPDATA%\legaia-studio\` on Windows,
//! `~/Library/Application Support/legaia-studio/` on macOS).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
}

impl Settings {
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
}

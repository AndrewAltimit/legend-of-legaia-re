//! Remembered choices, stored as JSON in the per-user config folder
//! (`~/.config/legaia-explorer/settings.json` on Linux,
//! `%APPDATA%\legaia-explorer\` on Windows,
//! `~/Library/Application Support/legaia-explorer/` on macOS).

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
    /// Extract tab: leave out the sweep that writes every texture as PNG.
    /// Named like the other skips so a settings file without it runs the
    /// sweep: without it a whole-disc extraction writes almost no images.
    pub skip_all_textures: bool,
    /// The window's inner size (logical points) when it last closed.
    pub window_size: Option<[f32; 2]>,
    /// The window's outer top-left corner (physical pixels) when it last
    /// closed; restored only onto a connected monitor (`placement`).
    pub window_pos: Option<[i32; 2]>,
    /// The view (`browse`, `actors`, `media`, `extract`) and preview tab
    /// (`overview`, `textures`, ...) open when the app last closed.
    pub mode: Option<String>,
    pub tab: Option<String>,
    /// The PROT index selected in Browse when the app last closed, selected
    /// again (and scrolled to) when the next start reopens a disc.
    pub entry: Option<u32>,
    /// The character or monster open in *Characters & monsters* when the app
    /// last closed: `char:<slot>` or `monster:<id>`, the
    /// `LEGAIA_EXPLORER_ACTOR` form.
    pub actor: Option<String>,
    /// The ISO path open in *Movies & voice* when the app last closed.
    pub media: Option<String>,
    /// The width of each view's left panel (points), by view key, when the
    /// app last closed. A hand edit is held to the panel's limits on use.
    pub side_widths: std::collections::BTreeMap<String, f32>,
    /// Disc images and extracted folders opened recently, newest first, at
    /// most [`RECENT_MAX`] ([`Settings::remember_recent`]).
    pub recent: Vec<PathBuf>,
    /// The colour theme.
    pub theme: Theme,
    /// The UI zoom factor (`Ctrl+=` / `Ctrl+-` / `Ctrl+0`); `None` is 1.0.
    pub zoom: Option<f32>,
    /// The Browse view's Overview tab shows its technical details (archive
    /// index, CDNAME block, sectors, chunk table) open.
    pub overview_details: bool,
    /// Audio preview volume, percent of the full mixer output (View menu);
    /// `None` is [`legaia_engine_audio::DESKTOP_MASTER_VOLUME_DEFAULT`].
    pub volume: Option<u8>,
}

impl Settings {
    /// The audio preview volume in percent, `0..=100`.
    pub fn volume_percent(&self) -> u8 {
        self.volume
            .map_or(legaia_engine_audio::DESKTOP_MASTER_VOLUME_DEFAULT, |v| {
                v.min(100)
            })
    }
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
/// The smallest window (points at 100% zoom) every view is laid out for.
pub const MIN_WINDOW: [f32; 2] = [720.0, 480.0];

/// The zoom a window of `width` x `height` (points at 100%) can carry: the
/// asked-for `wanted`, held down so the window stays at least [`MIN_WINDOW`]
/// in zoomed points. Past that the views' toolbars wrap and take the height
/// the 3D view and the lists live in. Zooming out is never held back.
pub fn fit_zoom(wanted: f32, width: f32, height: f32) -> f32 {
    if width <= 0.0 || height <= 0.0 {
        // No window size yet.
        return wanted;
    }
    let room = (width / MIN_WINDOW[0]).min(height / MIN_WINDOW[1]).max(1.0);
    wanted.min(room)
}

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
    /// folder. A settings file left by the app's earlier name
    /// (`legaia-studio`) is copied across on first use, so a renamed
    /// install keeps its recent list and preferences.
    pub fn default_path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("LEGAIA_EXPLORER_SETTINGS") {
            return Some(PathBuf::from(p));
        }
        let config = dirs::config_dir()?;
        let path = config.join("legaia-explorer").join("settings.json");
        let legacy = config.join("legaia-studio").join("settings.json");
        if !path.exists() && legacy.exists() {
            let _ = std::fs::create_dir_all(config.join("legaia-explorer"));
            let _ = std::fs::copy(&legacy, &path);
        }
        Some(path)
    }

    /// Load from `path`; a missing or unreadable file gives the defaults.
    /// [`Settings::open`] is the start-up form that also keeps a broken file.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Load from `path` at start-up. A missing file is the first run and
    /// gives the defaults quietly. A file that does not parse is set aside
    /// as `settings.json.broken` ([`legaia_app_theme::settings_file`]) so
    /// the next save cannot overwrite the recent list and preferences in it,
    /// and the result carries the sentence that tells the user. When the
    /// file cannot be read or moved, [`Opened::keep_file`] is `false`: the
    /// app then runs on the defaults without writing over it.
    pub fn open(path: &Path) -> Opened {
        use legaia_app_theme::settings_file::{set_aside, set_aside_notice};
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Opened::fresh(),
            Err(e) => {
                return Opened {
                    settings: Self::default(),
                    notice: Some(set_aside_notice(path, &e.to_string(), None)),
                    keep_file: false,
                };
            }
        };
        let err = match serde_json::from_str(&text) {
            Ok(settings) => {
                return Opened {
                    settings,
                    ..Opened::fresh()
                };
            }
            Err(e) => e,
        };
        log::warn!("settings: {} does not parse ({err})", path.display());
        let (kept, keep_file) = match set_aside(path) {
            Ok(kept) => (Some(kept), true),
            Err(e) => {
                log::warn!("settings: could not set {} aside ({e})", path.display());
                (None, false)
            }
        };
        Opened {
            settings: Self::default(),
            notice: Some(set_aside_notice(path, &err.to_string(), kept.as_deref())),
            keep_file,
        }
    }

    /// Write the settings, whole or not at all
    /// ([`legaia_app_theme::settings_file::write_atomic`]).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        legaia_app_theme::settings_file::write_atomic(path, json.as_bytes())
    }
}

/// What [`Settings::open`] found.
#[derive(Debug)]
pub struct Opened {
    pub settings: Settings,
    /// A sentence for the user when the file was not usable.
    pub notice: Option<String>,
    /// Whether later saves may write the file (`false` when it could be
    /// neither read nor moved aside).
    pub keep_file: bool,
}

impl Opened {
    fn fresh() -> Self {
        Self {
            settings: Settings::default(),
            notice: None,
            keep_file: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the views reopen on survives a save and a load, and a settings
    /// file from before these keys loads without them.
    #[test]
    fn view_memory_round_trips_and_older_files_load() {
        let mut s = Settings {
            actor: Some("monster:10".into()),
            media: Some("MOV/MV1.STR".into()),
            ..Default::default()
        };
        s.side_widths.insert("browse".into(), 512.0);
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&text).unwrap(), s);
        let old: Settings = serde_json::from_str(r#"{"skip_png":true}"#).unwrap();
        assert!(old.actor.is_none() && old.media.is_none() && old.side_widths.is_empty());
    }

    #[test]
    fn round_trips_and_tolerates_missing_fields() {
        let dir =
            std::env::temp_dir().join(format!("legaia-explorer-settings-{}", std::process::id()));
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

    /// A settings file that does not parse is moved aside, not overwritten
    /// by the defaults the next save writes, and the user is told where it
    /// went; a good file and a missing one open quietly.
    #[test]
    fn a_broken_settings_file_is_kept_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let fresh = Settings::open(&path);
        assert!(fresh.notice.is_none() && fresh.keep_file);
        let s = Settings {
            skip_png: true,
            ..Default::default()
        };
        s.save(&path).unwrap();
        let good = Settings::open(&path);
        assert_eq!(good.settings, s);
        assert!(good.notice.is_none());
        let broken = r#"{"recent": ["/x/disc.bin"], "skip_png": tru"#;
        std::fs::write(&path, broken).unwrap();
        let opened = Settings::open(&path);
        assert_eq!(opened.settings, Settings::default());
        assert!(opened.keep_file);
        let notice = opened.notice.unwrap();
        let kept = dir.path().join("settings.json.broken");
        assert!(notice.contains(&kept.display().to_string()), "{notice}");
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), broken);
        // The defaults the app now saves land beside the backup.
        opened.settings.save(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), broken);
        assert_eq!(Settings::load(&path), Settings::default());
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

    /// The preview volume starts at a quarter of the full mix, and a saved
    /// value (or an old file without one) loads as written.
    #[test]
    fn volume_defaults_to_a_quarter_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(Settings::default().volume_percent(), 25);
        std::fs::write(&path, "{}").unwrap();
        assert_eq!(Settings::load(&path).volume_percent(), 25);
        let s = Settings {
            volume: Some(60),
            ..Default::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).volume_percent(), 60);
        let loud = Settings {
            volume: Some(250),
            ..Default::default()
        };
        assert_eq!(loud.volume_percent(), 100);
    }

    #[test]
    fn zoom_is_held_to_what_the_window_has_room_for() {
        // The minimum window has no room to zoom in: every view is laid out
        // for at least that many points.
        assert_eq!(fit_zoom(1.25, 720.0, 480.0), 1.0);
        // Either axis can be the one short of room.
        assert_eq!(fit_zoom(2.0, 1440.0, 600.0), 1.25);
        assert_eq!(fit_zoom(2.0, 1080.0, 1200.0), 1.5);
        // Room to spare gives what was asked for; zooming out always works.
        assert_eq!(fit_zoom(1.25, 1280.0, 820.0), 1.25);
        assert_eq!(fit_zoom(0.8, 720.0, 480.0), 0.8);
        // Before the window has a size there is nothing to hold it to.
        assert_eq!(fit_zoom(1.25, 0.0, 0.0), 1.25);
    }

    #[test]
    fn a_settings_file_without_the_texture_sweep_key_runs_it() {
        let s: Settings = serde_json::from_str(r#"{"all_textures": false}"#).unwrap();
        assert!(!s.skip_all_textures);
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

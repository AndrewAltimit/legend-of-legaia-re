//! Declarative scenario manifest.
//!
//! `scripts/mednafen/scenarios.toml` maps each `.mc{0..9}` save state to a
//! labelled scenario with:
//!   - a human-readable description ("loading into a new area"),
//!   - the recommended overlay slice range,
//!   - watchpoint hints (regions of interest to diff against),
//!   - downstream artefacts (output Ghidra program label, CSV path, etc).
//!
//! The manifest is a single source of truth consumed by both the CLI
//! (`mednafen-state scenarios`) and the shell scripts under
//! `scripts/mednafen/`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioManifest {
    /// Mednafen `.mc{0..9}` slot index → scenario.
    pub scenarios: Vec<Scenario>,
    /// User-overridable defaults.
    #[serde(default)]
    pub defaults: ScenarioDefaults,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ScenarioDefaults {
    /// Default mednafen `mcs/` directory. If unset, callers must pass
    /// `--mcs-dir` or set `LEGAIA_MEDNAFEN_DIR`.
    pub mcs_dir: Option<PathBuf>,
    /// Default save-state filename pattern. `{slot}` is substituted with
    /// the slot index (0..9).
    #[serde(default = "default_pattern")]
    pub filename_pattern: String,
}

fn default_pattern() -> String {
    "Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.mc{slot}".to_owned()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Scenario {
    /// Mednafen save-state slot index (0..9).
    pub slot: u8,
    /// Short label used as overlay program name (e.g. `area_load_early`).
    pub label: String,
    /// Human-readable description.
    pub description: String,
    /// Topics this scenario informs (free-text labels - e.g.
    /// `["scene_bundle preamble", "navmesh"]`).
    #[serde(default)]
    pub topics: Vec<String>,
    /// Recommended overlay slice (PSX virtual addresses).
    #[serde(default)]
    pub overlay_slice: Option<OverlaySlice>,
    /// Watchpoint regions to diff against neighbouring scenarios.
    #[serde(default)]
    pub watchpoints: Vec<WatchpointSpec>,
    /// Sister-slot indices to diff against (e.g. an area-load scenario
    /// might pair with `[1, 2, 3]` so `auto-capture.sh` produces the full
    /// progressive diff set).
    #[serde(default)]
    pub diff_against: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OverlaySlice {
    pub start: u32,
    pub end: u32,
}

impl Default for OverlaySlice {
    fn default() -> Self {
        Self {
            start: 0x801C0000,
            end: 0x80200000,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WatchpointSpec {
    pub label: String,
    pub start: u32,
    pub end: u32,
    /// Free-text describing what writes to this region are expected to mean.
    #[serde(default)]
    pub hint: String,
}

impl ScenarioManifest {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading scenario manifest {}", path.display()))?;
        Self::parse_toml(&text)
    }

    pub fn parse_toml(text: &str) -> Result<Self> {
        let m: Self = toml::from_str(text).context("parsing scenarios.toml")?;
        Ok(m)
    }

    pub fn by_slot(&self, slot: u8) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.slot == slot)
    }

    pub fn by_label(&self, label: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.label == label)
    }

    /// Resolve the on-disk path for a scenario given its slot.
    /// Honours `mcs_dir` from the manifest, then `LEGAIA_MEDNAFEN_DIR`,
    /// then `~/.mednafen/mcs/`.
    pub fn save_path(&self, slot: u8) -> Result<PathBuf> {
        let dir: PathBuf = if let Some(d) = &self.defaults.mcs_dir {
            d.clone()
        } else if let Ok(d) = std::env::var("LEGAIA_MEDNAFEN_DIR") {
            PathBuf::from(d)
        } else {
            let home = std::env::var("HOME").context("no HOME and no mcs_dir set")?;
            PathBuf::from(home).join(".mednafen").join("mcs")
        };
        let filename = self
            .defaults
            .filename_pattern
            .replace("{slot}", &slot.to_string());
        Ok(dir.join(filename))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_toml() -> &'static str {
        r#"
[defaults]
filename_pattern = "test.{slot}"

[[scenarios]]
slot = 0
label = "title"
description = "title screen"
topics = ["move-table boot path"]

[scenarios.overlay_slice]
start = 0x801C0000
end = 0x80200000

[[scenarios.watchpoints]]
label = "battle_actor_pool"
start = 0x801C9370
end = 0x801C93B0
hint = "FUN_8004E2F0 fills slots 0..7"

[[scenarios]]
slot = 1
label = "area_load_early"
description = "loading into a new area"
diff_against = [2, 3]
"#
    }

    #[test]
    fn parses_minimal_manifest() {
        let m = ScenarioManifest::parse_toml(sample_toml()).unwrap();
        assert_eq!(m.scenarios.len(), 2);
        let title = m.by_slot(0).unwrap();
        assert_eq!(title.label, "title");
        assert_eq!(title.topics, vec!["move-table boot path"]);
        let slice = title.overlay_slice.as_ref().unwrap();
        assert_eq!(slice.start, 0x801C0000);
        assert_eq!(slice.end, 0x80200000);
        assert_eq!(title.watchpoints.len(), 1);
        assert_eq!(title.watchpoints[0].label, "battle_actor_pool");

        let load = m.by_slot(1).unwrap();
        assert_eq!(load.diff_against, vec![2, 3]);
    }

    #[test]
    fn lookup_by_label() {
        let m = ScenarioManifest::parse_toml(sample_toml()).unwrap();
        assert!(m.by_label("title").is_some());
        assert!(m.by_label("missing").is_none());
    }

    #[test]
    fn save_path_uses_pattern() {
        let m = ScenarioManifest::parse_toml(sample_toml()).unwrap();
        // Force LEGAIA_MEDNAFEN_DIR via env so the test is hermetic.
        // SAFETY: we control the test harness environment.
        unsafe {
            std::env::set_var("LEGAIA_MEDNAFEN_DIR", "/tmp/scenario_test");
        }
        let p = m.save_path(3).unwrap();
        assert_eq!(p, PathBuf::from("/tmp/scenario_test/test.3"));
        unsafe {
            std::env::remove_var("LEGAIA_MEDNAFEN_DIR");
        }
    }
}

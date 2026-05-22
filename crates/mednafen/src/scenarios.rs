//! Declarative scenario manifest.
//!
//! `scripts/scenarios.toml` is the unified save-state catalogue. Each
//! scenario maps an investigation label to:
//!   - a human-readable description ("loading into a new area"),
//!   - mednafen's `.mc{0..9}` slot + the recommended overlay slice range,
//!   - watchpoint hints (regions of interest to diff against),
//!   - downstream artefacts (output Ghidra program label, CSV path, etc),
//!   - cross-emulator save-state paths so PCSX-Redux probes and
//!     Duckstation captures can resolve the same scenario by name,
//!   - optional phase / expected-game-mode / expected-active-scene /
//!     ram-fingerprint metadata used by `manage-states.py validate` to
//!     detect save-state drift.
//!
//! The manifest is a single source of truth consumed by:
//!   - `mednafen-state` CLI + the shell scripts under `scripts/mednafen/`,
//!   - `run_probe.sh --scenario <label>` for PCSX-Redux probes,
//!   - `scripts/manage-states.py` for cross-emulator state validation.

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

    // ------------------------------------------------------------------
    // Cross-emulator state paths. Optional — a scenario may be captured
    // in one emulator and not another. Paths may contain `$HOME` or
    // `~`; consumers expand them at use time.
    /// PCSX-Redux save-state path (resolved by `run_probe.sh --scenario`).
    #[serde(default)]
    pub pcsx_redux_sstate: Option<PathBuf>,
    /// Duckstation `.sav` path.
    #[serde(default)]
    pub duckstation_sav: Option<PathBuf>,

    // ------------------------------------------------------------------
    // State-validation metadata. Populated by `manage-states.py
    // fingerprint`; checked by `manage-states.py validate` to detect
    // save-state drift.
    /// Boot-arc phase: `boot` / `title` / `menu` / `field` / `battle` /
    /// `world_map` / `cutscene`.
    #[serde(default)]
    pub phase: Option<String>,
    /// Expected `_DAT_8007B83C` (game-mode register) value at scenario
    /// start. `0x1A` = StrInit, etc.
    #[serde(default)]
    pub expected_game_mode: Option<u8>,
    /// Expected CDNAME label of the active scene (e.g. `map01`, `town01`).
    #[serde(default)]
    pub expected_active_scene: Option<String>,
    /// SHA-256 of the first 64 KiB of main RAM after the save-state
    /// load settles (default 60 vsyncs). Reproducible across emulators
    /// modulo non-deterministic uninitialised regions — validates the
    /// save state hasn't drifted vs the committed manifest.
    #[serde(default)]
    pub ram_fingerprint_sha256: Option<String>,
    /// SHA-256 of the **save-state file bytes** of an immutable copy stashed
    /// in `saves/library/<emulator>/<fingerprint>.<ext>`. When set, consumers
    /// resolve this stable copy in preference to the wipe-prone live `.mc{slot}`
    /// (live emulator slots get overwritten as the user plays). Mirrors the
    /// `backup_fingerprint` field `scripts/manage-states.py` reads.
    #[serde(default)]
    pub backup_fingerprint: Option<String>,
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

    /// Resolve the mednafen save for a scenario, **preferring an immutable
    /// library backup** over the wipe-prone live slot.
    ///
    /// If the scenario carries a [`Scenario::backup_fingerprint`] and a file
    /// in `library_dir/mednafen/` has a stem starting with that fingerprint,
    /// that stable copy is returned. Otherwise this falls back to
    /// [`Self::save_path`] for the scenario's live `.mc{slot}`. Mirrors
    /// `scripts/manage-states.py`'s `mednafen_path`. Pass `library_dir = None`
    /// to skip the library and use the live slot only.
    pub fn mednafen_save_path(
        &self,
        scenario: &Scenario,
        library_dir: Option<&Path>,
    ) -> Result<PathBuf> {
        if let (Some(lib), Some(fp)) = (library_dir, scenario.backup_fingerprint.as_deref())
            && let Some(p) = library_backup_for("mednafen", lib, fp)
        {
            return Ok(p);
        }
        self.save_path(scenario.slot)
    }
}

/// Look up an immutable library backup by `(emulator, fingerprint)`: the first
/// file in `library_dir/<emulator>/` whose stem starts with `fingerprint`.
/// Returns `None` when the directory or a matching file is absent.
pub fn library_backup_for(
    emulator: &str,
    library_dir: &Path,
    fingerprint: &str,
) -> Option<PathBuf> {
    let emu_dir = library_dir.join(emulator);
    let entries = std::fs::read_dir(&emu_dir).ok()?;
    let mut hits: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|stem| stem.starts_with(fingerprint))
        })
        .collect();
    hits.sort();
    hits.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes the tests that mutate the process-global `LEGAIA_MEDNAFEN_DIR`
    /// env var so they don't race each other under the default parallel test
    /// runner. Poison is recovered (a panicking test still releases the guard
    /// logically) so one failure doesn't cascade into spurious failures.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

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

    // This test and `save_path_uses_pattern` both mutate the process-global
    // `LEGAIA_MEDNAFEN_DIR` env var, so they share `ENV_LOCK` to serialize
    // against each other under the default parallel test runner.
    #[test]
    fn mednafen_save_path_prefers_library_backup_then_falls_back_to_live_slot() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let m = ScenarioManifest::parse_toml(sample_toml()).unwrap();
        // Hermetic mcs dir for the live-slot fallback.
        // SAFETY: test-controlled environment, serialized by ENV_LOCK.
        unsafe {
            std::env::set_var("LEGAIA_MEDNAFEN_DIR", "/tmp/scenario_test_mcs");
        }

        // A stable library backup dir with one mednafen save.
        let tmp = std::env::temp_dir().join(format!("legaia_lib_{}", std::process::id()));
        let emu = tmp.join("mednafen");
        std::fs::create_dir_all(&emu).unwrap();
        let fp = "deadbeefcafef00d";
        std::fs::write(emu.join(format!("{fp}.mcr")), b"x").unwrap();

        // Scenario with a backup_fingerprint resolves to the library copy.
        let mut scn = m.scenarios[0].clone();
        scn.backup_fingerprint = Some(fp.to_string());
        let p = m.mednafen_save_path(&scn, Some(&tmp)).unwrap();
        assert_eq!(p, emu.join(format!("{fp}.mcr")));

        // Without a backup_fingerprint, it falls back to the live slot path.
        let scn_no_fp = m.scenarios[0].clone();
        let p = m.mednafen_save_path(&scn_no_fp, Some(&tmp)).unwrap();
        assert_eq!(p, PathBuf::from("/tmp/scenario_test_mcs/test.0"));

        // A backup_fingerprint with no matching library file also falls back.
        let mut scn_missing = m.scenarios[0].clone();
        scn_missing.backup_fingerprint = Some("0000notpresent".to_string());
        let p = m.mednafen_save_path(&scn_missing, Some(&tmp)).unwrap();
        assert_eq!(p, PathBuf::from("/tmp/scenario_test_mcs/test.0"));

        std::fs::remove_dir_all(&tmp).ok();
        unsafe {
            std::env::remove_var("LEGAIA_MEDNAFEN_DIR");
        }
    }

    #[test]
    fn save_path_uses_pattern() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let m = ScenarioManifest::parse_toml(sample_toml()).unwrap();
        // Force LEGAIA_MEDNAFEN_DIR via env so the test is hermetic.
        // SAFETY: we control the test harness environment, serialized by ENV_LOCK.
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

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
//!
//! The schema is game-agnostic. What a manifest leaves out of `[defaults]`
//! is filled from a [`GameProfile`]: [`ScenarioManifest::parse_toml`] uses
//! Legaia's ([`crate::game_anchors::SCENARIO_DEFAULTS`]), and
//! [`ScenarioManifest::parse_toml_for`] takes any other.

use anyhow::{Context, Result};

use crate::profile::GameProfile;
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
    /// Environment variable naming the `mcs/` directory, consulted when
    /// `mcs_dir` is unset. `None` skips straight to `~/.mednafen/mcs/`.
    #[serde(default = "default_mcs_dir_env")]
    pub mcs_dir_env: Option<String>,
}

fn default_pattern() -> String {
    crate::game_anchors::SCENARIO_DEFAULTS
        .filename_pattern
        .to_owned()
}

fn default_mcs_dir_env() -> Option<String> {
    crate::game_anchors::SCENARIO_DEFAULTS
        .mcs_dir_env
        .map(str::to_owned)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Scenario {
    /// Mednafen save-state slot index (0..9). Absent (or the `255`
    /// sentinel `scripts/manage-states.py` shares) for fingerprint-only
    /// scenarios that live solely as immutable library backups - resolve
    /// those via [`ScenarioManifest::library_save_path`].
    #[serde(default)]
    pub slot: Option<u8>,
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
    // Cross-emulator state paths. Optional - a scenario may be captured
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
    /// modulo non-deterministic uninitialised regions - validates the
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
    /// The patch family whose executable the state holds resident, when it
    /// was made on a patched disc (`scripts/pcsx-redux/patch_taint_audit.py`
    /// tags it). Loading such a state replays that build's `SCUS_942.54`, so
    /// anything the patch writes is not retail behaviour.
    #[serde(default)]
    pub resident_patch: Option<String>,
    /// File name of the patched disc image the state was made on, when the
    /// image is known (`legaia-patcher` names its output
    /// `<input>.legaia-patcher-<stamp>.bin`). The retail comparison runs the
    /// engine on that image's files; the image itself is never committed.
    #[serde(default)]
    pub resident_disc: Option<String>,
    /// Battle combatant fields the capture probe wrote **after** battle init,
    /// named the way the retail-compare corpus names them (`p0.mp_max`,
    /// `m1.hp`). Such a field holds the probe's value, not anything the game
    /// computed, so the corpus does not score it.
    #[serde(default)]
    pub ram_injected: Vec<String>,
    /// File name of the memory-card image the state was captured with,
    /// under `saves/library/cards/` (gitignored, like the states). A state
    /// on a card screen shows that card's contents, so the retail
    /// comparison mounts the same card on the engine side.
    #[serde(default)]
    pub memory_card: Option<String>,
    /// RAM words the capture probe wrote before the state was saved, as
    /// `"<va> = <value> (<why>)"` - a poked state is not pure retail play,
    /// and the poke is named here so nothing downstream reads it as such.
    #[serde(default)]
    pub ram_poked: Vec<String>,
    /// The door a walked capture last came through: the scene it was taken
    /// from and, when that scene has several into this one, the
    /// partition-2 record. A state reached through a door carries history a
    /// card load does not replay (the music a door keeps), so a seeding
    /// harness may enter the source scene and take the door instead.
    #[serde(default)]
    pub arrival: Option<ScenarioArrival>,
}

/// [`Scenario::arrival`]: the door a capture came through.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioArrival {
    /// CDNAME label of the scene the door is in.
    pub from: String,
    /// The door's partition-2 record in `from`'s MAN.
    #[serde(default)]
    pub door: Option<u16>,
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

    /// Parse a manifest, filling absent defaults from the Legaia profile.
    pub fn parse_toml(text: &str) -> Result<Self> {
        let m: Self = toml::from_str(text).context("parsing scenarios.toml")?;
        Ok(m)
    }

    /// [`Self::parse_toml`] for another game: a `[defaults]` field the text
    /// leaves out takes `profile`'s value rather than Legaia's.
    pub fn parse_toml_for(text: &str, profile: &dyn GameProfile) -> Result<Self> {
        let mut m = Self::parse_toml(text)?;
        let table: toml::Table = toml::from_str(text).context("parsing scenarios.toml")?;
        let given = |key: &str| {
            table
                .get("defaults")
                .and_then(|d| d.as_table())
                .is_some_and(|d| d.contains_key(key))
        };
        let defaults = profile.scenario_defaults();
        if !given("filename_pattern") {
            m.defaults.filename_pattern = defaults.filename_pattern.to_owned();
        }
        if !given("mcs_dir_env") {
            m.defaults.mcs_dir_env = defaults.mcs_dir_env.map(str::to_owned);
        }
        Ok(m)
    }

    /// [`Self::from_path`] with [`Self::parse_toml_for`]'s defaults.
    pub fn from_path_for(path: impl AsRef<Path>, profile: &dyn GameProfile) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading scenario manifest {}", path.display()))?;
        Self::parse_toml_for(&text, profile)
    }

    pub fn by_slot(&self, slot: u8) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.slot == Some(slot))
    }

    pub fn by_label(&self, label: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.label == label)
    }

    /// Resolve the on-disk path for a scenario given its slot.
    /// Honours `mcs_dir` from the manifest, then the `mcs_dir_env` variable
    /// (`LEGAIA_MEDNAFEN_DIR` by default), then `~/.mednafen/mcs/`.
    pub fn save_path(&self, slot: u8) -> Result<PathBuf> {
        let from_env = self
            .defaults
            .mcs_dir_env
            .as_deref()
            .and_then(|k| std::env::var(k).ok());
        let dir: PathBuf = if let Some(d) = &self.defaults.mcs_dir {
            d.clone()
        } else if let Some(d) = from_env {
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
        self.save_path(scenario.live_slot()?)
    }

    /// Library-backup-**only** resolution: like [`Self::mednafen_save_path`]
    /// but never falls back to the wipe-prone live `.mc{slot}`. A scenario
    /// without a mednafen library backup (no fingerprint, or a PCSX-only
    /// fingerprint) yields `None`.
    ///
    /// Parity / trace oracles must use this: under the live-slot fallback a
    /// PCSX-only scenario silently resolves to whatever the live slot
    /// currently holds, polluting a scene group's static mask or diffing
    /// the engine against an unrelated game state.
    pub fn library_save_path(&self, scenario: &Scenario, library_dir: &Path) -> Option<PathBuf> {
        let fp = scenario.backup_fingerprint.as_deref()?;
        library_backup_for("mednafen", library_dir, fp)
    }
}

impl Scenario {
    /// The scenario's live mednafen `.mc{slot}` index, or an error for a
    /// fingerprint-only scenario (no `slot` field, or the `255` "no live
    /// slot" sentinel). Live-slot consumers (watch, trace, vram) call
    /// this; library-backed oracles never need it.
    pub fn live_slot(&self) -> Result<u8> {
        match self.slot {
            Some(n) if n != 255 => Ok(n),
            _ => anyhow::bail!(
                "scenario {:?} has no live mc slot (fingerprint-only library capture); \
                 resolve it through its backup_fingerprint library copy",
                self.label
            ),
        }
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
    fn fingerprint_only_scenario_parses_and_rejects_live_slot_use() {
        let toml = r#"
[[scenarios]]
label = "library_only"
backup_fingerprint = "abcd"
description = "captured straight into the library, no live mc slot"
"#;
        let m = ScenarioManifest::parse_toml(toml).expect("slot-less entry parses");
        let s = m.by_label("library_only").expect("present");
        assert_eq!(s.slot, None);
        assert!(m.by_slot(0).is_none(), "no live slot to match");
        let err = s.live_slot().unwrap_err().to_string();
        assert!(
            err.contains("library_only"),
            "error names the scenario: {err}"
        );
        let err = m.mednafen_save_path(s, None).unwrap_err().to_string();
        assert!(
            err.contains("no live mc slot"),
            "live fallback refuses: {err}"
        );
    }

    #[test]
    fn slot_255_is_the_no_live_slot_sentinel() {
        let toml = r#"
[[scenarios]]
slot = 255
label = "sentinel"
description = "marker entry"
"#;
        let m = ScenarioManifest::parse_toml(toml).expect("parses");
        assert!(m.by_label("sentinel").unwrap().live_slot().is_err());
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

    #[test]
    fn another_games_profile_fills_the_absent_defaults() {
        struct OtherGame;
        impl GameProfile for OtherGame {
            fn name(&self) -> &str {
                "other"
            }
            fn scenario_defaults(&self) -> crate::profile::ScenarioProfile {
                crate::profile::ScenarioProfile {
                    filename_pattern: "Other Game (USA).0123.mc{slot}",
                    mcs_dir_env: Some("OTHER_GAME_MCS"),
                }
            }
        }
        let toml = r#"
[defaults]
mcs_dir = "/states"

[[scenarios]]
slot = 2
label = "boss"
description = "boss arena"
"#;
        let m = ScenarioManifest::parse_toml_for(toml, &OtherGame).unwrap();
        assert_eq!(
            m.defaults.filename_pattern,
            "Other Game (USA).0123.mc{slot}"
        );
        assert_eq!(m.defaults.mcs_dir_env.as_deref(), Some("OTHER_GAME_MCS"));
        assert_eq!(
            m.save_path(2).unwrap(),
            PathBuf::from("/states/Other Game (USA).0123.mc2")
        );

        // A field the manifest sets wins over the profile.
        let m = ScenarioManifest::parse_toml_for(sample_toml(), &OtherGame).unwrap();
        assert_eq!(m.defaults.filename_pattern, "test.{slot}");

        // The legacy entry point keeps Legaia's defaults.
        let m = ScenarioManifest::parse_toml(toml).unwrap();
        assert_eq!(
            m.defaults.filename_pattern,
            crate::game_anchors::SCENARIO_DEFAULTS.filename_pattern
        );
        assert_eq!(
            m.defaults.mcs_dir_env.as_deref(),
            Some("LEGAIA_MEDNAFEN_DIR")
        );
    }
}

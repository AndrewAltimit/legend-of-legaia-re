//! Engine integration scenarios manifest.
//!
//! Mirror of [`scripts/scenarios.toml`](../../../scripts/scenarios.toml)
//! for the clean-room engine: a declarative list of (scene, frames-to-tick,
//! expected SaveFile SHA-256) tuples that a CI test runs headlessly to
//! catch state drift across release velocity.
//!
//! ## Manifest shape
//!
//! ```toml
//! [defaults]
//! frames = 5
//!
//! [[scenario]]
//! name = "town01_5_frames"
//! scene = "town01"
//! frames = 5
//! expected_save_sha256 = "deadbeef..."  # 64 hex chars
//! ```
//!
//! Each scenario boots [`BootSession::open`] with the configured scene
//! and audio off, ticks `frames` times, dumps the engine's SaveFile via
//! `World::save_full().write()`, and asserts the SHA-256 of that byte
//! stream matches `expected_save_sha256`. A `--bless` flow rewrites the
//! manifest with the observed hash for new scenarios.
//!
//! Scenarios with empty / missing `expected_save_sha256` are recorded
//! as "needs blessing" and the runner exits non-zero unless `bless`
//! mode is on. This forces each new scenario to be reviewed once
//! before it can drift silently.
//!
//! See [`docs/subsystems/engine.md`](../../../docs/subsystems/engine.md)
//! for the broader engine architecture and where this fits.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{BootConfig, BootSession};

/// Top-level manifest. Maps to a `scripts/engine/scenarios.toml` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenariosManifest {
    /// Optional defaults applied to every scenario when its own field
    /// is missing.
    #[serde(default)]
    pub defaults: ScenarioDefaults,
    /// All scenarios. The runner walks them in order.
    #[serde(default, rename = "scenario")]
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScenarioDefaults {
    /// Default `frames` value when a scenario doesn't set its own.
    #[serde(default)]
    pub frames: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Human-readable label, used in CI output.
    pub name: String,
    /// CDNAME scene to boot.
    pub scene: String,
    /// How many `BootSession::tick` calls to issue. Defaults to
    /// `defaults.frames` if absent (and finally to 5 if both are unset).
    #[serde(default)]
    pub frames: Option<u32>,
    /// Lower-case 64-char hex string. Empty means "not yet blessed".
    #[serde(default)]
    pub expected_save_sha256: String,
}

/// Result of running one scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub name: String,
    pub scene: String,
    pub frames: u32,
    /// Observed SHA-256 of the SaveFile byte stream.
    pub observed_sha256: String,
    /// Expected hash from the manifest. `None` if the manifest has an
    /// empty `expected_save_sha256` (i.e. unblessed).
    pub expected_sha256: Option<String>,
}

impl ScenarioResult {
    pub fn passed(&self) -> bool {
        match &self.expected_sha256 {
            Some(exp) => exp.eq_ignore_ascii_case(&self.observed_sha256),
            None => false,
        }
    }
}

const DEFAULT_FRAMES: u32 = 5;

impl ScenariosManifest {
    pub fn from_toml_str(s: &str) -> Result<Self> {
        toml::from_str(s).context("parse engine scenarios manifest")
    }

    pub fn from_toml_path(path: &Path) -> Result<Self> {
        let s = fs::read_to_string(path)
            .with_context(|| format!("read manifest {}", path.display()))?;
        Self::from_toml_str(&s)
    }

    pub fn to_toml_string(&self) -> Result<String> {
        toml::to_string_pretty(self).context("serialise engine scenarios manifest")
    }

    /// Resolve the effective frame-count for a scenario.
    pub fn effective_frames(&self, sc: &Scenario) -> u32 {
        sc.frames.or(self.defaults.frames).unwrap_or(DEFAULT_FRAMES)
    }
}

/// Run every scenario in `manifest` against an extracted PROT directory.
/// Returns one [`ScenarioResult`] per scenario, in manifest order.
pub fn run_all(manifest: &ScenariosManifest, extracted_root: &Path) -> Result<Vec<ScenarioResult>> {
    let mut out = Vec::with_capacity(manifest.scenarios.len());
    for sc in &manifest.scenarios {
        let frames = manifest.effective_frames(sc);
        let observed = run_one(extracted_root, &sc.scene, frames)
            .with_context(|| format!("run scenario '{}'", sc.name))?;
        let expected = if sc.expected_save_sha256.trim().is_empty() {
            None
        } else {
            Some(sc.expected_save_sha256.clone())
        };
        out.push(ScenarioResult {
            name: sc.name.clone(),
            scene: sc.scene.clone(),
            frames,
            observed_sha256: observed,
            expected_sha256: expected,
        });
    }
    Ok(out)
}

/// Boot the engine into `scene_name`, tick `frames` times, hash the
/// `SaveFile` byte stream, and return the lower-case hex digest.
pub fn run_one(extracted_root: &Path, scene_name: &str, frames: u32) -> Result<String> {
    let cfg = BootConfig {
        scene: scene_name.to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(extracted_root, &cfg)
        .with_context(|| format!("boot scene '{}'", scene_name))?;
    for _ in 0..frames {
        session
            .tick()
            .with_context(|| format!("tick scene '{}'", scene_name))?;
    }
    let bytes = session.host.world.save_full().write();
    Ok(hex_digest(&bytes))
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Bless the manifest at `path` with the observed hashes from a previous
/// `run_all`. Overwrites the file in place. Only updates rows where the
/// observed hash differs from the recorded one (so blessing is idempotent
/// when nothing has drifted).
pub fn bless(path: &Path, results: &[ScenarioResult]) -> Result<usize> {
    let mut manifest = ScenariosManifest::from_toml_path(path)?;
    let mut updated = 0;
    for sc in &mut manifest.scenarios {
        if let Some(r) = results.iter().find(|r| r.name == sc.name)
            && !sc
                .expected_save_sha256
                .eq_ignore_ascii_case(&r.observed_sha256)
        {
            sc.expected_save_sha256 = r.observed_sha256.clone();
            updated += 1;
        }
    }
    if updated > 0 {
        let s = manifest.to_toml_string()?;
        fs::write(path, s).with_context(|| format!("write blessed manifest {}", path.display()))?;
    }
    Ok(updated)
}

/// Discover the canonical manifest path. Default is
/// `scripts/engine/scenarios.toml` relative to the repo root, but tests
/// pass an absolute override.
pub fn default_manifest_path() -> PathBuf {
    PathBuf::from("scripts/engine/scenarios.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[defaults]
frames = 7

[[scenario]]
name = "town01_quick"
scene = "town01"
expected_save_sha256 = "abcd"

[[scenario]]
name = "town0c_blessed"
scene = "town0c"
frames = 12
expected_save_sha256 = "ef01"
"#;

    #[test]
    fn manifest_parses_with_defaults() {
        let m = ScenariosManifest::from_toml_str(SAMPLE).unwrap();
        assert_eq!(m.scenarios.len(), 2);
        assert_eq!(m.defaults.frames, Some(7));
        assert_eq!(m.effective_frames(&m.scenarios[0]), 7);
        assert_eq!(m.effective_frames(&m.scenarios[1]), 12);
    }

    #[test]
    fn unblessed_row_yields_none_expected() {
        let s = r#"
[[scenario]]
name = "fresh"
scene = "town01"
"#;
        let m = ScenariosManifest::from_toml_str(s).unwrap();
        assert!(m.scenarios[0].expected_save_sha256.is_empty());
    }

    #[test]
    fn scenario_result_passes_when_expected_matches() {
        let r = ScenarioResult {
            name: "x".into(),
            scene: "town01".into(),
            frames: 1,
            observed_sha256: "deadbeef".into(),
            expected_sha256: Some("DEADBEEF".into()),
        };
        assert!(r.passed());
    }

    #[test]
    fn scenario_result_fails_without_expected() {
        let r = ScenarioResult {
            name: "x".into(),
            scene: "town01".into(),
            frames: 1,
            observed_sha256: "deadbeef".into(),
            expected_sha256: None,
        };
        assert!(!r.passed());
    }
}

//! Helpers shared between the headless (`commands/*`) and windowed
//! (`window/*`) subcommand trees. Behavior-preserving extractions of
//! logic that was copy-pasted across the two `play` paths and the
//! several scene-inspection commands.

use anyhow::{Context, Result};
use legaia_engine_core::scene::{CutsceneMap, ProtIndex, Scene};
use legaia_engine_core::scene_resources::FIELD_SHARED_BLOCKS;
use legaia_engine_shell::{BootConfig, BootSession};
use std::path::{Path, PathBuf};

/// Load every [`FIELD_SHARED_BLOCKS`] scene that resolves against `index`,
/// returning the ones that loaded. Each miss is reported through `on_miss`
/// so callers keep their site-specific logging (a stderr warning, a
/// `log::warn!`, or a silent skip). Callers build their own
/// `Vec<&Scene>` from the result.
pub(crate) fn load_shared_scenes(
    index: &ProtIndex,
    mut on_miss: impl FnMut(&str, anyhow::Error),
) -> Vec<Scene> {
    let mut shared_scenes: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        match Scene::load(index, name) {
            Ok(s) => shared_scenes.push(s),
            Err(e) => on_miss(name, e),
        }
    }
    shared_scenes
}

/// Resolve the cutscene map (explicit `--cutscene-map` TOML override or the
/// heuristic default) plus the auto-resolved STR file for the requested
/// scene. Mirrors the identical prologue of both `play` paths: emits the
/// "cutscene-map loaded" info line when an explicit map was supplied, and
/// only auto-resolves an STR when the user passed neither `--str-file` nor
/// `--disc`. Returns the map (the windowed path reuses it for disc-mode STR
/// resolution) and the auto-resolved path; the caller still computes its own
/// `resolved_str` borrow from `str_file.or(auto_str.as_deref())`.
pub(crate) fn resolve_cutscene_map_and_str(
    cutscene_map_path: Option<&Path>,
    scene: &str,
    extracted_root: &Path,
    str_file: Option<&Path>,
    disc: Option<&Path>,
) -> Result<(CutsceneMap, Option<PathBuf>)> {
    let cutscene_map = if let Some(p) = cutscene_map_path {
        CutsceneMap::from_toml_path(p)
            .with_context(|| format!("load cutscene map {}", p.display()))?
    } else {
        CutsceneMap::default()
    };
    if cutscene_map_path.is_some() {
        eprintln!(
            "info: cutscene-map loaded with {} explicit entry/entries",
            cutscene_map.len()
        );
    }
    let auto_str = match (str_file, disc) {
        (Some(_), _) => None,
        (None, None) => cutscene_map
            .resolve(scene)
            .map(|rel| extracted_root.join(rel))
            .filter(|p| p.exists()),
        // Disc-mode resolution would need an ISO9660 read; punt.
        (None, Some(_)) => None,
    };
    Ok((cutscene_map, auto_str))
}

/// Open a [`BootSession`] for `scene`, selecting the disc-image or
/// extracted-root boot path from `disc`. Shared by both `play` entry points.
pub(crate) fn open_boot_session(
    scene: &str,
    enable_audio: bool,
    extracted_root: &Path,
    disc: Option<&Path>,
) -> Result<BootSession> {
    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio,
    };
    match disc {
        Some(disc_path) => BootSession::open_disc(disc_path, &cfg),
        None => BootSession::open(extracted_root, &cfg),
    }
}

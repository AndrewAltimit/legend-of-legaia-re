//! CLI-facing value enums + bundle/scene directory resolution helpers.

use anyhow::{Context, Result};
use clap::ValueEnum;
use legaia_prot::cdname;
use std::path::{Path, PathBuf};

/// Known scene-loader bundles, derived from static analysis of asset
/// loaders in SCUS_942.54 (see `ghidra/scripts/funcs/800520f0.txt` for
/// the battle path).
#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum Bundle {
    /// Battle / level_up / monster_se assets. Mirrors what `FUN_800520f0`
    /// loads at battle-scene init: sound_data + befect_data + player_data +
    /// sound_data2 (PROT entries 871-890). Includes the CLUT rows shared
    /// across all character bodies (e.g. row 484 lives in `0873_befect_data`).
    Battle,
}

impl Bundle {
    /// Return the `tim_scan/<entry>/` directories this bundle overlays.
    /// Skips entries that don't exist on disk.
    pub(crate) fn dirs(self, root: &Path) -> Vec<PathBuf> {
        let entries: &[&str] = match self {
            // Extraction entries 865..890 (directory names carry the
            // extractor's CDNAME-derived labels, which sit +2 from the
            // retail-semantic blocks - docs/formats/cdname.md numbering
            // space). FUN_800520f0 loads raw indices 0x367-0x36B =
            // extraction 869..873 (the befect cluster: etim/etmd/vdf/
            // efect) plus the player files (extraction 863..866). This
            // list is the empirically-tuned VRAM overlay set the battle
            // preset needs - observed that level_up hero meshes pull
            // CLUTs from both extraction 0866 (row 490) and 0873
            // (row 484, x=144); resist renumbering it without a visual
            // re-check.
            Bundle::Battle => &[
                "0865_battle_data",
                "0866_battle_data",
                "0867_battle_data",
                "0868_battle_data",
                "0869_monster_data",
                "0870_sound_data",
                "0871_sound_data",
                "0872_befect_data",
                "0873_befect_data",
                "0874_befect_data",
                "0875_befect_data",
                "0876_player_data",
                "0877_sound_data2",
                "0878_sound_data2",
                "0879_sound_data2",
                "0880_sound_data2",
                "0881_sound_data2",
                "0882_sound_data2",
                "0883_sound_data2",
                "0884_sound_data2",
                "0885_sound_data2",
                "0886_sound_data2",
                "0887_sound_data2",
                "0888_sound_data2",
                "0889_sound_data2",
                "0890_sound_data2",
            ],
        };
        let tim_root = root.join("tim_scan");
        entries
            .iter()
            .map(|e| tim_root.join(e))
            .filter(|p| p.is_dir())
            .collect()
    }
}

/// Resolve a CDNAME scene name to the list of `tim_scan/<entry>/` dirs
/// for every PROT entry in that block. Mirrors what the runtime field
/// loader (`FUN_8001f7c0` + `FUN_800255b8`) co-loads for one scene -
/// six file types per scene, all under the same CDNAME block.
///
/// Walks the `tim_scan/` directory tree to discover the actual entry
/// folder names (which include the index prefix, e.g. `0006_town01`).
/// Skips PROT indices in the block range that don't have a tim_scan dir
/// (e.g. stage-only entries with no TIMs).
pub(crate) fn scene_bundle_dirs(
    cdname_path: &Path,
    scene_name: &str,
    extracted_root: &Path,
) -> Result<Vec<PathBuf>> {
    let map = cdname::parse(cdname_path)
        .with_context(|| format!("parse CDNAME at {}", cdname_path.display()))?;
    // `tim_scan` dir names carry EXTRACTION indices, so resolve the block in
    // the retail extraction frame (raw define - 2); the unshifted window
    // would drop the block's first two entries and bleed into the next block.
    let (start, end) =
        cdname::block_range_for_name_extraction(&map, scene_name).ok_or_else(|| {
            anyhow::anyhow!(
                "scene '{}' not found in CDNAME at {}",
                scene_name,
                cdname_path.display()
            )
        })?;
    // tim_scan dir names look like "<NNNN>_<scene>". Walk the dir and
    // pick entries whose numeric prefix falls in [start, end).
    let tim_root = extracted_root.join("tim_scan");
    let mut dirs: Vec<PathBuf> = Vec::new();
    let Ok(rd) = std::fs::read_dir(&tim_root) else {
        anyhow::bail!(
            "no tim_scan dir at {} - run `asset tim-scan` first",
            tim_root.display()
        );
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let Some(stem) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some((num_str, _)) = stem.split_once('_') else {
            continue;
        };
        let Ok(idx) = num_str.parse::<u32>() else {
            continue;
        };
        if idx >= start && idx < end {
            dirs.push(p);
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Shape-based AABB filter for directory navigation.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum ShapeFilter {
    /// No shape filter applied - every `*.tmd` file under the dir is included.
    Any,
    /// Tall meshes (height > 1.5 × max horizontal extent). Hero/NPC bodies.
    Character,
    /// Wide flat meshes (height < 0.5 × max horizontal extent). Battle arenas
    /// and floor pieces.
    Arena,
}

impl ShapeFilter {
    /// Decide whether `path` matches this filter. `Any` always returns true;
    /// other variants parse the TMD and check its AABB aspect.
    pub(crate) fn accepts(self, path: &Path) -> bool {
        match self {
            ShapeFilter::Any => true,
            ShapeFilter::Character | ShapeFilter::Arena => {
                let Ok(bytes) = std::fs::read(path) else {
                    return false;
                };
                let Ok(parsed) = legaia_tmd::parse(&bytes) else {
                    return false;
                };
                let Some(aabb) = tmd_aabb(&parsed) else {
                    return false;
                };
                let w = aabb.1[0] - aabb.0[0];
                let h = (aabb.1[1] - aabb.0[1]).abs();
                let d = aabb.1[2] - aabb.0[2];
                let horizontal = w.max(d).max(1.0);
                let aspect = h.max(1.0) / horizontal;
                match self {
                    ShapeFilter::Character => aspect > 1.5,
                    ShapeFilter::Arena => aspect < 0.5,
                    ShapeFilter::Any => true,
                }
            }
        }
    }
}

fn tmd_aabb(parsed: &legaia_tmd::Tmd) -> Option<([f32; 3], [f32; 3])> {
    let mut iter = parsed.objects.iter().flat_map(|o| o.vertices.iter());
    let first = iter.next()?;
    let mut lo = [first.x as f32, first.y as f32, first.z as f32];
    let mut hi = lo;
    for v in iter {
        let p = [v.x as f32, v.y as f32, v.z as f32];
        for i in 0..3 {
            if p[i] < lo[i] {
                lo[i] = p[i];
            }
            if p[i] > hi[i] {
                hi[i] = p[i];
            }
        }
    }
    Some((lo, hi))
}

pub(crate) fn parse_hex_u64(s: &str) -> std::result::Result<u64, String> {
    if let Some(stripped) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(stripped, 16).map_err(|e| e.to_string())
    } else {
        s.parse::<u64>().map_err(|e| e.to_string())
    }
}

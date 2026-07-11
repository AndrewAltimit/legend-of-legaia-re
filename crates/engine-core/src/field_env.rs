//! Shared field/world **environment-geometry draw resolver**.
//!
//! A field scene's static geometry is composed from two sources that live in
//! different assets:
//!
//! - the **environment mesh pack**: the scene-owned PROT entry whose LZS
//!   sections carry the scene's object-local Legaia TMDs (buildings, props,
//!   terrain-decor tiles) - the `scene_asset_table` type-2 `Tmd` descriptor;
//! - the **placements**: the field `.MAP` object grid
//!   ([`legaia_asset::field_objects`], retail `FUN_8003A55C`), whose records
//!   select a pack mesh (`+0x10`) and give it a world transform, with world Y
//!   resolved through the MAN header's 16-entry floor-height LUT.
//!
//! This module is the platform-independent kernel both renderers share: the
//! native play-window (`engine-shell`, which maps the draws onto wgpu model
//! matrices) and the WASM web viewer (which streams them to a WebGL assembled
//! view). It resolves *which* [`SceneResources`] TMD each placement draws and
//! *where*, leaving mesh upload / matrix conventions to the caller.
//!
//! REF: FUN_8003A55C (object-grid walk), FUN_8003AEB0 (floor-LUT install)

use crate::scene::Scene;
use crate::scene_resources::SceneResources;
use legaia_asset::field_objects::Placement;
use std::collections::{HashMap, HashSet};

/// One resolved environment draw: a scene-pack mesh instanced at a world
/// position. `world_*` are PSX field-frame coordinates (retail Y-down); the
/// caller applies its own render-frame flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvDraw {
    /// Index into the environment-pack subset (the [`env_pack_tmd_indices`]
    /// order) - i.e. the placement's resolved pack index.
    pub env_slot: usize,
    /// Index into `res.tmds` for the mesh this draw instances.
    pub res_tmd: usize,
    /// World X (`col*0x80 + x_off + 0x40`).
    pub world_x: i32,
    /// World Y (`-lut[floor_nibble] + y_off`, or `0` without a LUT/nibble).
    pub world_y: i32,
    /// World Z (`row*0x80 - (z_off - 0x40)`).
    pub world_z: i32,
    /// Yaw in PSX angle units (`4096` = full revolution), from the object
    /// record's `+0x0A` field (see [`Placement::rot_y`]): the authored mesh
    /// orientation (bridge quarter-turns, tree variety). For a pure-Y angle
    /// retail's matrix builder (`FUN_80026988`) maps local `+Z` to
    /// `(sin, 0, cos)` in the retail Y-down frame - `glam`'s
    /// `Mat4::from_rotation_y` with the same angle reproduces it exactly.
    /// The record's `+0x08`/`+0x0C` X/Z tilts (zero on every retail walk
    /// map, rare in towns) stay on the [`Placement`] until the full
    /// three-angle composition order of `FUN_80026988` is ported.
    pub rot_y: u16,
}

/// Why a placement produced no [`EnvDraw`]. Surfaced so callers can log
/// diagnostics (the shell's `LEGAIA_DIAG_PLACE` path) without re-walking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvDrawDrop {
    /// The placement's object id resolves to no scene-pack mesh
    /// (protagonist / NPC ids; `Placement::pack_index == None`).
    NoPackIndex { world_x: i32, world_z: i32 },
    /// The record's pack index exceeds the environment pack's TMD count.
    SlotOutOfRange {
        pack_index: u16,
        world_x: i32,
        world_z: i32,
    },
}

/// Select the scene's **environment geometry pack** entry: the scene-owned
/// PROT entry that produced the most parsed TMDs in `res.tmds`.
///
/// Neither "the bundle entry" nor "the first `SceneAssetTable`" is
/// universally right - two scene shapes split them in opposite directions:
///
/// - the opening cutscene `opdeene` keeps its MAN in a
///   `SceneScriptedAssetTable` (entry 748) and its 72-TMD vignette geometry
///   in a *separate* `SceneAssetTable` sibling (entry 749), so keying on the
///   bundle finds zero env meshes;
/// - a world-map kingdom bundle keeps its geometry in the
///   `SceneScriptedAssetTable` (entry 85) while a sibling `SceneAssetTable`
///   (entry 86) holds an unrelated sub-area, so "prefer the SceneAssetTable"
///   breaks the overworld.
///
/// Voting by parsed-TMD count resolves the pack the placements index in every
/// case (opdeene 749, town01 4, map01 85). The scene-entry filter keeps
/// shared blocks (the resident player mesh) out of the vote. Ties break to
/// the lowest entry index so the choice is deterministic.
pub fn env_pack_entry(scene: &Scene, res: &SceneResources) -> Option<u32> {
    let scene_entry_ids: HashSet<u32> = scene.entries.iter().map(|e| e.idx).collect();
    let mut entry_tmd_counts: HashMap<u32, usize> = HashMap::new();
    for t in &res.tmds {
        if scene_entry_ids.contains(&t.entry_idx) {
            *entry_tmd_counts.entry(t.entry_idx).or_default() += 1;
        }
    }
    entry_tmd_counts
        .into_iter()
        .max_by_key(|&(idx, n)| (n, std::cmp::Reverse(idx)))
        .map(|(idx, _)| idx)
}

/// The environment pack's TMDs as indices into `res.tmds`, in scan order
/// (byte-offset order within the winning entry) - the index space a
/// placement's `pack_index` selects from. Empty when the scene owns no
/// parsed TMDs.
pub fn env_pack_tmd_indices(scene: &Scene, res: &SceneResources) -> Vec<usize> {
    let Some(env_entry) = env_pack_entry(scene, res) else {
        return Vec::new();
    };
    res.tmds
        .iter()
        .enumerate()
        .filter(|(_, t)| t.entry_idx == env_entry)
        .map(|(i, _)| i)
        .collect()
}

/// Resolve placements (or terrain tiles - any
/// [`legaia_asset::field_objects::Placement`] list) into environment draws.
///
/// `env_tmds` is the [`env_pack_tmd_indices`] subset; `floor_lut` is the
/// scene's MAN floor-height LUT (`Scene::field_floor_height_lut`). World Y is
/// `-lut[nibble & 0xF] + y_off` when both are available, else the ground
/// plane - exactly the retail placement math.
pub fn resolve_env_draws(
    env_tmds: &[usize],
    placements: &[Placement],
    floor_lut: Option<[i16; 16]>,
) -> (Vec<EnvDraw>, Vec<EnvDrawDrop>) {
    let mut draws = Vec::new();
    let mut drops = Vec::new();
    for p in placements {
        let Some(pack_index) = p.pack_index else {
            drops.push(EnvDrawDrop::NoPackIndex {
                world_x: p.world_x,
                world_z: p.world_z,
            });
            continue;
        };
        let Some(&res_tmd) = env_tmds.get(pack_index as usize) else {
            drops.push(EnvDrawDrop::SlotOutOfRange {
                pack_index,
                world_x: p.world_x,
                world_z: p.world_z,
            });
            continue;
        };
        let world_y = match (floor_lut, p.floor_nibble) {
            (Some(lut), Some(nib)) => -(lut[(nib & 0x0F) as usize] as i32) + p.y_off as i32,
            _ => 0,
        };
        draws.push(EnvDraw {
            env_slot: pack_index as usize,
            res_tmd,
            world_x: p.world_x,
            world_y,
            world_z: p.world_z,
            rot_y: p.rot_y,
        });
    }
    (draws, drops)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(pack_index: Option<u16>, nibble: Option<u8>, y_off: i16) -> Placement {
        Placement {
            obj_idx: 0,
            col: 2,
            row: 3,
            world_x: 2 * 0x80 + 0x40,
            world_z: 3 * 0x80 + 0x40,
            y_off,
            floor_nibble: nibble,
            pack_index,
            flags: 0x4,
            rot_x: 0,
            rot_y: 0x400,
            rot_z: 0,
            collider_x: 0,
            collider_z: 0,
        }
    }

    #[test]
    fn draws_resolve_pack_index_and_floor_y() {
        let env_tmds = vec![10, 11, 12];
        let mut lut = [0i16; 16];
        lut[6] = 192;
        let placements = vec![placement(Some(2), Some(6), 8)];
        let (draws, drops) = resolve_env_draws(&env_tmds, &placements, Some(lut));
        assert!(drops.is_empty());
        assert_eq!(
            draws,
            vec![EnvDraw {
                env_slot: 2,
                res_tmd: 12,
                world_x: 2 * 0x80 + 0x40,
                world_y: -192 + 8,
                world_z: 3 * 0x80 + 0x40,
                rot_y: 0x400,
            }]
        );
    }

    #[test]
    fn drops_classify_missing_and_out_of_range() {
        let env_tmds = vec![10];
        let placements = vec![placement(None, None, 0), placement(Some(5), None, 0)];
        let (draws, drops) = resolve_env_draws(&env_tmds, &placements, None);
        assert!(draws.is_empty());
        assert!(matches!(drops[0], EnvDrawDrop::NoPackIndex { .. }));
        assert!(matches!(
            drops[1],
            EnvDrawDrop::SlotOutOfRange { pack_index: 5, .. }
        ));
    }

    #[test]
    fn no_lut_lands_on_ground_plane() {
        let env_tmds = vec![10];
        let placements = vec![placement(Some(0), Some(6), 8)];
        let (draws, _) = resolve_env_draws(&env_tmds, &placements, None);
        assert_eq!(draws[0].world_y, 0);
    }
}

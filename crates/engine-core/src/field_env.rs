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
//! A placed object is additionally **bound** to a MAN partition-0 record (see
//! [`object_binds`]), which decides both whether it spawns at all and, when its
//! mesh has more than one TMD object, which animation clip poses those objects.
//!
//! REF: FUN_8003A55C (object-grid walk), FUN_8003AEB0 (floor-LUT install)

use crate::field_regions::{self, TileTrigger};
use crate::scene::Scene;
use crate::scene_resources::SceneResources;
use legaia_asset::field_objects::Placement;
use legaia_asset::man_section::ManFile;
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
    /// Animation id from the placement's object bind ([`ObjectBind::anim_id`]);
    /// `0` = none.
    ///
    /// Nonzero means the draw is **posed**: the mesh's TMD objects are the
    /// bones of scene-ANM-bundle record `anim_id - 1`, and the object's rest
    /// state is that record's **frame 0**. Retail does exactly this - the
    /// per-actor anim tick `FUN_800204f8` binds the record into `actor+0x4C`
    /// and flips the actor to draw kind `1`, whose draw walker `FUN_8001b964`
    /// applies a per-bone rigid transform to each TMD object before drawing it
    /// (and refuses to draw at all unless bone count == object count). An
    /// `anim_id` of `0` leaves the actor at draw kind `5`, which draws every
    /// object with the actor's single transform - correct only for the
    /// single-object props.
    ///
    /// Drawing a multi-object prop *unposed* is what leaves Rim Elm's cupboard
    /// doors floating inside the cabinet: their vertices are authored about
    /// their own hinge, and the frame-0 bone transform is what swings them onto
    /// the front face (closed). The clip's later frames are the door opening -
    /// retail advances the frame only while the interaction script runs.
    pub anim_id: u8,
}

/// The **object bind** of a placed field object: the MAN partition-0 record
/// retail attaches to it at scene init.
///
/// `FUN_8003A55C` resolves it by the object's footprint-anchor tile
/// (`func_0x801d5630(1, anchor_col, anchor_row)` - the `.MAP` kind-1
/// tile-trigger sub-table, primary block first then the `+0x12000` fallback),
/// takes the trigger's `record` byte, and reads that record out of the MAN's
/// flat record-offset table. Partition 0 comes first in that table, so the
/// index is a partition-0 record index.
///
/// The record's header is `[u8 n][n*2 name bytes][u8 anim_id]`, and the script
/// begins right after it. `FUN_8003A55C` stores the record base into the
/// actor's `+0x90` (script buffer), the post-header offset into `+0x9E` (PC),
/// and the header's last byte into `+0x5C` - the actor's animation id.
///
/// **A placed record whose anchor tile has no trigger is never spawned.** In
/// `town01` that silently drops six placements (an `obj456` in five houses, one
/// `obj230` cupboard); a live Rim Elm capture's actor list contains exactly the
/// bound ones.
// REF: FUN_8003A55C, FUN_801D5630, FUN_800204f8
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectBind {
    /// MAN partition-0 record index the anchor tile's trigger names.
    pub record: u8,
    /// The record header's trailing byte: the actor's animation id
    /// (`0` = none). Indexes the per-scene ANM bundle as `anim_id - 1`.
    pub anim_id: u8,
}

/// Resolve every object bind a scene's `.MAP` + MAN define, keyed by the
/// **anchor tile** (`(anchor_col, anchor_row)`) the placement looks it up by.
///
/// `field_map` is the extended `.MAP` footprint (the trigger blocks live at
/// `+0x10000` and `+0x12000`); `man_file` / `man` are the scene's parsed MAN.
/// Triggers are scanned primary-block-first, so a tile carried by both blocks
/// resolves to the primary entry, as in retail's `FUN_801D5630`. The trigger's
/// dispatch `gate` is **not** consulted: `FUN_8003A55C` binds whatever kind-1
/// entry sits on the tile (retail towns only put gate-0 entries there).
///
/// PORT: FUN_8003A55C (the `func_0x801d5630` bind lookup + record-header decode)
pub fn object_binds(
    field_map: &[u8],
    man_file: &ManFile,
    man: &[u8],
) -> HashMap<(u8, u8), ObjectBind> {
    let mut triggers: Vec<TileTrigger> = Vec::new();
    for base in [
        field_regions::MAP_REGION_BLOCK_OFFSET,
        field_regions::MAP_TRIGGER_FALLBACK_OFFSET,
    ] {
        if let Some(block) = field_map.get(base..) {
            triggers.extend(field_regions::parse_tile_triggers(block));
        }
    }
    let mut out: HashMap<(u8, u8), ObjectBind> = HashMap::new();
    for t in triggers {
        let key = (t.tile_x, t.tile_z);
        if out.contains_key(&key) {
            continue; // primary block wins (retail scans it first)
        }
        let Some(anim_id) = partition0_anim_id(man_file, man, t.record as usize) else {
            continue;
        };
        out.insert(
            key,
            ObjectBind {
                record: t.record,
                anim_id,
            },
        );
    }
    out
}

/// The animation id in MAN partition-0 record `index`'s header
/// (`[u8 n][n*2 name bytes][u8 anim_id]`). `None` when the record or its header
/// runs past the buffer.
fn partition0_anim_id(man_file: &ManFile, man: &[u8], index: usize) -> Option<u8> {
    let off = man_file
        .data_region_offset
        .checked_add(*man_file.partitions[0].get(index)? as usize)?;
    let n = *man.get(off)? as usize;
    man.get(off.checked_add(1)?.checked_add(2 * n)?).copied()
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
    /// The placed object has no [`ObjectBind`] at its footprint-anchor tile, so
    /// retail never spawns an actor for it (`FUN_8003A55C` skips the tile).
    Unbound {
        anchor: (u8, u8),
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
///
/// This is the **unbound** resolver: every draw comes back with
/// [`EnvDraw::anim_id`] `0`. It is what the terrain / decoration cell sweeps
/// want (those cells are not `FUN_8003A55C` actors and carry no bind). The
/// *placed*-object layer must go through [`resolve_placed_env_draws`] instead,
/// so it inherits the spawn gate and the pose.
pub fn resolve_env_draws(
    env_tmds: &[usize],
    placements: &[Placement],
    floor_lut: Option<[i16; 16]>,
) -> (Vec<EnvDraw>, Vec<EnvDrawDrop>) {
    resolve_placed_env_draws(env_tmds, placements, floor_lut, None)
}

/// Resolve the **placed**-object layer (`FUN_8003A55C`'s actors) into
/// environment draws, applying the two things a bind decides:
///
/// - **the spawn gate**: a placement whose footprint-anchor tile has no
///   [`ObjectBind`] is dropped ([`EnvDrawDrop::Unbound`]) - retail skips the
///   tile outright;
/// - **the pose**: the bind's `anim_id` lands on [`EnvDraw::anim_id`], and a
///   nonzero one means the mesh's objects must be posed from frame 0 of scene
///   ANM record `anim_id - 1` rather than drawn at their raw object-local
///   vertices.
///
/// Passing `binds = None` disables both (the [`resolve_env_draws`] behaviour),
/// which is what the bind-less terrain sweeps want.
pub fn resolve_placed_env_draws(
    env_tmds: &[usize],
    placements: &[Placement],
    floor_lut: Option<[i16; 16]>,
    binds: Option<&HashMap<(u8, u8), ObjectBind>>,
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
        let anchor = (p.anchor_col, p.anchor_row);
        let anim_id = match binds {
            None => 0,
            Some(b) => match b.get(&anchor) {
                Some(bind) => bind.anim_id,
                None => {
                    drops.push(EnvDrawDrop::Unbound {
                        anchor,
                        world_x: p.world_x,
                        world_z: p.world_z,
                    });
                    continue;
                }
            },
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
            anim_id,
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
            anchor_col: 2,
            anchor_row: 3,
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
                anim_id: 0,
            }]
        );
    }

    /// The bind decides both halves: an anchor tile with a bind carries its
    /// `anim_id` onto the draw (so a multi-object prop gets posed), and one
    /// without a bind is dropped - retail never spawns an actor for it.
    #[test]
    fn binds_gate_the_spawn_and_carry_the_anim_id() {
        let env_tmds = vec![10, 11, 12];
        let mut bound = placement(Some(1), None, 0);
        bound.anchor_col = 7;
        bound.anchor_row = 9;
        let unbound = placement(Some(2), None, 0); // anchor (2, 3): no bind
        let mut binds = HashMap::new();
        binds.insert(
            (7u8, 9u8),
            ObjectBind {
                record: 12,
                anim_id: 2,
            },
        );

        let (draws, drops) =
            resolve_placed_env_draws(&env_tmds, &[bound, unbound], None, Some(&binds));
        assert_eq!(draws.len(), 1, "the unbound placement must not spawn");
        assert_eq!(draws[0].env_slot, 1);
        assert_eq!(draws[0].anim_id, 2);
        assert!(matches!(
            drops.as_slice(),
            [EnvDrawDrop::Unbound { anchor: (2, 3), .. }]
        ));

        // Without binds (the terrain-cell sweeps) both draw, unposed.
        let (draws, drops) = resolve_placed_env_draws(&env_tmds, &[bound, unbound], None, None);
        assert_eq!(draws.len(), 2);
        assert!(draws.iter().all(|d| d.anim_id == 0));
        assert!(drops.is_empty());
    }

    /// The bind's anim id is the trailing byte of the partition-0 record's
    /// `[u8 n][n*2 name][u8 anim]` header.
    #[test]
    fn partition0_header_yields_the_anim_id() {
        // Two records: #0 with a 3-"char" name and anim 2, #1 with no name
        // and anim 0.
        let dro = 0x40usize;
        let mut man = vec![0u8; dro];
        let mut offsets = Vec::new();
        for (name_len, anim) in [(3u8, 2u8), (0, 0)] {
            offsets.push((man.len() - dro) as u32);
            man.push(name_len);
            man.extend(std::iter::repeat_n(0x82u8, name_len as usize * 2));
            man.push(anim);
            man.extend_from_slice(&[0x21, 0x00]); // a token script body
        }
        let mf = ManFile {
            header: legaia_asset::man_section::ManHeader {
                status_flags: 0,
                low_flag: false,
                depth_lut: [0; 16],
                partition_counts: [2, 0, 0],
                u24_at_28: 0,
            },
            partitions: [offsets, Vec::new(), Vec::new()],
            data_region_offset: dro,
            sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
                offset: man.len(),
                length: 0,
            }),
        };
        assert_eq!(partition0_anim_id(&mf, &man, 0), Some(2));
        assert_eq!(partition0_anim_id(&mf, &man, 1), Some(0));
        assert_eq!(partition0_anim_id(&mf, &man, 2), None);
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

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
//! A placed object may additionally be **bound** to a MAN partition-0 record (see
//! [`object_binds`]). The bind is not a spawn gate - retail's two placed-object
//! sweeps between them create every placed record - but it does decide, when the
//! object's mesh has more than one TMD object, which animation clip poses those
//! objects.
//!
//! REF: FUN_8003A55C (object-grid walk), FUN_801D7B50 (window rebuild),
//! REF: FUN_8003AEB0 (floor-LUT install)

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
    /// The placement's **footprint-anchor tile** (`col + record[+0x06]`,
    /// `row + record[+0x07]`) - the tile `FUN_8003A55C` resolves the object
    /// bind by, and therefore the identity of this prop's actor. Keyed on by
    /// [`PropAnimBank`] so each placed instance keeps its own frame cursor.
    pub anchor: (u8, u8),
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
/// **A placed record whose anchor tile has no trigger is not `FUN_8003A55C`'s** -
/// it is skipped there, and a live Rim Elm capture's init actor list holds exactly
/// the bound ones (37 of `town01`'s 46 placements). It is **not** unspawned,
/// though: the sub-area window sweep `FUN_801D7B50` creates those nine, gated on
/// the complementary anchor-tile bit
/// [`CELL_BIND_OWNED`](legaia_asset::field_objects::CELL_BIND_OWNED) rather than
/// on a bind, so they draw unscripted and unposed.
// REF: FUN_8003A55C, FUN_801D5630, FUN_800204f8, FUN_801D7B50
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
    /// **Neither** of retail's two placed-object sweeps would create this actor:
    /// the footprint-anchor tile carries no [`ObjectBind`] (so `FUN_8003A55C`
    /// skips it) *and* it is marked
    /// [`CELL_BIND_OWNED`](legaia_asset::field_objects::CELL_BIND_OWNED) (so the
    /// window sweep `FUN_801D7B50` skips it too). No retail map has such a cell -
    /// the bind and the bit are complementary - so this is a malformed-map guard.
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
/// This is the **bind-less** resolver: every draw comes back with
/// [`EnvDraw::anim_id`] `0`. It is what the terrain / decoration cell sweeps
/// want (those cells are not static-object actors and carry no bind). The
/// *placed*-object layer must go through [`resolve_placed_env_draws`] instead,
/// so it inherits the pose.
pub fn resolve_env_draws(
    env_tmds: &[usize],
    placements: &[Placement],
    floor_lut: Option<[i16; 16]>,
) -> (Vec<EnvDraw>, Vec<EnvDrawDrop>) {
    resolve_placed_env_draws(env_tmds, placements, floor_lut, None)
}

/// Resolve the **placed**-object layer (the static-object actors) into
/// environment draws, applying what the object bind decides.
///
/// Retail creates these actors from two sweeps, and every placed record belongs
/// to exactly one of them (see [`legaia_asset::field_objects`]):
///
/// - **bound** (a kind-1 trigger sits on the footprint-anchor tile): the scene-init
///   sweep `FUN_8003A55C` creates it, and the bind's `anim_id` lands on
///   [`EnvDraw::anim_id`]. A nonzero one means the mesh's TMD objects are that
///   clip's bones and must be **posed** from frame 0 of scene ANM record
///   `anim_id - 1` rather than drawn at their raw object-local vertices.
/// - **unbound**: the sub-area window sweep `FUN_801D7B50` creates it instead.
///   That sweep does no bind lookup, so the actor has no script and no clip -
///   `anim_id` is `0` and the mesh draws raw. Rim Elm's cavern shell is one of
///   these; culling it (an earlier reading of the bind as a *spawn gate*) leaves
///   the cave a black hole.
///
/// A placement is dropped only in the case retail's two sweeps *both* skip:
/// unbound **and** [`CELL_BIND_OWNED`] set on its anchor tile
/// ([`EnvDrawDrop::Unbound`]). No retail scene has one - the bit and the bind are
/// complementary across the corpus - so the drop list stays empty in practice;
/// it exists so a malformed / hand-edited map cannot conjure an actor retail
/// would not.
///
/// Passing `binds = None` skips the whole bind pass (every draw comes back with
/// `anim_id` `0`, nothing is dropped), which is what the bind-less terrain-cell
/// sweeps want.
///
/// [`CELL_BIND_OWNED`]: legaia_asset::field_objects::CELL_BIND_OWNED
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
                // Bound: the init sweep's actor, posed by the bind's clip.
                Some(bind) => bind.anim_id,
                // Unbound: the window sweep's actor - no script, no clip. It
                // still draws; only a record the *window* sweep also skips
                // (anchor tile marked `CELL_BIND_OWNED`) has no actor at all.
                None => {
                    if p.anchor_cell & legaia_asset::field_objects::CELL_BIND_OWNED != 0 {
                        drops.push(EnvDrawDrop::Unbound {
                            anchor,
                            world_x: p.world_x,
                            world_z: p.world_z,
                        });
                        continue;
                    }
                    0
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
            anchor,
        });
    }
    (draws, drops)
}

// ---------------------------------------------------------------------------
// Placed-prop animation: the door swing.
// ---------------------------------------------------------------------------

/// `actor+0x62` bit `0x0002` - **hold**. Set, the per-frame anim tick skips the
/// cursor advance, so the clip freezes at whatever frame it is on.
pub const ANIM_HOLD: u16 = 0x0002;
/// `actor+0x62` bit `0x0008` - **clamp** (one-shot). Set, the cursor stops at
/// the end of the clip; clear, it wraps (the looping idle case).
pub const ANIM_CLAMP: u16 = 0x0008;
/// `actor+0x62` bit `0x0080` - **reverse**. Set, the cursor counts down.
pub const ANIM_REVERSE: u16 = 0x0080;
/// `actor+0x62` bit `0x0100` - the **end latch**. Set by the tick on the frame
/// the cursor reaches either end of the clip; scripts wait on it.
pub const ANIM_END: u16 = 0x0100;
/// `actor+0x62` bit `0x0200` - **restart request**. Consumed by the next tick,
/// which snaps the cursor to frame `0` (forward) or the last frame (reverse).
pub const ANIM_RESTART: u16 = 0x0200;

/// The `+0x62` word every actor is born with, from the placed-object actor
/// template `DAT_80073E70` (`FUN_80020DE0` copies `template[1]` into `+0x62`):
/// no hold, no clamp, forward - i.e. **looping playback**, which is what the
/// scene's NPCs run their idle clips under.
pub const ANIM_SPAWN_FLAGS: u16 = 0x0015;

/// The `+0x6A` rate a placed object is born with: the template's `0x10`
/// (1 frame per tick, since the cursor is in 1/16-frame units) halved by
/// `FUN_8003A55C`'s `*(short *)(actor + 0x6a) >>= 1`.
pub const ANIM_SPAWN_RATE: i16 = 8;

/// Half-extent of the prop contact box, in world units. Same box the field
/// locomotion's static-entity collision arm uses for a placed record
/// (`World::field_prop_colliders` / `FIELD_PROP_BOX_HALF`); retail's
/// `FUN_801CFC40` links the player and the touched actor through their `+0x98`
/// partner slots and `FUN_801D5B5C` then resumes the touched actor's script.
pub const PROP_TOUCH_BOX_HALF: i32 = 0x40 + 0x10;

/// Live animation state of one placed prop - the fields of the retail actor
/// record that the per-frame anim tick reads and writes.
///
/// | field | actor offset | meaning |
/// |---|---|---|
/// | `anim_id` | `+0x5C` | clip id; `0` = none. Clip = scene-ANM record `anim_id - 1`. |
/// | `cursor` | `+0x68` | frame cursor in **1/16-frame** units (`frame = cursor >> 4`). |
/// | `flags` | `+0x62` | the `ANIM_*` bits above. |
/// | `rate` | `+0x6A` | cursor units added per tick. |
///
/// PORT: FUN_800204F8 (the binder + per-frame advancer, called from the actor
/// tick `FUN_80021DF4`)
/// REF: FUN_8001B964 (the draw walker: `frame = (i16)(actor+0x68) >> 4`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropAnim {
    /// Clip id (`actor+0x5C`).
    pub anim_id: u8,
    /// Frame count of the bound clip (the ANM record's `b` header field).
    pub frames: u16,
    /// Bit 0 of the ANM record header's `a` high byte: selects the scaled step.
    pub scaled_step: bool,
    /// The ANM record header's `flag` low byte - the scaled step's divisor.
    pub step_div: u8,
    /// Frame cursor in 1/16-frame units (`actor+0x68`).
    pub cursor: i16,
    /// Animation control word (`actor+0x62`).
    pub flags: u16,
    /// Per-tick cursor step (`actor+0x6A`).
    pub rate: i16,
}

impl PropAnim {
    /// A freshly spawned placed prop: the actor-template flags and the halved
    /// template rate, cursor at zero.
    pub fn spawned(anim_id: u8, frames: u16, scaled_step: bool, step_div: u8) -> Self {
        Self {
            anim_id,
            frames,
            scaled_step,
            step_div,
            cursor: 0,
            flags: ANIM_SPAWN_FLAGS,
            rate: ANIM_SPAWN_RATE,
        }
    }

    /// The clip frame the draw walker poses from: `(i16)(actor+0x68) >> 4`.
    pub fn frame(&self) -> usize {
        (self.cursor >> 4).max(0) as usize
    }

    /// True once the cursor has reached an end of the clip and latched
    /// [`ANIM_END`] - what a script's "wait for the animation" spin tests.
    pub fn at_end(&self) -> bool {
        self.flags & ANIM_END != 0
    }

    /// One frame of the retail anim tick: consume a restart request, step the
    /// cursor unless held, then wrap (loop) or clamp (one-shot) at either end,
    /// latching [`ANIM_END`] when an end is hit.
    ///
    /// PORT: FUN_800204F8
    pub fn tick(&mut self) {
        if self.anim_id == 0 || self.frames == 0 {
            return;
        }
        let span = (self.frames as i32) * 16;
        // The clip's own per-frame step scaling (`clip[1] & 1` selects it,
        // `clip[6]` divides). Every field door / prop clip takes the plain arm.
        let step = if self.scaled_step && self.step_div != 0 {
            let d = self.step_div as i32;
            ((self.rate as i32 * 2 + d - 1) / d) as i16
        } else {
            self.rate
        };
        if self.flags & ANIM_RESTART != 0 {
            self.flags &= !ANIM_RESTART;
            self.cursor = if self.flags & ANIM_REVERSE == 0 {
                0
            } else {
                (span - 16).max(0) as i16
            };
        }
        // Retail re-reads the flag word here, so the restart bit it just
        // cleared is not what the hold test below sees.
        let flags = self.flags;
        self.flags &= !ANIM_END;
        if flags & ANIM_HOLD == 0 {
            self.cursor = if flags & ANIM_REVERSE == 0 {
                self.cursor.wrapping_add(step)
            } else {
                self.cursor.wrapping_sub(step)
            };
        }
        if self.cursor < 0 {
            self.cursor = if self.flags & ANIM_CLAMP == 0 {
                self.cursor.wrapping_add(span as i16)
            } else {
                0
            };
            self.flags |= ANIM_END;
        }
        let last = (span - 1) as i16;
        if last <= self.cursor {
            self.cursor = if self.flags & ANIM_CLAMP == 0 {
                0
            } else {
                last
            };
            self.flags |= ANIM_END;
        }
    }
}

/// One animation command a placed prop's bind script issues against `+0x62` /
/// `+0x6A`. The field VM's whole animation surface, as the retail prop records
/// use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimCmd {
    /// `0x4C` nibble-4 sub-1 with a zero ramp: `actor+0x6A = max(1, value >> 1)`.
    Rate(i16),
    /// `0x2B <bit>`: `actor+0x62 |= 1 << bit`.
    SetBit(u8),
    /// `0x2C <bit>`: `actor+0x62 &= !(1 << bit)`.
    ClearBit(u8),
    /// `0x4C 0x35`: `actor+0x62 = (+0x62 & !REVERSE) | 0x20A` - restart at
    /// frame 0, one-shot, and hold. The rest pose of every closed door.
    ResetHold,
    /// `0x4C 0x36`: `actor+0x62 |= 0x28A` - restart at the *last* frame,
    /// reverse, one-shot, hold. The "already opened" snap (the shelf whose
    /// story flag says it has been searched).
    EndHold,
}

impl AnimCmd {
    /// Apply the command to a live prop.
    pub fn apply(self, a: &mut PropAnim) {
        match self {
            AnimCmd::Rate(v) => a.rate = (v >> 1).max(1),
            AnimCmd::SetBit(b) => a.flags |= 1u16.wrapping_shl(u32::from(b) & 0x1F),
            AnimCmd::ClearBit(b) => a.flags &= !1u16.wrapping_shl(u32::from(b) & 0x1F),
            AnimCmd::ResetHold => a.flags = (a.flags & !ANIM_REVERSE) | 0x020A,
            AnimCmd::EndHold => a.flags |= 0x028A,
        }
    }
}

/// A run of a prop script's animation commands, ending either at the script's
/// "wait until the clip finishes" spin (`0x2D` on bit 8 = [`ANIM_END`]) or at
/// the end of the pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnimSegment {
    /// Commands applied on entering the segment.
    pub cmds: Vec<AnimCmd>,
    /// True when the segment ends on the end-latch spin: the script parks here
    /// until the clip reaches its end, then runs the next segment.
    pub wait_for_end: bool,
}

/// A placed prop's animation program, decoded from its bind record's script.
///
/// The record is a field-VM script whose passes are delimited by the `0x21`
/// park opcode:
///
/// - **spawn**: `FUN_8003A55C` runs the record from its post-header PC up to
///   the first `0x21` (the retail loop only enters when the leading opcode is
///   `0x24`/`0x25`, which every posed prop record carries). That pass sets the
///   rate and issues `0x4C 0x35`, leaving the prop **held at frame 0** - a
///   closed door, a shut cupboard.
/// - **touch**: the next pass, which `FUN_801D5B5C` resumes when the player's
///   body hits the prop. For a house door that is `[clear REVERSE, clear HOLD,
///   set CLAMP, clear END]` then the end-latch spin: the clip plays forward and
///   clamps open. Rim Elm's cupboard adds a second segment after the spin -
///   `[set REVERSE, clear HOLD, set CLAMP]` - which plays the doors back shut.
///
/// Records with no animation commands in their touch pass (the locked drawer,
/// the clock) decode to an empty program and never move, exactly as in retail.
///
/// Beyond the `+0x62` animation surface, the spawn prologue's own-context
/// `0x31 <bit>` CFLAG_SET ops (writes into the actor flag word `+0x10`) carry
/// the prop's **collision / interaction class**, collected into
/// [`Self::spawn_cflags`]:
///
/// - bits `0`/`1` (`31 00`) - **collision exempt**: the collision candidate
///   list builder `FUN_801CF754` and the interact probe `FUN_801CF9F4` both
///   skip any actor whose `+0x10 & 3 != 0`.
/// - bit `30` (`31 1E`, Rim Elm's cupboards) or bit `17` - the
///   `flags & 0x40020000` class of `FUN_801CFC40`: contact returns result bit
///   `1` instead of `4`, which the locomotion dispatch (`FUN_801D01B0`
///   `0x801d0800`) does NOT auto-post - only the just-pressed-confirm facing
///   probe posts it. This is the **interact gate**: cupboards open on the
///   confirm button, doors on body contact.
/// - bit `17`/`24` - the `flags & 0x1020000` box-source class: the contact box
///   anchors at the live position with the moving-actor extents instead of the
///   record-derived footprint centre.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PropProgram {
    /// Commands the spawn pass issues.
    pub spawn: Vec<AnimCmd>,
    /// The touch pass, split at the end-latch spins.
    pub touch: Vec<AnimSegment>,
    /// Actor `+0x10` bits the spawn prologue sets via own-context `0x31`
    /// CFLAG_SET ops. `0` when the record has no `0x24`/`0x25` prologue marker
    /// (retail then skips the spawn run entirely).
    pub spawn_cflags: u32,
}

impl PropProgram {
    /// True when contact plays the clip - the doors, the cupboards, the shop
    /// sign. False for the props whose record only poses them once (the locked
    /// drawer) and for the ones that never stop (the windmill: its spawn pass
    /// issues no `0x4C 0x35`, so it keeps the template's looping
    /// [`ANIM_SPAWN_FLAGS`] and spins forever).
    pub fn animates(&self) -> bool {
        !self.touch.is_empty()
    }

    /// The `FUN_801CFC40` `flags & 0x40020000` class: contact yields result
    /// bit `1`, which the locomotion never auto-posts - the prop fires only
    /// from the button-gated facing probe. Rim Elm's cupboards (`31 1E`).
    pub fn interact_gated(&self) -> bool {
        self.spawn_cflags & 0x4002_0000 != 0
    }

    /// The `flags & 0x1020000` box-source class: the contact box anchors at
    /// the live position with the moving-actor extents (±40) instead of the
    /// static footprint centre (±80).
    pub fn moving_box(&self) -> bool {
        self.spawn_cflags & 0x0102_0000 != 0
    }

    /// Born collision-exempt: the spawn prologue sets `+0x10` bit `0`/`1`
    /// (`31 00`), so `FUN_801CF754`'s `flags & 3` filter never admits the
    /// actor to the collision candidate list.
    pub fn spawn_collision_off(&self) -> bool {
        self.spawn_cflags & 3 != 0
    }
}

/// The field-VM opcodes the prop-script decoder reacts to.
const OP_PARK: u8 = 0x21;
const OP_FLAG_SET: u8 = 0x2B;
const OP_FLAG_CLEAR: u8 = 0x2C;
const OP_FLAG_TEST: u8 = 0x2D;
/// Bit index of [`ANIM_END`] - what the "wait for the clip" spin tests.
const ANIM_END_BIT: u8 = 8;
/// The bits of `actor+0x62` the per-frame anim tick reads: hold, clamp,
/// reverse, end, restart. A `0x2B` / `0x2C` on any other bit is a per-actor
/// flag the animation does not see, so the decoder ignores it - which also
/// keeps a linear walk that drifts into a record's inline dialogue text from
/// synthesising phantom animation commands out of ASCII (a `,` in a message is
/// the byte `0x2C`).
const ANIM_BITS: &[u8] = &[1, 3, 7, 8, 9];

/// Decode a bind record's animation program. `record` is the partition-0
/// record's bytes, `pc0` the offset just past its `[u8 n][n*2 name][u8 anim]`
/// header (where `FUN_8003A55C` parks the actor's PC).
///
/// The **spawn** commands are the ones before the record's first `0x21` park -
/// exactly the run `FUN_8003A55C`'s prologue loop executes (it stops on that
/// opcode). Everything after it is the resumable body, scanned linearly for
/// animation commands and cut at each `0x2D 0x08` end-latch spin.
///
/// A body segment is only kept when it actually **plays** the clip, i.e. it
/// clears the hold bit (`0x2C 0x01`). That drops the flag-gated pose snaps
/// (`0x4C 0x36` on a shelf whose story flag says it has already been searched)
/// that a linear walk sees but a branch-following VM would only reach through a
/// story-flag test - they are alternate *spawn* states, not contact reactions -
/// and it makes the decode robust against the walk drifting through a record's
/// inline dialogue text.
///
/// PORT: FUN_8003A55C (the spawn-prologue run) + the field-VM animation ops
/// (`0x2B` / `0x2C` / `0x2D` on `actor+0x62`, `0x4C` nibble-3 sub-5/6, `0x4C`
/// nibble-4 sub-1)
pub fn decode_prop_program(record: &[u8], pc0: usize) -> PropProgram {
    use legaia_asset::field_disasm::{FlagKind, InsnInfo, LinearWalker, MenuCtrlKind};

    let anim_cmd = |insn: &legaia_asset::field_disasm::Insn| -> Option<AnimCmd> {
        // Cross-context writes target a different actor's record, not this
        // prop's - retail's `0x2B`/`0x2C` write `iVar18 + 0x62`, the executing
        // context's own actor.
        if insn.extended.is_some() {
            return None;
        }
        match (insn.opcode, &insn.info) {
            (OP_FLAG_SET, InsnInfo::LFlag { bit, .. }) if ANIM_BITS.contains(bit) => {
                Some(AnimCmd::SetBit(*bit))
            }
            (OP_FLAG_CLEAR, InsnInfo::LFlag { bit, .. }) if ANIM_BITS.contains(bit) => {
                Some(AnimCmd::ClearBit(*bit))
            }
            (
                _,
                InsnInfo::MenuCtrl {
                    kind: MenuCtrlKind::Nibble3 { sub: 5 },
                    ..
                },
            ) => Some(AnimCmd::ResetHold),
            (
                _,
                InsnInfo::MenuCtrl {
                    kind: MenuCtrlKind::Nibble3 { sub: 6 },
                    ..
                },
            ) => Some(AnimCmd::EndHold),
            (
                _,
                InsnInfo::MenuCtrl {
                    kind:
                        MenuCtrlKind::Nibble4 {
                            sub: 1,
                            target,
                            ticks: 0,
                        },
                    ..
                },
            ) => Some(AnimCmd::Rate(*target)),
            _ => None,
        }
    };

    let mut prog = PropProgram::default();
    // Retail's spawn-prologue loop only enters when the record's first opcode
    // is the `0x24`/`0x25` marker (`FUN_8003A55C`, `uVar15 - 0x24 < 2`); a
    // record without it parks at `pc0` untouched and its leading ops run on
    // the first touch instead.
    let has_prologue = matches!(record.get(pc0), Some(0x24) | Some(0x25));
    let mut spawned = !has_prologue; // past the prologue's terminating `0x21`?
    let mut seg = AnimSegment::default();
    let mut body: Vec<AnimSegment> = Vec::new();
    for insn in LinearWalker::new(record, pc0).flatten() {
        if !spawned {
            if insn.opcode == OP_PARK && insn.extended.is_none() {
                spawned = true;
            } else if let Some(c) = anim_cmd(&insn) {
                prog.spawn.push(c);
            } else if insn.extended.is_none()
                && insn.opcode == 0x31
                && let InsnInfo::CFlag {
                    kind: FlagKind::Set,
                    bit,
                } = insn.info
            {
                // Actor `+0x10` class bits (see [`PropProgram::spawn_cflags`]).
                prog.spawn_cflags |= 1u32 << (bit & 0x1F);
            }
            continue;
        }
        // The end-latch spin closes the current segment.
        if insn.opcode == OP_FLAG_TEST
            && insn.extended.is_none()
            && matches!(
                insn.info,
                InsnInfo::LFlag {
                    kind: FlagKind::Test,
                    bit: ANIM_END_BIT
                }
            )
        {
            seg.wait_for_end = true;
            body.push(std::mem::take(&mut seg));
            continue;
        }
        if let Some(c) = anim_cmd(&insn) {
            seg.cmds.push(c);
        }
    }
    body.push(seg);
    /// `0x2C 0x01` - the command that un-holds the clip. A segment without it
    /// does not play anything.
    const PLAY: AnimCmd = AnimCmd::ClearBit(1);
    prog.touch = body
        .into_iter()
        .filter(|s| s.cmds.contains(&PLAY))
        .collect();
    prog
}

/// One placed prop's animation + interaction runtime: its live [`PropAnim`],
/// the program its bind record decodes to, the record itself (the prop's
/// field-VM script), and the actor state the retail collision / touch probes
/// read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropAnimState {
    /// Live clip state.
    pub anim: PropAnim,
    /// The bind record's decoded animation program + class bits.
    pub program: PropProgram,
    /// The actor's live position (the placement's world position) - what the
    /// moving-arm contact box anchors at.
    pub world: (i32, i32),
    /// Static-arm contact-box centre: the placement's world position plus the
    /// record's collision-footprint offset (`FUN_801CFC40`'s tile-record
    /// derivation; [`legaia_asset::field_objects::Placement::collider_x`]).
    pub collider: (i32, i32),
    /// Flat MAN partition-0 record index of the bind (the prop's script).
    pub record: usize,
    /// The record's bytes (`[u8 n][n*2 name][u8 anim]` header + script), the
    /// buffer a touch interaction runs through the field VM.
    pub record_body: std::sync::Arc<Vec<u8>>,
    /// The parked script cursor (`actor+0x9E`): where the next touch resumes.
    /// Initially just past the spawn prologue's terminating `0x21` (or at the
    /// post-header `pc0` when the record has no `0x24`/`0x25` prologue
    /// marker); updated to wherever an interaction run ends.
    pub parked_pc: usize,
    /// The actor flag word `+0x10`: seeded from the spawn prologue's `0x31`
    /// ops, updated by an interaction run's own `0x31`/`0x32`. Bits `0`/`1`
    /// make the prop collision-exempt (`FUN_801CF754` / `FUN_801CF9F4` skip
    /// `flags & 3` actors) - how an opened door stops blocking.
    pub cflags: u32,
}

impl PropAnimState {
    /// `FUN_801CF754` / `FUN_801CF9F4`'s `flags & 3` filter: the prop no
    /// longer enters the collision candidate list nor the touch probes. Set
    /// by the door's touch pass (`31 00` as the swing starts).
    pub fn collision_exempt(&self) -> bool {
        self.cflags & 3 != 0
    }

    /// `FUN_801CFC40`'s `flags & 0x40020000` class - contact result bit `1`:
    /// blocks like any actor, but the touch is only posted by the
    /// button-gated facing probe (the cupboard interact gate).
    pub fn interact_gated(&self) -> bool {
        self.cflags & 0x4002_0000 != 0
    }

    /// `FUN_801CFC40`'s `flags & 0x1020000` box-source class: live-position
    /// anchor with the moving-actor extents (±40) instead of the static
    /// footprint box (±80).
    pub fn moving_box(&self) -> bool {
        self.cflags & 0x0102_0000 != 0
    }
}

/// Per-scene bank of placed-prop animation runtimes, keyed by the placement's
/// footprint-anchor tile ([`EnvDraw::anchor`]) so the four Rim Elm cupboards
/// each keep their own cursor.
#[derive(Debug, Clone, Default)]
pub struct PropAnimBank {
    /// Live props, keyed by anchor tile.
    pub props: std::collections::BTreeMap<(u8, u8), PropAnimState>,
}

impl PropAnimBank {
    /// Build the bank for a scene: one entry per posed placement (`anim_id !=
    /// 0`), with its bind record's program run through the spawn prologue so
    /// the prop starts in its authored rest state.
    ///
    /// `clip` resolves an anim id to `(frame_count, scaled_step, step_div)`
    /// from the scene's ANM bundle (record `anim_id - 1`); a prop whose clip
    /// does not resolve gets a 1-frame stand-in clip, so its script's
    /// end-latch spin still passes (a missing ANM bundle must not leave a
    /// door's `2D 08` spin - and therefore its collision - stuck forever).
    pub fn build(
        placements: &[Placement],
        binds: &HashMap<(u8, u8), ObjectBind>,
        man_file: &ManFile,
        man: &[u8],
        mut clip: impl FnMut(u8) -> Option<(u16, bool, u8)>,
    ) -> Self {
        let mut bank = PropAnimBank::default();
        for p in placements {
            let anchor = (p.anchor_col, p.anchor_row);
            if bank.props.contains_key(&anchor) {
                continue;
            }
            let Some(bind) = binds.get(&anchor) else {
                continue;
            };
            if bind.anim_id == 0 {
                continue;
            }
            let (frames, scaled, div) = clip(bind.anim_id).unwrap_or((1, false, 0));
            let Some((record, pc0)) = partition0_record(man_file, man, bind.record as usize) else {
                continue;
            };
            let program = decode_prop_program(record, pc0);
            let mut anim = PropAnim::spawned(bind.anim_id, frames, scaled, div);
            for c in &program.spawn {
                c.apply(&mut anim);
            }
            // The spawn pass is run by `FUN_8003A55C` before the first actor
            // tick, and the tick then consumes the restart request it left.
            anim.tick();
            let parked_pc = parked_pc_after_prologue(record, pc0);
            let cflags = program.spawn_cflags;
            bank.props.insert(
                anchor,
                PropAnimState {
                    anim,
                    program,
                    world: (p.world_x, p.world_z),
                    collider: (p.collider_x, p.collider_z),
                    record: bind.record as usize,
                    record_body: std::sync::Arc::new(record.to_vec()),
                    parked_pc,
                    cflags,
                },
            );
        }
        bank
    }

    /// The clip frame prop `anchor` currently poses from, if it has one.
    pub fn frame(&self, anchor: (u8, u8)) -> Option<usize> {
        self.props.get(&anchor).map(|p| p.anim.frame())
    }

    /// Advance every prop's clip one frame (the per-actor anim tick
    /// `FUN_800204F8`, which runs unconditionally - the windmill turns whether
    /// or not anyone is near). Touch / interact dispatch is the world's job
    /// (`World::start_prop_interaction` runs the touched prop's record through
    /// the field VM); this only steps the cursors.
    pub fn tick_anims(&mut self) {
        for p in self.props.values_mut() {
            p.anim.tick();
        }
    }
}

/// The parked script cursor `FUN_8003A55C` leaves on a freshly spawned prop:
/// just past the spawn prologue's terminating own-context `0x21`, or `pc0`
/// itself when the record carries no `0x24`/`0x25` prologue marker (retail
/// skips the prologue run entirely then).
fn parked_pc_after_prologue(record: &[u8], pc0: usize) -> usize {
    use legaia_asset::field_disasm::LinearWalker;
    if !matches!(record.get(pc0), Some(0x24) | Some(0x25)) {
        return pc0;
    }
    for insn in LinearWalker::new(record, pc0).flatten() {
        if insn.opcode == OP_PARK && insn.extended.is_none() {
            return insn.pc + insn.size;
        }
    }
    pc0
}

/// The actor `+0x10` bits MAN partition-0 record `index`'s spawn prologue
/// sets ([`PropProgram::spawn_cflags`]) - the collision / interaction class
/// of the placed object the record binds, available without building a bank
/// entry (an `anim_id == 0` bound placement still carries its class: e.g. a
/// `31 00` born-exempt marker object).
pub fn record_spawn_cflags(man_file: &ManFile, man: &[u8], index: usize) -> u32 {
    match partition0_record(man_file, man, index) {
        Some((record, pc0)) => decode_prop_program(record, pc0).spawn_cflags,
        None => 0,
    }
}

/// A partition-0 record's bytes and the PC just past its
/// `[u8 n][n*2 name bytes][u8 anim_id]` header - where `FUN_8003A55C` parks the
/// actor's script cursor (`actor+0x9E`).
fn partition0_record<'a>(
    man_file: &ManFile,
    man: &'a [u8],
    index: usize,
) -> Option<(&'a [u8], usize)> {
    let start = man_file
        .data_region_offset
        .checked_add(*man_file.partitions[0].get(index)? as usize)?;
    let n = *man.get(start)? as usize;
    let pc0 = 1 + 2 * n + 1;
    // The record runs to the next record start in the flat table (partitions
    // are laid out back to back), or to the end of the MAN.
    let mut end = man.len();
    for part in &man_file.partitions {
        for &off in part {
            let a = man_file.data_region_offset.checked_add(off as usize)?;
            if a > start && a < end {
                end = a;
            }
        }
    }
    let body = man.get(start..end)?;
    if pc0 >= body.len() {
        return None;
    }
    Some((body, pc0))
}

#[cfg(test)]
mod anim_tests {
    use super::*;

    /// A 30-frame clip - the length of Rim Elm's door / cupboard swing.
    fn door(flags: u16, rate: i16) -> PropAnim {
        PropAnim {
            anim_id: 1,
            frames: 30,
            scaled_step: false,
            step_div: 2,
            cursor: 0,
            flags,
            rate,
        }
    }

    /// The state `0x4C 0x35` leaves a prop in - the closed door. Retail's live
    /// Rim Elm actors read `+0x62 = 0x001F`, cursor `0`: the restart has been
    /// consumed, hold + clamp are set, and the cursor never moves.
    #[test]
    fn reset_hold_freezes_the_clip_at_frame_zero() {
        let mut a = door(ANIM_SPAWN_FLAGS, 16);
        AnimCmd::ResetHold.apply(&mut a);
        a.tick(); // consumes the restart request
        assert_eq!(a.flags, 0x001F, "the live actors' resting +0x62");
        for _ in 0..120 {
            a.tick();
            assert_eq!(a.frame(), 0, "a held clip must not advance");
        }
    }

    /// Clearing hold with clamp set plays the clip forward once and stops on
    /// the last frame, latching the end bit. Retail's open door reads
    /// `+0x62 = 0x011D`, cursor `479` = `30 * 16 - 1`.
    #[test]
    fn clearing_hold_plays_forward_and_clamps_open() {
        let mut a = door(ANIM_SPAWN_FLAGS, 16);
        AnimCmd::ResetHold.apply(&mut a);
        a.tick();
        // The house-door touch pass: `2c 07`, `2c 01`, `2b 03`, `2c 08`.
        for c in [
            AnimCmd::ClearBit(7),
            AnimCmd::ClearBit(1),
            AnimCmd::SetBit(3),
            AnimCmd::ClearBit(8),
        ] {
            c.apply(&mut a);
        }
        for _ in 0..60 {
            a.tick();
        }
        assert_eq!(a.cursor, 479, "clamped at the last frame (30 * 16 - 1)");
        assert_eq!(a.frame(), 29);
        assert_eq!(a.flags, 0x011D, "the live open door's +0x62");
        assert!(a.at_end());
    }

    /// The cupboard's second segment (`2b 07`, `2c 01`, `2b 03`) plays the
    /// clip backwards to frame 0 and clamps there - retail's `+0x62 = 0x019D`.
    #[test]
    fn setting_reverse_plays_the_clip_shut() {
        let mut a = door(0x011D, 16);
        a.cursor = 479;
        for c in [AnimCmd::SetBit(7), AnimCmd::ClearBit(1), AnimCmd::SetBit(3)] {
            c.apply(&mut a);
        }
        for _ in 0..60 {
            a.tick();
        }
        assert_eq!(a.cursor, 0);
        assert_eq!(a.flags, 0x019D, "the live closing door's +0x62");
    }

    /// The template flags an actor is born with loop, so an NPC idle clip runs
    /// forever - which is why only the props' own `0x4C 0x35` freezes them.
    #[test]
    fn the_spawn_flags_loop() {
        let mut a = door(ANIM_SPAWN_FLAGS, 8);
        let mut wrapped = false;
        let mut last = 0;
        for _ in 0..200 {
            a.tick();
            if a.frame() < last {
                wrapped = true;
            }
            last = a.frame();
        }
        assert!(wrapped, "no clamp bit -> the cursor wraps");
    }

    /// The house-door script shape, byte for byte from a retail `town01`
    /// partition-0 record (`主人公の家`, anim 1), decodes to: a spawn pass that
    /// sets the rate and resets+holds, and a one-segment touch pass that plays
    /// the clip forward and waits for the end.
    #[test]
    fn a_house_door_record_decodes_to_open_and_wait() {
        // `[u8 n=0][u8 anim=1]` header, then the script.
        let rec: &[u8] = &[
            0x00, 0x01, // header: no name, anim 1
            0x25, // prologue marker
            0x4C, 0x41, 0x20, 0x00, 0x00, 0x00, // rate <- 0x20
            0x4C, 0x35, // reset + hold
            0x21, // park
            0x36, 0x00, 0x80, 0x2E, 0x00, // SFX (the door creak)
            0x2C, 0x07, // clear reverse
            0x2C, 0x01, // clear hold
            0x2B, 0x03, // set clamp
            0x31, 0x00, // (actor local flag, not an anim bit)
            0x2C, 0x08, // clear the end latch
            0x2D, 0x08, // wait for the end latch
            0x21, // park
            0x26, 0xED, 0xFF, // jump back
        ];
        let p = decode_prop_program(rec, 2);
        assert_eq!(p.spawn, vec![AnimCmd::Rate(0x20), AnimCmd::ResetHold]);
        assert_eq!(p.touch.len(), 1);
        assert!(p.touch[0].wait_for_end);
        assert_eq!(
            p.touch[0].cmds,
            vec![
                AnimCmd::ClearBit(7),
                AnimCmd::ClearBit(1),
                AnimCmd::SetBit(3),
                AnimCmd::ClearBit(8),
            ]
        );
        assert!(p.animates());

        // The spawn pass leaves it closed; the touch pass swings it open.
        let mut a = PropAnim::spawned(1, 30, false, 2);
        for c in &p.spawn {
            c.apply(&mut a);
        }
        a.tick();
        assert_eq!(a.frame(), 0);
        assert_eq!(a.rate, 16, "0x20 >> 1");
        for c in &p.touch[0].cmds {
            c.apply(&mut a);
        }
        for _ in 0..40 {
            a.tick();
        }
        assert_eq!(a.frame(), 29, "the door ends fully open");
    }

    /// A record with no animation commands past its park (the locked drawer,
    /// the clock) decodes to a program that never moves the clip.
    #[test]
    fn a_static_prop_record_decodes_to_no_animation() {
        let rec: &[u8] = &[
            0x00, 0x03, // header: no name, anim 3
            0x25, 0x31, 0x00, 0x21, 0x21, 0x26, 0xFE, 0xFF,
        ];
        let p = decode_prop_program(rec, 2);
        assert!(p.spawn.is_empty());
        assert!(!p.animates(), "nothing in the touch pass touches +0x62");
    }
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
            anchor_cell: 0,
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
                anchor: (2, 3),
            }]
        );
    }

    /// The bind carries the `anim_id` onto the draw (so a multi-object prop gets
    /// posed), but it is **not** a spawn gate: an unbound placement is the *other*
    /// retail sweep's actor (`FUN_801D7B50`) and still draws, unposed. Only a
    /// placement that is unbound **and** `CELL_BIND_OWNED` on its anchor tile -
    /// which neither sweep would take - is dropped.
    #[test]
    fn binds_carry_the_anim_id_and_only_bind_owned_cells_drop() {
        let env_tmds = vec![10, 11, 12];
        let mut bound = placement(Some(1), None, 0);
        bound.anchor_col = 7;
        bound.anchor_row = 9;
        bound.anchor_cell = legaia_asset::field_objects::CELL_BIND_OWNED;
        let unbound = placement(Some(2), None, 0); // anchor (2, 3): no bind, no 0x400
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
        assert_eq!(
            draws.len(),
            2,
            "the unbound placement is the window sweep's"
        );
        assert_eq!((draws[0].env_slot, draws[0].anim_id), (1, 2));
        assert_eq!(
            (draws[1].env_slot, draws[1].anim_id),
            (2, 0),
            "the window sweep does no bind lookup, so its actor has no clip"
        );
        assert!(drops.is_empty());

        // Unbound *and* bind-owned: neither retail sweep takes it.
        let mut orphan = placement(Some(2), None, 0);
        orphan.anchor_cell = legaia_asset::field_objects::CELL_BIND_OWNED;
        let (draws, drops) = resolve_placed_env_draws(&env_tmds, &[orphan], None, Some(&binds));
        assert!(draws.is_empty());
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

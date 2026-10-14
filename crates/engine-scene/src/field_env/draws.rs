//! The environment draw list: `EnvDraw`, the scene's environment pack lookup, the resolver
//! that turns it into draws, and the world-space bounds of a draw list.

use super::*;

/// One resolved environment draw: a scene-pack mesh instanced at a world
/// position. `world_*` are PSX field-frame coordinates (retail Y-down); the
/// caller applies its own render-frame flip.
/// The record-flag bits the decoration pass `FUN_801F7088` turns into a
/// camera-facing matrix (`andi v0,v0,0x380` at `0x801F7714`); see
/// [`EnvDraw::view_skip`].
pub const DECORATION_VIEW_SKIP_MASK: u16 = 0x0380;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvDraw {
    /// Index into the environment-pack subset (the [`env_pack_tmd_indices`]
    /// order) - i.e. the placement's resolved pack index.
    pub env_slot: usize,
    /// Index into `res.tmds` for the mesh this draw instances.
    pub res_tmd: usize,
    /// World X (`col*0x80 + x_off + 0x40`).
    pub world_x: i32,
    /// World Y. Placed objects: `-lut[floor_nibble] + y_off` (`FUN_8003A55C`).
    /// Terrain / decoration cells (`floor_corner_nibbles` present): the
    /// round-toward-zero average of the four corner tiles' `-lut[nibble]`
    /// heights plus `y_off` (`FUN_801F69D8` / `FUN_801F7088`). `0` without a
    /// LUT / nibble.
    pub world_y: i32,
    /// World Z (`row*0x80 - (z_off - 0x40)`).
    pub world_z: i32,
    /// Yaw in PSX angle units (`4096` = full revolution), from the object
    /// record's `+0x0A` field (see [`Placement::rot_y`]): the authored mesh
    /// orientation (bridge quarter-turns, tree variety). For a pure-Y angle
    /// retail's matrix builder (`FUN_80026988`) maps local `+Z` to
    /// `(sin, 0, cos)` in the retail Y-down frame - `glam`'s
    /// `Mat4::from_rotation_y` with the same angle reproduces it exactly.
    pub rot_y: u16,
    /// Pitch / roll in the same PSX angle units, from the object record's
    /// `+0x08` and `+0x0C`.
    ///
    /// Retail feeds all three angles to the Euler matrix builder
    /// (`FUN_8003A55C` copies record `+0x08`/`+0x0A`/`+0x0C` to actor
    /// `+0x24`/`+0x26`/`+0x28`; `FUN_8001ADA4` passes `actor+0x24` to
    /// `FUN_80026988`, which composes `Rx * Ry * Rz`). Most placements are
    /// pure yaw, but a minority across the disc carry a real tilt - and
    /// dropping it does more than leave a prop upright: a mesh authored off
    /// its own origin also lands in the wrong place, because the rotation is
    /// about the origin, not the geometry's centre.
    // REF: FUN_80026988, FUN_8003A55C, FUN_8001ADA4
    pub rot_x: u16,
    /// See [`Self::rot_x`].
    pub rot_z: u16,
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
    /// Which rungs of the scene floor-height ladder [`Self::world_y`] was
    /// resolved from, so a host can re-resolve it when the ladder **moves**
    /// under a baked draw list - see [`FloorAnchor`] and [`FloorWave`].
    pub floor: FloorAnchor,
    /// The object-grid cell `(col, row)` whose word selected this draw's
    /// record - the cell retail's per-cell decoration pass (`FUN_801F7088`)
    /// walks, and so the key the camera's visible-tile crop
    /// (`crate::field_view_window`) tests.
    pub cell: (u8, u8),
    /// The record's `+0x1E` cull radius ([`Placement::cull_radius`]), which
    /// widens that per-cell window test.
    pub cull_radius: u8,
    /// The **camera-facing** bits of a terrain / decoration cell's record
    /// flags (`+0x12 & 0x380`; `0` for a placed object). The decoration pass
    /// `FUN_801F7088` tests them at `0x801F770C..0x801F779C`: set, it rebuilds
    /// the cell's matrix from the base matrix and the camera rotation with
    /// the flagged axes left out (`0x80` pitch, `0x100` yaw, `0x200` roll),
    /// before the record's own rotation - so a flagged cell keeps facing the
    /// camera on those axes. `rugi`'s candle glows (`0x80`), the forest
    /// trees of `vell` / `vozz` (`0x180`) and the `0x380` billboards of
    /// `deene` / `retona` are such cells. Hosts resolve the basis per frame
    /// through `legaia_render_kernels::gte::decoration_cell_basis`.
    // REF: FUN_801F7088
    pub view_skip: u16,
}

impl EnvDraw {
    /// Place one object-local vertex in the Y-down world frame: the record's
    /// three authored angles composed `Rx * Ry * Rz` (retail's
    /// `FUN_80026988`, the order [`Self::rot_x`] documents), then the draw's
    /// world position plus a coplanar `lift`.
    ///
    /// This is the composition the native window's placement passes build
    /// with `legaia_render_kernels::battle_intro::placement_rotation`, as one
    /// kernel for the bakes that write world-space vertices themselves (the
    /// dance hall and the fishing venue, on every host). A bake that reads
    /// [`Self::rot_y`] alone stands every tilted placement upright - the
    /// dance hall's 21 tilted draws include its three video walls, which a
    /// yaw-only bake buried inside the back wall.
    // REF: FUN_80026988
    pub fn place_point(&self, p: [f32; 3], lift: [f32; 3]) -> [f32; 3] {
        let [x, y, z] = self.rotate(p);
        [
            x + self.world_x as f32 + lift[0],
            y + self.world_y as f32 + lift[1],
            z + self.world_z as f32 + lift[2],
        ]
    }

    /// The rotation half of [`Self::place_point`]: `Rx * Ry * Rz` applied to
    /// a direction (a normal, an extent corner), no translation.
    // REF: FUN_80026988
    pub fn rotate(&self, v: [f32; 3]) -> [f32; 3] {
        rotate_euler([self.rot_x, self.rot_y, self.rot_z], v)
    }
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
    /// [`CELL_BIND_OWNED`](legaia_scene_formats::field_objects::CELL_BIND_OWNED) (so the
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
/// [`legaia_scene_formats::field_objects::Placement`] list) into environment draws.
///
/// `env_tmds` is the [`env_pack_tmd_indices`] subset; `floor_lut` is the
/// scene's MAN floor-height LUT (`Scene::field_floor_height_lut`). World Y
/// follows the retail sweep the placement list came from: a terrain /
/// decoration cell (its [`Placement::floor_corner_nibbles`] is `Some`)
/// averages the four corner tiles' `-lut[nibble]` heights (round toward
/// zero) before adding `y_off` - the `FUN_801F69D8` / `FUN_801F7088` per-cell
/// emitter math - while a placed object uses the single placement-tile
/// nibble, `-lut[nibble & 0xF] + y_off` (`FUN_8003A55C`). Without a LUT or
/// nibble the draw lands on the ground plane.
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

/// World-space bounding box of a scene's static environment draws.
///
/// Each mesh's object-local vertex extent is carried through its own
/// placement transform - `T(world_xyz) . Ry(rot_y)`, the yaw-only form
/// `crate::field_occlusion::FieldOccluders::add_instanced_mesh` already
/// instances with - and the boxes are unioned. `ground`, when present, is
/// already world-space (`WalkHeightfield::positions` are
/// `(col*128, -lut[nibble], row*128)`) and joins the union directly.
///
/// The frame is retail **Y-down**, the same frame [`EnvDraw`] and the
/// heightfield are authored in; a host applies its own flip afterwards.
///
/// Why this exists: framing a scene off the meshes' *local* extents boxes the
/// authoring origin rather than the map, because every env-pack mesh is
/// authored about its own origin while the geometry that draws sits at
/// placement coordinates up to `0x8000`. A top-view camera handed the local
/// union frames the origin corner and the map sits off-centre.
///
/// Returns `None` when no draw resolves a mesh and no ground is supplied.
pub fn env_draws_world_aabb(
    draw_lists: &[&[EnvDraw]],
    res: &crate::scene_resources::SceneResources,
    ground: Option<&legaia_scene_formats::field_objects::WalkHeightfield>,
) -> Option<([f32; 3], [f32; 3])> {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    let mut any = false;

    // One local extent per distinct mesh - the same mesh is instanced dozens
    // of times across a terrain list, and the extent does not depend on the
    // instance.
    let mut local: std::collections::HashMap<usize, ([f32; 3], [f32; 3])> =
        std::collections::HashMap::new();

    for list in draw_lists {
        for d in list.iter() {
            let ext = match local.get(&d.res_tmd) {
                Some(e) => *e,
                None => {
                    let Some(rtmd) = res.tmds.get(d.res_tmd) else {
                        continue;
                    };
                    let mut mlo = [f32::INFINITY; 3];
                    let mut mhi = [f32::NEG_INFINITY; 3];
                    for obj in &rtmd.tmd.objects {
                        for v in &obj.vertices {
                            let p = [f32::from(v.x), f32::from(v.y), f32::from(v.z)];
                            for ax in 0..3 {
                                mlo[ax] = mlo[ax].min(p[ax]);
                                mhi[ax] = mhi[ax].max(p[ax]);
                            }
                        }
                    }
                    if !mlo[0].is_finite() {
                        continue;
                    }
                    let e = (mlo, mhi);
                    local.insert(d.res_tmd, e);
                    e
                }
            };
            // Rotate the local box's eight corners, then translate. Rotating
            // the box rather than the vertices is conservative by at most the
            // box's own slack and costs one pass per placement instead of one
            // per vertex.
            for cx in [ext.0[0], ext.1[0]] {
                for cy in [ext.0[1], ext.1[1]] {
                    for cz in [ext.0[2], ext.1[2]] {
                        let w = d.place_point([cx, cy, cz], [0.0; 3]);
                        for ax in 0..3 {
                            lo[ax] = lo[ax].min(w[ax]);
                            hi[ax] = hi[ax].max(w[ax]);
                        }
                        any = true;
                    }
                }
            }
        }
    }

    if let Some(g) = ground {
        for p in &g.positions {
            for ax in 0..3 {
                lo[ax] = lo[ax].min(p[ax]);
                hi[ax] = hi[ax].max(p[ax]);
            }
            any = true;
        }
    }

    any.then_some((lo, hi))
}

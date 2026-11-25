//! What a field frame draws, read off engine state: the inputs of the field
//! frame-cost model that stands in for the frame time retail's pacer
//! `FUN_80016B6C` measures.
//!
//! Retail steps a field frame by the longest of its last sixteen frame times
//! (`DAT_80084098`) against the `0xF0` / `0x1FE` / `0x2D0` hblank rungs, over
//! the floor of `2` the field init stores (`0x801D6990`). The engine ports
//! that law (`FrameStepTelemetry::resolve`) but has no frame time to measure,
//! so without a cost it holds the floor. This module counts, from the scene's
//! static tables and the cull view the follow camera publishes, what the
//! frame's render passes walk:
//!
//! - the decoration pass `FUN_801F7088`'s per-cell pack meshes that pass its
//!   gate ([`ViewCells::decoration_visible`]), as draws and as primitives;
//! - the ground emitter's walk-visible cells ([`ViewCells::ground_visible`]
//!   over the `.MAP` grid's `0x1000` bit);
//! - the placed objects and the placement NPCs the actor cull `FUN_801D79E8`
//!   leaves visible;
//! - the live fog records and the live ambient move-VM parts.
//!
//! None of it reads renderer state: the table is built from the scene's
//! `.MAP` and pack meshes at scene entry, the gates are the ported retail
//! gates, so the native window, the headless runs and the browser page see the
//! same numbers.
//!
//! REF: FUN_80016B6C, FUN_801F7088, FUN_801F6D48, FUN_801D79E8

use super::World;
use super::npc_morph::MorphOwner;
use legaia_engine_scene::field_view_window::{ViewCells, view_cells};

/// What a mesh hands the prim dispatcher `FUN_80043390`: its TMD objects
/// (one dispatcher draw each), its primitive groups (a group decode and a
/// leaf set-up each) and its primitives by leaf kind (`flags >> 1`, kinds
/// `8..=19`, index `kind - 8`), the units the derived frame cost prices
/// ([`super::field_frame_units`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeshCost {
    pub objects: u16,
    /// The vertices of all its objects.
    pub verts: u16,
    pub groups: u16,
    pub kinds: [u16; 12],
}

impl MeshCost {
    /// The mesh as an actor whose clip binds `bones` objects draws it: the
    /// object loop runs over the clip's bone count (`FUN_8001B964`), so the
    /// objects past it are not drawn. Groups and primitives by kind scale by
    /// the drawn objects' share of the primitives.
    /// `prims_through[i]` is the primitives of its first `i + 1` objects.
    pub fn cut_to(&self, bones: usize, prims_through: &[u16]) -> MeshCost {
        let n = usize::from(self.objects);
        if bones == 0 || bones >= n || prims_through.len() < n {
            return *self;
        }
        let total = u32::from(prims_through[n - 1]).max(1);
        let kept = u32::from(prims_through[bones - 1]);
        let scale = |v: u16| (u32::from(v) * kept / total) as u16;
        MeshCost {
            objects: bones as u16,
            verts: scale(self.verts),
            groups: scale(self.groups),
            kinds: self.kinds.map(scale),
        }
    }
}

/// One decoration draw of the scene's terrain layer: its `.MAP` cell, its
/// record's cull radius and its pack mesh's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostCell {
    pub col: u8,
    pub row: u8,
    pub radius: u8,
    pub prims: u16,
    pub verts: u16,
    /// Of [`Self::prims`]: the quads, and the light-source rows (group
    /// flags `0x10..=0x17`, the dispatcher's `NCCS` / `NCCT` handlers).
    pub quads: u16,
    pub lit: u16,
    pub mesh: MeshCost,
    /// The draw's world position and record angles (`+0x08 / +0x0A /
    /// +0x0C`), its camera-facing bits (`+0x12 & 0x380`) and its leaf bank:
    /// the colour word's top byte is non-zero (`cued`: a cull radius or
    /// `+0x12 & 0x800`) and carries the off-screen-test bit `0x40000000`
    /// (`offtest`: a cull radius), `FUN_801F7088` at `0x801F7898..0x801F78C8`.
    pub pos: [i32; 3],
    pub angles: [u16; 3],
    pub view_skip: u16,
    pub cued: bool,
    pub offtest: bool,
    /// Index into [`FieldCostTable::decoration_geoms`], `u32::MAX` for none.
    pub geom: u32,
}

/// One placed object: its world position, cull radius, how the draw walk
/// treats it, and its mesh size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostPlaced {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    pub radius: u8,
    /// Posed through a bound clip (draw kind `1`, `+0x5C != 0`); else the
    /// static single-transform arm (draw kind `5`).
    pub posed: bool,
    /// The record's `+0x12 & 0x20` (actor `+0x52 & 0x20`): the walk's near
    /// reject passes over it.
    pub near_exempt: bool,
    /// The window sweep's key for a placement it owns (no object bind on the
    /// anchor tile, `field_env::placed_window_key`); `None` for the
    /// scene-init sweep's, whose actor lives for the whole scene.
    pub window: Option<legaia_engine_scene::field_env::PlacedWindowKey>,
    /// The object key the bound actor's state is found under
    /// (`field_env::placed_object_key`); `None` for a window-owned placement.
    pub object: Option<usize>,
    /// The partition-0 record the anchor tile binds, which owns the actor's
    /// op-`0x4B` morph lanes ([`MorphOwner::Object`]).
    pub bind: Option<u16>,
    pub prims: u16,
    pub verts: u16,
    /// Of [`Self::prims`]: the quads, and the light-source rows (group
    /// flags `0x10..=0x17`, the dispatcher's `NCCS` / `NCCT` handlers).
    pub quads: u16,
    pub lit: u16,
    pub mesh: MeshCost,
}

/// A field scene's static cost inputs, built once at scene entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldCostTable {
    /// The terrain layer's decoration draws.
    pub decorations: Vec<CostCell>,
    /// The placed-object layer.
    pub placed: Vec<CostPlaced>,
    /// The walk-visible ground cells, one bit a cell: row `r`'s bit `c`.
    /// Empty for a scene with no `.MAP` grid.
    pub ground: Vec<u128>,
    /// The ground emitter's per-cell inputs beside [`Self::ground`]: each
    /// cell's height tier (the low nibble of the `.MAP` byte at `+0x4000`,
    /// `128 * row + col`), and its depth-sorted bit (object-grid word bit
    /// `0x8000`) one bit a cell as `ground` holds them. Empty when the scene
    /// has no grid.
    pub ground_tiers: Vec<u8>,
    pub ground_sorted: Vec<u128>,
    /// Each placement slot's spawn mesh size in primitives, by slot (`0` for
    /// a slot with no resolvable mesh).
    pub npc_prims: Vec<u16>,
    /// Each placement slot's spawn mesh as the dispatcher sees it, by slot.
    pub npc_meshes: Vec<MeshCost>,
    /// Each placement slot's spawn tile centre `(x, z)`, by slot.
    pub npc_spawns: Vec<(i16, i16)>,
    /// Each placement slot's spawn mesh, the primitives of its first
    /// `i + 1` objects at `[i]` ([`MeshCost::cut_to`]).
    pub npc_prims_through: Vec<Vec<u16>>,
    /// The player's model (PROT 0874 §0 head `0`), which the actor passes
    /// draw every field frame and no placement holds.
    pub player: Option<MeshCost>,
    /// The decoration draws' pack meshes as their leaves walk them, one per
    /// distinct mesh ([`CostCell::geom`]).
    pub decoration_geoms: Vec<super::field_deco_outcomes::DecoGeom>,
}

impl FieldCostTable {
    /// The walk-visible bits of a `.MAP` image's object grid (`+0x8000`,
    /// cell bit `0x1000`), the cells the ground emitter draws a quad on.
    pub fn ground_from_map(map: &[u8]) -> Vec<u128> {
        use legaia_scene_formats::field_objects::{
            CELL_WALK_VISIBLE, GRID_DIM, OBJECT_GRID_OFFSET,
        };
        let Some(grid) = map.get(OBJECT_GRID_OFFSET..) else {
            return Vec::new();
        };
        (0..GRID_DIM)
            .map(|row| {
                (0..GRID_DIM).fold(0u128, |bits, col| {
                    let off = (row * GRID_DIM + col) * 2;
                    let cell = grid
                        .get(off..off + 2)
                        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
                    if cell & CELL_WALK_VISIBLE != 0 {
                        bits | (1u128 << col)
                    } else {
                        bits
                    }
                })
            })
            .collect()
    }

    /// The `.MAP` image's height tiers and depth-sorted bits
    /// ([`Self::ground_tiers`], [`Self::ground_sorted`]).
    pub fn ground_tiers_from_map(map: &[u8]) -> (Vec<u8>, Vec<u128>) {
        use legaia_scene_formats::field_objects::{
            CELL_GROUND_DEPTH_SORTED, COLLISION_GRID_OFFSET, GRID_DIM, OBJECT_GRID_OFFSET,
        };
        let (Some(tiers), Some(grid)) = (
            map.get(COLLISION_GRID_OFFSET..COLLISION_GRID_OFFSET + GRID_DIM * GRID_DIM),
            map.get(OBJECT_GRID_OFFSET..OBJECT_GRID_OFFSET + GRID_DIM * GRID_DIM * 2),
        ) else {
            return (Vec::new(), Vec::new());
        };
        let sorted = (0..GRID_DIM)
            .map(|row| {
                (0..GRID_DIM).fold(0u128, |bits, col| {
                    let off = (row * GRID_DIM + col) * 2;
                    let cell = u16::from_le_bytes([grid[off], grid[off + 1]]);
                    bits | (u128::from(cell & CELL_GROUND_DEPTH_SORTED != 0) << col)
                })
            })
            .collect();
        (tiers.iter().map(|b| b & 0xF).collect(), sorted)
    }

    fn ground_sorted_at(&self, col: i32, row: i32) -> bool {
        (0..128).contains(&col)
            && self
                .ground_sorted
                .get((row & 0x7F) as usize)
                .is_some_and(|bits| bits >> col & 1 != 0)
    }

    /// The height of the corner at `(col, row)` under the live floor ladder.
    fn ground_height(&self, col: i32, row: i32, lut: &[i16; 16]) -> i16 {
        let i = ((row & 0x7F) * 128 + (col & 0x7F)) as usize;
        self.ground_tiers
            .get(i)
            .map_or(0, |&t| lut[usize::from(t & 0xF)])
    }

    fn ground_at(&self, col: i32, row: i32) -> bool {
        if !(0..128).contains(&col) {
            return false;
        }
        self.ground
            .get((row & 0x7F) as usize)
            .is_some_and(|bits| bits >> col & 1 != 0)
    }
}

/// The frame time, in hblanks, a field frame that draws `f` costs.
///
/// Every weight but the cyclers' is **fitted**, not read off a routine: the counts are what
/// the ported gates say the frame's passes walk, and the weights price them
/// against retail's measured frame times (PCSX-Redux walk films' `frame_cost`
/// and `step` columns, and the sixteen-sample ring of every retail library
/// field state). The weights start from the frame profile's per-routine
/// times (`autorun_field_frame_profile.lua`: the decoration pass's
/// primitives and the cells its loop visits, the ground emitter's cells, the
/// actor draw pass's primitives and NPCs, the fog walk's flat cost) and
/// climb on how many measured frames land on retail's rung of the step law.
/// The CLUT-cell cyclers' recolour is priced at what the kernel was measured
/// to cost a texel. Integer arithmetic, so every host prices a frame alike; weights in
/// 1/1024 hblank.
///
/// Not capped: a frame priced past the `0x2D0` rung is a spike, which the
/// pacer stores as `0x2BC` and steps at `4` on the frame itself
/// (`0x8001709C`, `0x800170F8`), as retail runs `concend` on every frame.
pub fn field_frame_cost(f: &FieldFrameFeatures) -> i32 {
    let terms: [(u32, i64); 8] = [
        (f.decoration_prims, COST_PER_DECORATION_PRIM),
        (f.decoration_loop_cells, COST_PER_DECORATION_LOOP_CELL),
        (f.ground_cells, COST_PER_GROUND_CELL),
        (f.placed_prims, COST_PER_PLACED_PRIM),
        (f.npcs, COST_PER_NPC),
        (f.fog, COST_PER_FOG_RECORD),
        (f.fog_quads, COST_PER_FOG_HALF_SHEET),
        (f.parts, COST_PER_PART),
    ];
    let cycler: [(u32, i64); 3] = [
        (f.cycler_calls, COST_PER_CYCLER_CALL),
        (f.cycler_texels, COST_PER_CYCLER_TEXEL),
        (f.cycler_zero_texels, COST_PER_CYCLER_ZERO_TEXEL),
    ];
    let sum: i64 = terms
        .iter()
        .chain(&cycler)
        .map(|&(n, w)| i64::from(n) * w)
        .sum();
    (FIELD_FRAME_BASE_COST + (sum >> 10)).clamp(0, FIELD_FRAME_COST_MAX) as i32
}

/// The hblanks a field frame costs with nothing in view (fitted).
pub const FIELD_FRAME_BASE_COST: i64 = 33;
/// The most [`field_frame_cost`] prices a frame at: a bound on the integer,
/// far past the `0x2D0` rung, not a model term.
pub const FIELD_FRAME_COST_MAX: i64 = 0x7FFF;
/// Weights of [`field_frame_cost`], 1/1024 hblank a unit (fitted): a
/// decoration primitive, a cell of the decoration loop's span, a ground
/// cell, a placed-object primitive, a visible placement NPC, a live fog
/// record, an emitted fog half-sheet and a live ambient part.
pub const COST_PER_DECORATION_PRIM: i64 = 93;
pub const COST_PER_DECORATION_LOOP_CELL: i64 = 55;
pub const COST_PER_GROUND_CELL: i64 = 162;
pub const COST_PER_PLACED_PRIM: i64 = 121;
pub const COST_PER_NPC: i64 = 25_364;
pub const COST_PER_FOG_RECORD: i64 = 207;
pub const COST_PER_FOG_HALF_SHEET: i64 = 6;
pub const COST_PER_PART: i64 = 1_426;
/// What the CLUT-cell recolour `FUN_80019D50` costs, 1/1024 hblank a unit
/// (measured, not fitted): a call, a non-zero texel (`RGB -> HSV -> RGB`,
/// `FUN_8001A78C` / `FUN_8001A6C8`) and a zero texel, which the kernel
/// copies through (`0x80019DF4`). Cycle-stamped on PCSX-Redux across
/// `opstati`'s `320 x 34` and `320 x 2` strips and `jou`'s sixteen-texel
/// rows: `1185`, `74` and `5` hblanks a call.
pub const COST_PER_CYCLER_CALL: i64 = 1_311;
pub const COST_PER_CYCLER_TEXEL: i64 = 234;
pub const COST_PER_CYCLER_ZERO_TEXEL: i64 = 38;

/// Whether the field draw walk `FUN_8001ADA4` drops placed object `p`
/// before its primitives under the frame's camera `gte`, past the tile cull.
///
/// The walk transforms each actor's origin (`RTPS` at `0x8001AE20`, the
/// view-space result into `+0x2C..+0x34`; every placed object carries
/// `+0x10 & 0x80`) and its draw arms then test the depth `+0x34`:
///
/// ```text
/// kind 5 (0x8001B1A8): +0x72 != 0 && depth < 0xA0 && !(+0x52 & 0x20) -> skip
/// kind 1 (0x8001AE94): depth < 0xA1 && !(+0x52 & 0x20)              -> skip
/// ```
///
/// A skipped object costs its origin's transform and none of its
/// primitives. The render scale `+0x72` is the spawn's `0x1000`
/// (`0x80020F30`), so the model takes the kind-5 test as live. A retail
/// capture of `vahn_walks_out` (Mednafen) holds the four `town01` cliff
/// sections at `(1408..5632, 0, 960)` at depths `-9999..-3731`, past the
/// tile cull and dropped here.
///
/// REF: FUN_8001ADA4
pub fn placed_near_rejected(
    p: &CostPlaced,
    gte: &legaia_render_kernels::gte::FieldGteView,
) -> bool {
    if p.near_exempt {
        return false;
    }
    let depth = gte.rtps([p.x, p.y, p.z]).mac[2];
    walker_near_rejected(depth, p.posed, false)
}

/// The draw walk's near reject on an actor's view depth `+0x34`: under
/// `0xA1` on the posed arms (kind 1, `slti v0,v0,0xa1` at `0x8001AE9C`; a
/// kind-4 node in mode 6), under `0xA0` on the scaled model arm, unless
/// `+0x52 & 0x20` exempts the actor (`near_exempt`).
///
/// PORT: FUN_8001ADA4 (`0x8001AE94..0x8001AEB8`, the kind-1 near reject)
pub fn walker_near_rejected(depth: i32, posed: bool, near_exempt: bool) -> bool {
    !near_exempt && depth < if posed { 0xA1 } else { 0xA0 }
}

/// The render dispatcher's on-screen probe `FUN_8001B73C` for an actor at
/// `[x, y, z]` with cull radius `r` (`+0x58`): with `h = (r + 1) * 64`, the
/// eight corners of the box `h` either side of its origin in X and Z, from
/// `y` up to `y - 2h`, and the point `h` above the box's top centre (four
/// `RTPT`s, the last re-projecting the third's three points), each in 16-bit
/// stack words; on screen when any lands at `0..320` across (an unsigned
/// test) and `0..=240` down.
// PORT: FUN_8001B73C
pub fn actor_box_on_screen(
    g: &legaia_render_kernels::gte::FieldGteView,
    [x, y, z]: [i16; 3],
    r: i16,
) -> bool {
    let h = r.wrapping_add(1).wrapping_shl(6);
    let top = y.wrapping_sub(r.wrapping_add(1).wrapping_shl(7));
    let (x0, x1) = (x.wrapping_sub(h), x.wrapping_add(h));
    let (z0, z1) = (z.wrapping_sub(h), z.wrapping_add(h));
    [
        [x0, y, z0],
        [x1, y, z0],
        [x0, y, z1],
        [x0, top, z0],
        [x1, top, z0],
        [x0, top, z1],
        [x1, y, z1],
        [x1, top, z1],
        [x, top.wrapping_add(h), z],
    ]
    .into_iter()
    .any(|v| {
        let (sx, sy) = g.rtps(v).sxy;
        (sx as u16) < 0x140 && (0..0xF1).contains(&sy)
    })
}

/// The matrix `FUN_800172C0` builds from the camera globals `g`: the field
/// base matrix times the angles' rotation, the eye trio as `TR` past the
/// focus. Only its `MAC3` is read, so the screen constants are the field's.
///
/// REF: FUN_800172C0
pub fn placed_cull_gte(
    g: &legaia_engine_camera::retail_cam::RetailCamGlobals,
) -> legaia_render_kernels::gte::FieldGteView {
    use legaia_render_kernels::gte::FieldGteView;
    let a = g.angles().map(|v| (v & 0xFFF) as u16);
    FieldGteView::from_globals(
        a,
        g.tr_eye(),
        g.focus_stored(),
        &FieldGteView::field_base(),
        g.h().clamp(0, 0xFFFF) as u16,
        legaia_render_kernels::gte::FIELD_OFX,
        legaia_engine_camera::projection::GTE_OFY as i32,
    )
}

impl FieldFrameFeatures {
    fn add_morph(&mut self, [calls, copied, verts]: [u32; 3]) {
        self.morph_calls += calls;
        self.morph_copied += copied;
        self.morph_vertices += verts;
    }

    fn add_placed(&mut self, p: &CostPlaced) {
        self.placed_objects += u32::from(p.mesh.objects);
        self.placed_groups += u32::from(p.mesh.groups);
        // A cull radius raises the colour word's off-screen-test bit, which
        // sends every primitive through the depth-cued leaves' test.
        let bank = if p.radius != 0 {
            &mut self.placed_offtest_kinds
        } else {
            &mut self.placed_kinds
        };
        for (k, n) in bank.iter_mut().zip(p.mesh.kinds) {
            *k += u32::from(n);
        }
        self.placed_draws += 1;
        self.placed_prims += u32::from(p.prims);
        self.placed_verts += u32::from(p.verts);
        self.placed_quads += u32::from(p.quads);
        self.placed_lit += u32::from(p.lit);
    }
}

/// What one field frame draws ([`World::field_frame_features`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldFrameFeatures {
    pub decoration_draws: u32,
    pub decoration_prims: u32,
    pub decoration_verts: u32,
    pub ground_cells: u32,
    pub placed_draws: u32,
    pub placed_prims: u32,
    pub placed_verts: u32,
    pub decoration_quads: u32,
    pub decoration_lit: u32,
    pub placed_quads: u32,
    pub placed_lit: u32,
    /// Placed objects past the tile cull that retail's draw walk never
    /// reaches, with their primitives: window-owned ones the window list
    /// does not hold (`unlisted`), and ones the walk's near reject drops
    /// ([`placed_near_rejected`], `near`). Counted whether or not
    /// [`WorldToggles::field_cost_placed_walk`] takes them out of the placed
    /// counts.
    ///
    /// [`WorldToggles::field_cost_placed_walk`]: super::WorldToggles::field_cost_placed_walk
    pub placed_unlisted_draws: u32,
    pub placed_unlisted_prims: u32,
    /// Of the `unlisted`: bound placements whose actor its spawn prologue
    /// parked at the off-map hide box or retired.
    pub placed_parked_draws: u32,
    pub placed_near_draws: u32,
    pub placed_near_prims: u32,
    pub npcs: u32,
    /// Of [`Self::npcs`]: their spawn meshes' primitives.
    pub npc_prims: u32,
    /// The decoration pass's loop: the cells its pre-clamp extents visit
    /// (`ViewCells::span`), each a gate test whether it draws or not.
    pub decoration_loop_cells: u32,
    pub fog: u32,
    /// Of [`Self::fog`]'s records: the half-sheets the last walk emitted
    /// (`FUN_8003F86C` past its screen cull), each a `POLY_FT4` the GPU
    /// fills.
    pub fog_quads: u32,
    pub parts: u32,
    /// The CLUT-cell cyclers uploading on the frame's tick, and the
    /// non-zero and zero texels of their captures (`FUN_80019D50`).
    pub cycler_calls: u32,
    pub cycler_texels: u32,
    pub cycler_zero_texels: u32,
    /// The derived cost's finer counts ([`super::field_frame_units`]): the
    /// decoration loop's rows and the cells of its span inside the walk
    /// box; the ground window's rows and cells; each layer's dispatcher
    /// objects, primitive groups and primitives by leaf kind (`kind - 8`);
    /// and the actors the five render lists walk.
    pub decoration_rows: u32,
    pub decoration_box_cells: u32,
    pub decoration_groups: u32,
    pub decoration_kinds: [u32; 12],
    /// Of [`Self::decoration_kinds`]: the primitives priced by their leaf
    /// outcome ([`super::field_deco_outcomes`]), and what they cost, in
    /// cycles x 10.
    pub decoration_outcome_kinds: [u32; 12],
    pub decoration_outcome_cycles_x10: u64,
    pub ground_rows: u32,
    pub ground_window_cells: u32,
    /// Of [`Self::ground_cells`]: those the emitter's near test drops before
    /// the quad, and those it draws through the depth-sorted path.
    pub ground_far_cells: u32,
    pub ground_sorted_cells: u32,
    pub placed_objects: u32,
    pub placed_groups: u32,
    /// The placed meshes' primitives through the opaque leaves.
    pub placed_kinds: [u32; 12],
    /// The placed meshes' primitives that take the off-screen-tested
    /// depth-cued leaves (a non-zero cull radius).
    pub placed_offtest_kinds: [u32; 12],
    /// The placements whose meshes the hosts draw this frame (the derived
    /// model's count; [`Self::npcs`] is the fitted model's).
    pub npc_models: u32,
    pub npc_objects: u32,
    pub npc_groups: u32,
    pub npc_kinds: [u32; 12],
    /// The models' primitives through the depth-cued leaves (a colour word
    /// with a non-zero mode byte).
    pub npc_cued_kinds: [u32; 12],
    /// Of the models: the objects, and their primitives, that a raised
    /// `+0x42` (field-VM `4C C2`) sends through the object-effect clip arm,
    /// `FUN_8001C204` and the TMD renderer `FUN_8002735C`, in place of the
    /// dispatcher (`0x8001BC34..0x8001BC64`, `0x8001BD80..0x8001BD88`).
    pub npc_clip_objects: u32,
    pub npc_clip_prims: u32,
    /// Of [`Self::npc_clip_prims`]: those of models whose anchor stands
    /// inside the row's slab, which the renderer keeps and links
    /// ([`World::effect_clip_keeps_anchor`]).
    pub npc_clip_kept_prims: u32,
    pub list_actors: u32,
    /// The live script actors (field-VM channels: placements and bound
    /// objects), each an entry of the tick and render lists.
    pub tick_actors: u32,
    /// Whether the player moved on the last tick.
    pub player_moving: bool,
    /// The live move-VM parts on draw kind `4` (the sprite emitter).
    pub sprite_parts: u32,
    /// Of [`Self::sprite_parts`]: those on the default arm (`+0x9E & 0x6000`
    /// clear), and the packets their builds write
    /// (`effect_default_arm::default_arm_packet_count`).
    pub default_arm_parts: u32,
    pub default_arm_packets: u32,
    /// Of [`Self::default_arm_packets`]: those built under a texture mode.
    pub default_arm_textured_packets: u32,
    /// The draws' morph substitution: `FUN_8001C604` calls, the vertices
    /// they copy, and the vertices `FUN_8005B038` blends.
    pub morph_calls: u32,
    pub morph_copied: u32,
    pub morph_vertices: u32,
    /// The player's model, when the scene's table holds it.
    pub player: Option<MeshCost>,
    /// The player's `+0x42` is raised: its model takes the clip arm.
    pub player_clipped: bool,
    /// The raised player's anchor stands inside its row's slab.
    pub player_clip_kept: bool,
}

impl World {
    /// Install the scene's static cost inputs (a scene entry's), or drop them.
    /// The pacer's ring starts the scene empty: the mode transition
    /// `FUN_80016230` zeroes all sixteen entries (`0x80016234..0x80016274`)
    /// on every mode change, a field load's included, and retail's ring reads
    /// `0` on the first field frame after one (`doman_arrival_from_korb2`,
    /// `son_arrival_from_doman`).
    pub fn install_field_cost_table(&mut self, table: Option<FieldCostTable>) {
        self.field_cost_offset = 0;
        self.field_drawn_table_cost = None;
        if table.is_some() {
            self.clock.frame_step_telemetry =
                legaia_engine_vm::actor_tick::FrameStepTelemetry::new();
        }
        self.field_cost_table = table;
    }

    /// Price the field frame about to open, when the model is on
    /// ([`WorldToggles::field_frame_cost`](super::WorldToggles::field_frame_cost)):
    /// the frame cost [`FrameClock::open_costed_game_frame`] resolves the
    /// next step with. A frame with no published view keeps the last price.
    ///
    /// [`FrameClock::open_costed_game_frame`]: legaia_engine_vm::frame_clock::FrameClock::open_costed_game_frame
    ///
    /// Only a field or cutscene frame is priced. A minigame screen keeps its
    /// own step: a price the model left from the field it opened over is
    /// dropped there, and the step holds where it is.
    pub(in crate::world) fn price_field_frame(&mut self) {
        if !self.toggles.field_frame_cost {
            return;
        }
        if !self.field_frame_priced_mode() {
            return;
        }
        if let Some(f) = self.field_frame_features() {
            let cost = self.field_price(&f) + self.field_cost_offset;
            // Retail's entry is `max(cpu, gpu of the previous table)`.
            let cost = match self.field_drawn_table_cost {
                Some(gpu) if self.toggles.field_gpu_cost_step => cost.max(i32::from(gpu)),
                _ => cost,
            };
            let cost = self.field_xa_polled_cost(cost);
            self.clock.frame_cost = Some(cost.clamp(0, 0x7FFF));
            self.field_cost_priced = true;
        }
    }

    /// A field frame's cost while a CD-XA one-shot reads, under
    /// [`WorldToggles::field_frame_span_law`](super::WorldToggles::field_frame_span_law):
    /// the clip's stop watch `FUN_8003D764` polls the drive from the CD
    /// interrupt for the whole read, so the frame's work runs in what the
    /// polls leave of the CPU. PCSX-Redux's polls take about seven tenths
    /// ([`field_xa_loaded_frame_cost`], the battle's floor), a console's
    /// `.065` (`console_poll_cost`). `garmel` `P2[12]`'s voice line on
    /// PCSX-Redux: frames of `6` to `7` vsyncs at step `4` over a work of
    /// about `440` hblanks, from the clip's start to the drive word's clear.
    ///
    /// REF: FUN_8003D53C, FUN_8003D764
    pub(in crate::world) fn field_xa_polled_cost(&self, cost: i32) -> i32 {
        use legaia_engine_battle_loop::frame_clock::CdTiming;
        if !self.toggles.field_frame_span_law || !self.field_xa_reading() {
            return cost;
        }
        let work = cost.clamp(0, i32::from(u16::MAX)) as u16;
        i32::from(match self.toggles.field_cd_timing {
            CdTiming::PcsxRedux => field_xa_loaded_frame_cost(work),
            CdTiming::Console | CdTiming::Mednafen => {
                legaia_engine_cast::cast_module_frame_step::console_poll_cost(work)
            }
        })
    }

    /// Whether the draw walk's near reject drops field NPC model `slot`
    /// (spawned at `spawn`) this frame: its live anchor
    /// ([`Self::field_npc_live_anchor`]) at a view depth under `0xA1` through
    /// the camera the cull view publishes. A placement model is a posed
    /// actor (draw kind `1`), whose arm skips the whole actor there
    /// (`0x8001AE94..0x8001AEB4`) unless its `+0x52 & 0x20` exempts it; no
    /// placement model on the disc's captured states carries that bit. The
    /// frame-cost model drops the same models (`npc_models`). `false` with
    /// no published camera or while the model is not drawn.
    ///
    /// REF: FUN_8001ADA4
    pub fn field_npc_near_rejected(&self, slot: u8, spawn: (i16, i16)) -> bool {
        let (Some(cam), Some([x, y, z])) = (
            self.npcs.cull_camera.as_ref(),
            self.field_npc_live_anchor(slot, spawn),
        ) else {
            return false;
        };
        let depth = placed_cull_gte(cam)
            .rtps([x as i16, y as i16, z as i16])
            .mac[2];
        walker_near_rejected(depth, true, false)
    }

    /// Whether a raised actor's anchor `at` (world units) stands inside the
    /// slab of the object-effect row its `+0x42 = kind` names, at render
    /// scale `scale` (`+0x72`, `1.0 = 0x1000`, `0` read as `1.0`).
    ///
    /// The renderer behind the clip arm keeps a vertex whose effect-space `Y`
    /// lies inside the row's bounds and drops one past them
    /// (`FUN_80027F00`, `0x80027F60..0x80028098`); a primitive left with no
    /// corner links nothing, and one inside is projected and linked
    /// (`0x80027C6C..0x80027EFC`). A model's corners sit close about its
    /// anchor along the row's axis, so the anchor decides the model: on
    /// `rugi` `P2[72]` each of the three flyers links nothing while its
    /// anchor is outside the slab and most of its primitives from the frame
    /// its anchor crosses in. A row the port does not track keeps.
    // REF: FUN_8001C204, FUN_80027F00
    pub fn effect_clip_keeps_anchor(&self, kind: u16, scale: u16, at: [f32; 3]) -> bool {
        let Some(clip) = self
            .object_effect
            .clip_for(kind, &self.sin_lut, &self.cos_lut)
        else {
            return true;
        };
        let s = match scale {
            0 => 1.0,
            s => f32::from(s) / 4096.0,
        };
        let y = s * (0..3).map(|i| clip.n[i] * at[i]).sum::<f32>();
        y >= clip.lo && y <= clip.hi
    }

    /// The model's price of a frame that draws `f`: the fitted weights, or
    /// the derived per-unit table under
    /// [`WorldToggles::derived_frame_cost`](super::WorldToggles::derived_frame_cost).
    pub fn field_price(&self, f: &FieldFrameFeatures) -> i32 {
        if self.toggles.derived_frame_cost {
            super::field_frame_units::derived_field_frame_cost(f)
        } else {
            field_frame_cost(f)
        }
    }

    /// Whether the field model prices this mode's frames: a field or a
    /// cutscene.
    pub fn field_frame_priced_mode(&self) -> bool {
        matches!(
            self.mode,
            legaia_engine_mode::mode::SceneMode::Field
                | legaia_engine_mode::mode::SceneMode::Cutscene
        )
    }

    /// Drop the model's price once the world has left the modes it prices
    /// (a minigame screen, a battle): every vsync, ahead of the cadence test,
    /// so no other mode's wait runs on a field price. The ring goes with it,
    /// as `FUN_80016230` zeroes it on the mode change, so a field the world
    /// comes back to measures its step afresh.
    pub(in crate::world) fn release_field_frame_cost(&mut self) {
        if self.field_cost_priced && !self.field_frame_priced_mode() {
            self.field_cost_priced = false;
            self.clock.clear_frame_cost();
            self.clock.frame_step_telemetry =
                legaia_engine_vm::actor_tick::FrameStepTelemetry::new();
        }
    }

    /// Anchor the model on a capture's measured frame time: from here on a
    /// field frame costs what the model prices it at plus what the capture's
    /// newest frame cost over the model's price of the frame it stands on
    /// now. A harness seat for a walk film standing on a retail state, whose
    /// ring says what that scene costs where the model is off; the model then
    /// carries only the change. A scene entry drops it. `None` without a
    /// published view.
    pub fn anchor_field_frame_cost(&mut self, measured: i32) -> Option<i32> {
        let f = self.field_frame_features()?;
        self.field_cost_offset = measured - self.field_price(&f);
        Some(self.field_cost_offset)
    }

    /// What the draw's morph substitution runs for `owner`'s mesh `mesh`
    /// while its `+0x10 & 0x1000` gate is up (`FUN_8001ADA4` `0x8001B424..`),
    /// whatever the lane weights: per TMD object, one `FUN_8001C604` call
    /// copying the object's vertices; per record of every armed lane's VDF
    /// entry, `FUN_8005B038` over its deltas. `[calls, copied vertices,
    /// morphed vertices]`.
    // REF: FUN_8001C604, FUN_8005B038
    fn field_morph_work(&self, owner: MorphOwner, mesh: &MeshCost) -> [u32; 3] {
        let Some(st) = self.npcs.morphs.get(&owner) else {
            return [0; 3];
        };
        if st.flags & legaia_engine_vm::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE == 0 {
            return [0; 3];
        }
        // The lanes and records walked in place: this runs every frame for
        // each gated actor, so it allocates nothing.
        let lanes =
            usize::from(st.keyframe_count).min(legaia_engine_vm::vdf_morph::ACTOR_MORPH_LANES);
        let word = |b: &[u8], o: usize| {
            b.get(o..o + 4)
                .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) as usize)
        };
        let mut verts = 0u32;
        for lane in 0..lanes {
            let Some(entry) = self.vdf_record_bytes(st.anim_block_u8(0x04 + lane)) else {
                continue;
            };
            let mut off = 4;
            for _ in 0..word(entry, 0).unwrap_or(0) {
                let (Some(group), Some(n)) = (word(entry, off), word(entry, off + 8)) else {
                    break;
                };
                off += 12 + n * 8;
                if off > entry.len() {
                    break;
                }
                if group < usize::from(mesh.objects) {
                    verts += n as u32;
                }
            }
        }
        [u32::from(mesh.objects), u32::from(mesh.verts), verts]
    }

    /// What the field frame about to run draws, counted through the retail
    /// gates against the cull view the follow camera last published. `None`
    /// without a cost table or a published view.
    pub fn field_frame_features(&self) -> Option<FieldFrameFeatures> {
        let table = self.field_cost_table.as_ref()?;
        let view = self.npcs.cull_view.as_ref()?;
        let cells: ViewCells = view_cells(view);
        let mut f = FieldFrameFeatures::default();
        let outcome_gte = (self.toggles.derived_frame_cost
            && self.toggles.field_cost_prim_outcomes)
            .then(|| self.npcs.cull_camera.as_ref().map(placed_cull_gte))
            .flatten();
        let mut scratch = std::mem::take(&mut *self.field_cost_scratch.borrow_mut());
        for d in &table.decorations {
            if cells.decoration_visible(i32::from(d.col), i32::from(d.row), d.radius) {
                if let (Some(g), Some(geom)) = (
                    outcome_gte.as_ref().filter(|_| d.view_skip == 0),
                    table.decoration_geoms.get(d.geom as usize),
                ) {
                    let t = super::field_deco_outcomes::DecoTransform::new(g, d.pos, d.angles);
                    let (c, k) = super::field_deco_outcomes::price_decoration(
                        geom,
                        &t,
                        d.cued,
                        d.offtest,
                        &mut scratch,
                    );
                    f.decoration_outcome_cycles_x10 += c as u64;
                    for (a, b) in f.decoration_outcome_kinds.iter_mut().zip(k) {
                        *a += b;
                    }
                }
                f.decoration_draws += 1;
                f.decoration_prims += u32::from(d.prims);
                f.decoration_verts += u32::from(d.verts);
                f.decoration_quads += u32::from(d.quads);
                f.decoration_lit += u32::from(d.lit);
                f.decoration_groups += u32::from(d.mesh.groups);
                for (k, n) in f.decoration_kinds.iter_mut().zip(d.mesh.kinds) {
                    *k += u32::from(n);
                }
            }
        }
        {
            // The loop's span against the walk box it tests each cell on
            // (`0x801F74DC..0x801F7540`): col `first - 1 + i`, row
            // `first - 3 + j`, inside when `r0 <= col < r2`, `r1 - 1 <= row < r3`.
            let [r0, r1, r2, r3] = cells.region.map(i32::from);
            let c0 = cells.first[0] - 1;
            let w0 = cells.first[1] - 3;
            let cols = (c0.max(r0)..(c0 + cells.span[0].max(0)).min(r2)).len() as u32;
            let rows = (w0.max(r1 - 1)..(w0 + cells.span[1].max(0)).min(r3)).len() as u32;
            f.decoration_box_cells = cols * rows;
            f.decoration_rows = cells.span[1].max(0) as u32;
        }
        if !table.ground.is_empty() {
            let cols = (i32::from(cells.window[2]) - i32::from(cells.window[0])).max(1);
            let rows = (i32::from(cells.window[3]) - i32::from(cells.window[1])).max(1);
            let c0 = cells.first[0];
            let r0 = (cells.first[1] - 1) & 0x7F;
            f.ground_rows = rows as u32;
            f.ground_window_cells = (cols * rows) as u32;
            // The emitter's near test (`0x801F6F60..0x801F6F6C`): after the
            // `RTPT` of the cell's corners, a third corner (`(col, row + 1)`)
            // nearer than `SZ 0x40` drops the cell before its quad; the
            // depth-sorted bit then picks the far corner's sort
            // (`0x801F6F94..`).
            let gte = self.npcs.cull_camera.as_ref().map(placed_cull_gte);
            let lut = &self.terrain.floor_height_lut;
            for j in 0..rows {
                for i in 0..cols {
                    let (col, row) = (c0 + i, (r0 + j) & 0x7F);
                    if !table.ground_at(col, row) {
                        continue;
                    }
                    f.ground_cells += 1;
                    let Some(g) = gte.as_ref() else {
                        continue;
                    };
                    let row_z = r0 + j;
                    let y = table.ground_height(col, row + 1, lut);
                    let corner = [
                        (col * 128).clamp(-0x8000, 0x7FFF) as i16,
                        y,
                        ((row_z + 1) * 128).clamp(-0x8000, 0x7FFF) as i16,
                    ];
                    if g.rtps(corner).sz < 0x40 {
                        f.ground_far_cells += 1;
                    } else if table.ground_sorted_at(col, row) {
                        f.ground_sorted_cells += 1;
                    }
                }
            }
        }
        let gte = self.npcs.cull_camera.as_ref().map(placed_cull_gte);
        let sw = &self.terrain.static_window;
        for p in &table.placed {
            if self.field_actor_culled_at(p.x, p.z, i16::from(p.radius)) {
                continue;
            }
            // Which of retail's reasons, if any, keeps the draw walk off
            // this placement's primitives: no actor (a bound one parked or
            // retired, a window-owned one the list the last re-centre
            // planned does not hold, `FUN_801D7B50`), or the near reject.
            // A parked actor stands at the hide box (`0x3FC0, 0x3FC0`), where
            // the tile cull drops it; a retired one is off the list.
            let is_parked = p.object.is_some_and(|k| self.placed_object_parked(k));
            let unlisted = is_parked
                || p.window
                    .as_ref()
                    .is_some_and(|k| sw.window.is_some() && !sw.draws(k));
            if unlisted {
                f.placed_unlisted_draws += 1;
                f.placed_unlisted_prims += u32::from(p.prims);
                f.placed_parked_draws += u32::from(is_parked);
            } else if gte.as_ref().is_some_and(|g| placed_near_rejected(p, g)) {
                f.placed_near_draws += 1;
                f.placed_near_prims += u32::from(p.prims);
            } else {
                f.add_placed(p);
                if let Some(r) = p.bind {
                    f.add_morph(self.field_morph_work(MorphOwner::Object(r), &p.mesh));
                }
                continue;
            }
            if !self.toggles.field_cost_placed_walk {
                f.add_placed(p);
            }
        }
        f.decoration_loop_cells = (cells.span[0].max(0) * cells.span[1].max(0)) as u32;
        for (&slot, &(x, z)) in &self.npcs.positions {
            if !self.field_npc_culled(x, z) {
                f.npcs += 1;
                f.npc_prims +=
                    u32::from(table.npc_prims.get(usize::from(slot)).copied().unwrap_or(0));
            }
        }
        // The derived counts take every placement the hosts draw a mesh for
        // (a placement with no talk text, such as `teien`'s Genesis tree
        // `P1[1]`, holds no `npcs.positions` anchor and stands at its spawn),
        // where the hosts draw it (`field_npc_live_anchor`: none while
        // parked or at zero scale).
        for (i, m) in table.npc_meshes.iter().enumerate() {
            let (Ok(slot), Some(&spawn)) = (u8::try_from(i), table.npc_spawns.get(i)) else {
                continue;
            };
            if m.objects == 0 {
                continue;
            }
            let Some([x, y, z]) = self.field_npc_live_anchor(slot, spawn) else {
                continue;
            };
            if self.field_npc_culled(x as i16, z as i16) {
                continue;
            }
            // The dispatcher's near reject on the actor's view depth
            // (`slti v0,v0,0xa1` at `0x8001AE9C`), the one both play hosts
            // ask (`World::field_npc_near_rejected`).
            let at = [x as i16, y as i16, z as i16];
            if gte
                .as_ref()
                .is_some_and(|g| walker_near_rejected(g.rtps(at).mac[2], true, false))
            {
                continue;
            }
            // A model whose `+0x10 & 4` is up draws only when its box
            // reaches the screen (`0x8001AEBC..0x8001AED8`).
            let probed = self.field_vm.channels.iter().any(|c| {
                !c.object_bind && c.placement_index == usize::from(slot) && c.ctx.flags & 4 != 0
            });
            if probed && gte.as_ref().is_some_and(|g| !actor_box_on_screen(g, at, 0)) {
                continue;
            }
            let cut;
            let m = match self.npcs.clip_bones.get(&slot) {
                Some(&bones) => {
                    cut = m.cut_to(
                        bones,
                        table.npc_prims_through.get(i).map_or(&[][..], |v| &v[..]),
                    );
                    &cut
                }
                None => m,
            };
            f.npc_models += 1;
            f.add_morph(self.field_morph_work(MorphOwner::Placement(slot), m));
            // A raised `+0x42` (`lhu s2,0x42(s0)` at `0x8001B9E0`, kept as
            // `s7` at `0x8001BB00`) takes every object of the model through
            // the clip arm: `FUN_8001C204` (`0x8001BC3C`), then the TMD
            // renderer `FUN_8002735C` over the object's primitives
            // (`0x8001BC64` -> `0x8001BD88`), none of them through the
            // dispatcher `FUN_80043390`.
            let clipped = self.field_vm.channels.iter().find(|c| {
                !c.object_bind && c.placement_index == usize::from(slot) && c.ctx.field_42 != 0
            });
            if let Some(c) = clipped {
                let prims = m.kinds.iter().map(|&k| u32::from(k)).sum::<u32>();
                f.npc_clip_objects += u32::from(m.objects);
                f.npc_clip_prims += prims;
                if self.effect_clip_keeps_anchor(c.ctx.field_42, c.ctx.field_72, [x, y, z]) {
                    f.npc_clip_kept_prims += prims;
                }
                continue;
            }
            f.npc_objects += u32::from(m.objects);
            f.npc_groups += u32::from(m.groups);
            // The actor's colour word `+0x74` (the channel's `field_74`): a
            // non-zero mode byte sends its primitives through the depth-cued
            // leaves (`koin1b`'s `0x10808080` models).
            let cued = self.field_vm.channels.iter().any(|c| {
                !c.object_bind
                    && c.placement_index == usize::from(slot)
                    && c.ctx.field_74 & 0xFF00_0000 != 0
            });
            let bank = if cued {
                &mut f.npc_cued_kinds
            } else {
                &mut f.npc_kinds
            };
            for (k, n) in bank.iter_mut().zip(m.kinds) {
                *k += u32::from(n);
            }
        }
        f.list_actors = self.actors.iter().filter(|a| a.active).count() as u32;
        // The tick passes' entries (`FUN_8002519C`), one per live script
        // actor: the placements and the bound objects.
        f.tick_actors = self
            .field_vm
            .channels
            .iter()
            .filter(|c| c.is_live())
            .count() as u32;
        f.player_moving = self
            .player_actor_slot
            .is_some_and(|s| self.locomotion.actor_moving.contains(&s));
        f.player = table.player;
        // The player's model takes the clip arm on its own `+0x42` too
        // (`CC F8 C2`, `World::field_vm.player_field_42`): the animated
        // renderer reads the word off whichever actor it draws.
        f.player_clipped = self.field_vm.player_field_42 != 0;
        f.player_clip_kept = f.player_clipped
            && self
                .player_actor_slot
                .and_then(|s| self.actors.get(usize::from(s)))
                .is_some_and(|a| {
                    let m = &a.move_state;
                    self.effect_clip_keeps_anchor(
                        self.field_vm.player_field_42,
                        m.field_72,
                        [m.world_x, m.world_y, m.world_z].map(f32::from),
                    )
                });
        f.fog = u32::from(self.fog.live);
        // The quads the last walk left; a closed gate runs no walk.
        f.fog_quads = if self.fog.gate {
            self.fog.quads.len() as u32
        } else {
            0
        };
        f.parts = self.ambient.fx.iter().filter(|p| !p.finished).count() as u32;
        // The move-VM parts the actor passes draw through the procedural
        // sprite emitter (draw kind `+0x56 == 4`).
        f.sprite_parts = self
            .ambient
            .fx
            .iter()
            .filter(|p| !p.finished && p.state.move_substate == 4)
            .count() as u32;
        for p in self.ambient.fx.iter().filter(|p| {
            !p.finished && legaia_engine_effects::effect_default_arm::is_default_arm(&p.state)
        }) {
            let (n, textured) =
                legaia_engine_effects::effect_default_arm::default_arm_packet_count(&p.state);
            f.default_arm_parts += 1;
            f.default_arm_packets += n;
            if textured {
                f.default_arm_textured_packets += n;
            }
        }
        // The CLUT-cell cyclers that upload on this frame's tick: the arm runs
        // `FUN_80019D50` on a part whose gate `+0x9C` already reads past `1`
        // (the tick has not run yet, so the gate is the last tick's).
        for p in &self.ambient.fx {
            if p.finished || p.state.move_submode != 3 || p.state.field_9c < 2 {
                continue;
            }
            f.cycler_calls += 1;
            match p.cycler.texels.as_ref() {
                Some(t) => {
                    f.cycler_texels += p.cycler.lit;
                    f.cycler_zero_texels += t.len() as u32 - p.cycler.lit;
                }
                None => {
                    let d = p.state.keyframe_desc;
                    f.cycler_zero_texels += u32::from(d[2]) * u32::from(d[3]);
                }
            }
        }
        *self.field_cost_scratch.borrow_mut() = scratch;
        Some(f)
    }
}

/// What a field frame the clip's polls load costs on PCSX-Redux, in
/// thousandths of its work: the frame's CPU time over its time outside the CD
/// interrupt. Two `autorun_field_cd_trace.lua` recordings (breakpoint core)
/// price each frame of a voice line from the frame end's `VSync(n)` return
/// (`0x800171C8`) to the CPU-done `VSync(1)` (`0x800174CC`), every CD
/// interrupt from the exception vector to the response dispatcher's exit
/// (`0x8005DB74`): `garmel` `P2[12]` (Zeto's appearance, from
/// `chapter2_garmel_pre_zeto`) reads `3.34` over 85 frames and record 14
/// (his defeat, from `garmel_zeto_defeat_pre_line`) `3.36` over 62, against
/// the battle's `3.1` ([`legaia_engine_battle_loop::frame_clock::XA_POLL_COST_RATIO_MILLI`]).
pub const FIELD_XA_POLL_COST_RATIO_MILLI: u32 = 3340;

/// What a field frame of `work` hblanks costs while the clip's polls load it
/// on PCSX-Redux ([`FIELD_XA_POLL_COST_RATIO_MILLI`]), at least the battle's
/// floor ([`legaia_engine_battle_loop::frame_clock::XA_STREAM_FRAME_COST`]).
pub const fn field_xa_loaded_frame_cost(work: u16) -> u16 {
    let floor = legaia_engine_battle_loop::frame_clock::XA_STREAM_FRAME_COST;
    let c = (work as u32 * FIELD_XA_POLL_COST_RATIO_MILLI + 500) / 1000;
    if c > u16::MAX as u32 {
        u16::MAX
    } else if c < floor as u32 {
        floor
    } else {
        c as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_field::camera::FieldCullView;

    #[test]
    fn the_gates_count_what_the_view_reaches() {
        let mut w = World::new();
        let mut ground = vec![0u128; 128];
        ground[92] = 1 << 32;
        w.install_field_cost_table(Some(FieldCostTable {
            decorations: vec![
                CostCell {
                    col: 32,
                    row: 92,
                    radius: 0,
                    prims: 10,
                    verts: 12,
                    quads: 0,
                    lit: 0,
                    mesh: MeshCost::default(),
                    ..CostCell::default()
                },
                CostCell {
                    col: 100,
                    row: 10,
                    radius: 0,
                    prims: 99,
                    verts: 99,
                    quads: 0,
                    lit: 0,
                    mesh: MeshCost::default(),
                    ..CostCell::default()
                },
            ],
            placed: vec![CostPlaced {
                x: 4160,
                y: 0,
                z: 11840,
                radius: 0,
                posed: false,
                near_exempt: false,
                window: None,
                object: None,
                bind: None,
                prims: 7,
                verts: 9,
                quads: 0,
                lit: 0,
                mesh: MeshCost::default(),
            }],
            ground,
            ground_tiers: Vec::new(),
            ground_sorted: Vec::new(),
            npc_prims: Vec::new(),
            npc_meshes: Vec::new(),
            npc_spawns: Vec::new(),
            npc_prims_through: Vec::new(),
            player: None,
            decoration_geoms: Vec::new(),
        }));
        assert_eq!(w.field_frame_features(), None, "no view published");
        w.npcs.cull_view = Some(FieldCullView {
            focus_stored: [-4160, -11840],
            attr_box: [20, 77, 44, 108],
            window: [-8, -6, 8, 12],
        });
        let f = w.field_frame_features().unwrap();
        assert_eq!((f.decoration_draws, f.decoration_prims), (1, 10));
        assert_eq!((f.placed_draws, f.placed_prims), (1, 7));
        assert_eq!(f.ground_cells, 1);
    }

    fn town01_view() -> FieldCullView {
        FieldCullView {
            focus_stored: [-4160, -11840],
            attr_box: [20, 77, 44, 108],
            window: [-8, -6, 8, 12],
        }
    }

    /// The steps of the first `frames` game frames of a field world with
    /// `table` installed and the model `on`.
    fn steps(table: FieldCostTable, on: bool, frames: usize) -> Vec<u8> {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        w.toggles.field_frame_cost = on;
        w.set_frame_step_floor(2);
        w.install_field_cost_table(Some(table));
        w.npcs.cull_view = Some(town01_view());
        let mut out = Vec::new();
        for _ in 0..frames * 6 {
            let _ = w.tick();
            w.npcs.cull_view = Some(town01_view());
            if w.clock.game_tick_fired {
                out.push(w.clock.frame_step);
            }
            if out.len() == frames {
                break;
            }
        }
        out
    }

    /// A cheap view holds the field floor; one priced past `0x1FE` runs at
    /// `3` once its cost is the ring's longest, and the model off holds the
    /// floor whatever the view.
    /// The camera globals of a retail `vahn_walks_out` capture (Mednafen):
    /// angles `(186, -170, 0)`, eye `(-2585, 3994, -380)`, stored focus
    /// `(-3940, 0, -2014)`, `H = 0x200`.
    fn vahn_camera() -> legaia_engine_camera::retail_cam::RetailCamGlobals {
        legaia_engine_camera::retail_cam::RetailCamGlobals([
            186, -170, 0, -2585, 3994, -380, -3940, 0, -2014, 0x200,
        ])
    }

    fn placed(x: i16, y: i16, z: i16, prims: u16) -> CostPlaced {
        CostPlaced {
            x,
            y,
            z,
            radius: 0,
            posed: false,
            near_exempt: false,
            window: None,
            object: None,
            bind: None,
            prims,
            verts: 0,
            quads: 0,
            lit: 0,
            mesh: MeshCost::default(),
        }
    }

    #[test]
    fn the_near_reject_reads_the_walks_view_depth() {
        let g = placed_cull_gte(&vahn_camera());
        // The capture's `+0x2C..+0x34` for three placed actors.
        assert_eq!(g.rtps([1408, 0, 960]).mac, [-15632, 6813, -9999]);
        assert_eq!(g.rtps([2880, -192, 2368]).mac, [-9278, 2772, -310]);
        assert_eq!(g.rtps([4864, -192, 3208]).mac, [923, 539, 7305]);
        // The cliff section behind the eye is dropped; Vahn's house is not.
        assert!(placed_near_rejected(&placed(1408, 0, 960, 91), &g));
        assert!(!placed_near_rejected(&placed(4864, -192, 3208, 177), &g));
        // `+0x52 & 0x20` exempts it.
        let exempt = CostPlaced {
            near_exempt: true,
            ..placed(1408, 0, 960, 91)
        };
        assert!(!placed_near_rejected(&exempt, &g));
    }

    #[test]
    fn the_walk_count_drops_what_retail_never_draws() {
        let mut w = World::new();
        let listed = legaia_engine_scene::field_env::PlacedWindowKey {
            anchor: (36, 37),
            world_x: 4672,
            world_z: 4800,
        };
        let unlisted = legaia_engine_scene::field_env::PlacedWindowKey {
            anchor: (30, 44),
            world_x: 3136,
            world_z: 5696,
        };
        w.install_field_cost_table(Some(FieldCostTable {
            placed: vec![
                placed(4864, -192, 3208, 177),
                placed(1408, 0, 960, 91),
                CostPlaced {
                    window: Some(listed),
                    ..placed(4672, -128, 4800, 101)
                },
                CostPlaced {
                    window: Some(unlisted),
                    ..placed(3136, -128, 5696, 21)
                },
            ],
            ..FieldCostTable::default()
        }));
        w.npcs.cull_view = Some(FieldCullView {
            focus_stored: [-3940, -2014],
            attr_box: [0, 0, 0x7F, 0x7F],
            window: [-40, -40, 40, 40],
        });
        w.npcs.cull_camera = Some(vahn_camera());
        w.terrain.static_window.window = Some([9, 0, 45, 60]);
        w.terrain.static_window.set_drawn_for_test([listed]);
        w.toggles.field_cost_placed_walk = false;
        let f = w.field_frame_features().unwrap();
        assert_eq!((f.placed_draws, f.placed_prims), (4, 390));
        assert_eq!((f.placed_near_draws, f.placed_near_prims), (1, 91));
        assert_eq!((f.placed_unlisted_draws, f.placed_unlisted_prims), (1, 21));
        w.toggles.field_cost_placed_walk = true;
        let f = w.field_frame_features().unwrap();
        assert_eq!((f.placed_draws, f.placed_prims), (2, 278));
    }

    #[test]
    fn the_scene_cost_sets_the_step_over_the_floor() {
        let cheap = FieldCostTable::default();
        assert!(steps(cheap, true, 24).iter().all(|&x| x == 2));
        // Several thousand quads of decoration in view price a frame past 0x1FE.
        let heavy = FieldCostTable {
            decorations: (0..12)
                .map(|i| CostCell {
                    col: 30 + i,
                    row: 92,
                    radius: 0,
                    prims: 500,
                    verts: 500,
                    quads: 500,
                    lit: 0,
                    mesh: MeshCost {
                        objects: 1,
                        groups: 20,
                        kinds: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 500],
                        ..MeshCost::default()
                    },
                    ..CostCell::default()
                })
                .collect(),
            ..FieldCostTable::default()
        };
        let s = steps(heavy.clone(), true, 24);
        assert_eq!(s[0], 2, "the first frame runs on the step it opened with");
        assert!(s[1..].iter().all(|&x| x == 3), "{s:?}");
        assert!(steps(heavy, false, 24).iter().all(|&x| x == 2));
    }

    /// The opt-in field GPU step prices a frame at the larger of the model
    /// and the drawn table's GPU wait; down (the default) the drawn table
    /// changes nothing, and a scene entry forgets it.
    #[test]
    fn the_field_gpu_step_takes_the_larger_wait() {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        w.toggles.field_frame_cost = true;
        w.install_field_cost_table(Some(FieldCostTable::default()));
        w.npcs.cull_view = Some(town01_view());
        w.price_field_frame();
        let model = w.clock.frame_cost.unwrap();
        w.note_drawn_table_cost(600);
        w.price_field_frame();
        assert_eq!(w.clock.frame_cost, Some(model), "off by default");
        w.toggles.field_gpu_cost_step = true;
        w.price_field_frame();
        assert_eq!(w.clock.frame_cost, Some(600));
        w.note_drawn_table_cost(1);
        w.price_field_frame();
        assert_eq!(w.clock.frame_cost, Some(model));
        w.note_drawn_table_cost(600);
        w.install_field_cost_table(Some(FieldCostTable::default()));
        w.price_field_frame();
        assert_eq!(w.clock.frame_cost, Some(model));
    }

    /// The derived per-unit table prices the frame under its toggle (the
    /// default); down, the fitted weights do.
    #[test]
    fn the_toggle_picks_the_derived_or_fitted_model() {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        w.toggles.field_frame_cost = true;
        assert!(w.toggles.derived_frame_cost, "on by default");
        w.toggles.derived_frame_cost = false;
        w.install_field_cost_table(Some(FieldCostTable::default()));
        w.npcs.cull_view = Some(town01_view());
        w.price_field_frame();
        let f = w.field_frame_features().unwrap();
        assert_eq!(w.clock.frame_cost, Some(field_frame_cost(&f)));
        w.toggles.derived_frame_cost = true;
        w.price_field_frame();
        assert_eq!(
            w.clock.frame_cost,
            Some(super::super::field_frame_units::derived_field_frame_cost(
                &f
            ))
        );
        assert_ne!(field_frame_cost(&f), w.clock.frame_cost.unwrap());
    }

    /// Leaving the field for a mode the model does not price drops its price
    /// and empties the ring, as the mode transition `FUN_80016230` zeroes it.
    #[test]
    fn a_mode_change_drops_the_price_and_empties_the_ring() {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        w.toggles.field_frame_cost = true;
        w.set_frame_step_floor(2);
        w.install_field_cost_table(Some(FieldCostTable::default()));
        w.npcs.cull_view = Some(town01_view());
        for _ in 0..8 {
            let _ = w.tick();
        }
        assert!(w.clock.frame_cost.is_some());
        w.mode = legaia_engine_mode::mode::SceneMode::Battle;
        w.release_field_frame_cost();
        assert_eq!(w.clock.frame_cost, None);
        assert_eq!(
            w.clock.frame_step_telemetry,
            legaia_engine_vm::actor_tick::FrameStepTelemetry::new()
        );
    }

    /// The model is not capped at the ring's clamp: `concend`'s view (a
    /// `2704`-cell ground and a `4900`-cell loop span, two NPCs) prices past
    /// the `0x2D0` rung, so the pacer steps it at `4` as retail does on every
    /// frame of its film, while `son`'s (retail `613`, step `3`) prices past
    /// `0x1FE` and `retona_field_card_boot`'s (retail `454`, step `2`) stays
    /// under it.
    #[test]
    fn concend_prices_past_the_spike_rung_and_son_runs_at_three() {
        let concend = FieldFrameFeatures {
            decoration_prims: 24,
            decoration_loop_cells: 4900,
            ground_cells: 2704,
            npcs: 2,
            ..FieldFrameFeatures::default()
        };
        assert!(field_frame_cost(&concend) > 0x2D0);
        let son = FieldFrameFeatures {
            decoration_prims: 2480,
            decoration_loop_cells: 2173,
            npcs: 6,
            parts: 1,
            ..FieldFrameFeatures::default()
        };
        assert!((0x1FF..0x2D0).contains(&field_frame_cost(&son)));
        let retona = FieldFrameFeatures {
            decoration_prims: 2649,
            decoration_loop_cells: 2244,
            ground_cells: 205,
            placed_prims: 43,
            npcs: 2,
            fog: 27,
            fog_quads: 45,
            ..FieldFrameFeatures::default()
        };
        assert!(field_frame_cost(&retona) <= 0x1FE);
        assert_eq!(
            field_frame_cost(&FieldFrameFeatures::default()),
            FIELD_FRAME_BASE_COST as i32
        );
        let mut t = legaia_engine_vm::actor_tick::FrameStepTelemetry::new();
        let step = t
            .resolve(field_frame_cost(&concend), true, 2)
            .vsyncs_per_tick();
        assert_eq!(step, 4);
    }
}

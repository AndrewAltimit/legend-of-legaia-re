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
use legaia_engine_scene::field_view_window::{ViewCells, view_cells};

/// One decoration draw of the scene's terrain layer: its `.MAP` cell, its
/// record's cull radius and its pack mesh's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

/// One placed object: its world position, cull radius, how the draw walk
/// treats it, and its mesh size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub prims: u16,
    pub verts: u16,
    /// Of [`Self::prims`]: the quads, and the light-source rows (group
    /// flags `0x10..=0x17`, the dispatcher's `NCCS` / `NCCT` handlers).
    pub quads: u16,
    pub lit: u16,
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
    /// Each placement slot's spawn mesh size in primitives, by slot (`0` for
    /// a slot with no resolvable mesh).
    pub npc_prims: Vec<u16>,
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
    depth < if p.posed { 0xA1 } else { 0xA0 }
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
    fn add_placed(&mut self, p: &CostPlaced) {
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
            let cost = field_frame_cost(&f) + self.field_cost_offset;
            // Retail's entry is `max(cpu, gpu of the previous table)`.
            let cost = match self.field_drawn_table_cost {
                Some(gpu) if self.toggles.field_gpu_cost_step => cost.max(i32::from(gpu)),
                _ => cost,
            };
            self.clock.frame_cost = Some(cost.clamp(0, 0x7FFF));
            self.field_cost_priced = true;
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
        self.field_cost_offset = measured - field_frame_cost(&f);
        Some(self.field_cost_offset)
    }

    /// What the field frame about to run draws, counted through the retail
    /// gates against the cull view the follow camera last published. `None`
    /// without a cost table or a published view.
    pub fn field_frame_features(&self) -> Option<FieldFrameFeatures> {
        let table = self.field_cost_table.as_ref()?;
        let view = self.npcs.cull_view.as_ref()?;
        let cells: ViewCells = view_cells(view);
        let mut f = FieldFrameFeatures::default();
        for d in &table.decorations {
            if cells.decoration_visible(i32::from(d.col), i32::from(d.row), d.radius) {
                f.decoration_draws += 1;
                f.decoration_prims += u32::from(d.prims);
                f.decoration_verts += u32::from(d.verts);
                f.decoration_quads += u32::from(d.quads);
                f.decoration_lit += u32::from(d.lit);
            }
        }
        if !table.ground.is_empty() {
            let cols = (i32::from(cells.window[2]) - i32::from(cells.window[0])).max(1);
            let rows = (i32::from(cells.window[3]) - i32::from(cells.window[1])).max(1);
            let c0 = cells.first[0];
            let r0 = (cells.first[1] - 1) & 0x7F;
            for j in 0..rows {
                for i in 0..cols {
                    if table.ground_at(c0 + i, (r0 + j) & 0x7F) {
                        f.ground_cells += 1;
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
        f.fog = u32::from(self.fog.live);
        // The quads the last walk left; a closed gate runs no walk.
        f.fog_quads = if self.fog.gate {
            self.fog.quads.len() as u32
        } else {
            0
        };
        f.parts = self.ambient.fx.iter().filter(|p| !p.finished).count() as u32;
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
        Some(f)
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
                },
                CostCell {
                    col: 100,
                    row: 10,
                    radius: 0,
                    prims: 99,
                    verts: 99,
                    quads: 0,
                    lit: 0,
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
                prims: 7,
                verts: 9,
                quads: 0,
                lit: 0,
            }],
            ground,
            npc_prims: Vec::new(),
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
            prims,
            verts: 0,
            quads: 0,
            lit: 0,
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

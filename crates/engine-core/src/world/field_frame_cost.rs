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

/// One placed object: its world position, cull radius and mesh size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostPlaced {
    pub x: i16,
    pub z: i16,
    pub radius: u8,
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
/// field state). The fit maximises how many measured frames land on
/// retail's rung of the step law, four hblanks clear of each rung, under the
/// rule that no frame the previous model stepped right steps wrong; it was
/// solved as a mixed-integer program over the films' frames and the states'
/// rings. The terms follow the frame profile's parts (`autorun_field_frame_profile.lua`):
/// the decoration pass's primitives and the cells its loop visits, the ground
/// emitter's cells, the actor passes' primitives and NPCs, the tick pass's
/// ambient parts, and the fog walk's records and the half-sheets it emits.
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
pub const FIELD_FRAME_BASE_COST: i64 = 123;
/// The most [`field_frame_cost`] prices a frame at: a bound on the integer,
/// far past the `0x2D0` rung, not a model term.
pub const FIELD_FRAME_COST_MAX: i64 = 0x7FFF;
/// Weights of [`field_frame_cost`], 1/1024 hblank a unit (fitted): a
/// decoration primitive, a cell of the decoration loop's span, a ground
/// cell, a placed-object primitive, a visible placement NPC, a live fog
/// record, an emitted fog half-sheet and a live ambient part.
pub const COST_PER_DECORATION_PRIM: i64 = 73;
pub const COST_PER_DECORATION_LOOP_CELL: i64 = 22;
pub const COST_PER_GROUND_CELL: i64 = 176;
pub const COST_PER_PLACED_PRIM: i64 = 87;
pub const COST_PER_NPC: i64 = 16_692;
pub const COST_PER_FOG_RECORD: i64 = 1_277;
pub const COST_PER_FOG_HALF_SHEET: i64 = 958;
pub const COST_PER_PART: i64 = 1_784;
/// What the CLUT-cell recolour `FUN_80019D50` costs, 1/1024 hblank a unit
/// (measured, not fitted): a call, a non-zero texel (`RGB -> HSV -> RGB`,
/// `FUN_8001A78C` / `FUN_8001A6C8`) and a zero texel, which the kernel
/// copies through (`0x80019DF4`). Cycle-stamped on PCSX-Redux across
/// `opstati`'s `320 x 34` and `320 x 2` strips and `jou`'s sixteen-texel
/// rows: `1185`, `74` and `5` hblanks a call.
pub const COST_PER_CYCLER_CALL: i64 = 1_311;
pub const COST_PER_CYCLER_TEXEL: i64 = 234;
pub const COST_PER_CYCLER_ZERO_TEXEL: i64 = 38;

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
            self.clock.frame_cost = Some(cost.clamp(0, 0x7FFF));
            self.field_cost_priced = true;
        }
    }

    fn field_frame_priced_mode(&self) -> bool {
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
        for p in &table.placed {
            if !self.field_actor_culled_at(p.x, p.z, i16::from(p.radius)) {
                f.placed_draws += 1;
                f.placed_prims += u32::from(p.prims);
                f.placed_verts += u32::from(p.verts);
                f.placed_quads += u32::from(p.quads);
                f.placed_lit += u32::from(p.lit);
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
                z: 11840,
                radius: 0,
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
                    prims: 700,
                    verts: 700,
                    quads: 700,
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
    /// frame of its film, while `ending_vignette_fullscreen`'s (retail `502`,
    /// step `2`) stays under `0x1FF`.
    #[test]
    fn concend_prices_past_the_spike_rung_and_the_vignette_stays_under_three() {
        let concend = FieldFrameFeatures {
            decoration_prims: 24,
            decoration_loop_cells: 4900,
            ground_cells: 2704,
            npcs: 2,
            ..FieldFrameFeatures::default()
        };
        assert!(field_frame_cost(&concend) > 0x2D0);
        let vignette = FieldFrameFeatures {
            decoration_prims: 1674,
            decoration_loop_cells: 2436,
            ground_cells: 400,
            placed_prims: 540,
            npcs: 6,
            ..FieldFrameFeatures::default()
        };
        assert!(field_frame_cost(&vignette) <= 0x1FE);
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

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
/// A fit of retail's measured frame times (PCSX-Redux walk films' `frame_cost`
/// column, and the sixteen-sample ring of every library field state) against
/// these counts on the same frames: a 30% quantile fit, so a frame near the
/// `0x1FE` rung is priced under it rather than over, with each film weighing
/// the same. Integer arithmetic, so every host prices a frame alike; weights
/// in 1/1024 hblank.
///
/// Capped at the ring's clamp `0x2BC`: the model never prices a frame past
/// the `0x2D0` rung, so it steps a scene at `3` at most.
pub fn field_frame_cost(f: &FieldFrameFeatures) -> i32 {
    let terms: [(u32, i64); 6] = [
        (f.decoration_prims, COST_PER_DECORATION_PRIM),
        (f.ground_cells, COST_PER_GROUND_CELL),
        (f.placed_prims, COST_PER_PLACED_PRIM),
        (f.npcs, COST_PER_NPC),
        (f.fog, COST_PER_FOG_RECORD),
        (f.parts, COST_PER_PART),
    ];
    let sum: i64 = terms.iter().map(|&(n, w)| i64::from(n) * w).sum();
    (FIELD_FRAME_BASE_COST + (sum >> 10)).clamp(0, FIELD_FRAME_COST_CAP) as i32
}

/// The hblanks a field frame costs with nothing in view.
pub const FIELD_FRAME_BASE_COST: i64 = 90;
/// The most [`field_frame_cost`] prices a frame at: the ring's clamp `0x2BC`.
pub const FIELD_FRAME_COST_CAP: i64 = 0x2BC;
/// Weights of [`field_frame_cost`], 1/1024 hblank a unit: a decoration
/// primitive, a ground cell, a placed-object primitive, a visible placement
/// NPC, a live fog record and a live ambient part.
pub const COST_PER_DECORATION_PRIM: i64 = 103;
pub const COST_PER_GROUND_CELL: i64 = 179;
pub const COST_PER_PLACED_PRIM: i64 = 93;
pub const COST_PER_NPC: i64 = 18_012;
pub const COST_PER_FOG_RECORD: i64 = 1_331;
pub const COST_PER_PART: i64 = 1_988;

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
    pub fog: u32,
    pub parts: u32,
}

impl World {
    /// Install the scene's static cost inputs (a scene entry's), or drop them.
    /// The pacer's ring starts the scene empty: retail's ring reads `0` on
    /// the first field frame after a load (`doman_arrival_from_korb2`,
    /// `son_arrival_from_doman`), so nothing the scene before cost carries
    /// over.
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
    /// so no other mode's wait runs on a field price.
    pub(in crate::world) fn release_field_frame_cost(&mut self) {
        if self.field_cost_priced && !self.field_frame_priced_mode() {
            self.field_cost_priced = false;
            self.clock.clear_frame_cost();
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
        f.npcs = self
            .npcs
            .positions
            .values()
            .filter(|&&(x, z)| !self.field_npc_culled(x, z))
            .count() as u32;
        f.fog = u32::from(self.fog.live);
        f.parts = self.ambient.fx.iter().filter(|p| !p.finished).count() as u32;
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

    #[test]
    fn the_model_never_prices_a_frame_past_the_clamp() {
        let f = FieldFrameFeatures {
            decoration_prims: 100_000,
            ..FieldFrameFeatures::default()
        };
        assert_eq!(field_frame_cost(&f), 0x2BC);
        assert_eq!(
            field_frame_cost(&FieldFrameFeatures::default()),
            FIELD_FRAME_BASE_COST as i32
        );
    }
}

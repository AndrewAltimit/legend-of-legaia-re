//! A battle frame's CPU work by unit, derived from retail's own code rather
//! than fitted to films: each constant is the PCSX-Redux price (two cycles
//! an instruction, one a data access, the GTE and GPU free) of one unit of
//! retail work, counted in `retail-exec` by resuming library battle states
//! and timing each part of their frames (`psx_retail_exec::battle_cost`).
//! Not wired into any cost path; the fitted census line
//! ([`crate::frame_clock::linked_frame_work`]) stays the model the engine
//! prices with. `docs/tooling/retail-frame-cost.md` ("Battle frames by
//! unit") has the method and the residual check, and
//! `docs/subsystems/battle-stage-camera.md` ("The census") sets these beside
//! the fitted terms.
//!
//! Every price is in CPU cycles on PCSX-Redux's clock, interrupts out: the
//! runner takes no interrupt, so a frame's measured ring entry also carries
//! [`VBLANK_IRQ_CYCLES`] for each vblank that lands inside its work.
//!
//! REF: FUN_80043390, FUN_80044798, FUN_80048A08, FUN_8004998C, FUN_80049348
//! REF: FUN_800480D8, FUN_8001ADA4, FUN_80028158, FUN_80021DF4, FUN_801D02C0
//! REF: FUN_8004DA00, FUN_801E0080, FUN_8002735C, FUN_8002B28C, FUN_8002519C

/// PCSX-Redux's CPU cycles in one scanline, as a fraction: `33868800 / 60
/// / 263` (`PCSX_CYCLES_PER_HBLANK` in `retail-exec`), the unit the pacer's
/// ring counts in. [`crate::frame_clock::CYCLES_PER_HBLANK`] (`2172`, the
/// video clock's `3413 * 7 / 11`) is `1.2%` longer.
pub const PCSX_CYCLES_PER_HBLANK_NUM: u64 = 33_868_800;
/// See [`PCSX_CYCLES_PER_HBLANK_NUM`].
pub const PCSX_CYCLES_PER_HBLANK_DEN: u64 = 60 * 263;

/// `cycles` in hblanks on PCSX-Redux's clock, rounded.
pub const fn hblanks(cycles: u64) -> u64 {
    (cycles * PCSX_CYCLES_PER_HBLANK_DEN + PCSX_CYCLES_PER_HBLANK_NUM / 2)
        / PCSX_CYCLES_PER_HBLANK_NUM
}

/// How a primitive leaves the dispatcher's leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimFate {
    /// A packet written and linked into the ordering table.
    Linked,
    /// Through the winding and near tests, dropped by the off-screen reject
    /// `FUN_80044798` (every corner past one screen edge).
    OffScreen,
    /// Dropped by the winding test or under the near floor, before the
    /// off-screen test.
    Culled,
}

/// One leaf of the per-primitive dispatcher `FUN_80043390`'s table: what a
/// primitive costs in its loop, from the loop head to the `bgtz t3` back
/// edge, by how it leaves (`0` where no library state reached that path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeafCost {
    pub entry: u32,
    /// Primitive kind (`flags >> 1`): even a triangle, odd a quad.
    pub kind: u8,
    /// The alpha bank the leaf serves (`0` opaque, `1..=3` depth-cued).
    pub bank: u8,
    pub linked: u32,
    pub off_screen: u32,
    pub culled: u32,
}

const fn leaf(
    entry: u32,
    kind: u8,
    bank: u8,
    linked: u32,
    off_screen: u32,
    culled: u32,
) -> LeafCost {
    LeafCost {
        entry,
        kind,
        bank,
        linked,
        off_screen,
        culled,
    }
}

/// The leaves the library's battle frames reach, mean cycles a primitive.
/// Each path is nearly fixed: a linked primitive's cost spreads over a few
/// cycles (a far corner's depth-cue arm), a culled one's over the winding
/// reject (cheap) and the near reject (up to twice that). Measured over
/// `36` battle states, `8` frames each, about `800000` primitives.
pub const LEAF_COSTS: &[LeafCost] = &[
    leaf(0x8004_48B0, 12, 1, 152, 0, 112),
    leaf(0x8004_4A3C, 13, 1, 169, 0, 93),
    leaf(0x8004_4FDC, 14, 1, 260, 226, 130),
    leaf(0x8004_5194, 15, 1, 301, 260, 92),
    leaf(0x8004_4C14, 16, 1, 263, 243, 80),
    leaf(0x8004_4DC8, 17, 1, 292, 221, 92),
    leaf(0x8004_53BC, 18, 1, 275, 254, 92),
    leaf(0x8004_57C4, 18, 2, 269, 251, 68),
    leaf(0x8004_5584, 19, 1, 236, 206, 92),
    leaf(0x8004_5988, 19, 2, 208, 0, 92),
];

/// The leaf at `entry`, if the table has it.
pub fn leaf_cost(entry: u32) -> Option<&'static LeafCost> {
    LEAF_COSTS.iter().find(|l| l.entry == entry)
}

/// A dispatcher call (one object): its register save, colour and depth-cue
/// set-up and its exit, over its groups.
pub const DISPATCH_CALL_CYCLES: u32 = 252;
/// A primitive group: the dispatcher's group head (`0x80043580..0x80043614`)
/// and the leaf's set-up and first loads, over its primitives.
pub const DISPATCH_GROUP_CYCLES: u32 = 226;

/// A body's object draw `FUN_80048A08`, its own work outside the dispatcher:
/// a fixed part a draw and the per-object matrix set-up
/// (`FUN_800461A4` / `FUN_8004629C` / `FUN_8004638C` and the GTE loads).
pub const BODY_DRAW_CYCLES: u32 = 1412;
/// See [`BODY_DRAW_CYCLES`].
pub const BODY_OBJECT_CYCLES: u32 = 679;
/// The same draw re-run by the ghost walk `FUN_80049348` for an after-image,
/// a drawn object of the ghost (its own work, outside the dispatcher).
pub const GHOST_OBJECT_CYCLES: u32 = 1159;
/// The body's keyframe decode `FUN_8004998C`, once a draw (ghosts included);
/// it spreads `2400..11300` with the playing stream's parts.
pub const KEYFRAME_DECODE_CYCLES: u32 = 6241;
/// The per-actor draw tick `FUN_800480D8`'s own work, a call.
pub const DRAW_TICK_CYCLES: u32 = 1703;
/// The ghost walk `FUN_80049348` with no ghost to draw, a call.
pub const GHOST_WALK_CYCLES: u32 = 304;
/// The actor draw `FUN_8001ADA4`'s own work, a call (every drawn actor: the
/// bodies, the stage's objects, an effect's meshes).
pub const ACTOR_DRAW_CYCLES: u32 = 2254;

/// A kind-4 node's procedural mesh build `FUN_80028158`: fixed, and a
/// segment of a shape-`0` ring (`count & 0xFF`). A shape-`3` build (thirty-two
/// segments in every frame read) costs [`MESH_BUILD_SHAPE3_CYCLES`]. The census
/// counts what such a node links and nothing of its build.
pub const MESH_BUILD_CYCLES: u32 = 6386;
/// See [`MESH_BUILD_CYCLES`].
pub const MESH_BUILD_SEGMENT_CYCLES: u32 = 59;
/// See [`MESH_BUILD_CYCLES`].
pub const MESH_BUILD_SHAPE3_CYCLES: u32 = 46_577;

/// A live move-VM part's node tick `FUN_80021DF4`, mean. It runs the part's
/// bytecode, so a frame's mean spreads `650..28000` with the op mix.
pub const PART_TICK_CYCLES: u32 = 1578;

/// The ground grid `FUN_801D02C0`: its walk of every cell (the culls) is
/// fixed, each visible cell's four `POLY_GT4` on top. About `90` hblanks on
/// every stage read, `113..124` visible cells.
pub const GRID_FIXED_CYCLES: u32 = 145_452;
/// See [`GRID_FIXED_CYCLES`].
pub const GRID_CELL_CYCLES: u32 = 389;
/// A ground-shadow packet (`FUN_8004DA00`; a drawn body's shadow is `24`).
pub const SHADOW_PACKET_CYCLES: u32 = 266;

/// The effect walker `FUN_801E0080` timed visit by visit inside its frames:
/// a call, a sweep of the first pass (all `32` master and `128` child slots),
/// a live child's step (its countdown, or one frame advance: `143` and
/// `164`), each advance of a retired child's overrun past its batch, and a
/// live child's second-pass draw (its `POLY_FT4` built, projected and
/// linked). The interpreter fit over random walks
/// ([`crate::frame_clock::EFFECT_WALK_SWEEP_CYCLES`] and the rest) reads the
/// same branches within `5%`.
pub const WALKER_CALL_CYCLES: u32 = 2820;
/// See [`WALKER_CALL_CYCLES`].
pub const WALKER_SWEEP_CYCLES: u32 = 4446;
/// See [`WALKER_CALL_CYCLES`].
pub const CHILD_STEP_CYCLES: u32 = 150;
/// See [`WALKER_CALL_CYCLES`].
pub const CHILD_OVERRUN_ADVANCE_CYCLES: u32 = 150;
/// See [`WALKER_CALL_CYCLES`].
pub const CHILD_DRAW_CYCLES: u32 = 665;

/// The frame's work outside every unit above (the frame driver, the actor
/// tick pass, the battle VMs, the HUD and the pacer up to its mark): fixed,
/// and a body draw. Spread `12` hblanks rms over the library's frames.
pub const FRAME_BASE_CYCLES: u32 = 39_760;
/// See [`FRAME_BASE_CYCLES`].
pub const FRAME_BASE_BODY_CYCLES: u32 = 4394;

/// What PCSX-Redux's vblank interrupt (the kernel's dispatch, libetc's
/// callbacks and the pad exchange) adds to the frame whose work it lands in:
/// `11.6` hblanks, measured on the field states' profiles
/// (`docs/tooling/retail-frame-cost.md`, "What the interrupts cost").
pub const VBLANK_IRQ_CYCLES: u32 = 24_900;

/// The `"ME"` keyframe reader `FUN_8002B28C` (an art's commit pass) on
/// PCSX-Redux: `2.27` cycles an instruction with its data accesses, where
/// [`crate::frame_clock::me_decode_work`] charges `2`.
pub const ME_DECODE_CYCLES_PER_PART_FRAME: u32 = 1162;

/// The floor-clipped walk (`FUN_8002735C` through `FUN_8001C204`) of a
/// sinking body's object, a primitive: `445` once most of the body is under
/// the floor, `740` as the sink opens, against `273` the same body's
/// primitive costs through the dispatcher.
pub const FLOOR_CLIP_PRIM_CYCLES: (u32, u32) = (445, 740);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hblanks_round_on_pcsx_redux_clock() {
        assert_eq!(hblanks(0), 0);
        assert_eq!(hblanks(2146), 1);
        assert_eq!(hblanks(u64::from(GRID_FIXED_CYCLES)), 68);
        assert_eq!(hblanks(u64::from(VBLANK_IRQ_CYCLES)), 12);
    }

    #[test]
    fn every_leaf_is_a_cued_kind_with_a_linked_price() {
        for l in LEAF_COSTS {
            assert!((12..=19).contains(&l.kind), "{:08X}", l.entry);
            assert!(l.linked > l.culled, "{:08X}", l.entry);
        }
        assert!(leaf_cost(0x8004_57C4).is_some());
        assert!(leaf_cost(0x8004_409C).is_none());
    }
}

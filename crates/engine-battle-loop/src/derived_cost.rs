//! A battle frame's CPU work by unit, derived from retail's own code rather
//! than fitted to films: each constant is the PCSX-Redux price (two cycles
//! an instruction, one a data access, the GTE and GPU free) of one unit of
//! retail work, counted in `retail-exec` by resuming library battle states
//! and timing each part of their frames (`psx_retail_exec::battle_cost`).
//! Priced under `WorldToggles::derived_frame_cost` (`derived_cost::frame_hblanks`, off by default); the fitted census line
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
//! REF: FUN_8004DA00, FUN_801E0080, FUN_8001D140, FUN_8002735C, FUN_8002B28C, FUN_8002519C

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
/// The actor render's list walk `FUN_8001ADA4`, its own work a call (five a
/// frame, one a list: each actor's view cull and set-up); a frame's mean
/// spreads with what the lists hold.
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
/// The battle template's tick `FUN_8004DA00` (the HUD's packets and the
/// per-frame sweeps), a packet it links. A body's ground shadow is not here:
/// it is a shape-`0` mesh build of `24` segments inside the body draw
/// ([`MESH_BUILD_CYCLES`]) and its packets through the dispatcher.
pub const TEMPLATE_TICK_PACKET_CYCLES: u32 = 266;
/// What the template tick costs a frame on the library's states (mean; it
/// links `20..211` packets), for a model that does not count its packets.
pub const TEMPLATE_TICK_CYCLES: u32 = 23_750;

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
    fn interrupts_land_once_a_vblank() {
        assert_eq!(with_vblank_irqs(1000), 1000);
        let one = VBLANK_CYCLES + 10;
        assert_eq!(with_vblank_irqs(one), one + u64::from(VBLANK_IRQ_CYCLES));
        // Done short of the vblank: no interrupt lands inside it.
        let near = VBLANK_CYCLES - 100;
        assert_eq!(with_vblank_irqs(near), near);
        // Past two vblanks once the first interrupt is in.
        let two = 2 * VBLANK_CYCLES - 10_000;
        assert_eq!(
            with_vblank_irqs(two),
            two + 2 * u64::from(VBLANK_IRQ_CYCLES)
        );
    }

    #[test]
    fn an_empty_frame_costs_its_base() {
        let u = DerivedUnits::default();
        let w = crate::frame_clock::EffectWalkCounts::default();
        let h = frame_hblanks(&u, 0, &w, 0);
        // The base, the list walks, the template tick and an idle walk.
        assert!((30..45).contains(&h), "{h}");
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

/// The dispatcher's primitives by topology, each class's mean over the leaves
/// the library's battle frames reach, weighted by how often each leaf ran:
/// for a count that knows a primitive's topology and fate but not its leaf
/// (the engine's census).
pub const TRI_LINKED_CYCLES: u32 = 270;
/// See [`TRI_LINKED_CYCLES`].
pub const TRI_OFF_SCREEN_CYCLES: u32 = 254;
/// See [`TRI_LINKED_CYCLES`].
pub const TRI_CULLED_CYCLES: u32 = 78;
/// See [`TRI_LINKED_CYCLES`].
pub const QUAD_LINKED_CYCLES: u32 = 230;
/// See [`TRI_LINKED_CYCLES`].
pub const QUAD_OFF_SCREEN_CYCLES: u32 = 211;
/// See [`TRI_LINKED_CYCLES`].
pub const QUAD_CULLED_CYCLES: u32 = 92;
/// A drawn object's primitive groups, in hundredths: the measured ratio of
/// dispatcher groups to dispatcher calls (`13900` to `7246`), for a count
/// that knows objects but not their groups.
pub const GROUPS_PER_OBJECT_CENTI: u32 = 192;
/// The actor render's five list walks (`FUN_8001D140` / `FUN_8001ADA4`) a
/// frame, their own work at [`ACTOR_DRAW_CYCLES`] each. The walks' own cost
/// spreads `3300..37800` cycles a frame and follows no count the census
/// keeps (not the live parts: on one strike fourteen live parts read `1.9`
/// hblanks a frame and three read `15`), so the mean stands in.
pub const ACTOR_LISTS: u32 = 5;
/// The segments of a body's ground-shadow disc (`FUN_80028158` mode `1`).
pub const SHADOW_SEGMENTS: u32 = 24;
/// The walker's per-branch prices this table does not time in-frame (a
/// master visit, a spawn record), the random-walk fit's
/// ([`crate::frame_clock::EFFECT_WALK_MASTER_CYCLES`],
/// [`crate::frame_clock::EFFECT_WALK_SPAWN_CYCLES`]).
pub const WALKER_MASTER_CYCLES: u32 = crate::frame_clock::EFFECT_WALK_MASTER_CYCLES;
/// See [`WALKER_MASTER_CYCLES`].
pub const WALKER_SPAWN_CYCLES: u32 = crate::frame_clock::EFFECT_WALK_SPAWN_CYCLES;
/// A PCSX-Redux vblank in cycles (`263` hblanks).
pub const VBLANK_CYCLES: u64 = 263 * PCSX_CYCLES_PER_HBLANK_NUM / PCSX_CYCLES_PER_HBLANK_DEN;

/// Dispatcher primitives by topology and fate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrimTally {
    pub tris_linked: u32,
    pub tris_off_screen: u32,
    pub tris_culled: u32,
    pub quads_linked: u32,
    pub quads_off_screen: u32,
    pub quads_culled: u32,
}

impl PrimTally {
    pub fn add(&mut self, o: &Self) {
        self.tris_linked += o.tris_linked;
        self.tris_off_screen += o.tris_off_screen;
        self.tris_culled += o.tris_culled;
        self.quads_linked += o.quads_linked;
        self.quads_off_screen += o.quads_off_screen;
        self.quads_culled += o.quads_culled;
    }
    pub fn linked(&self) -> u32 {
        self.tris_linked + self.quads_linked
    }
    pub fn cycles(&self) -> u64 {
        let c = |n: u32, per: u32| u64::from(n) * u64::from(per);
        c(self.tris_linked, TRI_LINKED_CYCLES)
            + c(self.tris_off_screen, TRI_OFF_SCREEN_CYCLES)
            + c(self.tris_culled, TRI_CULLED_CYCLES)
            + c(self.quads_linked, QUAD_LINKED_CYCLES)
            + c(self.quads_off_screen, QUAD_OFF_SCREEN_CYCLES)
            + c(self.quads_culled, QUAD_CULLED_CYCLES)
    }
}

/// What the engine's census counts of a battle frame for the derived
/// pricing: the dispatcher's primitives by fate, the objects they come in,
/// the bodies and ghosts drawn, the grid, the kind-4 builds, the floor-clipped
/// walk. The effect pool, the move-VM parts and the `"ME"` decodes are the
/// world's own counts and are priced beside it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DerivedUnits {
    pub prims: PrimTally,
    /// Objects through the dispatcher (bodies', ghosts', the stage's, the
    /// effects').
    pub objects: u32,
    pub bodies: u32,
    pub body_objects: u32,
    pub ghosts: u32,
    pub ghost_objects: u32,
    /// The ground grid's visible cells, `None` while it does not draw.
    pub grid_cells: Option<u32>,
    /// Kind-4 mesh builds (the shadows' among them) and their segments.
    pub builds: u32,
    pub build_segments: u32,
    /// Packets the swept bands link (weapon trails, streaks, ribbons).
    pub trail_packets: u32,
    /// Primitives of floor-clipped bodies, walked by `FUN_8002735C`.
    pub floor_clip_prims: u32,
}

/// The cycles of what [`DerivedUnits`] counts, the frame's base included
/// (its fixed part, its share a body, the actor-list walks and the template
/// tick), interrupts out.
pub fn units_cycles(u: &DerivedUnits) -> u64 {
    let c = |n: u32, per: u32| u64::from(n) * u64::from(per);
    let groups = u64::from(u.objects) * u64::from(GROUPS_PER_OBJECT_CENTI) / 100;
    let draws = u.bodies + u.ghosts;
    let mut s = u.prims.cycles()
        + groups * u64::from(DISPATCH_GROUP_CYCLES)
        + c(u.objects, DISPATCH_CALL_CYCLES)
        + c(
            u.bodies,
            BODY_DRAW_CYCLES + DRAW_TICK_CYCLES + GHOST_WALK_CYCLES + FRAME_BASE_BODY_CYCLES,
        )
        + c(u.body_objects, BODY_OBJECT_CYCLES)
        + c(u.ghost_objects, GHOST_OBJECT_CYCLES)
        + c(draws, KEYFRAME_DECODE_CYCLES)
        + c(u.builds, MESH_BUILD_CYCLES)
        + c(u.build_segments, MESH_BUILD_SEGMENT_CYCLES)
        + c(u.trail_packets, TRI_LINKED_CYCLES)
        + c(
            u.floor_clip_prims,
            (FLOOR_CLIP_PRIM_CYCLES.0 + FLOOR_CLIP_PRIM_CYCLES.1) / 2,
        )
        + u64::from(FRAME_BASE_CYCLES)
        + c(ACTOR_LISTS, ACTOR_DRAW_CYCLES)
        + u64::from(TEMPLATE_TICK_CYCLES);
    if let Some(cells) = u.grid_cells {
        s += u64::from(GRID_FIXED_CYCLES) + c(cells, GRID_CELL_CYCLES);
    }
    s
}

/// The effect walker's branches over a frame, priced in-frame.
pub fn walker_cycles(w: &crate::frame_clock::EffectWalkCounts) -> u64 {
    let c = |n: u32, per: u32| u64::from(n) * u64::from(per);
    u64::from(WALKER_CALL_CYCLES)
        + c(w.live_sweeps.max(1), WALKER_SWEEP_CYCLES)
        + c(w.masters, WALKER_MASTER_CYCLES)
        + c(w.spawns, WALKER_SPAWN_CYCLES)
        + c(w.child_waits + w.advances, CHILD_STEP_CYCLES)
        + c(w.overruns, CHILD_OVERRUN_ADVANCE_CYCLES)
        + c(w.drawn, CHILD_DRAW_CYCLES)
}

/// An art commit's `"ME"` decode of `parts * frames` part-frames.
pub const fn me_decode_cycles(parts: u32, frames: u32) -> u64 {
    parts as u64 * frames as u64 * ME_DECODE_CYCLES_PER_PART_FRAME as u64
}

/// `work` cycles with the vblank interrupts that land inside them: the
/// window opens on a `VSync` return, just past one, and each later vblank
/// the work (interrupts in) reaches adds [`VBLANK_IRQ_CYCLES`].
pub fn with_vblank_irqs(work: u64) -> u64 {
    let mut total = work;
    loop {
        let n = total / VBLANK_CYCLES;
        let next = work + n * u64::from(VBLANK_IRQ_CYCLES);
        if next == total {
            return total;
        }
        total = next;
    }
}

/// A battle frame's work in hblanks on PCSX-Redux's clock: the census's
/// units, the move-VM part ticks, the walker's branches, the `"ME"` decodes'
/// cycles and any other cycles the caller holds, with their interrupts.
pub fn frame_hblanks(
    units: &DerivedUnits,
    parts: u32,
    walker: &crate::frame_clock::EffectWalkCounts,
    extra_cycles: u64,
) -> u16 {
    let work = units_cycles(units)
        + u64::from(parts) * u64::from(PART_TICK_CYCLES)
        + walker_cycles(walker)
        + extra_cycles;
    hblanks(with_vblank_irqs(work)).min(u64::from(u16::MAX)) as u16
}

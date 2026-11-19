//! What one repetition of each unit of the field frame's render passes
//! costs on PCSX-Redux's clock (two cycles an instruction, one a data
//! access, GTE and GPU free), derived from instruction counts rather than
//! fitted: `psx-retail-exec`'s `field-units` runner resumes a library state,
//! times every loop iteration of the decoration pass `FUN_801F7088`, the
//! ground emitter `FUN_801F6D48`, the actor render pass `FUN_8001D140`, the
//! fog record walk `FUN_8003F3FC`, the prim dispatcher `FUN_80043390` and
//! each prim leaf, and names the path it took. The figures are the pooled
//! means over ten library field states; the units, their classes and the
//! engine count each needs are in `docs/tooling/retail-frame-cost.md`
//! (section "Per-unit costs"), and `crates/retail-exec/tests/field_units.rs`
//! holds them against the runner.
//!
//! [`derived_field_frame_cost`] prices a field frame from these figures,
//! under `WorldToggles::derived_frame_cost` (on by default; down, the fitted
//! `world::field_frame_cost` prices it).

/// `(unit, cycles x 10, repetitions the pool saw)`. Unit names are
/// `pass.loop[class]` (a loop iteration by the marks it passed),
/// `pass.call` (a call's own work outside its loops), `dispatch.draw` /
/// `dispatch.group decode` (the prim dispatcher), `leaf group k<kind> <bank>
/// <tri|quad>` (a leaf's set-up for one prim group) and `prim k<kind> <bank>
/// <tri|quad> <outcome>` (one primitive through a leaf: `rejected` - a back
/// face or a near / off-screen reject the leaf makes without the off-screen
/// test - `rejected+offtest`, `linked`, `linked+offtest`). Banks: `0`
/// opaque, `cued` the depth-cued handler set, `lit` the light-source rows.
pub const FIELD_UNIT_CYCLES_X10: &[(&str, u32, u32)] = &[
    ("actor.actor[-]", 610, 414),
    ("actor.actor[gate1+gate2+kind+model]", 1479, 43),
    ("actor.actor[gate1+gate2+kind+placed]", 12022, 58),
    ("actor.actor[gate1+gate2+kind]", 8986, 19),
    ("actor.actor[gate1]", 500, 55),
    ("actor.call", 1203, 50),
    ("actor.model[-]", 12967, 43),
    ("actor.morph lane[-]", 60, 34),
    ("actor.morph vertex[-]", 610, 2215),
    ("actor.object[-]", 9188, 287),
    ("actor.object[morph]", 12519, 56),
    ("actor.placed mesh[drawn]", 748, 87),
    ("actor.morph stage[-]", 2532, 69),
    ("actor.morph copy[-]", 220, 1939),
    ("decoration.call", 16220, 10),
    ("decoration.cell[-]", 543, 4420),
    ("decoration.cell[drawn+inbox+record]", 5031, 461),
    ("decoration.cell[inbox+record]", 1303, 125),
    ("decoration.cell[inbox]", 990, 14256),
    ("decoration.row[-]", 240, 436),
    ("dispatch.draw", 1995, 891),
    ("dispatch.group decode", 710, 2260),
    ("fog.call", 40, 86),
    ("fog.half-sheet[culled]", 1486, 57),
    ("fog.half-sheet[kept]", 2330, 115),
    ("fog.record[-]", 6350, 18),
    ("fog.record[near1+wind]", 6770, 10),
    ("fog.record[near1]", 6556, 57),
    ("fog.record[wind]", 6710, 1),
    ("ground.call", 1290, 10),
    ("ground.cell[-]", 350, 4052),
    ("ground.cell[near+sorted+walk]", 3143, 3676),
    ("ground.cell[near+walk]", 2820, 2305),
    ("ground.row[-]", 240, 294),
    ("leaf group k11 lit quad", 460, 2),
    ("leaf group k12 0 tri", 400, 184),
    ("leaf group k12 cued tri", 400, 8),
    ("leaf group k13 0 quad", 460, 184),
    ("leaf group k13 cued quad", 460, 13),
    ("leaf group k14 0 tri", 400, 275),
    ("leaf group k14 cued tri", 400, 10),
    ("leaf group k15 0 quad", 460, 289),
    ("leaf group k15 cued quad", 460, 23),
    ("leaf group k16 0 tri", 400, 59),
    ("leaf group k16 cued tri", 400, 58),
    ("leaf group k17 0 quad", 460, 260),
    ("leaf group k17 cued quad", 460, 136),
    ("leaf group k18 0 tri", 400, 122),
    ("leaf group k18 cued tri", 400, 184),
    ("leaf group k19 0 quad", 460, 148),
    ("leaf group k19 cued quad", 460, 303),
    ("leaf group k8 lit tri", 440, 1),
    ("leaf group k9 lit quad", 460, 1),
    ("prim k11 lit quad rejected", 743, 6),
    ("prim k11 lit quad rejected+offtest", 2090, 5),
    ("prim k12 0 tri linked", 1201, 316),
    ("prim k12 0 tri rejected", 734, 279),
    ("prim k12 cued tri linked", 1573, 15),
    ("prim k12 cued tri rejected", 743, 9),
    ("prim k13 0 quad linked", 1423, 140),
    ("prim k13 0 quad rejected", 978, 296),
    ("prim k13 cued quad linked", 1796, 56),
    ("prim k13 cued quad rejected", 975, 44),
    ("prim k14 0 tri linked", 1253, 2444),
    ("prim k14 0 tri rejected", 663, 1302),
    ("prim k14 cued tri linked+offtest", 2642, 124),
    ("prim k14 cued tri rejected", 672, 73),
    ("prim k15 0 quad linked", 1591, 1005),
    ("prim k15 0 quad rejected", 965, 1208),
    ("prim k15 cued quad linked+offtest", 3040, 55),
    ("prim k15 cued quad rejected", 962, 121),
    ("prim k15 cued quad rejected+offtest", 2839, 56),
    ("prim k16 0 tri linked", 1392, 551),
    ("prim k16 0 tri rejected", 727, 284),
    ("prim k16 cued tri linked+offtest", 2686, 107),
    ("prim k16 cued tri rejected", 724, 170),
    ("prim k16 cued tri rejected+offtest", 2390, 420),
    ("prim k17 0 quad linked", 1672, 417),
    ("prim k17 0 quad rejected", 969, 290),
    ("prim k17 cued quad linked+offtest", 3022, 79),
    ("prim k17 cued quad rejected", 966, 647),
    ("prim k17 cued quad rejected+offtest", 2165, 418),
    ("prim k18 0 tri linked", 1513, 1140),
    ("prim k18 0 tri rejected", 722, 968),
    ("prim k18 cued tri linked+offtest", 2812, 548),
    ("prim k18 cued tri rejected", 808, 1270),
    ("prim k18 cued tri rejected+offtest", 2475, 1672),
    ("prim k19 0 quad linked", 1836, 1041),
    ("prim k19 0 quad rejected", 967, 604),
    ("prim k19 cued quad linked", 2499, 27),
    ("prim k19 cued quad linked+offtest", 3306, 755),
    ("prim k19 cued quad rejected", 961, 1930),
    ("prim k19 cued quad rejected+offtest", 2096, 3907),
    ("prim k8 lit tri linked", 1674, 10),
    ("prim k8 lit tri rejected", 730, 10),
    ("prim k9 lit quad linked", 2040, 2),
];

/// The cycles one repetition of `unit` costs, if the table has it.
pub fn unit_cycles(unit: &str) -> Option<f64> {
    FIELD_UNIT_CYCLES_X10
        .iter()
        .find(|(u, _, _)| *u == unit)
        .map(|(_, c, _)| f64::from(*c) / 10.0)
}

/// The vblank interrupt on PCSX-Redux, kernel dispatch and libetc's
/// callbacks with the pad exchange: `24400..26200` cycles (`11.4..12.2`
/// hblanks) on every vblank of the profiled field states.
pub const VBLANK_INTERRUPT_CYCLES: u32 = 24_900;

/// The other interrupts (DMA completion, CD-ROM) a field vsync takes on
/// PCSX-Redux: `2081..4500` cycles each, `0.6..3.8` hblanks a vsync over
/// the profiled states (mean `2.3`).
pub const OTHER_INTERRUPT_CYCLES_PER_VSYNC: u32 = 5_000;

use super::field_frame_cost::{
    COST_PER_CYCLER_CALL, COST_PER_CYCLER_TEXEL, COST_PER_CYCLER_ZERO_TEXEL, FIELD_FRAME_COST_MAX,
    FieldFrameFeatures,
};

/// PCSX-Redux cycles a scanline, x 100 (`33868800 / 60 / 263`).
const CYCLES_PER_HBLANK_X100: i64 = 214_631;

/// What a primitive costs through its leaf, by kind (`flags >> 1`, index
/// `kind - 8`), pooled over the outcomes (back face, off screen, linked)
/// and banks the ten states' decoration draws took: the depth-cued leaves
/// with the off-screen test, mostly. Kinds the decoration never drew take
/// the actor figure.
pub const DECORATION_PRIM_CYCLES: [i64; 12] =
    [120, 204, 120, 141, 157, 137, 100, 166, 179, 152, 168, 188];
/// The same for the actor render passes' opaque leaves (an actor whose colour
/// word `+0x74` has a zero high byte: the models, and the placed meshes with
/// no cull radius), pooled over the fourteen profiled states.
pub const ACTOR_PRIM_CYCLES: [i64; 12] =
    [120, 204, 120, 141, 98, 112, 104, 125, 118, 130, 122, 152];
/// The actor passes' depth-cued leaves with the off-screen test, which a
/// placed mesh with a cull radius takes (`+0x1E != 0` ORs `+0x74` bit
/// `0x40000000`), pooled the same way. Kinds `12`/`13` never drew there and
/// take kinds `14`/`15`'s figures; the light-source kinds are bank-invariant
/// but for kind `11`'s test.
pub const ACTOR_OFFTEST_PRIM_CYCLES: [i64; 12] =
    [120, 184, 151, 213, 133, 145, 189, 209, 199, 188, 196, 199];

/// A primitive group through the dispatcher: its decode (`71`) and the
/// leaf's set-up (`40` a triangle group, `46` a quad one).
const GROUP_CYCLES: i64 = 71 + 43;
/// A drawn decoration cell's own path (`503`) and its dispatcher draw
/// (`200`).
const DECORATION_DRAW_CYCLES: i64 = 503 + 200;
/// A ground walk cell: the plain path (`282`), the depth-sorted one
/// (`314`), and one the near test drops after its corners' transform
/// (`221`).
const GROUND_WALK_CELL_CYCLES: i64 = 282;
const GROUND_SORTED_CELL_CYCLES: i64 = 314;
const GROUND_FAR_CELL_CYCLES: i64 = 221;
/// A model object off the morph stage, the ten states' `287` such objects'
/// mean (`919`), and its dispatcher draw (`200`).
const NPC_OBJECT_CYCLES: i64 = 919 + 200;
/// A drawn model actor: its list entry's path (`148`) and the model set-up
/// (`1297`).
const NPC_CYCLES: i64 = 148 + 1297;
/// A placed actor (`1202`), and each of its meshes (`75` and a dispatcher
/// draw, `200`).
const PLACED_CYCLES: i64 = 1202;
const PLACED_MESH_CYCLES: i64 = 75 + 200;
/// The draw's morph substitution: per object, the `FUN_8001C604` call
/// (`396` besides its copy, `rim_elm_zoom_intro_pcsx`'s four tide meshes)
/// and the object path's own share (`son_field_door_hop`'s `56` objects on
/// the stage cost `1252` against the pool's `919`); a vertex the call
/// copies (`22`), a vertex `FUN_8005B038` blends (`61`).
const MORPH_CALL_CYCLES: i64 = 396 + 333;
const MORPH_COPY_CYCLES: i64 = 22;
const MORPH_VERTEX_CYCLES: i64 = 61;
/// A move-VM part on draw kind `4`, its sprite-emitter draw through the
/// actor pass: `3300` drawn, `705` off screen, `3071` at
/// `uru2_field_door_hop`'s `62` drawn of `68`.
const SPRITE_PART_CYCLES: i64 = 3071;
/// A fog record, its half-sheets kept (`233`) and culled (`149`); each
/// record emits two.
const FOG_RECORD_CYCLES: i64 = 655;
const FOG_KEPT_CYCLES: i64 = 233;
const FOG_CULLED_CYCLES: i64 = 149;

/// The player's model when the scene's table lacks it: one model
/// actor of `10` objects and `388` primitives at the actor pool's mean
/// (`136` a primitive), the shape of `stone_field_door_hop`'s only model.
const PLAYER_CYCLES: i64 = NPC_CYCLES + 10 * NPC_OBJECT_CYCLES + 10 * GROUP_CYCLES + 388 * 136;
/// A render-list entry the walk tests (`50..61` cycles, mean `59`).
const LIST_ENTRY_CYCLES: i64 = 59;
/// The lists' entries besides the script actors and the parts: the player
/// and the system actors.
const LIST_FIXED_ENTRIES: i64 = 5;

/// The tick passes (`FUN_8002519C`, `retail-exec actor-ticks` over thirteen
/// states): a script actor's tick through `FUN_8003BC08` (`485`, `669`
/// entries), the player's (`FUN_801D1344`: `5071` standing, `31115`
/// walking or running, `3106..44018` in all), the system actors' (the
/// ambient emitter, the camera and the rest: `2967..24399`, mean `12300`),
/// a sprite part's (`1475`), and any other move-VM part's own tick off a
/// cycler upload (`689..3940`, mean `2243` over `35`; an upload frame adds
/// the cycler terms).
const TICK_ACTOR_CYCLES: i64 = 485;
const TICK_PLAYER_STILL_CYCLES: i64 = 5_071;
const TICK_PLAYER_MOVING_CYCLES: i64 = 31_115;
const TICK_SYSTEM_CYCLES: i64 = 12_300;
const TICK_SPRITE_PART_CYCLES: i64 = 1_475;
const TICK_PART_CYCLES: i64 = 2_243;

/// Fitted, not derived: the frame driver's work outside the priced passes
/// and the pacer's measure past it (`2..6` hblanks over the ten states).
const OTHER_HBLANKS: i64 = 4;

/// The frame time, in hblanks, a field frame that draws `f` costs on
/// PCSX-Redux's clock, priced from the derived per-unit table: the work of
/// each pass from its counts, the tick passes, the fitted other work, and the interrupts the
/// work spans (`VBLANK_INTERRUPT_CYCLES` and `OTHER_INTERRUPT_CYCLES_PER_VSYNC`
/// once every `263` hblanks of work).
///
/// A primitive is priced at its kind's pooled outcome mix, not at what the
/// camera makes of it: retail transforms every primitive of a drawn mesh
/// and links only the visible part, and across the ten states the mix
/// moves a pass's price by up to `6%`.
pub fn derived_field_frame_cost(f: &FieldFrameFeatures) -> i32 {
    derived_field_frame_parts(f)[5]
}

/// [`derived_field_frame_cost`] by part, in hblanks: the decoration pass,
/// the ground emitter, the actor render passes, the fog walk, the tick share
/// (the tick passes, the cyclers and the fitted other work), and the whole price
/// with the interrupt term.
pub fn derived_field_frame_parts(f: &FieldFrameFeatures) -> [i32; 6] {
    let kinds = |k: &[u32; 12], w: &[i64; 12]| -> i64 {
        k.iter().zip(w).map(|(n, c)| i64::from(*n) * c).sum()
    };
    let n = |v: u32| i64::from(v);
    let outside = n(f.decoration_loop_cells)
        .saturating_sub(n(f.decoration_box_cells))
        .max(0);
    let decoration = 1622
        + n(f.decoration_rows) * 24
        // A drawn cell's path (`503`) is the whole iteration, its box test
        // among it: the box's other cells take the `99` one.
        + (n(f.decoration_box_cells) - n(f.decoration_draws)).max(0) * 99
        + outside * 54
        + n(f.decoration_draws) * DECORATION_DRAW_CYCLES
        + n(f.decoration_groups) * GROUP_CYCLES
        + kinds(&f.decoration_kinds, &DECORATION_PRIM_CYCLES);
    let ground = 129
        + n(f.ground_rows) * 24
        + (n(f.ground_window_cells) - n(f.ground_cells)).max(0) * 35
        + n(f.ground_far_cells) * GROUND_FAR_CELL_CYCLES
        + n(f.ground_sorted_cells) * GROUND_SORTED_CELL_CYCLES
        + (n(f.ground_cells) - n(f.ground_far_cells) - n(f.ground_sorted_cells)).max(0)
            * GROUND_WALK_CELL_CYCLES;
    let actors = 5 * 120
        + (n(f.tick_actors) + n(f.parts) + LIST_FIXED_ENTRIES) * LIST_ENTRY_CYCLES
        + f.player.map_or(PLAYER_CYCLES, |m| {
            let k: [u32; 12] = m.kinds.map(u32::from);
            NPC_CYCLES
                + i64::from(m.objects) * NPC_OBJECT_CYCLES
                + i64::from(m.groups) * GROUP_CYCLES
                + kinds(&k, &ACTOR_PRIM_CYCLES)
        })
        + n(f.placed_draws) * PLACED_CYCLES
        + n(f.placed_objects) * PLACED_MESH_CYCLES
        + n(f.placed_groups) * GROUP_CYCLES
        + kinds(&f.placed_kinds, &ACTOR_PRIM_CYCLES)
        + kinds(&f.placed_offtest_kinds, &ACTOR_OFFTEST_PRIM_CYCLES)
        + n(f.npc_models) * NPC_CYCLES
        + n(f.npc_objects) * NPC_OBJECT_CYCLES
        + n(f.npc_groups) * GROUP_CYCLES
        + kinds(&f.npc_kinds, &ACTOR_PRIM_CYCLES)
        + kinds(&f.npc_cued_kinds, &ACTOR_OFFTEST_PRIM_CYCLES)
        + n(f.sprite_parts) * SPRITE_PART_CYCLES
        + n(f.morph_calls) * MORPH_CALL_CYCLES
        + n(f.morph_copied) * MORPH_COPY_CYCLES
        + n(f.morph_vertices) * MORPH_VERTEX_CYCLES;
    let fog = n(f.fog) * FOG_RECORD_CYCLES
        + n(f.fog_quads) * FOG_KEPT_CYCLES
        + (2 * n(f.fog) - n(f.fog_quads)).max(0) * FOG_CULLED_CYCLES;
    let tick = if f.player_moving {
        TICK_PLAYER_MOVING_CYCLES
    } else {
        TICK_PLAYER_STILL_CYCLES
    } + n(f.tick_actors) * TICK_ACTOR_CYCLES
        + TICK_SYSTEM_CYCLES
        + n(f.sprite_parts) * TICK_SPRITE_PART_CYCLES
        + n(f.parts).saturating_sub(n(f.sprite_parts)) * TICK_PART_CYCLES;
    let cycles = decoration + ground + actors + fog + tick;
    // The cyclers' measured terms, in 1/1024 hblank.
    let ticked = n(f.cycler_calls) * COST_PER_CYCLER_CALL
        + n(f.cycler_texels) * COST_PER_CYCLER_TEXEL
        + n(f.cycler_zero_texels) * COST_PER_CYCLER_ZERO_TEXEL;
    let work_x100 =
        cycles * 10_000 / CYCLES_PER_HBLANK_X100 + (ticked >> 10) * 100 + OTHER_HBLANKS * 100;
    // Each 263 hblanks of work takes one vblank interrupt and the vsync's
    // others.
    let irq = i64::from(VBLANK_INTERRUPT_CYCLES + OTHER_INTERRUPT_CYCLES_PER_VSYNC);
    let total_x100 = work_x100 + work_x100 * irq * 100 / (263 * CYCLES_PER_HBLANK_X100);
    let hb = |c: i64| (c * 100 / CYCLES_PER_HBLANK_X100) as i32;
    [
        hb(decoration),
        hb(ground),
        hb(actors),
        hb(fog),
        hb(tick) + ((ticked >> 10) + OTHER_HBLANKS) as i32,
        (total_x100 / 100).clamp(0, FIELD_FRAME_COST_MAX) as i32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_holds_each_unit_once() {
        let mut seen = std::collections::BTreeSet::new();
        for (u, c, n) in FIELD_UNIT_CYCLES_X10 {
            assert!(seen.insert(*u), "{u} twice");
            assert!(*n > 0 && *c > 0, "{u}");
        }
        assert_eq!(unit_cycles("decoration.cell[inbox]"), Some(99.0));
    }

    /// `son_field_door_hop`'s first frame as retail counts it (the runner's
    /// units: `2480` decoration primitives in `92` draws and `247` groups,
    /// `2173` span cells all in the box over `53` rows, `1333` ground window
    /// cells none walkable, `6` models besides the player's with `76` objects and `285` groups,
    /// `2158` actor primitives, the player's among them, and four models' `56`
    /// objects through the morph stage: `893` vertices copied, `120` blended)
    /// prices within a tenth of its ring (`610..621` hblanks with interrupts).
    #[test]
    fn son_prices_near_its_ring() {
        let mut f = FieldFrameFeatures {
            decoration_draws: 92,
            decoration_loop_cells: 2173,
            decoration_box_cells: 2173,
            decoration_rows: 53,
            decoration_groups: 247,
            ground_window_cells: 1333,
            ground_rows: 43,
            npcs: 6,
            npc_models: 6,
            npc_objects: 76,
            npc_groups: 285,
            list_actors: 41,
            parts: 1,
            morph_calls: 56,
            morph_copied: 893,
            morph_vertices: 120,
            ..FieldFrameFeatures::default()
        };
        f.decoration_kinds[17 - 8] = 504;
        f.decoration_kinds[18 - 8] = 736;
        f.decoration_kinds[19 - 8] = 1240;
        // Less the player's `388`, which the table prices apart.
        f.npc_kinds[14 - 8] = 422;
        f.npc_kinds[15 - 8] = 369;
        f.npc_kinds[18 - 8] = 436;
        f.npc_kinds[19 - 8] = 180;
        f.npc_kinds[12 - 8] = 208;
        f.npc_kinds[13 - 8] = 111;
        f.npc_kinds[17 - 8] = 44;
        let c = derived_field_frame_cost(&f);
        assert!((560..680).contains(&c), "{c}");
    }
}

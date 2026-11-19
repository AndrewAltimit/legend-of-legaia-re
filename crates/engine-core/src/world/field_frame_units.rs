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
//! Nothing reads this table yet: the frame-cost model
//! (`world::field_frame_cost`) stays the fitted one. It is the data a
//! derived model prices with.

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
    ("actor.object[posed]", 12519, 56),
    ("actor.placed mesh[drawn]", 748, 87),
    ("actor.pose[-]", 2532, 69),
    ("actor.track[-]", 220, 1939),
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
}

//! Battle frames priced by unit off library states, and the derived table
//! (`legaia_engine_battle_loop::derived_cost`) held against them.
//!
//! `PartRunner` resumes a battle state and times every part of each frame
//! (each dispatcher primitive by its leaf and fate, each group and call, each
//! body draw and its objects, the grid, the template tick, the walker visit by
//! visit, the mesh builds, the part ticks), each part's own cycles with its
//! nested parts taken out. The table prices the same units; the frame's ring
//! entry (the pacer's CPU word, interrupt-free here) is what the sum must
//! reproduce.

use crate::common::{library_state, resident_is_retail};
use legaia_engine_battle_loop::derived_cost as dc;
use legaia_engine_battle_loop::frame_clock as fc;
use psx_retail_exec::PCSX_CYCLES_PER_HBLANK;
use psx_retail_exec::battle_cost::{DISPATCHER, FrameParts, Part, PartRunner, PrimFate};

const BODY_DRAW: u32 = 0x8004_8A08;
const GRID: u32 = 0x801D_02C0;
const TEMPLATE_TICK: u32 = 0x8004_DA00;
const WALKER: u32 = 0x801E_0080;
const ME_DECODE: u32 = 0x8002_B28C;
const TMD_RENDER: u32 = 0x8002_735C;

/// `(label, fingerprint)`: a lone attacker, an enemy breath, a ground cast
/// with a hundred pool children, full-party strikes and arts, a summoner's
/// cast, a crowded strike.
const STATES: &[(&str, &str)] = &[
    (
        "party_basic_attack_vs_gobu_gobu",
        "658af41bc58b055801090f4fc91f7b4311b87aa9d505f49b94342e21bb0543f6",
    ),
    (
        "battle_rikuroa_caruban_fire_breath_mid_cast",
        "3d8836bfd265c92fc204e88a64b6dbc59f626da0cd020bc9edf6105eee0c7aa8",
    ),
    (
        "battle_nilboa_rock_lizard_earthquake_mid_cast",
        "cc764ffdeb492498e0cd4baba1484916a7e72b5fa3b2cd5d973e45d6d67a253e",
    ),
    (
        "battle_jou_gimard_v3_strike",
        "616bffb8fc47870f5e93805a7f3496a98d81f27fb230fe2b6840eee5c3f8f0c3",
    ),
    (
        "battle_jou_party_vahn_chain_open",
        "a99eef4667a4f42fa3918c2f4876c1b292696c871f07454a54ee868e496c8f0f",
    ),
    (
        "battle_jou_party_vahn_tri_somersault_super_open",
        "bfbe7535d122494ef15495c084ed312f49a48344b6aec71e29cfb4eb242126a8",
    ),
    (
        "battle_jou_cort_181_strike",
        "56c4635113e779df5b6efd693ef63edf53fdecec03237f4f87263b4418c9028e",
    ),
    (
        "battle_chitei2_gilium_thunder_storm_mid_cast",
        "387391845c741f7abdc1472fe60474ed8bd20707074f55c9dab36d3d2e42c8dd",
    ),
    (
        "battle_dohaty_dohati_strike",
        "d92c74786152c5360d264d993eaa23da155fe04e0f93e4a69c99587ea03c8d43",
    ),
    (
        "battle_noaru_songi_strike",
        "5bc174d4d66c19a5b780e415a2ebb7500d0b7072f9028ce4bebc8aa997b896b2",
    ),
];

/// The table's price of one frame's units, in cycles (`None` for a frame
/// that holds a unit the table does not price by count: an art's `"ME"`
/// decode, a floor-clipped walk).
fn derived_cycles(f: &FrameParts) -> Option<u64> {
    if f.get(Part::Call(ME_DECODE)).0 > 0 || f.get(Part::Call(TMD_RENDER)).0 > 0 {
        return None;
    }
    let n = |p: Part| f.get(p).0;
    let objects = |p: Part| f.objects.get(&p).copied().unwrap_or(0);
    let linked = |va: u32| f.linked.get(&va).copied().unwrap_or(0).max(0) as u64;
    let c = |v: u32| u64::from(v);
    let bodies = n(Part::Call(BODY_DRAW));
    let mut s = c(dc::FRAME_BASE_CYCLES) + bodies * c(dc::FRAME_BASE_BODY_CYCLES);
    for (p, &(visits, _)) in &f.parts {
        if let Part::Prim(leaf, fate) = *p {
            let l = dc::leaf_cost(leaf)?;
            let per = match fate {
                PrimFate::Linked => l.linked,
                PrimFate::OffScreen => l.off_screen,
                PrimFate::Culled => l.culled,
            };
            s += visits * c(per);
        }
    }
    s += n(Part::Group) * c(dc::DISPATCH_GROUP_CYCLES);
    s += n(Part::Call(DISPATCHER)) * c(dc::DISPATCH_CALL_CYCLES);
    s += bodies * c(dc::BODY_DRAW_CYCLES)
        + objects(Part::Call(BODY_DRAW)) * c(dc::BODY_OBJECT_CYCLES);
    s += objects(Part::GhostDraw) * c(dc::GHOST_OBJECT_CYCLES);
    s += n(Part::Call(0x8004_998C)) * c(dc::KEYFRAME_DECODE_CYCLES);
    s += n(Part::Call(0x8004_80D8)) * c(dc::DRAW_TICK_CYCLES);
    s += n(Part::Call(0x8004_9348)) * c(dc::GHOST_WALK_CYCLES);
    s += n(Part::Call(0x8001_ADA4)) * c(dc::ACTOR_DRAW_CYCLES);
    s += n(Part::Call(0x8002_1DF4)) * c(dc::PART_TICK_CYCLES);
    for &(shape, segments) in &f.builds {
        s += if shape == 0 {
            c(dc::MESH_BUILD_CYCLES) + u64::from(segments) * c(dc::MESH_BUILD_SEGMENT_CYCLES)
        } else {
            c(dc::MESH_BUILD_SHAPE3_CYCLES)
        };
    }
    if n(Part::Call(GRID)) > 0 {
        s += c(dc::GRID_FIXED_CYCLES) + linked(GRID) / 4 * c(dc::GRID_CELL_CYCLES);
    }
    s += linked(TEMPLATE_TICK) * c(dc::TEMPLATE_TICK_PACKET_CYCLES);
    s += n(Part::Call(WALKER)) * c(dc::WALKER_CALL_CYCLES)
        + u64::from(f.sweeps) * c(dc::WALKER_SWEEP_CYCLES)
        + n(Part::ChildStep) * c(dc::CHILD_STEP_CYCLES)
        + f.overrun_advances * c(dc::CHILD_OVERRUN_ADVANCE_CYCLES)
        + n(Part::ChildDraw) * c(dc::CHILD_DRAW_CYCLES);
    Some(s)
}

#[test]
fn the_derived_table_reproduces_the_battle_frames() {
    not_a_differential!(
        "prices retail battle frames by unit off library states against the derived table; compares no port"
    );
    let mut ran = 0;
    let mut sq = 0.0;
    let mut frames = 0usize;
    // Walker visit totals over every state: (step visits, cycles), (draws,
    // cycles), (overrun advances, cycles).
    let mut walk = [(0u64, 0u64); 3];
    for (label, fp) in STATES {
        let Some(state) = library_state(fp) else {
            continue;
        };
        for va in [DISPATCHER, BODY_DRAW] {
            assert!(
                resident_is_retail(&state, va, None),
                "{label}: FUN_{va:08X} is not the retail image's"
            );
        }
        let mut run = PartRunner::new(&state).unwrap_or_else(|e| panic!("{label}: {e}"));
        let (mut ring, mut derived, mut n) = (0.0, 0.0, 0.0);
        for _ in 0..4 {
            let f = run.frame().unwrap_or_else(|e| panic!("{label}: {e}"));
            for (i, p) in [Part::ChildStep, Part::ChildDraw, Part::ChildOverrun]
                .iter()
                .enumerate()
            {
                let (v, cy) = f.get(*p);
                walk[i].0 += if i == 2 { f.overrun_advances } else { v };
                walk[i].1 += cy;
            }
            // A clamped entry (`0x2BC`) is not the frame's work.
            if f.ring >= 0x2BC {
                continue;
            }
            let Some(d) = derived_cycles(&f) else {
                continue;
            };
            let d = d as f64 / PCSX_CYCLES_PER_HBLANK;
            sq += (d - f64::from(f.ring)).powi(2);
            frames += 1;
            ring += f64::from(f.ring);
            derived += d;
            n += 1.0;
        }
        if n == 0.0 {
            continue;
        }
        let (ring, derived) = (ring / n, derived / n);
        eprintln!(
            "{label}: ring {ring:.1} derived {derived:.1} ({:+.1}%)",
            (derived - ring) / ring * 100.0
        );
        assert!(
            (derived - ring).abs() / ring <= 0.05,
            "{label}: ring {ring:.1} hblanks, derived {derived:.1}"
        );
        ran += 1;
    }
    if frames > 0 {
        let rms = (sq / frames as f64).sqrt();
        eprintln!("{ran} states, {frames} frames, rms {rms:.1} hblanks");
        assert!(rms < 20.0, "rms {rms:.1}");
    }
    // The walker's visits read the branch prices the random-walk fit gave
    // (`frame_clock::EFFECT_WALK_*`): a countdown or one advance against
    // the fit's `157` / `166`, an overrun advance against `149`, a draw
    // against `652`.
    let mean = |(v, c): (u64, u64)| c as f64 / v.max(1) as f64;
    if walk[1].0 > 100 {
        let near = |got: f64, want: u32| (got - f64::from(want)).abs() / f64::from(want) <= 0.1;
        assert!(
            near(mean(walk[0]), fc::EFFECT_WALK_CHILD_WAIT_CYCLES),
            "step {}",
            mean(walk[0])
        );
        assert!(
            near(mean(walk[1]), fc::EFFECT_WALK_DRAWN_CYCLES),
            "draw {}",
            mean(walk[1])
        );
        assert!(
            near(mean(walk[2]), fc::EFFECT_WALK_OVERRUN_CYCLES),
            "overrun {}",
            mean(walk[2])
        );
    }
    eprintln!("{ran} library states priced");
}

#[test]
fn each_leaf_path_costs_what_the_table_says() {
    not_a_differential!(
        "times the dispatcher's leaves per primitive off library states against the derived table; compares no port"
    );
    let mut sums: std::collections::BTreeMap<(u32, PrimFate), (u64, u64)> = Default::default();
    for (label, fp) in STATES.iter().take(4) {
        let Some(state) = library_state(fp) else {
            continue;
        };
        let mut run = PartRunner::new(&state).unwrap_or_else(|e| panic!("{label}: {e}"));
        for _ in 0..2 {
            let f = run.frame().unwrap_or_else(|e| panic!("{label}: {e}"));
            for (p, &(v, cy)) in &f.parts {
                if let Part::Prim(leaf, fate) = *p {
                    let e = sums.entry((leaf, fate)).or_default();
                    e.0 += v;
                    e.1 += cy;
                }
            }
        }
    }
    for ((leaf, fate), (v, cy)) in sums {
        if v < 200 {
            continue;
        }
        let l = dc::leaf_cost(leaf).unwrap_or_else(|| panic!("leaf {leaf:08X} not in the table"));
        let want = match fate {
            PrimFate::Linked => l.linked,
            PrimFate::OffScreen => l.off_screen,
            PrimFate::Culled => l.culled,
        };
        let got = cy as f64 / v as f64;
        assert!(
            (got - f64::from(want)).abs() / f64::from(want) <= 0.06,
            "leaf {leaf:08X} {fate:?}: {got:.1} cycles a primitive over {v}, table {want}"
        );
    }
}

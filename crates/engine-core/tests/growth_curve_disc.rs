//! Disc-gated: the per-character stat-growth port drives the engine level-up
//! from the real `SCUS_942.54` curves.
//!
//! `LevelUpTracker::with_growth_tables` installs the parsed static-SCUS growth
//! tables (`DAT_800769CC` curves + `DAT_80076918` parameter block, read by the
//! retail victory applier `FUN_801E9504`) as per-level `StatGrowthCurve`s for
//! Vahn / Noa / Gala. This test closes the loop at the *engine* level (the asset
//! parser is covered by `crates/asset/tests/level_up_tables_real.rs`; the full
//! boot install by `crates/engine-shell/tests/new_game_seed.rs`): it validates
//! the installed curve against the new-game starting-party seed (each stat's
//! level-1 `start`) and against the byte-pinned Noa L2->L3 capture. Skips without
//! `LEGAIA_DISC_BIN`.

use legaia_asset::level_up_tables::{
    GROWTH_CHAR_COUNT, GROWTH_STAT_COUNT, growth_tables_from_scus,
};
use legaia_asset::new_game::StartingParty;
use legaia_engine_core::Vfs;
use legaia_engine_core::levelup::{LevelUpTracker, StatGrowthCurve};
use std::path::PathBuf;

fn read_scus() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return None;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    Some(scus)
}

#[test]
fn growth_port_installs_per_level_curves_and_matches_seed_and_capture() {
    let Some(scus) = read_scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let tables = growth_tables_from_scus(&scus).expect("parse SCUS growth tables");
    let party = StartingParty::from_scus(&scus).expect("parse new-game starting party");

    // Install the parsed curves into the engine's level-up driver.
    let tracker = LevelUpTracker::new().with_growth_tables(&tables);

    // Vahn / Noa / Gala get real per-level curves; the flat placeholder is gone.
    for (slot, curve) in tracker
        .stat_curves
        .iter()
        .take(GROWTH_CHAR_COUNT)
        .enumerate()
    {
        assert!(
            matches!(curve, StatGrowthCurve::PerLevel(_)),
            "slot {slot} should carry the SCUS-derived per-level curve"
        );
    }
    // The 4th roster slot is never grown by FUN_801E9504 - keeps the flat default.
    assert!(matches!(tracker.stat_curves[3], StatGrowthCurve::Flat(_)));

    // The tracker retains the raw tables (needed for the opt-in jitter pass); use
    // them to cross-check each character's level-1 `start` against the new-game
    // starting-party seed - the same pin the asset parser makes, now proven to
    // survive the engine install.
    let retained = tracker
        .growth_tables
        .as_ref()
        .expect("with_growth_tables retains the parsed tables");
    let template = |slot: usize| {
        let m = party.member(slot).expect("template member");
        [
            m.hp_max, m.mp_max, m.agl, m.atk, m.udf, m.ldf, m.spd, m.intel,
        ]
    };
    for slot in 0..GROWTH_CHAR_COUNT {
        let cp = retained.char_params(slot).expect("char growth params");
        let t = template(slot);
        // HP / MP / AGL `start` match the template for every character.
        assert_eq!(cp.stats[0].start, t[0], "slot {slot} HP start == seed");
        assert_eq!(cp.stats[1].start, t[1], "slot {slot} MP start == seed");
        assert_eq!(cp.stats[2].start, t[2], "slot {slot} AGL start == seed");
    }
    // Gala (slot 2) matches the seed on all 8 stats - the decisive pin.
    let gala = retained.char_params(2).unwrap();
    let gt = template(2);
    assert_eq!(gala.stats.len(), GROWTH_STAT_COUNT);
    for (s, st) in gala.stats.iter().enumerate() {
        assert_eq!(st.start, gt[s], "Gala stat {s} start == seed");
    }

    // Known level: Noa (slot 1) leveling FROM L2 -> L3 reads curve index 1. The
    // deterministic (jitter-free) core is byte-validated against the
    // noa_levelup_field_pre/_post capture: HP core 37 (observed +39 with jitter),
    // MP core 6 (observed +5). Every one of the eight installed-curve gains is at
    // least the applier's floor of 1.
    let noa_l2 = tracker.stat_curves[1].gain_for(2);
    assert_eq!(noa_l2.hp, 37, "Noa L2->L3 HP growth core");
    assert_eq!(noa_l2.mp, 6, "Noa L2->L3 MP growth core");
    assert_ne!((noa_l2.hp, noa_l2.mp), (10, 5), "not the flat placeholder");
    for g in noa_l2.battle() {
        assert!(
            g >= 1,
            "each battle-stat gain honours the applier's floor of 1"
        );
    }

    // Drive an actual grant through the engine: 365 total XP reaches L3, and the
    // summed HP gain (L1->L2 plus L2->L3) is the two per-level cores, not the
    // 20-HP flat placeholder (2 x 10).
    let mut driven = LevelUpTracker::new().with_growth_tables(&tables);
    let r = driven.grant_xp(1, 365).expect("Noa reaches L3 on 365 XP");
    assert_eq!(r.new_level, 3);
    let expect_hp = tracker.stat_curves[1].gain_for(1).hp + noa_l2.hp;
    assert_eq!(
        r.hp_gained, expect_hp,
        "summed HP core across the two levels"
    );
    assert_ne!(r.hp_gained, 20, "not the flat 2x10 placeholder");
}

//! Unit tests for `levelup`, extracted verbatim.

use super::*;
use legaia_save::CharacterRecord;

#[test]
fn no_level_up_when_xp_below_threshold() {
    // Use placeholder table for stable threshold values (L2 threshold = 100).
    let mut t = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
    assert!(t.grant_xp(0, 99).is_none()); // threshold for level 2 = 100
    assert_eq!(t.level[0], 1);
    assert_eq!(t.xp[0], 99);
}

#[test]
fn level_up_at_exact_threshold() {
    let mut t = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
    let r = t.grant_xp(0, 100).expect("should level up");
    assert_eq!(r.old_level, 1);
    assert_eq!(r.new_level, 2);
    assert_eq!(r.hp_gained, 10);
    assert_eq!(r.mp_gained, 5);
    assert_eq!(t.level[0], 2);
}

#[test]
fn multi_level_jump() {
    let mut t = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
    // level 1→2 needs 100 XP, 1→3 needs 400 XP total (placeholder: 100×n²)
    let r = t.grant_xp(0, 400).expect("should jump levels");
    assert_eq!(r.old_level, 1);
    assert_eq!(r.new_level, 3);
    assert_eq!(r.hp_gained, 20); // 2 × 10
    assert_eq!(r.mp_gained, 10); // 2 × 5
}

#[test]
fn xp_corrections_shift_slots_one_and_two_only() {
    // Placeholder L2 threshold = 100; divisor 50 at level 1 gives a
    // correction of 100*0x14/50 = 40. Slot 1 (Noa) levels at 60, slot 2
    // (Gala) at 140, slots 0/3 at the uncorrected 100 (FUN_801E9504).
    let divisors = vec![0i16, 50, 50, 50];
    let make = || {
        LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_xp_corrections(divisors.clone())
    };

    assert_eq!(make().threshold_for(0, 1), Some(100), "Vahn uncorrected");
    assert_eq!(make().threshold_for(1, 1), Some(60), "Noa earlier");
    assert_eq!(make().threshold_for(2, 1), Some(140), "Gala later");
    assert_eq!(make().threshold_for(3, 1), Some(100), "slot 3 uncorrected");

    // grant_xp consumes the corrected threshold: 60 XP levels Noa but
    // leaves Vahn and Gala at L1.
    let mut t = make();
    assert!(t.grant_xp(0, 60).is_none(), "Vahn needs 100");
    assert!(t.grant_xp(1, 60).is_some(), "Noa levels at 60");
    assert!(t.grant_xp(2, 60).is_none(), "Gala needs 140");

    // Without divisors installed every slot takes the base threshold.
    let mut bare = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
    assert_eq!(bare.threshold_for(1, 1), Some(100));
    assert!(bare.grant_xp(1, 60).is_none());
}

#[test]
fn growth_tables_drive_deterministic_per_level_hp_mp() {
    use legaia_asset::level_up_tables::{
        GROWTH_PARAM_LEN, GROWTH_ROW_COUNT, GROWTH_ROW_STRIDE, GrowthTables,
    };
    // A curve of all 0x60 sums to 98 × 96 = 9408 = 0x24C0, so the divide is
    // exact and gain = (max-start) × 0x60 / 0x24C0 every level.
    let curves = vec![vec![0x60u8; GROWTH_ROW_STRIDE]; GROWTH_ROW_COUNT];
    let mut param = vec![0u8; GROWTH_PARAM_LEN];
    // slot 0 HP: start=100, max=4900 → (4800 × 96) / 9408 = 48; jitter 0, row 0.
    param[0..6].copy_from_slice(&[100, 0, 0x24, 0x13, 0, 0]); // 0x1324 = 4900
    // slot 0 MP: start=10, max=970 → (960 × 96) / 9408 = 9.
    param[6..12].copy_from_slice(&[10, 0, 0xCA, 0x03, 0, 0]); // 0x03CA = 970
    let g = GrowthTables { curves, param };

    let mut t = LevelUpTracker::new()
        .with_xp_table(placeholder_xp_table())
        .with_growth_tables(&g);

    // Curve replaced the flat placeholder for slot 0.
    assert!(matches!(t.stat_curves[0], StatGrowthCurve::PerLevel(_)));
    // L1 → L2: deterministic core, not the 10/5 flat rate.
    let r = t.grant_xp(0, 100).expect("level up");
    assert_eq!(r.new_level, 2);
    assert_eq!(r.hp_gained, 48);
    assert_eq!(r.mp_gained, 9);
    // Slot 3 (no growth record) keeps the flat default.
    assert!(matches!(t.stat_curves[3], StatGrowthCurve::Flat(_)));
}

/// Build a synthetic `GrowthTables` whose every curve is the flat `0x60`
/// ramp (sums to `0x24C0`, so the divide is exact), with slot-0 HP/MP params
/// `start/max/jitter` configurable. HP is stat 0, MP stat 1.
fn synth_growth(hp_jitter: u8, mp_jitter: u8) -> legaia_asset::level_up_tables::GrowthTables {
    use legaia_asset::level_up_tables::{
        GROWTH_PARAM_LEN, GROWTH_ROW_COUNT, GROWTH_ROW_STRIDE, GrowthTables,
    };
    let curves = vec![vec![0x60u8; GROWTH_ROW_STRIDE]; GROWTH_ROW_COUNT];
    let mut param = vec![0u8; GROWTH_PARAM_LEN];
    // HP: start=100, max=4900 → raw core 48 every level.
    param[0..6].copy_from_slice(&[100, 0, 0x24, 0x13, hp_jitter, 0]);
    // MP: start=10, max=970 → raw core 9 every level.
    param[6..12].copy_from_slice(&[10, 0, 0xCA, 0x03, mp_jitter, 0]);
    GrowthTables { curves, param }
}

#[test]
fn bios_rand_is_the_psx_lcg() {
    // seed=1: 1×0x41C64E6D + 0x3039 = 0x41C67EA6; (>>16)&0x7FFF = 0x41C6.
    let mut r = BiosRand::new(1);
    assert_eq!(r.next_u15(), 0x41C6);
    // Deterministic for a given seed.
    let mut a = BiosRand::new(0xDEAD_BEEF);
    let mut b = BiosRand::new(0xDEAD_BEEF);
    for _ in 0..256 {
        assert_eq!(a.next_u15(), b.next_u15());
    }
    // 15-bit range.
    let mut c = BiosRand::new(7);
    for _ in 0..10_000 {
        assert!(c.next_u15() <= 0x7FFF);
    }
}

#[test]
fn level_up_jitter_off_draws_nothing_and_equals_core() {
    let g = synth_growth(4, 2);
    let mut t = LevelUpTracker::new()
        .with_xp_table(placeholder_xp_table())
        .with_growth_tables(&g);
    // No jitter RNG installed → deterministic core, no rand consumed.
    assert!(t.jitter_rng.is_none());
    let r = t.grant_xp(0, 100).expect("level up");
    assert_eq!((r.hp_gained, r.mp_gained), (48, 9));
    assert!(t.jitter_rng.is_none()); // still none - nothing was drawn
}

#[test]
fn level_up_jitter_stays_in_band_and_is_seed_deterministic() {
    let g = synth_growth(4, 0); // HP jitters ±4, MP has zero jitter
    let build = |seed| {
        LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_growth_tables(&g)
            .with_level_up_jitter(seed)
    };
    // Same seed → identical roll.
    let mut t1 = build(0x1234_5678);
    let mut t2 = build(0x1234_5678);
    let r1 = t1.grant_xp(0, 100).unwrap();
    let r2 = t2.grant_xp(0, 100).unwrap();
    assert_eq!((r1.hp_gained, r1.mp_gained), (r2.hp_gained, r2.mp_gained));

    // Across seeds: HP lands in [44, 52] (raw 48 ± 4, floored at 1); MP is
    // exactly 9 every time (jitter 0 ⇒ spread 0, even though a draw is made).
    let mut saw_low = false;
    let mut saw_high = false;
    for seed in 1u32..400 {
        let mut t = build(seed);
        let r = t.grant_xp(0, 100).unwrap();
        assert!(
            (44..=52).contains(&r.hp_gained),
            "hp {} out of band",
            r.hp_gained
        );
        assert_eq!(r.mp_gained, 9, "mp must be unaffected by jitter==0");
        saw_low |= r.hp_gained < 48;
        saw_high |= r.hp_gained > 48;
    }
    // The spread actually moves the value both ways (not a stuck constant).
    assert!(
        saw_low && saw_high,
        "jitter should vary above and below the core"
    );
}

#[test]
fn level_up_jitter_multilevel_matches_per_level_draw_order() {
    // A 2-level jump must draw rand per stat per level (HP then MP, level
    // L1→L2 then L2→L3) - so the total equals the sum of two independently
    // jittered single levels off the same stream.
    let g = synth_growth(4, 0);
    let seed = 0xABCD_1234;
    // One tracker that jumps two levels at once.
    let mut jump = LevelUpTracker::new()
        .with_xp_table(placeholder_xp_table())
        .with_growth_tables(&g)
        .with_level_up_jitter(seed);
    let rj = jump.grant_xp(0, 400).expect("two-level jump"); // placeholder L3 = 400
    assert_eq!(rj.new_level, 3);

    // A tracker that levels one at a time off the same seed stream.
    let mut step = LevelUpTracker::new()
        .with_xp_table(placeholder_xp_table())
        .with_growth_tables(&g)
        .with_level_up_jitter(seed);
    let s1 = step.grant_xp(0, 100).unwrap();
    let s2 = step.grant_xp(0, 300).unwrap(); // cumulative 400 → L3
    assert_eq!(
        (rj.hp_gained, rj.mp_gained),
        (s1.hp_gained + s2.hp_gained, s1.mp_gained + s2.mp_gained)
    );
}

#[test]
fn retail_xp_table_level2_threshold() {
    // Retail: 121 XP to reach L2 (the New Game "Next Level 121"); 120 is
    // not enough.
    let mut t = LevelUpTracker::new();
    assert!(t.grant_xp(0, 120).is_none());
    let r = t.grant_xp(0, 1).expect("121 total = level 2");
    assert_eq!(r.new_level, 2);
}

#[test]
fn retail_xp_table_cumulative_check() {
    // Table[1] = 365 (the L3 threshold): granting 365 XP at once should
    // reach level 3.
    let mut t = LevelUpTracker::new();
    let r = t.grant_xp(0, 365).expect("365 XP reaches L3");
    assert_eq!(r.new_level, 3);
}

#[test]
fn already_at_max_level_returns_none() {
    let mut t = LevelUpTracker::new();
    t.level[0] = MAX_LEVEL;
    assert!(t.grant_xp(0, u32::MAX).is_none());
}

#[test]
fn out_of_bounds_char_returns_none() {
    let mut t = LevelUpTracker::new();
    assert!(t.grant_xp(MAX_PARTY as u8, 9999).is_none());
}

#[test]
fn accumulated_xp_carries_across_calls() {
    let mut t = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
    assert!(t.grant_xp(0, 50).is_none());
    // 50 + 50 = 100 → level up (placeholder threshold for L2 = 100)
    let r = t.grant_xp(0, 50).expect("should level up on second call");
    assert_eq!(r.new_level, 2);
    assert_eq!(t.xp[0], 100);
}

#[test]
fn custom_xp_table() {
    let mut t = LevelUpTracker::new().with_xp_table(vec![50, 150, 300]);
    let r = t.grant_xp(0, 50).expect("table[0] = 50");
    assert_eq!(r.new_level, 2);
}

#[test]
fn apply_to_record_bumps_max_and_restores_cur() {
    let mut rec = CharacterRecord::zeroed();
    let mut hms = rec.hp_mp_sp();
    hms.hp_max = 100;
    hms.hp_cur = 40;
    hms.mp_max = 50;
    hms.mp_cur = 10;
    rec.set_hp_mp_sp(hms);
    // Seed the six battle stats in both windows (a real record keeps them
    // equal) so the level-up grows the record side then mirrors to live.
    let seed = legaia_save::character::LiveStats {
        agl: 20,
        atk: 24,
        udf: 16,
        ldf: 12,
        spd: 19,
        int: 9,
    };
    rec.set_live_stats(seed);
    let mut rs = rec.record_stats();
    rs.agl = 20;
    rs.atk = 24;
    rs.udf = 16;
    rs.ldf = 12;
    rs.spd = 19;
    rs.int = 9;
    rec.set_record_stats(rs);

    let result = LevelUpResult {
        char_id: 0,
        old_level: 1,
        new_level: 2,
        xp_gained: 100,
        hp_gained: 10,
        mp_gained: 5,
        battle_gained: [2, 4, 4, 3, 4, 3], // AGL/ATK/UDF/LDF/SPD/INT
    };
    LevelUpTracker::apply_to_record(&result, &mut rec);

    let updated = rec.hp_mp_sp();
    assert_eq!(updated.hp_max, 110);
    assert_eq!(updated.mp_max, 55);
    // HP/MP restored to new max
    assert_eq!(updated.hp_cur, 110);
    assert_eq!(updated.mp_cur, 55);

    // The six battle stats grew in both the live and record-side windows.
    let ls = rec.live_stats();
    assert_eq!(
        [ls.agl, ls.atk, ls.udf, ls.ldf, ls.spd, ls.int],
        [22, 28, 20, 15, 23, 12]
    );
    let rs = rec.record_stats();
    assert_eq!(
        [rs.agl, rs.atk, rs.udf, rs.ldf, rs.spd, rs.int],
        [22, 28, 20, 15, 23, 12]
    );
    assert_eq!((rs.hp_max, rs.mp_max), (110, 55));
}

#[test]
fn multiple_party_slots_independent() {
    let mut t = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
    // char 0 levels up (100 XP ≥ threshold 100), char 1 doesn't (50 < 100)
    assert!(t.grant_xp(0, 100).is_some());
    assert!(t.grant_xp(1, 50).is_none());
    assert_eq!(t.level[0], 2);
    assert_eq!(t.level[1], 1);
}

#[test]
fn with_stat_gain_override() {
    let mut t = LevelUpTracker::new()
        .with_xp_table(placeholder_xp_table())
        .with_stat_gain(StatGain::hp_mp(20, 15));
    let r = t.grant_xp(0, 100).expect("level up");
    assert_eq!(r.hp_gained, 20);
    assert_eq!(r.mp_gained, 15);
}

#[test]
fn per_slot_stat_gains_independent() {
    let gains = [
        StatGain::hp_mp(30, 5),
        StatGain::hp_mp(10, 20),
        StatGain::default(),
        StatGain::default(),
    ];
    let mut t = LevelUpTracker::new()
        .with_xp_table(placeholder_xp_table())
        .with_stat_gains(gains);

    let r0 = t.grant_xp(0, 100).expect("slot 0 levels up");
    assert_eq!(r0.hp_gained, 30);
    assert_eq!(r0.mp_gained, 5);

    let r1 = t.grant_xp(1, 100).expect("slot 1 levels up");
    assert_eq!(r1.hp_gained, 10);
    assert_eq!(r1.mp_gained, 20);
}

#[test]
fn stat_growth_curve_flat_matches_legacy_behavior() {
    let curve = StatGrowthCurve::Flat(StatGain::hp_mp(7, 3));
    // Per-level lookup is the flat value regardless of level.
    for prev in 1u8..10 {
        assert_eq!(curve.gain_for(prev), StatGain::hp_mp(7, 3));
    }
    // Sum across 5 levels = 5×.
    let total = curve.sum_range(1, 6);
    assert_eq!(total, StatGain::hp_mp(35, 15));
}

#[test]
fn stat_growth_curve_per_level_lookup() {
    let curve = StatGrowthCurve::PerLevel(vec![
        StatGain::hp_mp(10, 2), // L1→2
        StatGain::hp_mp(12, 3), // L2→3
        StatGain::hp_mp(15, 4), // L3→4
        StatGain::hp_mp(18, 5), // L4→5
    ]);
    assert_eq!(curve.gain_for(1), StatGain::hp_mp(10, 2));
    assert_eq!(curve.gain_for(4), StatGain::hp_mp(18, 5));
    // Past-table indices fall back to default.
    assert_eq!(curve.gain_for(10), StatGain::default());
    // Sum across 1..=4: 10+12+15+18 = 55, 2+3+4+5 = 14.
    assert_eq!(curve.sum_range(1, 5), StatGain::hp_mp(55, 14));
}

#[test]
fn level_up_uses_per_level_curve_when_installed() {
    // Multi-level jump (L1 → L3 with 400 XP under placeholder table).
    // Curve gives 7 HP for L1→2 and 13 HP for L2→3 (total 20).
    let curve = StatGrowthCurve::PerLevel(vec![
        StatGain::hp_mp(7, 1),
        StatGain::hp_mp(13, 2),
        // … rest unused for this test
    ]);
    let mut t = LevelUpTracker::new()
        .with_xp_table(placeholder_xp_table())
        .with_stat_curve(curve);
    let r = t.grant_xp(0, 400).expect("level up");
    assert_eq!(r.old_level, 1);
    assert_eq!(r.new_level, 3);
    assert_eq!(r.hp_gained, 20); // 7 + 13
    assert_eq!(r.mp_gained, 3); // 1 + 2
}

#[test]
fn observation_to_curve_yields_per_level_average_inside_range() {
    let obs = LevelUpObservation {
        label: "test 4-level jump".into(),
        from_level: 6,
        to_level: 10,
        hp_gained: 8,
        mp_gained: 4,
        sp_gained: 8,
        stat_deltas: [0; 18],
    };
    let avg = obs.average_per_level();
    assert_eq!(avg.hp, 2);
    assert_eq!(avg.mp, 1);
    let curve = obs.to_curve();
    // Inside the observed range each level emits the average.
    assert_eq!(curve.gain_for(6), StatGain::hp_mp(2, 1));
    assert_eq!(curve.gain_for(9), StatGain::hp_mp(2, 1));
    // Outside the range falls back to default.
    assert_eq!(curve.gain_for(1), StatGain::default());
    assert_eq!(curve.gain_for(50), StatGain::default());
    // Sum across the observed range == hp_gained / mp_gained.
    let total = curve.sum_range(6, 10);
    assert_eq!(total, StatGain::hp_mp(8, 4));
}

#[test]
fn observation_with_zero_levels_gained_is_zero_avg() {
    let obs = LevelUpObservation {
        label: "no-op".into(),
        from_level: 5,
        to_level: 5,
        hp_gained: 0,
        mp_gained: 0,
        sp_gained: 0,
        stat_deltas: [0; 18],
    };
    assert_eq!(obs.levels_gained(), 0);
    assert_eq!(obs.average_per_level(), StatGain::hp_mp(0, 0));
}

#[test]
fn vahn_legacy_observation_matches_capture() {
    let obs = observations::vahn_4_level_jump();
    assert_eq!(obs.from_level, 6);
    assert_eq!(obs.to_level, 10);
    assert_eq!(obs.levels_gained(), 4);
    // Spirit-max gain captured at +0x10E (single-byte +8).
    assert_eq!(obs.sp_gained, 8);
    // First stat delta byte is the wrap-around 0xDD->0x03 = +0x26.
    assert_eq!(obs.stat_deltas[0], 0x26);
    // u16 LE projection: HP_max delta = 0x0126 (rolled past 0xFF).
    let stats = obs.record_stats_u16();
    assert_eq!(stats[0], 0x0126);
    // [+0x120] cap constant unchanged.
    assert_eq!(stats[2], 0);
}

#[test]
fn noa_observation_pins_settled_deltas() {
    let obs = observations::noa_4_level_jump();
    assert_eq!(obs.from_level, 2);
    assert_eq!(obs.to_level, 6);
    assert_eq!(obs.levels_gained(), 4);
    assert_eq!(obs.hp_gained, 32);
    assert_eq!(obs.mp_gained, 6);
    // Noa is a Seru-magic user; level-up grants Spirit at +0x10E.
    assert_eq!(obs.sp_gained, 40);
    let stats = obs.record_stats_u16();
    // HP_max delta at +0x11C.
    assert_eq!(stats[0], 32);
    // MP_max delta at +0x11E.
    assert_eq!(stats[1], 6);
    // [+0x120] per-stat cap constant unchanged.
    assert_eq!(stats[2], 0);
    // Six record-side stat deltas at +0x122..+0x12D.
    assert_eq!(&stats[3..9], &[4, 3, 3, 2, 4, 3]);
}

#[test]
fn gala_observation_pins_settled_deltas() {
    let obs = observations::gala_4_level_jump();
    assert_eq!(obs.from_level, 3);
    assert_eq!(obs.to_level, 7);
    assert_eq!(obs.levels_gained(), 4);
    assert_eq!(obs.hp_gained, 44);
    assert_eq!(obs.mp_gained, 8);
    // Gala uses physical Tactical Arts; level-up grants no SP.
    assert_eq!(obs.sp_gained, 0);
    let stats = obs.record_stats_u16();
    assert_eq!(stats[0], 44);
    assert_eq!(stats[1], 8);
    assert_eq!(stats[2], 0);
    assert_eq!(&stats[3..9], &[2, 4, 4, 2, 2, 2]);
}

#[test]
fn record_stats_u16_lifts_18_byte_window() {
    let mut obs = LevelUpObservation {
        label: "round-trip".into(),
        from_level: 1,
        to_level: 2,
        hp_gained: 0,
        mp_gained: 0,
        sp_gained: 0,
        stat_deltas: [0; 18],
    };
    // Set the second u16 (at +0x11E) to 0x1234 LE.
    obs.stat_deltas[2] = 0x34;
    obs.stat_deltas[3] = 0x12;
    let stats = obs.record_stats_u16();
    assert_eq!(stats[1], 0x1234);
}

#[test]
fn with_observed_curve_installs_per_slot() {
    let obs = LevelUpObservation {
        label: "synthetic".into(),
        from_level: 1,
        to_level: 3,
        hp_gained: 20,
        mp_gained: 4,
        sp_gained: 0,
        stat_deltas: [0; 18],
    };
    let mut t = LevelUpTracker::new()
        .with_xp_table(placeholder_xp_table())
        .with_observed_curve(0, &obs);
    let r = t.grant_xp(0, 400).expect("level up");
    // Each level inside [1, 3) yields avg(20/2) = 10 HP, avg(4/2) = 2 MP.
    assert_eq!(r.new_level, 3);
    assert_eq!(r.hp_gained, 20);
    assert_eq!(r.mp_gained, 4);
}

#[test]
fn with_seru_roster_installs_flat_curve_summed_from_table() {
    use crate::seru_stats::{SeruStatGrant, SeruStatTable};
    let mut table = SeruStatTable::new();
    table.insert(0, SeruStatGrant::hp_mp(8, 3));
    table.insert(1, SeruStatGrant::hp_mp(4, 2));
    // Roster sum: hp 12, mp 5.
    let mut t = LevelUpTracker::new()
        .with_xp_table(placeholder_xp_table())
        .with_seru_roster(0, &table, &[0, 1]);
    let r = t.grant_xp(0, 100).expect("level up");
    assert_eq!(r.hp_gained, 12);
    assert_eq!(r.mp_gained, 5);
}

#[test]
fn level_up_default_flat_still_uses_stat_gains_field() {
    // No curve installed (default = Flat(default)). The legacy
    // `with_stat_gain` path should still drive the result.
    let mut t = LevelUpTracker::new()
        .with_xp_table(placeholder_xp_table())
        .with_stat_gain(StatGain::hp_mp(25, 11));
    let r = t.grant_xp(0, 400).expect("multi-level");
    assert_eq!(r.new_level, 3);
    assert_eq!(r.hp_gained, 50); // 2 levels × 25
    assert_eq!(r.mp_gained, 22); // 2 levels × 11
}

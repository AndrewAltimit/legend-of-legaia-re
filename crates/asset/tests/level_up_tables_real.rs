//! Decode the real level-up XP curve + growth tables out of
//! `extracted/SCUS_942.54` if present. Skips and passes when the executable
//! isn't on disk - same gating pattern as the other disc-dependent tests so CI
//! doesn't need Sony bytes.

use legaia_asset::level_up_tables::{
    GROWTH_CHAR_COUNT, GROWTH_PARAM_LEN, GROWTH_ROW_COUNT, GROWTH_ROW_STRIDE, GROWTH_STAT_COUNT,
    MAX_LEVEL, growth_tables_from_scus, xp_thresholds_from_scus,
};
use legaia_asset::new_game::{StartingChar, StartingParty};
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

#[test]
fn decodes_the_xp_curve_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let scus = std::fs::read(&path).expect("read SCUS");
    let thr = xp_thresholds_from_scus(&scus).expect("parse XP thresholds");

    assert_eq!(thr.len(), MAX_LEVEL - 1, "98 per-level thresholds");

    // The retail curve: DAT_80076AF4 deltas 1,2,3,5,7,... summed and scaled by
    // FUN_801E9504's formula. The first thresholds are byte-validated against a
    // captured retail level-up (the character record's +0x04 "XP to next" field
    // reads 365 at L2->L3, 730 at L3->L4).
    assert_eq!(thr[0], 121, "XP to reach L2");
    assert_eq!(thr[1], 365, "XP to reach L3 (captured)");
    assert_eq!(thr[2], 730, "XP to reach L4 (captured)");
    assert_eq!(thr[3], 1338, "XP to reach L5");
    assert_eq!(thr[4], 2190, "XP to reach L6");

    // Strictly increasing - a monotonic curve, unlike the fabricated sin-LUT
    // slice the engine ships as a placeholder.
    assert!(
        thr.windows(2).all(|w| w[1] > w[0]),
        "thresholds strictly increasing"
    );
}

#[test]
fn decodes_the_growth_tables_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let scus = std::fs::read(&path).expect("read SCUS");
    let g = growth_tables_from_scus(&scus).expect("parse growth tables");

    assert_eq!(g.curves.len(), GROWTH_ROW_COUNT, "3 growth curves");
    assert!(
        g.curves.iter().all(|c| c.len() == GROWTH_ROW_STRIDE),
        "each curve is 0x62 bytes"
    );
    assert_eq!(g.param.len(), GROWTH_PARAM_LEN, "0xB4 param block");

    // Pin a couple of known bytes from the disc (row0 ramps 0x50, 0x52, ...; the
    // high-level tail is the 0x40 plateau byte).
    assert_eq!(g.curves[0][0], 0x50);
    assert_eq!(g.curves[0][1], 0x52);
    assert_eq!(*g.curves[0].last().unwrap(), 0x40);

    // The param block is a per-character record (stride 0x3C) of 8 contiguous
    // 6-byte {start, max, jitter, row} sub-records — NOT a length-prefixed blob
    // (the leading 0x00B4 is Vahn's HP `start` = 180, not a length word). `start`
    // is the base stat: validate it against the new-game starting template.
    let party = StartingParty::from_scus(&scus).expect("starting party");
    let tmpl = |c: &StartingChar| {
        [
            c.hp_max, c.mp_max, c.agl, c.atk, c.udf, c.ldf, c.spd, c.intel,
        ]
    };

    for slot in 0..GROWTH_CHAR_COUNT {
        let cp = g.char_params(slot).expect("char growth params");
        for (s, st) in cp.stats.iter().enumerate() {
            assert!(st.start <= st.max, "slot {slot} stat {s}: start > max");
            assert!(
                (st.row as usize) < GROWTH_ROW_COUNT,
                "slot {slot} stat {s}: row {} out of range",
                st.row
            );
        }
        // HP / MP / AGL `start` match the template for every character.
        let t = tmpl(party.member(slot).expect("template member"));
        assert_eq!(cp.stats[0].start, t[0], "slot {slot} HP start == template");
        assert_eq!(cp.stats[1].start, t[1], "slot {slot} MP start == template");
        assert_eq!(cp.stats[2].start, t[2], "slot {slot} AGL start == template");
    }

    // Gala (slot 2) matches the template on ALL 8 stats — the decisive pin that
    // `start` is the base stat and the sub-records are contiguous 6-byte.
    let gala = g.char_params(2).unwrap();
    let gt = tmpl(party.member(2).unwrap());
    assert_eq!(gala.stats.len(), GROWTH_STAT_COUNT);
    for (s, st) in gala.stats.iter().enumerate() {
        assert_eq!(st.start, gt[s], "Gala stat {s} start == template");
    }

    // The decoded gain arithmetic is faithful to the applier but OVERSHOOTS the
    // captured multi-level deltas (open reconciliation — see
    // docs/subsystems/level-up.md § Stat gains). Pin the overshoot direction so
    // the discrepancy stays visible: Noa HP (slot 1, row 0) summed L2->L6 core
    // gain is well above the observed +32.
    let noa_hp = g.char_params(1).unwrap().stats[0];
    let core_sum: u32 = (2..6)
        .map(|lvl| g.level_gain_core(&noa_hp, lvl).expect("gain core"))
        .sum();
    assert!(
        core_sum > 32,
        "core-sum {core_sum} overshoots observed +32 (open discrepancy)"
    );
}

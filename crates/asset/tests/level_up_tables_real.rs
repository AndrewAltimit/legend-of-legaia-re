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

    // The slots-1/2 threshold-correction divisor table (FUN_801E9504 reads
    // i16s at 0x80070A2C + level*0x28 through the corpus-constant pointer
    // _DAT_8007B81C; Noa subtracts threshold*0x14/divisor, Gala adds it).
    let divs =
        legaia_asset::level_up_tables::xp_correction_divisors_from_scus(&scus).expect("divisors");
    assert_eq!(divs.len(), MAX_LEVEL);
    assert_eq!(divs[0], 0, "level byte is never 0; entry 0 unused");
    assert_eq!(&divs[1..6], &[125, 251, 376, 501, 625], "first divisors");
    assert!(
        divs[1..].iter().all(|&d| d > 0),
        "every reachable level has a positive divisor"
    );
    // The correction magnitude at the captured L2->L3 threshold (365):
    // 365*20/251 = 29 XP - Noa reaches L3 at 336, Gala at 394.
    assert_eq!(
        legaia_asset::level_up_tables::xp_threshold_correction(365, divs[2]),
        29
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
    // 6-byte {start, max, jitter, row} sub-records - NOT a length-prefixed blob
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

    // Gala (slot 2) matches the template on ALL 8 stats - the decisive pin that
    // `start` is the base stat and the sub-records are contiguous 6-byte.
    let gala = g.char_params(2).unwrap();
    let gt = tmpl(party.member(2).unwrap());
    assert_eq!(gala.stats.len(), GROWTH_STAT_COUNT);
    for (s, st) in gala.stats.iter().enumerate() {
        assert_eq!(st.start, gt[s], "Gala stat {s} start == template");
    }

    // Each curve sums to 0x24C0 (= the gain divisor): the divide is a
    // normalizer, so the per-level term accumulates to exactly (max-start) over
    // all 98 levels, landing each stat at `max` at L99.
    for (r, c) in g.curves.iter().enumerate() {
        let sum: u32 = c.iter().map(|&b| b as u32).sum();
        assert_eq!(sum, 0x24C0, "curve row {r} sums to the gain divisor 0x24C0");
    }

    // VALIDATED against a single-level capture (Noa = growth slot 1, L2->L3,
    // the noa_levelup_field_pre / _post library saves): leveling FROM L2 reads
    // curve index level-1 = 1. Each observed record-stat delta falls within the
    // deterministic core +/- the stat's jitter half-range. These small integer
    // deltas are observations (not Sony bytes); the full footprint is the
    // noa_levelup_* scenario set in scripts/scenarios.toml.
    // Applier stat order: HP, MP, then the six record stats at +0x122..+0x12C.
    let noa = g.char_params(1).unwrap();
    let observed = [39u32, 5, 2, 4, 4, 3, 4, 3]; // Noa L2->L3 record-stat deltas
    for (s, &obs) in observed.iter().enumerate() {
        let p = &noa.stats[s];
        let core = g.level_gain_core(p, 2).expect("gain core at L2");
        let lo = core.saturating_sub(p.jitter as u32).max(1);
        let hi = core + p.jitter as u32;
        assert!(
            (lo..=hi).contains(&obs),
            "Noa stat {s} L2->L3: observed +{obs} outside core band [{lo},{hi}] (core {core}, jitter {})",
            p.jitter
        );
    }
}

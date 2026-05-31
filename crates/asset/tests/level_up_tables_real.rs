//! Decode the real level-up XP curve + growth tables out of
//! `extracted/SCUS_942.54` if present. Skips and passes when the executable
//! isn't on disk - same gating pattern as the other disc-dependent tests so CI
//! doesn't need Sony bytes.

use legaia_asset::level_up_tables::{
    GROWTH_PARAM_LEN, GROWTH_ROW_COUNT, GROWTH_ROW_STRIDE, MAX_LEVEL, growth_tables_from_scus,
    xp_thresholds_from_scus,
};
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
    // high-level tail is the 0x40 sentinel; the param block opens with its own
    // 0x00B4 length word).
    assert_eq!(g.curves[0][0], 0x50);
    assert_eq!(g.curves[0][1], 0x52);
    assert_eq!(*g.curves[0].last().unwrap(), 0x40);
    assert_eq!(u16::from_le_bytes([g.param[0], g.param[1]]), 0x00B4);
}

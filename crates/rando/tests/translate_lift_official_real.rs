//! Disc-gated oracle for `translate lift-official`.
//!
//! With only the USA disc (`LEGAIA_DISC_BIN`) it exercises region detection:
//! the boot exe is `SCUS_942.54`, which is not a liftable PAL localization.
//! When a PAL disc is *also* supplied via `LEGAIA_PAL_DISC_BIN` it runs the
//! full lift and asserts the name tables locate, the party names fill, and the
//! dialog corpus pairs at the ~99% the alignment doc claims - all keyed to the
//! USA coordinate space so the pack imports back onto the USA disc.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (no disc committed / CI).

use legaia_rando::disc::DiscPatcher;
use legaia_rando::translation::lift;

fn load(var: &str) -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os(var)?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn lift_official_pairs_and_locates() {
    let Some(usa_bytes) = load("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let usa = DiscPatcher::open(usa_bytes).expect("open USA disc");

    // Region detection on the USA disc: SCUS boot exe is not liftable.
    let exe = lift::boot_exe_name(&usa).expect("read SYSTEM.CNF");
    assert!(
        exe.starts_with("SCUS_942"),
        "expected the USA boot exe, got {exe}"
    );

    let Some(pal_bytes) = load("LEGAIA_PAL_DISC_BIN") else {
        eprintln!("[skip-pal] LEGAIA_PAL_DISC_BIN unset - region detection only");
        return;
    };
    let pal = DiscPatcher::open(pal_bytes).expect("open PAL disc");
    let (pack, rep) = lift::lift_official(&usa, &pal).expect("lift official");

    // Every name table located against its USA-populated id set.
    assert!(!rep.tables.is_empty());
    for t in &rep.tables {
        assert!(t.located, "table {} failed to locate", t.name);
        assert!(
            t.valid_fraction >= 0.75,
            "table {} weak: {}",
            t.name,
            t.valid_fraction
        );
    }
    assert_eq!(
        rep.names_unmapped, 0,
        "all pooled names should map id-for-id"
    );
    assert_eq!(rep.party_filled, rep.party_total);
    assert!(rep.party_total >= 4);

    // Dialog corpus pairs by position at the documented rate.
    assert!(rep.man_total > 10_000, "expected a large MAN corpus");
    let man_pct = rep.man_paired as f64 / rep.man_total as f64;
    let raw_pct = rep.raw_paired as f64 / rep.raw_total.max(1) as f64;
    assert!(man_pct > 0.97, "MAN pairing {man_pct:.3} below 97%");
    assert!(raw_pct > 0.97, "raw pairing {raw_pct:.3} below 97%");

    // The pack is a filled working pack keyed to USA coordinates.
    assert_eq!(pack.language, rep.language);
    let filled = pack.sections.filled();
    assert!(
        filled > 20_000,
        "expected a substantial filled pack, got {filled}"
    );
}

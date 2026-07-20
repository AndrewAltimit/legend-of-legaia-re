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

/// The accent fold the in-browser official-localization transfer applies by
/// default: every PAL accent cell becomes a plain-ASCII glyph the unmodified
/// NTSC font can actually draw, and the folded pack encodes without a single
/// glyph-set error. Counts only - no text is printed or asserted on.
#[test]
fn folded_lift_is_encodable_on_the_ntsc_glyph_set() {
    let (Some(usa_bytes), Some(pal_bytes)) = (load("LEGAIA_DISC_BIN"), load("LEGAIA_PAL_DISC_BIN"))
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN / LEGAIA_PAL_DISC_BIN unset");
        return;
    };
    let usa = DiscPatcher::open(usa_bytes).expect("open USA disc");
    let pal = DiscPatcher::open(pal_bytes).expect("open PAL disc");
    let (mut pack, _) = lift::lift_official(&usa, &pal).expect("lift official");

    // Unfolded, a PAL lift necessarily carries high-glyph bytes.
    let high_before = count_high_escapes(&pack);
    assert!(
        high_before > 0,
        "a PAL lift should carry accented glyph bytes"
    );

    let fold = lift::fold_pack_accents(&mut pack);
    assert!(fold.folded > 0, "nothing folded");
    // The residual is the high cells that are *not* accents: the retail glyph
    // atlas also uses a handful of symbol cells above 0x7E (they occur in the
    // USA disc's own spell names), plus the odd byte in a marginal raw-carrier
    // segment. Those are left verbatim - the USA font draws them - so the fold
    // is not expected to reach zero, only to dominate.
    assert!(
        fold.unmapped * 10 < fold.folded,
        "unexpectedly many unfoldable high cells: {} raw vs {} folded",
        fold.unmapped,
        fold.folded
    );
    assert_eq!(
        count_high_escapes(&pack),
        fold.unmapped,
        "an accent cell survived the fold"
    );

    // Folded text is plain ASCII plus those symbol cells, so it encodes for
    // both target policies.
    use legaia_rando::translation::markup::{self, Target};
    for (_, entries) in pack.sections.iter() {
        for e in entries {
            if e.translation.is_empty() {
                continue;
            }
            let target = if e.key.starts_with("scus:") {
                Target::CString
            } else {
                Target::Segment
            };
            assert!(
                markup::encode(&e.translation, target).is_ok(),
                "folded entry {} does not encode",
                e.key
            );
        }
    }
}

/// Count bare `{xx}` escapes with `xx >= 0x80` that are not 2-byte opcodes -
/// i.e. accented / high glyph cells. Counts only, never text.
fn count_high_escapes(pack: &legaia_rando::translation::LanguagePack) -> usize {
    use legaia_rando::translation::markup;
    let mut n = 0;
    for (_, entries) in pack.sections.iter() {
        for e in entries {
            let (_, stats) = markup::fold_high_glyphs(&e.translation);
            n += stats.folded + stats.unmapped;
        }
    }
    n
}

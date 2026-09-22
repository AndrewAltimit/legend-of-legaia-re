//! Disc-gated oracle for `translate lift-official`.
//!
//! With only the USA disc (`LEGAIA_DISC_BIN`) it exercises region detection
//! and the **identity lift**: a retail USA disc lifted onto itself is the path
//! a fan-patched USA disc takes (bases located from the USA VAs, not pinned),
//! and it must pair everything and reproduce every string verbatim.
//! When a PAL disc is *also* supplied via `LEGAIA_PAL_DISC_BIN` it runs the
//! full lift and asserts the name tables locate, the party names fill, and the
//! dialog corpus pairs at the ~99% the alignment doc claims - all keyed to the
//! USA coordinate space so the pack imports back onto the USA disc - and that
//! the **unpinned** search (the path an unmeasured build such as the Spanish
//! disc takes) lands on the hand-pinned bases of that measured build.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (no disc committed / CI).

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::translation::lift;

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

    // Region detection on the USA disc.
    let exe = lift::boot_exe_name(&usa).expect("read SYSTEM.CNF");
    assert!(
        exe.starts_with("SCUS_942"),
        "expected the USA boot exe, got {exe}"
    );
    assert_eq!(
        lift::source_build_for_exe(&exe).map(|b| b.lang),
        Some("en"),
        "the USA build is a liftable (fan-patchable) Latin source"
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
    use legaia_patcher::translation::markup::{self, Target};
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
fn count_high_escapes(pack: &legaia_patcher::translation::LanguagePack) -> usize {
    use legaia_patcher::translation::markup;
    let mut n = 0;
    for (_, entries) in pack.sections.iter() {
        for e in entries {
            let (_, stats) = markup::fold_high_glyphs(&e.translation);
            n += stats.folded + stats.unmapped;
        }
    }
    n
}

/// The unpinned search - what an unmeasured build (Spain, EU English, any
/// fan-patched disc) gets - must land exactly on the hand-pinned bases of a
/// measured PAL build, tables and party template alike.
#[test]
fn unpinned_search_recovers_the_pinned_pal_bases() {
    let (Some(usa_bytes), Some(pal_bytes)) = (load("LEGAIA_DISC_BIN"), load("LEGAIA_PAL_DISC_BIN"))
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN / LEGAIA_PAL_DISC_BIN unset");
        return;
    };
    let usa = DiscPatcher::open(usa_bytes).expect("open USA disc");
    let pal = DiscPatcher::open(pal_bytes).expect("open PAL disc");
    let exe = lift::boot_exe_name(&pal).expect("read SYSTEM.CNF");
    let Some((pinned_tables, pinned_party)) = lift::pinned_bases_for_exe(&exe) else {
        eprintln!("[skip] {exe} is not a hand-pinned build - nothing to vouch against");
        return;
    };
    let usa_exe = usa.read_named_file("SCUS_942.54").expect("USA exe");
    let pal_exe = pal.read_named_file(&exe).expect("PAL exe");

    let found = lift::locate_unpinned(&usa_exe, &pal_exe);
    assert_eq!(found.tables.len(), pinned_tables.len());
    for ((name, hit), pinned) in found.tables.iter().zip(pinned_tables) {
        assert_eq!(
            *hit,
            Some(pinned),
            "{exe} table {name}: unpinned search landed on {hit:x?}, pinned 0x{pinned:08x}"
        );
    }
    assert_eq!(
        found.party,
        Some(pinned_party),
        "{exe} party template: fingerprint search landed on {:x?}, pinned 0x{pinned_party:08x}",
        found.party
    );
}

/// A retail USA disc lifted onto itself takes the located (unpinned) path and
/// must be the identity: every table found at its USA VA, every name and both
/// dialog domains paired 100%, every `translation` equal to its `source`. This
/// is the path a fan-patched USA disc takes, minus the patch.
#[test]
fn usa_disc_lifts_onto_itself_as_the_identity() {
    let Some(usa_bytes) = load("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let usa = DiscPatcher::open(usa_bytes.clone()).expect("open USA disc");
    let again = DiscPatcher::open(usa_bytes).expect("open USA disc again");
    let (pack, rep) = lift::lift_official(&usa, &again).expect("identity lift");

    assert_eq!(rep.language, "en");
    for t in &rep.tables {
        assert!(
            t.located,
            "table {} not located on the USA exe itself",
            t.name
        );
        assert!(
            (t.valid_fraction - 1.0).abs() < 1e-9,
            "table {} valid fraction {}",
            t.name,
            t.valid_fraction
        );
    }
    assert_eq!(rep.names_unmapped, 0, "every USA string maps to itself");
    assert!(
        rep.party_fingerprint_ok,
        "party template fingerprint on itself"
    );
    assert_eq!(rep.party_filled, rep.party_total);
    assert_eq!(
        rep.man_paired, rep.man_total,
        "MAN dialog pairs 100% with itself"
    );
    assert_eq!(
        rep.raw_paired, rep.raw_total,
        "raw dialog pairs 100% with itself"
    );

    // Structural pairing on the same disc is the identity: nothing shifts.
    assert_eq!(
        rep.man_pairing.shifted, 0,
        "MAN lines shifted against itself"
    );
    assert_eq!(
        rep.raw_pairing.shifted, 0,
        "raw lines shifted against itself"
    );
    assert_eq!(
        rep.ui_paired, rep.ui_total,
        "overlay pools pair with themselves"
    );
    assert_eq!(
        rep.system_paired, rep.system_total,
        "SCUS pools pair with themselves"
    );
    assert_eq!(
        rep.cells_paired, rep.cells_total,
        "place-name cells pair with themselves"
    );
    assert!(rep.ui_total > 100 && rep.cells_total == 16 && rep.system_total >= 6);

    let mut checked = 0usize;
    for (_, entries) in pack.sections.iter() {
        for e in entries {
            // The lift drops trailing pad spaces (they draw nothing and cost
            // budget), so the identity holds up to that normalization.
            assert_eq!(
                e.translation,
                e.source.trim_end_matches(' '),
                "{}: identity lift changed the text",
                e.key
            );
            checked += 1;
        }
    }
    assert!(
        checked > 1000,
        "identity lift covered only {checked} entries"
    );
}

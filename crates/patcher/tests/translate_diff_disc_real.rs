//! Disc-gated self-diff oracle for the cross-region alignment tool
//! (`translate diff-disc`). Diffing a disc against itself must report perfect
//! structural parity and positional alignment: every PROT entry LBA-aligns,
//! every dialog segment pairs by order, every paired line fits its own budget,
//! and (for a Latin NTSC build) no high glyph bytes appear.
//!
//! Uses only the one disc `LEGAIA_DISC_BIN` points at, so it skips+passes when
//! that env var is unset (no PAL disc is required or committed).

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::translation::diff;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn self_diff_is_perfectly_aligned() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let a = DiscPatcher::open(disc.clone()).expect("open disc a");
    let b = DiscPatcher::open(disc).expect("open disc b");
    let rep = diff::diff_disc(&a, &b);

    // Structural parity: identical images LBA-align at every index.
    assert_eq!(rep.entries_a, rep.entries_b);
    assert_eq!(rep.entries_lba_aligned, rep.entries_a.min(rep.entries_b));

    // Non-vacuous: the NTSC disc has a real dialog corpus.
    assert!(
        rep.man.total_segs_a > 1000,
        "expected a substantial MAN dialog corpus, got {}",
        rep.man.total_segs_a
    );

    for d in [&rep.man, &rep.raw] {
        // Same disc twice => same segment set, every entry count-matches, every
        // line pairs by order and fits its own (identical) budget.
        assert_eq!(d.total_segs_a, d.total_segs_b);
        assert_eq!(d.entries_a, d.entries_both);
        assert_eq!(d.count_matched_entries, d.entries_both);
        assert_eq!(d.order_delta, 0);
        assert_eq!(d.order_pairable, d.total_segs_a);
        assert_eq!(d.order_overflow, 0);
        assert_eq!(d.order_fit, d.order_pairable);
        assert_eq!(d.overflow, 0);
        assert_eq!(d.fit, d.paired_segments);
    }

    // A Latin NTSC build uses no *accented* glyph tiles in its dialog: the
    // prose-gated census has only a negligible noise floor (a few coincidental
    // high bytes in prose-shaped binary runs), never a dominant accent - which
    // on a PAL build reaches the hundreds-to-thousands.
    let max_high = rep.high_byte_census.values().copied().max().unwrap_or(0);
    assert!(
        max_high < 50,
        "unexpected dominant high glyph byte in an NTSC self-diff: {:?}",
        rep.high_byte_census
    );
}

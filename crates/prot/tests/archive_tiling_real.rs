//! Disc-gated proof that PROT entry extents **partition `PROT.DAT`**.
//!
//! This is the assertion that makes the entry-size reading self-defending.
//! An entry's size is the sector gap to the next entry, and the resulting
//! extents tile the archive exactly: monotonic starts, no gaps, no overlaps,
//! and a total equal to the contiguous span from the first entry's start LBA
//! to the file's last sector. The historical `toc[p+5] - toc[p+3] + 4`
//! expression cannot satisfy this - it sums to about two and a half times the
//! archive, and no partition of a file exceeds the file.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or `extracted/PROT.DAT` is
//! missing.

use std::path::PathBuf;

use legaia_prot::archive::{Archive, SECTOR};
use legaia_prot::tiling;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

#[test]
fn entries_tile_the_archive_exactly() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let arch = Archive::open(&prot).expect("PROT.DAT parses");
    let file_len = arch.file_len();

    let t = tiling::check(&arch.entries);
    assert!(
        t.entries > 1000,
        "sanity: the retail TOC resolves a full entry set, got {}",
        t.entries
    );
    assert!(
        t.non_monotonic.is_empty(),
        "start LBAs must ascend; back-edges at {:?}",
        t.non_monotonic
    );
    assert!(t.gaps.is_empty(), "unclaimed sectors between {:?}", t.gaps);
    assert!(t.overlaps.is_empty(), "entries overlap at {:?}", t.overlaps);
    assert_eq!(
        t.total_sectors,
        (t.last_end_lba - t.first_start_lba) as u64,
        "the sizes must sum to the span they cover"
    );

    // ...and that span reaches the end of the image. The region below
    // `first_start_lba` is the TOC plus the boot-resident system-UI block the
    // extraction index space does not cover (raw TOC entries 0 and 1).
    assert!(
        t.covers_to_end_of(file_len),
        "last entry ends at LBA {} but the archive is {} sectors",
        t.last_end_lba,
        file_len / SECTOR as u64
    );
    assert!(t.is_exact(), "{t:?}");

    eprintln!(
        "[tiling] {} entries, LBA {}..{} = {} sectors, no gaps/overlaps",
        t.entries, t.first_start_lba, t.last_end_lba, t.total_sectors
    );
}

/// Non-vacuity, and the arithmetic behind the correction: the historical
/// `toc[p+5] - toc[p+3] + 4` expression is `footprint(p+1) + footprint(p+2) +
/// 4`, so its total dwarfs the archive it claims to describe.
#[test]
fn the_declared_span_measures_the_two_following_entries() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let arch = Archive::open(&prot).expect("PROT.DAT parses");
    let file_sectors = arch.file_len() / SECTOR as u64;

    // The identity holds wherever the expression did not underflow against
    // the zeroed TOC tail (where the parser substitutes the real size).
    let mut identity_holds = 0usize;
    for w in arch.entries.windows(3) {
        let want = w[1].size_sectors + w[2].size_sectors + 4;
        if w[0].declared_span_sectors == w[0].size_sectors && want != w[0].size_sectors {
            continue; // tail row: the parser fell back to the real size
        }
        assert_eq!(
            w[0].declared_span_sectors, want,
            "entry {}: declared span is footprint(p+1) + footprint(p+2) + 4",
            w[0].index
        );
        identity_holds += 1;
    }
    assert!(
        identity_holds > 1000,
        "identity checked on only {identity_holds} entries"
    );

    let declared_total: u64 = arch
        .entries
        .iter()
        .map(|e| e.declared_span_sectors as u64)
        .sum();
    let real_total: u64 = arch.entries.iter().map(|e| e.size_sectors as u64).sum();
    assert!(
        declared_total > 2 * file_sectors,
        "declared spans total {declared_total} sectors vs a {file_sectors}-sector archive"
    );
    assert!(real_total <= file_sectors);
    eprintln!(
        "[declared] {declared_total} sectors ({:.2}x the {file_sectors}-sector archive) \
         vs {real_total} real",
        declared_total as f64 / file_sectors as f64
    );
}

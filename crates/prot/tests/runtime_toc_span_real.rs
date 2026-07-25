//! Disc-gated oracle for `FUN_8003E68C` (`legaia_prot::runtime_toc`).
//!
//! The routine returns `TABLE[i+3] - TABLE[i+2]` over the in-RAM PROT TOC,
//! which is the entry's size in sectors. `Archive` sizes every entry from
//! this same routine, so the real TOC is a direct oracle for the port - and
//! for the parser having exactly one implementation of the arithmetic.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or `extracted/PROT.DAT`
//! is missing.

use std::path::PathBuf;

use legaia_prot::archive::Archive;
use legaia_prot::runtime_toc::entry_sector_span_from_archive_toc;

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
fn span_matches_next_start_minus_start_for_every_real_entry() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let arch = Archive::open(&prot).expect("PROT.DAT parses");
    assert!(
        arch.entries.len() > 1000,
        "sanity: the retail TOC resolves a full entry set, got {}",
        arch.entries.len()
    );

    let mut checked = 0usize;
    let mut disagreed_with_declared = 0usize;
    for e in &arch.entries {
        let p = e.index as usize;
        let span = entry_sector_span_from_archive_toc(&arch.toc, p)
            .unwrap_or_else(|| panic!("entry {p} in range"));

        // The two words the routine subtracts are exactly the entry's
        // start LBA and the next row's start LBA.
        assert_eq!(
            arch.toc[p + 2],
            e.start_lba,
            "entry {p}: toc[p+2] is the start LBA"
        );
        assert_eq!(
            span,
            arch.toc[p + 3].wrapping_sub(e.start_lba),
            "entry {p}: span is next_start - start"
        );

        // The entry's size IS this routine's result - always, not just
        // where some other formula happens to fall short.
        assert_eq!(
            span, e.size_sectors,
            "entry {p}: the entry size is the FUN_8003E68C span"
        );
        if e.declared_span_sectors != e.size_sectors {
            disagreed_with_declared += 1;
        }
        checked += 1;
    }

    // Non-vacuity: if the historical `toc[p+5] - toc[p+3] + 4` expression
    // agreed with the span everywhere, the assertion above would be
    // measuring nothing.
    assert!(checked > 1000, "checked {checked} entries");
    assert!(
        disagreed_with_declared > 100,
        "only {disagreed_with_declared} entries where the declared span differs \
         from the real size - the oracle would be near-vacuous"
    );
}

#[test]
fn ram_index_space_is_the_extraction_index_plus_two() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let arch = Archive::open(&prot).expect("PROT.DAT parses");

    // Reconstruct the RAM word array the boot loader installs: PROT.DAT
    // from byte 0, i.e. the archive header's two words in front of
    // `Archive::toc`. Then the RAM index for extraction entry `p` is
    // `p + 2`, and both routes must agree.
    let mut ram: Vec<u32> = vec![arch.header.file_num, arch.header.header_sectors];
    ram.extend_from_slice(&arch.toc);

    for e in arch.entries.iter().take(64) {
        let p = e.index as usize;
        assert_eq!(
            legaia_prot::runtime_toc::entry_sector_span(&ram, p + 2),
            entry_sector_span_from_archive_toc(&arch.toc, p),
            "entry {p}: RAM index p+2 and archive index p resolve the same span"
        );
    }
}

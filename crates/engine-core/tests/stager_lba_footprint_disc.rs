//! Disc-gated: [`ProtIndex::entry_bytes_lba_footprint`] trims a summon-stager
//! overlay to the TOC-gap window the boot loader actually streams.
//!
//! The per-summon move-VM stagers (PROT 0903.., the high-summon block) have
//! extraction `.BIN`s that **over-read** into the following entry. Parsing the
//! raw on-disc footprint makes the spawn-site pointer table in the over-read
//! tail dereference unrelated bytes; parsing the LBA-gap window
//! (`unique_content_len`) recovers the real scene-graph. This pins that the
//! engine accessor produces the same trimmed view the disc-gated asset test
//! (`summon_overlay_real`) gets by reading the full footprint and truncating.

use std::path::PathBuf;

use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::scene::ProtIndex;

fn extracted_root() -> Option<PathBuf> {
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").is_file() {
            return Some(d);
        }
    }
    None
}

#[test]
fn lba_footprint_trims_summon_stager_to_unique_content() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");

    // Gimard's summon stager (extraction PROT 0903) and the high-summon
    // Vera slot (0905) both over-read; both must trim to a window that still
    // contains their record table and parse into a populated scene-graph.
    for idx in [903u32, 905] {
        let extended = index
            .entry_bytes_extended(idx)
            .expect("read extended footprint");
        let trimmed = index
            .entry_bytes_lba_footprint(idx)
            .expect("read LBA footprint");

        // The trim matches the asset-side `unique_content_len` math exactly.
        let start = index.entry_start_lba_retail(idx as u16).expect("start lba");
        let next = index
            .entry_start_lba_retail(idx as u16 + 1)
            .expect("next start lba");
        let expected = summon_overlay::unique_content_len(extended.len(), start, next);
        assert_eq!(
            trimmed.len(),
            expected,
            "PROT {idx}: footprint must equal unique_content_len"
        );
        assert!(
            trimmed.len() <= extended.len(),
            "PROT {idx}: trimmed window cannot exceed the raw footprint"
        );

        // The trimmed window parses into the stager's move-VM scene-graph
        // with every recovered record sitting inside the window (the whole
        // point of the trim - the over-read tail's pointers are dropped).
        let overlay = summon_overlay::parse(&trimmed, SUMMON_OVERLAY_LINK_BASE);
        assert!(
            overlay.spawn_sites >= 1,
            "PROT {idx}: expected FUN_80021B04 spawn sites (got {})",
            overlay.spawn_sites
        );
        for p in &overlay.parts {
            assert!(
                p.record_off + 4 <= trimmed.len(),
                "PROT {idx}: record {:#x} must sit inside the trimmed window",
                p.record_off
            );
        }
    }
}

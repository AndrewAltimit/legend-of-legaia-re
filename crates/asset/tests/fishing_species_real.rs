//! Disc-gated reproducibility for the fishing-minigame per-species table.
//!
//! Re-extract the fishing overlay (PROT 0972) from the user's `PROT.DAT`, decode
//! the species table out of it, and assert the structural invariants that pin
//! the layout (no Sony bytes asserted - the literal stat values + fish names
//! stay on the user's disc):
//!
//! * exactly [`SPECIES_COUNT`] records before the table ends;
//! * every record's `+0x00` is an in-overlay name pointer resolving to a
//!   non-empty printable-ASCII string;
//! * every score / pull factor is positive (the kernel divides by them);
//! * the score formula is well-formed (monotone in strength, positive award).
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` are absent.

use std::path::PathBuf;

use legaia_asset::fishing_species::{self as fish, SPECIES_COUNT};
use legaia_asset::static_overlay;
use legaia_prot::archive::Archive;

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

fn fishing_overlay() -> Option<Vec<u8>> {
    let prot = prot_dat()?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let rec = static_overlay::overlay_map()
        .by_prot_index(fish::FISHING_OVERLAY_PROT_INDEX as u32)
        .expect("fishing overlay in static map");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .expect("PROT entry present");
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).expect("read entry");
    Some(static_overlay::as_loaded(&raw, rec).expect("as-loaded form"))
}

#[test]
fn species_table_reproduces_and_is_well_formed() {
    let Some(overlay) = fishing_overlay() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let species = fish::parse(&overlay).expect("species table parses");
    assert_eq!(species.len(), SPECIES_COUNT, "species count");

    // Record 10 is past the table: its +0x00 must NOT be an in-overlay name.
    let one_more = fish::parse_at(&overlay, fish::SPECIES_TABLE_FILE_OFFSET, SPECIES_COUNT + 1)
        .expect("over-read parses");
    assert!(
        one_more[SPECIES_COUNT].name(&overlay).is_none(),
        "record {SPECIES_COUNT} should be outside the table (no in-overlay name)"
    );

    for s in &species {
        let name = s
            .name(&overlay)
            .unwrap_or_else(|| panic!("species {} name pointer resolves", s.index));
        assert!(
            (1..=16).contains(&name.len()),
            "species {} name length sane",
            s.index
        );
        assert!(s.score_value > 0, "species {} score base positive", s.index);
        assert!(
            s.pull_factor > 0,
            "species {} pull factor positive",
            s.index
        );

        // Score formula: positive and strictly monotone in strength.
        assert!(
            s.score_for(0) > 0,
            "species {} base award positive",
            s.index
        );
        assert!(
            s.score_for(1000) > s.score_for(0),
            "species {} score rises with strength",
            s.index
        );
    }
}

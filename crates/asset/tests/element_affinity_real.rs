//! Disc-gated: the battle element-affinity matrix (`0x801F53E8`) + per-character
//! element table (`0x801F5480`) parse out of the real PROT 0898 (battle-action
//! overlay) entry at the pinned offsets.
//!
//! Pins, on real disc bytes, the static tables `FUN_801dd864` reads when it
//! scales the attacker roll by element affinity: the matrix's same-element
//! diagonal (96), the reciprocal opposite-element bonus pairs (104), and the
//! per-character element assignment (Vahn fire / Noa wind / Gala thunder / Terra
//! wind), cross-checked against the curated gamedata character elements. Skips
//! and passes when `LEGAIA_DISC_BIN` / `extracted/` is absent (the workspace
//! disc-gated convention).

use std::path::PathBuf;

use legaia_asset::element_affinity::{self, ELEMENT_COUNT, Element};
use legaia_prot::archive::Archive;

fn extracted_prot() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if prot.is_file() {
            return Some(prot);
        }
    }
    None
}

#[test]
fn element_affinity_tables_parse_with_pinned_values() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(element_affinity::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");

    let aff = element_affinity::parse(&bytes).expect("element-affinity tables parse");

    // Same-element diagonal is a slight self-resist (96) for every real
    // element; the neutral element (7) is exempt — its whole row/column is 100.
    for e in 0..7u8 {
        assert_eq!(
            aff.affinity_pct(e, e),
            Some(96),
            "diagonal element {e} should be 96"
        );
    }
    assert_eq!(aff.affinity_pct(7, 7), Some(100), "neutral self = 100");

    // Reciprocal opposite-element pairs each carry the 104% bonus, both ways.
    for &(a, b) in &[(0u8, 3u8), (1, 2), (5, 6)] {
        assert_eq!(aff.affinity_pct(a, b), Some(104), "atk {a} vs def {b}");
        assert_eq!(aff.affinity_pct(b, a), Some(104), "atk {b} vs def {a}");
    }

    // The neutral element (7) neither gives nor takes affinity: its row and
    // column are all 100.
    for e in 0..ELEMENT_COUNT as u8 {
        if e != 7 {
            assert_eq!(aff.affinity_pct(7, e), Some(100), "neutral attacks {e}");
            assert_eq!(aff.affinity_pct(e, 7), Some(100), "{e} attacks neutral");
        }
    }

    // Per-character element assignment (1-based char id).
    assert_eq!(
        aff.character_element(1),
        Some(Element::Fire as u8),
        "Vahn = fire"
    );
    assert_eq!(
        aff.character_element(2),
        Some(Element::Wind as u8),
        "Noa = wind"
    );
    assert_eq!(
        aff.character_element(3),
        Some(Element::Thunder as u8),
        "Gala = thunder"
    );
    assert_eq!(
        aff.character_element(4),
        Some(Element::Wind as u8),
        "Terra = wind"
    );
}

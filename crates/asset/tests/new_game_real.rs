//! Decode the real new-game starting-party template out of
//! `extracted/SCUS_942.54` if present. Skips and passes when the executable
//! isn't on disk - same gating pattern as the other disc-dependent tests so CI
//! doesn't need Sony bytes.

use legaia_asset::new_game::{PARTY_RECORDS, StartingParty};
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

#[test]
fn decodes_the_starting_party_template_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let party = StartingParty::from_scus(&bytes).expect("parse starting-party template");

    assert_eq!(party.len(), PARTY_RECORDS, "roster slot count");

    // Slot 0 is Vahn - the only member who has actually joined at a true New
    // Game. His record is byte-validated against an early `town01` save state
    // (HP 180 / MP 20 / AGL 100 / ATK 24 / uDEF 16 / lDEF 12 / SPD 19 / INT 9).
    let vahn = party.member(0).expect("Vahn slot");
    assert_eq!(vahn.name, "Vahn");
    assert_eq!(vahn.hp_max, 180);
    assert_eq!(vahn.mp_max, 20);
    assert_eq!(vahn.agl, 100);
    assert_eq!(vahn.atk, 24);
    assert_eq!(vahn.udf, 16);
    assert_eq!(vahn.ldf, 12);
    assert_eq!(vahn.spd, 19);
    assert_eq!(vahn.intel, 9);

    // The remaining roster templates are present in order.
    assert_eq!(party.member(1).map(|m| m.name.as_str()), Some("Noa"));
    assert_eq!(party.member(2).map(|m| m.name.as_str()), Some("Gala"));
    assert_eq!(party.member(3).map(|m| m.name.as_str()), Some("Terra"));
}

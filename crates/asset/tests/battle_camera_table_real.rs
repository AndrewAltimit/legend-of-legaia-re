//! Disc-gated: the battle-camera per-character height table (`0x801F4D2C`)
//! parses out of the real PROT 0898 (battle-action overlay) entry at the
//! pinned offset.
//!
//! This is the `TR.y` component `FUN_801D5854` case `0` (the command-submenu
//! close-up) reads for whichever character is acting. Vahn's entry is the one
//! value a solo-Vahn camera trace can observe (`0x480`), so it doubles as the
//! offset check: if the base or stride were wrong, index 0 would not land on
//! the traced height. Skips and passes when `LEGAIA_DISC_BIN` / `extracted/`
//! is absent (the workspace disc-gated convention).

use std::path::PathBuf;

use legaia_asset::battle_camera_table::{self, CAMERA_HEIGHT_LEN};
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
fn battle_camera_height_table_parses_with_pinned_values() {
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
        .get(battle_camera_table::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");

    let table = battle_camera_table::parse(&bytes).expect("camera-height table parses");

    // Vahn (char id 1) is the traced value: the submenu close-up's measured
    // TR.y of 1152 on the solo-Vahn tutorial fight.
    assert_eq!(
        table.height_for_char_id(1),
        Some(0x480),
        "Vahn's camera height must match the traced submenu framing"
    );
    // The remaining three are per-model heights the solo trace cannot see.
    assert_eq!(table.height_for_char_id(2), Some(0x3C0), "Noa");
    assert_eq!(table.height_for_char_id(3), Some(0x580), "Gala");
    assert_eq!(table.height_for_char_id(4), Some(0x200), "fourth character");
    assert_eq!(
        table.height_for_char_id(CAMERA_HEIGHT_LEN as u8 + 1),
        None,
        "table is exactly one entry per playable character"
    );

    // Every entry is a plausible eye height, not a pointer or a sentinel -
    // the structural check that the extent stops before the pointer table
    // that follows it.
    for (i, &h) in table.heights().iter().enumerate() {
        assert!(
            (0x100..0x1000).contains(&h),
            "entry {i} = {h:#x} is outside the camera-height range"
        );
    }
}

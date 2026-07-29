//! Disc-gated pin of the outdoor depth-cue stage table
//! (`DAT_80078C1C`) that selects the battle ground grid's far colour.
//!
//! The capture side of the same claim lives in
//! `scripts/pcsx-redux/autorun_grid_far_colour.lua`: exec breakpoints on
//! the grid emitter (`func_0x801d02c0`) read GTE control regs 21-23 at the
//! draw, and the settled values are exactly
//! [`grid_far_colour`] of the neutral base through the arm this table
//! picks - `(0x40, 0x40, 0x40)` off-table, `(0xFE, 0xFE, 0xFE)` on-table
//! (observed on stage id `0x55`, the map01 overworld dome).
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or the extracted
//! `SCUS_942.54` isn't on disk (`extracted/`, `../../extracted/`, or
//! `LEGAIA_EXTRACTED_DIR`).

use legaia_engine_vm::battle_ground_grid::{
    GRID_FAR_INDOOR, GRID_FAR_OUTDOOR, OutdoorCueTable, grid_far_colour,
};
use std::path::PathBuf;

fn scus_bytes() -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let mut roots = vec![PathBuf::from("extracted"), PathBuf::from("../../extracted")];
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        roots.insert(0, PathBuf::from(d));
    }
    let path = roots
        .into_iter()
        .map(|r| r.join("SCUS_942.54"))
        .find(|p| p.is_file())?;
    std::fs::read(path).ok()
}

/// The retail table: 13 outdoor stage ids, and the three capture-pinned
/// battles classify onto the two far colours the probe measured.
#[test]
fn outdoor_cue_table_matches_the_retail_scus_and_the_captures() {
    let Some(scus) = scus_bytes() else {
        eprintln!("[skip] extracted SCUS_942.54 not found");
        return;
    };
    let t = OutdoorCueTable::from_scus(&scus).expect("outdoor cue table parses");
    // 13 ids: the nine kingdom-overworld dome variants plus retona, deene,
    // kor5 and rikuroa. A disc invariant, not a project-state count.
    assert_eq!(t.ids().len(), 13, "ids = {:?}", t.ids());
    for band in [
        [0x55u16, 0x56, 0x57],
        [0xF4, 0xF5, 0xF6],
        [0x187, 0x188, 0x189],
    ] {
        for id in band {
            assert!(t.contains_runtime_id(id), "overworld id {id:#x} missing");
        }
    }
    // The three capture-pinned battles:
    //   vs Gobu Gobu     stage id 0x55 -> outdoor arm, FC (0xFE,0xFE,0xFE)
    //   Queen Bee ambush stage id 0x15 -> indoor arm,  FC (0x40,0x40,0x40)
    //   scripted Gimard  stage id 0x0C -> indoor arm,  FC (0x40,0x40,0x40)
    assert_eq!(
        grid_far_colour([0x80; 3], t.contains_runtime_id(0x55)),
        GRID_FAR_OUTDOOR
    );
    for indoor in [0x15u16, 0x0C] {
        assert_eq!(
            grid_far_colour([0x80; 3], t.contains_runtime_id(indoor)),
            GRID_FAR_INDOOR
        );
    }
    // Every id maps into the scene_tmd_stream band under the +3 PROT
    // offset the battle_backdrop id space uses.
    for &id in t.ids() {
        assert!(t.contains_prot_index(u32::from(id) + 3));
    }
}

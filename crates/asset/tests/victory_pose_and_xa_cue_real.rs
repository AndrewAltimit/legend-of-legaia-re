//! Disc-gated: the two static `SCUS_942.54` tables the battle-end sequence
//! and the melee sound sites read parse off the extracted executable with
//! the shape the disassembly gives them. Skip-passes when `extracted/` is
//! absent (CLAUDE.md disc-gated convention).

use std::path::PathBuf;

use legaia_asset::victory_pose::{
    VICTORY_POSE_ID_MAX, VICTORY_POSE_ID_MIN, victory_pose_table_from_scus,
};
use legaia_asset::xa_cue_table::{XA_CUE_DURATION_ENTRIES, xa_cue_durations_from_scus};

fn scus() -> Option<Vec<u8>> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let p = PathBuf::from(c).join("SCUS_942.54");
        if p.exists() {
            return std::fs::read(p).ok();
        }
    }
    None
}

#[test]
fn victory_pose_rows_are_permutations_of_the_win_pose_band() {
    let Some(scus) = scus() else {
        eprintln!("skip: extracted/SCUS_942.54 absent");
        return;
    };
    let table = victory_pose_table_from_scus(&scus).expect("table parses");
    for row in &table {
        // Each row is one pair per tier over six distinct win-pose entries.
        let mut seen = [false; 8];
        for &id in row {
            assert!((VICTORY_POSE_ID_MIN..=VICTORY_POSE_ID_MAX).contains(&id));
            let i = usize::from(id - VICTORY_POSE_ID_MIN);
            assert!(!seen[i], "a row names each pose once: {row:?}");
            seen[i] = true;
        }
        // The weak pair is the last two entries of the archive in every row
        // (the near-static breathing streams retail loops).
        assert_eq!(
            &row[4..6],
            &[0x15, 0x16],
            "weak pair is 0x15 / 0x16: {row:?}"
        );
    }
}

#[test]
fn the_melee_cue_duration_entry_covers_the_whole_sting() {
    let Some(scus) = scus() else {
        eprintln!("skip: extracted/SCUS_942.54 absent");
        return;
    };
    let t = xa_cue_durations_from_scus(&scus).expect("table parses");
    assert_eq!(t.len(), XA_CUE_DURATION_ENTRIES);
    // `0x10C` -> entry 0x0C -> `(raw * 60 + 99) / 100` = 224 sectors, the
    // read span the sound funnel's voice leg requests (docs/subsystems/audio.md).
    let dur = (u32::from(t[0x0C]) * 60).div_ceil(100);
    assert_eq!(dur, 224);
    // Every clip-slot band has a populated duration; the tail is unused.
    assert!(t[..0x37].iter().all(|&d| d > 0));
    assert!(t[0x37..].iter().all(|&d| d == 0));
}

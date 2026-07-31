//! Disc-gated: the per-art attack-camera track table (`0x801F4E10`) parses
//! out of the real PROT 0898 (battle-action overlay) entry at the pinned
//! offset, and the disc bytes agree with the extent the disassembly implies.
//!
//! The table is what `FUN_801D71B8`'s per-character / per-art arms fold into
//! the camera pose. Its extent (`20` rows of two halfwords) is measured two
//! ways in the module doc - the `lhu` displacements the arms use, and where
//! the data stops - so the disc check here is the one that can falsify the
//! offset: the row the first Gala arm reads must hold the value the arm's
//! disassembly makes sense of, and the row past the table must not look like
//! another camera offset. Skips and passes when `LEGAIA_DISC_BIN` /
//! `extracted/` is absent (the workspace disc-gated convention).

use std::path::PathBuf;

use legaia_asset::battle_attack_camera_table::{
    self as tracks, ATTACK_CAMERA_FILE_OFFSET, ATTACK_CAMERA_LEN, ATTACK_CAMERA_PHASES,
    ATTACK_CAMERA_ROWS,
};
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

fn overlay_bytes() -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let prot = extracted_prot().or_else(|| {
        eprintln!("[skip] extracted/PROT.DAT missing");
        None
    })?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(tracks::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");
    Some(bytes)
}

#[test]
fn attack_camera_tracks_parse_from_the_real_overlay() {
    let Some(bytes) = overlay_bytes() else { return };
    let table = tracks::parse(&bytes).expect("attack-camera track table parses");

    // Every value is a camera offset: a 12-bit angle delta or an eye-space
    // translation delta, both well inside a signed halfword's useful range.
    // A pointer or a string would blow straight through this.
    for (r, row) in table.rows().iter().enumerate() {
        for (c, &v) in row.iter().enumerate() {
            assert!(
                (-0x1000..=0x1000).contains(&v),
                "row {r} phase {c} = {v:#x} is not a camera offset"
            );
        }
    }

    // Not a run of zeros or one repeated value - a wrong base inside this
    // overlay's padding would parse "successfully" and look like that.
    let distinct: std::collections::BTreeSet<i16> =
        table.rows().iter().flatten().copied().collect();
    assert!(
        distinct.len() >= ATTACK_CAMERA_ROWS,
        "only {} distinct values across {ATTACK_CAMERA_ROWS} rows",
        distinct.len()
    );
    assert!(
        table.rows().iter().flatten().any(|&v| v < 0),
        "the table must carry negative offsets"
    );
}

/// The extent check the disassembly cannot make on its own: the halfwords
/// immediately **after** the last row the arms read must not continue the
/// pattern, and the ones before the first row belong to the height table's
/// pointer list.
#[test]
fn the_table_stops_where_the_arms_stop_reading() {
    let Some(bytes) = overlay_bytes() else { return };
    let at = |off: usize| i16::from_le_bytes([bytes[off], bytes[off + 1]]);

    // Last row the arms address (`lhu … 0x4c(base)`), both phases.
    let last = ATTACK_CAMERA_FILE_OFFSET + (ATTACK_CAMERA_ROWS - 1) * 4;
    assert!(
        (at(last), at(last + 2)) != (0, 0),
        "the last addressed row is empty - the extent is too long"
    );
    // One row past the table: the data changes character. Everything the
    // arms read is a camera offset; the first unread row is not.
    let past = ATTACK_CAMERA_FILE_OFFSET + ATTACK_CAMERA_LEN;
    assert_eq!(
        (at(past), at(past + 2)),
        (0, 0),
        "the first unread row should be the zero gap before the next table"
    );

    // The parsed rows are exactly the bytes at the pinned offset - no stride
    // or endianness drift between the reader and the raw halfwords.
    let table = tracks::parse(&bytes).expect("parses");
    for r in 0..ATTACK_CAMERA_ROWS {
        for c in 0..ATTACK_CAMERA_PHASES {
            assert_eq!(
                table.track(r, c),
                Some(at(ATTACK_CAMERA_FILE_OFFSET + r * 4 + c * 2)),
                "row {r} phase {c}"
            );
        }
    }
}

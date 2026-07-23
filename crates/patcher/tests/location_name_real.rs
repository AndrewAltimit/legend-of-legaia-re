//! Disc-gated tests for the location/landmark rename
//! (`legaia_patcher::location_name` + `apply::rename_locations`): the SCUS
//! landmark table decodes the known 16 names, a rename lands as a same-size
//! 32-byte slot overwrite that re-parses, only the targeted slot changes,
//! re-applying is a no-op, and an oversized/non-ASCII name is refused. Gates
//! on `LEGAIA_DISC_BIN`; skips+passes when unset.

use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::location_name::{self, current_name, list_names, plan_rename, slot_offset};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn known_landmark_names_decode() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    let names = list_names(&scus).expect("table");
    assert_eq!(names.len(), 16);
    // Spot-check the pinned coordinates (the element caves + the ravine names).
    assert_eq!(names[0].1, "Rim Elm");
    assert_eq!(names[3].1, "Ancient Wind Cave");
    assert_eq!(names[4].1, "Ancient Water Cave");
    assert_eq!(names[6].1, "Vidna");
    assert_eq!(names[14].1, "Conkram");
    // The slot offset math agrees with the parser.
    assert_eq!(slot_offset(&scus, 0), Some(0x64318));
    assert_eq!(slot_offset(&scus, 3), Some(0x64378));
}

#[test]
fn rename_is_surgical_and_reparses() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let before = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");

    // Rename the wind cave (idx 3) to a shorter and a longer-ish name.
    let report = apply::rename_locations(
        &mut patcher,
        &[
            (3, "Ancient Fire Cave".to_string()),
            (4, "Ancient Ice Cave".to_string()),
        ],
    )
    .expect("apply");
    assert_eq!(report.renames.len(), 2);
    assert!(
        report
            .renames
            .iter()
            .any(|(i, o, n)| *i == 3 && o == "Ancient Wind Cave" && n == "Ancient Fire Cave")
    );

    let after = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    // Only the two 32-byte slots changed.
    let off3 = slot_offset(&before, 3).unwrap();
    let off4 = slot_offset(&before, 4).unwrap();
    let changed: std::collections::BTreeSet<usize> = before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    for &i in &changed {
        let in3 = (off3..off3 + 0x20).contains(&i);
        let in4 = (off4..off4 + 0x20).contains(&i);
        assert!(in3 || in4, "changed byte {i:#x} is inside a renamed slot");
    }
    // The patched slots re-parse to the new names, zero-padded (no stale tail).
    assert_eq!(current_name(&after, 3).unwrap(), "Ancient Fire Cave");
    assert_eq!(current_name(&after, 4).unwrap(), "Ancient Ice Cave");
    assert_eq!(
        after[off4 + "Ancient Ice Cave".len()],
        0,
        "slot is NUL-terminated"
    );
}

#[test]
fn reapply_noop_and_bad_names_refused() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    // Renaming to the existing name is a no-op plan.
    assert!(plan_rename(&scus, 6, "Vidna").unwrap().is_none());
    // Too long / non-ASCII / OOB are refused.
    assert!(plan_rename(&scus, 6, &"x".repeat(40)).is_err());
    assert!(plan_rename(&scus, 6, "Vïdna Ravine").is_err());
    assert!(location_name::plan_rename(&scus, 99, "X").is_err());
}

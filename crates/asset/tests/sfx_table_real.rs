//! Decode the real static SFX descriptor table (`DAT_8006F198`) out of
//! `extracted/SCUS_942.54` if present. Skips and passes when the executable
//! isn't on disk - same gating pattern as the other disc-dependent tests so CI
//! doesn't need Sony bytes.

use legaia_asset::sfx_table::{
    PINNED_SLOT_BANKS, SFX_TABLE_ENTRIES, SfxTable, prot_index_for_slot,
};
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

#[test]
fn decodes_the_sfx_descriptor_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let table = SfxTable::from_scus(&bytes).expect("parse SFX table");

    assert_eq!(table.len(), SFX_TABLE_ENTRIES, "100 static descriptors");

    // The static table is fully populated: every one of the 100 entries is an
    // active cue (voice count >= 1) and the trailing 3 bytes are always zero.
    // That is exactly what marks the table's extent - id 0x64 onward is
    // unrelated rodata (the `\PSX.EXE` dev path).
    for (id, d) in table.active() {
        assert!(d.voice_count() >= 1, "id {id:#x} active");
        assert!(
            d.voice_count() <= 3,
            "id {id:#x} voice count {} within observed max",
            d.voice_count()
        );
        assert_eq!(d.reserved, [0, 0, 0], "id {id:#x} reserved bytes zero");
    }
    assert_eq!(table.active().count(), SFX_TABLE_ENTRIES, "all 100 active");

    // Pinned descriptors, including the two cue ids the engine's default
    // SfxBank already references (0x1A generic hit, 0x4C).
    let e00 = table.get(0x00).unwrap();
    assert_eq!((e00.program, e00.note, e00.voice_count()), (0, 60, 1));

    let e1a = table.get(0x1A).unwrap();
    assert_eq!((e1a.program, e1a.note, e1a.voice_count()), (3, 67, 1));

    let e4c = table.get(0x4C).unwrap();
    assert_eq!((e4c.program, e4c.tone, e4c.voice_count()), (3, 8, 2));

    // Last real entry; id 0x64 would be the `\PSX.EXE` string if we over-read.
    let e63 = table.get(0x63).unwrap();
    assert_eq!((e63.program, e63.note, e63.voice_count()), (4, 60, 2));
    assert!(
        table.get(0x64).is_none(),
        "table stops at the static extent"
    );
}

/// The routing half: the real table's category histogram and the slot set it
/// reaches. Both are invariants of the disc, so a change here means either the
/// parser moved or the executable did.
#[test]
fn category_histogram_and_slot_set_are_the_disc_ones_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let table = SfxTable::from_scus(&bytes).expect("parse SFX table");

    // Four categories, and the count behind each.
    for (category, want) in [(0u8, 16usize), (2, 53), (6, 30), (11, 1)] {
        let n = table
            .active()
            .filter(|(_, d)| d.category == category)
            .count();
        assert_eq!(n, want, "category {category} descriptor count");
    }
    assert_eq!(table.slots_used(), vec![0, 2, 6, 11]);

    // The 16 shared UI cues spread across five programs, so routing them needs
    // the whole slot-0 bank rather than a subset of it.
    let mut progs: Vec<u8> = table
        .active()
        .filter(|(_, d)| d.category == 0)
        .map(|(_, d)| d.program)
        .collect();
    progs.sort_unstable();
    progs.dedup();
    assert_eq!(progs, vec![0, 1, 2, 3, 10], "category-0 programs");

    // The four traced pause-menu / strike cues are category 0, so they route to
    // the slot-0 system bank (PROT 0868) - not to the class-2 bank a
    // single-bank host stages.
    for id in [0x1Au8, 0x20, 0x21, 0x37] {
        assert_eq!(table.slot_for_cue(id), Some(0), "cue {id:#x} is category 0");
    }
    // The Baka duel hit is the contrasting case.
    assert_eq!(table.slot_for_cue(0x09), Some(2));

    // Exactly two slots are pinned to PROT entries; the other two the table
    // reaches are the open unknowns.
    for slot in table.slots_used() {
        let pinned = PINNED_SLOT_BANKS.iter().any(|(s, _)| *s == slot);
        assert_eq!(pinned, prot_index_for_slot(slot).is_some());
    }
    assert_eq!(PINNED_SLOT_BANKS.len(), 2);
}

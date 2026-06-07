//! Decode the real static SFX descriptor table (`DAT_8006F198`) out of
//! `extracted/SCUS_942.54` if present. Skips and passes when the executable
//! isn't on disk - same gating pattern as the other disc-dependent tests so CI
//! doesn't need Sony bytes.

use legaia_asset::sfx_table::{SFX_TABLE_ENTRIES, SfxTable};
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

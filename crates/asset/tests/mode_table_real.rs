//! Decode the real game-mode dispatch table out of `extracted/SCUS_942.54`
//! if present. Skips and passes when the executable isn't on disk - same
//! gating pattern as the other disc-dependent SCUS-table tests so CI doesn't
//! need Sony bytes.

use legaia_asset::mode_table::{MODE_COUNT, ModeTable, SHARED_PER_FRAME_HANDLER};
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

#[test]
fn decodes_the_mode_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let table = ModeTable::from_scus(&bytes).expect("parse mode table");

    assert_eq!(table.entries.len(), MODE_COUNT);

    // The four handler pointers pinned in docs/subsystems/boot.md are the
    // ground-truth cross-check for the table layout + offset math.
    assert_eq!(
        table.entry(0).unwrap().handler,
        0x8002_5C68,
        "mode 0 CONFIG INIT"
    );
    assert_eq!(
        table.entry(2).unwrap().handler,
        0x8002_5B64,
        "mode 2 MAIN INIT"
    );
    assert_eq!(
        table.entry(24).unwrap().handler,
        0x8002_5980,
        "mode 24 OTHER INIT"
    );
    assert_eq!(
        table.entry(26).unwrap().handler,
        0x8002_5FB4,
        "mode 26 STR INIT"
    );

    // Even modes are init, odd modes per-frame.
    for e in &table.entries {
        assert_eq!(e.is_per_frame(), e.index % 2 == 1);
    }

    // The +0x0A next-mode field only carries two retail values: -1
    // (self-managed) or 0 (fall back to mode 0 / CONFIG); the low half of
    // the +0x08 word is always zero.
    for e in &table.entries {
        assert_eq!(e.next_word & 0xFFFF, 0, "mode {} +0x08 low half", e.index);
        assert!(
            matches!(e.next_mode(), None | Some(0)),
            "mode {} next_mode = {:?}",
            e.index,
            e.next_mode()
        );
    }
    // Spot-pin both shapes: MAIN MODE (3) falls back to mode 0 on
    // completion; BATTLE MODE (21) is self-managed.
    assert_eq!(table.entry(3).unwrap().next_mode(), Some(0));
    assert_eq!(table.entry(21).unwrap().next_mode(), None);

    // The structural finding: exactly 12 of the 14 per-frame modes route
    // through the shared generic per-frame handler; only Mode 13 (MAPDISP
    // MODE, 0x80025F2C) and Mode 23 (CARD MODE, 0x80025F74) carry their own.
    assert_eq!(
        table.shared_handler_count(),
        12,
        "shared per-frame handler count"
    );
    assert_eq!(
        table.entry(13).unwrap().handler,
        0x8002_5F2C,
        "mode 13 MAPDISP MODE"
    );
    assert_eq!(
        table.entry(23).unwrap().handler,
        0x8002_5F74,
        "mode 23 CARD MODE"
    );
    assert_ne!(table.entry(13).unwrap().handler, SHARED_PER_FRAME_HANDLER);
    assert_ne!(table.entry(23).unwrap().handler, SHARED_PER_FRAME_HANDLER);

    // Dev name strings resolve from the executable (the on-disc strings,
    // including the "MAPDSIP" misspelling at mode 12/13).
    assert!(table.entry(2).unwrap().name.contains("MAIN"));
    assert!(table.entry(20).unwrap().name.contains("BATTLE"));
    assert!(table.entry(26).unwrap().name.contains("STR"));
}

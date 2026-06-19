//! Reconcile the engine's transcribed mode table (`engine_core::mode::TABLE`)
//! against the dispatch table decoded straight off the user's executable
//! (`legaia_asset::mode_table`). Skips and passes when `extracted/SCUS_942.54`
//! isn't on disk - same gating pattern as the other SCUS-table tests so CI
//! runs without Sony bytes.

use legaia_engine_core::mode::{GameMode, TABLE};
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

#[test]
fn engine_table_matches_disc_recovered_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let retail =
        legaia_asset::mode_table::ModeTable::from_scus(&bytes).expect("parse retail mode table");

    assert_eq!(retail.entries.len(), TABLE.len());
    for (i, engine) in TABLE.iter().enumerate() {
        let disc = retail.entry(i).unwrap();
        assert_eq!(
            engine.mode.as_index(),
            i,
            "engine TABLE ordering at index {i}"
        );
        // Dev name strings (the disc carries trailing whitespace on some,
        // e.g. mode 2 "MAIN "; read_name trims it).
        assert_eq!(engine.name, disc.name, "mode {i} name");
        // Handler parameter word.
        assert_eq!(engine.param, disc.param, "mode {i} param");
        // Next-mode field: the engine's Option<GameMode> must agree with the
        // retail i16 at +0x0A (-1 = None, 0 = ConfigInit).
        assert_eq!(
            engine.next.map(GameMode::as_index),
            disc.next_mode(),
            "mode {i} next-mode"
        );
    }
}

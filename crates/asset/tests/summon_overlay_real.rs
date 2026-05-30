//! Disc-gated regression pinning the Gimard Tail Fire summon record table.
//!
//! PROT entry 905 is the Gimard *Tail Fire* summon code overlay (spell id
//! `0x81`). Its embedded scene-graph record table is byte-pinned here: located
//! structurally after the leading function's `jr ra` epilogue (offset `0x180C`),
//! 19 records of `0x58` bytes, the first three of which are transform nodes
//! (`model_sel == -1`), ending exactly at the MIPS-code boundary `0x1E94`.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset (and the extracted PROT dir is absent).

use std::path::PathBuf;

use legaia_asset::summon_overlay::{
    self, GIMARD_RECORD_COUNT, GIMARD_TABLE_OFFSET, SUMMON_BYTECODE_LEN, SUMMON_RECORD_STRIDE,
};

fn prot_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .to_path_buf();
    let p = repo.join("extracted").join("PROT");
    p.is_dir().then_some(p)
}

fn locate_entry(dir: &PathBuf, prot_index: u32) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let s = e.file_name().to_string_lossy().into_owned();
        if s.starts_with(&format!("{prot_index:04}_")) && s.ends_with(".BIN") {
            return Some(e.path());
        }
    }
    None
}

#[test]
fn gimard_summon_table_is_pinned() {
    let Some(dir) = prot_dir() else {
        eprintln!("LEGAIA_DISC_BIN or extracted/PROT not available; skipping");
        return;
    };
    let path = locate_entry(&dir, summon_overlay::GIMARD_PROT_INDEX)
        .expect("PROT 0905 (Gimard summon overlay) not found");
    let bytes = std::fs::read(&path).expect("read PROT 0905");

    // The table offset is recovered structurally from the disc (the leading
    // function's `jr ra` epilogue), independent of the hard-coded constant.
    let located = summon_overlay::locate_table_offset(&bytes, 0x2000)
        .expect("a jr-ra epilogue precedes the table");
    assert_eq!(
        located, GIMARD_TABLE_OFFSET,
        "located table offset must match the pinned PROT 905 offset"
    );

    let overlay = summon_overlay::parse_gimard(&bytes).expect("table fits in PROT 905");
    assert_eq!(overlay.records.len(), GIMARD_RECORD_COUNT);

    // The table ends exactly at the MIPS-code boundary (the next function's
    // code resumes here; see the module docs).
    let table_end = GIMARD_TABLE_OFFSET + GIMARD_RECORD_COUNT * SUMMON_RECORD_STRIDE;
    assert_eq!(table_end, 0x1E94);

    // The first three records are transform nodes.
    for (i, rec) in overlay.records.iter().take(3).enumerate() {
        assert!(
            rec.is_transform_node(),
            "record {i} should be a transform node (model_sel == -1)"
        );
    }
    // Every record exposes a full move-VM bytecode slot.
    for rec in &overlay.records {
        assert_eq!(rec.bytecode.len(), SUMMON_BYTECODE_LEN);
    }
    // At least one mesh-binding record exists past the transform-node prefix.
    assert!(
        overlay.records.iter().any(|r| r.model_sel >= 0),
        "the table mixes transform nodes with mesh-binding records"
    );

    // The first record's move-VM bytecode begins with a valid opcode (< 0x47).
    let first_op = u16::from_le_bytes([
        overlay.records[0].bytecode[0],
        overlay.records[0].bytecode[1],
    ]);
    assert!(
        first_op < 0x47,
        "record 0's move-VM stream starts with opcode 0x{first_op:04X} (must be < 0x47)"
    );
}

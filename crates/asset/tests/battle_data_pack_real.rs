//! Disc-gated regression test for the [`battle_data_pack`] detector
//! against the retail player battle files (extraction 0865 = Gala's
//! `PLAYER3`, 0863 = Vahn's `PLAYER1`; retail `battle_data` block).
//! Skips silently when `LEGAIA_DISC_BIN` is unset (CI without disc data).

use std::path::PathBuf;

use legaia_asset::battle_data_pack;

fn extracted_prot_dir() -> Option<PathBuf> {
    let cands = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    cands.into_iter().find(|p| p.is_dir())
}

#[test]
fn detects_retail_0865_battle_data_pack() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let path = prot_dir.join("0865_battle_data.BIN");
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return;
    }
    let raw = std::fs::read(&path).expect("read 0865");
    let pack = battle_data_pack::parse(&raw).expect("parse pack");

    // 0865 has 87 declared records with 43 non-zero. The retail layout
    // is stable across the corpus (the disc data is read-only) so these
    // counts are safe invariants.
    assert_eq!(pack.record_count, 87, "0865 record_count");
    assert_eq!(pack.records.len(), 43, "0865 non-zero records");
    assert_eq!(pack.table_offset, 0x6C70, "0865 table_offset");
    assert_eq!(pack.data_base, 0x8000, "0865 data_base");

    // Every non-zero record must LZS-decode and (for the offset-zero
    // filler at rec 42) carry a recognizable Legaia TMD at offset 0x20
    // (canonical) or somewhere word-aligned past that.
    let mut decoded_total = 0usize;
    let mut tmds_found = 0usize;
    for record in &pack.records {
        let entry = battle_data_pack::decode_record(&raw, &pack, record.index)
            .unwrap_or_else(|e| panic!("decode rec{}: {e}", record.index));
        decoded_total += entry.bytes.len();
        if entry.tmd_range.is_some() {
            tmds_found += 1;
        }
    }
    // All 43 retail records carry a TMD - the locator's 0x20-or-scan
    // fallback finds every one.
    assert_eq!(tmds_found, 43, "every retail record has an embedded TMD");
    // Total decompressed payload is in the 800KB ballpark - assert a
    // floor that flags decoder regression.
    assert!(
        decoded_total > 600_000,
        "expected > 600KB decompressed total, got {}",
        decoded_total
    );
}

#[test]
fn detects_retail_0863_edstati3_pack() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let path = prot_dir.join("0863_edstati3.BIN");
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return;
    }
    let raw = std::fs::read(&path).expect("read 0863");
    let pack = battle_data_pack::parse(&raw)
        .expect("0863_edstati3 should match the same battle_data pack shape");
    // Sanity: edstati3 has ~54 non-zero records.
    assert!(
        pack.records.len() >= 30 && pack.records.len() <= 70,
        "0863 record count out of range: {}",
        pack.records.len()
    );
}

/// `clut_uploads` is currently the documented no-op (the descriptor at
/// `u32[3..0x20]` has not been pinned by the byte-match corpus - see
/// `docs/formats/battle-data-pack.md`). Guard the contract so any
/// future descriptor work has to explicitly update this expectation
/// rather than silently change [`SceneResources::build_targeted`]'s
/// CLUT pass.
#[test]
fn clut_uploads_is_empty_for_every_retail_0865_record() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let path = prot_dir.join("0865_battle_data.BIN");
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return;
    }
    let raw = std::fs::read(&path).expect("read 0865");
    let pack = battle_data_pack::parse(&raw).expect("parse pack");
    let mut total_uploads = 0usize;
    for record in &pack.records {
        let entry = battle_data_pack::decode_record(&raw, &pack, record.index)
            .unwrap_or_else(|e| panic!("decode rec{}: {e}", record.index));
        total_uploads += battle_data_pack::clut_uploads(&entry).len();
    }
    assert_eq!(
        total_uploads, 0,
        "clut_uploads should stay a no-op until the descriptor encoding is pinned"
    );
}

//! Disc-gated end-to-end: pull a real per-scene MAN out of the PROT.DAT
//! corpus, decode it via [`legaia_engine_core::encounter_man::encounter_table_from_man`],
//! and assert the result matches the byte-exact retail layout (formation
//! count, formation row 3 = `[count=2, ids=4,4]` for `map01`).
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - The MAN → EncounterTable wiring regresses (table comes back empty
//!   when it shouldn't, or rates clamp wrong).
//! - The `formation_record_for_row` helper drifts from
//!   `legaia_asset::man_section`'s record layout.
//! - The scene-bundle MAN extractor (`extract_man_payload`) drops bytes.

use legaia_asset::{monster_archive, scene_asset_table};
use legaia_engine_core::encounter_man::{
    encounter_table_from_man, formation_defs_from_man, formation_record_for_row,
};
use std::collections::HashSet;
use std::path::PathBuf;

fn extracted_prot() -> Option<PathBuf> {
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let d = PathBuf::from(p);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

fn load_man_from_scene(bytes: &[u8]) -> Option<Vec<u8>> {
    let table = scene_asset_table::detect(bytes)?;
    let man = table
        .descriptors
        .iter()
        .find(|d| d.type_byte == 0x03)
        .copied()?;
    let start = man.data_offset as usize;
    if start >= bytes.len() {
        return None;
    }
    let body = &bytes[start..];
    let (decoded, _) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
    if decoded.len() != man.size as usize {
        return None;
    }
    Some(decoded)
}

#[test]
fn real_map01_encounter_table_round_trips() {
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let bytes = std::fs::read(prot.join("0086_map01.BIN"))
        .expect("0086_map01.BIN missing - check extracted/PROT");
    let man = load_man_from_scene(&bytes).expect("MAN extract for map01");
    let table = encounter_table_from_man("map01", &man).expect("encounter table for map01");

    // map01 declares 37 formations in the encounter section. Zero-count
    // formations get filtered out (none in this scene's table).
    assert_eq!(
        table.entries.len(),
        37,
        "expected 37 formation entries for map01"
    );
    // Trigger rate is the mean of the 64 region rate_increments. The
    // first 6 regions all have rate 32, and the rest are non-zero too;
    // mean lands in the low-30s for map01.
    assert!(
        (32..=48).contains(&table.trigger_rate_q8),
        "expected map01 mean region rate in 32..=48, got {}",
        table.trigger_rate_q8,
    );

    // Row 3 = `[00 00 00 02 04 04 00 00]` per the FUN_8003A110 walk and
    // the manual hex dump in docs/formats/encounter.md.
    let r3 = formation_record_for_row(&man, 3).expect("row 3");
    assert_eq!(r3.count, 2);
    assert_eq!(r3.monster_ids, [4, 4, 0, 0]);
    // `apply_to_formation_cell` reproduces the in-RAM mc2 snapshot.
    let mut cell = [0u8; 4];
    r3.apply_to_formation_cell(&mut cell);
    assert_eq!(
        cell,
        [4, 4, 0, 0],
        "row 3 matches mc2 formation cell snapshot",
    );
}

#[test]
fn real_corpus_encounter_tables_load_for_every_scene_bundle() {
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&prot)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    entries.sort();

    let mut scenes_with_tables = 0usize;
    let mut total_entries = 0usize;
    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(man) = load_man_from_scene(&bytes) else {
            continue;
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        if let Some(table) = encounter_table_from_man(&stem, &man) {
            scenes_with_tables += 1;
            total_entries += table.entries.len();
        }
    }

    // Most scenes have real encounter tables; a handful (cutscene-style
    // bundles like `0796_edlast.BIN`) might declare zero formations.
    // The floor here is generous; tighten if regressions creep in.
    assert!(
        scenes_with_tables >= 60,
        "expected ≥ 60 scenes with encounter tables, found {scenes_with_tables}"
    );
    eprintln!(
        "[encounter_man_real] {scenes_with_tables} scenes wired, {total_entries} total formation entries"
    );
}

/// Cross-table integrity: every monster id a scene's encounter formations
/// reference must resolve to a real monster in the archive (PROT 867). This
/// ties the encounter-MAN parser to the monster-archive parser — a drift in
/// either (a shifted formation field, a wrong slot stride) would make a
/// formation point at an empty/out-of-range monster slot, which the encounter
/// and door randomizers would then spawn as garbage. Guards both at once.
#[test]
fn every_encounter_formation_references_a_real_monster() {
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    // The set of monster ids the archive actually populates (real records,
    // non-empty name).
    let archive = std::fs::read(prot.join("0867_battle_data.BIN"))
        .expect("0867_battle_data.BIN missing - check extracted/PROT");
    let valid: HashSet<u16> = monster_archive::records(&archive)
        .expect("decode monster archive")
        .into_iter()
        .filter(|r| !r.name.trim().is_empty())
        .map(|r| r.id)
        .collect();
    assert!(valid.len() > 150, "expected a populated monster archive");

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&prot)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    entries.sort();

    let mut formations_checked = 0usize;
    let mut slots_checked = 0usize;
    let mut dangling: Vec<String> = Vec::new();
    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(man) = load_man_from_scene(&bytes) else {
            continue;
        };
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        for def in formation_defs_from_man(&man) {
            formations_checked += 1;
            for slot in &def.slots {
                let id = slot.monster_id;
                if id == 0 {
                    continue;
                }
                slots_checked += 1;
                if !valid.contains(&id) {
                    dangling.push(format!(
                        "{stem} formation {} references monster id {id} not in the archive",
                        def.formation_id
                    ));
                }
            }
        }
    }

    eprintln!(
        "[encounter-xref] formations_checked={formations_checked} slots_checked={slots_checked} \
         dangling={}",
        dangling.len()
    );
    assert!(
        formations_checked > 300 && slots_checked > 300,
        "too few formations/slots checked ({formations_checked}/{slots_checked}) - sweep not exercising the corpus"
    );
    assert!(
        dangling.is_empty(),
        "encounter formations reference monsters absent from the archive (parser drift?):\n{}",
        dangling
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

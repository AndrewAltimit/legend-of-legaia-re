//! Disc-gated: per-scene STR FMV trigger ids recovered from the scene MANs.
//!
//! The field-VM FMV trigger (`0x4C 0xE2`) carries its `fmv_id` as a literal
//! `i16` operand, so "which movie does each scene's script fire" is
//! disc-sourced data: [`scene_fmv_triggers`] walks every scene MAN's
//! partition-1 scripts and decodes the ops, the static complement of the
//! trigger-side save corpus (which pins the runtime globals across
//! `fmv_id 0..=8` but was captured via debug-menu paths, not the per-scene
//! ops). Skips when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::scene_fmv_triggers;

fn extracted_prot() -> Option<PathBuf> {
    for p in [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ] {
        let d = PathBuf::from(p);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

fn load_man_from_scene(bytes: &[u8]) -> Option<Vec<u8>> {
    let table = legaia_asset::scene_asset_table::detect(bytes)?;
    let man = table
        .descriptors
        .iter()
        .find(|d| d.type_byte == 0x03)
        .copied()?;
    let start = man.data_offset as usize;
    if start >= bytes.len() {
        return None;
    }
    let (decoded, _) = legaia_lzs::decompress_tracked(&bytes[start..], man.size as usize).ok()?;
    (decoded.len() == man.size as usize).then_some(decoded)
}

#[test]
fn every_scene_fmv_trigger_decodes_from_disc() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing - run `legaia-extract` first");
        return;
    };

    // entry-file-name -> sorted distinct fmv_ids fired by that scene's scripts
    let mut found: BTreeMap<String, Vec<i16>> = BTreeMap::new();
    let mut scenes_with_man = 0usize;

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&prot)
        .expect("read extracted/PROT")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "BIN"))
        .collect();
    entries.sort();

    for path in entries {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Some(man) = load_man_from_scene(&bytes) else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        scenes_with_man += 1;
        let triggers = scene_fmv_triggers(&mf, &man);
        if triggers.is_empty() {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let mut ids: Vec<i16> = triggers.iter().map(|t| t.fmv_id).collect();
        ids.sort_unstable();
        ids.dedup();
        eprintln!("{name}: fmv_ids {ids:?} ({} trigger op(s))", triggers.len());
        found.insert(name, ids);
    }

    eprintln!(
        "scanned {scenes_with_man} scene MANs; {} carry FMV triggers",
        found.len()
    );
    assert!(scenes_with_man > 50, "MAN extraction regressed");

    // The pinned per-scene movie assignment (one trigger op per scene, ids
    // literal in the bytecode). Under the corrected 32-byte-stride dispatch
    // table (`legaia_asset::fmv_dispatch`) every id below is a retail
    // movie: 1 = MV2, 2..=4 = MV3 segments 1..3, 6 = MV4, 7 = MV5,
    // 8 = MV6. fmv_id 0 (the MV1 intro) fires from the title/new-game
    // path, not a scene MAN; fmv_id 5 (MV3 segment 4, the one slot whose
    // post-play hand-off stays in the current scene) appears in no scene
    // MAN script.
    let expected: &[(&str, &[i16])] = &[
        ("0004_town01", &[1]),
        ("0095_garmel", &[2]),
        ("0218_dohaty", &[4]),
        ("0348_town0d", &[6]),
        ("0435_uru", &[7]),
        ("0606_deroa", &[3]),
        ("0689_jouine", &[8]),
        ("0706_chitei2", &[3]),
    ];
    assert_eq!(found.len(), expected.len(), "FMV-trigger scene set drifted");
    for (scene, ids) in expected {
        assert_eq!(
            found.get(*scene).map(Vec::as_slice),
            Some(*ids),
            "{scene} trigger ids"
        );
    }
}

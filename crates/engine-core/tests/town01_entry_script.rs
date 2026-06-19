//! Disc-gated: town01's MAN scene-entry system script resolves + the Tetsu
//! arm lives in a MAN partition-1 interaction script.
//!
//! Regression guard against the (now-corrected) "town01 is a standalone
//! SceneEventScripts scene whose script source is unpinned, so the engine
//! runs a halting record-0 stub" framing. town01 is a `SceneAssetTable`
//! bundle (PROT entry 4); its MAN scene-entry script resolves and
//! `enter_field_scene` loads it. The opening Tetsu fight is armed from a
//! MAN partition-1 NPC interaction script, not the event-scripts prescript.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::{ProtIndex, Scene};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn town01_man_entry_script_resolves_and_arm_lives_in_partition_one() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scene = Scene::load(&index, "town01").expect("load town01");

    // town01 IS a scene_asset_table bundle (PROT entry 4) - not a "standalone
    // SceneEventScripts scene with no MAN".
    let bundle = legaia_engine_core::scene_bundle::find_bundle(&scene)
        .expect("town01 has a scene_asset_table bundle");
    assert_eq!(bundle.entry_idx(), 4, "town01 bundle entry");
    assert_eq!(
        index.class_of(bundle.entry_idx()).expect("class"),
        legaia_asset::categorize::Class::SceneAssetTable,
        "town01 bundle class"
    );

    // The MAN scene-entry system script resolves (the engine loads this via
    // load_field_script_at, overriding the record-0 fallback). pc0 = 11 =
    // 1 + N*2 + 4 with the entry record's local-count N = 3.
    let entry = scene
        .field_man_entry_script(&index)
        .expect("entry-script resolve")
        .expect("town01 entry script is Some - the engine runs real bytecode");
    let (bytecode, pc0) = entry;
    assert_eq!(pc0, 11, "entry-script first-opcode offset");
    assert!(
        bytecode.len() > 4096,
        "entry-script slice is the whole tail of the MAN (got {} bytes)",
        bytecode.len()
    );

    // The encounter table also resolves from the same MAN (formation index 4 =
    // the lone Tetsu, per docs/formats/encounter.md).
    let enc = scene
        .field_man_encounter_table(&index, "town01")
        .expect("encounter resolve")
        .expect("town01 encounter table is Some");
    assert!(enc.1.len() >= 5, "town01 has the expected formation set");

    // The Tetsu arm lives in a MAN partition-1 NPC interaction script, not the
    // event-scripts prescript: at least one P1 record carries a bare
    // arm-encounter op (0x37 / 0x41). (P1[0] is the system entry script; the
    // NPC scripts are P1[1..].)
    let entry_bytes = index
        .entry_bytes_extended(bundle.entry_idx())
        .expect("entry bytes");
    let man = legaia_engine_core::scene_bundle::extract_man_payload(&bundle, &entry_bytes)
        .expect("man extract")
        .expect("town01 MAN payload");
    let parsed = legaia_asset::man_section::parse(&man).expect("man parse");
    assert_eq!(
        parsed.header.partition_counts,
        [36, 53, 39],
        "town01 MAN partition counts"
    );
    let n1 = parsed.header.partition_counts[1].max(0) as usize;
    let mut arm_op_records = 0usize;
    for i in 1..n1 {
        let Some(start) = parsed.actor_placement_record_offset(i, man.len()) else {
            continue;
        };
        let end = (1..n1)
            .filter_map(|j| parsed.actor_placement_record_offset(j, man.len()))
            .filter(|&o| o > start)
            .min()
            .unwrap_or(man.len());
        if man[start..end.min(man.len())]
            .iter()
            .any(|&b| b == 0x37 || b == 0x41)
        {
            arm_op_records += 1;
        }
    }
    assert!(
        arm_op_records > 0,
        "at least one partition-1 NPC script carries an arm-encounter op"
    );
    eprintln!(
        "[ok] town01: entry script {} bytes (pc0 {}), {} P1 NPC scripts, \
         {} carry an arm-encounter op",
        bytecode.len(),
        pc0,
        n1 - 1,
        arm_op_records
    );
}

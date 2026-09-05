//! Disc-gated: pin the sparring tutorial's one-shot arm to real disc bytecode
//! through the census a direct `--battle <row>` entry replays it from.
//!
//! Retail turns the Tetsu tutorial on with a system-flag SET (`0x19`) that
//! sits in town01's sparring record three ops before the record's `3E FF 04`
//! battle-entry op (`docs/subsystems/battle.md`, "Who writes stage id 1").
//! `World::replay_scripted_battle_arm` keys on that SET -> entry pairing
//! (`man_field_scripts::walk_battle_entry_arms`), so this walks the real MAN
//! and asserts the pairing is found where the doc says - and that a scene
//! with no scripted arm (the overworld) yields none for that flag.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::battle_tutorial::TUTORIAL_ARM_FLAG;
use legaia_engine_core::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
use legaia_engine_core::man_field_scripts::walk_battle_entry_arms;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::World;
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

fn scene_man(index: &ProtIndex, name: &str) -> (legaia_asset::man_section::ManFile, Vec<u8>) {
    let scene = Scene::load(index, name).expect("load scene");
    let man = scene
        .field_man_payload(index)
        .expect("man payload fetch")
        .expect("scene has a MAN payload");
    let man_file = legaia_asset::man_section::parse(&man).expect("man parse");
    (man_file, man)
}

#[test]
fn town01_sparring_record_arms_flag_0x19_into_formation_row_4() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let (man_file, man) = scene_man(&index, "town01");

    let arms = walk_battle_entry_arms(&man_file, &man);
    for a in &arms {
        eprintln!(
            "  town01 P{}[{}] SET 0x{:03X} -> 3E FF {} @ 0x{:05X}",
            a.partition, a.record, a.flag, a.row, a.abs_pc
        );
    }
    let tutorial: Vec<_> = arms
        .iter()
        .filter(|a| a.flag == TUTORIAL_ARM_FLAG)
        .collect();
    assert_eq!(
        tutorial.len(),
        1,
        "exactly one record raises the tutorial arm on its way into a fight"
    );
    assert_eq!(
        u16::from(tutorial[0].row),
        RIM_ELM_TRAINING_FORMATION_ID,
        "the arm enters the training formation row"
    );
    assert_eq!(
        tutorial[0].partition, 1,
        "the sparring record is a P1 actor script"
    );

    // The world-level replay: installing town01's carriers reads the census,
    // and a direct entry into row 4 raises the flag `World::enter_battle`
    // consumes; any other row does not.
    let mut world = World::new();
    world.install_field_carriers_from_man(&man_file, &man);
    assert!(!world.replay_scripted_battle_arm(RIM_ELM_TRAINING_FORMATION_ID + 1));
    assert!(!world.system_flag_test(TUTORIAL_ARM_FLAG));
    assert!(world.replay_scripted_battle_arm(RIM_ELM_TRAINING_FORMATION_ID));
    assert!(world.system_flag_test(TUTORIAL_ARM_FLAG));
    // The entry consumes it exactly once.
    assert!(world.take_battle_tutorial_arm());
    assert!(!world.system_flag_test(TUTORIAL_ARM_FLAG));
    assert!(!world.take_battle_tutorial_arm());
}

#[test]
fn overworld_map01_raises_no_tutorial_arm() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let (man_file, man) = scene_man(&index, "map01");
    let arms = walk_battle_entry_arms(&man_file, &man);
    assert!(
        arms.iter().all(|a| a.flag != TUTORIAL_ARM_FLAG),
        "map01 must not arm the sparring tutorial: {arms:?}"
    );
    let mut world = World::new();
    world.install_field_carriers_from_man(&man_file, &man);
    assert!(!world.replay_scripted_battle_arm(0));
    assert!(!world.system_flag_test(TUTORIAL_ARM_FLAG));
}

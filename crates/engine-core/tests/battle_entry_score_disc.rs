//! Disc-gated: pin the op-`0x35` words a scripted boss's record runs on its
//! way into the fight, through the census a direct `--battle <row>` entry
//! replays them from (`man_field_scripts::walk_battle_entry_scores`,
//! `World::replay_scripted_battle_score`).
//!
//! The event picks the fight's music, not the formation
//! (`docs/subsystems/audio.md`, "The battle sound set picks the fight's
//! track"): korb3's Gaza record starts `2028` and selects sound set `-1`
//! before `3E FF 0F`; nilboa's first Nivora duel record does the same before
//! `3E FF 1D`; jouine's evolved-Cort record starts `2071` and selects set `8`
//! before `3E FF 15`. A forced entry that skips the record otherwise keeps the
//! scene entry's word (korb3 parks at `0x1000`) and the default battle theme.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::man_field_scripts::walk_battle_entry_scores;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::World;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
        && d.join("CDNAME.TXT").exists()
    {
        return Some(d);
    }
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

/// `(scene, row, track word after the replay, battle sound set)`.
const BOSS_ENTRIES: [(&str, u16, u16, i32); 3] = [
    ("korb3", 15, 2028, -1),
    ("nilboa", 29, 2028, -1),
    ("jouine", 21, 2071, 8),
];

#[test]
fn boss_records_replay_their_theme_and_sound_set() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    for (scene, row, track, set) in BOSS_ENTRIES {
        let (man_file, man) = scene_man(&index, scene);
        let scores = walk_battle_entry_scores(&man_file, &man);
        for s in &scores {
            eprintln!(
                "[ran] {scene} P{}[{}] -> 3E FF {}: {:?}",
                s.partition, s.record, s.row, s.words
            );
        }
        let mut world = World::new();
        world.install_field_carriers_from_man(&man_file, &man);
        world.audio.battle_sound_set = 0;
        world.audio.current_bgm = Some(4096);
        assert!(
            world.replay_scripted_battle_score(row),
            "{scene}: row {row} has a record that picks its music"
        );
        assert_eq!(
            world.audio.current_bgm,
            Some(track),
            "{scene} row {row} track"
        );
        assert_eq!(
            world.audio.battle_sound_set, set,
            "{scene} row {row} sound set"
        );
        // The words reach the host as ordinary op-0x35 events.
        assert!(
            world.pending_field_events.iter().any(|e| matches!(
                e,
                legaia_engine_core::field_events::FieldEvent::Bgm { text_id, sub_op: 9 | 1 }
                    if *text_id == track
            )),
            "{scene}: the start reaches the BGM director"
        );
    }
}

#[test]
fn a_row_no_record_scores_replays_nothing() {
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
    let mut world = World::new();
    world.install_field_carriers_from_man(&man_file, &man);
    world.audio.battle_sound_set = 0;
    assert!(!world.replay_scripted_battle_score(0));
    assert_eq!(world.audio.battle_sound_set, 0);
    assert_eq!(world.audio.current_bgm, None);
}

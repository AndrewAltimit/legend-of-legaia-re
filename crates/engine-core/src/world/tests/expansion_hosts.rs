//! What the hosts show and route for an expansion pack (`world::expansion_hosts`):
//! retail-scene entrances, the Journal's content for the Rebirth pack, quest
//! toasts, retail NPC talks, mode wins, the New Game restart and the notice
//! for a save naming a pack that is not loaded. Disc-free: the World is a
//! default one and the packs are synthetic or the committed `packs/rebirth`.

use super::*;
use legaia_engine_expansion::quest::{QuestEvent, QuestStatus};
use legaia_engine_expansion::quest_save::ExpansionProgress;
use legaia_engine_expansion::toast::TOAST_FRAMES;
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::ExpansionPack;

fn pack() -> std::sync::Arc<ExpansionPack> {
    let mut files = legaia_expansion_format::PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert(
        "maps/hall.toml".into(),
        br#"
name = "Hall"
base = { scene = { kind = "scene", name = "tower" } }
[tiles]
rows = ["..."]
[[spawns]]
id = "door"
tile = [1, 0]
[[entrances]]
id = "well"
scene = { kind = "scene", name = "town01" }
rect = [10, 20, 10, 20]
spawn = "door"
requires = { flags = ["open"] }
"#
        .to_vec(),
    );
    files.insert(
        "quests.toml".into(),
        br#"
[[quests]]
id = "elder"
title = "Ask the Elder"
auto_start = true
objectives = [{ kind = "talk", npc = "town01:7" }]

[[quests]]
id = "poker"
title = "Card Shark"
auto_start = true
objectives = [{ kind = "win_mode", mode = "poker", count = 2 }]
"#
        .to_vec(),
    );
    std::sync::Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

fn rebirth() -> std::sync::Arc<ExpansionPack> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    std::sync::Arc::new(ExpansionPack::from_files(&files).expect("the Rebirth pack parses"))
}

/// A one-member party at `level`, standing in a field scene.
fn field_world(level: u8) -> World {
    let mut w = World::default();
    let mut vahn = legaia_save::CharacterRecord::zeroed();
    vahn.set_level(level);
    w.party.roster.members = vec![vahn];
    w.party.party_count = 1;
    w.player_actor_slot = Some(0);
    w.mode = SceneMode::Field;
    w
}

fn stand_on(w: &mut World, x: i16, z: i16) {
    w.actors[0].move_state.world_x = x * 128 + 64;
    w.actors[0].move_state.world_z = z * 128 + 64;
}

/// Every toast the queue holds, oldest first.
fn drain_toasts(w: &mut World) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(t) = w.expansion_toast() {
        out.push(t.to_string());
        w.expansion.toasts.tick(TOAST_FRAMES);
    }
    out
}

#[test]
fn every_host_hook_is_inert_with_no_pack() {
    let mut w = field_world(1);
    w.set_active_scene_label("town01");
    stand_on(&mut w, 9, 20);
    assert_eq!(w.step_expansion_entrance(), None);
    stand_on(&mut w, 10, 20);
    assert_eq!(w.step_expansion_entrance(), None);
    assert!(!w.request_expansion_travel("hall", ""));
    assert!(w.expansion_journal().is_none());
    w.expansion_mode_won("poker");
    w.trigger_field_interact(0, 7);
    assert_eq!(w.expansion_toast(), None);
    assert_eq!(w.expansion_dormant_pack_id(), None);
}

#[test]
fn an_entrance_fires_on_entry_once_its_gate_opens() {
    let mut w = field_world(1);
    w.activate_expansion(pack()).expect("activates");
    w.set_active_scene_label("town01");
    stand_on(&mut w, 9, 20);
    assert_eq!(w.step_expansion_entrance(), None, "the first tick records");
    stand_on(&mut w, 10, 20);
    assert_eq!(
        w.step_expansion_entrance(),
        None,
        "shut: flag `open` is clear"
    );
    w.expansion_notify(QuestEvent::FlagSet("open".into()));
    stand_on(&mut w, 9, 20);
    assert_eq!(w.step_expansion_entrance(), None);
    stand_on(&mut w, 10, 20);
    let hit = w
        .step_expansion_entrance()
        .expect("open: the entrance fires");
    assert_eq!((hit.map.as_str(), hit.spawn.as_str()), ("hall", "door"));
    // Another scene, a dialogue-held or non-field tick: nothing.
    w.set_active_scene_label("town02");
    stand_on(&mut w, 9, 20);
    assert_eq!(w.step_expansion_entrance(), None);
    stand_on(&mut w, 10, 20);
    assert_eq!(w.step_expansion_entrance(), None);
    w.set_active_scene_label("town01");
    w.mode = SceneMode::Battle;
    assert_eq!(w.step_expansion_entrance(), None);
    // The Travel page's request names a map of the pack only.
    assert!(w.request_expansion_travel("hall", ""));
    assert_eq!(
        w.take_expansion_travel(),
        Some(("hall".into(), String::new()))
    );
    assert!(!w.request_expansion_travel("nowhere", ""));
}

#[test]
fn a_retail_npc_talk_and_a_mode_win_reach_the_quests_and_toast() {
    let mut w = field_world(1);
    w.activate_expansion(pack()).expect("activates");
    w.set_active_scene_label("town01");
    w.expansion_notify(QuestEvent::Refresh);
    assert_eq!(
        drain_toasts(&mut w),
        ["Quest started: Ask the Elder", "Quest started: Card Shark"]
    );
    // The touch post's interaction with retail placement 7.
    assert_eq!(w.retail_npc_id(7), "town01:7");
    w.trigger_field_interact(0, 7);
    assert_eq!(
        w.expansion.quests.quest("elder").unwrap().status,
        QuestStatus::Completed
    );
    w.expansion_mode_won("poker");
    assert_eq!(w.expansion.quests.quest("poker").unwrap().objectives, [1]);
    assert_eq!(
        drain_toasts(&mut w),
        ["Quest complete: Ask the Elder", "Card Shark: 1/2"]
    );
}

#[test]
fn the_rebirth_journal_lists_its_chain_and_challenge() {
    let mut w = field_world(30);
    w.activate_expansion(rebirth()).expect("Rebirth activates");
    let empty = w.expansion_journal().expect("a pack is active");
    assert_eq!(empty.pack_name, "Legend of Legaia: Rebirth");
    assert!(empty.quests.is_empty(), "no quest has started");
    assert_eq!(empty.challenges.len(), 1);
    assert_eq!(empty.challenges[0].title, "Bare Hands");
    assert_eq!(empty.challenges[0].best, None);
    assert!(empty.maps.iter().any(|m| m.map_id == "rebirth_tower_1f"));

    // Level 30: the rumour starts on its own.
    w.expansion_notify(QuestEvent::Refresh);
    let j = w.expansion_journal().unwrap();
    assert_eq!(j.quests[0].title, "A Tower Wakes");
    assert_eq!(j.quests[0].status, QuestStatus::Active);
    assert_eq!(
        j.quests[0].objectives[0].text,
        "Find the guide who has seen the tower"
    );
    assert_eq!(
        (
            j.quests[0].objectives[0].progress,
            j.quests[0].objectives[0].target
        ),
        (0, 1)
    );
    // Talking to the guide completes it and opens the trial.
    w.expansion_talked("tower_guide");
    let j = w.expansion_journal().unwrap();
    let titles: Vec<(&str, QuestStatus)> = j
        .quests
        .iter()
        .map(|q| (q.title.as_str(), q.status))
        .collect();
    assert_eq!(
        titles,
        [
            ("The Rebirth Tower", QuestStatus::Active),
            ("A Tower Wakes", QuestStatus::Completed),
        ]
    );
    assert_eq!(j.quests[0].rewards, "10000 G");
    assert_eq!(j.quests[0].objectives.len(), 3);
    let toasts = drain_toasts(&mut w);
    assert!(toasts.contains(&"Quest started: A Tower Wakes".to_string()));
    assert!(toasts.contains(&"Quest complete: A Tower Wakes".to_string()));
    assert!(toasts.contains(&"Quest started: The Rebirth Tower".to_string()));
}

#[test]
fn a_new_game_restarts_the_pack_and_drops_dormant_progress() {
    let mut w = field_world(1);
    w.activate_expansion(pack()).expect("activates");
    w.expansion_notify(QuestEvent::FlagSet("open".into()));
    w.begin_new_game();
    assert!(w.expansion.is_active(), "the pack stays active");
    assert!(!w.expansion.quests.progress.flags.is_set("open"));
    assert_eq!(w.expansion.quests.progress.pack_id, "t");

    // With no pack, a New Game drops a loaded save's dormant block.
    let mut w = field_world(1);
    let block = ExpansionProgress {
        pack_id: "rebirth".into(),
        pack_version: "0.1.0".into(),
        ..Default::default()
    }
    .encode();
    w.expansion_load_block(Some(&block));
    w.begin_new_game();
    assert_eq!(w.expansion_save_block(), None);
}

#[test]
fn a_save_naming_a_pack_that_is_not_loaded_says_so_and_keeps_its_block() {
    let mut w = field_world(1);
    let block = ExpansionProgress {
        pack_id: "rebirth".into(),
        pack_version: "0.1.0".into(),
        ..Default::default()
    }
    .encode();
    w.expansion_load_block(Some(&block));
    assert_eq!(w.expansion_dormant_pack_id().as_deref(), Some("rebirth"));
    assert_eq!(
        w.expansion_toast(),
        Some("Saved with expansion 'rebirth' - not loaded")
    );
    assert_eq!(w.expansion_save_block(), Some(block.clone()));
    // Loaded with that pack active: no notice, and the progress is adopted.
    let mut w = field_world(1);
    w.activate_expansion(rebirth()).expect("activates");
    w.expansion_load_block(Some(&block));
    assert_eq!(w.expansion_dormant_pack_id(), None);
    assert_eq!(w.expansion_toast(), None);
}

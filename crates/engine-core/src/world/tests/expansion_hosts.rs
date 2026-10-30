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

/// The Rebirth journal follows the pack's own content: every challenge and
/// map listed, the auto-start quest opening on its level, and its talk
/// opening the quest that names it as a prerequisite. Derived from the pack
/// files, so the content can grow without the test pinning its shape.
#[test]
fn the_rebirth_journal_lists_its_chain_and_challenge() {
    let pack = rebirth();
    let mut w = field_world(30);
    w.activate_expansion(pack.clone())
        .expect("Rebirth activates");
    let empty = w.expansion_journal().expect("a pack is active");
    assert_eq!(empty.pack_name, pack.manifest.name);
    assert!(empty.quests.is_empty(), "no quest has started");
    let titles: Vec<&str> = empty.challenges.iter().map(|c| c.title.as_str()).collect();
    let want: Vec<&str> = pack
        .quests
        .challenges
        .iter()
        .map(|c| c.title.as_str())
        .collect();
    assert_eq!(titles, want);
    assert!(empty.challenges.iter().all(|c| c.best.is_none()));
    for id in pack.maps.keys() {
        assert!(empty.maps.iter().any(|m| m.map_id == *id), "map {id}");
    }

    // The first auto-start quest the party's level opens, and its talk.
    let first = pack
        .quests
        .quests
        .iter()
        .find(|q| q.auto_start && q.prerequisites.min_level.is_some_and(|l| l <= 30))
        .expect("an auto-start quest at level 30");
    let legaia_expansion_format::quest::Goal::Talk { npc } = &first.objectives[0].goal else {
        panic!("the opening quest starts with a talk");
    };
    w.expansion_notify(QuestEvent::Refresh);
    let j = w.expansion_journal().unwrap();
    assert_eq!(j.quests[0].title, first.title);
    assert_eq!(j.quests[0].status, QuestStatus::Active);
    if let Some(text) = &first.objectives[0].text {
        assert_eq!(&j.quests[0].objectives[0].text, text);
    }
    assert_eq!(
        (
            j.quests[0].objectives[0].progress,
            j.quests[0].objectives[0].target
        ),
        (0, 1)
    );
    // Talking completes it and opens the quest given there next.
    w.expansion_talked(npc);
    let next = pack
        .quests
        .quests
        .iter()
        .find(|q| q.prerequisites.quests == [first.id.clone()] && q.giver.as_deref() == Some(npc))
        .expect("a quest follows the opener from the same giver");
    let j = w.expansion_journal().unwrap();
    let titles: Vec<(&str, QuestStatus)> = j
        .quests
        .iter()
        .map(|q| (q.title.as_str(), q.status))
        .collect();
    assert_eq!(
        titles,
        [
            (next.title.as_str(), QuestStatus::Active),
            (first.title.as_str(), QuestStatus::Completed),
        ]
    );
    assert_eq!(j.quests[0].objectives.len(), next.objectives.len());
    let toasts = drain_toasts(&mut w);
    for t in [
        format!("Quest started: {}", first.title),
        format!("Quest complete: {}", first.title),
        format!("Quest started: {}", next.title),
    ] {
        assert!(toasts.contains(&t), "{t} in {toasts:?}");
    }
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

#[test]
fn the_journal_cards_page_opens_the_card_menu_once_the_field_runs() {
    let mut w = field_world(1);
    w.activate_expansion(rebirth()).expect("Rebirth activates");
    let j = w.expansion_journal().unwrap();
    assert!(j.has_cards, "Rebirth defines trading cards");
    // The Journal's Cards pick leaves the request; the field tick opens it.
    w.expansion.open_cards = true;
    w.tick();
    assert!(w.card_screen_open(), "the card menu opened over the field");
    assert!(!w.expansion.open_cards);
}

/// A pack with one challenge on one formation, for the map events.
fn challenge_pack() -> std::sync::Arc<ExpansionPack> {
    let mut files = legaia_expansion_format::PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"ev\"\nname = \"Events\"\nversion = \"0.1.0\"\n".to_vec(),
    );
    files.insert(
        "battle.toml".into(),
        b"[monsters.wolf]\nbase = { kind = \"monster\", id = 9 }\n\n[formations.pair]\nmonsters = [\"wolf\"]\n".to_vec(),
    );
    files.insert(
        "quests.toml".into(),
        b"[[challenges]]\nid = \"sprint\"\ntitle = \"Sprint\"\nformation = \"pair\"\nrules = { turn_limit = 5 }\n\n[[challenges]]\nid = \"solo\"\ntitle = \"Solo\"\nrules = { solo = 2 }\n".to_vec(),
    );
    std::sync::Arc::new(ExpansionPack::from_files(&files).expect("parses"))
}

#[test]
fn a_rest_point_restores_the_party_charges_and_opens_the_save_screen() {
    let mut w = field_world(10);
    let mut hms = w.party.roster.members[0].hp_mp_sp();
    (hms.hp_max, hms.hp_cur, hms.mp_max, hms.mp_cur) = (500, 12, 40, 3);
    w.party.roster.members[0].set_hp_mp_sp(hms);
    // No pack: a rest point is not a thing.
    assert!(!w.expansion_rest(0, true));
    w.activate_expansion(challenge_pack()).unwrap();
    w.party.money = 50;
    assert!(!w.expansion_rest(100, false), "a short purse refuses");
    assert_eq!(w.party.roster.members[0].hp_mp_sp().hp_cur, 12);
    assert_eq!(drain_toasts(&mut w), ["Resting here costs 100 G."]);
    assert!(w.expansion_rest(30, true));
    let hms = w.party.roster.members[0].hp_mp_sp();
    assert_eq!((hms.hp_cur, hms.mp_cur), (500, 40));
    assert_eq!(w.party.money, 20);
    assert_eq!(drain_toasts(&mut w), ["Rested for 30 G."]);
    assert!(
        w.scripted_menu_open_pending(),
        "the save prompt is a save point's menu press"
    );
    assert_eq!(w.menu_entry_context_kind(), Some(1));
}

#[test]
fn a_challenge_event_arms_and_starts_its_fight_or_says_why_not() {
    let mut w = field_world(10);
    w.activate_expansion(challenge_pack()).unwrap();
    assert!(
        !w.expansion_take_challenge("solo", false),
        "slot 2 is absent"
    );
    assert_eq!(
        drain_toasts(&mut w),
        ["Challenge refused: the party breaks its solo rule"]
    );
    // Armed, and its formation's fight started through the forced
    // encounter path.
    assert!(w.expansion_take_challenge("sprint", true));
    assert_eq!(
        w.expansion.quests.challenge.as_ref().map(|c| c.id.as_str()),
        Some("sprint")
    );
    assert_eq!(drain_toasts(&mut w), ["Challenge ready: Sprint"]);
}

#[test]
fn a_prerequisite_gate_reads_the_packs_flags() {
    use legaia_expansion_format::quest::Prerequisites;
    let mut w = field_world(10);
    let gate = Prerequisites {
        flags_clear: vec!["beaten".into()],
        ..Default::default()
    };
    assert!(w.expansion_prerequisites_hold(&Prerequisites::default()));
    assert!(!w.expansion_prerequisites_hold(&gate), "no pack: shut");
    w.activate_expansion(challenge_pack()).unwrap();
    assert!(w.expansion_prerequisites_hold(&gate));
    w.expansion_notify(QuestEvent::FlagSet("beaten".into()));
    assert!(!w.expansion_prerequisites_hold(&gate));
}

/// A save made with another version of the active pack: ids the pack no
/// longer defines are dropped with one notice, whether the save loads onto
/// the active pack or the pack activates after the load; what remains plays
/// and saves again cleanly.
#[test]
fn a_save_from_another_pack_version_drops_vanished_ids_with_a_notice() {
    use legaia_engine_expansion::quest::QuestState;
    let old = || {
        let mut p = ExpansionProgress {
            pack_id: "t".into(),
            pack_version: "0.9.0".into(),
            ..Default::default()
        };
        p.quests.insert(
            "retired_quest".into(),
            QuestState {
                status: QuestStatus::Active,
                objectives: vec![1],
                completions: 0,
                last_completed_at: 0,
            },
        );
        p.cards.collection.add("retired_card", 2);
        p.flags.flags.insert("open".into());
        p.encode()
    };

    // Loaded onto the active pack.
    let mut w = field_world(1);
    w.activate_expansion(pack()).expect("activates");
    w.expansion_load_block(Some(&old()));
    let toast = w.expansion_toast().expect("a notice").to_string();
    assert!(
        toast.contains("0.9.0") && toast.contains("2 saved entries"),
        "{toast}"
    );
    let r = w.expansion_compat_report().expect("a report");
    assert_eq!(
        (r.saved_version.as_str(), r.pack_version.as_str()),
        ("0.9.0", "1.0.0")
    );
    assert_eq!(r.dropped.len(), 2);
    assert!(w.expansion.quests.quest("retired_quest").is_none());
    assert_eq!(w.expansion.quests.progress.cards.collection.total(), 0);
    assert!(
        w.expansion.quests.progress.flags.is_set("open"),
        "flags are kept"
    );
    // The next save carries the reconciled progress under the new version.
    let saved = ExpansionProgress::decode(&w.expansion_save_block().unwrap()).unwrap();
    assert_eq!(saved.pack_version, "1.0.0");
    assert!(saved.quests.is_empty());

    // Loaded first, the pack activated after: the same outcome.
    let mut w = field_world(1);
    w.expansion_load_block(Some(&old()));
    w.activate_expansion(pack()).expect("activates");
    assert!(w.expansion_toast().is_some_and(|t| t.contains("dropped")));
    assert!(w.expansion.quests.quest("retired_quest").is_none());
    assert_eq!(w.expansion_dormant_pack_id(), None);
}

fn press_pack() -> std::sync::Arc<ExpansionPack> {
    let mut files = legaia_expansion_format::PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"p\"\nname = \"P\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert(
        "dialog.toml".into(),
        b"[lines.board]\ntext = \"\"\"\nThe board lists\nthe tables.\n\nSecond page.\"\"\"\n"
            .to_vec(),
    );
    files.insert(
        "maps/hall.toml".into(),
        br#"
name = "Hall"
base = { scene = { kind = "scene", name = "tower" } }
[tiles]
origin = [0, 0]
rows = [".....", ".....", "....."]
[tiles.legend]
"." = {}
[[spawns]]
id = "door"
tile = [0, 0]
[[triggers]]
id = "board"
rect = [2, 1, 2, 1]
event = { kind = "toast", line = "board" }
press = true
"#
        .to_vec(),
    );
    std::sync::Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

#[test]
fn a_press_zone_fires_on_the_button_and_a_toast_event_shows_the_first_page() {
    use legaia_engine_expansion::map::ActiveMap;
    use legaia_expansion_format::map::MapEvent;
    let mut w = field_world(1);
    w.actors[0].active = true;
    w.activate_expansion(press_pack()).expect("activates");
    let pack = w.expansion.pack.clone().unwrap();
    let def = std::sync::Arc::new(pack.maps["hall"].clone());
    let grid = std::sync::Arc::new(def.tile_grid().unwrap());
    w.expansion.map = Some(ActiveMap::new("hall", def, grid, (0, 0), |_| false));

    // Walking onto the zone fires nothing.
    stand_on(&mut w, 2, 1);
    let open = |_: &legaia_expansion_format::quest::Prerequisites| true;
    let map = w.expansion.map.as_mut().unwrap();
    map.step_gated(Some((1, 1)), open);
    map.step_gated(Some((2, 1)), open);
    assert!(
        map.pending_events().is_empty(),
        "a press zone is not walk-on"
    );

    // The button on it fires it.
    assert!(w.expansion_press_triggers());
    let fired = w.expansion.map.as_mut().unwrap().take_events();
    assert_eq!(fired.len(), 1);
    let MapEvent::Toast { line } = &fired[0].event else {
        panic!("{fired:?}");
    };
    // The toast is the line's first page on one row; no dialogue opens.
    assert!(w.expansion_toast_line(line));
    assert!(w.dialog.inline.is_none());
    assert_eq!(drain_toasts(&mut w), vec!["The board lists the tables."]);

    // Facing it from the next tile fires it too; facing away does not.
    stand_on(&mut w, 1, 1);
    let facing_at = (0..8u16).find(|&s| {
        w.actors[0].move_state.render_26 = ((s * 0x200 + 0x800) & 0xfff) as i16;
        w.expansion_press_triggers()
    });
    assert!(facing_at.is_some(), "some facing reaches the next tile");
    stand_on(&mut w, 4, 2);
    assert!(
        (0..8u16).all(|s| {
            w.actors[0].move_state.render_26 = ((s * 0x200 + 0x800) & 0xfff) as i16;
            !w.expansion_press_triggers()
        }),
        "two tiles away is out of reach"
    );
    // With no pack map active the button is the identity.
    w.expansion.map = None;
    assert!(!w.expansion_press_triggers());
}

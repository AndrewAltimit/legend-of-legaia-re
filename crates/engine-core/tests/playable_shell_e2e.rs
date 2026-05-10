//! End-to-end smoke test for the playable-shell loop.
//!
//! Drives a synthetic world through the boot UI → encounter trigger →
//! battle target pick → command commit → save → reload round trip,
//! asserting state survives across the cycle.

use legaia_art::Character;
use legaia_engine_core::battle_session::{
    BattlePhase, BattleSession, SessionInput, SessionSlotInfo, SubPhase,
};
use legaia_engine_core::battle_stats::StatRecord;
use legaia_engine_core::encounter::{
    EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
};
use legaia_engine_core::menu_runtime::MenuRuntime;
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::save_select::{
    SaveSelectMode, SaveSelectSession, SelectInput, SelectOutcome, SlotSnapshot,
};
use legaia_engine_core::target_picker::TargetKind;
use legaia_engine_core::title::{TitleInput, TitleOutcome, TitleSession};
use legaia_engine_core::world::{Actor, SceneMode, World};

fn build_world_with_party() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    for i in 0..3 {
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.mp = 30;
        w.ap_gauges[i] = legaia_engine_core::ap_gauge::ApGauge::with_base(8);
    }
    // Wire vanilla monster + formation tables.
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    // Money + a placeholder party so save_full produces valid data.
    w.money = 1234;
    w.story_flags = 0xCAFE;
    w.load_party(legaia_save::Party::zeroed(3));
    w.play_time_seconds = 4500;
    w
}

#[test]
fn title_session_advances_to_new_game() {
    let mut t = TitleSession::without_save_data();
    t.skip_fade_in();
    t.tick(TitleInput {
        start: true,
        ..Default::default()
    });
    // Cursor at 0 (NewGame, since Continue is disabled).
    let _ = t.tick(TitleInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(t.outcome(), Some(TitleOutcome::NewGame));
}

#[test]
fn save_select_load_outcome_round_trips() {
    let snaps = vec![
        SlotSnapshot {
            slot: 0,
            present: true,
            label: "Slot 0".into(),
            play_time_seconds: 1234,
            party_lv: 5,
            location: "Town01".into(),
            money: 100,
        },
        SlotSnapshot::empty(1),
    ];
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, snaps);
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(SelectOutcome::Loaded(0)));
}

#[test]
fn encounter_trigger_resolves_to_formation_def() {
    let mut t = EncounterTable::new("test_scene");
    t.set_trigger_rate(255);
    t.push(EncounterEntry::new(1, 100));
    let mut session = EncounterSession::new(EncounterTracker::new(t));
    session.transition_frames = 0;

    let mut w = build_world_with_party();
    w.mode = SceneMode::Field;
    w.set_encounter_session(Some(session));

    // Force a triggering step.
    let triggered = w.on_field_step();
    assert!(triggered, "fully-saturated rate should always trigger");
    // Tick the transition timer — `transition_frames=0` means the
    // session moves Idle → Transition → Triggered after one tick.
    w.tick_encounter();
    let roll = w.drain_encounter_formation().expect("triggered formation");
    assert_eq!(roll.formation_id, 1);

    let formation = w
        .formation_table
        .formation(roll.formation_id)
        .expect("vanilla formation 1");
    assert_eq!(formation.slots[0].monster_id, 1);
    let monster = w
        .monster_catalog
        .get(formation.slots[0].monster_id)
        .expect("vanilla goblin");
    assert!(monster.hp > 0);
    assert_eq!(monster.name, "Goblin");
    // End the battle so the session enters grace.
    w.end_encounter_battle();
}

#[test]
fn battle_session_target_picker_subphase_round_trip() {
    let mut s = BattleSession::new();
    s.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    for (i, name) in ["Vahn", "Noa", "Gala"].iter().enumerate() {
        s.set_slot_info(
            i as u8,
            SessionSlotInfo {
                name: (*name).into(),
                is_party: true,
                record: Some(StatRecord {
                    base_attack: 50,
                    base_udf: 30,
                    base_ldf: 25,
                    base_accuracy: 80,
                    base_evasion: 20,
                    ..Default::default()
                }),
                mp_max: 30,
            },
        );
    }
    // Two monsters present.
    s.set_slot_info(
        3,
        SessionSlotInfo {
            name: "Goblin".into(),
            is_party: false,
            record: Some(StatRecord::default()),
            mp_max: 0,
        },
    );
    s.set_slot_info(
        4,
        SessionSlotInfo {
            name: "Wolf".into(),
            is_party: false,
            record: Some(StatRecord::default()),
            mp_max: 0,
        },
    );
    s.set_monster_count(2);

    let mut w = build_world_with_party();
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    w.actors[4].battle.hp = 30;
    w.actors[4].battle.max_hp = 30;

    s.begin_round(&mut w);
    assert_eq!(s.phase(), BattlePhase::RoundIntro);

    // Force into CommandInput.
    let mut events = Vec::new();
    while !matches!(s.phase(), BattlePhase::CommandInput) {
        events.extend(s.tick(&mut w, SessionInput::default()));
    }
    assert_eq!(s.sub_phase(), SubPhase::CommandSelect);

    // Open the picker.
    let mut evs = Vec::new();
    s.open_target_picker(&w, TargetKind::SingleEnemy, 0, None, &mut evs);
    assert_eq!(s.sub_phase(), SubPhase::TargetPick);

    // Confirm.
    let confirm = SessionInput {
        cross: true,
        ..Default::default()
    };
    let evs = s.tick(&mut w, confirm);
    let confirmed = evs.iter().any(|e| {
        matches!(
            e,
            legaia_engine_core::battle_session::SessionEvent::TargetConfirmed { .. }
        )
    });
    assert!(confirmed, "expected TargetConfirmed event after cross");
    assert_eq!(s.sub_phase(), SubPhase::CommandSelect);
}

#[test]
fn save_full_load_full_round_trips_v2_extension() {
    // Save a populated world to LGSF v2, parse it back, and verify the
    // extension block survived.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut w = build_world_with_party();
    // Force a specific learned-art bit so we can verify the mask survives.
    w.tactical_arts.set_threshold(1);
    let _ = w.tactical_arts.notify_art_used(0, 5);
    // Add a saved chain.
    w.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 1,
        name: "Combo A".into(),
        sequence: vec![0x10, 0x20, 0x30],
    });

    let runtime = MenuRuntime::new(tempdir.path().to_path_buf());
    let path = runtime.save_to_slot(&mut w, 0).expect("save_to_slot");
    assert!(path.exists());
    let bytes = std::fs::read(&path).expect("read save");
    let parsed = legaia_save::SaveFile::parse(&bytes).expect("parse v2");
    assert_eq!(parsed.ext.story_flags, 0xCAFE);
    assert_eq!(parsed.ext.money, 1234);
    assert_eq!(parsed.ext_v2.play_time_seconds, 4500);
    assert!(!parsed.ext_v2.saved_chains.is_empty());
    let chain = &parsed.ext_v2.saved_chains[0];
    assert_eq!(chain.char_slot, 1);
    assert_eq!(chain.name, "Combo A");
    assert_eq!(chain.sequence, vec![0x10, 0x20, 0x30]);
    let ce0 = parsed
        .ext_v2
        .per_char
        .iter()
        .find(|(s, _)| *s == 0)
        .expect("per_char[0]");
    assert_eq!(ce0.1.learned_arts_mask & (1 << 5), 1 << 5);

    // Load into a fresh world.
    let mut w2 = World::new();
    while w2.actors.len() < 8 {
        w2.actors.push(Actor::default());
    }
    let _ = runtime.load_from_slot(&mut w2, 0).expect("load_from_slot");
    assert_eq!(w2.story_flags, 0xCAFE);
    assert_eq!(w2.money, 1234);
    assert_eq!(w2.play_time_seconds, 4500);
    assert!(!w2.saved_chains.is_empty());
    assert_eq!(w2.saved_chains[0].name, "Combo A");
    // Tactical-arts learned bit re-marked.
    assert!(w2.tactical_arts.is_learned(0, 5));
}

#[test]
fn target_picker_wiring_writes_active_target_and_admits_buffered_command() {
    // Closes the wiring gap noted in the post-#26 session-recommendations:
    // BattleSession opened the target picker but didn't route the resolved
    // target back into the runner's command queue or the actor's
    // `active_target` field. After the post-#26 batch 13 wiring, both
    // happen on `TargetConfirmed` (when called via `tick_command_input`'s
    // mutable-world path).
    use legaia_art::Command;
    let mut s = BattleSession::new();
    s.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    for (i, name) in ["Vahn", "Noa", "Gala"].iter().enumerate() {
        s.set_slot_info(
            i as u8,
            SessionSlotInfo {
                name: (*name).into(),
                is_party: true,
                record: Some(StatRecord {
                    base_attack: 50,
                    base_udf: 30,
                    base_ldf: 25,
                    base_accuracy: 80,
                    base_evasion: 20,
                    ..Default::default()
                }),
                mp_max: 30,
            },
        );
    }
    s.set_slot_info(
        3,
        SessionSlotInfo {
            name: "Goblin".into(),
            is_party: false,
            record: Some(StatRecord::default()),
            mp_max: 0,
        },
    );
    s.set_slot_info(
        4,
        SessionSlotInfo {
            name: "Wolf".into(),
            is_party: false,
            record: Some(StatRecord::default()),
            mp_max: 0,
        },
    );
    s.set_monster_count(2);
    let mut w = build_world_with_party();
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    w.actors[4].battle.hp = 30;
    w.actors[4].battle.max_hp = 30;
    s.begin_round(&mut w);
    // Force into CommandInput by ticking through the intro.
    while !matches!(s.phase(), BattlePhase::CommandInput) {
        let _ = s.tick(&mut w, SessionInput::default());
    }
    // Push a command with single-enemy targeting. Runner buffer stays
    // empty until the picker resolves.
    let ok = s.push_command_with_target(
        &mut w,
        Command::Right,
        legaia_engine_core::target_picker::TargetKind::SingleEnemy,
        0,
    );
    assert!(ok, "command should admit");
    assert_eq!(s.sub_phase(), SubPhase::TargetPick);
    // Cross — confirm target. Wiring writes `active_target` and admits
    // the buffered command into the runner queue.
    let confirm = SessionInput {
        cross: true,
        ..Default::default()
    };
    let evs = s.tick(&mut w, confirm);
    let confirmed = evs.iter().any(|e| {
        matches!(
            e,
            legaia_engine_core::battle_session::SessionEvent::TargetConfirmed { .. }
        )
    });
    assert!(confirmed);
    let pushed = evs.iter().any(|e| {
        matches!(
            e,
            legaia_engine_core::battle_session::SessionEvent::CommandPushed {
                slot: 0,
                command: Command::Right,
            }
        )
    });
    assert!(pushed, "buffered command should auto-admit on confirm");
    // Active-target write — the picker confirmed monster slot 0 (the first
    // alive enemy). The active_target byte is the *row-relative* slot
    // index emitted by `PickerOutcome::Single` (so `0` here, even though
    // the absolute battle-actor slot is 3).
    assert_eq!(w.actors[0].battle.active_target, 0);
    assert_eq!(s.runner.current_buffer(), &[Command::Right]);
    assert_eq!(s.sub_phase(), SubPhase::CommandSelect);
}

#[test]
fn target_picker_cancel_drops_buffered_command_keeps_runner_empty() {
    use legaia_art::Command;
    let mut s = BattleSession::new();
    s.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    for i in 0..3 {
        s.set_slot_info(
            i,
            SessionSlotInfo {
                name: format!("p{i}"),
                is_party: true,
                record: Some(StatRecord::default()),
                mp_max: 30,
            },
        );
    }
    s.set_slot_info(
        3,
        SessionSlotInfo {
            name: "G".into(),
            is_party: false,
            record: Some(StatRecord::default()),
            mp_max: 0,
        },
    );
    s.set_monster_count(1);
    let mut w = build_world_with_party();
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    s.begin_round(&mut w);
    while !matches!(s.phase(), BattlePhase::CommandInput) {
        let _ = s.tick(&mut w, SessionInput::default());
    }
    let ok = s.push_command_with_target(
        &mut w,
        Command::Down,
        legaia_engine_core::target_picker::TargetKind::SingleEnemy,
        0,
    );
    assert!(ok);
    let cancel = SessionInput {
        circle: true,
        ..Default::default()
    };
    let evs = s.tick(&mut w, cancel);
    let cancelled = evs.iter().any(|e| {
        matches!(
            e,
            legaia_engine_core::battle_session::SessionEvent::TargetCancelled
        )
    });
    assert!(cancelled);
    // Runner buffer empty — cancellation drops the buffered command.
    assert!(s.runner.current_buffer().is_empty());
}

#[test]
fn cutscene_str_routing_resolves_op_scenes_to_mv_files() {
    use legaia_engine_core::scene::{cutscene_label_for_str, cutscene_str_for};
    // 5 op* scenes + 1 ed* scene cover all 6 disc-side MV*.STR files.
    assert_eq!(cutscene_str_for("opdeene"), Some("MOV/MV1.STR"));
    assert_eq!(cutscene_str_for("edteien"), Some("MOV/MV6.STR"));
    // Round-trip every known pairing.
    for &(label, path) in legaia_engine_core::scene::FMV_CUTSCENE_SCENES.iter() {
        assert_eq!(cutscene_str_for(label), Some(path));
        assert_eq!(cutscene_label_for_str(path), Some(label));
    }
    // Non-FMV scenes return None.
    assert_eq!(cutscene_str_for("edlast"), None);
    assert_eq!(cutscene_str_for("town01"), None);
}

#[test]
fn full_loop_title_then_encounter_then_battle_then_save_then_load() {
    // 1. Title screen → New Game.
    let mut t = TitleSession::without_save_data();
    t.skip_fade_in();
    t.tick(TitleInput {
        start: true,
        ..Default::default()
    });
    t.tick(TitleInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(t.outcome(), Some(TitleOutcome::NewGame));

    // 2. Build world with vanilla tables and a guaranteed encounter rate.
    let mut w = build_world_with_party();
    w.mode = SceneMode::Field;
    let mut tbl = EncounterTable::new("smoke");
    tbl.set_trigger_rate(255);
    tbl.push(EncounterEntry::new(1, 100));
    let mut sess = EncounterSession::new(EncounterTracker::new(tbl));
    sess.transition_frames = 0;
    w.set_encounter_session(Some(sess));

    // 3. Step → trigger.
    assert!(w.on_field_step(), "guaranteed trigger");
    w.tick_encounter();
    let roll = w.drain_encounter_formation().expect("triggered");
    assert_eq!(roll.formation_id, 1);

    // 4. Resolve a battle session against the formation.
    let formation = w
        .formation_table
        .formation(roll.formation_id)
        .expect("vanilla formation")
        .clone();
    let mut bs = BattleSession::new();
    bs.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    for i in 0..3 {
        bs.set_slot_info(
            i,
            SessionSlotInfo {
                name: format!("p{i}"),
                is_party: true,
                record: Some(StatRecord::default()),
                mp_max: 30,
            },
        );
    }
    for (i, slot) in formation.slots.iter().enumerate() {
        let monster = w.monster_catalog.get(slot.monster_id).expect("monster def");
        let actor_idx = 3 + i;
        w.actors[actor_idx].battle.hp = monster.hp;
        w.actors[actor_idx].battle.max_hp = monster.hp;
        bs.set_slot_info(
            (3 + i) as u8,
            SessionSlotInfo {
                name: monster.name.clone(),
                is_party: false,
                record: Some(StatRecord::default()),
                mp_max: 0,
            },
        );
    }
    bs.set_monster_count(formation.slots.len() as u8);
    bs.begin_round(&mut w);
    assert_eq!(bs.phase(), BattlePhase::RoundIntro);

    // End the encounter (skip the action SM — the runner is already
    // exercised in unit tests).
    w.end_encounter_battle();

    // 5. Save the world to a temp slot.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let runtime = MenuRuntime::new(tempdir.path().to_path_buf());
    let path = runtime.save_to_slot(&mut w, 0).expect("save");

    // 6. Load it back into a fresh world and verify state survived.
    let mut w2 = World::new();
    while w2.actors.len() < 8 {
        w2.actors.push(Actor::default());
    }
    let _ = runtime.load_from_slot(&mut w2, 0).expect("load");
    assert_eq!(w2.money, 1234);
    assert_eq!(w2.story_flags, 0xCAFE);
    assert_eq!(w2.play_time_seconds, 4500);
    assert!(path.exists());
}

//! Disc-free check of the battle **arts-voice shout cue** emission: executing
//! a Tactical Art through the live player-driven Arts menu queues exactly one
//! [`BattleShoutCue`] carrying the caster's character slot and the matched art
//! record's action constant, on the art's animation-start frame - and a
//! synthetic art with no matched record queues none (the retail silent-art
//! degradation). The disc-gated sibling
//! (`engine-shell/tests/arts_shout_battle.rs`) carries the cue through the XA
//! clip bank into the audio mix.
//!
//! [`BattleShoutCue`]: legaia_engine_core::battle_events::BattleShoutCue

use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

fn stage_somersault(w: &mut World) {
    let action = legaia_art::ActionConstant::from_byte(0x27).unwrap();
    let rec = legaia_art::ArtRecord {
        action,
        commands: vec![legaia_art::Command::Up],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power: vec![legaia_art::power::PowerByte::from_byte(0x16); 2],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: legaia_art::EnemyEffect::None,
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    };
    w.set_art_record(legaia_art::Character::Vahn, action, rec);
}

fn build_world(with_record: bool) -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 90);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    if with_record {
        stage_somersault(&mut w);
    }
    w.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "Som".into(),
        sequence: vec![4], // Up
    });

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("shout_cue_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;
    w
}

fn drive_art_and_collect_shouts(
    w: &mut World,
) -> Vec<legaia_engine_core::battle_events::BattleShoutCue> {
    use legaia_engine_core::battle_input::BattleCommand;

    let up = InputState::mask_of([PadButton::Up]);
    let mut entered = false;
    for _ in 0..6000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            entered = true;
            break;
        }
    }
    assert!(entered, "walking should trigger Field -> Battle");

    let mut shouts = Vec::new();
    let mut press = true;
    for _ in 0..4000 {
        let pad = if !press {
            0
        } else if w.battle_arts_menu.is_some() {
            InputState::mask_of([PadButton::Cross])
        } else if let Some(cmd) = w.battle_command.as_ref() {
            if cmd.menu_command() == Some(BattleCommand::Arts) {
                InputState::mask_of([PadButton::Cross])
            } else {
                InputState::mask_of([PadButton::Down])
            }
        } else {
            0
        };
        w.set_pad(pad);
        press = !press;
        w.tick();
        shouts.extend(w.drain_battle_shout_cues());
        if w.mode == SceneMode::Field && w.last_battle_rewards.is_some() {
            break;
        }
    }
    shouts
}

#[test]
fn matched_art_emits_one_shout_cue_with_its_action_constant() {
    let mut w = build_world(true);
    let shouts = drive_art_and_collect_shouts(&mut w);
    assert_eq!(shouts.len(), 1, "one cue per executed art: {shouts:?}");
    assert_eq!(shouts[0].cslot, 0, "Vahn = character slot 0 (XA2 bank)");
    assert_eq!(shouts[0].action, 0x27, "the matched record's constant");
}

#[test]
fn synthetic_art_without_record_emits_no_shout_cue() {
    let mut w = build_world(false);
    let shouts = drive_art_and_collect_shouts(&mut w);
    assert!(shouts.is_empty(), "synthetic art stays silent: {shouts:?}");
}

//! In-app verification: a Super Art is selectable and executes through the
//! **live** player-driven battle menu - not just the row builder.
//!
//! Drives the same `World::tick` path the windowed app uses: walk into a
//! battle, navigate the command menu to Arts, open the Arts submenu, and
//! confirm a saved chain that performs Vahn's Tri-Somersault combo
//! (Somersault → Cyclone → Somersault). Asserts the live menu row carries the
//! Super name (`ArtRow::super_art`) and that selecting it deals damage and
//! resolves the battle. Disc-free; runs in CI.

use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

fn stage_vahn_art(w: &mut World, byte: u8, cmd: legaia_art::Command, strikes: usize) {
    let action = legaia_art::ActionConstant::from_byte(byte).unwrap();
    let rec = legaia_art::ArtRecord {
        action,
        commands: vec![cmd],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power: vec![legaia_art::power::PowerByte::from_byte(0x16); strikes],
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

fn build_world() -> World {
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

    // Vahn's Tri-Somersault = Somersault (Art27) -> Cyclone (Art1F) ->
    // Somersault (Art27). Give each component art a one-direction command so a
    // flat chain recognizes the sequence.
    stage_vahn_art(&mut w, 0x27, legaia_art::Command::Up, 2);
    stage_vahn_art(&mut w, 0x1F, legaia_art::Command::Down, 1);
    // Chain Up Down Up (Left=1 Right=2 Down=3 Up=4).
    w.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "TriSom".into(),
        sequence: vec![4, 3, 4],
    });

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("super_art_live_test");
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

fn monster_hp_total(w: &World) -> u32 {
    (w.party_count as usize..w.actors.len())
        .map(|i| w.actors[i].battle.hp as u32)
        .sum()
}

#[test]
fn live_arts_menu_selects_and_fires_a_super() {
    use legaia_engine_core::battle_input::BattleCommand;

    let mut w = build_world();
    let up = InputState::mask_of([PadButton::Up]);

    // --- Walk into a battle. ---
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
    assert!(w.battle_command.is_some(), "battle opens a command session");
    let hp_before = monster_hp_total(&w);
    assert!(hp_before > 0, "monster alive on entry");

    // --- Drive command -> Arts -> select Super row -> confirm target. ---
    // Edge-triggered: emit a button only on alternate "press" frames, choosing
    // it from the live menu state so navigation is deterministic.
    let mut press = true;
    let mut saw_super_row = false;
    let mut resolved = false;
    for _ in 0..4000 {
        let pad = if !press {
            0
        } else if let Some(menu) = w.battle_arts_menu.as_ref() {
            // Arts submenu open. In the Select phase the row under the cursor
            // must be the recognized Super; confirm it, then confirm target.
            if menu
                .menu_art()
                .is_some_and(|row| row.super_art == Some("Tri-Somersault"))
            {
                saw_super_row = true;
            }
            InputState::mask_of([PadButton::Cross])
        } else if let Some(cmd) = w.battle_command.as_ref() {
            // Command menu: move the cursor onto Arts, then confirm it.
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
        if w.mode == SceneMode::Field && w.last_battle_rewards.is_some() {
            resolved = true;
            break;
        }
    }

    assert!(
        saw_super_row,
        "the live Arts menu row for the chain must be flagged as the Super"
    );
    assert!(
        resolved,
        "selecting the Super must execute and resolve the battle"
    );
    assert_eq!(w.mode, SceneMode::Field, "return to field after the wipe");
    let rewards = w
        .last_battle_rewards
        .as_ref()
        .expect("victory records rewards");
    assert!(rewards.xp > 0, "victory grants XP: {rewards:?}");
}

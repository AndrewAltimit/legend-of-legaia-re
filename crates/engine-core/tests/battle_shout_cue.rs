//! Disc-free check of the battle **arts-voice shout cue** emission: executing
//! Tactical Arts through the live player-driven Arts command input queues one
//! [`BattleShoutCue`] **per art the turn performs**, each carrying the caster's
//! character slot and that art's record action constant - and a synthetic
//! entry with no matched record queues none (the retail silent-art
//! degradation). The disc-gated sibling
//! (`engine-shell/tests/arts_shout_battle.rs`) carries the cue through the XA
//! clip bank into the audio mix.
//!
//! The pad path here is the one a player actually walks under the retail
//! model: command menu → Arts → per-press directional entry → the entry's own
//! end (auto on the exhausting press, or Cross once the sequence is in) →
//! Begin → target. There is no saved-chain row to confirm; a chain is
//! something the player *types* with the d-pad, which is why a driver that
//! only presses Cross reaches no art at all.
//!
//! [`BattleShoutCue`]: legaia_engine_core::battle_events::BattleShoutCue

use legaia_engine_core::arts_command_input::ArtsInputScreen;
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

/// Walk into the encounter (the driver's precondition), asserting it fires.
fn walk_into_battle(w: &mut World) {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            return;
        }
    }
    panic!("walking should trigger Field -> Battle");
}

/// Drive the battle out from the pad, taking **Arts** on the first party turn
/// and entering `entry` one direction per press, then Attack on any later
/// turn so the collected cues measure exactly one arts entry.
///
/// Shout cues are drained every tick - battle teardown clears the queue, so a
/// drain that only runs at the end sees nothing.
fn drive_arts_entry_and_collect_shouts(
    w: &mut World,
    entry: &[PadButton],
) -> Vec<legaia_engine_core::battle_events::BattleShoutCue> {
    use legaia_engine_core::battle_input::BattleCommand;

    walk_into_battle(w);

    let mut shouts = Vec::new();
    let mut press = true;
    let mut next_dir = 0usize;
    let mut arts_turns = 0usize;
    let mut opened_input = false;
    for _ in 0..4000 {
        let pad = if !press {
            // Edge-triggered: every session reads `just_pressed`, so a press
            // frame has to be followed by a release frame.
            0
        } else if let Some(view) = w.arts_input_view() {
            opened_input = true;
            match view.phase {
                ArtsInputScreen::Entering => {
                    if next_dir < entry.len() {
                        let dir = entry[next_dir];
                        next_dir += 1;
                        InputState::mask_of([dir])
                    } else {
                        // Sequence is in and the pool hasn't exhausted itself:
                        // Cross ends the entry early (the port's disclosed
                        // convenience over retail's auto-end-only rule).
                        InputState::mask_of([PadButton::Cross])
                    }
                }
                // Review → Begin|Reselect (cursor 0 = Begin) → target picker:
                // Cross walks all three.
                _ => InputState::mask_of([PadButton::Cross]),
            }
        } else if let Some(cmd) = w.battle_command.as_ref() {
            if arts_turns == 0 {
                // First party turn: walk the cursor onto Arts and take it.
                if cmd.menu_command() == Some(BattleCommand::Arts) {
                    InputState::mask_of([PadButton::Cross])
                } else {
                    InputState::mask_of([PadButton::Down])
                }
            } else if cmd.menu_command() == Some(BattleCommand::Attack) {
                InputState::mask_of([PadButton::Cross])
            } else {
                InputState::mask_of([PadButton::Up])
            }
        } else {
            0
        };
        let input_was_open = w.arts_input_active();
        w.set_pad(pad);
        press = !press;
        w.tick();
        if input_was_open && !w.arts_input_active() {
            arts_turns += 1;
        }
        shouts.extend(w.drain_battle_shout_cues());
        if w.mode == SceneMode::Field && w.last_battle_rewards.is_some() {
            break;
        }
    }
    assert!(
        opened_input,
        "the Arts command must open the per-press input session"
    );
    assert_eq!(arts_turns, 1, "exactly one arts entry was driven");
    // Non-vacuity: an entry that aborted (or a loop that spun out) would leave
    // the world in Battle, and a shout-free expectation would pass for the
    // wrong reason.
    assert_eq!(w.mode, SceneMode::Field, "the battle resolved");
    assert!(
        w.last_battle_rewards.is_some(),
        "the party won, so an art actually landed"
    );
    shouts
}

#[test]
fn matched_art_emits_one_shout_cue_with_its_action_constant() {
    let mut w = build_world(true);
    // One Up press = Somersault's whole command string.
    let shouts = drive_arts_entry_and_collect_shouts(&mut w, &[PadButton::Up]);
    assert_eq!(shouts.len(), 1, "one cue per executed art: {shouts:?}");
    assert_eq!(shouts[0].cslot, 0, "Vahn = character slot 0 (XA2 bank)");
    assert_eq!(shouts[0].action, 0x27, "the matched record's constant");
}

#[test]
fn synthetic_art_without_record_emits_no_shout_cue() {
    let mut w = build_world(false);
    let shouts = drive_arts_entry_and_collect_shouts(&mut w, &[PadButton::Up]);
    assert!(shouts.is_empty(), "synthetic art stays silent: {shouts:?}");
}

/// Retail's entry runs until the AP pool is spent, so performing **several**
/// arts in one turn is the ordinary case, not an edge one - and each art is a
/// separately staged animation whose materialiser calls the cue selector. A
/// turn that performs three Somersaults must therefore request three shouts;
/// keying the cue off the entry instead of off the art silently mutes every
/// art after the first.
///
/// Three presses is also the pool's own end: at the disc-free fallback pool
/// (100 AP) and the favored-class press cost (0x1E), the third press leaves
/// nothing affordable and the entry auto-ends, so this walks retail's
/// no-confirm path end to end.
#[test]
fn every_art_in_a_multi_art_entry_gets_its_own_shout_cue() {
    let mut w = build_world(true);
    let shouts =
        drive_arts_entry_and_collect_shouts(&mut w, &[PadButton::Up, PadButton::Up, PadButton::Up]);
    assert_eq!(
        shouts.len(),
        3,
        "one cue per performed art, not one per turn: {shouts:?}"
    );
    for cue in &shouts {
        assert_eq!(cue.cslot, 0, "Vahn = character slot 0 (XA2 bank)");
        assert_eq!(cue.action, 0x27, "each performed art keys its own cue");
    }
}

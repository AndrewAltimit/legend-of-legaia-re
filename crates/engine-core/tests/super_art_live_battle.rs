//! In-app verification: a Super Art is triggerable and executes through the
//! **live** player-driven battle input - not just the row builder.
//!
//! Drives the same `World::tick` path the windowed app uses: walk into a
//! battle, navigate the command menu to Arts, and *type* Vahn's Tri-Somersault
//! combo (Somersault → Cyclone → Somersault = Up Down Up) into the retail
//! per-press Arts command input. Three presses is also the AP pool's own end
//! at the disc-free fallback (100 AP, favored-class cost 0x1E), so the entry
//! auto-ends on the third press exactly like retail's `0x50 -> 0x5A` edge -
//! no confirm involved.
//!
//! What proves the Super fired is the **shout cue's action constant**: the
//! Super path keys the cue on the combo's finisher (`0x2B`, Tri-Somersault's
//! `replace` tail), while three unrecognized-as-a-Super Somersaults would key
//! three cues on `0x27`. The test then asserts the entry deals damage and
//! resolves the battle. Disc-free; runs in CI.

use legaia_engine_core::arts_command_input::ArtsInputScreen;
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
    // flat Up-Down-Up entry recognizes the sequence.
    stage_vahn_art(&mut w, 0x27, legaia_art::Command::Up, 2);
    stage_vahn_art(&mut w, 0x1F, legaia_art::Command::Down, 1);

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
fn live_arts_input_types_and_fires_a_super() {
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

    // --- Drive command -> Arts -> type Up Down Up -> Begin -> target. ---
    // Edge-triggered: emit a button only on alternate "press" frames, choosing
    // it from the live session state so navigation is deterministic.
    let combo = [PadButton::Up, PadButton::Down, PadButton::Up];
    let mut next_dir = 0usize;
    let mut press = true;
    let mut opened_input = false;
    let mut auto_ended_without_confirm = false;
    let mut arts_turns = 0usize;
    let mut shouts = Vec::new();
    let mut resolved = false;
    for _ in 0..4000 {
        let pad = if !press {
            0
        } else if let Some(view) = w.arts_input_view() {
            opened_input = true;
            match view.phase {
                ArtsInputScreen::Entering => {
                    if next_dir < combo.len() {
                        let dir = combo[next_dir];
                        next_dir += 1;
                        InputState::mask_of([dir])
                    } else {
                        InputState::mask_of([PadButton::Cross])
                    }
                }
                other => {
                    // The pool ended the entry itself once the combo was in -
                    // retail's `0x50 -> 0x5A` edge, reached with no confirm.
                    if next_dir == combo.len() && other == ArtsInputScreen::Review {
                        auto_ended_without_confirm = true;
                    }
                    InputState::mask_of([PadButton::Cross])
                }
            }
        } else if let Some(cmd) = w.battle_command.as_ref() {
            // Retail's open flow: `Begin` on the round prompt, the ring's
            // `Attack` arm, then the `Auto | Command` prompt - `Command` is
            // the directional arts entry, `Auto` the plain swing. Cross takes
            // whatever the cursor sits on; the other presses are the spatial
            // seatings - Left onto the `Attack` arm, Left/Right onto the
            // `Auto`/`Command` chip.
            use legaia_engine_core::battle_input::{AttackMode, CommandPhase};
            match cmd.phase {
                CommandPhase::Menu { .. } if cmd.menu_command() != Some(BattleCommand::Attack) => {
                    InputState::mask_of([PadButton::Left])
                }
                CommandPhase::AttackMode { .. } => {
                    let want = if arts_turns == 0 {
                        AttackMode::Command
                    } else {
                        AttackMode::Auto
                    };
                    if cmd.attack_mode() == Some(want) {
                        InputState::mask_of([PadButton::Cross])
                    } else if want == AttackMode::Auto {
                        InputState::mask_of([PadButton::Left])
                    } else {
                        InputState::mask_of([PadButton::Right])
                    }
                }
                _ => InputState::mask_of([PadButton::Cross]),
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
            resolved = true;
            break;
        }
    }

    assert!(
        opened_input,
        "the Arts command must open the per-press input session"
    );
    assert!(
        auto_ended_without_confirm,
        "three presses spend the pool, so the entry ends by itself"
    );
    assert_eq!(arts_turns, 1, "exactly one arts entry was driven");
    // The Super replaced the recognized art tail: one cue keyed on the combo's
    // finisher constant. Without the Super match the same three directions
    // would perform three plain Somersaults and queue three `0x27` cues.
    assert_eq!(
        shouts.len(),
        1,
        "a Super replacement is one performed finisher: {shouts:?}"
    );
    assert_eq!(shouts[0].cslot, 0, "Vahn = character slot 0 (XA2 bank)");
    assert_eq!(
        shouts[0].action, 0x2B,
        "Tri-Somersault's finisher constant, not a component art's"
    );
    assert!(
        resolved,
        "the typed Super must execute and resolve the battle"
    );
    assert_eq!(w.mode, SceneMode::Field, "return to field after the wipe");
    let rewards = w
        .last_battle_rewards
        .as_ref()
        .expect("victory records rewards");
    assert!(rewards.xp > 0, "victory grants XP: {rewards:?}");
}

//! In-app verification: a Super Art is triggerable and executes through the
//! **live** player-driven battle input - not just the row builder.
//!
//! Drives the same `World::tick` path the windowed app uses: walk into a
//! battle, navigate the command menu to Arts, and *type* Vahn's Tri-Somersault
//! input (`↑↓↑↑↑↓↑` - Somersault `↑↓↑`, Cyclone `↓↑↑↑` and Somersault again,
//! sharing arrows) into the retail per-press Arts command input. Seven presses
//! is also the AP pool's own end (100 AP at the disc-free fallback, swing
//! costs seeded at 14), so the entry auto-ends on the seventh press exactly
//! like retail's `0x50 -> 0x5A` edge - no confirm involved.
//!
//! What proves the Super fired is the **shout cue list**: the byte-exact
//! queue-builder tokenizes the input to the find row `19 27 0F 19 1F 0E 19 27`
//! and the tail-replace rewrites its closing `19 27` to `1A 2B 2B 2B`, so the
//! constants the queue stages - one shout each - read `27 1F 2B 2B 2B`;
//! without the Super match the same seven presses would close on a third
//! `0x27`. The test then asserts the entry deals damage and resolves the
//! battle. Disc-free; runs in CI.

use legaia_engine_core::arts_command_input::ArtsInputScreen;
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

fn stage_vahn_art(w: &mut World, byte: u8, cmds: &[legaia_art::Command], strikes: usize) {
    let action = legaia_art::ActionConstant::from_byte(byte).unwrap();
    let rec = legaia_art::ArtRecord {
        action,
        commands: cmds.to_vec(),
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
    // The zeroed records seed HP 0 / no seat, so they go in FIRST: retail's
    // member walk (`FUN_801DB81C`) hands no ring to a member with no HP.
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 90);
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());

    // Vahn's Tri-Somersault = Somersault (Art27) -> Cyclone (Art1F) ->
    // Somersault (Art27), with the component arts' REAL command strings
    // (Somersault `↑↓↑`, Cyclone `↓↑↑↑`): the queue-builder is byte-exact,
    // and only the retail input `↑↓↑↑↑↓↑` tokenizes to the Super's find row
    // `19 27 0F 19 1F 0E 19 27` (`legaia_art::tokenize`).
    {
        use legaia_art::Command::{Down, Up};
        stage_vahn_art(&mut w, 0x27, &[Up, Down, Up], 2);
        stage_vahn_art(&mut w, 0x1F, &[Down, Up, Up, Up], 1);
    }
    // A Super's find row is written with `0x19` starters, and the builder
    // writes `0x1A` over the starter of an art this very performance learns
    // (`FUN_801EFBFC` verdict 2), so the component arts must already be
    // known - retail's own "no NEW arts in a Super" rule.
    w.tactical_arts.mark_known(0, 0x27);
    w.tactical_arts.mark_known(0, 0x1F);
    // Seven presses on the disc-free 100-AP pool: 14 each spends 98, the
    // eighth is unaffordable and the entry ends by itself.
    w.battle_swing_costs[0] = [14; 4];

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
    // The retail Tri-Somersault input (walkthrough string, capture-pinned
    // queue `0F 0E 19 27 0F 19 1F 0E 1A 2B 2B 2B` after the replacement).
    let combo = [
        PadButton::Up,
        PadButton::Down,
        PadButton::Up,
        PadButton::Up,
        PadButton::Up,
        PadButton::Down,
        PadButton::Up,
    ];
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
        "seven presses spend the pool, so the entry ends by itself"
    );
    assert_eq!(arts_turns, 1, "exactly one arts entry was driven");
    // The Super replaced the recognized tail: the queue's art constants are
    // the replace row's - the leading Somersault and Cyclone survive as the
    // find prefix, the closing `19 27` is rewritten to `1A 2B 2B 2B` - and
    // the shout list is one cue per art constant the queue stages, exactly
    // as the materialiser is called once per commit. Without the Super
    // match the same seven directions would end on a third `0x27`.
    assert_eq!(
        shouts.iter().map(|s| s.action).collect::<Vec<_>>(),
        vec![0x27, 0x1F, 0x2B, 0x2B, 0x2B],
        "the replaced queue's constants, Tri-Somersault x3 closing: {shouts:?}"
    );
    assert!(
        shouts.iter().all(|s| s.cslot == 0),
        "Vahn = character slot 0 (XA2 bank)"
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

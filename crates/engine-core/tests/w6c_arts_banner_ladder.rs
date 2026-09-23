//! Ladder for the Arts announcement banner, end to end through the live
//! battle: a pad-typed Super Art's SpecialStarter commit raises the banner
//! byte `ctx[+0x28B]` (the raiser is SCUS `FUN_8004AD80`, ported as
//! `banner_on_starter_commit`), `World::tick_arts_banner` walks its clock
//! (`FUN_801E2524`), and `World::battle_arts_banner_quads` - the one read the
//! native redraw and the play page's battle compose both draw through - emits
//! the banner's textured quads (`FUN_801E2650`, `flash_quads`).
//!
//! The union reached the tick on every battle ladder and never once reached
//! the quad emitter, because no ladder's battle commits a SpecialStarter:
//! they swing, cast and flee. The drive below is the disc-free Super drive of
//! `super_art_live_battle.rs` (same world, same seven presses), with the
//! per-frame host read added where a host makes it.
//!
//! Disc-free; runs in CI.

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
    w.party.party_count = 3;
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
    w.party.tactical_arts.mark_known(0, 0x27);
    w.party.tactical_arts.mark_known(0, 0x1F);
    // Seven presses on the disc-free 100-AP pool: 14 each spends 98, the
    // eighth is unaffordable and the entry ends by itself.
    w.battle.swing_costs[0] = [14; 4];

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.locomotion.camera_azimuth = 0;

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
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;
    w
}

fn monster_hp_total(w: &World) -> u32 {
    (w.party.party_count as usize..w.actors.len())
        .map(|i| w.actors[i].battle.hp as u32)
        .sum()
}

#[test]
fn a_typed_super_raises_the_banner_and_the_host_read_draws_it() {
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
    assert!(w.battle.command.is_some(), "battle opens a command session");
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
    let mut banner_frames = 0usize;
    let mut banner_quads_max = 0usize;
    let mut banner_stages = std::collections::BTreeSet::new();
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
        } else if let Some(cmd) = w.battle.command.as_ref() {
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
        // The read both hosts draw through, once per frame, exactly where
        // the native redraw and the page's battle compose call it.
        let quads = w.battle_arts_banner_quads();
        if !quads.is_empty() {
            banner_frames += 1;
            banner_quads_max = banner_quads_max.max(quads.len());
            banner_stages.insert(w.battle_ctx.arts_banner_stage);
        }
        if input_was_open && !w.arts_input_active() {
            arts_turns += 1;
        }
        shouts.extend(w.drain_battle_shout_cues());
        if w.mode == SceneMode::Field && w.battle.last_rewards.is_some() {
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
    assert!(
        monster_hp_total(&w) < hp_before || resolved,
        "the Super dealt damage"
    );
    assert!(
        banner_frames > 0,
        "the Super's SpecialStarter commit must raise ctx[+0x28B] and the \
         per-frame host read must emit its quads"
    );
    // Two textured quads per layer, up to four layers on a live frame.
    assert!(
        banner_quads_max >= 2,
        "a live banner frame carries at least one layer's two quads"
    );
    assert!(
        banner_stages.iter().all(|s| (1..=4).contains(s)),
        "only a live stage (1..=4) draws: {banner_stages:?}"
    );
    eprintln!(
        "[w6c] banner drawn on {banner_frames} frames, stages {banner_stages:?}, \
         up to {banner_quads_max} quads, {} shout cues",
        shouts.len()
    );
    assert!(
        resolved,
        "the typed Super must execute and resolve the battle"
    );
}

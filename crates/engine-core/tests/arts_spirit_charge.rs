//! **An art body is paid out of Spirit, and a plain swing is not** - the
//! second half of the two-gauge split, driven by pad through the live loop.
//!
//! Retail runs two currencies over one Arts turn and they are easy to
//! conflate (`docs/subsystems/arts-command-gauge.md` calls that the standing
//! trap in the area). The *directions* are spent out of the per-turn command
//! pool `ctx+0x6DC`, seeded from the actor's AGL, at the per-`(character,
//! weapon)` `+0x74` byte each. The *art* is not paid from that pool at all:
//! the queue-builder `FUN_801EED1C` computes a Spirit price from three code
//! immediates - **retail stores no per-art AP cost anywhere on the disc** -
//! accrues it into `actor[+0x224]`, and the battle-action cleanup arm
//! subtracts it from the Spirit gauge `actor[+0x170]` at `0x801E5D74`.
//!
//! The port charged the swings and comped the art body, so an Arts turn was
//! free past its arrows. This drives the real input path (no engine call
//! reaches into the session) and reads the gauge every frame, because the
//! charge is one step in a series a landing hit also *adds* to: what is
//! asserted is the size of the drop, not the value afterwards.
//!
//! Disc-free; runs in CI.

use legaia_engine_core::arts_command_input::ArtsInputScreen;
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

/// Vahn's Somersault: three commands, and the only art staged - so it is
/// visit ordinal `0`, multiplier `11`, price `11 x 3`.
const SOMERSAULT: u8 = 0x27;
const SOMERSAULT_COMMANDS: usize = 3;
const EXPECTED_SPIRIT_CHARGE: u16 = 11 * SOMERSAULT_COMMANDS as u16;
/// Seeded gauge. Above the charge so the drop is visible, below the `100`
/// ceiling so an accrual cannot mask it by clamping.
const SPIRIT_SEED: u16 = 90;

fn stage_somersault(w: &mut World) {
    use legaia_art::Command::{Down, Up};
    let action = legaia_art::ActionConstant::from_byte(SOMERSAULT).unwrap();
    let rec = legaia_art::ArtRecord {
        action,
        commands: vec![Up, Down, Up],
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

fn build_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 90);
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    stage_somersault(&mut w);
    w.tactical_arts.mark_known(0, SOMERSAULT);
    // Three presses spend 99 of the disc-free 100-AP pool and a fourth is
    // unaffordable, so the entry auto-ends on the third exactly - retail's
    // `0x50 -> 0x5A` edge, reached with no confirm to time.
    w.battle_swing_costs[0] = [33; 4];

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("arts_spirit_charge_test");
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

/// Drive one party turn and return every **negative** step the leader's
/// Spirit gauge took while it ran. `combo` empty = take the ring's `Auto`
/// arm (a plain two-swing attack); otherwise take `Command` and type the
/// combo, then confirm.
fn spirit_drops_over_one_turn(w: &mut World, combo: &[PadButton]) -> (Vec<i32>, Vec<u8>) {
    use legaia_engine_core::battle_input::{AttackMode, BattleCommand, CommandPhase};

    w.actors[0].battle.spirit_gauge = SPIRIT_SEED;
    let mut prev = i32::from(w.actors[0].battle.spirit_gauge);
    let mut drops = Vec::new();
    let mut next_dir = 0usize;
    let mut press = true;
    let mut turns = 0usize;
    let mut settle = 0usize;
    let mut shouts: Vec<u8> = Vec::new();

    for _ in 0..4000 {
        let pad = if !press {
            0
        } else if let Some(view) = w.arts_input_view() {
            match view.phase {
                ArtsInputScreen::Entering if next_dir < combo.len() => {
                    let dir = combo[next_dir];
                    next_dir += 1;
                    InputState::mask_of([dir])
                }
                _ => InputState::mask_of([PadButton::Cross]),
            }
        } else if let Some(cmd) = w.battle_command.as_ref() {
            match cmd.phase {
                CommandPhase::Menu { .. } if cmd.menu_command() != Some(BattleCommand::Attack) => {
                    InputState::mask_of([PadButton::Left])
                }
                CommandPhase::AttackMode { .. } => {
                    // Only the FIRST entry is the arts one. Every later
                    // member (and every later round) takes `Auto`, because a
                    // second `Command` would reopen the entry with the combo
                    // already spent, confirm an empty buffer, and park the
                    // round - which reads as "the art never fired".
                    let want = if combo.is_empty() || turns > 0 {
                        AttackMode::Auto
                    } else {
                        AttackMode::Command
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
            turns += 1;
        }
        shouts.extend(w.drain_battle_shout_cues().into_iter().map(|c| c.action));
        // Battle teardown zeroes the gauge, so a delta sampled on the frame
        // the mode changes is the wipe, not a charge. Leave first.
        if w.mode != SceneMode::Battle {
            break;
        }
        let now = i32::from(w.actors[0].battle.spirit_gauge);
        if now < prev {
            drops.push(now - prev);
        }
        prev = now;
        // Stop at the first charge. The dispatch that runs the art is a whole
        // round away from the entry closing - retail commits every member
        // before anyone acts - so the window cannot be a fixed frame count;
        // and past the first one a second Arts turn would add a second art
        // price and stop this being a per-turn claim. The empty-combo arm has
        // no charge to wait for and simply runs the budget out.
        if !drops.is_empty() {
            settle += 1;
            if settle > 2 {
                break;
            }
        }
    }
    if !combo.is_empty() {
        assert_eq!(turns, 1, "exactly one arts entry was driven");
    }
    (drops, shouts)
}

/// A typed art charges its body out of Spirit, once, at the builder's price.
#[test]
fn a_committed_art_charges_its_body_out_of_spirit() {
    let mut w = build_world();
    walk_into_battle(&mut w);
    // Somersault's own command string, so the tokenizer matches it and the
    // queue stages the art rather than three plain swings.
    let action = legaia_art::ActionConstant::from_byte(SOMERSAULT).unwrap();
    assert_eq!(
        w.art_records
            .get(&(legaia_art::Character::Vahn, action))
            .map(|r| r.commands.len()),
        Some(SOMERSAULT_COMMANDS),
        "the staged art is the {SOMERSAULT_COMMANDS}-command one the price assumes"
    );
    let (drops, shouts) =
        spirit_drops_over_one_turn(&mut w, &[PadButton::Up, PadButton::Down, PadButton::Up]);
    assert_eq!(
        shouts,
        vec![SOMERSAULT],
        "the typed string must tokenize to the art, else there is no body to charge"
    );
    assert_eq!(
        drops,
        vec![-i32::from(EXPECTED_SPIRIT_CHARGE)],
        "one drop of exactly 11 x {SOMERSAULT_COMMANDS} (visit ordinal 0), and no other"
    );
}

/// The contrast control. Same battle, same pad driver, same gauge seed - the
/// ring's `Auto` arm instead of `Command`. It seeds the identical two-swing
/// queue (`seed_basic_attack_queue`), so anything that charged Spirit here
/// would be charging it for the swings rather than for an art body.
#[test]
fn a_plain_attack_charges_no_spirit() {
    let mut w = build_world();
    walk_into_battle(&mut w);
    let (drops, shouts) = spirit_drops_over_one_turn(&mut w, &[]);
    assert!(
        shouts.is_empty(),
        "the Auto arm performs no art: {shouts:?}"
    );
    assert!(
        drops.is_empty(),
        "a plain attack performs no art, so nothing is charged: {drops:?}"
    );
}

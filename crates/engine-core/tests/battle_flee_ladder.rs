//! Fleeing a battle through the pad - the ladder for the action SM's run band
//! (`RunBegin`/`RunWait`/`RunEscape`, retail `FUN_801E295C` states
//! `0x64..0x66`) and the retail escape roll (`FUN_801E791C`,
//! `battle_formulas::escape_roll`).
//!
//! Retail reaches Run from the round-open `Begin | Run` prompt - state `0x1E`
//! takes it on the Circle press without a confirm. The live loop's
//! `Resolution::RunAway` arm rolls `World::roll_battle_escape` and arms
//! category 5; the SM's `0x64` arm floors every downed party member's
//! liveness at 1 on a successful roll (the mechanism behind "escape restores
//! a Stoned member"), `0x65` routes success to the `0x66` teardown and
//! failure back to the Done band (the action is consumed, the battle goes
//! on).
//!
//! Both arms are driven here: an assured escape (the latched pre-emptive
//! formation advantage `ctx+0x291 == 2`, which sets `roll_p = roll_e` so the
//! compare cannot fail) and a failed roll (party score floored to the
//! minimum, deterministic under the fixed RNG seed).
//!
//! Disc-free: synthetic party + the vanilla monster/formation tables. Runs in
//! CI unconditionally.

use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_action::ActionState;
use legaia_engine_vm::status_effects::StatusKind;

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
        w.set_battle_attack(i as u8, 60);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("flee_ladder_test");
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

fn enter_battle(w: &mut World) {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        let _ = w.tick();
        if w.mode == SceneMode::Battle {
            return;
        }
    }
    panic!("no encounter triggered in 6000 field ticks");
}

/// Wait for the party command session (the round prompt) to open.
fn wait_for_prompt(w: &mut World) {
    for _ in 0..0x80 {
        if w.battle_command.is_some() {
            return;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    panic!("no command session opened");
}

fn press(w: &mut World, b: PadButton, trace: &mut Vec<u8>) {
    w.set_pad(InputState::mask_of([b]));
    let _ = w.tick();
    trace.push(w.battle_ctx.action_state);
    w.set_pad(0);
    let _ = w.tick();
    trace.push(w.battle_ctx.action_state);
}

fn idle_ticks(w: &mut World, n: usize, trace: &mut Vec<u8>) {
    for _ in 0..n {
        w.set_pad(0);
        let _ = w.tick();
        trace.push(w.battle_ctx.action_state);
    }
}

#[test]
fn assured_escape_runs_the_run_band_and_leaves_the_battle() {
    let mut w = build_world();
    enter_battle(&mut w);
    wait_for_prompt(&mut w);

    // The latched pre-emptive advantage (`ctx+0x291 == 2`): retail's
    // `FUN_801E791C` reads it at `0x801E7AD8` and sets `roll_p = roll_e`, so
    // the escape compare cannot fail. (The battle-open `Begin` latch cleared
    // the live `+0x290` byte already; this writes the latched copy the roll
    // reads.)
    w.set_battle_formation_latched(
        legaia_engine_vm::battle_formulas::FormationAdvantage::Preemptive,
    );

    // A downed member and a petrified member: the `0x64` success arm floors
    // the downed liveness at 1 and the Escaped teardown clears Stone
    // (`cure_stone_on_escape`).
    w.actors[2].battle.hp = 0;
    w.actors[2].battle.liveness = 0;
    w.status_effects
        .apply_with_duration(1, StatusKind::Stone, 255);
    let xp_before = w.roster.members[0].raw.to_vec();

    // Round prompt: Circle takes Run without a confirm (retail state 0x1E).
    let mut trace: Vec<u8> = Vec::new();
    press(&mut w, PadButton::Circle, &mut trace);
    // Run band: RunBegin (0x3C-frame banner) -> RunWait -> RunEscape.
    idle_ticks(&mut w, 0x100, &mut trace);

    for state in [
        ActionState::Begin,
        ActionState::RunBegin,
        ActionState::RunWait,
        ActionState::RunEscape,
    ] {
        assert!(
            trace.contains(&state.as_byte()),
            "run band never reached {state:?}; trace = {trace:02X?}"
        );
    }
    assert_eq!(
        w.mode,
        SceneMode::Field,
        "a successful escape returns to the field"
    );
    assert_eq!(
        w.roster.members[0].raw, xp_before,
        "fleeing grants no loot / XP (no victory record write)"
    );
    assert_eq!(
        w.actors[2].battle.liveness, 1,
        "the 0x64 success arm floors a downed member's liveness at 1"
    );
    assert!(
        w.status_effects.statuses(1).is_empty(),
        "the Escaped teardown cures Stone (cure_stone_on_escape)"
    );
}

#[test]
fn failed_escape_consumes_the_turn_and_the_battle_goes_on() {
    let mut w = build_world();
    enter_battle(&mut w);
    wait_for_prompt(&mut w);

    // Force the roll's shape: party SPD 0 at full HP gives a party score of 0
    // (floored to 1), so `roll_p = rand % 1 = 0`; the enemy score is large,
    // so `roll_e = rand % score` is non-zero under this fixed seed and
    // `roll_p < roll_e` fails the escape. Deterministic: the world RNG is
    // seeded, and the roll consumes exactly two draws.
    for i in 0..3 {
        w.battle_speed[i] = 0;
        w.actors[i].battle.hp = w.actors[i].battle.max_hp;
    }
    let monster_slot = w.party_count as usize;
    w.battle_speed[monster_slot] = 20000;
    w.rng_state = 0xDEAD_BEEF;

    let mut trace: Vec<u8> = Vec::new();
    press(&mut w, PadButton::Circle, &mut trace);
    idle_ticks(&mut w, 0x200, &mut trace);

    // The band ran its failure shape: RunBegin -> RunWait -> Done band, no
    // RunEscape, and the encounter is still live.
    assert!(
        trace.contains(&ActionState::RunBegin.as_byte()),
        "run band never armed; trace = {trace:02X?}"
    );
    assert!(
        !trace.contains(&ActionState::RunEscape.as_byte()),
        "the failed roll must not reach RunEscape"
    );
    assert!(
        trace.contains(&ActionState::DoneCleanup.as_byte()),
        "a failed run is consumed through the Done band; trace = {trace:02X?}"
    );
    assert_eq!(
        w.mode,
        SceneMode::Battle,
        "a failed escape leaves the party in the battle"
    );
}

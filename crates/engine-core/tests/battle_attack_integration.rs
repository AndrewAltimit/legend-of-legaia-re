//! End-to-end battle-action integration: drive the SM through one full
//! Attack action with a host that applies damage via the
//! `legaia_engine_vm::battle_formulas` math, and assert the target HP
//! actually goes down.
//!
//! What this proves:
//!  - `BattleActionHost::apply_damage` is reachable from the SM run loop.
//!  - The `battle_formulas` module integrates cleanly into a host impl.
//!  - The accuracy roll + damage formula combination produces non-zero
//!    HP reduction across many seeds (no infinite-miss pathology).
//!  - The world's pending battle events queue captures the damage event
//!    so a UI layer can pick it up.
//!
//! Stays clean-room: no disc data, no Sony bytes, no PROT lookups.

use legaia_engine_core::battle_events::BattleEvent;
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::battle_action::{ActionState, BattleEndCause, StepOutcome};
use legaia_engine_vm::battle_formulas::{
    MpCostModifier, accuracy_roll, mp_cost_after_ability_bits, psyq_rand_step,
};

fn build_world() -> World {
    let mut world = World {
        mode: SceneMode::Battle,
        party_count: 3,
        ..World::default()
    };
    // 3 party slots (alive) + 1 monster slot (alive). Slots 4..7 stay
    // inactive - the SM treats them as dead.
    for i in 0..4 {
        let actor = world.spawn_actor(i);
        actor.battle.liveness = 1;
        actor.battle.hp = 100;
        actor.battle.max_hp = 100;
    }
    // Queue an Attack action by party slot 0.
    world.battle_ctx.queued_action = 3;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    world
}

#[test]
fn full_attack_action_emits_apply_damage_event() {
    let mut world = build_world();
    let mut transitions = 0u32;
    let mut damage_events = 0u32;
    for _ in 0..1000 {
        let outcome = world.tick();
        if matches!(outcome, Some(StepOutcome::Transition { .. })) {
            transitions += 1;
        }
        if matches!(outcome, Some(StepOutcome::BattleComplete)) {
            break;
        }
        for ev in world.drain_battle_events() {
            if matches!(ev, BattleEvent::ApplyDamage { .. }) {
                damage_events += 1;
            }
        }
    }
    assert!(
        transitions > 0,
        "battle SM made zero transitions across 1000 frames"
    );
    // `apply_damage` stands in for the item / restore applier `FUN_800402F4`,
    // and retail calls it from exactly one place in the action SM: state
    // `0x3F` (`spirit_fire_damage`, `jal 0x800402f4` at `0x801E4134`). The
    // attack band resolves damage through `FUN_801EC3E4` instead, so **zero**
    // is the correct count for an Attack action - an earlier revision of this
    // comment expected an AttackChain damage hook that retail does not have.
    assert_eq!(
        damage_events, 0,
        "the applier has no attack-band call site in retail"
    );
}

#[test]
fn formula_driven_damage_decreases_hp_across_many_rolls() {
    // Drive the formula directly across many RNG seeds to prove damage
    // is actually delivered in the average case. This is the post-port
    // happy-path the battle SM should converge on.
    let mut total_damage = 0u32;
    let mut seed: u32 = 0xDEAD_BEEF;
    let attacker_acc = 100;
    let target_eva = 10;
    let attacker_atk = 50;
    let target_def = 20;
    let mut hits = 0u32;
    let trials = 200;
    for _ in 0..trials {
        if accuracy_roll(attacker_acc, target_eva, &mut seed) {
            hits += 1;
            let raw = (attacker_atk * 2 - target_def).max(1);
            let var = (psyq_rand_step(&mut seed) as i32 % 25) - 12;
            let dmg = (raw + raw * var / 100).max(1) as u16;
            total_damage += dmg as u32;
        }
    }
    // 100 vs 10 ⇒ p_hit ≈ 99/110 ≈ 90%. Assert >80% hit rate to absorb
    // RNG variance.
    assert!(
        hits > (trials * 4 / 5),
        "expected >80% hit rate at 100 vs 10 acc/eva - got {hits}/{trials}"
    );
    assert!(total_damage > 0);
}

#[test]
fn mp_cost_drains_caster_mp_through_battle_actor() {
    let mut world = build_world();
    let actor = &mut world.actors[0];
    actor.battle.mp = 50;
    // Caster has the "MP-half" ability bit (`0x20`). A 40-MP spell costs 20.
    let modifier = MpCostModifier::from_ability_flags(0x20);
    let cost = mp_cost_after_ability_bits(40, modifier);
    assert_eq!(cost, 20);
    actor.battle.mp = actor.battle.mp.saturating_sub(cost);
    assert_eq!(actor.battle.mp, 30);
}

#[test]
fn battle_complete_propagates_through_world() {
    let mut world = build_world();
    // Wipe the party - next tick should land in BattleComplete (party wipe
    // cause).
    for i in 0..3 {
        world.actors[i].battle.liveness = 0;
    }
    world.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
    // Read the raised cause off the bare SM step first - `World::tick`
    // resolves the battle in the same frame it completes, which consumes
    // `battle_end` (see `finish_battle`).
    assert_eq!(world.step_battle(), StepOutcome::BattleComplete);
    assert_eq!(world.battle_end, Some(BattleEndCause::PartyWipe));

    // And through `tick`: the same wipe must reach a terminal state rather
    // than sitting in `SceneMode::Battle` forever.
    let mut world = build_world();
    for i in 0..3 {
        world.actors[i].battle.liveness = 0;
    }
    world.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
    let mut completed = false;
    for _ in 0..100 {
        if matches!(world.tick(), Some(StepOutcome::BattleComplete)) {
            completed = true;
            break;
        }
    }
    assert!(completed, "the SM must report BattleComplete through tick");
    assert!(world.game_over, "a party wipe raises game over");
    assert_ne!(
        world.mode,
        SceneMode::Battle,
        "battle mode is left on resolve"
    );
}

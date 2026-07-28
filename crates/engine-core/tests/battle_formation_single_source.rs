//! The formation advantage is **one** byte, and the action state machine is
//! the thing that consumes it.
//!
//! Retail keeps the roll at `ctx+0x290` and its latched copy at `ctx+0x291`,
//! both inside the battle context `FUN_801E295C` owns:
//!
//! * `FUN_80051D84` is the only writer of `+0x290` (`0x80051FE0`,
//!   `0x80052098`);
//! * the initiative seeder `FUN_801DA780` reads the **unlatched** byte for its
//!   side lockout (`0x801DAA40`);
//! * state `0x00` seeds the turn cursor `+0x1A` from it
//!   (`0x801E2AC0..0x801E2B24`), latches it into `+0x291` (`0x801E2B38` - the
//!   byte's only writer) and clears the original (`0x801E2B48`);
//! * the escape roll `FUN_801E791C` is the only reader of `+0x291`
//!   (`0x801E7AD8`).
//!
//! The engine used to carry a *second* copy on `World` that nothing ever
//! pushed into the context, so the SM's advantage arms were unreachable: the
//! turn cursor was seeded from a byte that was permanently zero, whatever the
//! roll said. These tests drive the value in through the host-side name and
//! assert the **state machine** acted on it, so re-splitting the storage fails
//! them.

use legaia_engine_core::world::World;
use legaia_engine_vm::battle_action::ActionState;
use legaia_engine_vm::battle_formulas::FormationAdvantage;

/// Stage a 3-party / 2-monster battle parked at the SM's `Begin` state.
fn battle_at_begin() -> World {
    let mut world = World::default();
    world.enter_battle(3, 2);
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::Begin.as_byte(),
        "enter_battle parks the SM at 0x00"
    );
    world
}

/// A back attack (`+0x290 == 1`) starts the round `ctx[+0x00]` entries down the
/// order - the party count. Retail: `0x801E2B0C` `lbu v0,0x0(a0)` /
/// `0x801E2B14` `sb v0,0x9(s5)`.
#[test]
fn back_attack_seeds_the_turn_cursor_at_the_party_count() {
    let mut world = battle_at_begin();
    world.set_battle_formation(FormationAdvantage::BackAttack);
    assert_eq!(world.battle_ctx.turn_cursor, 0, "cursor starts at the head");

    world.step_battle();

    assert_eq!(
        world.battle_ctx.turn_cursor, 3,
        "the ambushed party opens its round `party_count` entries down"
    );
}

/// A pre-emptive strike (`+0x290 == 2`) seeds from `ctx[+0x01]`, the monster
/// count. Retail: `0x801E2B18` `lbu v0,0x1(a0)` / `0x801E2B20`.
#[test]
fn preemptive_strike_seeds_the_turn_cursor_at_the_monster_count() {
    let mut world = battle_at_begin();
    world.set_battle_formation(FormationAdvantage::Preemptive);

    world.step_battle();

    assert_eq!(
        world.battle_ctx.turn_cursor, 2,
        "the ambushed monsters open their round `monster_count` entries down"
    );
}

/// No advantage seeds the head of the order (`0x801E2B08` `sb zero,0x9(s5)`).
#[test]
fn no_advantage_seeds_the_head_of_the_order() {
    let mut world = battle_at_begin();
    world.battle_ctx.turn_cursor = 5; // stale value from a previous round
    world.set_battle_formation(FormationAdvantage::None);

    world.step_battle();

    assert_eq!(world.battle_ctx.turn_cursor, 0);
}

/// The latch is the SM's, and the host-side reader sees it. Writing the
/// advantage through `World::set_battle_formation` and stepping the SM must
/// move it into `+0x291` and clear `+0x290` - if the two ever become separate
/// storage again, the latched read comes back `None`.
#[test]
fn the_state_machine_latches_what_the_host_rolled() {
    for advantage in [
        FormationAdvantage::BackAttack,
        FormationAdvantage::Preemptive,
    ] {
        let mut world = battle_at_begin();
        world.set_battle_formation(advantage);
        assert_eq!(world.battle_formation(), advantage, "host read of +0x290");

        world.step_battle();

        assert_eq!(
            world.battle_formation_latched(),
            advantage,
            "{advantage:?}: the SM must latch +0x290 into +0x291"
        );
        assert_eq!(
            world.battle_formation(),
            FormationAdvantage::None,
            "{advantage:?}: and then clear +0x290"
        );
    }
}

/// The arm is **one-shot per battle**, because retail's is: `ctx[7]` has a
/// single zero-writer in the corpus (`FUN_801D0748`'s `0xFE` arm at
/// `0x801D3224`), and the end-of-action gate hands the next actor `0x0A`,
/// never `0x00`. The port re-arms `Begin` once per action, so an unguarded
/// re-entry would copy the already-cleared `+0x290` over `+0x291` - silently
/// disabling pre-emptive-strike escapes - and rewind the turn cursor
/// mid-round.
#[test]
fn re_entering_begin_does_not_unlatch_the_advantage_or_rewind_the_cursor() {
    let mut world = battle_at_begin();
    world.set_battle_formation(FormationAdvantage::Preemptive);
    world.step_battle();
    assert_eq!(
        world.battle_formation_latched(),
        FormationAdvantage::Preemptive
    );
    assert_eq!(world.battle_ctx.turn_cursor, 2);

    // The engine re-arms `Begin` for every action; retail never does.
    world.battle_ctx.turn_cursor = 4; // three more actors have acted
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    world.step_battle();

    assert_eq!(
        world.battle_formation_latched(),
        FormationAdvantage::Preemptive,
        "+0x291 survives the whole battle - it has one writer in retail"
    );
    assert_eq!(
        world.battle_ctx.turn_cursor, 4,
        "the turn cursor is a position in the round's order, not a per-action counter"
    );
}

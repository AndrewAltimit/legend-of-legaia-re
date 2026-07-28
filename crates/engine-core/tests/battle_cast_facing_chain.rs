//! The cast-begin facing store, end to end through the live world.
//!
//! Chain under test: `World::enter_battle` seats every combatant from the
//! retail stage tables (`legaia_engine_core::battle_seats`, `FUN_800513F0`) ->
//! `World::tick` -> `World::step_battle` -> `BattleHostImpl::actor_position`
//! (the `+0x34`/`+0x38` read) -> the action SM's `MagicCastBegin` arm ->
//! `BattleActor::facing_angle` (`+0x46`).
//!
//! Retail's block is `overlay_0898_801e295c.txt` `0x801E4334..0x801E43A4`: the
//! bearing from the actor to its target (or to the centroid `FUN_801DCEAC`
//! folds out of a target-group code), plus a half-turn, masked to 12 bits.
//!
//! REF: FUN_801E295C, FUN_801DCEAC, FUN_80019B28

use legaia_engine_core::battle_seats::{monster_seat, party_seat};
use legaia_engine_core::world::World;
use legaia_engine_vm::battle_action::{ActionState, bearing_12bit_approx};

/// A three-versus-four battle at the authored seats, with party slot 0 armed
/// to cast at `target`, ticked until the SM leaves `MagicCastBegin`.
fn cast_at(target: u8) -> World {
    let mut world = World::default();
    world.enter_battle(3, 4);
    world.actors[0].battle.action_category = 2; // Magic
    world.actors[0].battle.active_target = target;
    world.actors[0].battle.params[0] = 0x81; // a player Seru spell id
    world.battle_ctx.queued_action = 2;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    for _ in 0..64 {
        let _ = world.tick();
        if world.battle_ctx.action_state != ActionState::Begin.as_byte()
            && world.battle_ctx.action_state != ActionState::PreActionWait.as_byte()
            && world.battle_ctx.action_state != ActionState::ActionSeed.as_byte()
            && world.battle_ctx.action_state != ActionState::MagicCastBegin.as_byte()
        {
            break;
        }
    }
    world
}

/// The bearing the block must produce for an actor at `from` aiming at `to`,
/// stated the way retail composes it rather than restated from the port.
fn expected(from: (i16, i16), to: (i16, i16)) -> u16 {
    bearing_12bit_approx(to.1, to.0, from.1, from.0).wrapping_add(0x800) & 0xFFF
}

/// Party slot 0 casting at each of the four seated monsters turns to face it,
/// with the seat pair coming from the retail stage table rather than from the
/// test.
#[test]
fn a_party_cast_faces_the_targeted_monster_seat() {
    let caster = party_seat(3, 0);
    for monster in 0..4usize {
        let world = cast_at(3 + monster as u8);
        let seat = monster_seat(4, monster, false);
        assert_eq!(
            world.actors[0].battle.facing_angle,
            expected((caster.x, caster.z), (seat.x, seat.z)),
            "monster {monster} at ({}, {})",
            seat.x,
            seat.z
        );
    }
    // Not a constant: the four seats are distinct, so the four facings are.
    let mut seen: Vec<u16> = (0..4u8)
        .map(|m| cast_at(3 + m).actors[0].battle.facing_angle)
        .collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 4, "each seat must produce its own facing");
}

/// The target-group arm. Target code `9` is the enemy row - the value
/// `World::resolve_monster_target`'s class-`8` arm (retail `FUN_801E7320`)
/// writes into `active_target` - and it must aim the caster at the centroid of
/// the four monster seats, not at any one of them.
#[test]
fn a_group_coded_cast_faces_the_enemy_row_centroid() {
    let world = cast_at(9);
    let caster = party_seat(3, 0);
    let seats: Vec<_> = (0..4).map(|m| monster_seat(4, m, false)).collect();
    // Retail's centroid is `-sum / count` and the SM negates it back, so the
    // aim point is the plain integer mean of the row.
    let mean_x = (seats.iter().map(|s| -(s.x as i32)).sum::<i32>() / 4) as i16;
    let mean_z = (seats.iter().map(|s| -(s.z as i32)).sum::<i32>() / 4) as i16;
    assert_eq!(
        world.actors[0].battle.facing_angle,
        expected((caster.x, caster.z), (-mean_x, -mean_z))
    );
    // And it is genuinely a group answer: no single seat produces it.
    for (m, s) in seats.iter().enumerate() {
        assert_ne!(
            world.actors[0].battle.facing_angle,
            expected((caster.x, caster.z), (s.x, s.z)),
            "group facing collapsed onto monster {m}"
        );
    }
}

/// Regression guard on the accessor itself: with the seats zeroed the block
/// still runs and produces the degenerate answer, so a facing of `0x800` is
/// not evidence that the wire fired.
#[test]
fn co_located_actors_produce_the_half_turn_not_a_skip() {
    let mut world = World::default();
    world.enter_battle(1, 1);
    for a in world.actors.iter_mut() {
        a.move_state.world_x = 0;
        a.move_state.world_z = 0;
    }
    world.actors[0].battle.action_category = 2;
    world.actors[0].battle.active_target = 1;
    world.actors[0].battle.facing_angle = 0x555;
    world.battle_ctx.queued_action = 2;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    for _ in 0..64 {
        let _ = world.tick();
    }
    assert_eq!(world.actors[0].battle.facing_angle, 0x800);
}

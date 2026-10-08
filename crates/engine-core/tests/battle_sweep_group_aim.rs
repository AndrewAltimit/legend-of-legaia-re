//! A sweep-targeted cast has to come out of the picker as a **target-group
//! code**, because that is the only thing the cast-begin facing store knows how
//! to aim.
//!
//! `FUN_801E295C`'s cast-begin tail splits the target byte `+0x1DD` on one
//! instruction - `sltiu v0,t2,0x8` at `0x801E433C` - and hands everything at or
//! above `8` to `FUN_801DCEAC`, which decodes `8` to party slots `[0, 3)` and
//! `9` to the enemy row `[3, 7)`, folds the live seats into a centroid and
//! aims the caster at it. Every writer of `+0x1DD` in the dumped corpus stays
//! inside `{0..=6, 8, 9}`.
//!
//! A sweep once reached the SM as `0xFF`. `FUN_801DCEAC`'s "anything else"
//! arm reads that as the one-element group `[0xFF, 0x100)`, so the port's
//! decode produced an empty range, the aim came back `None`, and the caster was
//! left facing wherever it already was. These tests stage each group code the
//! live command flow writes for a sweep (`World::cast_target_code`) and assert
//! the cast-begin facing actually moves to the group's bearing.

use legaia_engine_core::world::World;
use legaia_engine_vm::battle_action::{ActionState, BattleActor};
use legaia_engine_vm::battle_target_group::{
    GroupSlot, TARGET_GROUP_ENEMIES, TARGET_GROUP_PARTY, target_group_aim,
};

/// The facing an actor that has never been aimed carries.
const UNAIMED: u16 = 0;

/// A seated 3v2 battle.
fn seated_battle() -> World {
    use legaia_engine_core::ap_gauge::ApGauge;

    let mut world = World::default();
    world.enter_battle(3, 2);
    // The retail 3-member seats put slot 0 on the stage's centre axis, where
    // both group centroids share `x == 0` and the bearing degenerates to the
    // same value for either code. Nudge the caster off-axis so the two codes
    // are distinguishable by the facing they produce.
    world.actors[0].move_state.world_x = 250;
    for slot in 0..5 {
        world.actors[slot].battle.hp = 100;
        world.actors[slot].battle.max_hp = 100;
        world.actors[slot].battle.mp = 30;
    }
    for gauge in world.battle.ap_gauges.iter_mut().take(3) {
        *gauge = ApGauge::with_base(8);
    }
    world
}

/// The bearing `face_cast_target` should land on for `code`, computed
/// independently from the seats the world stamped.
fn expected_facing(world: &World, caster: u8, code: u8) -> u16 {
    let mut slots = [GroupSlot {
        live: false,
        x: 0,
        z: 0,
    }; 8];
    let party_count = world.party.party_count;
    for (retail_slot, out) in slots.iter_mut().enumerate() {
        let retail_slot = retail_slot as u8;
        let engine_slot = if retail_slot < 3 {
            if retail_slot >= party_count {
                continue;
            }
            retail_slot
        } else {
            party_count + (retail_slot - 3)
        };
        let Some(actor) = world.actors.get(engine_slot as usize) else {
            continue;
        };
        *out = GroupSlot {
            live: true,
            x: actor.move_state.world_x,
            z: actor.move_state.world_z,
        };
    }
    let aim = target_group_aim(code, &slots).expect("the group has live seats");
    let caster = &world.actors[caster as usize];
    let bearing = legaia_engine_vm::battle_action::bearing_12bit_approx(
        aim.centroid_z.wrapping_neg(),
        aim.centroid_x.wrapping_neg(),
        caster.move_state.world_z,
        caster.move_state.world_x,
    );
    bearing.wrapping_add(0x800) & 0xFFF
}

/// Run the cast-begin state for the acting slot and return its resulting
/// facing.
fn run_cast_begin(world: &mut World, caster: u8) -> u16 {
    world.battle_ctx.active_actor = caster;
    world.battle_ctx.action_state = ActionState::MagicCastBegin.as_byte();
    world.step_battle();
    world.actors[caster as usize].battle.facing_angle
}

/// An all-allies cast resolves to retail's party group code `8`, and the SM
/// aims the caster at the party centroid.
#[test]
fn an_all_allies_sweep_aims_at_the_party_group() {
    let mut world = seated_battle();
    world.actors[0].battle.active_target = TARGET_GROUP_PARTY;
    assert_eq!(world.actors[0].battle.facing_angle, UNAIMED);

    let facing = run_cast_begin(&mut world, 0);
    assert_ne!(facing, UNAIMED, "the cast must have aimed at something");
    assert_eq!(
        facing,
        expected_facing(&world, 0, TARGET_GROUP_PARTY),
        "the cast-begin store must aim at the party centroid"
    );
}

/// The enemy-row sweep is the mirror: code `9`, aimed at the monster centroid,
/// and it must not land on the same bearing as the party group (which would
/// mean the code was ignored).
#[test]
fn an_all_enemies_sweep_aims_at_the_enemy_group() {
    let mut world = seated_battle();
    world.actors[0].battle.active_target = TARGET_GROUP_ENEMIES;

    let facing = run_cast_begin(&mut world, 0);
    assert_ne!(facing, UNAIMED, "the cast must have aimed at something");
    assert_eq!(facing, expected_facing(&world, 0, TARGET_GROUP_ENEMIES));
    assert_ne!(
        facing,
        expected_facing(&world, 0, TARGET_GROUP_PARTY),
        "the two rows sit on opposite sides of the stage - the code must pick"
    );
}

/// The regression itself, stated as the property that failed: a code outside
/// retail's space produces no aim at all. `0xFF` is what a sweep once reached the SM
/// as.
#[test]
fn a_code_outside_the_retail_group_space_produces_no_aim() {
    let mut world = seated_battle();
    world.actors[0].battle.active_target = 0xFF;

    let facing = run_cast_begin(&mut world, 0);

    assert_eq!(
        facing, UNAIMED,
        "0xFF decodes to an empty group - nothing to aim at"
    );
}

/// A self-target is not a group: it is the caster's own slot, which is the
/// value retail's self-skip (`beq v0,t2` at `0x801E4350`) expects.
#[test]
fn a_self_target_writes_the_casters_own_slot_not_a_group_code() {
    let mut world = seated_battle();
    world.actors[0].battle.active_target = 0;

    let facing = run_cast_begin(&mut world, 0);
    assert_eq!(facing, UNAIMED, "retail skips the store for a self-target");
}

/// Guard the shape the fix depends on: `BattleActor::active_target` defaults to
/// a real slot, so "unaimed" above is a meaningful baseline.
#[test]
fn the_actor_default_target_is_a_slot() {
    assert!((BattleActor::new().active_target as usize) < 8);
}

//! Field-runtime wiring oracles: the two `engine-core` ports this lane moved
//! from "reachable only from tests" onto the live per-frame field path.
//!
//! - `FUN_801D0B90` walk-regen (`walk_regen::tick_walk_regen`), reached from
//!   `World::tick`'s Field arm through `World::tick_field_walk_regen`.
//! - `FUN_801CFC40` actor collision (`World::field_actor_dir_blocked`),
//!   reached from the locomotion step's per-axis gate.
//!
//! Disc-free: every fixture is synthetic engine state.

use legaia_engine_core::walk_regen::{
    AP_WALK_MASK, HP_WALK_MASK, MP_WALK_MASK, WALK_REGEN_STEP_COST,
};
use legaia_engine_core::world::{SceneMode, World};

/// Give roster slot 0 a record with the three walk passives set and every
/// pool one bump short of full, and make it the whole present party.
fn party_with_walk_passives(world: &mut World, ability_hi: u32) {
    world.roster = legaia_save::Party::zeroed(1);
    world.party_count = 1;
    let rec = &mut world.roster.members[0];
    let mut bits = [0u8; legaia_save::ABILITY_BITS_LEN];
    bits[4..8].copy_from_slice(&ability_hi.to_le_bytes());
    rec.set_ability_bits(bits);
    rec.set_hp_mp_sp(legaia_save::HpMpSp {
        hp_max: 100,
        hp_cur: 50,
        mp_max: 40,
        mp_cur: 20,
        sp_max: 30,
        sp_cur: 10,
    });
}

fn pools(world: &World) -> (u16, u16, u16) {
    let h = world.roster.members[0].hp_mp_sp();
    (h.hp_cur, h.mp_cur, h.sp_cur)
}

/// The Field frame tick reaches the walk-regen kernel: with the accumulator
/// banked past the retail `0x20` cost, one tick drains exactly that cost and
/// applies the retail 8 / 2 / 1 bumps to a party member carrying all three
/// passives.
#[test]
fn field_tick_drains_the_walk_regen_accumulator_and_bumps_the_gauges() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    party_with_walk_passives(&mut world, HP_WALK_MASK | MP_WALK_MASK | AP_WALK_MASK);
    world.walk_regen_steps = WALK_REGEN_STEP_COST + 1;

    world.tick();

    assert_eq!(world.walk_regen_steps, 1, "the tick drained 0x20");
    assert_eq!(pools(&world), (58, 22, 11), "HP +8 / MP +2 / AP +1");
}

/// The gates are per-passive: a member with only Magic Source set moves only
/// its MP pool, and a member with none moves nothing - which is why wiring
/// this changes no existing behaviour for a party that carries no Source
/// accessory.
#[test]
fn walk_regen_gates_are_per_passive_and_a_bare_party_is_untouched() {
    for (mask, want) in [
        (0u32, (50, 20, 10)),
        (HP_WALK_MASK, (58, 20, 10)),
        (MP_WALK_MASK, (50, 22, 10)),
        (AP_WALK_MASK, (50, 20, 11)),
    ] {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        party_with_walk_passives(&mut world, mask);
        world.walk_regen_steps = WALK_REGEN_STEP_COST + 1;
        world.tick();
        assert_eq!(pools(&world), want, "mask {mask:#x}");
    }
}

/// An accumulator at or below the retail cost is a no-op: the gate is
/// `0x20 < counter`, so exactly `0x20` neither drains nor bumps.
#[test]
fn field_tick_walk_regen_is_gated_on_the_retail_step_cost() {
    for start in [0, WALK_REGEN_STEP_COST] {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        party_with_walk_passives(&mut world, HP_WALK_MASK);
        world.walk_regen_steps = start;
        world.tick();
        assert_eq!(world.walk_regen_steps, start, "counter untouched");
        assert_eq!(pools(&world), (50, 20, 10), "no bump below the cost");
    }
}

/// The accumulator has a producer: a locomotion step that actually commits
/// feeds it, so walking eventually reaches a regen tick on its own. (The
/// drain is retail-pinned; the fill unit is the engine's - one per retail
/// frame whose step committed.)
#[test]
fn committed_locomotion_steps_feed_the_walk_regen_accumulator() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 2000;
    world.actors[0].move_state.world_z = 2000;
    party_with_walk_passives(&mut world, HP_WALK_MASK);
    // Hold X+ (pad bit 0x2000 is the decoded direction; drive the pad the way
    // a host does).
    world.set_pad(legaia_engine_core::input::PadButton::Right.mask());

    let start_x = world.actors[0].move_state.world_x;
    for _ in 0..200 {
        world.tick();
    }
    assert!(
        world.actors[0].move_state.world_x > start_x,
        "the player actually walked"
    );
    let (hp, _, _) = pools(&world);
    assert!(
        hp > 50,
        "walking fed the accumulator until a regen tick fired (hp {hp})"
    );
}

/// The locomotion step's per-axis gate now goes through the combined
/// `FUN_801CFC40` probe: with `solid_field_npcs` on, a field NPC standing in
/// the walk lane stops the player at the retail actor standoff, and with it
/// off the player walks through - the same two outcomes the arm-by-arm form
/// produced, now sourced from `World::field_actor_dir_blocked`.
#[test]
fn locomotion_actor_gate_routes_through_the_combined_probe() {
    let press = |solid: bool| {
        let mut world = World::new();
        world.install_field_player(0);
        world.solid_field_npcs = solid;
        world.field_npc_positions.insert(1, (2000, 2526));
        world.actors[0].move_state.world_x = 1800;
        world.actors[0].move_state.world_z = 2526;
        for _ in 0..100 {
            world.advance_with_collision(0, 0x2000, 8);
        }
        world.actors[0].move_state.world_x
    };
    // Head-on rest: 102 units short of the NPC (the pre-step probe parity).
    assert_eq!(press(true), 2000 - 102);
    assert!(press(false) > 2000, "flag off: NPCs stay transparent");

    // And the direct probe agrees with what the gate did.
    let mut world = World::new();
    world.field_npc_positions.insert(1, (2000, 2526));
    assert!(
        world.field_actor_dir_blocked(2000 - 102, 2526, 3),
        "X+ into the NPC reads blocked"
    );
    assert!(
        !world.field_actor_dir_blocked(2000 - 400, 2526, 3),
        "far away reads clear"
    );
}

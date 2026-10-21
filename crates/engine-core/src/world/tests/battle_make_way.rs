//! A melee's formation moves: the action seed's recentre on the target
//! (`FUN_801EFE44`) and the bystanders' make-way step (state `0x14` plus the
//! action SM's shared tail), pinned against a retail position capture.
//!
//! The capture is the Cort fight from `cort_evolved_battle_first_menu`
//! (three on one, Cort's size class `34` - his node radius reads `1088 =
//! 34 << 5`), with Noa's melee the round's first action. Retail's live pairs:
//!
//! | moment | Vahn | Gala | Cort |
//! |---|---|---|---|
//! | round prompt | `(0, -812)` | `(-600, -762)` | `(0, 813)` |
//! | the pass after the seed | `(0, -1625)` | `(-600, -1575)` | `(0, 0)` |
//! | countdown spent | `(-276, -1733)` | `(-876, -1683)` | `(0, 0)` |
//!
//! Retail ran that capture at a frame step of `3`, twelve passes of `24`
//! units (`288`); the port's per-vsync pass drains the same countdown in
//! thirty-four passes of `8` (`272`). Both are `size * 8` to within a pass.

use super::*;
use legaia_engine_battle_vm::battle_action::ActionState;

const CORT_SIZE_CLASS: u8 = 34;

fn cort_fight() -> World {
    use legaia_engine_battle::monster_catalog::MonsterDef;
    let mut world = World::new();
    world.enter_battle(3, 1);
    let mut def = MonsterDef::new(200, "Cort".to_string(), 4000, 5);
    def.size_class = CORT_SIZE_CLASS;
    world.tables.monster_catalog.insert(def);
    let m = &mut world.actors[3];
    m.battle_monster_id = Some(200);
    m.battle.hp = 4000;
    m.battle.max_hp = 4000;
    for i in 0..3 {
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    // The round prompt's formation (the round-start recentre has run).
    world.normalize_battle_formation();
    // Noa's melee on Cort is the action the SM seeds.
    world.actors[1].battle.action_category = 3;
    world.actors[1].battle.active_target = 3;
    world.actors[1].battle.params[0] = 0x0C;
    world.battle_ctx.active_actor = 1;
    world.battle_ctx.action_state = ActionState::ActionSeed.as_byte();
    world
}

fn pos(world: &World, slot: usize) -> (i16, i16) {
    let m = &world.actors[slot].move_state;
    (m.world_x, m.world_z)
}

#[test]
fn a_melee_seed_puts_its_target_on_the_origin() {
    let mut world = cort_fight();
    assert_eq!(pos(&world, 0), (0, -812));
    assert_eq!(pos(&world, 3), (0, 813));
    let _ = world.step_battle();
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::AttackFace.as_byte()
    );
    assert_eq!(pos(&world, 0), (0, -1625));
    assert_eq!(pos(&world, 1), (600, -1575));
    assert_eq!(pos(&world, 2), (-600, -1575));
    assert_eq!(pos(&world, 3), (0, 0));
}

#[test]
fn the_bystanders_step_out_of_the_attackers_lane() {
    let mut world = cort_fight();
    let _ = world.step_battle(); // the seed
    let (v0, g0) = (pos(&world, 0), pos(&world, 2));
    for _ in 0..80 {
        let _ = world.step_battle();
    }
    assert_eq!(
        world.battle_ctx.dodge_countdown, 0,
        "the countdown is spent"
    );
    let (v1, g1) = (pos(&world, 0), pos(&world, 2));
    // Both stand left of Noa's line to Cort, so both step the same way,
    // at right angles to it - retail's `(-276, -108)`.
    let dv = (i32::from(v1.0 - v0.0), i32::from(v1.1 - v0.1));
    let dg = (i32::from(g1.0 - g0.0), i32::from(g1.1 - g0.1));
    assert_eq!(dv, dg, "one heading for both bystanders");
    // Each pass is `(lut * step) >> 9` per axis. Retail's step-3 pass read
    // `(-23, -9)` (twelve of them: `(-276, -108)`); the port's step-1 pass
    // over the same heading is `(-8, -3)`, thirty-four times.
    assert_eq!(
        dv,
        (
            -8 * i32::from(CORT_SIZE_CLASS),
            -3 * i32::from(CORT_SIZE_CLASS)
        )
    );
    // The attacker and its target are not moved by the tail.
    assert_eq!(pos(&world, 3), (0, 0), "the target holds the origin");
}

/// A solo fight's two empty party seats are structs the walks move, and
/// they weigh in the box. `gimard_summon_start` holds Vahn at `(82, -542)`,
/// both empty seats at `(-82, 543)` and Gimard at `(63, -228)` - a box
/// already centred on the origin, so the cast's seed recentre moves nobody.
/// With the empty seats on the origin instead the same seed would shift the
/// fight by `(-41, 271)`.
#[test]
fn a_solo_fights_empty_seats_hold_its_formation_centred() {
    use legaia_engine_battle::monster_catalog::MonsterDef;
    let mut world = World::new();
    world.enter_battle(1, 1);
    world
        .tables
        .monster_catalog
        .insert(MonsterDef::new(201, "Gimard".to_string(), 300, 5));
    world.actors[1].battle_monster_id = Some(201);
    world.actors[1].battle.hp = 76;
    world.actors[0].battle.hp = 196;
    world.actors[0].move_state.world_x = 82;
    world.actors[0].move_state.world_z = -542;
    world.actors[1].move_state.world_x = 63;
    world.actors[1].move_state.world_z = -228;
    for pool in [1, 2] {
        world.seat_empty_battle_seat(pool, -82, 543);
    }
    world.actors[0].battle.action_category = 2; // Magic
    world.actors[0].battle.active_target = 1;
    world.seed_battle_formation_recentre(0);
    assert_eq!(pos(&world, 0), (82, -542));
    assert_eq!(pos(&world, 1), (63, -228));
    assert_eq!(world.battle_pool_position(1), Some((-82, 543)));

    // The empty seats move with the formation: a melee seed on Gimard
    // carries them by the same shift as Vahn.
    world.actors[0].battle.action_category = 3;
    world.seed_battle_formation_recentre(0);
    assert_eq!(pos(&world, 1), (0, 0));
    assert_eq!(pos(&world, 0), (82 - 63, -542 + 228));
    assert_eq!(world.battle_pool_position(2), Some((-82 - 63, 543 + 228)));
}

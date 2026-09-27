//! The battle-intro enemy-name banner: flow `0x0A` seeds the `ctx[+0x6D6]`
//! hold, the labels are one per monster group, and `0x0B` sweeps them.

use super::*;

fn intro_world() -> World {
    use crate::monster_catalog::MonsterDef;
    let mut world = World::new();
    world.party.party_count = 1;
    world.mode = SceneMode::Battle;
    world.clock.frame_step = 1;
    world
        .tables
        .monster_catalog
        .insert(MonsterDef::new(7, "Killer Bee", 40, 5));
    for i in 1..4 {
        let a = &mut world.actors[i];
        a.active = true;
        a.battle.hp = 40;
        a.battle.max_hp = 40;
        a.battle.liveness = 1;
        a.battle_monster_id = Some(7);
    }
    world
}

#[test]
fn a_run_of_three_reads_as_one_label_with_the_composers_suffix() {
    let world = intro_world();
    let rows = crate::battle_hud::battle_enemy_target_rows(&world);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "Killer Bee * 3");
}

#[test]
fn the_hold_is_ninety_frames_then_the_labels_go() {
    let mut world = intro_world();
    world.arm_battle_intro_names();
    assert_eq!(world.battle.intro_names_frames, 0x5A);
    for _ in 0..0x59 {
        world.step_battle_intro_names();
    }
    assert_eq!(world.battle.intro_names_frames, 1);
    world.step_battle_intro_names();
    assert_eq!(world.battle.intro_names_frames, 0);
}

#[test]
fn an_advantage_holds_the_names_longer() {
    let mut world = intro_world();
    world.battle_ctx.formation_advantage = 2;
    world.arm_battle_intro_names();
    assert_eq!(world.battle.intro_names_frames, 0x78);
}

#[test]
fn monster_0xb5_opens_without_names() {
    let mut world = intro_world();
    world.actors[1].battle_monster_id = Some(0xB5);
    world.arm_battle_intro_names();
    assert_eq!(world.battle.intro_names_frames, 0);
}

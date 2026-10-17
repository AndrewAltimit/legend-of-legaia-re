//! The cast modules' `+0x1BA` shatter writers, driven through the band seam:
//! the word they raise is what `World::battle_actor_shatter` hands the hosts.

use super::*;

/// A lone member's row with engine monsters `1..5` seated.
fn shatter_world() -> World {
    let mut world = module_code_world();
    for i in 1..5 {
        world.actors[i].battle_monster_id = Some(1);
    }
    for i in 5..12 {
        world.actors[i].active = false;
        world.actors[i].battle.hp = 0;
    }
    world.casting.summon_actor_slot = Some(9);
    world.actors[9].active = true;
    world.actors[9].battle.hp = 100;
    world.actors[0].battle.active_target = 1;
    world
}

/// Run PROT 0908 from its finisher arm to the end, returning the peak
/// shatter word each engine slot reached.
fn run_zenoir_tail(world: &mut World) -> Vec<u16> {
    use legaia_engine_cast::seru_magic_ticks::ZENOIR_FINISH_ARM;
    world.casting.module_phase = ZENOIR_FINISH_ARM;
    let mut peak = vec![0u16; world.actors.len()];
    for _ in 0..4000 {
        let Some(run) = world.run_cast_module_code(0x86, 0) else {
            break;
        };
        for (p, a) in peak.iter_mut().zip(&world.actors) {
            *p = (*p).max(a.battle.shatter);
        }
        if !run.busy {
            break;
        }
    }
    peak
}

/// Zenoir's arm 12 breaks up the monsters the blast killed, a speed scalar a
/// tick for as long as the arm's `scalar * 192` countdown runs, and leaves
/// the living ones whole. The word reaches the draw plan as the shatter.
#[test]
fn zenoir_shatters_the_monsters_it_killed() {
    let mut world = shatter_world();
    world.actors[2].battle.hp = 0;
    let peak = run_zenoir_tail(&mut world);
    assert!(peak[2] >= 1000, "the dead monster comes apart: {}", peak[2]);
    assert_eq!(peak[3], 0, "a living monster stays whole");
    assert_eq!(peak[0], 0, "the party is never shattered");
    assert!(
        peak[5..].iter().all(|&p| p == 0),
        "an empty seat's colour word is a real zero: {peak:?}"
    );
    let s = world
        .battle_actor_shatter(2)
        .expect("the word stays up: nothing clears it in the fight");
    assert_eq!(s.word & 0xFFFF, u32::from(world.actors[2].battle.shatter));
    assert_eq!(s.row.flags, 0, "a battle node reads the zero row");
}

/// The boss stand-in: a scripted fight whose latched victim's record carries
/// `+0x20` shatters nothing.
#[test]
fn zenoir_spares_a_scripted_fight_whose_victim_has_the_boss_byte() {
    let mut world = shatter_world();
    let mut def = legaia_engine_battle::monster_catalog::MonsterDef::new(1, "Boss", 300, 20);
    def.wide_texture_page = 1;
    world.tables.monster_catalog.insert(def);
    world.battle_ctx.scripted_fight = 4;
    world.actors[2].battle.hp = 0;
    let peak = run_zenoir_tail(&mut world);
    assert_eq!(peak[2], 0);
}

/// Gimard's settle arm dissolves the creature on its own seat.
#[test]
fn gimard_settle_arm_breaks_up_the_creature() {
    use legaia_engine_cast::seru_magic_ticks::{GIMARD_SETTLE_ARM, GIMARD_SHATTER_STEP_PER_TICK};
    let mut world = shatter_world();
    world.casting.module_phase = GIMARD_SETTLE_ARM;
    world.run_cast_module_code(0x81, 0).expect("PROT 0903 runs");
    assert_eq!(
        world.actors[9].battle.shatter, GIMARD_SHATTER_STEP_PER_TICK,
        "one pass of the settle arm on the creature seat"
    );
}

/// The Spikefish flute's outcome arm breaks its creature up a step a pass
/// while the arm's countdown holds it.
#[test]
fn spikefish_outcome_arm_breaks_up_the_creature_while_it_holds() {
    use legaia_engine_cast::cast_module_ticks::{
        SPIKEFISH_OUTCOME_ARM, SPIKEFISH_SHATTER_STEP_PER_TICK,
    };
    let mut world = shatter_world();
    assert_eq!(world.cast_module_for(0x97), Some(925));
    world.casting.module_phase = SPIKEFISH_OUTCOME_ARM;
    world.casting.module_cam.countdown.0 = 0x40;
    for _ in 0..3 {
        world.run_cast_module_code(0x97, 0).expect("PROT 0925 runs");
    }
    assert_eq!(
        world.casting.module_phase, SPIKEFISH_OUTCOME_ARM,
        "still held"
    );
    assert_eq!(
        world.actors[9].battle.shatter,
        3 * SPIKEFISH_SHATTER_STEP_PER_TICK
    );
}

//! Nighto's verdict: drawn once, resisted in a scripted fight, written on both
//! fork branches.

use super::*;

/// The band draws Nighto's verdict once and holds it - retail's arm 0 parks
/// both rolls in the module words `0x801F8534` / `0x801F853C` and arm 13 only
/// reads them, so a verdict re-rolled per frame could flicker a resist into a
/// kill mid-cast.
#[test]
fn the_nighto_verdict_is_drawn_once_and_held_for_the_whole_cast() {
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x85), Some(907));
    world.casting.summon_actor_slot = Some(7);
    world.actors[0].battle.active_target = 3;
    assert_eq!(world.casting.module_nighto_outcome, None);
    world.run_cast_module_code(0x85, 0).expect("PROT 0907 runs");
    let first = world
        .casting
        .module_nighto_outcome
        .expect("the band drew the verdict on the first tick");
    let cursor = world.rng_state;
    for _ in 0..8 {
        world.run_cast_module_code(0x85, 0);
        assert_eq!(
            world.casting.module_nighto_outcome,
            Some(first),
            "the verdict changed mid-cast"
        );
    }
    assert_eq!(
        world.rng_state, cursor,
        "a later tick drew from the RNG again"
    );
}

/// The `+0x20` immunity is the byte AND the scripted-fight flag: retail's
/// resist force reads `ctx[+0x287] != 0 && record[+0x20] != 0`, so an "immune"
/// monster in a random encounter is not immune at all.
#[test]
fn a_wide_texture_page_monster_resists_nighto_only_in_a_scripted_fight() {
    use legaia_engine_cast::seru_magic_ticks::NightoOutcome;
    let verdict = |wide: u8, scripted: u8, seed: u32| -> NightoOutcome {
        let mut world = module_code_world();
        world.rng_state = seed;
        let mut def = legaia_engine_battle::monster_catalog::MonsterDef::new(77, "Big", 300, 20);
        def.wide_texture_page = wide;
        world.tables.monster_catalog.insert(def);
        world.actors[3].battle_monster_id = Some(77);
        world.battle_ctx.scripted_fight = scripted;
        world.casting.summon_actor_slot = Some(7);
        world.actors[0].battle.active_target = 3;
        run_nighto_to_the_fork(&mut world);
        world.casting.module_nighto_outcome.expect("verdict drawn")
    };
    // `resisted()`, not a variant compare: the resist is one of two
    // independent bits, and the kill word still picks which leg the cast
    // takes - so a forced resist is `KillResisted` or `ConfuseResisted`
    // depending on a roll this test does not pin.
    // A stream on which the plain throw does not resist, so a resist below
    // can only be the forced one. Scanned rather than pinned: the resist
    // throw's draw index depends on how many draws the band takes first, and
    // roughly half of all streams pass.
    let seed = (0..64u32)
        .map(|i| i.wrapping_mul(0x9E37_79B9).wrapping_add(1))
        .find(|&s| !verdict(0, 0, s).resisted())
        .expect("some stream throws no natural resist");
    assert!(
        verdict(1, 4, seed).resisted(),
        "the byte plus the scripted flag forces the resist"
    );
    // Same monster, same rolls, random encounter: the gate is open, so the
    // fork runs. Whichever side it lands on, it is not the forced resist.
    assert!(
        !verdict(1, 0, seed).resisted(),
        "a random encounter must not force the resist"
    );
    assert!(
        !verdict(0, 4, seed).resisted(),
        "a monster without the byte must not force the resist"
    );
}

/// The fork actually reaches the victim: a killed victim's HP is zeroed and a
/// confused one takes `+0x16E |= 0x380`. Before the verdict was driven, the
/// tick always took the resisted branch and wrote neither.
#[test]
fn the_nighto_fork_writes_the_victim_on_both_of_its_branches() {
    use legaia_engine_cast::seru_magic_ticks::{NIGHTO_CONFUSE_BITS, NightoOutcome};
    let mut kills = 0usize;
    let mut confuses = 0usize;
    for seed in 0..512u32 {
        let mut world = module_code_world();
        world.rng_state = seed.wrapping_mul(0x9E37_79B9).wrapping_add(1);
        world.casting.summon_actor_slot = Some(7);
        world.actors[0].battle.active_target = 3;
        run_nighto_to_the_fork(&mut world);
        match world.casting.module_nighto_outcome {
            Some(NightoOutcome::Kill) => {
                kills += 1;
                assert_eq!(world.actors[3].battle.hp, 0, "the kill branch zeroes HP");
            }
            Some(NightoOutcome::Confuse) => {
                confuses += 1;
                assert_eq!(
                    world.actors[3].battle.field_flags & NIGHTO_CONFUSE_BITS,
                    NIGHTO_CONFUSE_BITS,
                    "the confuse branch sets the +0x16E bits"
                );
            }
            _ => {}
        }
    }
    // 512 seeds, not 64: `World::next_rng` is an LCG mod 2^32, whose low
    // three bits have period 8 and correlate with the next draw's parity, so
    // a short consecutive-seed sample can miss the `kill % 8 == 0` branch
    // entirely (64 seeds yields none).
    assert!(kills > 0, "no seed took the kill branch");
    assert!(confuses > 0, "no seed took the confuse branch");
}

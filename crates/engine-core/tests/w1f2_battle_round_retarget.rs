//! The round boundary's **stale-target re-pick**, driven by the live battle
//! loop rather than called directly.
//!
//! `BattleRound::boundary` (the actor sweep `FUN_801D88CC`) runs every round
//! of every battle, so it is trivially reached. The call one level deeper is
//! not: `FUN_801DB8B4` - the first-living-monster scan - runs only for a
//! party member whose `active_target` is a monster that **died**, and only at
//! the next round boundary after that death. A fight that ends inside one
//! round never gets there, and every driven fight in the ladders does.
//!
//! Two properties of the scan, both taken from what the disassembly does
//! rather than from what the port currently returns:
//!
//! 1. **A survivor is found.** With a living monster on the field, no party
//!    member may still be aiming at a dead one once a boundary has run.
//! 2. **The answer is the *first* living slot**, not any living slot and not
//!    slot 0 of the band - retail scans the monster band forward from
//!    `party_count` and returns the first hit, and returns the band end as
//!    the wiped-out sentinel.
//!
//! ## Why the fixture pins the round, not the frame
//!
//! The re-pick fires at the round boundary, not at the moment of death, so a
//! frame-by-frame assertion is wrong in both directions: it fails on the
//! legal frames between the death and the boundary. The test gates on
//! `monster_ai_state.mode_flags` (the port's round counter, retail
//! `ctx[+0x28A]`), which `advance_battle_mode` bumps immediately before
//! `BattleRound::boundary` in the same arm - so "the counter moved" is
//! exactly "a boundary ran".
//!
//! ## The wiped-out sentinel is not reachable from the live loop
//!
//! `FUN_801DB8B4`'s other answer - the band end, returned when no monster is
//! alive - has no live-loop path: the loop's round arm ends the battle as
//! soon as `monsters_alive` goes false, so a boundary never runs over an
//! empty band. Measured, not assumed: running this fixture with every
//! monster at 1 HP ends with a party member still aiming at the corpse it
//! killed, because the fight is over before any boundary could clean the
//! target up. So that arm is a reach-triage row, not an assertion here - a
//! test that demanded the cleanup would be asserting a beat retail never
//! reaches either.
//!
//! Disc-free. The fight is the live loop's own (`live_gameplay_loop` on, not
//! player-driven); the front monster is seated at 1 HP so it falls early, but
//! the party still has to land the hit that kills it - nothing here zeroes a
//! monster.

use legaia_engine_core::world::{SceneMode, World};

const PARTY: u8 = 3;
const MONSTERS: u8 = 3;

/// The live-loop fight: three party members against three monsters, the front
/// one one hit from death and the two behind it effectively unkillable inside
/// the tick budget, so the fight reaches a *partial* wipe and stays there.
fn live_three_monster_battle() -> World {
    let mut world = World::new();
    world.enter_battle(PARTY, MONSTERS);
    world.live_gameplay_loop = true;
    world.battle_player_driven = false;
    for slot in 0..(PARTY + MONSTERS) as usize {
        if let Some(s) = world.battle_speed.get_mut(slot) {
            *s = 10;
        }
    }
    for i in 0..PARTY as usize {
        world.actors[i].battle.max_hp = 9999;
        world.actors[i].battle.hp = 9999;
        world.set_battle_attack(i as u8, 60);
    }
    let band = PARTY as usize;
    world.actors[band].battle.max_hp = 1;
    world.actors[band].battle.hp = 1;
    for i in (band + 1)..(PARTY + MONSTERS) as usize {
        world.actors[i].battle.max_hp = 30_000;
        world.actors[i].battle.hp = 30_000;
    }
    world
}

fn first_living_monster(world: &World) -> Option<usize> {
    (PARTY as usize..(PARTY + MONSTERS) as usize).find(|&i| world.actors[i].battle.liveness != 0)
}

fn living_monsters(world: &World) -> usize {
    (PARTY as usize..(PARTY + MONSTERS) as usize)
        .filter(|&i| world.actors[i].battle.liveness != 0)
        .count()
}

#[test]
fn a_dead_target_is_re_pointed_at_the_first_living_monster_at_the_round_boundary() {
    let mut world = live_three_monster_battle();
    let aim = PARTY; // the 1-HP front monster
    for i in 0..PARTY as usize {
        world.actors[i].battle.active_target = aim;
    }
    assert_eq!(living_monsters(&world), MONSTERS as usize);

    let mut death_round: Option<u8> = None;
    let mut checked = 0usize;
    for _ in 0..60_000 {
        world.tick();
        if world.mode != SceneMode::Battle {
            break;
        }
        let alive = living_monsters(&world);
        if alive == 0 {
            break;
        }
        let round = world.monster_ai_state.mode_flags;
        if world.actors[aim as usize].battle.liveness == 0 && death_round.is_none() {
            death_round = Some(round);
        }
        let Some(at) = death_round else { continue };
        if round == at {
            continue;
        }
        // A round boundary has run since the aimed-at monster fell, and the
        // band still has survivors.
        let survivor = first_living_monster(&world).expect("alive > 0");
        for i in 0..PARTY as usize {
            let t = world.actors[i].battle.active_target as usize;
            assert!(
                t < world.actors.len() && world.actors[t].battle.liveness != 0,
                "party slot {i} still aims at slot {t}, which is not a living \
                 monster, a full round after its target fell ({alive} alive) - \
                 the boundary's stale-target re-pick did not run"
            );
            assert_eq!(
                t, survivor,
                "the re-pick must return the *first* living monster slot \
                 (retail scans the band forward from party_count), not any \
                 living slot"
            );
        }
        checked += 1;
        if checked >= 3 {
            break;
        }
    }

    assert!(
        death_round.is_some(),
        "the aimed-at monster never died - the fight never reached the state \
         the re-pick exists for"
    );
    assert!(
        checked > 0,
        "the aimed-at monster died but no round boundary ran afterwards, so \
         the re-pick was never exercised"
    );
    eprintln!("w1f2 battle-round retarget: {checked} boundaries checked after the wipe");
}

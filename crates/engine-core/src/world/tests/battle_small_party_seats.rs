//! Small-party seating at the `World` seam: a party of one or two against one
//! to three monsters. Retail seats monster `k` at pool slot `3 + k` whatever
//! the party size; the engine seats it at `party_count + k`, and every crossing
//! goes through [`World::battle_seat_map`].

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// `pc` party seats then `mc` monster seats, all standing at 100 HP.
fn seated(pc: u8, mc: u8) -> World {
    let mut world = World::new();
    world.enter_battle(pc, mc);
    for i in 0..usize::from(pc + mc) {
        let a = &mut world.actors[i];
        a.active = true;
        a.battle.liveness = 1;
        a.battle.hp = 100;
        a.battle.max_hp = 100;
        if i >= usize::from(pc) {
            a.battle_monster_id = Some(1);
        }
    }
    world
}

fn kill(world: &mut World, slot: u8) {
    let a = &mut world.actors[usize::from(slot)];
    a.battle.hp = 0;
    a.battle.liveness = 0;
}

#[test]
fn the_seat_map_puts_every_small_partys_first_monster_on_pool_slot_three() {
    for pc in 1..=3u8 {
        for mc in 1..=3u8 {
            let world = seated(pc, mc);
            let seats = world.battle_seat_map();
            for e in 0..pc + mc {
                let r = seats.retail(e);
                assert_eq!(r.is_party(), e < pc, "pc {pc} mc {mc} slot {e}");
                if e >= pc {
                    assert_eq!(r.get(), 3 + e - pc);
                }
                assert_eq!(
                    world.retail_battle_pool_slot(usize::from(e)),
                    Some(usize::from(r.get()))
                );
            }
        }
    }
}

/// `FUN_801DB124` over the engine's seating: a strike at a dead monster
/// re-rolls onto a living monster, never onto the party and never past the
/// row, for every small seating and every choice of survivor.
#[test]
fn a_dead_target_redirect_stays_on_the_monster_row_of_any_small_party() {
    for pc in 1..=3u8 {
        for mc in 2..=3u8 {
            for survivor in 0..mc {
                for dead in (0..mc).filter(|&k| k != survivor) {
                    let mut world = seated(pc, mc);
                    for k in (0..mc).filter(|&k| k != survivor) {
                        kill(&mut world, pc + k);
                    }
                    let got = world.redirect_dead_battle_target(pc + dead, 3, 0);
                    assert_eq!(got, pc + survivor, "pc {pc} mc {mc} dead {dead}");
                }
            }
        }
        // And a party target re-rolls inside the party.
        if pc > 1 {
            let mut world = seated(pc, 1);
            kill(&mut world, 0);
            let got = world.redirect_dead_battle_target(0, 3, 0);
            assert!((1..pc).contains(&got), "pc {pc}: {got}");
        }
    }
}

/// The AI companion (`DAT_8007BD10[slot] == 4`, Terra) sits in the retail
/// Noa + Terra party of two. Its target roll names a pool slot `3 + k`; the
/// strike must land on that monster's engine slot `2 + k`, alive.
#[test]
fn the_ai_companion_strikes_a_living_monster_of_the_two_member_party() {
    for mc in 1..=3u8 {
        for dead in 0..mc {
            let mut attacks = 0;
            for seed in 0..64u32 {
                let mut world = World::new();
                world.load_party(legaia_save::Party::zeroed(4));
                world.set_active_party(vec![1, 3]);
                let mut w = {
                    world.enter_battle(2, mc);
                    world
                };
                for i in 0..usize::from(2 + mc) {
                    let a = &mut w.actors[i];
                    a.active = true;
                    a.battle.liveness = 1;
                    a.battle.hp = 100;
                    a.battle.max_hp = 100;
                    a.battle.field_flags = 0;
                    if i >= 2 {
                        a.battle_monster_id = Some(1);
                    }
                }
                if mc > 1 {
                    kill(&mut w, 2 + dead);
                }
                w.rng_state = seed.wrapping_mul(0x9E37_79B9).wrapping_add(1);
                w.arm_party_physical(1);
                let a = &w.actors[1].battle;
                if a.action_category != 3 {
                    continue;
                }
                attacks += 1;
                let t = a.active_target;
                assert!(
                    (2..2 + mc).contains(&t),
                    "mc {mc} seed {seed}: target {t} is not a monster seat"
                );
                assert_ne!(w.actors[usize::from(t)].battle.hp, 0, "mc {mc} seed {seed}");
            }
            assert!(attacks > 0, "mc {mc}: no seed took the attack arm");
        }
    }
}

/// The per-character tables key on the seat's roster id, not the seat: a
/// Vahn + Gala party seats Gala (id 3) on seat 1.
#[test]
fn a_party_seat_reads_its_roster_id() {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(4));
    world.set_active_party(vec![0, 2]);
    world.mode = SceneMode::Battle;
    assert_eq!(world.battle_party_char_id(0), 1);
    assert_eq!(world.battle_party_char_id(1), 3);
}

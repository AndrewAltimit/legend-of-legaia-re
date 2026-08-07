use super::*;

// --- Level-up banner ---

#[test]
fn apply_battle_xp_sets_level_up_banner() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    // Slot 0 must be alive for the split to credit XP.
    world.actors[0].battle.hp = 100;
    // Retail XP table: 121 XP to reach level 2 (DAT_80076AF4 via FUN_801E9504;
    // the New Game "Next Level 121"). The reward is scaled 3/4 + ceil-split
    // (FUN_8004E568): feed 161 so the lone member receives
    // 161 - (161 >> 2) = 121 >= the 121 threshold.
    world.apply_battle_xp(161);
    let banner = world
        .current_level_up_banner
        .as_ref()
        .expect("level-up banner should be set");
    assert_eq!(banner.char_id, 0);
    assert_eq!(banner.new_level, 2);
    assert_eq!(banner.hp_gained, 10); // default StatGain
    assert_eq!(banner.mp_gained, 5);
    assert_eq!(
        banner.frames_remaining,
        crate::levelup::LevelUpBanner::DEFAULT_FRAMES
    );
}

/// A fight that levels several members shows a banner for each, one after
/// the other.
///
/// The banner is a single slot and `apply_battle_xp` used to assign it inside
/// the per-member loop, so every leveller after the first overwrote its
/// predecessor in the same frame: a battle that levelled three showed one
/// banner. The queue is what makes the other two reachable, so this test
/// walks the whole drain rather than only checking that a banner exists.
#[test]
fn every_member_who_levels_gets_their_own_banner_in_turn() {
    let mut world = World {
        party_count: 3,
        ..World::default()
    };
    for slot in 0..3 {
        world.actors[slot].battle.hp = 100;
    }
    // Scaled 3/4 + ceil-split over 3 alive: ceil((486 - 486>>2)/3) = 122
    // each, past the 121 threshold, so all three level in one call.
    let results = world.apply_battle_xp(486);
    assert_eq!(results.len(), 3, "all three members should level");

    let mut seen = vec![
        world
            .current_level_up_banner
            .as_ref()
            .expect("first banner")
            .char_id,
    ];
    assert_eq!(
        world.pending_level_up_banners.len(),
        2,
        "the other two levellers must be queued, not dropped"
    );

    // Drain: each banner runs its countdown, then the next takes the slot.
    for _ in 0..2 {
        for _ in 0..=crate::levelup::LevelUpBanner::DEFAULT_FRAMES {
            world.tick();
        }
        seen.push(
            world
                .current_level_up_banner
                .as_ref()
                .expect("queued banner should take the slot")
                .char_id,
        );
    }
    seen.sort_unstable();
    assert_eq!(
        seen,
        vec![0, 1, 2],
        "each member who levelled should get their own banner"
    );
}

#[test]
fn apply_battle_xp_skips_dead_members() {
    let mut world = World {
        party_count: 3,
        ..World::default()
    };
    // Alive: slots 0 + 2. Dead: slot 1 (HP = 0).
    world.actors[0].battle.hp = 100;
    world.actors[1].battle.hp = 0;
    world.actors[2].battle.hp = 100;
    // Scaled 3/4 + ceil-split over 2 alive: ceil((324 - 324>>2)/2) = ceil(243/2)
    // = 122 each; both reach L2 (121 threshold).
    let results = world.apply_battle_xp(324);
    let slot_ids: Vec<u8> = results.iter().map(|r| r.char_id).collect();
    assert!(slot_ids.contains(&0));
    assert!(slot_ids.contains(&2));
    assert!(
        !slot_ids.contains(&1),
        "dead slot 1 must not appear in level-up results"
    );
}

#[test]
fn apply_battle_xp_no_alive_returns_empty() {
    let mut world = World {
        party_count: 3,
        ..World::default()
    };
    // No actor with HP > 0 → nobody to credit.
    let results = world.apply_battle_xp(500);
    assert!(results.is_empty());
    assert!(world.current_level_up_banner.is_none());
}

#[test]
fn apply_battle_loot_rolls_drop_item_when_rate_is_max() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};
    let mut cat = MonsterCatalog::new();
    let mut def = MonsterDef::new(7, "Slime", 10, 5);
    def.drop_item = Some(0x42);
    def.drop_rate_q8 = 255; // near-guaranteed roll
    cat.insert(def);
    let formation = FormationDef::new(1000, vec![FormationSlot::new(7)]);
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    let rewards = world.apply_battle_loot(&formation, &cat);
    assert_eq!(rewards.drops, vec![0x42]);
    assert_eq!(world.inventory.get(&0x42).copied(), Some(1));
}

#[test]
fn apply_basic_attack_queues_hit_fx_for_damaged_monster() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    // Slot 0 attacker, slot 1 a living monster.
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.hp = 60;
    world.actors[1].battle.max_hp = 60;
    world.actors[1].battle.liveness = 1;
    world.battle_ctx.active_actor = 0;
    // Arm the strike target the way every production path does (retail's
    // `+0x1DD` is always written before the SM strikes; the resolver honors
    // it on either band, so a fixture must not lean on the fallback).
    world.actors[0].battle.active_target = 1;
    // Give the attacker enough ATK to chip the monster (>defense).
    world.battle_attack[0] = 40;
    world.battle_defense[1] = 10;
    world.apply_basic_attack();
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1);
    assert_eq!(fx[0].target_slot, 1);
    assert!(fx[0].amount > 0);
    assert!(!fx[0].is_heal);
    // Drain empties the queue.
    assert!(world.drain_battle_hit_fx().is_empty());
}

/// One basic strike, run against a fixed RNG seed; returns
/// `(damage, rng_advanced)`.
fn one_basic_strike(attack: u16, defense: u16, gate: bool) -> (u16, bool) {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.rng_state = 0xABCD_1234;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.hp = 60_000;
    world.actors[1].battle.max_hp = 60_000;
    world.actors[1].battle.liveness = 1;
    world.battle_attack[0] = attack;
    world.battle_defense[1] = defense;
    world.use_damage_finish = gate;
    let rng_before = world.rng_state;
    world.battle_ctx.active_actor = 0;
    world.apply_basic_attack();
    let dmg = world
        .drain_battle_hit_fx()
        .first()
        .map(|f| f.amount)
        .unwrap_or(0);
    (dmg, world.rng_state != rng_before)
}

/// The melee roll pair (`FUN_801EC3E4`) is what a basic strike runs, and its
/// **underdog rewrite** is the property a flat `attack - defense` model got
/// wrong: an attacker whose ATK is under the defender's DEF does not chip for
/// the old `min_floor` of 1, it lands a scaling hit.
#[test]
fn apply_basic_attack_runs_the_melee_roll_pair() {
    // A comfortable attacker out-damages a hopeless one, and both roll.
    let (strong, drew_strong) = one_basic_strike(400, 10, false);
    let (weak, drew_weak) = one_basic_strike(10, 400, false);
    assert!(drew_strong && drew_weak, "each strike rolls attack + guard");
    assert!(strong > weak, "{strong} vs {weak}");

    // The old model floored ATK <= DEF at exactly 1. Retail's rewrite floors
    // the *plain-swing* case at `guard + rand%3 + 3 - guard`, so the weak hit
    // is never the 1-damage chip that made real fights unwinnable.
    assert!(
        weak >= 3,
        "underdog rewrite floors above the old 1, got {weak}"
    );

    // A strictly stronger attacker never does less; monotone in ATK.
    let (a, _) = one_basic_strike(40, 20, false);
    let (b, _) = one_basic_strike(200, 20, false);
    assert!(b > a, "{b} !> {a}");
}

/// `use_damage_finish` adds the finisher's **post** stages on top of the melee
/// roll (the 9999 cap being the observable one here); it no longer swaps in a
/// different pre-damage model, and the melee kernel's own Spirit-guard term
/// means the finisher's guard halve must not fire a second time.
#[test]
fn apply_basic_attack_damage_finish_gate() {
    // The melee kernel carries retail's own `guard + 9999` clamp
    // (`0x801EDA00`), so the cap holds with the gate either way.
    assert_eq!(one_basic_strike(60_000, 0, false).0, 9999);
    assert_eq!(one_basic_strike(60_000, 0, true).0, 9999);

    // And an ordinary hit is unchanged by the gate - the finisher's remaining
    // stages are all no-ops without equipment resists, and the guard halve
    // must NOT fire on top of the melee kernel's own guard-roll triple.
    assert_eq!(
        one_basic_strike(400, 10, false).0,
        one_basic_strike(400, 10, true).0
    );
}

#[test]
fn basic_attack_accrues_defender_spirit_gauge() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.hp = 200;
    world.actors[1].battle.max_hp = 200;
    world.actors[1].battle.liveness = 1;
    world.battle_attack[0] = 40;
    world.battle_defense[1] = 10;
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.active_target = 1;

    // The gauge fills by `damage * 100 / max_hp` (at least 1 per landing hit).
    assert_eq!(world.spirit_gauge(1), 0);
    world.apply_basic_attack();
    let first = world.drain_battle_hit_fx()[0].amount;
    let after_one = world.spirit_gauge(1);
    assert_eq!(after_one, (u32::from(first) * 100 / 200).max(1) as u16);
    // A second hit accumulates on top.
    world.actors[1].battle.liveness = 1;
    world.apply_basic_attack();
    let second = world.drain_battle_hit_fx()[0].amount;
    assert_eq!(
        world.spirit_gauge(1),
        after_one + (u32::from(second) * 100 / 200).max(1) as u16
    );
    assert!(!world.spirit_gauge_full(1));
}

#[test]
fn spirit_gauge_clamps_at_full() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    // A small max-HP so each ~50-damage hit is ~50% of the gauge.
    world.actors[1].battle.hp = 9999;
    world.actors[1].battle.max_hp = 100;
    world.actors[1].battle.liveness = 1;
    world.battle_attack[0] = 60;
    world.battle_defense[1] = 10;
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.active_target = 1;

    // 50 damage on a 100-HP gauge denominator -> pct 50 each hit.
    for _ in 0..4 {
        world.actors[1].battle.liveness = 1;
        world.apply_basic_attack();
        let _ = world.drain_battle_hit_fx();
    }
    assert_eq!(world.spirit_gauge(1), 100);
    assert!(world.spirit_gauge_full(1));
}

#[test]
fn spell_damage_accrues_spirit_gauge() {
    use crate::spells::{SpellElement, SpellOutcome};
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[1].battle.hp = 400;
    world.actors[1].battle.max_hp = 400;
    world.actors[1].battle.liveness = 1;

    // A 100-damage cast -> pct = 100*100/400 = 25.
    world.fold_spell_outcome(SpellOutcome::Damage {
        target: 1,
        amount: 100,
        element: SpellElement::Fire,
        weakness: false,
    });
    assert_eq!(world.spirit_gauge(1), 25);
    // Out-of-range slot reads 0, never panics.
    assert_eq!(world.spirit_gauge(250), 0);
}

/// A melee swing does **not** roll accuracy: `FUN_801EC3E4` never reads the
/// `+0x168` accuracy / evasion halfword, and `FUN_800402F4`'s selector-9 roll
/// (which the port used to gate this strike on) is the queued-action interrupt
/// check, not a to-hit check.
///
/// Gating melee on it inverted the fight against real disc stats: a party
/// slot's `+0x168` is seeded from AGL (~100 at level one), a monster's from
/// its record INT (~12 for the opening bestiary), so `acc / (acc + eva)` gave
/// the party an ~89% hit rate and the monsters ~11%.
#[test]
fn apply_basic_attack_does_not_roll_accuracy() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.rng_state = 0x1234_5678;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.hp = 60_000;
    world.actors[1].battle.max_hp = 60_000;
    world.actors[1].battle.liveness = 1;
    world.battle_attack[0] = 40;
    world.battle_defense[1] = 10;
    // A matchup the old roll would have whiffed most of the time.
    world.battle_accuracy[0] = 1;
    world.battle_evasion[1] = 500;
    let mut hits = 0;
    for _ in 0..200 {
        world.battle_ctx.active_actor = 0;
        world.apply_basic_attack();
        hits += world.drain_battle_hit_fx().len();
    }
    assert_eq!(hits, 200, "every melee swing connects");
}

#[test]
fn first_living_opponent_is_chosen_by_attacker_side() {
    let mut world = World {
        party_count: 2,
        ..World::default()
    };
    // Party slots 0,1 dead+alive; monster slots 2,3.
    world.actors[0].battle.liveness = 0;
    world.actors[1].battle.liveness = 1;
    world.actors[2].battle.liveness = 0;
    world.actors[3].battle.liveness = 1;
    // Party attacker -> first living monster (slot 3, since 2 is dead).
    assert_eq!(world.first_living_opponent_of(1), Some(3));
    // Monster attacker -> first living party member (slot 1, since 0 dead).
    assert_eq!(world.first_living_opponent_of(3), Some(1));
}

#[test]
fn next_living_combatant_round_robins_skipping_dead() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    world.actors[0].battle.liveness = 1; // party
    world.actors[2].battle.liveness = 1; // monster
    // After party (0) comes monster (2); after monster (2) wraps to party (0).
    assert_eq!(world.next_living_combatant(0), Some(2));
    assert_eq!(world.next_living_combatant(2), Some(0));
}

/// Three living actors with well-separated SPD: the per-turn key ranges
/// (`speed + rand()%(speed/2+1) + 1`) can't overlap, so the order is fixed
/// by SPD regardless of the RNG. Highest SPD acts first; each turn is
/// consumed; a fresh round is seeded once everyone has acted.
#[test]
fn initiative_orders_turns_by_speed_then_reseeds() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    // slot 0 (party) SPD 10, slot 1 (monster) SPD 50, slot 2 (monster) 30.
    // Key ranges: 11..=16, 51..=76, 31..=46 - disjoint.
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.liveness = 1;
    world.actors[2].battle.liveness = 1;
    world.battle_speed[0] = 10;
    world.battle_speed[1] = 50;
    world.battle_speed[2] = 30;
    // Fresh keys (all 0): the first pick seeds a round, then orders by SPD.
    assert_eq!(world.next_combatant_by_initiative(), Some(1)); // SPD 50
    assert_eq!(world.next_combatant_by_initiative(), Some(2)); // SPD 30
    assert_eq!(world.next_combatant_by_initiative(), Some(0)); // SPD 10
    // Round exhausted -> reseed -> highest SPD again.
    assert_eq!(world.next_combatant_by_initiative(), Some(1));
}

/// A dead actor never wins a turn even with the highest SPD: the selector
/// zeroes dead actors' keys (the `FUN_801daba4` first loop).
#[test]
fn initiative_skips_dead_high_speed_actor() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    world.actors[0].battle.liveness = 1; // party, SPD 20
    world.actors[1].battle.liveness = 0; // dead monster, SPD 90
    world.actors[2].battle.liveness = 1; // monster, SPD 40
    world.battle_speed[0] = 20;
    world.battle_speed[1] = 90;
    world.battle_speed[2] = 40;
    // Slot 1 is dead -> skipped; slot 2 (40) outruns slot 0 (20).
    assert_eq!(world.next_combatant_by_initiative(), Some(2));
    assert_eq!(world.next_combatant_by_initiative(), Some(0));
}

/// With no SPD anywhere the selector defers to round-robin slot order.
#[test]
fn initiative_falls_back_to_round_robin_without_speed() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    world.actors[0].battle.liveness = 1;
    world.actors[2].battle.liveness = 1;
    assert!(!world.any_battle_speed());
    world.battle_ctx.active_actor = 0;
    assert_eq!(world.next_combatant_by_initiative(), Some(2));
    world.battle_ctx.active_actor = 2;
    assert_eq!(world.next_combatant_by_initiative(), Some(0));
}

/// Setup seeding arms **every** living actor and consumes nothing, so round 1's
/// opener is the max-key pick like every later turn (`FUN_801DABA4`).
///
/// The seeder used to zero slot 0's key here, which handed slot 0 the opening
/// turn of every battle regardless of SPD. This setup is the case that exposed
/// it: a monster ten times the party's speed still could not open.
#[test]
fn seed_battle_initiative_arms_every_slot_and_the_fastest_opens() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    world.actors[0].battle.liveness = 1; // party, SPD 5
    world.actors[1].battle.liveness = 1; // monster, SPD 200
    world.battle_speed[0] = 5;
    world.battle_speed[1] = 200;
    world.seed_battle_initiative();
    // Nothing is consumed: both sides carry a live key into the first pick.
    assert!(
        world.actors[0].battle.init_key > 0,
        "slot 0's key must survive setup - consuming it is what let slot 0 \
         open every battle in the game"
    );
    assert!(world.actors[1].battle.init_key > 0);
    // The spread is wide enough that the roll cannot close it, so the fast
    // monster opens.
    assert!(world.actors[1].battle.init_key > world.actors[0].battle.init_key);
    assert_eq!(world.next_combatant_by_initiative(), Some(1));
}

/// `any_battle_speed` only fires for SPD carried by a *living* actor.
#[test]
fn any_battle_speed_requires_a_living_carrier() {
    let mut world = World::default();
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    assert!(!world.any_battle_speed());
    // SPD on a dead slot doesn't count.
    world.battle_speed[3] = 40;
    assert!(!world.any_battle_speed());
    // Living carrier flips the gate.
    world.actors[3].battle.liveness = 1;
    assert!(world.any_battle_speed());
}

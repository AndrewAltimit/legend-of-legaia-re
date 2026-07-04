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

#[test]
fn apply_basic_attack_damage_finish_gate() {
    // One-on-one auto-hit setup (no accuracy seeded -> no accuracy RNG), so the
    // only RNG the call can draw is the finisher's no-damage floor. Returns
    // (damage, did_draw_rng).
    let run = |attack: u16, defense: u16, gate: bool| -> (u16, bool) {
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
    };

    // Gate off: flat path. 40 atk vs 10 def -> 30, no RNG.
    assert_eq!(run(40, 10, false), (30, false));
    // Gate on, normal hit: same raw damage (no mitigation modelled), and the
    // finisher's rand fires only on a zeroed hit, so still no RNG.
    assert_eq!(run(40, 10, true), (30, false));

    // Zeroed hit (atk <= def). Gate on: no-damage floor (rand()%9 + 8 -> 8..=16)
    // and exactly one RNG draw. Gate off: flat min-floor of 1, no RNG.
    let (dmg_on, drew_on) = run(10, 40, true);
    assert!(
        (8..=16).contains(&dmg_on),
        "zeroed hit floored, got {dmg_on}"
    );
    assert!(drew_on, "zeroed hit draws one RNG");
    assert_eq!(run(10, 40, false), (1, false));

    // Overflow: the finisher caps at 9999 (the flat path caps at 0xFFFF).
    assert_eq!(run(50_000, 0, true), (9999, false));
    assert_eq!(run(50_000, 0, false).0, 50_000);
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

    // 40 atk vs 10 def -> 30 damage; pct = 30*100/200 = 15.
    assert_eq!(world.spirit_gauge(1), 0);
    world.apply_basic_attack();
    let _ = world.drain_battle_hit_fx();
    assert_eq!(world.spirit_gauge(1), 15);
    // A second identical hit accumulates.
    world.actors[1].battle.liveness = 1;
    world.apply_basic_attack();
    let _ = world.drain_battle_hit_fx();
    assert_eq!(world.spirit_gauge(1), 30);
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

#[test]
fn apply_basic_attack_rolls_accuracy_when_stats_are_seeded() {
    // Count landed strikes over many calls of a seeded attacker (acc) against a
    // high-evasion, can't-die target.
    let run = |rng_seed: u32| -> usize {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.rng_state = rng_seed;
        world.actors[0].battle.hp = 100;
        world.actors[0].battle.liveness = 1;
        world.actors[1].battle.hp = 60_000;
        world.actors[1].battle.max_hp = 60_000;
        world.actors[1].battle.liveness = 1;
        world.battle_attack[0] = 40;
        world.battle_defense[1] = 10;
        // Seed an ~even accuracy/evasion matchup so the roll engages.
        world.battle_accuracy[0] = 50;
        world.battle_evasion[1] = 50;
        let mut hits = 0;
        for _ in 0..200 {
            world.battle_ctx.active_actor = 0;
            world.apply_basic_attack();
            hits += world.drain_battle_hit_fx().len();
        }
        hits
    };

    let hits = run(0x1234_5678);
    // The roll genuinely engages: some strikes land and some whiff.
    assert!(
        hits > 0 && hits < 200,
        "seeded accuracy should produce a mix of hits and misses, got {hits}/200"
    );
    // Deterministic under a fixed RNG seed.
    assert_eq!(
        hits,
        run(0x1234_5678),
        "accuracy roll must be deterministic"
    );
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

/// Setup seeding consumes slot 0's key so the party lead opens round 1 and
/// the rest order by initiative behind it.
#[test]
fn seed_battle_initiative_lets_slot0_lead_round_one() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    world.actors[0].battle.liveness = 1; // party, SPD 10
    world.actors[1].battle.liveness = 1; // monster, SPD 50
    world.battle_speed[0] = 10;
    world.battle_speed[1] = 50;
    world.seed_battle_initiative();
    // Slot 0 consumed (leads round 1 separately); slot 1 still armed.
    assert_eq!(world.actors[0].battle.init_key, 0);
    assert!(world.actors[1].battle.init_key > 0);
    // The selector therefore picks slot 1 next, then slot 0 (after reseed).
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

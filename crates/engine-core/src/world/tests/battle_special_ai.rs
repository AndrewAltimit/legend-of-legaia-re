use super::*;

/// Monster-AI cast path: a monster whose record carries a castable spell
/// it can afford folds a real spell onto the party (HP drops, MP spent, a
/// damage popup queues) and parks the SM at `EndOfAction` so the loop
/// cycles - rather than the generic physical strike. RNG is pinned so the
/// cast-vs-strike roll lands on "cast".
#[test]
fn monster_ai_casts_a_castable_spell_under_fixed_rng() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::spells::SpellCatalog;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.monster_catalog = vanilla_monster_catalog();
    // Party member at slot 0.
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    // Bandit Boss (id 5) at slot 1: carries [Flame 0x20, Thunder Bolt 0x23]
    // and 10 MP - enough to afford either.
    world.actors[1].battle.max_hp = 120;
    world.actors[1].battle.hp = 120;
    world.actors[1].battle.mp = 10;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(5);
    world.set_battle_magic(1, 40);
    // Seed 0: the action picker's first `rand % (1 + magic_count)` (magic
    // count 2 -> `% 3`) lands on 1, so it casts magic[0] = Flame (0x20).
    world.rng_state = 0;

    let party_hp_before = world.actors[0].battle.hp;
    world.take_monster_turn(1);

    assert_eq!(
        world.actors[1].battle.params[0], 0x20,
        "picker chose Flame (magic_attacks[0])"
    );
    assert!(
        world.actors[0].battle.hp < party_hp_before,
        "the monster's spell dealt damage to the party"
    );
    assert!(world.actors[1].battle.mp < 10, "the monster spent MP");
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "a cast is the whole turn; SM parks at EndOfAction"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1, "one damage popup queued");
    assert!(!fx[0].is_heal, "damage-coloured popup");
    assert_eq!(fx[0].target_slot, 0, "the party member took the hit");
}

/// The opt-in `smarter_monster_targeting` tweak redirects a single-target
/// monster attack to the lowest-HP living party member, but does NOT move the
/// RNG stream: the faithful random pick is still rolled in full, so for every
/// seed the post-decision RNG state is byte-identical between the faithful and
/// smart modes - only the chosen slot differs. Default (faithful) behaviour is
/// thus bit-for-bit unchanged, and a smart-mode replay stays deterministic.
#[test]
fn smarter_targeting_redirects_to_lowest_hp_without_moving_rng() {
    fn world3() -> World {
        let mut w = World {
            party_count: 3,
            ..World::default()
        };
        w.mode = SceneMode::Battle;
        // Party HP: slot 1 is the lowest-HP living member.
        for (i, hp) in [200u16, 50, 200].into_iter().enumerate() {
            w.actors[i].battle.max_hp = 200;
            w.actors[i].battle.hp = hp;
            w.actors[i].battle.liveness = 1;
        }
        // Monster at slot 3 with no castable magic -> always a physical strike
        // at a single living party member (no scripted override / ring filter).
        w.actors[3].battle.max_hp = 100;
        w.actors[3].battle.hp = 100;
        w.actors[3].battle.liveness = 1;
        w
    }
    fn target_of(a: MonsterAction) -> u8 {
        match a {
            MonsterAction::Physical { target } => target,
            MonsterAction::Cast { targets, .. } => targets[0],
        }
    }

    let mut saw_redirect = false;
    for seed in 0u32..32 {
        let mut faithful = world3();
        faithful.rng_state = seed;
        let ft = target_of(faithful.pick_monster_action(3));
        let frng = faithful.rng_state;

        let mut smart = world3();
        smart.smarter_monster_targeting = true;
        smart.rng_state = seed;
        let st = target_of(smart.pick_monster_action(3));
        let srng = smart.rng_state;

        assert_eq!(st, 1, "seed {seed}: smart mode targets the lowest-HP slot");
        assert_eq!(
            frng, srng,
            "seed {seed}: RNG state identical across modes (override consumes none)"
        );
        if ft != 1 {
            saw_redirect = true;
        }
    }
    assert!(
        saw_redirect,
        "expected at least one seed where the faithful pick is not the lowest-HP slot"
    );
}

/// When the move-power table is installed and the monster's cast id resolves
/// to a power record, the special-attack damage rolls through the faithful
/// arts/physical kernel (move-power-seeded) instead of the MP-scaled spell
/// placeholder. Proven by comparing two identically-seeded worlds - one with
/// the table, one without - and asserting (a) the table changes the dealt
/// damage (the path engaged) and (b) the table path is deterministic.
#[test]
fn move_power_table_drives_monster_special_attack_damage() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::move_power::MovePowerCatalog;
    use crate::spells::SpellCatalog;
    use legaia_asset::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };

    // Synthetic PROT-0898-shaped overlay: map the monster's first magic id
    // (Bandit Boss id 5 -> Flame 0x20) to power record 1, with a large power so
    // the kernel's roll is clearly distinct from the MP-scaled placeholder.
    fn overlay_with_flame_power() -> Vec<u8> {
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1; // structural guard (id 4 -> idx 1)
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x20] = 1; // Flame -> power record 1
        // record 1 power 0x0BB8 (3000) -> >>2 = 750 roll-modulus base.
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xB8;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x0B;
        buf
    }

    fn run(install_table: bool) -> u16 {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.monster_catalog = vanilla_monster_catalog();
        // Party target at slot 0 with a healthy HP pool + seeded AGL/DEF so the
        // kernel reads live defender stats.
        world.actors[0].battle.max_hp = 4000;
        world.actors[0].battle.hp = 4000;
        world.actors[0].battle.liveness = 1;
        world.battle_accuracy[0] = 30;
        world.battle_defense[0] = 40;
        // Bandit Boss (id 5) at slot 1: casts magic[0] = Flame (0x20) on seed 0.
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_accuracy[1] = 25;
        world.set_battle_magic(1, 40);
        if install_table {
            world.move_power = MovePowerCatalog::from_overlay_0898(&overlay_with_flame_power());
            assert!(world.move_power.is_some(), "synthetic table installs");
        }
        world.rng_state = 0;

        let before = world.actors[0].battle.hp;
        world.take_monster_turn(1);
        assert_eq!(world.actors[1].battle.params[0], 0x20, "picker chose Flame");
        before - world.actors[0].battle.hp
    }

    let placeholder = run(false);
    let move_power = run(true);
    assert!(placeholder > 0, "placeholder path still deals damage");
    assert!(move_power > 0, "move-power path deals damage");
    assert_ne!(
        placeholder, move_power,
        "installing the move-power table changes the special-attack damage"
    );
    // Deterministic: same seed + table -> identical damage.
    assert_eq!(move_power, run(true), "move-power damage is deterministic");
}

/// A party member wearing an elemental-guard accessory takes HALF damage from
/// a monster special of the matching element - the `FUN_801ddb30` finisher's
/// party-resist ladder, reading the guard passive (`0x1D + element`) off the
/// character's rebuilt ability bitfield. A non-matching element (or no
/// accessory) passes through at full magnitude.
#[test]
fn elemental_guard_accessory_halves_matching_monster_special() {
    use crate::accessory_passives::AccessoryPassives;
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::move_power::MovePowerCatalog;
    use crate::spells::SpellCatalog;
    use legaia_asset::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };

    fn overlay_with_flame_power() -> Vec<u8> {
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x20] = 1; // Flame -> power record 1
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xB8;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x0B;
        buf
    }

    // `guard_passive`: None = bare character; Some(idx) = an accessory whose
    // passive index is `idx` equipped in the Goods slot.
    fn run(guard_passive: Option<u8>) -> u16 {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.monster_catalog = vanilla_monster_catalog();
        // Pin the attacking monster's element to 2 (Fire) so the resist
        // ladder has a real element to test against.
        world.monster_catalog.by_id.get_mut(&5).unwrap().element = 2;
        world.actors[0].battle.max_hp = 4000;
        world.actors[0].battle.hp = 4000;
        world.actors[0].battle.liveness = 1;
        world.battle_accuracy[0] = 30;
        world.battle_defense[0] = 40;
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_accuracy[1] = 25;
        world.set_battle_magic(1, 40);
        world.move_power = MovePowerCatalog::from_overlay_0898(&overlay_with_flame_power());

        let mut party = legaia_save::Party::zeroed(1);
        if let Some(idx) = guard_passive {
            world.set_accessory_passives(AccessoryPassives::from_entries([(0x50, idx)], []));
            let mut eq = party.members[0].equipment();
            eq.slots[7] = 0x50;
            party.members[0].set_equipment(eq);
        }
        world.roster = party;
        world.refresh_party_ability_bits();
        world.rng_state = 0;

        let before = world.actors[0].battle.hp;
        world.take_monster_turn(1);
        before - world.actors[0].battle.hp
    }

    let bare = run(None);
    let fire_guard = run(Some(0x1F)); // Fire Guard: matches element 2
    let water_guard = run(Some(0x1E)); // Water Guard: element 1, no match
    assert!(bare > 1, "baseline special deals real damage");
    assert_eq!(
        fire_guard,
        bare >> 1,
        "matching elemental guard halves the finished damage"
    );
    assert_eq!(
        water_guard, bare,
        "non-matching elemental guard leaves damage unchanged"
    );
}

/// The two "spirit gain up" finisher bits are the AP Boost accessory passives
/// (`0x28`/`0x29`): a wearer's spirit-art gauge charges faster from the same
/// hit, read off the rebuilt ability bitfield via `World::defender_resist`.
#[test]
fn ap_boost_accessory_accelerates_spirit_gauge() {
    use crate::accessory_passives::AccessoryPassives;

    fn gauge_after_hit(ap_boost_passive: Option<u8>) -> u16 {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.max_hp = 500;
        world.actors[0].battle.hp = 500;
        world.actors[0].battle.liveness = 1;
        let mut party = legaia_save::Party::zeroed(1);
        if let Some(idx) = ap_boost_passive {
            world.set_accessory_passives(AccessoryPassives::from_entries([(0x50, idx)], []));
            let mut eq = party.members[0].equipment();
            eq.slots[7] = 0x50;
            party.members[0].set_equipment(eq);
        }
        world.roster = party;
        world.refresh_party_ability_bits();
        world.accrue_spirit_gauge(0, 200); // pct = 200*100/500 = 40
        world.actors[0].battle.spirit_gauge
    }

    assert_eq!(gauge_after_hit(None), 40, "base accrual is pct");
    // AP Boost 1 (0x28 -> word1 0x100): +pct/10 = +4.
    assert_eq!(gauge_after_hit(Some(0x28)), 44);
    // AP Boost 2 (0x29 -> word1 0x200): +pct>>2 = +10.
    assert_eq!(gauge_after_hit(Some(0x29)), 50);
}

/// One-party-member battle world for the Run escape roll (`FUN_801E791C`):
/// party SPD vs enemy SPD, both sides at full HP, optional accessory passive
/// on the member's slot-7 equip.
fn escape_world(party_speed: u16, enemy_speed: u16, passive: Option<u8>) -> World {
    use crate::accessory_passives::AccessoryPassives;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.liveness = 1;
    world.battle_speed[0] = party_speed;
    world.battle_speed[1] = enemy_speed;
    let mut party = legaia_save::Party::zeroed(1);
    if let Some(idx) = passive {
        world.set_accessory_passives(AccessoryPassives::from_entries([(0x50, idx)], []));
        let mut eq = party.members[0].equipment();
        eq.slots[7] = 0x50;
        party.members[0].set_equipment(eq);
    }
    world.roster = party;
    world.refresh_party_ability_bits();
    world
}

/// The Run command's escape roll follows the retail `FUN_801E791C` score
/// compare: a fast party vs a slow enemy escapes on every seed (the enemy
/// score of 1 makes `roll_e` always 0 and the fail compare is strict `<`),
/// while a slow party vs a fast enemy is caught on essentially every seed
/// (`roll_p` pinned at 0 by a party score of ~1).
#[test]
fn run_escape_roll_follows_speed_and_hp_scores() {
    let mut always = 0;
    let mut rarely = 0;
    for seed in 0..50u32 {
        let mut fast = escape_world(1000, 1, None);
        fast.rng_state = seed;
        always += u32::from(fast.roll_battle_escape());

        let mut slow = escape_world(1, 1000, None);
        slow.rng_state = seed;
        rarely += u32::from(slow.roll_battle_escape());
    }
    assert_eq!(always, 50, "overwhelming speed advantage always escapes");
    assert!(
        rarely <= 2,
        "pinned-at-0 party roll is caught on almost every seed (got {rarely}/50 escapes)"
    );
}

/// Chicken King (Great Escape, passive `0x37` -> ability bit 55) forces the
/// party roll equal to the enemy roll, so even the worst matchup escapes;
/// Chicken Heart (Escape Boost, passive `0x34` -> bit 52) scales the party
/// roll 1.5x, raising the escape rate over the unboosted baseline.
#[test]
fn escape_accessories_fold_from_the_ability_bitfield() {
    for seed in 0..50u32 {
        let mut w = escape_world(1, 1000, Some(0x37));
        w.rng_state = seed;
        assert!(w.roll_battle_escape(), "Great Escape wins every compare");
    }

    let mut base = 0;
    let mut boosted = 0;
    for seed in 0..200u32 {
        let mut w = escape_world(30, 45, None);
        w.rng_state = seed;
        base += u32::from(w.roll_battle_escape());

        let mut w = escape_world(30, 45, Some(0x34));
        w.rng_state = seed;
        boosted += u32::from(w.roll_battle_escape());
    }
    assert!(
        boosted > base,
        "Escape Boost raises the escape rate ({boosted} vs {base} of 200)"
    );
}

/// **Wiring oracle for the `ctx+0x290` -> `ctx+0x291` formation latch.**
///
/// A pre-emptive strike (`ctx+0x291 == 2`) makes `FUN_801E791C` set the party
/// roll equal to the enemy roll, so the `roll_p < roll_e` compare can no longer
/// fail. The worst possible matchup - party score pinned at ~1 against an enemy
/// score of 1000 - is caught on essentially every seed without it, so if
/// `roll_battle_escape` stops folding [`World::battle_formation_latched`] into
/// the escape flags this flips from 50/50 escapes to ~0/50.
///
/// This is the assertion that was missing while the latch existed as a
/// write-only field: latching `+0x290` into `+0x291` and never reading it back
/// is indistinguishable from not latching at all.
#[test]
fn latched_preemptive_strike_makes_the_escape_compare_unfailable() {
    use legaia_engine_vm::battle_formulas::FormationAdvantage;

    let mut caught_without = 0;
    let mut escaped_with = 0;
    for seed in 0..50u32 {
        let mut w = escape_world(1, 1000, None);
        w.rng_state = seed;
        caught_without += u32::from(!w.roll_battle_escape());

        let mut w = escape_world(1, 1000, None);
        w.rng_state = seed;
        w.battle_formation_latched = FormationAdvantage::Preemptive;
        escaped_with += u32::from(w.roll_battle_escape());
    }
    assert!(
        caught_without >= 48,
        "baseline: the hopeless matchup is caught (got {caught_without}/50)"
    );
    assert_eq!(
        escaped_with, 50,
        "a latched pre-emptive strike escapes every seed"
    );
}

/// A **back attack** (`ctx+0x291 == 1`) is not the mirror of a pre-emptive
/// strike: `FUN_801E791C` compares the latched byte against `2` only, so a back
/// attack has no effect on the escape roll at all. Its cost is paid entirely in
/// the initiative lockout.
#[test]
fn latched_back_attack_does_not_touch_the_escape_roll() {
    use legaia_engine_vm::battle_formulas::FormationAdvantage;

    for seed in 0..50u32 {
        let mut plain = escape_world(30, 45, None);
        plain.rng_state = seed;
        let a = plain.roll_battle_escape();

        let mut backed = escape_world(30, 45, None);
        backed.rng_state = seed;
        backed.battle_formation_latched = FormationAdvantage::BackAttack;
        let b = backed.roll_battle_escape();

        assert_eq!(a, b, "seed {seed}: back attack must not change the roll");
    }
}

/// "Escape assured" overstates the pre-emptive arm. Retail sets `roll_p =
/// roll_e` *before* testing `ctx+0x287` (`801e7af0` then `801e7b14`), so a
/// scripted no-flee battle still catches the party even after a pre-emptive
/// strike. Only the forced-flee arm bypasses `+0x287`.
#[test]
fn latched_preemptive_strike_still_loses_to_the_no_escape_flag() {
    use legaia_engine_vm::battle_formulas::FormationAdvantage;

    for seed in 0..25u32 {
        let mut w = escape_world(1000, 1, None);
        w.rng_state = seed;
        w.battle_formation_latched = FormationAdvantage::Preemptive;
        w.battle_no_escape = true;
        assert!(
            !w.roll_battle_escape(),
            "seed {seed}: ctx+0x287 outranks the pre-emptive arm"
        );
    }
}

/// **Wiring oracle for the latch ordering.** The initiative seeder is the only
/// reader of the *unlatched* `ctx+0x290`, and the escape roll is the only
/// reader of the latched `ctx+0x291`, so the two passes must run in that order.
/// This drives the real sequence and checks both consumers saw their copy:
/// the monsters lose their round-one keys (the lockout ran against `+0x290`)
/// **and** the advantage survives into `+0x291` (the latch ran after it).
#[test]
fn seed_then_latch_feeds_both_formation_consumers() {
    use legaia_engine_vm::battle_formulas::FormationAdvantage;

    let mut w = escape_world(40, 40, None);
    w.party_count = 3;
    for slot in 0..7 {
        w.actors[slot].battle.liveness = 1;
        w.actors[slot].battle.hp = 100;
        w.actors[slot].battle.max_hp = 100;
        w.battle_speed[slot] = 40;
    }
    w.battle_formation = FormationAdvantage::Preemptive;

    w.seed_battle_initiative();
    // Lockout consumer: monster slots 3..=6 sat out round one.
    for slot in 3..7 {
        assert_eq!(
            w.actors[slot].battle.init_key, 0,
            "slot {slot}: pre-emptive strike zeroes the monster keys"
        );
    }
    // Party slots 1 and 2 keep real keys (slot 0's is consumed to open).
    assert!(w.actors[1].battle.init_key > 0);
    assert!(w.actors[2].battle.init_key > 0);

    w.latch_battle_formation();
    assert_eq!(
        w.battle_formation_latched,
        FormationAdvantage::Preemptive,
        "the advantage must survive into +0x291"
    );
    assert_eq!(
        w.battle_formation,
        FormationAdvantage::None,
        "+0x290 is cleared by the latch"
    );
    // And the escape roll now sees it.
    w.rng_state = 7;
    assert!(w.roll_battle_escape());
}

/// The side lockout splits on the engine's `party_count`, not on retail's fixed
/// slot-3 boundary. Retail always reserves three party slots, so
/// `apply_side_lockout` can hardcode `0..=2` / `3..=6`; the engine compacts and
/// seats the first monster at `party_count`. With a one-member party, slot 1 is
/// a **monster**, and a back attack must lock out slot 0 alone - the fixed
/// split would zero the monsters' keys instead, handing round one to the side
/// that was supposed to lose it.
#[test]
fn side_lockout_follows_party_count_not_the_fixed_retail_boundary() {
    use legaia_engine_vm::battle_formulas::FormationAdvantage;

    for (advantage, party_locked) in [
        (FormationAdvantage::BackAttack, true),
        (FormationAdvantage::Preemptive, false),
    ] {
        let mut w = escape_world(40, 40, None);
        w.party_count = 1; // slot 0 party, slots 1..=6 monsters
        for slot in 0..7 {
            w.actors[slot].battle.liveness = 1;
            w.actors[slot].battle.hp = 100;
            w.actors[slot].battle.max_hp = 100;
            w.battle_speed[slot] = 40;
        }
        w.battle_formation = advantage;
        w.rng_state = 3;
        w.reseed_initiative();

        assert_eq!(
            w.actors[0].battle.init_key == 0,
            party_locked,
            "{advantage:?}: party slot 0 lockout"
        );
        for slot in 1..7 {
            assert_eq!(
                w.actors[slot].battle.init_key == 0,
                !party_locked,
                "{advantage:?}: monster slot {slot} lockout"
            );
        }
    }
}

/// **Wiring oracle for the initiative kernel.** `FUN_801DA780` adds a
/// wounded-HP bonus to the rolled key, and for a party slot below a quarter HP
/// that bonus is `(max_hp - hp) >> 4` - far larger than the whole SPD roll.
///
/// With both slots at SPD 10 the rolled key spans `[11, 16]` (`rand % 6`); a party member at
/// 1/1000 HP adds `999 >> 4 = 62`, so its key lands in `[73, 78]` and beats the
/// healthy slot on *every* seed. The pre-remediation
/// `speed + rng % (speed/2 + 1) + 1` inline formula has no such term, so both
/// slots drew from the same `[11, 16]` band and this assertion fails on most
/// seeds.
#[test]
fn initiative_keys_carry_the_wounded_bonus_from_the_kernel() {
    for seed in 0..40u32 {
        let mut w = escape_world(10, 10, None);
        w.party_count = 2;
        w.rng_state = seed;
        for slot in 0..2 {
            w.actors[slot].battle.liveness = 1;
            w.actors[slot].battle.max_hp = 1000;
            w.battle_speed[slot] = 10;
        }
        w.actors[0].battle.hp = 1; // near death
        w.actors[1].battle.hp = 1000; // untouched
        w.battle_formation = legaia_engine_vm::battle_formulas::FormationAdvantage::None;

        w.reseed_initiative();

        let wounded = w.actors[0].battle.init_key;
        let healthy = w.actors[1].battle.init_key;
        assert!(
            (73..=78).contains(&wounded),
            "seed {seed}: wounded key {wounded} outside the kernel's [73,78] band"
        );
        assert!(
            (11..=16).contains(&healthy),
            "seed {seed}: healthy key {healthy} outside the plain [11,16] band"
        );
        assert!(
            wounded > healthy,
            "seed {seed}: the wounded bonus must win ({wounded} vs {healthy})"
        );
    }
}

/// Retail folds the escape accessories only over party members with live HP
/// (`+0x14C != 0`): a downed Chicken King wearer contributes nothing.
#[test]
fn downed_wearer_does_not_fold_escape_accessories() {
    let mut caught = 0;
    for seed in 0..50u32 {
        let mut w = escape_world(1, 1000, Some(0x37));
        w.actors[0].battle.liveness = 0;
        w.rng_state = seed;
        caught += u32::from(!w.roll_battle_escape());
    }
    assert!(
        caught >= 48,
        "downed wearer's assured-escape bit is ignored (got {caught}/50 caught)"
    );
}

/// With both the move-power table AND the element-affinity tables installed, a
/// monster special attack scales by `matrix[enemy_element][party_member_element]`
/// (`FUN_801dd864`). Proven by running the same seeded cast through a neutral
/// (100%) matrix vs a weakness (200%) matrix and asserting the weakness matrix
/// deals more damage. A `None` affinity table reproduces the neutral result
/// exactly - so the affinity is gated and never perturbs the RNG stream.
#[test]
fn element_affinity_scales_monster_special_attack_damage() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::move_power::MovePowerCatalog;
    use crate::spells::SpellCatalog;
    use legaia_asset::element_affinity::ElementAffinity;
    use legaia_asset::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };

    fn overlay_with_flame_power() -> Vec<u8> {
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x20] = 1;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xB8;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x0B;
        buf
    }

    // `affinity_pct = None` leaves the affinity table uninstalled (gated off);
    // `Some(pct)` installs a matrix whose only non-neutral cell is the attacking
    // monster's element row vs the party member's element column.
    fn run(affinity_pct: Option<u8>) -> u16 {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.monster_catalog = vanilla_monster_catalog();
        world.actors[0].battle.max_hp = 4000;
        world.actors[0].battle.hp = 4000;
        world.actors[0].battle.liveness = 1;
        world.battle_accuracy[0] = 30;
        world.battle_defense[0] = 40;
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_accuracy[1] = 25;
        world.set_battle_magic(1, 40);
        world.move_power = MovePowerCatalog::from_overlay_0898(&overlay_with_flame_power());
        // vanilla monster 5 has the default element 7 (neutral); the party
        // member at slot 0 is char id 1 -> element 3 in the synthetic table.
        let enemy_elem = world.monster_catalog.get(5).unwrap().element as usize;
        if let Some(pct) = affinity_pct {
            let mut matrix = [[100u8; 8]; 8];
            matrix[enemy_elem][3] = pct;
            world.element_affinity = Some(ElementAffinity {
                matrix,
                character_elements: vec![3; 8],
                summon_power: [[100; 8]; 3],
            });
        }
        world.rng_state = 0;

        let before = world.actors[0].battle.hp;
        world.take_monster_turn(1);
        assert_eq!(world.actors[1].battle.params[0], 0x20, "picker chose Flame");
        before - world.actors[0].battle.hp
    }

    let neutral = run(Some(100));
    let weakness = run(Some(200));
    let gated_off = run(None);
    assert!(neutral > 0, "neutral affinity still deals damage");
    assert_eq!(
        neutral, gated_off,
        "no affinity table reproduces the neutral 100% multiplier exactly"
    );
    assert!(
        weakness > neutral,
        "a 200% affinity cell deals more than the neutral 100%"
    );
    assert_eq!(
        weakness,
        run(Some(200)),
        "affinity-scaled damage is deterministic"
    );
}

/// A player Seru-magic cast scales by the element affinity of its summon
/// CREATURE vs the target - `matrix[summon-creature element][target element]`
/// (`FUN_801dd864`), not the casting character's element. The Gimard spell
/// (id `0x81`) summons the namesake "Gimard" creature, so the attacker element
/// is that creature's record element. With the creature resolved, the
/// magnitude rolls through the faithful summon kernel (the affinity scales
/// the attacker roll *inside* the roll, before the bonus-arm threshold), so
/// the affinity relation is monotonic rather than an exact post-roll
/// multiply; a `None` affinity table reproduces the neutral magnitude
/// exactly (the summon power-percent stage defaults to 100).
#[test]
fn element_affinity_scales_player_summon_cast_by_creature_element() {
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};
    use crate::spells::{SpellDef, SpellEffect, SpellElement, SpellTarget};
    use legaia_asset::element_affinity::ElementAffinity;

    const SUMMON_ELEM: usize = 2; // the "Gimard" creature's element
    const ENEMY_ELEM: usize = 5; // the target enemy's element

    // The Gimard summon spell. Damage placeholder is MP-scaled
    // (caster_mag * base_power / 8 - mdef); base_power chosen so the affinity
    // delta is well above the 1-HP clamp.
    fn gimard_spell() -> SpellDef {
        SpellDef {
            id: 0x81,
            name: "Gimard".into(),
            mp_cost: 4,
            element: SpellElement::Neutral,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::Damage {
                base_power: 100,
                element: SpellElement::Neutral,
            },
            anim_id: 0,
        }
    }

    fn run(affinity_pct: Option<u8>) -> u16 {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        // Catalog: the summon creature (matched by the spell's display name) and
        // the target enemy, each with a distinct element. The summon body's HP
        // is kept SMALL and the caster's AGL large so the attacker roll
        // dominates the bonus-arm threshold (`defender + summon_hp >
        // attacker`) at every pct exercised here - the bonus re-roll rebuilds
        // the roll WITHOUT the affinity scale (retail-faithful; covered by the
        // kernel tests), which would break the monotonic relation this test
        // pins.
        let mut catalog = MonsterCatalog::new();
        let mut creature = MonsterDef::new(10, "Gimard", 10, 10);
        creature.element = SUMMON_ELEM as u8;
        catalog.insert(creature);
        let mut enemy = MonsterDef::new(5, "Goblin", 120, 8);
        enemy.element = ENEMY_ELEM as u8;
        catalog.insert(enemy);
        world.monster_catalog = catalog;

        // Caster = party slot 0; enough MP to afford the cast.
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.mp = 40;
        world.actors[0].battle.liveness = 1;
        world.set_battle_magic(0, 40);
        world.battle_accuracy[0] = 200;
        // Target = enemy slot 1, identified to the catalog by monster id.
        world.actors[1].battle.max_hp = 4000;
        world.actors[1].battle.hp = 4000;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_defense[1] = 0;

        if let Some(pct) = affinity_pct {
            let mut matrix = [[100u8; 8]; 8];
            matrix[SUMMON_ELEM][ENEMY_ELEM] = pct;
            world.element_affinity = Some(ElementAffinity {
                matrix,
                character_elements: vec![3; 8],
                summon_power: [[100; 8]; 3],
            });
        }

        let def = gimard_spell();
        let before = world.actors[1].battle.hp;
        world.cast_spell_on_slots(0, &def, &[1]);
        before - world.actors[1].battle.hp
    }

    let neutral = run(Some(100));
    let weakness = run(Some(200));
    let resist = run(Some(50));
    let gated_off = run(None);
    assert!(neutral > 0, "neutral affinity still deals damage");
    assert_eq!(
        neutral, gated_off,
        "no affinity table reproduces the neutral 100% multiplier exactly"
    );
    assert!(
        weakness > neutral,
        "a 200% affinity raises the faithful roll ({weakness} vs {neutral})"
    );
    assert!(
        resist < neutral,
        "a 50% affinity lowers the faithful roll ({resist} vs {neutral})"
    );
}

/// The player Seru-magic cast path rolls the faithful summon kernel: the
/// HP delta produced by `cast_spell_on_slots` equals the value built by
/// composing `summon_predamage_lazy` + `damage_finish_lazy` directly with the
/// same seeds - summon-body stats from the namesake creature's catalog def,
/// caster AGL doubled, and the shared LCG drawn in retail call order.
#[test]
fn player_summon_cast_matches_the_summon_kernel_composition() {
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};
    use crate::spells::{SpellDef, SpellEffect, SpellElement, SpellTarget};
    use legaia_engine_vm::battle_formulas::{
        DamageFinish, DefenderResist, SummonRollActor, damage_finish_lazy, summon_predamage_lazy,
    };

    const SEED: u32 = 0xC0FFEE;

    fn build_world() -> World {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        let mut catalog = MonsterCatalog::new();
        let mut creature = MonsterDef::new(10, "Gimard", 100, 10);
        creature.intel = 36;
        creature.element = 2;
        catalog.insert(creature);
        let mut enemy = MonsterDef::new(5, "Goblin", 120, 8);
        enemy.element = 5;
        catalog.insert(enemy);
        world.monster_catalog = catalog;
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.mp = 40;
        world.actors[0].battle.liveness = 1;
        world.set_battle_magic(0, 40);
        world.battle_accuracy[0] = 25;
        world.actors[1].battle.max_hp = 4000;
        world.actors[1].battle.hp = 4000;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_accuracy[1] = 12;
        world.battle_defense[1] = 30;
        world.rng_state = SEED;
        world
    }

    // Expected value: the kernels composed directly, drawing from the same
    // LCG in the same order (attacker, defender, lazy bonus, lazy floor).
    struct Lcg(u32);
    impl Lcg {
        fn draw(&mut self) -> u16 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 & 0x7fff) as u16
        }
    }
    let mut lcg = Lcg(SEED);
    let summon = SummonRollActor {
        hp: 100,
        agl: 36,
        ..Default::default()
    };
    let target = SummonRollActor {
        hp: 4000,
        agl: 12,
        stat_a: 30,
        stat_b: 0,
        ..Default::default()
    };
    let rng2 = [lcg.draw(), lcg.draw()];
    let (atk, def) = summon_predamage_lazy(&summon, 25, &target, 100, 1, rng2, || lcg.draw());
    let finish = DamageFinish {
        predamage: atk.saturating_sub(def),
        attacker_slot: 7,
        defender_slot: 4,
        attacker_element: 2,
        defender_resist: DefenderResist::default(),
        defender_guarding: false,
        enemy_defender_halve: false,
        bypass_party_resist: false,
        summon_power_pct: 100,
        floor_rand: 0,
    };
    let expected = damage_finish_lazy(&finish, || lcg.draw()).min(9999) as u16;

    // Direct method call.
    let mut world = build_world();
    assert_eq!(
        world.player_summon_predamage(0, 1, 0x81),
        Some(expected),
        "player_summon_predamage composes the kernels"
    );

    // Whole cast path: the HP delta is the same value.
    let mut world = build_world();
    let spell = SpellDef {
        id: 0x81,
        name: "Gimard".into(),
        mp_cost: 4,
        element: SpellElement::Neutral,
        target: SpellTarget::OneEnemy,
        effect: SpellEffect::Damage {
            base_power: 100,
            element: SpellElement::Neutral,
        },
        anim_id: 0,
    };
    let before = world.actors[1].battle.hp;
    world.cast_spell_on_slots(0, &spell, &[1]);
    assert_eq!(
        before - world.actors[1].battle.hp,
        expected,
        "cast_spell_on_slots folds the faithful magnitude"
    );
}

/// A monster with no castable spells always picks a physical strike: the
/// action picker rolls `rand % (1 + 0) == 0`, so the magic branch is never
/// taken regardless of the seed. It still targets a (single living) party
/// member and arms the SM at `Begin`.
#[test]
fn spell_less_monster_always_arms_physical_strike() {
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    // Goblin (id 1) has no magic_attacks; leave the catalog empty so the
    // monster id doesn't resolve either - the magic branch can't be taken.
    world.actors[1].battle.max_hp = 30;
    world.actors[1].battle.hp = 30;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(1);

    world.take_monster_turn(1);

    assert_eq!(world.battle_ctx.queued_action, 3, "physical strike queued");
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::Begin.as_byte(),
        "SM armed at Begin to run the strike"
    );
    assert_eq!(
        world.actors[1].battle.action_category, 3,
        "physical action category"
    );
    assert_eq!(
        world.actors[1].battle.active_target, 0,
        "targets the only living party member (slot 0)"
    );
}

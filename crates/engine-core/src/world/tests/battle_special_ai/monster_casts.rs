//! Monster action picks: castable spells under a fixed RNG, lowest-HP
//! targeting, move-power damage, the elemental guard, the physical fallback and
//! fallen-target exclusion.

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// Monster-AI cast path: a monster whose record carries a castable spell
/// it can afford folds a real spell onto the party (HP drops, MP spent, a
/// damage popup queues) and parks the SM at `EndOfAction` so the loop
/// cycles - rather than the generic physical strike. RNG is pinned so the
/// cast-vs-strike roll lands on "cast".
#[test]
fn monster_ai_casts_a_castable_spell_under_fixed_rng() {
    use legaia_engine_battle::monster_catalog::vanilla_monster_catalog;
    use legaia_engine_battle::spells::SpellCatalog;
    use legaia_engine_battle_vm::battle_action::ActionState;

    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.tables.monster_catalog = vanilla_monster_catalog();
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
    // The action picker's first `rand % (1 + magic_count)` (magic count 2 ->
    // `% 3`) lands on 1, so it casts magic[0] = Flame (0x20).
    world.rng_state = BANDIT_BOSS_FLAME_SEED;

    let party_hp_before = world.actors[0].battle.hp;
    world.take_monster_turn(1);

    assert_eq!(
        world.actors[1].battle.params[0], 0x20,
        "picker chose Flame (magic_attacks[0])"
    );
    // The pick arms the action SM's Magic band; the band charges the MP at
    // 0x28 and the outcome folds the frame it leaves 0x29.
    assert_eq!(world.battle_ctx.action_state, ActionState::Begin.as_byte());
    assert_eq!(
        world.actors[0].battle.hp, party_hp_before,
        "nothing lands at the pick"
    );
    tick_until_cast_folds(&mut world);
    assert!(
        world.actors[0].battle.hp < party_hp_before,
        "the monster's spell dealt damage to the party"
    );
    assert!(world.actors[1].battle.mp < 10, "the monster spent MP");
    assert!(
        world.battle_ctx.action_state >= ActionState::MagicAnimChain.as_byte(),
        "a cast is the whole turn; the band runs it out"
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
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 3,
                ..Default::default()
            },
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
            MonsterAction::Flee => panic!("flee checkpoint fired in a targeting test"),
        }
    }

    let mut saw_redirect = false;
    for seed in 0u32..32 {
        let mut faithful = world3();
        faithful.rng_state = seed;
        let ft = target_of(faithful.pick_monster_action(3));
        let frng = faithful.rng_state;

        let mut smart = world3();
        smart.toggles.smarter_monster_targeting = true;
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
    use legaia_engine_battle::monster_catalog::vanilla_monster_catalog;
    use legaia_engine_battle::move_power::MovePowerCatalog;
    use legaia_engine_battle::spells::SpellCatalog;
    use legaia_game_tables::move_power::{
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
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.tables.monster_catalog = vanilla_monster_catalog();
        // Party target at slot 0 with a healthy HP pool + seeded AGL/DEF so the
        // kernel reads live defender stats.
        world.actors[0].battle.max_hp = 4000;
        world.actors[0].battle.hp = 4000;
        world.actors[0].battle.liveness = 1;
        world.battle.accuracy[0] = 30;
        world.battle.defense[0] = 40;
        // Bandit Boss (id 5) at slot 1: casts magic[0] = Flame (0x20) on
        // `BANDIT_BOSS_FLAME_SEED`.
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle.accuracy[1] = 25;
        world.set_battle_magic(1, 40);
        if install_table {
            world.tables.move_power =
                MovePowerCatalog::from_overlay_0898(&overlay_with_flame_power());
            assert!(
                world.tables.move_power.is_some(),
                "synthetic table installs"
            );
        }
        world.rng_state = BANDIT_BOSS_FLAME_SEED;

        let before = world.actors[0].battle.hp;
        world.take_monster_turn(1);
        assert_eq!(world.actors[1].battle.params[0], 0x20, "picker chose Flame");
        tick_until_cast_folds(&mut world);
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
    use legaia_engine_battle::accessory_passives::AccessoryPassives;
    use legaia_engine_battle::monster_catalog::vanilla_monster_catalog;
    use legaia_engine_battle::move_power::MovePowerCatalog;
    use legaia_engine_battle::spells::SpellCatalog;
    use legaia_game_tables::move_power::{
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
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.tables.monster_catalog = vanilla_monster_catalog();
        // Pin the attacking monster's element to 2 (Fire) so the resist
        // ladder has a real element to test against.
        world
            .tables
            .monster_catalog
            .by_id
            .get_mut(&5)
            .unwrap()
            .element = 2;
        world.actors[0].battle.max_hp = 4000;
        world.actors[0].battle.hp = 4000;
        world.actors[0].battle.liveness = 1;
        world.battle.accuracy[0] = 30;
        world.battle.defense[0] = 40;
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle.accuracy[1] = 25;
        world.set_battle_magic(1, 40);
        world.tables.move_power = MovePowerCatalog::from_overlay_0898(&overlay_with_flame_power());

        let mut party = legaia_save::Party::zeroed(1);
        if let Some(idx) = guard_passive {
            world.set_accessory_passives(AccessoryPassives::from_entries([(0x50, idx)], []));
            let mut eq = party.members[0].equipment();
            eq.slots[7] = 0x50;
            party.members[0].set_equipment(eq);
        }
        world.party.roster = party;
        world.refresh_party_ability_bits();
        world.rng_state = BANDIT_BOSS_FLAME_SEED;

        let before = world.actors[0].battle.hp;
        world.take_monster_turn(1);
        tick_until_cast_folds(&mut world);
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

/// A monster with no castable spells always picks a physical strike: the
/// action picker rolls `rand % (1 + 0) == 0`, so the magic branch is never
/// taken regardless of the seed. It still targets a (single living) party
/// member and arms the SM at `Begin`.
#[test]
fn spell_less_monster_always_arms_physical_strike() {
    use legaia_engine_battle_vm::battle_action::ActionState;

    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
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

/// The turn picker's monster arm runs the dead-target redirect after the AI
/// pick (`FUN_801DABA4` `0x801DAF48..0x801DAF50` -> `FUN_801DB124`): whatever
/// the picker chose, a physical strike never leaves aimed at a party member
/// at zero HP while another stands. Property over many RNG seeds, with the
/// fallen member's liveness halfword left stale so the picker itself can
/// still choose it.
#[test]
fn a_monster_strike_never_targets_a_fallen_party_member() {
    let mut chose_fallen_before_redirect = false;
    for seed in 0..200u32 {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 2,
                ..Default::default()
            },
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        // Slot 0 has fallen (HP 0) but its liveness halfword is still up.
        world.actors[0].battle.max_hp = 200;
        world.actors[0].battle.hp = 0;
        world.actors[0].battle.liveness = 1;
        world.actors[1].battle.max_hp = 200;
        world.actors[1].battle.hp = 200;
        world.actors[1].battle.liveness = 1;
        world.actors[2].battle.max_hp = 120;
        world.actors[2].battle.hp = 120;
        world.actors[2].battle.liveness = 1;
        world.rng_state = seed.wrapping_mul(0x9E37_79B9) | 1;
        let probe = world.random_living_party_member(2);
        chose_fallen_before_redirect |= probe == Some(0);
        world.take_monster_turn(2);
        assert_eq!(
            world.actors[2].battle.active_target, 1,
            "seed {seed}: the strike re-rolled onto the standing member"
        );
    }
    assert!(
        chose_fallen_before_redirect,
        "non-vacuous: the picker alone does choose the fallen member"
    );
}

/// The same redirect covers a monster's **cast**: the turn picker's
/// `jal 0x801DB124` at `0x801DAF50` has no category gate in front of it, and
/// the kernel's Magic arm re-rolls a fallen single target whenever the
/// spell's class byte is at or above `0x0A`. So a single-target spell never
/// leaves aimed at a party member at zero HP while another stands. Property
/// over many seeds with the fallen member's liveness halfword left stale, so
/// the picker itself can still name it.
#[test]
fn a_monster_cast_never_targets_a_fallen_party_member() {
    use legaia_engine_battle::monster_catalog::vanilla_monster_catalog;
    use legaia_engine_battle::spells::SpellCatalog;
    let mut casts = 0;
    let mut chose_fallen_before_redirect = false;
    for seed in 0..400u32 {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 2,
                ..Default::default()
            },
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.tables.monster_catalog = vanilla_monster_catalog();
        world.actors[0].battle.max_hp = 200;
        world.actors[0].battle.hp = 0;
        world.actors[0].battle.liveness = 1;
        world.actors[1].battle.max_hp = 200;
        world.actors[1].battle.hp = 200;
        world.actors[1].battle.liveness = 1;
        // Bandit Boss (id 5): two single-target spells, MP for either.
        world.actors[2].battle.max_hp = 120;
        world.actors[2].battle.hp = 120;
        world.actors[2].battle.mp = 10;
        world.actors[2].battle.liveness = 1;
        world.actors[2].battle_monster_id = Some(5);
        world.set_battle_magic(2, 40);
        world.rng_state = seed.wrapping_mul(0x9E37_79B9) | 1;
        world.take_monster_turn(2);
        let Some(cast) = world.casting.pending_cast.as_ref() else {
            continue;
        };
        casts += 1;
        // The anti-repeat ring records the pick before the redirect runs.
        chose_fallen_before_redirect |= world.battle.monster_ai_state.recent_targets[0] == 0;
        assert_eq!(
            cast.targets,
            vec![1],
            "seed {seed}: the cast re-rolled onto the standing member"
        );
    }
    assert!(casts > 0, "non-vacuous: some seeds cast");
    assert!(
        chose_fallen_before_redirect,
        "non-vacuous: the picker alone does choose the fallen member"
    );
}

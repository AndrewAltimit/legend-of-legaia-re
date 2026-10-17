//! Element affinity on monster specials and player summons, and the summon
//! kernel composition.

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// With both the move-power table AND the element-affinity tables installed, a
/// monster special attack scales by `matrix[enemy_element][party_member_element]`
/// (`FUN_801dd864`). Proven by running the same seeded cast through a neutral
/// (100%) matrix vs a weakness (200%) matrix and asserting the weakness matrix
/// deals more damage. A `None` affinity table reproduces the neutral result
/// exactly - so the affinity is gated and never perturbs the RNG stream.
#[test]
fn element_affinity_scales_monster_special_attack_damage() {
    use legaia_engine_battle::monster_catalog::vanilla_monster_catalog;
    use legaia_engine_battle::move_power::MovePowerCatalog;
    use legaia_engine_battle::spells::SpellCatalog;
    use legaia_game_tables::element_affinity::ElementAffinity;
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
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.tables.monster_catalog = vanilla_monster_catalog();
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
        // vanilla monster 5 has the default element 7 (neutral); the party
        // member at slot 0 is char id 1 -> element 3 in the synthetic table.
        let enemy_elem = world.tables.monster_catalog.get(5).unwrap().element as usize;
        if let Some(pct) = affinity_pct {
            let mut matrix = [[100u8; 8]; 8];
            matrix[enemy_elem][3] = pct;
            world.tables.element_affinity = Some(ElementAffinity {
                matrix,
                character_elements: vec![3; 8],
                summon_power: [[100; 8]; 3],
            });
        }
        world.rng_state = BANDIT_BOSS_FLAME_SEED;

        let before = world.actors[0].battle.hp;
        world.take_monster_turn(1);
        assert_eq!(world.actors[1].battle.params[0], 0x20, "picker chose Flame");
        tick_until_cast_folds(&mut world);
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
    use legaia_engine_battle::monster_catalog::{MonsterCatalog, MonsterDef};
    use legaia_engine_battle::spells::{SpellDef, SpellEffect, SpellElement, SpellTarget};
    use legaia_game_tables::element_affinity::ElementAffinity;

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
            ..Default::default()
        }
    }

    fn run(affinity_pct: Option<u8>) -> u16 {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
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
        world.tables.monster_catalog = catalog;

        // Caster = party slot 0; enough MP to afford the cast.
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.mp = 40;
        world.actors[0].battle.liveness = 1;
        world.set_battle_magic(0, 40);
        world.battle.accuracy[0] = 200;
        // Target = enemy slot 1, identified to the catalog by monster id.
        world.actors[1].battle.max_hp = 4000;
        world.actors[1].battle.hp = 4000;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle.defense[1] = 0;

        if let Some(pct) = affinity_pct {
            let mut matrix = [[100u8; 8]; 8];
            matrix[SUMMON_ELEM][ENEMY_ELEM] = pct;
            world.tables.element_affinity = Some(ElementAffinity {
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
    use legaia_engine_battle::monster_catalog::{MonsterCatalog, MonsterDef};
    use legaia_engine_battle::spells::{SpellDef, SpellEffect, SpellElement, SpellTarget};
    use legaia_engine_battle_vm::battle_formulas::{
        DamageFinish, DefenderResist, SummonRollActor, damage_finish_lazy, summon_predamage_lazy,
    };

    const SEED: u32 = 0xC0FFEE;

    fn build_world() -> World {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
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
        world.tables.monster_catalog = catalog;
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.mp = 40;
        world.actors[0].battle.liveness = 1;
        world.set_battle_magic(0, 40);
        world.battle.accuracy[0] = 25;
        world.actors[1].battle.max_hp = 4000;
        world.actors[1].battle.hp = 4000;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle.accuracy[1] = 12;
        world.battle.defense[1] = 30;
        world.rng_state = SEED;
        world
    }

    // Expected value: the kernels composed directly, drawing from the same
    // LCG in the same order (attacker, defender, lazy bonus, lazy floor),
    // each draw shaped the way BIOS `rand()` returns it (the state's high
    // half).
    struct Lcg(u32);
    impl Lcg {
        fn draw(&mut self) -> u16 {
            self.0 = legaia_engine_system::rand::world_lcg_step(self.0);
            ((self.0 >> 16) & 0x7fff) as u16
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
        ..Default::default()
    };
    let before = world.actors[1].battle.hp;
    world.cast_spell_on_slots(0, &spell, &[1]);
    assert_eq!(
        before - world.actors[1].battle.hp,
        expected,
        "cast_spell_on_slots folds the faithful magnitude"
    );
}

/// The summon body resolves from the archive-backed
/// `DiscTables::summon_creatures` when the scene catalog does not field the
/// creature - which is almost every fight: the catalog holds only the scene's
/// own monsters. Without it the cast fell back to the MP-scaled placeholder,
/// which a boss's defence floors at 1 (the Dohati fight's Viguro casts).
#[test]
fn a_summon_rolls_from_its_archive_body_when_the_scene_lacks_the_creature() {
    use legaia_engine_battle::monster_catalog::{MonsterCatalog, MonsterDef};

    fn build_world(archive_body: bool) -> World {
        build_world_for(archive_body, 0x81)
    }

    fn build_world_for(archive_body: bool, spell_id: u8) -> World {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        // The scene's catalog: its own enemy, and no Gimard.
        let mut catalog = MonsterCatalog::new();
        catalog.insert(MonsterDef::new(5, "Goblin", 120, 8));
        world.tables.monster_catalog = catalog;
        if archive_body {
            let mut creature = MonsterDef::new(10, "Gimard", 100, 10);
            creature.intel = 36;
            creature.element = 2;
            world.tables.summon_creatures.insert(spell_id, creature);
        }
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.liveness = 1;
        world.battle.accuracy[0] = 25;
        world.actors[1].battle.max_hp = 4000;
        world.actors[1].battle.hp = 4000;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle.accuracy[1] = 12;
        world.battle.defense[1] = 30;
        world
    }

    assert_eq!(
        build_world(false).player_summon_predamage(0, 1, 0x81),
        None,
        "no body anywhere: the placeholder path"
    );
    let rolled = build_world(true).player_summon_predamage(0, 1, 0x81);
    assert!(
        rolled.is_some_and(|d| d > 1),
        "the archive body rolls the summon branch: {rolled:?}"
    );
    // The evolved block (`0x8C..=0x95`) summons a `battle_data` body too:
    // Gilium `0x95` / Aluru `0x8E` rolled the placeholder, floored at 1.
    for evolved in [0x8E, 0x95] {
        let rolled = build_world_for(true, evolved).player_summon_predamage(0, 1, evolved);
        assert!(
            rolled.is_some_and(|d| d > 1),
            "evolved {evolved:#x} rolls the summon branch: {rolled:?}"
        );
    }
}

/// A big summon (`0x9A..=0xA0`) rolls the same summon branch from its own
/// `summon.dat` record: every image `0927..=0934` calls `FUN_801DD0AC(_, 7,
/// seat)` (Terra's at `0x801F7CCC`), and slot 7 is the streamed record the
/// seat arm copies in (`FUN_80054CB0(record, 4)`). The body resolved only
/// for the Seru blocks, so a Terra cast took the MP-scaled placeholder,
/// which a Bio Castle Freed's defence floored at 1 per target. The roll here
/// is the kernel composition over the record's stats.
#[test]
fn a_big_summon_rolls_the_summon_branch_from_its_cast_record() {
    use legaia_actor_formats::summon_readef::CastRecordStats;
    use legaia_engine_battle::monster_catalog::{MonsterDef, monster_def_from_cast_record};

    // Terra's record head as it sits on the disc's shape: HP, MP, then
    // `[AGL, ATK, UDF, LDF, INT, SPD]`, element 3 (wind).
    let head = CastRecordStats {
        hp: 2176,
        mp: 0,
        stats: [999, 999, 999, 999, 272, 999],
        element: 3,
        size_class: 0,
    };
    let build = |body: bool| {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        let mut catalog = legaia_engine_battle::monster_catalog::MonsterCatalog::new();
        catalog.insert(MonsterDef::new(5, "Freed", 3518, 8));
        world.tables.monster_catalog = catalog;
        if body {
            world
                .tables
                .summon_creatures
                .insert(0x9F, monster_def_from_cast_record("Queen Twister", &head));
        }
        world.actors[0].battle.max_hp = 1500;
        world.actors[0].battle.hp = 1500;
        world.actors[0].battle.liveness = 1;
        world.battle.accuracy[0] = 90;
        world.actors[1].battle.max_hp = 3518;
        world.actors[1].battle.hp = 3518;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle.accuracy[1] = 78;
        world.battle.defense[1] = 300;
        world
    };

    assert_eq!(
        build(false).player_summon_predamage(0, 1, 0x9F),
        None,
        "no body: the placeholder path"
    );

    // The body carries the record's stats under the fight's boost profile:
    // INT x5/4 in a random encounter, x9/8 in a scripted one; HP and element
    // straight.
    let mut world = build(true);
    let def = world.tables.summon_creatures.get(&0x9F).unwrap();
    assert_eq!((def.hp, def.element), (2176, 3));
    assert_eq!(def.installed_stats(false)[4], 272 + 68);
    assert_eq!(def.installed_stats(true)[4], 272 + 34);
    // The roll's attacker term is at least the body's HP plus the caster's
    // doubled AGL, against a defender roll this target keeps under 400, so
    // the hit is in the thousands - never the placeholder's floor.
    let rolled = world.player_summon_predamage(0, 1, 0x9F);
    assert!(
        rolled.is_some_and(|d| d > 1500),
        "a big summon rolls its body: {rolled:?}"
    );
}

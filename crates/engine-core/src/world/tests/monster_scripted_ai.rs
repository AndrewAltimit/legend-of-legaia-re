use super::*;
use legaia_engine_mode::mode::SceneMode;

/// Per-monster-id scripted AI (the `FUN_801E9FD4` switch) end-to-end: a
/// wounded monster whose id has a low-HP self-heal case folds that heal onto
/// itself rather than striking the party. Monster id 6 (case `0x06`) casts
/// `0x52` at self when `HP <= maxHP/2` and its ability cooldown is clear.
#[test]
fn scripted_ai_monster_self_heals_when_wounded() {
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
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    // Monster id 6 (Skeleton -> AI case 0x06) at slot 1, wounded to 20/100
    // with MP to spare for the heal.
    world.actors[1].battle.max_hp = 100;
    world.actors[1].battle.hp = 20;
    world.actors[1].battle.mp = 20;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(6);
    world.rng_state = 7;

    world.take_monster_turn(1);

    assert_eq!(
        world.actors[1].battle.params[0], 0x52,
        "self-heal spell queued"
    );
    // The pick arms the Magic band; the heal folds at its 0x29 exit.
    tick_until_cast_folds(&mut world);
    assert!(
        world.actors[1].battle.hp > 20,
        "the monster healed itself instead of striking the party"
    );
    assert_eq!(world.actors[0].battle.hp, 200, "party untouched");
    assert!(
        world.battle_ctx.action_state >= ActionState::MagicAnimChain.as_byte(),
        "cast is the whole turn; the band runs it out"
    );
    assert_eq!(
        world.battle.monster_ai_state.dat[4], 1,
        "ability cooldown armed"
    );
    let fx = world.drain_battle_hit_fx();
    assert!(fx.iter().any(|f| f.is_heal && f.target_slot == 1));
}

/// Monster `0x8A` reads its own spirit-art gauge (`actor+0x170`) as a charge
/// gate: once it passes `0x31` the AI fires the `0x4E` all-enemies cast and the
/// live loop clamps the caster's gauge to `0x32`. Below the threshold the
/// generic core stands (this monster has no castable magic in the catalog, so
/// that means a physical strike) and the gauge is left untouched.
#[test]
fn monster_8a_charge_gate_drives_cast_and_clamps_gauge() {
    use legaia_engine_battle::monster_catalog::vanilla_monster_catalog;
    use legaia_engine_battle::spells::SpellCatalog;

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
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.mp = 200;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(0x8a);

    // Charged: the gate fires the 0x4E cast and clamps the gauge to 0x32.
    world.actors[1].battle.spirit_gauge = 100;
    world.rng_state = 7;
    match world.pick_monster_action(1) {
        MonsterAction::Cast { spell_id, .. } => assert_eq!(spell_id, 0x4e),
        other => panic!("expected the 0x4E charge cast, got {other:?}"),
    }
    assert_eq!(
        world.actors[1].battle.spirit_gauge, 0x32,
        "the gauge is clamped to 0x32 as the cast commits"
    );

    // Below threshold: no override, gauge left exactly as-is.
    world.actors[1].battle.spirit_gauge = 0x31;
    world.rng_state = 7;
    assert!(matches!(
        world.pick_monster_action(1),
        MonsterAction::Physical { .. }
    ));
    assert_eq!(
        world.actors[1].battle.spirit_gauge, 0x31,
        "an uncharged 0x8A leaves its gauge untouched"
    );
}

/// Faithful `FUN_801E7320`: a targeting class in `3..=6` resolves to a
/// living PARTY slot; a class in `0..=2` resolves to a living MONSTER slot.
/// (Dead slots are skipped via the re-roll loop.)
#[test]
fn monster_target_resolver_expands_class_to_correct_side() {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 3,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    // Party slots 0..2: slot 0 dead, slots 1+2 alive.
    for i in 0..3u8 {
        let a = &mut world.actors[i as usize];
        a.battle.max_hp = 100;
        a.battle.hp = if i == 0 { 0 } else { 100 };
        a.battle.liveness = if i == 0 { 0 } else { 1 };
    }
    // Monster slots 3+4 alive.
    for i in 3..5u8 {
        let a = &mut world.actors[i as usize];
        a.battle.max_hp = 80;
        a.battle.hp = 80;
        a.battle.liveness = 1;
    }

    // Caster = monster slot 3, class 3 (party-targeting). Resolves to a
    // LIVING party slot (1 or 2, never the dead slot 0).
    world.actors[3].battle.active_target = 3; // class 3..6 -> party
    world.rng_state = 12345;
    world.resolve_monster_target(3);
    let t = world.actors[3].battle.active_target;
    assert!(
        (1..=2).contains(&t),
        "class 3 -> living party slot, got {t}"
    );

    // Class 1 (monster-band targeting). Resolves to a living monster slot.
    world.actors[3].battle.active_target = 1; // class 0..2 -> monster band
    world.rng_state = 999;
    world.resolve_monster_target(3);
    let t = world.actors[3].battle.active_target;
    assert!(
        (3..=4).contains(&t),
        "class 1 -> living monster slot, got {t}"
    );
}

/// `advance_battle_mode` (the SM `case 0xFF` writer for `ctx+0x28A`) flips a
/// multi-phase boss from its first-phase cast to its phased cast on the next
/// turn. Monster id `0xB6` always casts, picking its spell purely by mode.
#[test]
fn advancing_the_battle_mode_drives_a_boss_to_its_next_phase() {
    use legaia_engine_battle::monster_catalog::MonsterDef;
    use legaia_engine_battle::spells::SpellCatalog;

    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    // A synthetic boss at monster slot 1 with id 0xB6 (no own magic - it
    // casts purely off its scripted phase table).
    world
        .tables
        .monster_catalog
        .insert(MonsterDef::new(0xb6, "Boss", 400, 50));
    world.actors[0].battle.max_hp = 300;
    world.actors[0].battle.hp = 300;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 400;
    world.actors[1].battle.hp = 400;
    world.actors[1].battle.mp = 250;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(0xb6);
    world.rng_state = 1;
    // Spend the once-per-pass flee checkpoint: this test drives the scripted
    // phase table, not the flee roll.
    world.battle.monster_flee_attempted = true;

    assert_eq!(world.battle_mode(), 0, "fresh battle starts in phase 0");
    world.take_monster_turn(1);
    assert_eq!(world.actors[1].battle.params[0], 0xa2, "phase 0 cast");

    // A scripted phase transition advances the mode; next turn is phase I.
    world.advance_battle_mode();
    assert_eq!(world.battle_mode(), 1);
    world.take_monster_turn(1);
    assert_eq!(world.actors[1].battle.params[0], 0xa3, "phase 1 cast");
}

/// The picker's once-per-pass flee checkpoint (`FUN_801E9FD4` loop bottom ->
/// `FUN_801EC0DC`): a wounded, outmatched monster can roll a flee, the roll
/// happens at most once per round, and a success arms the Run band
/// (category 5). Stats are shaped so the score compare can pass (strong
/// healthy party, weak wounded monster); the 1-in-8 gate means only some
/// seeds flee, so the test scans seeds for one and then re-runs that seed
/// with the latch already spent to prove the once-per-pass gate.
#[test]
fn monster_flee_checkpoint_rolls_once_and_arms_run_band() {
    let build = || {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.actors[0].battle.max_hp = 2000;
        world.actors[0].battle.hp = 2000;
        world.actors[0].battle.liveness = 1;
        world.battle.attack[0] = 800;
        world.actors[1].battle.max_hp = 100;
        world.actors[1].battle.hp = 1;
        world.actors[1].battle.liveness = 1;
        world.battle.attack[1] = 0;
        world
    };

    let mut flee_seed = None;
    for seed in 0..20_000u32 {
        let mut world = build();
        world.rng_state = seed;
        if matches!(world.pick_monster_action(1), MonsterAction::Flee) {
            flee_seed = Some(seed);
            break;
        }
    }
    let seed = flee_seed.expect("some seed in 0..20000 rolls a monster flee");

    // The fleeing pick armed category 5 (the Run band's monster arm).
    let mut world = build();
    world.rng_state = seed;
    assert!(matches!(world.pick_monster_action(1), MonsterAction::Flee));
    assert_eq!(world.actors[1].battle.action_category, 5);
    assert!(
        world.battle.monster_flee_attempted,
        "the pass latch is spent"
    );

    // Same seed, latch already spent: the checkpoint is skipped entirely.
    let mut world = build();
    world.rng_state = seed;
    world.battle.monster_flee_attempted = true;
    assert!(
        !matches!(world.pick_monster_action(1), MonsterAction::Flee),
        "a spent latch skips the flee roll until the round boundary re-arms it"
    );

    // The round boundary re-arms the latch (retail re-enters the picker with
    // its balance counter cleared each round).
    let mut world = build();
    world.battle.monster_flee_attempted = true;
    crate::battle_round::BattleRound::boundary(&mut world);
    assert!(!world.battle.monster_flee_attempted);
}

/// The flee roll's monster side is averaged over the **seated** monster count
/// `ctx[+1]` (`div s1,v0` at `0x801EC280`), not over the actor table: a lone
/// strong monster against a weak party never flees, however wounded, because
/// the preallocated empty slots above the battle layout do not dilute its
/// score. This is the Rim Elm sparring partner's shape (999 max HP, a
/// level-1 party) without the disc.
#[test]
fn a_lone_strong_monster_is_not_diluted_by_the_empty_pool() {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 80;
    world.actors[0].battle.hp = 80;
    world.actors[0].battle.liveness = 1;
    world.battle.attack[0] = 20;
    world.actors[1].battle.max_hp = 999;
    world.actors[1].battle.liveness = 1;
    world.battle.attack[1] = 30;
    assert_eq!(world.seated_monster_slots().collect::<Vec<_>>(), vec![1]);
    for hp in [999u16, 100, 1] {
        for seed in 0..2_000u32 {
            world.actors[1].battle.hp = hp;
            world.battle.monster_flee_attempted = false;
            world.rng_state = seed;
            assert!(
                !matches!(world.pick_monster_action(1), MonsterAction::Flee),
                "seed {seed} at {hp} HP fled"
            );
        }
    }
}

/// The scripted no-escape flag (`ctx+0x287`) blocks the monster flee roll
/// unconditionally - `FUN_801EC0DC` opens on the same gate the party roll's
/// failure arm tests.
#[test]
fn no_escape_flag_blocks_monster_flee() {
    for seed in 0..2_000u32 {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.actors[0].battle.max_hp = 2000;
        world.actors[0].battle.hp = 2000;
        world.actors[0].battle.liveness = 1;
        world.battle.attack[0] = 800;
        world.actors[1].battle.max_hp = 100;
        world.actors[1].battle.hp = 1;
        world.actors[1].battle.liveness = 1;
        world.battle.no_escape = true;
        world.rng_state = seed;
        assert!(
            !matches!(world.pick_monster_action(1), MonsterAction::Flee),
            "no-escape battle must never roll a monster flee (seed {seed})"
        );
    }
}

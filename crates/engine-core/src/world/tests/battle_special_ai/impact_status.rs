//! Statuses an enemy special lands on impact: rot and its guards, the damage-
//! over-time roll and the toxic tick.

use super::*;
use legaia_engine_mode::mode::SceneMode;

// ---------------------------------------------------------------------------
// Enemy special-attack impact status proc (monster -> party).
//
// Retail chain, read off the instruction stream (see
// `World::apply_enemy_move_status`): `FUN_801DEA50` stashes the acting actor's
// move-power record at `ctx+0x1014`, and `FUN_801E09F8` reads that record's
// `+0x0A` impact-effect selector at the impact phase and branches four ways.
// ---------------------------------------------------------------------------

/// Build a synthetic PROT-0898-shaped overlay whose Flame (`0x20`) move-power
/// record carries `impact` in its `+0x0A` impact-effect selector.
fn overlay_flame_with_impact(impact: u8) -> Vec<u8> {
    use legaia_game_tables::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };
    let mut buf =
        vec![0u8; MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN];
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1; // structural guard (id 4 -> idx 1)
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x20] = 1; // Flame -> power record 1
    let rec = MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE;
    buf[rec] = 0xB8; // +0x00 power 0x0BB8
    buf[rec + 1] = 0x0B;
    buf[rec + 0x0a] = impact; // +0x0a impact-effect selector
    buf
}

/// A 1-vs-1 battle where Bandit Boss (monster id 5) at slot 1 casts its
/// `magic_attacks[0]` = Flame (`0x20`) at the lone party member on `BANDIT_BOSS_FLAME_SEED`.
fn flame_caster_battle(impact: u8) -> World {
    use legaia_engine_battle::monster_catalog::vanilla_monster_catalog;
    use legaia_engine_battle::move_power::MovePowerCatalog;
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
    world.tables.move_power =
        MovePowerCatalog::from_overlay_0898(&overlay_flame_with_impact(impact));
    assert!(
        world.tables.move_power.is_some(),
        "synthetic table installs"
    );
    world.rng_state = BANDIT_BOSS_FLAME_SEED;
    world
}

/// Stage one already-resolved damaging impact on `target` so the applier has an
/// impact list without re-running the cast.
fn stage_impact(world: &mut World, target: u8) {
    world.battle.hit_fx.push(BattleHitFx {
        target_slot: target,
        amount: 100,
        is_heal: false,
        is_crit: false,
    });
}

/// **The monster -> party status wire.** A monster special whose move-power
/// record carries impact selector `5` (the random limb-disable arm, which has
/// no probability gate of its own) puts Rot on the party member it hits, with
/// a rolled limb - and a record carrying selector `0` puts nothing on, which
/// is what makes the assertion non-vacuous rather than "the tracker is always
/// populated".
///
/// Disable the `apply_enemy_move_status` call in `World::take_monster_turn`
/// and the `selector 5` half of this test fails at the `has(0, Rot)` assert.
#[test]
fn enemy_special_impact_selector_5_rots_the_party_target() {
    use legaia_engine_battle_vm::status_effects::StatusKind;

    // Baseline: selector 0 = "no impact effect" -> no status anywhere.
    let mut none = flame_caster_battle(0);
    none.take_monster_turn(1);
    tick_until_cast_folds(&mut none);
    assert_eq!(
        none.actors[1].battle.params[0], 0x20,
        "picker chose Flame (magic_attacks[0])"
    );
    assert!(
        none.actors[0].battle.hp < 4000,
        "the special connected (a status proc needs an impact)"
    );
    assert!(
        !none.battle.status_effects.is_afflicted(0),
        "selector 0 applies no status - the wire is selector-driven, not unconditional"
    );

    // Selector 5: the Rot arm. No 1-in-N gate, so it lands every time.
    let mut rot = flame_caster_battle(5);
    rot.take_monster_turn(1);
    tick_until_cast_folds(&mut rot);
    assert_eq!(rot.actors[1].battle.params[0], 0x20, "picker chose Flame");
    assert!(
        rot.battle.status_effects.has(0, StatusKind::Rot),
        "the enemy special's +0x0A selector 5 rotted the party member"
    );
    assert!(
        rot.battle.status_effects.rot_limb(0).is_some(),
        "the applier rolled which limb the Rot disables (rand % 3)"
    );
    assert_eq!(
        rot.actors[0].pending_status,
        Some(legaia_art::record::EnemyEffect::from_byte(5)),
        "the lingering-status visual latch (`sb v1,0x21f`) mirrors the selector"
    );
}

/// The Rot arm reads the *character record's* accessory-passive bitfield and
/// bails on Rot Guard (index `0x18`) or Master Guard (index `0x1C`) before it
/// draws its limb roll. Both guards block; an unrelated bit does not.
#[test]
fn enemy_special_rot_is_blocked_by_rot_guard_and_master_guard() {
    use legaia_engine_battle_vm::status_effects::StatusKind;

    for bit in [1u32 << 0x18, 1u32 << 0x1C] {
        let mut world = flame_caster_battle(5);
        world.party.character_ability_bits[0] = bit;
        world.take_monster_turn(1);
        tick_until_cast_folds(&mut world);
        assert!(
            !world.battle.status_effects.has(0, StatusKind::Rot),
            "ability bit {bit:#x} (Rot Guard / Master Guard) blocks the Rot arm"
        );
    }
    // An unrelated passive bit does not block it.
    let mut world = flame_caster_battle(5);
    world.party.character_ability_bits[0] = 1 << 0x10; // Steal Attack
    world.take_monster_turn(1);
    tick_until_cast_folds(&mut world);
    assert!(
        world.battle.status_effects.has(0, StatusKind::Rot),
        "an unrelated passive leaves the Rot arm alone"
    );
}

/// The DoT arms (selector `3` Venom / `4` Toxic) are a 1-in-8 roll, not a
/// certainty and not a never. Scanning the RNG seed space over the applier
/// alone pins both halves: some seeds land, most do not.
#[test]
fn enemy_special_dot_arms_are_a_one_in_eight_roll() {
    use legaia_engine_battle_vm::status_effects::StatusKind;

    const SEEDS: u32 = 64;
    for (selector, kind) in [(3u8, StatusKind::Venom), (4u8, StatusKind::Toxic)] {
        let mut landed = 0usize;
        for seed in 0..SEEDS {
            let mut world = flame_caster_battle(selector);
            world.rng_state = seed;
            stage_impact(&mut world, 0);
            world.apply_enemy_move_status(1, 0x20, 0);
            if world.battle.status_effects.has(0, kind) {
                landed += 1;
            }
        }
        assert!(landed > 0, "selector {selector} lands on some seeds");
        assert!(
            landed < SEEDS as usize,
            "selector {selector} is a roll, not an unconditional apply ({landed}/{SEEDS})"
        );
    }
}

/// A status the enemy applier lands is a real gameplay consequence, not just a
/// tracker row: the Toxic it installs drains HP at the round boundary through
/// the ported `FUN_801E752C` ticker.
#[test]
fn enemy_applied_toxic_ticks_at_the_round_boundary() {
    use legaia_engine_battle_vm::status_effects::StatusKind;

    // A seed on which the 1-in-8 gate lands (the scan above proves one exists).
    let seed = (0..64u32)
        .find(|&s| {
            let mut w = flame_caster_battle(4);
            w.rng_state = s;
            stage_impact(&mut w, 0);
            w.apply_enemy_move_status(1, 0x20, 0);
            w.battle.status_effects.has(0, StatusKind::Toxic)
        })
        .expect("some seed lands the 1-in-8 Toxic gate");

    let mut world = flame_caster_battle(4);
    world.rng_state = seed;
    stage_impact(&mut world, 0);
    world.apply_enemy_move_status(1, 0x20, 0);
    assert!(world.battle.status_effects.has(0, StatusKind::Toxic));

    let hp_before = world.actors[0].battle.hp;
    crate::battle_round::BattleRound::end(&mut world);
    assert!(
        world.actors[0].battle.hp < hp_before,
        "the enemy-applied Toxic drains HP at the round boundary"
    );
}

/// The move-power table is the **enemy special-attack** table, so a party-slot
/// caster is rejected by the applier's own seat guard - a party art's status
/// byte rides the `ApplyArtStrike` event instead.
#[test]
fn enemy_move_status_applier_rejects_a_party_caster() {
    let mut world = flame_caster_battle(5);
    stage_impact(&mut world, 1);
    world.apply_enemy_move_status(0, 0x20, 0);
    assert!(
        !world.battle.status_effects.is_afflicted(1),
        "a party-slot caster is rejected by the applier's own seat guard"
    );
}

//! Expansion-pack battles and progression on a disc-free world: a pack's
//! formation seats its derived monster, pays scaled spoils and hands the
//! catalog back; its encounter override rolls in place of retail's; and a
//! world with no pack (or a pack activated and dropped again) plays the
//! retail fight bit for bit.

use std::sync::Arc;

use super::*;
use legaia_engine_battle::encounter::{EncounterEntry, EncounterPhase, EncounterTable};
use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, MonsterCatalog, MonsterDef,
};
use legaia_engine_expansion::battle::EXPANSION_FORMATION_BASE;
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const PACK_TOML: &str = r#"
id = "testpack"
name = "Test Pack"
version = "0.1.0"
"#;

const BATTLE_TOML: &str = r#"
[monsters.dire_wolf]
base = { kind = "monster", id = 9 }
name = "Dire Wolf"
hp = { mul = 2.0 }
exp = 1000
gold = 400

[formations.den]
label = "Wolf Den"
monsters = ["dire_wolf"]

[[encounters]]
scene = "testscene"
formations = [{ formation = "den" }]
steps = [3, 3]

[[encounters]]
scene = "rollscene"
formations = [{ formation = "den" }]

[bosses.alpha]
formation = "den"
music = { kind = "bgm", id = 30 }
reward_items = [{ item = { kind = "item", id = 5 }, count = 2 }]
reward_gold = 1000
once = true
"#;

const PROGRESSION_TOML: &str = r#"
xp_mul = 2.0
gold_mul = 1.5
[xp_curve]
kind = "scaled"
mul = 0.5
[[level_rewards]]
level = 3
items = [{ item = { kind = "item", id = 7 } }]
"#;

fn pack(progression: bool) -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert("pack.toml".into(), PACK_TOML.as_bytes().to_vec());
    files.insert("battle.toml".into(), BATTLE_TOML.as_bytes().to_vec());
    if progression {
        files.insert(
            "progression.toml".into(),
            PROGRESSION_TOML.as_bytes().to_vec(),
        );
    }
    Arc::new(ExpansionPack::from_files(&files).expect("synthetic pack parses"))
}

/// One capable party member against a catalog holding the retail Wolf, with
/// retail's formation row `1` (one Wolf) registered.
fn world() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Field;
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.battle.set_battle_attack(0, 80);
    let mut cat = MonsterCatalog::new();
    let mut wolf = MonsterDef::new(9, "Wolf", 35, 12);
    wolf.exp = 20;
    wolf.gold = 40;
    cat.insert(wolf);
    world.set_monster_catalog(cat);
    world
        .tables
        .formation_table
        .insert(FormationDef::new(1, vec![FormationSlot::new(9)]));
    world.rng_state = 0x0BAD_5EED;
    world
}

/// Tick until the world enters battle, then until it leaves it.
fn fight(world: &mut World) {
    for _ in 0..400 {
        if world.mode == SceneMode::Battle {
            break;
        }
        world.tick();
    }
    assert_eq!(world.mode, SceneMode::Battle, "the fight starts");
    for _ in 0..8000 {
        world.tick();
        if world.mode != SceneMode::Battle {
            return;
        }
    }
    panic!("the fight never ended");
}

#[test]
fn a_pack_formation_seats_the_derived_monster_and_pays_scaled_spoils() {
    let mut w = world();
    w.activate_expansion(pack(true)).expect("pack activates");
    let id = w.expansion_formation_id("den").unwrap();
    assert_eq!(id, EXPANSION_FORMATION_BASE);
    assert!(w.start_expansion_battle("testpack:den"));
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    // The derived monster fights under the Wolf's id with its own numbers.
    assert_eq!(w.actors[1].battle_monster_id, Some(9));
    assert_eq!(w.actors[1].battle.max_hp, 70);
    assert_eq!(w.tables.monster_catalog.get(9).unwrap().name, "Dire Wolf");
    let money = w.party.money;
    fight(&mut w);
    // Spoils are only paid on a won fight.
    let rewards = w.battle.last_rewards.clone().expect("the party wins");
    // 1000 EXP x2 (the pack's xp_mul); one member takes 3/4 of it.
    assert_eq!(rewards.xp, 2000);
    assert_eq!(w.party.level_up_tracker.xp[0], 1500);
    // Gold: (400 >> 1) halved, x1.5.
    assert_eq!(rewards.gold, 150);
    assert_eq!(w.party.money, money + 150);
    // The halved curve levels the member past 3, which pays its reward.
    assert!(w.party.level_up_tracker.level[0] > 3);
    assert_eq!(w.party.inventory.get(&7).copied(), Some(1));
    // The scene's Wolf is back.
    let wolf = w.tables.monster_catalog.get(9).unwrap();
    assert_eq!((wolf.name.as_str(), wolf.hp), ("Wolf", 35));
    assert!(w.expansion.battle.shadowed.is_empty());
}

#[test]
fn a_boss_pays_its_reward_once() {
    let mut w = world();
    w.activate_expansion(pack(false)).unwrap();
    w.set_battle_bgm(Some(
        legaia_engine_system::music_labels::BATTLE_THEME_1_BGM_ID,
    ));
    assert!(w.start_expansion_boss("alpha"));
    fight(&mut w);
    assert!(w.battle.last_rewards.is_some(), "the party wins");
    assert_eq!(w.party.inventory.get(&5).copied(), Some(2));
    let rewards = w.battle.last_rewards.clone().unwrap();
    assert!(rewards.drops.contains(&5));
    // No pack multipliers: (400 >> 1) halved, plus the boss's 1000.
    assert_eq!(rewards.gold, 100 + 1000);
    assert!(w.expansion_boss_cleared("alpha"));
    assert!(w.expansion.battle.active_boss.is_none());
    // A once-only boss refuses a second start.
    for _ in 0..600 {
        w.tick();
    }
    assert!(!w.start_expansion_boss("testpack:alpha"));
}

#[test]
fn the_boss_track_replaces_the_battle_theme() {
    let mut w = world();
    w.activate_expansion(pack(false)).unwrap();
    w.set_battle_bgm(Some(
        legaia_engine_system::music_labels::BATTLE_THEME_1_BGM_ID,
    ));
    assert!(w.start_expansion_boss("alpha"));
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.audio.current_bgm, Some(2030));
}

#[test]
fn a_pack_owned_rate_rolls_its_own_formation() {
    let mut w = world();
    w.set_active_scene_label("testscene");
    w.activate_expansion(pack(false)).unwrap();
    // `steps = [3, 3]`: the third step fights.
    assert!(!w.on_field_step());
    assert!(!w.on_field_step());
    assert!(w.on_field_step());
    let phase = w.encounters.session.as_ref().unwrap().phase();
    assert!(
        matches!(phase, EncounterPhase::Transition { roll, .. } if roll.formation_id == EXPANSION_FORMATION_BASE),
        "{phase:?}"
    );
}

#[test]
fn a_retail_roll_is_swapped_for_the_pack_formation() {
    let roll = |with_pack: bool| {
        let mut w = world();
        w.set_active_scene_label("rollscene");
        let mut table = EncounterTable::new("rollscene");
        table.set_trigger_rate(255);
        table.push(EncounterEntry::new(1, 1));
        w.install_man_encounter(
            table,
            vec![FormationDef::new(1, vec![FormationSlot::new(9)])],
        );
        if with_pack {
            w.activate_expansion(pack(false)).unwrap();
        }
        for _ in 0..50 {
            if w.on_field_step() {
                break;
            }
        }
        match w.encounters.session.as_ref().unwrap().phase() {
            EncounterPhase::Transition { roll, .. } => roll.formation_id,
            p => panic!("no roll: {p:?}"),
        }
    };
    assert_eq!(roll(false), 1);
    assert_eq!(roll(true), EXPANSION_FORMATION_BASE);
}

/// What a retail fight leaves behind, for comparing worlds.
fn retail_fight(w: &mut World) -> (u32, u8, i32, u32, Vec<u32>) {
    assert!(w.force_encounter(1));
    fight(w);
    (
        w.party.level_up_tracker.xp[0],
        w.party.level_up_tracker.level[0],
        w.party.money,
        w.rng_state,
        w.party.level_up_tracker.xp_table.clone(),
    )
}

#[test]
fn no_pack_and_a_dropped_pack_play_the_retail_fight() {
    let mut plain = world();
    let retail = retail_fight(&mut plain);

    let mut dropped = world();
    dropped.activate_expansion(pack(true)).unwrap();
    assert_ne!(
        dropped.party.level_up_tracker.xp_table, plain.party.level_up_tracker.xp_table,
        "the pack installs its curve"
    );
    dropped.deactivate_expansion();
    assert!(dropped.expansion.pack.is_none());
    assert!(
        dropped
            .tables
            .formation_table
            .formation(EXPANSION_FORMATION_BASE)
            .is_none()
    );
    assert_eq!(retail_fight(&mut dropped), retail);

    // An active pack with no override for this scene and no scaling still
    // pays the retail fight's own numbers - only its multipliers differ.
    let mut active = world();
    active.activate_expansion(pack(false)).unwrap();
    let (xp, ..) = retail_fight(&mut active);
    assert_eq!(xp, retail.0);
}

#[test]
fn a_broken_pack_is_refused_and_leaves_no_pack() {
    let mut files = PackFiles::new();
    files.insert("pack.toml".into(), PACK_TOML.as_bytes().to_vec());
    files.insert(
        "battle.toml".into(),
        b"[formations.f]\nmonsters = [\"ghost\"]\n".to_vec(),
    );
    let p = Arc::new(ExpansionPack::from_files(&files).unwrap());
    let mut w = world();
    let err = w.activate_expansion(p).unwrap_err();
    assert!(
        err.iter().any(|l| l.contains("unknown monster `ghost`")),
        "{err:?}"
    );
    assert!(w.expansion.pack.is_none());
}

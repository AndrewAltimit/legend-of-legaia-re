//! Expansion quests over the pack's compiled battles: a quest's named
//! formation and boss resolve through the compiled table, a quest start
//! queues its boss fight through `start_expansion_boss`, activation brings
//! the quest runtime onto the pack, and deactivation keeps the progress
//! (once-only bosses included) for the next activation.

use std::sync::Arc;

use super::*;
use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, MonsterCatalog, MonsterDef,
};
use legaia_engine_expansion::quest::QuestStatus;
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::{ExpansionPack, PackFiles};

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"qb\"\nname = \"QB\"\nversion = \"0.1.0\"\n".to_vec(),
    );
    files.insert(
        "battle.toml".into(),
        br#"
[monsters.dire_wolf]
base = { kind = "monster", id = 9 }
hp = { mul = 2.0 }

[formations.den]
monsters = ["dire_wolf"]

[bosses.alpha]
formation = "den"
once = true
"#
        .to_vec(),
    );
    files.insert(
        "quests.toml".into(),
        br#"
[[quests]]
id = "hunt"
giver = "ranger"
start_boss = "alpha"
ordered = false
objectives = [
    { kind = "win_boss", boss = "alpha" },
    { kind = "defeat_formation", formation = "den" },
]
rewards = { set_flags = ["hunted"] }
"#
        .to_vec(),
    );
    Arc::new(ExpansionPack::from_files(&files).expect("synthetic pack parses"))
}

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
    cat.insert(MonsterDef::new(9, "Wolf", 35, 12));
    world.set_monster_catalog(cat);
    world
        .tables
        .formation_table
        .insert(FormationDef::new(1, vec![FormationSlot::new(9)]));
    world.rng_state = 0x0BAD_5EED;
    world
}

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
fn a_quest_start_fights_its_boss_and_the_named_objectives_count_it() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    assert_eq!(
        w.expansion.quests.progress.pack_id, "qb",
        "activation syncs quests"
    );
    w.expansion_talked("ranger");
    assert_eq!(
        w.expansion.quests.quest("hunt").unwrap().status,
        QuestStatus::Active
    );
    fight(&mut w);
    assert!(w.expansion_boss_cleared("alpha"));
    let q = w.expansion.quests.quest("hunt").unwrap();
    assert_eq!(q.status, QuestStatus::Completed, "{q:?}");
    assert!(w.expansion.quests.progress.flags.is_set("hunted"));
}

#[test]
fn deactivation_keeps_the_progress_and_the_cleared_boss() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    w.expansion_talked("ranger");
    fight(&mut w);
    assert!(w.expansion_boss_cleared("alpha"));
    w.deactivate_expansion();
    assert!(!w.expansion.is_active());
    assert!(w.expansion.quests.dormant.is_some());
    w.activate_expansion(pack()).unwrap();
    assert!(
        w.expansion_boss_cleared("alpha"),
        "the once-only record survives"
    );
    assert_eq!(
        w.expansion.quests.quest("hunt").unwrap().status,
        QuestStatus::Completed
    );
}

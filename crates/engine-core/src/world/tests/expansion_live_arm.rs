//! Arming the live loop with an expansion pack active: the synthetic
//! encounter fallback a scene with no encounter table takes replaces the
//! formation table and monster catalog, and must put back what the pack's
//! fights seat - the retail monsters the scene entry merged and the pack's
//! own formations.

use std::sync::Arc;

use super::*;
use legaia_engine_battle::monster_catalog::{MonsterCatalog, MonsterDef};
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const PACK_TOML: &str = r#"
id = "armpack"
name = "Arm Pack"
version = "0.1.0"
"#;

const BATTLE_TOML: &str = r#"
[monsters.dire_wolf]
base = { kind = "monster", id = 9 }
name = "Dire Wolf"
exp = 1000

[formations.pair]
monsters = ["dire_wolf", { kind = "monster", id = 10 }]
"#;

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert("pack.toml".into(), PACK_TOML.as_bytes().to_vec());
    files.insert("battle.toml".into(), BATTLE_TOML.as_bytes().to_vec());
    Arc::new(ExpansionPack::from_files(&files).expect("synthetic pack parses"))
}

#[test]
fn the_encounter_fallback_keeps_the_packs_monsters_and_formations() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    w.activate_expansion(pack()).expect("pack activates");
    // What the scene entry's merge leaves: the pack's base monsters.
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(9, "Wolf", 35, 12));
    cat.insert(MonsterDef::new(10, "Rat", 20, 6));
    w.set_monster_catalog(cat);
    w.install_expansion_formations();
    assert!(w.encounters.session.is_none());

    w.arm_live_loop("nowhere", &crate::live_loop::LiveLoopOpts::playable());

    assert_eq!(w.tables.monster_catalog.get(9).unwrap().name, "Wolf");
    assert_eq!(w.tables.monster_catalog.get(10).unwrap().name, "Rat");
    let id = w.expansion_formation_id("pair").expect("pack formation");
    assert!(w.tables.formation_table.by_id.contains_key(&id));
}

#[test]
fn with_no_pack_the_fallback_installs_the_stand_in_tables_as_before() {
    let mut plain = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    plain.arm_live_loop("nowhere", &crate::live_loop::LiveLoopOpts::playable());
    let vanilla = legaia_engine_battle::monster_catalog::vanilla_monster_catalog();
    assert_eq!(
        plain.tables.monster_catalog.by_id.len(),
        vanilla.by_id.len()
    );
    assert_eq!(
        plain.tables.formation_table.by_id.len(),
        legaia_engine_battle::monster_catalog::vanilla_formation_table()
            .by_id
            .len()
    );
}

//! Disc-gated: an expansion-pack boss derived from a real retail monster
//! enters a real battle through the scene host, fights with the pack's
//! numbers on the retail record, and hands the scene its catalog back.
//!
//! The boss is built on Caruban (monster `0x49`, the rikuroa boss) and
//! started in `town01`, a scene whose own formations never seat it - so the
//! fight also proves the host merges a pack's base monsters from the
//! archive in any scene. Skips and passes without `LEGAIA_DISC_BIN`.

use std::sync::Arc;

use legaia_engine_core::live_loop::LiveLoopOpts;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const CARUBAN: u16 = 0x49;

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"disctest\"\nname = \"Disc Test\"\nversion = \"0.1.0\"\n".to_vec(),
    );
    files.insert(
        "battle.toml".into(),
        format!(
            r#"
[monsters.ashen_caruban]
base = {{ kind = "monster", id = {CARUBAN} }}
name = "Ashen Caruban"
hp = {{ mul = 3.0 }}
attack = {{ mul = 1.5 }}
exp = 9000

[formations.ash_throne]
monsters = ["ashen_caruban"]

[bosses.ash_king]
formation = "ash_throne"
"#
        )
        .into_bytes(),
    );
    Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

#[test]
fn a_derived_boss_enters_a_real_battle_with_its_scaled_stats() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open the disc");
    let scene = "town01";
    host.enter_field_scene(scene, 0).expect("enter town01");
    host.world.arm_live_loop(scene, &LiveLoopOpts::playable());
    host.world.toggles.live_gameplay_loop = true;
    host.world.battle.player_driven = false;

    host.world
        .activate_expansion(pack())
        .expect("pack activates");
    host.merge_expansion_monsters();
    let base = host
        .world
        .tables
        .monster_catalog
        .get(CARUBAN)
        .cloned()
        .expect("the archive merge seats the retail Caruban");
    assert_ne!(base.raw_stats, [0; 6], "a real record, not a placeholder");

    assert!(host.world.start_expansion_boss("ash_king"));
    for _ in 0..2_000 {
        if host.world.mode == SceneMode::Battle {
            break;
        }
        host.world.tick();
    }
    assert_eq!(host.world.mode, SceneMode::Battle, "the boss fight starts");

    let seat = usize::from(host.world.party.party_count);
    let actor = &host.world.actors[seat];
    assert_eq!(actor.battle_monster_id, Some(CARUBAN), "the retail mesh id");
    let want_hp = ((u32::from(base.hp) * 3000 + 500) / 1000).min(u32::from(u16::MAX)) as u16;
    assert_eq!(actor.battle.max_hp, want_hp);
    // No-escape boss = a scripted fight: ATK installs at x5/4 of the
    // scaled record value.
    assert!(host.world.battle.no_escape);
    let raw_atk = ((u32::from(base.raw_stats[1]) * 1500 + 500) / 1000) as u16;
    assert_eq!(host.world.battle.attack[seat], raw_atk + (raw_atk >> 2));
    let live = host.world.tables.monster_catalog.get(CARUBAN).unwrap();
    assert_eq!((live.name.as_str(), live.exp), ("Ashen Caruban", 9000));

    host.world.abandon_live_battle();
    assert_eq!(
        host.world.tables.monster_catalog.get(CARUBAN).map(|d| d.hp),
        Some(base.hp),
        "the catalog's Caruban is the retail one again"
    );
    host.world.deactivate_expansion();
    assert!(host.world.expansion.pack.is_none());
}

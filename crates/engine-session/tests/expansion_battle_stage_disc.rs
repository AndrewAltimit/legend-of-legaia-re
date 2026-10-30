//! Disc-gated: an expansion pack's `stage` field moves a fight onto a
//! retail battle stage of another block, through the one stage resolver
//! both play hosts run, and its `tint` reaches the stage palettes.
//!
//! The fights start in `town01`, whose own stage is its second sub-area
//! backdrop (entry 7). A boss names the overworld stage (`map01`, entry 88)
//! with a tint; a plain formation names `town01`'s fourth stream. With the
//! pack gone the room's own stage is back. Skips without `LEGAIA_DISC_BIN`.

use std::sync::Arc;

use legaia_engine_core::live_loop::LiveLoopOpts;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::scene::{Scene, upload_battle_stage_tims_into_vram_tinted};
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_session::battle_stage::resolve_battle_stage;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const TINT: [u8; 3] = [200, 96, 64];

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"stagetest\"\nname = \"Stage Test\"\nversion = \"0.1.0\"\n".to_vec(),
    );
    files.insert(
        "battle.toml".into(),
        br#"
[formations.plain]
monsters = [{ kind = "monster", id = 1 }]
stage = { scene = "town01", index = 3 }

[formations.boss_row]
monsters = [{ kind = "monster", id = 1 }]
stage = { scene = "town01", index = 0 }

[bosses.sky_king]
formation = "boss_row"
stage = { scene = "map01", tint = [200, 96, 64] }
"#
        .to_vec(),
    );
    Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

fn run_to_battle(host: &mut SceneHost) {
    for _ in 0..2_000 {
        if host.world.mode == SceneMode::Battle {
            return;
        }
        host.world.tick();
    }
    panic!("the fight never started");
}

#[test]
fn a_pack_fight_is_fought_on_the_stage_it_names() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open the disc");
    host.enter_field_scene("town01", 0).expect("enter town01");
    host.world
        .arm_live_loop("town01", &LiveLoopOpts::playable());
    host.world.toggles.live_gameplay_loop = true;
    host.world.battle.player_driven = false;
    let own = host.battle_stage_entry();
    assert!(own.is_some(), "town01 has a stage of its own");
    assert_eq!(host.battle_stage_scene(), None);
    assert_eq!(host.battle_stage_tint(), None);

    host.world
        .activate_expansion(pack())
        .expect("pack activates");
    host.merge_expansion_monsters();
    // Outside a fight the pack names no stage.
    assert_eq!(host.battle_stage_entry(), own);

    assert!(host.world.start_expansion_boss("sky_king"));
    run_to_battle(&mut host);
    // The boss's stage wins over its formation's.
    assert_eq!(host.battle_stage_scene(), Some("map01"));
    assert_eq!(
        host.battle_stage_entry(),
        host.index.battle_stage_entry_for_scene("map01")
    );
    assert_eq!(host.battle_stage_tint(), Some(TINT));
    let stage = resolve_battle_stage(&host, None).expect("the overworld stage resolves");
    assert_eq!(Some(stage.stage_entry), host.battle_stage_entry());
    // The ground tile's palette row carries the tint.
    let map01 = Scene::load(&host.index, "map01").expect("map01");
    let mut plain = legaia_tim::Vram::new();
    let mut tinted = legaia_tim::Vram::new();
    upload_battle_stage_tims_into_vram_tinted(&map01, stage.stage_entry, &mut plain, None);
    upload_battle_stage_tims_into_vram_tinted(&map01, stage.stage_entry, &mut tinted, Some(TINT));
    let row: Vec<_> = (0..16).map(|x| stage.vram.pixel(x, 479)).collect();
    let want: Vec<_> = (0..16).map(|x| tinted.pixel(x, 479)).collect();
    let raw: Vec<_> = (0..16).map(|x| plain.pixel(x, 479)).collect();
    assert_eq!(row, want, "the stage VRAM holds the tinted palette");
    assert_ne!(row, raw, "the tint changed the palette");
    host.world.abandon_live_battle();

    assert!(host.world.start_expansion_battle("plain"));
    run_to_battle(&mut host);
    // Same block as the room: no other block to load, the fourth stream.
    assert_eq!(host.battle_stage_scene(), None);
    let fourth = host.index.battle_stage_entries("town01").get(3).copied();
    assert!(fourth.is_some());
    assert_eq!(host.battle_stage_entry(), fourth);
    assert_eq!(host.battle_stage_tint(), None);
    host.world.abandon_live_battle();

    host.world.deactivate_expansion();
    assert_eq!(host.battle_stage_scene(), None);
}

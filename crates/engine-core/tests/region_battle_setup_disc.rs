//! Disc-gated: the battle-setup half of the region reader `FUN_801D9E1C`
//! (`0x801DA058..0x801DA12C`) over every scene's MAN region records.
//!
//! - Every battle scene region's stage variant (`region[+8] & 0x1F`, `_DAT_8007BD60`)
//!   names a `scene_tmd_stream` entry at `scene_index + variant + 3` - the
//!   entry battle init `FUN_800513F0` loads through `FUN_8001FA88` - and the
//!   two pinned backdrops (`town01` -> 7, `map01` -> 88) fall out of it.
//! - The Door gates read the way the item list uses them: overworld regions
//!   open the Door of Wind, dungeon regions the Door of Light, town regions
//!   neither.
//!
//! Structural assertions only (indices, bits). Skip-passes without
//! `LEGAIA_DISC_BIN`.

use std::path::PathBuf;

use legaia_engine_core::region_encounter::{region_battle_setup, region_encounter_table_from_man};
use legaia_engine_core::scene::{Scene, SceneHost};

fn open_host() -> Option<(SceneHost, PathBuf)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            let host = SceneHost::open_extracted(&d).expect("open SceneHost");
            return Some((host, d));
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

fn setups(
    host: &SceneHost,
    scene: &str,
) -> Vec<legaia_engine_core::region_encounter::RegionBattleSetup> {
    let Ok(s) = Scene::load(&host.index, scene) else {
        return Vec::new();
    };
    let Ok(Some(man)) = s.field_man_payload(&host.index) else {
        return Vec::new();
    };
    region_encounter_table_from_man(scene, &man)
        .map(|t| {
            t.regions
                .iter()
                .map(|r| region_battle_setup(&r.setup))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn every_region_stage_variant_names_a_stage_stream() {
    let Some((host, dir)) = open_host() else {
        return;
    };
    let cdname = legaia_prot::cdname::parse(&dir.join("CDNAME.TXT")).expect("CDNAME");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();
    let (mut checked, mut scenes) = (0usize, 0usize);
    let mut misses: Vec<String> = Vec::new();
    for scene in &names {
        let s = setups(&host, scene);
        if s.is_empty() {
            continue;
        }
        scenes += 1;
        for setup in s {
            let entry = host
                .index
                .battle_stage_entry_for_region(scene, setup.stage_variant);
            if entry.is_none() {
                misses.push(format!("{scene}:{}", setup.stage_variant));
            }
            checked += 1;
        }
    }
    let miss_count = misses.len();
    misses.dedup();
    eprintln!("variants naming no stage stream: {misses:?}");
    // The misses are the opening / ending cutscene scenes and the minigame
    // blocks (no battle is fought there), `koin1b`, and `taiku2`, whose
    // region table is `taiku`'s verbatim while its own block carries fewer
    // stage streams - what retail loads for a `taiku2` fight is open. The
    // port falls back to `battle_stage_entry_for_scene` for all of them.
    for m in &misses {
        let scene = m.split(':').next().unwrap_or("");
        assert!(
            scene.starts_with("ed")
                || scene.starts_with("op")
                || scene.starts_with("other")
                || scene == "koin1b"
                || scene == "taiku2",
            "unexpected stage-variant miss {m}"
        );
    }
    eprintln!(
        "[ok] {} of {checked} region variants over {scenes} scenes name a stage stream",
        checked - miss_count
    );
    assert!(scenes > 50 && checked > 300, "the sweep covered the disc");
}

#[test]
fn the_pinned_backdrops_fall_out_of_the_region_variant() {
    let Some((host, _)) = open_host() else {
        return;
    };
    // Rim Elm's first region block (the village, where the Tetsu match is
    // fought) names variant 1; the overworld's names variant 0.
    let town = setups(&host, "town01");
    assert_eq!(town[0].stage_variant, 1);
    assert_eq!(
        host.index.battle_stage_entry_for_region("town01", 1),
        Some(7)
    );
    let map = setups(&host, "map01");
    assert!(map.iter().all(|s| s.stage_variant == 0));
    assert_eq!(
        host.index.battle_stage_entry_for_region("map01", 0),
        Some(88)
    );
    eprintln!("[ok] town01 -> 7, map01 -> 88 from the region variant");
}

#[test]
fn the_door_gates_split_overworld_dungeon_and_town() {
    let Some((host, _)) = open_host() else {
        return;
    };
    for s in setups(&host, "map01") {
        assert!(
            !s.door_of_wind_blocked,
            "the overworld allows the Door of Wind"
        );
        assert!(
            s.door_of_light_blocked,
            "the overworld has no dungeon to leave"
        );
        assert_eq!(
            s.world_map_return,
            Some(legaia_engine_core::region_encounter::WorldMapReturn {
                map_word: 0,
                tile_x: 0,
                tile_z: 0,
            })
        );
    }
    for s in setups(&host, "cave01") {
        assert!(
            !s.door_of_light_blocked,
            "a dungeon allows the Door of Light"
        );
        assert!(s.door_of_wind_blocked);
        // The return point is on map01 (CDNAME 85).
        assert_eq!(s.world_map_return.map(|r| r.map_word), Some(85));
    }
    for s in setups(&host, "town01") {
        assert!(s.door_of_light_blocked && s.door_of_wind_blocked);
    }
    eprintln!("[ok] Door gates: overworld / dungeon / town");
}

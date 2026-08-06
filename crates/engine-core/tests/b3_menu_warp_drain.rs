//! Lane B3: the menu-staged transitions actually drain.
//!
//! A committed Door of Wind pick stages retail's `0x80084628`/`24`/`2C`
//! triple on [`World::pending_menu_warp`]; a committed Door of Light stages
//! [`World::pending_menu_escape`]. Both were disclosed as undrained. The
//! world tick's `World::drain_staged_menu_warp` now converts them into the
//! named scene transition the scene host consumes
//! ([`World::pending_named_scene_transition`]).
//!
//! The id-space grounding (what unblocked the drain): a placement record's
//! `scene_id` is the destination scene's **raw CDNAME TOC index** - the
//! on-disc values are `0x55`/`0xF4`/`0x187` (the `map01/02/03` kingdom
//! bases, the same words `kingdom_index_for_scene_base` maps) plus `0x162`
//! (`son`, Soren Camp) and `0x215` (`korout`, Sol exterior). The tile pair
//! seats the party at `(tile << 7) + 0x40`, the world-map arrival kernel's
//! own conversion (`FUN_801EE328`).
//!
//! The disc-free tests drive the drain over a hand-built TOC map; the
//! disc-gated one proves the real disc's placement table resolves through
//! the map `SceneHost` installs at construction. Skips + passes without
//! `LEGAIA_DISC_BIN`.

use legaia_engine_core::pause_screens::StagedWarp;
use legaia_engine_core::world::{SceneMode, World};
use std::path::PathBuf;

fn toc_map() -> legaia_prot::cdname::IndexMap {
    let mut map = legaia_prot::cdname::IndexMap::new();
    map.insert(0x55, "map01".to_string());
    map.insert(0xF4, "map02".to_string());
    map.insert(0x187, "map03".to_string());
    map.insert(0x162, "son".to_string());
    map.insert(0x215, "korout".to_string());
    map
}

#[test]
fn a_staged_door_of_wind_warp_drains_to_a_named_scene_transition() {
    let mut w = World::new();
    w.install_scene_toc_names(toc_map());
    w.pending_menu_warp = Some(StagedWarp {
        scene_id: 0x55,
        menu_x: 96,
        menu_y: 25,
    });
    let _ = w.tick();
    assert!(w.pending_menu_warp.is_none(), "the stage is consumed");
    assert_eq!(
        w.pending_named_scene_transition,
        Some(("map01".to_string(), 96, 25, 0)),
        "Rim Elm's record warps onto the Drake kingdom map at its tile"
    );
}

#[test]
fn a_field_scene_destination_resolves_too() {
    // `son` (Soren Camp, 0x162) is a placement destination that is NOT a
    // kingdom overworld - the named-transition drain routes non-`mapNN`
    // names through `enter_field_scene`, so the drain must not special-case
    // the kingdom bases.
    let mut w = World::new();
    w.install_scene_toc_names(toc_map());
    w.pending_menu_warp = Some(StagedWarp {
        scene_id: 0x162,
        menu_x: 22,
        menu_y: 62,
    });
    let _ = w.tick();
    assert_eq!(
        w.pending_named_scene_transition,
        Some(("son".to_string(), 22, 62, 0))
    );
}

#[test]
fn an_unresolvable_scene_word_is_dropped_not_invented() {
    // Retail's miss arm is the `UNFIND MAP NUMBER %d` park (`FUN_801EE328`
    // phase 0x63): nothing warps. No TOC map installed = every id misses.
    let mut w = World::new();
    w.pending_menu_warp = Some(StagedWarp {
        scene_id: 0x55,
        menu_x: 96,
        menu_y: 25,
    });
    let _ = w.tick();
    assert!(w.pending_menu_warp.is_none(), "consumed either way");
    assert_eq!(
        w.pending_named_scene_transition, None,
        "no invented destination"
    );
}

#[test]
fn a_staged_escape_returns_to_the_visited_kingdom_tile() {
    let mut w = World::new();
    w.enter_world_map();
    w.world_map_ctrl
        .as_mut()
        .expect("controller installed")
        .panels
        .note_visit(1, 40, 50);
    // Back in a field scene (a dungeon), the Door of Light commits.
    w.mode = SceneMode::Field;
    w.pending_menu_escape = true;
    let _ = w.tick();
    assert!(!w.pending_menu_escape, "the stage is consumed");
    assert_eq!(
        w.pending_named_scene_transition,
        Some(("map02".to_string(), 40, 50, 0)),
        "escape returns to the stored world-map tile (kingdom 1 = map02)"
    );
}

#[test]
fn an_escape_with_no_visited_record_is_dropped() {
    let mut w = World::new();
    w.pending_menu_escape = true;
    let _ = w.tick();
    assert!(!w.pending_menu_escape);
    assert_eq!(w.pending_named_scene_transition, None);
}

// ---------------------------------------------------------------------------
// Disc-gated: the real placement table resolves through the map the scene
// host installs.
// ---------------------------------------------------------------------------

fn disc_path() -> Option<PathBuf> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    path.is_file().then_some(path)
}

#[test]
fn every_disc_placement_scene_id_resolves_through_the_installed_toc_map() {
    use legaia_engine_core::Vfs;
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let host = legaia_engine_core::scene::SceneHost::open_disc(&path).expect("open disc");
    assert!(
        !host.world.scene_toc_names.is_empty(),
        "SceneHost::new installed the CDNAME TOC map into the world"
    );
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc vfs")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let menu = legaia_asset::worldmap_menu::parse_scus(&scus).expect("placement table parses");
    assert!(!menu.placements.is_empty());
    for p in &menu.placements {
        let name = host
            .world
            .scene_toc_names
            .get(&u32::from(p.scene_id))
            .unwrap_or_else(|| {
                panic!(
                    "placement {} (scene_id 0x{:X}) has no CDNAME block at that TOC index",
                    p.index, p.scene_id
                )
            });
        // The three kingdom bases resolve to the overworld scenes the
        // named-transition drain routes through `enter_world_map_scene`.
        match p.scene_id {
            0x55 => assert_eq!(name, "map01"),
            0xF4 => assert_eq!(name, "map02"),
            0x187 => assert_eq!(name, "map03"),
            _ => assert!(
                !name.is_empty(),
                "non-kingdom destination resolves to a field scene name"
            ),
        }
    }
}

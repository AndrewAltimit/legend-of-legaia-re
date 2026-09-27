//! The menu-staged Door uses drain through the travel arts.
//!
//! A committed Door of Wind pick stages retail's `0x80084628`/`24`/`2C`
//! triple on [`crate::world::MenuState::pending_warp`]; a committed Door of Light stages
//! [`crate::world::MenuState::pending_escape`]. The world tick's
//! `World::drain_staged_menu_warp` hands either to the pause-menu session
//! (`FUN_801ED308`), whose ramp-down installs the travel art its exit code
//! names - Rula (`FUN_801EE328`) for the Door of Wind's `5 + 3`, Riremito
//! (`FUN_801EE094`) for the Door of Light's `4 + 3` - and the art's resolve
//! issues the named scene transition the scene host consumes
//! ([`World::pending_named_scene_transition`]).
//!
//! The id-space grounding: a placement record's `scene_id` is the
//! destination scene's **raw CDNAME TOC index** - the on-disc values are
//! `0x55`/`0xF4`/`0x187` (the `map01/02/03` kingdom bases, the same words
//! `kingdom_index_for_scene_base` maps) plus `0x162` (`son`, Soren Camp) and
//! `0x215` (`korout`, Sol exterior). The tile pair seats the party at
//! `(tile << 7) + 0x40`, the arts' own conversion.
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

/// A world with a live player actor: the arts' opener program (flag `0x0B`,
/// cleared by the program's own state 4) steps only while a player is seated.
fn world_with_player() -> World {
    let mut w = World::new();
    w.install_scene_toc_names(toc_map());
    w.spawn_actor(0).active = true;
    w.player_actor_slot = Some(0);
    w.mode = SceneMode::Field;
    w
}

/// Tick until the art issues its transition; the frame count it took.
fn run_until_transition(w: &mut World, max: usize) -> Option<usize> {
    for n in 1..=max {
        let _ = w.tick();
        if w.pending_named_scene_transition.is_some() {
            return Some(n);
        }
    }
    None
}

#[test]
fn a_staged_door_of_wind_warp_runs_rula_then_transitions() {
    use legaia_engine_core::world::pause_session::PauseSessionStage;
    use legaia_engine_vm::travel_art_actor::TravelArt;
    let mut w = world_with_player();
    w.menu.pending_warp = Some(StagedWarp {
        scene_id: 0x55,
        menu_x: 96,
        menu_y: 25,
    });
    let _ = w.tick();
    assert!(w.menu.pending_warp.is_none(), "the stage is consumed");
    assert!(w.pause_session_active(), "the session took it");
    assert_eq!(
        w.pending_named_scene_transition, None,
        "no direct transition: the art runs first"
    );
    // The session's ramp-down reaches its phase-7 arm and hands on to 0x2B.
    let mut art = None;
    for _ in 0..60 {
        let _ = w.tick();
        if let Some(s) = w.menu.pause_session.as_ref()
            && let PauseSessionStage::Art(a) = &s.stage
        {
            art = Some((s.handler_id, a.art));
            break;
        }
    }
    assert_eq!(art, Some((0x2B, TravelArt::Rula)));
    let frames = run_until_transition(&mut w, 600).expect("Rula resolves");
    assert!(frames > 10, "the lift takes time");
    assert_eq!(
        w.pending_named_scene_transition,
        Some(("map01".to_string(), 96, 25, 0)),
        "Rim Elm's record warps onto the Drake kingdom map at its tile"
    );
    let fade = w
        .presentation
        .fade
        .as_ref()
        .expect("the phase exit's fade is up");
    assert_eq!(fade.kind, 2, "the B - F fade to black");
    assert_eq!(fade.mode[1], -1, "held until the destination loads");
}

#[test]
fn a_field_scene_destination_resolves_too() {
    // `son` (Soren Camp, 0x162) is a placement destination that is NOT a
    // kingdom overworld - the named-transition drain routes non-`mapNN`
    // names through `enter_field_scene`, so the drain must not special-case
    // the kingdom bases.
    let mut w = world_with_player();
    w.menu.pending_warp = Some(StagedWarp {
        scene_id: 0x162,
        menu_x: 22,
        menu_y: 62,
    });
    assert!(run_until_transition(&mut w, 600).is_some());
    assert_eq!(
        w.pending_named_scene_transition,
        Some(("son".to_string(), 22, 62, 0))
    );
}

#[test]
fn an_unresolvable_scene_word_is_dropped_not_invented() {
    // Retail's miss arm is the `UNFIND MAP NUMBER %d` park (`FUN_801EE328`
    // phase 0x63): nothing warps. No TOC map installed = every id misses,
    // and the engine drops the use before installing an art.
    let mut w = World::new();
    w.menu.pending_warp = Some(StagedWarp {
        scene_id: 0x55,
        menu_x: 96,
        menu_y: 25,
    });
    let _ = w.tick();
    assert!(w.menu.pending_warp.is_none(), "consumed either way");
    assert!(!w.pause_session_active());
    assert_eq!(
        w.pending_named_scene_transition, None,
        "no invented destination"
    );
}

#[test]
fn a_staged_escape_runs_riremito_back_to_the_visited_kingdom_tile() {
    use legaia_engine_core::world::pause_session::PauseSessionStage;
    use legaia_engine_vm::travel_art_actor::TravelArt;
    let mut w = world_with_player();
    w.enter_world_map();
    w.world_map
        .ctrl
        .as_mut()
        .expect("controller installed")
        .panels
        .note_visit(1, 40, 50);
    // Back in a field scene (a dungeon), the Door of Light commits.
    w.mode = SceneMode::Field;
    w.menu.pending_escape = true;
    let _ = w.tick();
    assert!(!w.menu.pending_escape, "the stage is consumed");
    let mut art = None;
    for _ in 0..60 {
        let _ = w.tick();
        if let Some(s) = w.menu.pause_session.as_ref()
            && let PauseSessionStage::Art(a) = &s.stage
        {
            art = Some((s.handler_id, a.art));
            break;
        }
    }
    assert_eq!(art, Some((0x29, TravelArt::Riremito)));
    assert!(
        run_until_transition(&mut w, 600).is_some(),
        "Riremito resolves"
    );
    assert_eq!(
        w.pending_named_scene_transition,
        Some(("map02".to_string(), 40, 50, 0)),
        "escape returns to the stored world-map tile (kingdom 1 = map02)"
    );
}

#[test]
fn the_arrival_drops_the_held_fade() {
    let mut w = world_with_player();
    w.menu.pending_warp = Some(StagedWarp {
        scene_id: 0x162,
        menu_x: 22,
        menu_y: 62,
    });
    assert!(run_until_transition(&mut w, 600).is_some());
    assert!(w.presentation.fade.is_some());
    // The scene host takes the transition; the next tick clears the fade the
    // old scene's actor list would have taken with it.
    w.pending_named_scene_transition = None;
    let _ = w.tick();
    assert!(w.presentation.fade.is_none());
    assert!(!w.pause_session_active());
}

#[test]
fn an_escape_with_no_visited_record_is_dropped() {
    let mut w = World::new();
    w.menu.pending_escape = true;
    let _ = w.tick();
    assert!(!w.menu.pending_escape);
    assert!(!w.pause_session_active());
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
        !host.world.tables.scene_toc_names.is_empty(),
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
            .tables
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

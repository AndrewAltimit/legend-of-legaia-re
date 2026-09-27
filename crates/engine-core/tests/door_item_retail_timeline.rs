//! The Door of Light / Door of Wind hand-off, pinned to a retail capture.
//!
//! `scripts/pcsx-redux/autorun_door_item_use.lua` used a Door of Light
//! (item `0x88`) in cave01 (`cave01_attached_light`) and a Door of Wind
//! (`0x89`) on map03 (`karisto_sol_pre_encounter`) and logged, per field tick,
//! the subsystem actor's handler id `+0x50`, phase `+0x54` and dwell `+0x9E`
//! at the dispatcher's `jalr` (`0x801F1634`), the exit code `_DAT_8007B43C`,
//! the brightness `_DAT_8007B440` and every `FUN_80024E80` fade spawn. The
//! cave01 run steps at frame step 2. From the first field tick after the
//! menu closes (the close's `+3` has turned the Use screen's `4` into `7`):
//!
//! | ticks | handler | phase | what |
//! |---|---|---|---|
//! | 1 | `0x30` | 2/3 -> 4 | level pinned at `0xF2` |
//! | 12 | `0x30` | 4 | level `222, 202, ... 2` (`-10` per frame step) |
//! | 1 | `0x30` | 4 -> 6 | level `0`, code cleared |
//! | 1 | `0x30` -> `0x29` | 6 -> 0 | Riremito installed |
//! | 1 | `0x29` | 0 -> 1 | opener effect queued |
//! | 47 | `0x29` | 1 | gated on the queued effect (`FUN_8003CE64(0x0B)`) |
//! | 40 | `0x29` | 1 -> 2 | dwell `2 .. 0x50`; fade spawn, dwell zeroed |
//! | 20 | `0x29` | 2 -> 3 | dwell `2 .. 0x28`, **not** zeroed on exit |
//! | 1 | `0x29` | 3 -> 4 | resolve: `FUN_8001FD44(record, 0x55)` |
//!
//! The resolve seated the party at `(37 << 7) + 0x40, (109 << 7) + 0x40` on
//! map01 - the return triple cave01's long-layout region record stored at
//! `0x80084628` / `24` / `2C`, refreshed by the menu's installer.
//!
//! The effect gate is the one span this test does not pin to a count: the
//! engine's opener is the scene's program, not retail's, and the gate is
//! modelled by that program's own clear.

use legaia_engine_core::region_encounter::{RegionBattleSetup, WorldMapReturn};
use legaia_engine_core::world::pause_session::PauseSessionStage;
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::travel_art_actor::TravelArt;

fn toc_map() -> legaia_prot::cdname::IndexMap {
    let mut map = legaia_prot::cdname::IndexMap::new();
    map.insert(0x55, "map01".to_string());
    map
}

/// A cave01-like world: a seated player, frame step 2, and the region setup
/// cave01's record leaves (Door of Light open, return `0x55 @ (37, 109)`).
fn cave_world() -> World {
    let mut w = World::new();
    w.install_scene_toc_names(toc_map());
    w.spawn_actor(0).active = true;
    w.player_actor_slot = Some(0);
    w.mode = SceneMode::Field;
    w.clock.frame_step = 2;
    w.encounters.region_setup = Some(RegionBattleSetup {
        stage_variant: 0,
        keep_backdrop_object_1: Some(false),
        door_of_light_blocked: false,
        door_of_wind_blocked: true,
        world_map_return: Some(WorldMapReturn {
            map_word: 0x55,
            tile_x: 37,
            tile_z: 109,
        }),
    });
    w
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Session { phase: i16, level: i32 },
    Art { phase: u16, dwell: i16 },
    Arriving,
    None,
}

fn row(w: &World) -> Row {
    match w.menu.pause_session.as_ref().map(|s| &s.stage) {
        Some(PauseSessionStage::Session { phase, level, .. }) => Row::Session {
            phase: *phase,
            level: *level,
        },
        Some(PauseSessionStage::Art(a)) => Row::Art {
            phase: a.phase,
            dwell: a.dwell,
        },
        Some(PauseSessionStage::Arriving) => Row::Arriving,
        None => Row::None,
    }
}

#[test]
fn a_door_of_light_follows_the_captured_session_and_riremito_timeline() {
    let mut w = cave_world();
    w.menu.pending_escape = true;
    // Tick 1: the drain seeds the session and the session runs its first
    // post-menu tick in the same world tick.
    let _ = w.tick();
    assert_eq!(
        row(&w),
        Row::Session {
            phase: 4,
            level: 0xF2
        },
        "the first tick lands on phase 4 at the pinned level"
    );
    // Twelve ramp ticks, -20 each at frame step 2.
    for k in 1..=12 {
        let _ = w.tick();
        assert_eq!(
            row(&w),
            Row::Session {
                phase: 4,
                level: 0xF2 - 20 * k
            }
        );
    }
    let _ = w.tick();
    assert_eq!(row(&w), Row::Session { phase: 6, level: 0 });
    // Phase 6 installs Riremito at its phase 0 ...
    let _ = w.tick();
    let s = w.menu.pause_session.as_ref().expect("session");
    assert_eq!(s.handler_id, 0x29);
    assert!(matches!(&s.stage, PauseSessionStage::Art(a) if a.art == TravelArt::Riremito));
    assert_eq!(row(&w), Row::Art { phase: 0, dwell: 0 });
    // ... whose next tick queues the opener and enters phase 1.
    let _ = w.tick();
    assert_eq!(row(&w), Row::Art { phase: 1, dwell: 0 });
    // The effect gate: however long the engine's opener runs, the dwell
    // does not move until it clears.
    let mut gated = 0;
    while row(&w) == (Row::Art { phase: 1, dwell: 0 }) {
        let _ = w.tick();
        gated += 1;
        assert!(gated < 600, "the opener effect never cleared");
    }
    // The first ungated tick is already a dwell tick.
    let mut dwell_ticks = 1;
    while let Row::Art { phase: 1, dwell } = row(&w) {
        assert_eq!(dwell, 2 * dwell_ticks);
        let _ = w.tick();
        dwell_ticks += 1;
    }
    assert_eq!(dwell_ticks, 40, "0x50 at frame step 2");
    assert_eq!(row(&w), Row::Art { phase: 2, dwell: 0 });
    let fade = w.presentation.fade.as_ref().expect("phase 1's exit fade");
    assert_eq!(fade.kind, 2);
    assert_eq!(fade.duration, 0x20);
    assert_eq!(fade.mode[1], -1);
    for k in 1..=20 {
        let _ = w.tick();
        let expect = if k < 20 {
            Row::Art {
                phase: 2,
                dwell: 2 * k,
            }
        } else {
            Row::Art {
                phase: 3,
                dwell: 0x28,
            }
        };
        assert_eq!(row(&w), expect, "phase-2 tick {k}");
    }
    assert_eq!(w.pending_named_scene_transition, None);
    let _ = w.tick();
    assert_eq!(row(&w), Row::Arriving);
    assert_eq!(
        w.pending_named_scene_transition,
        Some(("map01".to_string(), 37, 109, 0)),
        "the region record's return triple, not a visited-map guess"
    );
}

#[test]
fn a_door_of_light_with_no_region_triple_falls_back_to_the_visited_map() {
    let mut w = cave_world();
    w.encounters.region_setup = None;
    w.enter_world_map();
    w.world_map
        .ctrl
        .as_mut()
        .expect("controller installed")
        .panels
        .note_visit(0, 37, 110);
    w.mode = SceneMode::Field;
    w.menu.pending_escape = true;
    for _ in 0..600 {
        let _ = w.tick();
        if w.pending_named_scene_transition.is_some() {
            break;
        }
    }
    assert_eq!(
        w.pending_named_scene_transition,
        Some(("map01".to_string(), 37, 110, 0))
    );
}

#[test]
fn a_region_triple_that_names_no_scene_drops_the_use() {
    let mut w = cave_world();
    if let Some(s) = w.encounters.region_setup.as_mut() {
        s.world_map_return = Some(WorldMapReturn {
            map_word: 0,
            tile_x: 0,
            tile_z: 0,
        });
    }
    w.menu.pending_escape = true;
    let _ = w.tick();
    assert!(
        !w.pause_session_active(),
        "UNFIND MAP NUMBER 0: nothing runs"
    );
}

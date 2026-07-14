//! Disc-gated: the **round-trip** scene-change teleports around the Drake
//! overworld hub - out of Rim Elm onto `map01`, back into town, into the
//! cave and back out.
//!
//! The one-way leg (Rim Elm -> `map01`) is already pinned by
//! `walk_on_trigger_dispatch_disc`. What this file adds is every **return**
//! leg, which exercises a different mechanism:
//!
//! - **field -> overworld** is the gate-1 `.MAP` walk-on tile trigger ->
//!   MAN partition-2 record -> field-VM `0x3F` named scene change
//!   (`SceneHost::dispatch_walk_on_trigger`);
//! - **overworld -> field** is the *world-map entity SM* path: the same
//!   trigger/record join is lifted at scene entry into an
//!   [`WorldMapEntityConfig::OverworldPortal`]
//!   (`man_field_scripts::overworld_portal_sites`), which
//!   `World::auto_engage_world_map_portals` engages on walk-over and the
//!   host drains as a `WorldMapTransition`.
//!
//! Both directions must land on the destination's **authored** arrival seat
//! and facing (the `0x3F` op's entry bytes), not a default spawn. The
//! authored seats are asserted as literal tiles, so a regression in the
//! entry-byte decode or the seat mapping fails here.
//!
//! Ground truth for the seat mapping: the `door_warp_town01_to_map01`
//! capture parks the live retail player at world `(3264, 5824)` in `town0c`
//! = `seat_player_at_tile(25, 45)` = exactly the arrival seat `map01`'s
//! town entrance names. See `docs/subsystems/world-map.md`.
//!
//! Structural assertions only (scene names, tiles, mode) - no Sony bytes.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{SceneHost, SceneTickEvent};
use legaia_engine_core::world::SceneMode;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

/// Rim Elm's south gate is **story-sealed**: the scene-entry script's
/// collision paints leave the gate band walled until flags 327 + 321 latch
/// (see [`gate_paints_match_retail_live_grid`]). These are the flags the
/// engine must have set for the exit to be walkable at all.
const GATE_FLAGS: [u16; 2] = [327, 321];

fn seat(host: &mut SceneHost, tile_x: i16, tile_z: i16) {
    let slot = host.world.player_actor_slot.expect("player installed") as usize;
    host.world.actors[slot].move_state.world_x = tile_x * 128 + 0x40;
    host.world.actors[slot].move_state.world_z = tile_z * 128 + 0x40;
}

/// The player's current tile under the retail locomotion quantisation
/// (`tile = (world - 0x40) >> 7`, the inverse of the `tile*128 + 0x40`
/// arrival-seat mapping).
fn tile(host: &SceneHost) -> (i32, i32) {
    let slot = host.world.player_actor_slot.expect("player installed") as usize;
    let ms = &host.world.actors[slot].move_state;
    (
        (i32::from(ms.world_x) - 0x40) >> 7,
        (i32::from(ms.world_z) - 0x40) >> 7,
    )
}

fn facing(host: &SceneHost) -> i16 {
    let slot = host.world.player_actor_slot.expect("player installed") as usize;
    host.world.actors[slot].move_state.render_26
}

/// Hold `pad` until a scene change fires (or `n` frames elapse).
fn walk_until_scene(host: &mut SceneHost, pad: u16, n: usize) -> Option<String> {
    let mut entered = None;
    for _ in 0..n {
        host.world.set_pad(pad);
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    host.world.set_pad(0);
    entered
}

/// Enter Rim Elm with the gate flags latched (post-story state) and settle.
fn enter_rim_elm(host: &mut SceneHost, scene: &str) {
    host.enter_field_scene(scene, 0).expect("enter Rim Elm");
    for f in GATE_FLAGS {
        host.world.system_flag_set(f);
    }
    // Re-enter so the scene-entry script's collision paints run against the
    // latched flags (retail re-runs the system script on every scene entry).
    host.enter_field_scene(scene, 0).expect("re-enter Rim Elm");
    for f in GATE_FLAGS {
        host.world.system_flag_set(f);
    }
    for _ in 0..150 {
        host.tick().expect("tick");
    }
}

/// The scene-entry script's gate paints reproduce the **retail live**
/// collision grid byte-for-byte at Rim Elm's south gate.
///
/// The gate is a story gate. With the flags clear (a fresh New Game) the
/// band stays walled - you cannot leave Rim Elm, which is what retail does.
/// Once flags 327 + 321 latch, the entry script's flag-SET arm runs
/// `clear x=24..25 z=45..46` (the gate opening) plus the two side blocks,
/// and the grid matches the `door_warp_town01_to_map01` capture exactly.
#[test]
fn gate_paints_match_retail_live_grid() {
    let Some(mut host) = open_host() else {
        return;
    };
    // Fresh boot: sealed.
    host.enter_field_scene("town0c", 0).expect("enter town0c");
    for _ in 0..150 {
        host.tick().expect("tick");
    }
    let sealed = host.world.field_collision_grid[25 + 47 * 0x80] >> 4;
    assert_ne!(
        sealed, 0,
        "a fresh New Game leaves Rim Elm's gate walled (the story seal)"
    );

    // Post-story: the gate opens, and the grid equals the retail live grid.
    let mut host = open_host().expect("host");
    enter_rim_elm(&mut host, "town0c");
    let g = &host.world.field_collision_grid;
    let cell = |col: usize, row: usize| g[col + row * 0x80] >> 4;
    // Retail live grid (from the door_warp_town01_to_map01 capture's
    // `*(_DAT_1f8003ec) + 0x4000` region) at the gate:
    assert_eq!(
        (22..=29).map(|c| cell(c, 47)).collect::<Vec<_>>(),
        vec![15, 15, 0, 0, 15, 15, 15, 15],
        "grid row 47 (world tile z=46, the gate band) matches the live capture"
    );
    assert_eq!(
        (21..=27).map(|c| cell(c, 46)).collect::<Vec<_>>(),
        vec![15, 15, 0, 0, 0, 15, 15],
        "grid row 46 (world tile z=45) matches the live capture"
    );
}

/// Rim Elm -> `map01`: walking south through the (now open) gate crosses the
/// gate-1 trigger band, whose partition-2 record runs the `0x3F` to `map01`
/// and seats the player at the authored arrival tile.
#[test]
fn town_exit_walks_out_to_the_overworld() {
    let Some(mut host) = open_host() else {
        return;
    };
    enter_rim_elm(&mut host, "town0c");
    seat(&mut host, 25, 42);
    for _ in 0..2 {
        host.tick().expect("tick");
    }
    // Field pad axes: Up = +Z (the gate is south of the spawn).
    let entered = walk_until_scene(&mut host, PadButton::Up.mask(), 600);
    assert_eq!(
        entered.as_deref(),
        Some("map01"),
        "walking into the south gate leaves Rim Elm for the overworld"
    );
    assert_eq!(
        host.world.mode,
        SceneMode::WorldMap,
        "map01 routes through the world-map entry"
    );
    assert_eq!(
        tile(&host),
        (0x60, 0x19),
        "arrival seats the player at the op-0x3F entry tile (96, 25)"
    );
}

/// `map01` -> Rim Elm: the **return** leg, and a different mechanism - the
/// world-map entity SM's `OverworldPortal`. Walking onto the town-entrance
/// tile (96, 24) engages the portal, which warps into `town0c` at the
/// authored arrival seat (25, 45).
///
/// That seat is the live-capture-pinned one: retail's player stands at world
/// `(3264, 5824)` there.
#[test]
fn overworld_walks_back_into_town() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_world_map_scene("map01").expect("enter map01");
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    // The town entrance sits one tile north of the exit's arrival seat.
    seat(&mut host, 0x60, 0x19);
    for _ in 0..2 {
        host.tick().expect("tick");
    }
    // Overworld pad is CAMERA-relative (azimuth 0: Right = world Z-), unlike
    // the field's world-axis pad. This is the axis that walks onto the portal.
    let entered = walk_until_scene(&mut host, PadButton::Right.mask(), 600);
    assert_eq!(
        entered.as_deref(),
        Some("town0c"),
        "walking onto the overworld town marker re-enters Rim Elm"
    );
    assert_eq!(host.world.mode, SceneMode::Field, "town0c is a field scene");
    assert_eq!(
        tile(&host),
        (25, 45),
        "arrival seats the player at the authored entry tile - the same spot \
         the live retail capture parks the player at (world 3264, 5824)"
    );
    let slot = host.world.player_actor_slot.unwrap() as usize;
    let ms = &host.world.actors[slot].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        (3264, 5824),
        "byte-exact against the door_warp_town01_to_map01 capture"
    );
    assert_eq!(
        facing(&host),
        0,
        "arrival faces the op-0x3F `dir` sector 0 (into the town)"
    );
}

/// `map01` -> `cave01` -> `map01`: the cave is enterable and exitable, and
/// each leg lands on its authored seat.
///
/// The cave entrance is `map01`'s gate-1 trigger at (37, 110) -> `cave01`
/// entry (93, 97); the exit is `cave01`'s own gate-1 band at (93..94, 96) ->
/// `map01` entry (37, 109), one tile clear of the entrance so the return
/// does not immediately re-fire.
#[test]
fn cave_is_enterable_and_exitable() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_world_map_scene("map01").expect("enter map01");
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    // Walk onto the cave-mouth portal tile.
    seat(&mut host, 37, 110);
    let mut entered = None;
    for _ in 0..120 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    assert_eq!(
        entered.as_deref(),
        Some("cave01"),
        "the map01 cave-mouth portal enters the cave"
    );
    assert_eq!(host.world.mode, SceneMode::Field);
    assert_eq!(
        tile(&host),
        (93, 97),
        "cave arrival seats at the authored entry tile"
    );

    // ...and back out: walk north onto the cave's own exit band.
    seat(&mut host, 93, 99);
    for _ in 0..2 {
        host.tick().expect("tick");
    }
    let entered = walk_until_scene(&mut host, PadButton::Down.mask(), 600);
    assert_eq!(
        entered.as_deref(),
        Some("map01"),
        "walking into the cave mouth from inside returns to the overworld"
    );
    assert_eq!(host.world.mode, SceneMode::WorldMap);
    assert_eq!(
        tile(&host),
        (37, 109),
        "the cave exit seats one tile clear of the entrance portal (37, 110), \
         so the return does not immediately re-fire the entrance"
    );
}

/// The full loop in one session: town -> overworld -> town -> overworld ->
/// cave -> overworld. Each hop must resolve without the previous one's
/// arrival re-firing a trigger (the ping-pong failure mode).
#[test]
fn full_round_trip_loop_is_stable() {
    let Some(mut host) = open_host() else {
        return;
    };
    enter_rim_elm(&mut host, "town0c");

    // town -> overworld
    seat(&mut host, 25, 46);
    let mut hops: Vec<String> = Vec::new();
    for _ in 0..120 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            hops.push(name);
            break;
        }
    }
    // overworld -> town (the portal one tile north of the arrival seat)
    seat(&mut host, 0x60, 0x18);
    for _ in 0..120 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            hops.push(name);
            break;
        }
    }
    // town -> overworld again (the gate flags are still latched)
    seat(&mut host, 25, 46);
    for _ in 0..120 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            hops.push(name);
            break;
        }
    }
    // overworld -> cave
    seat(&mut host, 37, 110);
    for _ in 0..120 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            hops.push(name);
            break;
        }
    }
    // cave -> overworld
    seat(&mut host, 93, 96);
    for _ in 0..200 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            hops.push(name);
            break;
        }
    }
    assert_eq!(
        hops,
        vec!["map01", "town0c", "map01", "cave01", "map01"],
        "the full town <-> overworld <-> cave loop resolves in order"
    );
    assert_eq!(host.world.mode, SceneMode::WorldMap);

    // Settling on the overworld must NOT bounce back through a portal: the
    // cave exit's arrival seat (37, 109) is clear of the entrance (37, 110).
    for _ in 0..60 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("arrival re-fired a portal into {name} (ping-pong)");
        }
    }
}

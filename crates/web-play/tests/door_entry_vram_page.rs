//! Disc-gated: a scene the play page walks into holds the texture image a
//! direct entry of that scene holds.
//!
//! The page keeps the entered scene's CLUT-walk shimmer
//! (`FieldSceneAnim`) beside the scene host, and the host swaps its scene
//! VRAM inside the session tick. A walker that outlives that swap by even one
//! step copies its 16x1 cells out of the new image, where its source strips
//! were never parked, onto destination cells the new scene's own textures
//! own. The overworld's walker writes CLUT cell `(32, 509)`, which
//! `izumi` and `town0c` both hand to their walk ground: with the cell blank
//! every texel under it is the transparent word, and that ground - the path
//! between Hunter's Spring's hedges, the floors inside Rim Elm's houses -
//! draws as a black hole until the scene is left.
//!
//! Whether a walker is due on the entry tick depends on its hold counter, so
//! each walk is repeated across a spread of idle offsets.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

const UP: u16 = 0x0010;
const DOWN: u16 = 0x0040;

/// A walk off `map01` into a scene: where to stand, which way to hold the
/// pad, and the scene the entrance loads.
struct Route {
    seat: (i16, i16),
    pad: u16,
    scene: &'static str,
}

/// One step north of `map01`'s Rim Elm entrance tile `(96, 24)`.
const RIM_ELM: Route = Route {
    seat: (12362, 3218),
    pad: DOWN,
    scene: "town0c",
};

/// South of `map01`'s Hunter's Spring pond.
const HUNTERS_SPRING: Route = Route {
    seat: (8960, 5060),
    pad: UP,
    scene: "izumi",
};

/// Idle offsets each route is walked at: the overworld walker's hold
/// counters all repeat inside this many page ticks.
const IDLE_OFFSETS: u32 = 64;

/// Walk the route after `idle` standing ticks and return the scene VRAM as
/// the page holds it on the entry tick.
fn walk_in(rt: &mut LegaiaRuntime, route: &Route, idle: u32) -> Vec<u8> {
    rt.enter_field("map01").expect("enter map01");
    for _ in 0..4 + idle {
        let _ = rt.tick_frame();
    }
    assert!(
        rt.debug_seat_player(route.seat.0, route.seat.1),
        "a player to seat"
    );
    for _ in 0..600u32 {
        rt.set_pad(route.pad);
        let entered = rt.tick_frame().expect("tick");
        if !entered.is_empty() {
            rt.set_pad(0);
            assert_eq!(entered, route.scene, "the entrance loads its scene");
            return rt.field_vram_bytes();
        }
    }
    panic!("the walk never entered {}", route.scene);
}

/// VRAM cells (as `(x, y)`) where two images differ.
fn differing_cells(a: &[u8], b: &[u8]) -> Vec<(usize, usize)> {
    a.as_chunks::<2>()
        .0
        .iter()
        .zip(b.as_chunks::<2>().0)
        .enumerate()
        .filter(|(_, (p, q))| p != q)
        .map(|(i, _)| (i % 1024, i / 1024))
        .collect()
}

/// The 16-entry CLUT cells the entered scene's walk ground samples, as
/// `(x, y)`.
fn ground_clut_cells(rt: &LegaiaRuntime) -> Vec<(usize, usize)> {
    let mut cells: Vec<(usize, usize)> = rt
        .field_ground_cba_tsb()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| {
            (
                usize::from(c[0] & 0x3F) * 16,
                usize::from((c[0] >> 6) & 0x1FF),
            )
        })
        .collect();
    cells.sort_unstable();
    cells.dedup();
    cells
}

fn assert_route_keeps_the_image(rt: &mut LegaiaRuntime, route: &Route) {
    // The image a direct entry holds before its first tick: the scene's own
    // uploads with its walker strips parked and nothing yet animated.
    rt.enter_field(route.scene).expect("enter the scene");
    let direct = rt.field_vram_bytes();

    let mut hits: Vec<(u32, usize, (usize, usize))> = Vec::new();
    let mut blanked_ground: Vec<(u32, (usize, usize))> = Vec::new();
    for idle in 0..IDLE_OFFSETS {
        let walked = walk_in(rt, route, idle);
        let cells = differing_cells(&walked, &direct);
        if let Some(&first) = cells.first() {
            hits.push((idle, cells.len(), first));
        }
        for (x, y) in ground_clut_cells(rt) {
            let row = |img: &[u8]| img[(y * 1024 + x) * 2..(y * 1024 + x + 16) * 2].to_vec();
            if row(&walked) != row(&direct) {
                blanked_ground.push((idle, (x, y)));
            }
        }
    }
    assert!(
        blanked_ground.is_empty(),
        "{}: the walked-in scene's ground palettes differ from the direct \
         entry's, as (idle offset, CLUT cell): {blanked_ground:?}",
        route.scene
    );
    assert!(
        hits.is_empty(),
        "{}: the walked-in scene's VRAM differs from the direct entry's, as \
         (idle offset, cells, first cell): {hits:?}",
        route.scene
    );
}

#[test]
fn a_walked_in_scene_holds_the_direct_entrys_texture_image() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(disc).expect("read disc"), String::new())
        .expect("load disc");
    for route in [&HUNTERS_SPRING, &RIM_ELM] {
        // Non-vacuous: the scene's ground does sample the cell the
        // overworld's walker writes.
        rt.enter_field(route.scene).expect("enter the scene");
        assert!(
            ground_clut_cells(&rt).contains(&(32, 509)),
            "{}: the walk ground samples CLUT cell (32, 509)",
            route.scene
        );
        assert_route_keeps_the_image(&mut rt, route);
    }
}

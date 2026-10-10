//! Ladder for the one shipped move-VM part that keeps the camera **yaw**
//! factor on a camera-relative node, driven through the play page.
//!
//! `FUN_8001CF50` rebuilds the camera rotation for a part whose `+0x52`
//! carries a bit of `0x780`, leaving out each flagged axis. The yaw factor
//! (`FUN_8004629C`, `GteMat3::rot_y` under
//! `legaia_engine_ui::gte::camera_view_rotation`) therefore runs only for a
//! word that skips pitch or roll and keeps yaw, and one word on the disc does:
//! `urudre1` stager record 14's `0x0080`
//! (`engine-core/tests/move_ctrl52_census_disc.rs` pins that census).
//!
//! The route is the scene's own content. `urudre1` is Vahn's dream of Rim Elm
//! at Uru Mais; its walk-on band at tile `(97, 10)` starts cutscene record
//! `P2[1]`, which reaches `34 30 05` once its conversation is paged. That op
//! installs stager record 6, whose op `0x25` spawns record 14 once every other
//! frame: a run of `0x4000` sprite-arm quads, each carrying `+0x52 = 0x0080`.
//! The page's field FX pass (`play_field_fx_sync`) composes each quad through
//! `camera_relative_model_prefix` under the engine camera the page drew
//! through, which is the arm that takes the yaw factor.
//!
//! Nothing is hand-built: the seat is a tile the disc's own trigger table
//! names, the pad pulse pages the record's text, and the parts are whatever
//! the record spawns.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset. CI runs without disc
//! data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;
use std::env;

/// Cross, the Confirm that pages a field conversation.
const CROSS: u16 = 0x4000;
/// Stage size the page's camera is asked for.
const W: f32 = 320.0;
const H: f32 = 240.0;
/// The walk-on tile whose trigger row names `P2[1]` (gate 1).
const DREAM_TILE: (i16, i16) = (97, 10);
/// The four d-pad directions (Up, Right, Down, Left).
const DIRECTIONS: [u16; 4] = [0x0010, 0x0020, 0x0040, 0x0080];
/// Frames a held direction gets to cross one 128-unit tile.
const WALK_FRAMES: u32 = 48;
/// Generous bound on the cutscene's run up to its `34 30 05`; the record
/// reaches it about 2000 ticks in under a steady Confirm pulse.
const BUDGET: u32 = 6000;

fn loaded_runtime() -> Option<LegaiaRuntime> {
    let disc = env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

/// World coordinate of a tile's centre.
fn tile_centre(t: i16) -> i16 {
    t * 128 + 0x40
}

/// One page frame: tick, then the camera and the field FX pass the page runs
/// after it. Returns the FX stream's vertex count.
fn frame(rt: &mut LegaiaRuntime, pad: u16) -> usize {
    rt.set_pad(pad);
    rt.tick_frame().expect("tick");
    let vp = rt.play_camera_vp(W, H);
    rt.play_field_fx_sync(&vp);
    rt.play_battle_fx_positions().len() / 3
}

#[test]
fn the_dream_cutscene_draws_its_yaw_keeping_parts_through_the_page() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    // Stand beside the band and walk onto it. The seat is a *standing* one
    // (`play_debug_seat` stamps the walk-on dispatcher's last tile so nothing
    // fires under it), so the record starts only on the crossing the held
    // direction makes - the walk-on dispatch a player's own step runs. Which
    // pad direction faces the band depends on the scene camera, so each
    // neighbour is tried with each direction until the timeline opens.
    let (tx, tz) = DREAM_TILE;
    let mut walked = None;
    'seat: for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        for dir in DIRECTIONS {
            rt.enter_field("urudre1").expect("enter urudre1");
            if !rt.play_debug_seat(tile_centre(tx + dx), tile_centre(tz + dz)) {
                continue;
            }
            assert_eq!(frame(&mut rt, 0), 0, "no field effect before the cutscene");
            for _ in 0..WALK_FRAMES {
                frame(&mut rt, dir);
                if rt.debug_timeline_active() {
                    walked = Some(((dx, dz), dir));
                    break 'seat;
                }
            }
        }
    }
    let Some((from, dir)) = walked else {
        panic!("no one-tile walk onto {DREAM_TILE:?} started the dream cutscene");
    };
    eprintln!("[dream-yaw] record started walking from offset {from:?} with pad {dir:#06x}");

    let mut first = None;
    let mut peak = 0usize;
    for f in 0..BUDGET {
        // Two held frames in sixteen: an edge the pager takes, with releases
        // between so each page needs its own press.
        let pad = if f % 16 < 2 { CROSS } else { 0 };
        let verts = frame(&mut rt, pad);
        if verts > 0 && first.is_none() {
            first = Some(f);
        }
        peak = peak.max(verts);
        // The stager spawns one quad every other frame; a few dozen frames
        // past the first is the whole run.
        if first.is_some_and(|at| f > at + 120) {
            break;
        }
    }
    let Some(first) = first else {
        panic!("the dream cutscene spawned no field effect part within {BUDGET} ticks");
    };
    eprintln!("[dream-yaw] first part quad at tick {first}, peak {peak} vertices");
    assert!(
        peak >= 4 * 8,
        "the stager's run is many quads, not one ({peak} vertices at peak)"
    );
}

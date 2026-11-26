//! Disc-gated: the New Game opening's first shot (`opdeene` `P2[18]`'s
//! `45 07` at `+0x19`) is black behind the crawl, as retail's is.
//!
//! The shot aims the camera at focus tile `(84, 95)`, while the region box
//! the scene entry latched at the player's tile is `(33, 0)..(73, 67)`, and
//! the record's window (`46 24 F5 FC 0D 1C`) reaches no cell the box holds.
//! Retail's cell passes and actor cull test the box whatever owns the camera,
//! so nothing of the map draws: a per-vsync film of the shot from the title
//! (`title_attract`, New Game) shows only the crawl's glyphs until the
//! record's `+0x2B8` shot re-aims at the player, and its `ng_v800` state
//! holds the same box, window and focus. The follow camera's fallback - draw
//! the map whole when the focus leaves the box - had put pack mesh `37`'s
//! panels from the map's far side on screen as the fade lifted.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_world_views::camera_view::cutscene_owns_camera;
use legaia_engine_world_views::field_view_window::{
    crop_framing, field_view_cells, placed_actor_visible,
};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The region box retail's `ng_v800` state holds (`0x1F800384..87`).
const BOX: [u8; 4] = [33, 0, 73, 67];
/// The record's window (`46 24 F5 FC 0D 1C` at `+0x23D`).
const RECORD_WINDOW: [i8; 4] = [-11, -4, 13, 28];
/// The first shot's focus, negated as stored (`45 07` at `+0x19`).
const FIRST_SHOT_FOCUS: [i32; 2] = [-10816, -12224];

#[test]
fn the_first_prologue_shot_draws_no_map() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    assert!(
        s.start_new_game(&FieldLiveOpts::default()).is_some(),
        "the New Game entered its opening"
    );
    assert_eq!(s.host.world.active_scene_label, "opdeene");

    let mut first_shot = 0;
    let mut second_shot = 0;
    for t in 0..900 {
        s.tick().expect("tick");
        let w = &s.host.world;
        if w.active_scene_label != "opdeene" || !cutscene_owns_camera(w) {
            continue;
        }
        let Some(view) = w.npcs.cull_view else {
            continue;
        };
        let cells = field_view_cells(w, crop_framing(w, &s.camera, false));
        if view.focus_stored == FIRST_SHOT_FOCUS {
            // The record's first frames still stand on the scene entry's
            // window and the first of its two player `MOVE_TO`s; retail runs
            // them all on its first field frame, under the timeline's
            // instant black wash. Score the shot from the record's window.
            if view.window != RECORD_WINDOW {
                continue;
            }
            assert_eq!(view.attr_box, BOX, "the box retail latched (t{t})");
            let cells = cells.unwrap_or_else(|| panic!("the shot crops (t{t})"));
            // The decoration pass tests the box per cell and reaches none.
            // The ground emitters' two `do`-`while` loops run once on the
            // window the clamp collapsed, visiting one cell past the box's
            // edge, where the map holds no ground.
            let mut ground = Vec::new();
            for col in 0..128 {
                for row in 0..128 {
                    assert!(
                        !cells.decoration_visible(col, row, 0),
                        "no decoration cell draws in the first shot (t{t}, cell ({col}, {row}))"
                    );
                    if cells.ground_visible(col, row) {
                        ground.push((col, row));
                    }
                }
            }
            assert!(
                ground.len() <= 1
                    && ground
                        .iter()
                        .all(|&(c, r)| { c >= i32::from(BOX[2]) || r >= i32::from(BOX[3]) }),
                "the ground pass visits only its one collapsed cell outside the box (t{t}: {ground:?})"
            );
            // Pack mesh 37's bound placement, the panels the whole-map
            // fallback drew.
            assert!(
                !placed_actor_visible(w, Some(&cells), 12608, 9920, 0),
                "the far-side placement is culled (t{t})"
            );
            first_shot += 1;
        } else if first_shot > 0 {
            // The `+0x2B8` shot re-aims inside the box: the map draws.
            let cells = cells.unwrap_or_else(|| panic!("the second shot crops (t{t})"));
            let any = (0..128).any(|c| (0..128).any(|r| cells.ground_visible(c, r)));
            assert!(any, "the second shot draws ground (t{t})");
            second_shot += 1;
            if second_shot > 30 {
                break;
            }
        }
    }
    eprintln!("[opdeene] first shot {first_shot} frames, second shot {second_shot} frames");
    assert!(
        first_shot > 200,
        "the first shot held its focus ({first_shot} frames)"
    );
    assert!(second_shot > 0, "the record reached its second shot");
}

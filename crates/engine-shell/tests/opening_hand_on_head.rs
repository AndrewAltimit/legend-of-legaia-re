//! The New Game opening's walk-out in `town01`, played from a New Game
//! through the native play window's own frame path (`HeadlessPlayWindow`):
//! Vahn steps out of his house, raises a hand to his head (scene record 45),
//! holds it there (record 46) and lowers it (record 45 again, bound with the
//! restart and reverse bits of `+0x62` raised) before the name entry opens.
//!
//! Retail draws that beat with no pose out of sequence: a PCSX-Redux film
//! from `s2_rimelm_town01` steps the reversed clip's cursor `136, 128, ...`
//! from its first frame (`FUN_800204F8` seats the restart and steps before
//! the draw). The port drew record 45's frame 0 - the hand at Vahn's side,
//! his head up - for one vsync between the hand on his head and the
//! reversed playback, in the default mode and under the retail toggles.
//!
//! The test walks every display frame from the `town01` entry to the name
//! entry and fails on any frame whose drawn player pose leaves its
//! neighbours and comes straight back (a one-vsync pop). Rotations compare
//! on the nearer of a part's two Euler forms, so the decoder's equivalent
//! sets never read as a jump. `LEGAIA_HOH_OUT=<file>` writes the per-frame
//! clip and pose rows.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;
use legaia_engine_title::title::TitlePhase;

const START: u16 = 0x0008;

type Bone = ([i16; 3], [i16; 3]);

fn title_pad(w: &HeadlessPlayWindow, frame: u32) -> u16 {
    if frame % 8 >= 2 {
        return 0;
    }
    match w
        .front_end()
        .and_then(|fe| fe.title_session())
        .map(|t| t.phase())
    {
        Some(TitlePhase::MainMenu { cursor }) if cursor > 0 => PadButton::Up.mask(),
        Some(TitlePhase::MainMenu { .. }) => PadButton::Cross.mask(),
        _ => START,
    }
}

/// The largest part step between two poses: rotation on the nearer Euler
/// form (`(x, y, z)` and `(x + 0x800, 0x800 - y, z + 0x800)` are one
/// orientation), translation as is.
fn pose_step(a: &[Bone], b: &[Bone]) -> i32 {
    let wrap = |d: i32| (d + 2048).rem_euclid(4096) - 2048;
    a.iter()
        .zip(b)
        .map(|(p, c)| {
            let r = |from: [i32; 3]| -> i32 {
                (0..3)
                    .map(|k| wrap(i32::from(c.1[k]) - from[k]).abs())
                    .max()
                    .unwrap_or(0)
            };
            let pr = p.1.map(i32::from);
            let flipped = [pr[0] + 2048, 2048 - pr[1], pr[2] + 2048];
            let t = (0..3)
                .map(|k| (i32::from(c.0[k]) - i32::from(p.0[k])).abs())
                .max()
                .unwrap_or(0);
            r(pr).min(r(flipped)).max(t)
        })
        .max()
        .unwrap_or(0)
}

/// Every drawn player pose from the `town01` entry to the name entry, with
/// a diagnostic row per frame.
fn walk_out(disc: &std::path::Path, retail: bool) -> (Vec<Vec<Bone>>, String) {
    let save_dir = std::env::temp_dir().join(format!(
        "legaia-hoh-{}-{}",
        u8::from(retail),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&save_dir);
    std::fs::create_dir_all(&save_dir).expect("scratch save dir");
    let mut w = HeadlessPlayWindow::open_boot_ui_in(disc, "town01", None, &save_dir)
        .expect("open the boot-UI window");
    let mut frame = 0u32;
    while w.boot_ui_active() {
        assert!(frame < 6000, "the title never resolved");
        let pad = title_pad(&w, frame);
        w.display_frame(pad, 1);
        frame += 1;
    }
    let mut poses = Vec::new();
    let mut rows = String::new();
    let mut in_town = false;
    for f in 0..40_000u32 {
        if w.session().host.world.name_entry_active() {
            break;
        }
        // Cross through the prologue; the walk-out itself takes no pad.
        let pad = if !in_town && f % 4 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        };
        w.display_frame(pad, 1);
        let world = &mut w.session_mut().host.world;
        if world.active_scene_label != legaia_game_tables::new_game::OPENING_SCENE {
            continue;
        }
        if !in_town {
            in_town = true;
            if retail {
                world.toggles.pin_retail();
            }
            continue;
        }
        let world = &w.session().host.world;
        let Some(slot) = world.player_actor_slot.map(usize::from) else {
            continue;
        };
        let Some(live) = world.actors[slot].pose_frame.as_ref() else {
            continue;
        };
        let drawn = world.field_draw_pose_frame(slot).unwrap_or(live);
        let clip = world
            .locomotion
            .player_anim
            .as_ref()
            .map_or(("none", 0, 0, 0), |a| a.playing_clip());
        rows.push_str(&format!(
            "{f};{};{};{};{};{:#x};{:?}\n",
            clip.0,
            clip.1,
            clip.2,
            clip.3,
            world.actors[slot].move_state.local_flags,
            drawn.bone_outputs
        ));
        poses.push(drawn.bone_outputs.clone());
    }
    assert!(
        w.session().host.world.name_entry_active(),
        "the opening never reached the name entry"
    );
    (poses, rows)
}

fn assert_no_pop(disc: &std::path::Path, retail: bool) {
    let (poses, rows) = walk_out(disc, retail);
    if let Some(out) = std::env::var_os("LEGAIA_HOH_OUT") {
        let mut p = std::path::PathBuf::from(out);
        p.set_extension(if retail { "retail.txt" } else { "default.txt" });
        std::fs::write(p, &rows).expect("write the rows");
    }
    assert!(
        poses.len() > 200,
        "the walk-out drew {} frames",
        poses.len()
    );
    let pops: Vec<(usize, i32, i32)> = poses
        .windows(3)
        .enumerate()
        .filter_map(|(i, w)| {
            let (out, back, across) = (
                pose_step(&w[0], &w[1]),
                pose_step(&w[1], &w[2]),
                pose_step(&w[0], &w[2]),
            );
            (out >= 512 && back >= 512 && across * 2 < out.min(back)).then_some((
                i + 1,
                out,
                across,
            ))
        })
        .collect();
    assert!(
        pops.is_empty(),
        "retail={retail}: one-vsync pose pops (frame, step out, step across): {pops:?}"
    );
}

#[test]
fn the_hand_on_head_beat_draws_no_one_vsync_pop() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let disc = std::path::Path::new(&disc);
    assert_no_pop(disc, false);
    assert_no_pop(disc, true);
}

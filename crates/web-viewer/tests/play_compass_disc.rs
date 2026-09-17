//! The browser play page's movement compass tracks the camera the page draws.
//!
//! Retail remaps the held d-pad by the camera azimuth (`func_0x800467e8`),
//! so "up" always walks away from the camera - turn the camera and the
//! controls turn with it. The page's drag-orbit swings
//! `Camera::manual_orbit`, the engine camera publishes
//! `compass_azimuth_units`, and the follow view renders the negated yaw; the
//! three signs have to agree or the controls come out mirrored at a quarter
//! turn while every camera-only oracle stays green.
//!
//! This drives the page's own object ([`LegaiaRuntime`]) through a real
//! scene: for four orbits, hold Up and Right, and require that the player's
//! world displacement runs along the axis the page's own view-projection
//! makes screen-up / screen-right for that frame. The ground truth is the
//! projected frame, not any constant a host could spell wrong the same way
//! twice.
//!
//! **Why the law is an angle and not a pixel ratio.** The remap is rung, not
//! continuous: the held mask is rotated by whole eighth-turns
//! (`World::remap_pad_direction`, retail's `func_0x800467e8`), so under a
//! camera whose yaw is not a multiple of 45 degrees the walk is off the
//! camera's screen-up by up to a half-step. `town01`'s zone camera sits about
//! 15 degrees off the ring, and that residual reaches the screen multiplied by
//! the camera's pitch: screen-X carries the sine of the offset while screen-Y
//! carries its cosine times the cosine of the pitch, so one fixed heading
//! error prints a larger pixel ratio the steeper the camera looks down. A
//! `|dx| < k*|dy|` bound therefore measures the framing, not the controls - it
//! moves when the focus height moves, with the walk direction bit-identical.
//! So both tests below recover the camera's true screen axes off the
//! projection and compare **headings**.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::input::PadButton;
use legaia_web_viewer::runtime::LegaiaRuntime;
use std::env;
use std::f32::consts::{FRAC_PI_2, PI};

const W: f32 = 960.0;
const H: f32 = 720.0;
const HOLD_FRAMES: usize = 12;

/// Project raw retail Y-down world `v` through the page's view-projection
/// into 320x240 stage pixels (Y down), exactly as `play_camera_parity_disc`
/// does.
fn stage(vp: &[f32], v: [f32; 3]) -> (f32, f32) {
    let p = [v[0], -v[1], v[2]];
    let cx = vp[0] * p[0] + vp[4] * p[1] + vp[8] * p[2] + vp[12];
    let cy = vp[1] * p[0] + vp[5] * p[1] + vp[9] * p[2] + vp[13];
    let cw = vp[3] * p[0] + vp[7] * p[1] + vp[11] * p[2] + vp[15];
    assert!(cw > 1.0, "point behind the eye");
    (160.0 * (1.0 + cx / cw), 120.0 * (1.0 - cy / cw))
}

/// The world-XZ heading (radians, `atan2(x, z)`) whose projection through
/// `vp` from `p` climbs the screen fastest - the camera's true screen-up
/// axis, read off the matrix the page actually draws with rather than off any
/// constant. `right = true` returns the screen-**right** axis instead.
fn screen_axis(vp: &[f32], p: [f32; 3], right: bool) -> f32 {
    let origin = stage(vp, p);
    let mut best = (0.0f32, f32::INFINITY);
    for tenth in 0..3600 {
        let a = (tenth as f32 / 10.0).to_radians();
        let q = stage(vp, [p[0] + 64.0 * a.sin(), p[1], p[2] + 64.0 * a.cos()]);
        // Screen Y is down, so "up" is the most negative dy; "right" is the
        // most positive dx, i.e. the most negative of its negation.
        let score = if right {
            -(q.0 - origin.0)
        } else {
            q.1 - origin.1
        };
        if score < best.1 {
            best = (a, score);
        }
    }
    best.0
}

/// Smallest angle between two headings, in degrees.
fn angle_off(a: f32, b: f32) -> f32 {
    (a - b).sin().atan2((a - b).cos()).abs().to_degrees()
}

/// The largest angle a correct rung remap can leave between the walk and the
/// camera's own screen axis: half an eighth-turn, plus slack for the sweep's
/// own 0.1-degree grid and the finite-displacement measurement.
const RING_HALF_STEP_DEG: f32 = 22.5 + 1.5;

/// One measured hold: press `button` for [`HOLD_FRAMES`] frames and report
/// the player's stage displacement `(dx, dy)`, the raw world delta, and the
/// camera's screen-up / screen-right world headings - all four read off the
/// **same** frame, the one the hold begins on. The follow camera eases as the
/// player moves, so an axis measured before an earlier hold is a different
/// camera, and comparing across the two measures the easing rather than the
/// controls.
struct Hold {
    stage: (f32, f32),
    world: (f32, f32),
    up_axis: f32,
    right_axis: f32,
}

fn walk(rt: &mut LegaiaRuntime, button: PadButton) -> Hold {
    rt.set_pad(0);
    for _ in 0..2 {
        rt.tick_frame().expect("tick");
    }
    let p0 = rt.player_transform();
    let vp = rt.play_camera_vp(W, H);
    assert_eq!(vp.len(), 16, "the page must get a full view-projection");
    let up_axis = screen_axis(&vp, [p0[0], p0[1], p0[2]], false);
    let right_axis = screen_axis(&vp, [p0[0], p0[1], p0[2]], true);
    rt.set_pad(button.mask());
    for _ in 0..HOLD_FRAMES {
        rt.tick_frame().expect("tick");
    }
    rt.set_pad(0);
    rt.tick_frame().expect("tick");
    let p1 = rt.player_transform();
    let world = (p1[0] - p0[0], p1[2] - p0[2]);
    assert!(
        world != (0.0, 0.0),
        "{button:?} held {HOLD_FRAMES} frames moved the player nowhere (blocked?)"
    );
    // Same height for both points: the question is the XZ heading only.
    let s0 = stage(&vp, [p0[0], p0[1], p0[2]]);
    let s1 = stage(&vp, [p1[0], p0[1], p1[2]]);
    Hold {
        stage: (s1.0 - s0.0, s1.1 - s0.1),
        world,
        up_axis,
        right_axis,
    }
}

#[test]
fn up_walks_away_from_the_camera_and_right_walks_screen_right_at_every_orbit() {
    let Ok(disc) = env::var("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc image");
    for orbit in [0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2] {
        let mut rt = LegaiaRuntime::new();
        rt.load_disc(bytes.clone(), String::new())
            .expect("load_disc");
        rt.enter_field("town01").expect("enter_field(town01)");
        for _ in 0..8 {
            rt.tick_frame().expect("tick");
        }
        rt.play_camera_set_orbit(orbit);
        assert!((rt.play_camera_orbit() - orbit).abs() < 1e-6);

        let h = walk(&mut rt, PadButton::Up);
        let (dx, dy) = h.stage;
        let off = angle_off(h.world.0.atan2(h.world.1), h.up_axis);
        assert!(
            dy < 0.0 && off <= RING_HALF_STEP_DEG,
            "orbit {orbit:.3}: Up walked {off:.1} deg off the camera's screen-up \
             (stage delta ({dx:.1}, {dy:.1}) px, world delta {:?})",
            h.world
        );
        let h = walk(&mut rt, PadButton::Right);
        let (dx, dy) = h.stage;
        let off = angle_off(h.world.0.atan2(h.world.1), h.right_axis);
        assert!(
            dx > 0.0 && off <= RING_HALF_STEP_DEG,
            "orbit {orbit:.3}: Right walked {off:.1} deg off the camera's screen-right \
             (stage delta ({dx:.1}, {dy:.1}) px, world delta {:?})",
            h.world
        );
    }
}

/// The same law through the flow a player actually takes: the New Game
/// opening enters `town01` under its establishing timeline (a scripted shot
/// whose yaw folds into the engine camera), the name-entry beat commits, the
/// timeline hands the controls back - and from that frame on, Up must walk
/// the compass's screen-up axis on the page's matrix at orbit 0 **and** after
/// a quarter-turn drag. A cinematic yaw that leaked into the compass but not
/// into the follow view would pass the `enter_field` oracle above and fail
/// here.
///
/// The hand-back seat sits under a zone camera that is **not** axis-aligned
/// (a look-at record; the follow yaw the engine composes there is ~130
/// degrees), so the law is measured as the remap states it: the held d-pad is
/// rotated by whole eighth-turns of retail's compass ring
/// (`func_0x800467e8`), so Up walks the ring direction nearest the camera's
/// screen-up - within a half-step of it - and still moves up the screen.
#[test]
fn the_compass_is_right_from_the_first_free_roam_frame_after_the_opening() {
    let Ok(disc) = env::var("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc image");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    rt.debug_enter_town01_opening()
        .expect("enter town01 as the new-game opening");
    let mut ticks = 0;
    while !rt.name_entry_is_active() && ticks < 8000 {
        rt.tick_frame().expect("tick");
        ticks += 1;
    }
    assert!(
        rt.name_entry_is_active(),
        "name entry never opened ({ticks} ticks)"
    );
    // Select -> confirm (opens on No) -> Up to Yes -> commit.
    rt.name_entry_input(PadButton::Cross.mask());
    rt.name_entry_input(PadButton::Up.mask());
    assert!(rt.name_entry_input(PadButton::Cross.mask()), "commit");
    let mut ticks = 0;
    while rt.debug_timeline_active() && ticks < 12000 {
        rt.set_pad(if ticks % 4 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        });
        rt.tick_frame().expect("tick");
        ticks += 1;
    }
    rt.set_pad(0);
    assert!(
        !rt.debug_timeline_active(),
        "the opening never handed back control"
    );

    for orbit in [0.0, FRAC_PI_2] {
        rt.play_camera_set_orbit(orbit);
        // The camera's true screen-up in world XZ: the ground direction whose
        // projection climbs the screen fastest, measured off the page's own
        // matrix rather than any constant.
        let h = walk(&mut rt, PadButton::Up);
        let (dx, dy) = h.stage;
        let off = angle_off(h.world.0.atan2(h.world.1), h.up_axis);
        assert!(
            dy < 0.0 && off <= RING_HALF_STEP_DEG,
            "after the opening, orbit {orbit:.3}: Up walked {off:.1} deg off the camera's \
             screen-up (stage delta ({dx:.1}, {dy:.1}) px, world delta {:?}); view {}",
            h.world,
            rt.play_camera_view_json()
        );
    }
}

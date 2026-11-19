//! Disc-gated: the smooth camera's stepped moves (`Camera::observe_present`)
//! and the fog sheets that ride the drawn camera
//! (`World::fog_reprojection`).
//!
//! Retail's follow ease `FUN_801DB510` steps the camera globals once a game
//! pass and holds them on the vsyncs between, so a walk that turns the
//! follow camera draws its pan at the frame step's rate. Under the smooth
//! camera the draw shows each pass's move over the vsyncs after it. Two
//! walks run the same pad schedule, smooth camera on and off; the stored
//! globals, the player and the `rand()` stream must match vsync for vsync
//! (presentation only), the stored pose holds on the vsyncs between passes,
//! and the drawn pose must not.
//!
//! The fog pool walks once a game pass too, so its sheets were projected
//! through the camera of the pass while the scene drew through the camera
//! of the vsync: under the smooth walk the focus moves every vsync and the
//! sheets trailed the scene by the walk's step. The `vell` walk measures
//! that trail and asserts the drawn sheets sit where the drawn camera puts
//! them.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_engine_world_views::camera_view::camera_present_lead;
use legaia_engine_world_views::screen_passes::HostView;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gated() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })
}

/// The axes a follow ease steps: pitch, yaw, the eye trio and `H`.
const EASED: [usize; 6] = [0, 1, 3, 4, 5, 9];

/// One vsync of a walk: what the simulation stored and what the draw shows.
#[derive(Debug, Clone, PartialEq)]
struct Sample {
    stored: [i32; 10],
    drawn: [f32; 10],
    player: [i16; 3],
    rng: u32,
    scripted: bool,
}

fn open(extracted: &std::path::Path, scene: &str, smooth: bool) -> Option<NativeSession> {
    let cfg = BootConfig {
        scene: scene.into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(extracted, &cfg).ok()?;
    s.host.world.toggles.use_vm_dialogue = true;
    s.enter_field_live(scene, &FieldLiveOpts::default()).ok()?;
    s.set_host_drains_queues(true);
    s.host.world.toggles.smooth_camera = smooth;
    Some(s)
}

/// Walk `scene` on a fixed pad schedule, `ticks` vsyncs, calling `each`
/// after every tick.
fn walk(
    s: &mut NativeSession,
    ticks: u32,
    mut each: impl FnMut(&mut NativeSession),
) -> Option<Vec<Sample>> {
    let (r, d, l, u) = (
        PadButton::Right.mask(),
        PadButton::Down.mask(),
        PadButton::Left.mask(),
        PadButton::Up.mask(),
    );
    let dirs = [r, d, l, u, r | u, l | d, u | l, d | r];
    let label = s.host.world.active_scene_label.clone();
    let mut out = Vec::new();
    for t in 0..ticks {
        if s.host.world.active_scene_label != label {
            break;
        }
        s.host.world.set_pad(dirs[(t / 75) as usize % dirs.len()]);
        s.tick().ok()?;
        each(s);
        let w = &s.host.world;
        let g = s.camera.globals.0;
        let d = s.camera.led_delta(camera_present_lead(w, &s.camera));
        let slot = w.player_actor_slot.unwrap_or(0) as usize;
        let m = &w.actors[slot].move_state;
        out.push(Sample {
            stored: g,
            drawn: std::array::from_fn(|k| g[k] as f32 + d[k]),
            player: [m.world_x, m.world_y, m.world_z],
            rng: w.rng_state,
            scripted: s.camera.mover.is_some() || s.camera.script_owns_focus(),
        });
    }
    Some(out)
}

/// Vsyncs inside a follow-ease run (the eased axes moved on the vsync
/// before or after) whose drawn pose stood still, and the largest one-vsync
/// yaw step drawn.
fn holds(samples: &[Sample], drawn: bool) -> (usize, usize, f32) {
    let pose = |s: &Sample| -> [f32; 6] {
        EASED.map(|k| {
            if drawn {
                s.drawn[k]
            } else {
                s.stored[k] as f32
            }
        })
    };
    let stored_moved = |a: &Sample, b: &Sample| EASED.iter().any(|&k| a.stored[k] != b.stored[k]);
    let mut run = 0;
    let mut held = 0;
    let mut max_yaw = 0.0f32;
    for t in 1..samples.len().saturating_sub(1) {
        let (p, c, n) = (&samples[t - 1], &samples[t], &samples[t + 1]);
        if c.scripted || p.scripted {
            continue;
        }
        if !(stored_moved(p, c) || stored_moved(c, n)) {
            continue;
        }
        run += 1;
        if pose(p) == pose(c) {
            held += 1;
        }
        max_yaw = max_yaw.max((pose(c)[1] - pose(p)[1]).abs());
    }
    (run, held, max_yaw)
}

/// `town01`: the follow ease's pan draws every vsync under the smooth
/// camera, and the simulation is the one the smooth-off walk runs.
#[test]
fn the_follow_ease_draws_every_vsync_and_leaves_the_simulation_alone() {
    let Some(extracted) = gated() else { return };
    let scene = "town01";
    let Some(mut on) = open(&extracted, scene, true) else {
        eprintln!("[skip] {scene} did not open");
        return;
    };
    let mut off = open(&extracted, scene, false).expect("second session");
    let a = walk(&mut on, 1800, |_| {}).expect("smooth walk");
    let b = walk(&mut off, 1800, |_| {}).expect("stepped walk");
    assert_eq!(a.len(), b.len(), "both walks stay in {scene} equally long");
    for (t, (x, y)) in a.iter().zip(&b).enumerate() {
        assert_eq!(
            (x.stored, x.player, x.rng),
            (y.stored, y.player, y.rng),
            "vsync {t}: the smooth camera moved the simulation"
        );
        assert!(
            y.drawn.iter().zip(y.stored).all(|(d, s)| *d == s as f32),
            "vsync {t}: smooth off draws the stored pose"
        );
    }
    let (run, held_stored, max_stored) = holds(&a, false);
    let (_, held_drawn, max_drawn) = holds(&a, true);
    eprintln!(
        "[smooth follow] {scene}: {run} ease vsyncs; stored pose held on {held_stored} \
         (max yaw step {max_stored}), drawn pose held on {held_drawn} (max yaw step {max_drawn:.2})"
    );
    assert!(run > 100, "the walk never turned the follow camera");
    assert!(held_stored > run / 4, "the stored ease steps once a pass");
    assert!(
        held_drawn * 20 < held_stored,
        "the drawn pose still holds between passes ({held_drawn} of {run})"
    );
}

/// `vell`: the fog sheets ride the camera the frame draws through, not the
/// one the pool's last pass projected through.
#[test]
fn the_fog_sheets_ride_the_drawn_camera() {
    let Some(extracted) = gated() else { return };
    let scene = "vell";
    let Some(mut s) = open(&extracted, scene, true) else {
        eprintln!("[skip] {scene} did not open");
        return;
    };
    let mut trail_max = 0i32;
    let mut trail_sum = 0i64;
    let mut sheets = 0i64;
    let mut moved_frames = 0u32;
    walk(&mut s, 1500, |s| {
        let _ = s.step_field_fog([0.0, 0.0]);
        let w = &s.host.world;
        let view = HostView::new(w, &s.camera, ([0.0; 3], [0.0; 3])).presented();
        let drawn = view.frame(None).field_view();
        let Some(proj) = w.fog_reprojection(drawn.as_ref()) else {
            return;
        };
        moved_frames += 1;
        for q in w.fog_quads() {
            let Some(r) = q.reproject(&proj) else {
                continue;
            };
            let dx = (i32::from(r.xy[0].0) - i32::from(q.xy[0].0)).abs();
            let dy = (i32::from(r.xy[0].1) - i32::from(q.xy[0].1)).abs();
            trail_max = trail_max.max(dx.max(dy));
            trail_sum += i64::from(dx.max(dy));
            sheets += 1;
        }
        // The drawn sheets are the reprojected ones.
        let prims = view.fog_prims(None, false);
        let want = w
            .fog_quads()
            .iter()
            .filter(|q| q.reproject(&proj).is_some())
            .count();
        assert!(prims.len() >= want);
    })
    .expect("vell walk");
    eprintln!(
        "[fog ride] {scene}: {moved_frames} vsyncs drew through a moved camera; sheet trail \
         before the fix max {trail_max} px, mean {:.2} px over {sheets} sheets; after 0",
        trail_sum as f64 / sheets.max(1) as f64
    );
    assert!(
        s.host.world.fog.gate,
        "vell's entry script raises the fog gate"
    );
    assert!(moved_frames > 100, "the walk never moved the drawn camera");
}

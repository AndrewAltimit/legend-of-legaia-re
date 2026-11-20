//! Disc-gated: the smooth field actors (`WorldToggles::smooth_actors`,
//! `World::observe_field_present`).
//!
//! Retail steps a scripted walker once a game frame (the actor-list walk
//! `FUN_8002519C`), so `izumi`'s first-visit choreography - the hunters
//! walking their rounds about the spring, Vahn walked to his mark - moves
//! each body a whole step on the vsync a frame opens and holds it on the
//! two or three between. Two runs of the intro on one pad schedule, smooth
//! actors on and off: the simulation must match vsync for vsync
//! (presentation only), off must draw the stored bodies, and on the drawn
//! walkers must move on the vsyncs the stored ones hold.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_engine_world_views::camera_view::cutscene_owns_camera;
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

/// One vsync of the intro: what the simulation stored and what the draw
/// shows, per body.
struct Sample {
    sim: String,
    /// `(key, stored [x, y, z], drawn [x, y, z])`: placement slots, and the
    /// player under key `256`.
    bodies: Vec<(u16, [f32; 3], [f32; 3])>,
    /// The drawn player's screen point through the drawn follow camera,
    /// `None` while a scripted shot owns the frame.
    screen: Option<[f32; 2]>,
    /// `(slot, stored clip cursor, drawn clip cursor)` per world-driven NPC
    /// clip, in 1/16-frame units.
    clips: Vec<(u8, i32, i32)>,
}

fn open(extracted: &std::path::Path, smooth: bool) -> Option<NativeSession> {
    open_scene(extracted, "izumi", smooth)
}

fn open_scene(extracted: &std::path::Path, scene: &str, smooth: bool) -> Option<NativeSession> {
    let cfg = BootConfig {
        scene: scene.into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(extracted, &cfg).ok()?;
    s.host.world.toggles.use_vm_dialogue = true;
    s.enter_field_live(scene, &FieldLiveOpts::default()).ok()?;
    s.set_host_drains_queues(true);
    s.host.world.toggles.smooth_actors = smooth;
    Some(s)
}

fn run(s: &mut NativeSession, ticks: u32) -> Vec<Sample> {
    let mut out = Vec::new();
    for t in 0..ticks {
        // Page the boxes the choreography opens.
        let pad = if t % 40 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        let w = &s.host.world;
        let mut npcs: Vec<_> = w.npcs.positions.iter().map(|(&k, &v)| (k, v)).collect();
        npcs.sort_unstable();
        let mut headings: Vec<_> = w.npcs.headings.iter().map(|(&k, &v)| (k, v)).collect();
        headings.sort_unstable();
        let actors: Vec<_> = w
            .actors
            .iter()
            .filter(|a| a.active)
            .map(|a| {
                let m = &a.move_state;
                (
                    m.world_x,
                    m.world_y,
                    m.world_z,
                    m.render_26,
                    a.pose_frame.as_ref().map(|p| p.bone_outputs.clone()),
                )
            })
            .collect();
        let sim = format!(
            "{} {:08x} {:?} {:?} {:?} {:?} {}",
            w.active_scene_label,
            w.rng_state,
            npcs,
            headings,
            actors,
            s.camera.globals.0,
            w.field_pc
        );
        let mut bodies = Vec::new();
        for &(slot, (x, z)) in &npcs {
            let Some(stored) = w.field_npc_draw_pose(slot, (x, z)) else {
                continue;
            };
            let drawn = w.field_npc_drawn_pose(slot, (x, z)).expect("drawn");
            bodies.push((u16::from(slot), stored.pos, drawn.pos));
        }
        let mut screen = None;
        if let Some(slot) = w.player_actor_slot.map(usize::from) {
            let m = &w.actors[slot].move_state;
            let stored = [m.world_x, m.world_y, m.world_z].map(f32::from);
            let (drawn, _) = w.field_draw_root(slot).expect("player");
            bodies.push((256, stored, drawn));
            if !cutscene_owns_camera(w)
                && let Some(v) = HostView::new(w, &s.camera, ([0.0; 3], [0.0; 3]))
                    .presented()
                    .frame(None)
                    .field_view()
            {
                let e = v.eye_space(drawn);
                if e[2] > 1.0 {
                    screen = Some([v.h * e[0] / e[2], v.h * e[1] / e[2]]);
                }
            }
        }
        let clips = w
            .npcs
            .clip_cursors
            .iter()
            .map(|(&slot, c)| {
                let stored = i32::from(c.cursor);
                let drawn = w
                    .field_npc_drawn_clip_cursor(slot)
                    .map_or(stored, |d| d as i32);
                (slot, stored, drawn)
            })
            .collect();
        out.push(Sample {
            sim,
            bodies,
            screen,
            clips,
        });
    }
    out
}

/// Per body over the run: vsyncs inside a walk (the stored body moved
/// within the last and the next four vsyncs) and how many of those the
/// stored and the drawn body stood still on, plus the largest one-vsync
/// step of each.
fn holds(samples: &[Sample]) -> (usize, usize, usize, f32, f32) {
    let at = |t: usize, key: u16| {
        samples[t]
            .bodies
            .iter()
            .find(|b| b.0 == key)
            .map(|b| (b.1, b.2))
    };
    let dist = |a: [f32; 3], b: [f32; 3]| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    let (mut walk, mut held_s, mut held_d, mut max_s, mut max_d) = (0, 0, 0, 0.0f32, 0.0f32);
    for (t, sample) in samples
        .iter()
        .enumerate()
        .take(samples.len().saturating_sub(4))
        .skip(4)
    {
        for b in &sample.bodies {
            let key = b.0;
            let (Some((s0, _)), Some((s1, _)), Some((sp, dp))) =
                (at(t - 4, key), at(t + 4, key), at(t - 1, key))
            else {
                continue;
            };
            let (s, d) = (b.1, b.2);
            // A steady walk: moving across the window on both sides, and
            // no step in it a placement.
            if s0 == s || s == s1 || dist(s0, s1) > 128.0 {
                continue;
            }
            walk += 1;
            held_s += usize::from(s == sp);
            held_d += usize::from(d == dp);
            max_s = max_s.max(dist(s, sp));
            max_d = max_d.max(dist(d, dp));
        }
    }
    (walk, held_s, held_d, max_s, max_d)
}

/// Over the vsyncs the stored player walks under the follow camera: the
/// largest one-vsync step of its drawn screen point, and the count of
/// vsyncs that step was over a pixel.
fn player_screen_shake(samples: &[Sample]) -> (usize, usize, f32) {
    let stored = |t: usize| samples[t].bodies.iter().find(|b| b.0 == 256).map(|b| b.1);
    let (mut n, mut over, mut max) = (0, 0, 0.0f32);
    for t in 1..samples.len() {
        let (Some(a), Some(b)) = (samples[t - 1].screen, samples[t].screen) else {
            continue;
        };
        if stored(t - 1) == stored(t) && stored(t).is_some() && t >= 4 && stored(t - 4) == stored(t)
        {
            continue;
        }
        n += 1;
        let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        over += usize::from(d > 1.0);
        max = max.max(d);
    }
    (n, over, max)
}

/// Over the vsyncs a world-driven NPC clip runs on a walking NPC (its
/// stored cursor and its body both moved within the last and the next four
/// vsyncs): how many the stored and the drawn cursor stood still on.
fn clip_holds(samples: &[Sample]) -> (usize, usize, usize) {
    let at = |t: usize, slot: u8| {
        samples[t]
            .clips
            .iter()
            .find(|c| c.0 == slot)
            .map(|c| (c.1, c.2))
    };
    let (mut run, mut held_s, mut held_d) = (0, 0, 0);
    for (t, sample) in samples
        .iter()
        .enumerate()
        .take(samples.len().saturating_sub(4))
        .skip(4)
    {
        for &(slot, s, d) in &sample.clips {
            let (Some((s0, _)), Some((s1, _)), Some((sp, dp))) =
                (at(t - 4, slot), at(t + 4, slot), at(t - 1, slot))
            else {
                continue;
            };
            if s0 == s || s == s1 {
                continue;
            }
            // A walker: its body moves across the window too.
            let body = |t: usize| {
                samples[t]
                    .bodies
                    .iter()
                    .find(|b| b.0 == u16::from(slot))
                    .map(|b| b.1)
            };
            if body(t - 4) == body(t) || body(t) == body(t + 4) {
                continue;
            }
            run += 1;
            held_s += usize::from(s == sp);
            held_d += usize::from(d == dp);
        }
    }
    (run, held_s, held_d)
}

#[test]
fn izumi_intro_walkers_draw_every_vsync_and_the_simulation_is_untouched() {
    let Some(extracted) = gated() else { return };
    let Some(mut on) = open(&extracted, true) else {
        eprintln!("[skip] izumi did not open");
        return;
    };
    let mut off = open(&extracted, false).expect("second session");
    let a = run(&mut on, 1500);
    let b = run(&mut off, 1500);
    for (t, (x, y)) in a.iter().zip(&b).enumerate() {
        assert_eq!(
            x.sim, y.sim,
            "vsync {t}: the smooth actors moved the simulation"
        );
        for c in &y.clips {
            assert_eq!(
                c.1, c.2,
                "vsync {t}: off draws clip {} at its playhead",
                c.0
            );
        }
        for body in &y.bodies {
            assert_eq!(
                body.1, body.2,
                "vsync {t}: off draws body {} stored",
                body.0
            );
        }
    }
    let (walk, held_stored, _, max_stored, _) = holds(&b);
    let (_, _, held_drawn, _, max_drawn) = holds(&a);
    eprintln!(
        "[smooth actors] izumi intro: {walk} walking body-vsyncs; stored held on {held_stored} \
         (max step {max_stored:.1}), drawn held on {held_drawn} (max step {max_drawn:.1})"
    );
    let (n, over_off, max_off) = player_screen_shake(&b);
    let (_, over_on, max_on) = player_screen_shake(&a);
    eprintln!(
        "[smooth actors] izumi intro: the player's screen point over {n} walking follow vsyncs \
         moved over a pixel on {over_off} (max {max_off:.1} px) stored, {over_on} \
         (max {max_on:.1} px) drawn"
    );
    assert!(
        max_on <= max_off + 1.0,
        "the follow focus left the drawn player behind"
    );
    let (run, clip_held_stored, _) = clip_holds(&b);
    let (_, _, clip_held_drawn) = clip_holds(&a);
    eprintln!(
        "[smooth actors] izumi intro: {run} walking NPC clip-vsyncs; stored cursor held on \
         {clip_held_stored}, drawn on {clip_held_drawn}"
    );
    assert!(run > 100, "the intro walked no NPC clip ({run})");
    assert!(
        clip_held_drawn * 4 < clip_held_stored,
        "the drawn clips still hold between frames ({clip_held_drawn} of {run})"
    );
    assert!(walk > 200, "the intro never walked anyone ({walk})");
    assert!(
        held_stored * 2 > walk,
        "the stored walkers step once a frame"
    );
    assert!(
        held_drawn * 10 < held_stored,
        "the drawn walkers still hold between frames ({held_drawn} of {walk})"
    );
    assert!(
        max_drawn < max_stored,
        "the drawn step is a share of the frame's"
    );
}

/// A camera glide draws a vsync **ahead** (`Camera::led_delta`, the pose
/// the next frame stores) while the bodies draw a frame **behind**: over the
/// opening's (`opdeene`) vsyncs where a glide is in flight under a scripted shot,
/// project each walking body through the frame's camera and measure how
/// unevenly its screen point moves (the mean change of its one-vsync step),
/// stored (both stepped) against drawn (camera led, body behind).
#[test]
fn a_glide_over_walkers_moves_them_evenly_on_screen() {
    let Some(extracted) = gated() else { return };
    let scenes = std::env::var("W50B_SCENES").unwrap_or_else(|_| "opdeene".into());
    for scene in scenes.split(',') {
        glide_over_walkers(&extracted, scene);
    }
}

/// A body's last two screen points, stored and drawn, and how many it has.
type ScreenTrail = ([[f32; 2]; 2], [[f32; 2]; 2], u32);

fn glide_over_walkers(extracted: &std::path::Path, scene: &str) {
    use legaia_engine_field::frame_step::{scripted_view, scripted_view_led};
    use legaia_engine_world_views::camera_view::camera_present_lead;
    let Some(mut s) = open_scene(extracted, scene, true) else {
        eprintln!("[skip] {scene} did not open");
        return;
    };
    // Per body: the last two screen points, stored and drawn.
    let mut last: std::collections::BTreeMap<u16, ScreenTrail> = Default::default();
    let (mut n, mut jit_s, mut jit_d, mut glide_vsyncs) = (0u32, 0.0f32, 0.0f32, 0u32);
    // How far the drawn screen point sits off the stored one on a walking
    // body: the glide's lead and the body's lag together.
    let (mut off_sum, mut off_max) = (0.0f32, 0.0f32);
    for t in 0..1500u32 {
        let pad = if t % 40 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        let w = &s.host.world;
        if s.camera.mover.is_none() || !cutscene_owns_camera(w) {
            last.clear();
            continue;
        }
        glide_vsyncs += 1;
        let hv = HostView::new(w, &s.camera, ([0.0; 3], [0.0; 3]));
        let target = hv.scripted_target();
        let stored_cam = scripted_view(&s.camera, target);
        let drawn_cam = scripted_view_led(&s.camera, target, camera_present_lead(w, &s.camera));
        let project = |v: &legaia_engine_camera::psx_camera::FieldCameraView, p: [f32; 3]| {
            let e = v.eye_space(p);
            (e[2] > 1.0).then(|| [v.h * e[0] / e[2], v.h * e[1] / e[2]])
        };
        for (&slot, &(x, z)) in &w.npcs.positions {
            let (Some(sp), Some(dp)) = (
                w.field_npc_draw_pose(slot, (x, z)),
                w.field_npc_drawn_pose(slot, (x, z)),
            ) else {
                continue;
            };
            let (Some(a), Some(b)) = (project(&stored_cam, sp.pos), project(&drawn_cam, dp.pos))
            else {
                continue;
            };
            let key = u16::from(slot);
            let e = last.entry(key).or_insert(([a; 2], [b; 2], 0));
            let step = |h: &[[f32; 2]; 2], c: [f32; 2]| {
                let s1 = [h[1][0] - h[0][0], h[1][1] - h[0][1]];
                let s2 = [c[0] - h[1][0], c[1] - h[1][1]];
                ((s2[0] - s1[0]).powi(2) + (s2[1] - s1[1]).powi(2)).sqrt()
            };
            // A walking body only (its stored root moved this vsync or the
            // last two).
            if e.2 >= 2 && sp.pos != dp.pos || e.2 >= 2 && e.0[1] != a {
                n += 1;
                jit_s += step(&e.0, a);
                jit_d += step(&e.1, b);
                let off = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
                off_sum += off;
                off_max = off_max.max(off);
            }
            e.0 = [e.0[1], a];
            e.1 = [e.1[1], b];
            e.2 += 1;
        }
    }
    let (ms, md) = (jit_s / n.max(1) as f32, jit_d / n.max(1) as f32);
    eprintln!(
        "[smooth actors] {scene}: {glide_vsyncs} glide vsyncs, {n} body-vsyncs; mean \
         screen-step change stored {ms:.2} px, drawn {md:.2} px; drawn off stored by mean \
         {:.2} px, max {off_max:.1} px",
        off_sum / n.max(1) as f32
    );
    if n > 0 {
        assert!(
            md <= ms,
            "the led glide and the lagged bodies move unevenly"
        );
    }
}

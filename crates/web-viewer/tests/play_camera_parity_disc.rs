//! The browser play page frames the field with the **engine's** camera, not a
//! camera of its own.
//!
//! The page used to run a spherical orbit projection (`buildWorldOrbitVp` -
//! yaw, pitch, a half-window) beside the engine, and re-mapped the op-`0x45`
//! cutscene shots onto it. That divergence is invisible to every host-drift
//! tier: both hosts draw the same meshes with the same shaders, and no file
//! holds two of the columns. Only a frame from each host, at the same world
//! tick, can compare them.
//!
//! This drives the page's own object ([`LegaiaRuntime`], the one
//! `site/js/play-app.js` constructs) into a real scene and asserts that what
//! its camera exports hand back is the retail model the native window frames
//! with: the pinned follow angles, the player anchor, the GTE `H`, and a
//! view-projection that *is* [`legaia_engine_vm::psx_camera::psx_camera_vp`]
//! of those inputs.
//!
//! Non-vacuity is the point of the last rung: a test that only asserted "the
//! matrix is finite and the player is on screen" passes on an orbit camera
//! too. So the page's matrix is also compared against the orbit projection the
//! page used to build for the same frame, and is required to **differ** - the
//! test measures which camera model is live, not merely that one is.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of the
//! disc-dependent suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::camera_view::{FIELD_FOLLOW_YAW_UNITS, FIELD_H, FIELD_PITCH_UNITS};
use legaia_engine_vm::psx_camera;
use legaia_web_viewer::runtime::LegaiaRuntime;
use std::env;

const W: f32 = 960.0;
const H: f32 = 720.0;

fn to_rad(units: f32) -> f32 {
    units / 4096.0 * std::f32::consts::TAU
}

/// Project `v` (raw retail Y-down world) through a page view-projection,
/// into 320x240 stage pixels. The page's model matrices carry the Y flip the
/// matrix cancels, so the vertex goes in Y-up.
fn project(vp: &[f32], v: [f32; 3]) -> Option<(f32, f32)> {
    let p = [v[0], -v[1], v[2]];
    let cx = vp[0] * p[0] + vp[4] * p[1] + vp[8] * p[2] + vp[12];
    let cy = vp[1] * p[0] + vp[5] * p[1] + vp[9] * p[2] + vp[13];
    let cw = vp[3] * p[0] + vp[7] * p[1] + vp[11] * p[2] + vp[15];
    if cw <= 1.0 {
        return None;
    }
    Some((160.0 * (1.0 + cx / cw), 120.0 * (1.0 - cy / cw)))
}

/// The orbit projection the page used to build for its follow camera: eye on
/// a sphere around the target at `halfHeight / tan(FOV/2)`, a 52-degree
/// perspective, screen X mirrored. Re-stated here as the CONTRAST, so the
/// assertions below cannot be satisfied by an orbit camera that happens to
/// look about right.
fn legacy_orbit_vp(target: [f32; 3], yaw: f32, pitch: f32, half: f32) -> [f32; 16] {
    const FOV_Y: f32 = 0.9;
    let aspect = W / H;
    let (mut hw, mut hh) = (half, half);
    if hw / hh < aspect {
        hw = hh * aspect;
    } else {
        hh = hw / aspect;
    }
    let dist = (hh / (FOV_Y / 2.0).tan()).max(1.0);
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let eye = [
        target[0] + dist * sp * sy,
        target[1] + dist * cp,
        target[2] - dist * sp * cy,
    ];
    let up = [-cp * sy, sp, cp * cy];
    let view = psx_camera::look_at_rh(eye, target, up);
    let near = (dist / 500.0).max(1.0);
    let far = dist + 16384.0 * 10.0 + 4096.0;
    let mut proj = psx_camera::perspective_rh(FOV_Y, aspect, near, far);
    proj[0] = -proj[0];
    let _ = hw;
    psx_camera::mat4_mul(&proj, &view)
}

#[test]
fn the_play_page_frames_the_field_with_the_engine_camera() {
    let Ok(disc) = env::var("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc image");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    rt.enter_field("town01").expect("enter_field(town01)");
    // A handful of ticks so the camera controller has published a frame and
    // the locomotion compass has been fed at least once.
    for _ in 0..8 {
        rt.tick_frame().expect("tick");
    }

    // ---- rung 1: the page reports the retail FOLLOW arm, not an orbit.
    let json = rt.play_camera_view_json();
    let v: serde_json::Value = serde_json::from_str(&json).expect("camera view json");
    assert_eq!(
        v["arm"].as_str(),
        Some("follow"),
        "a free-roam field frame must resolve to the retail follow camera: {json}"
    );

    // ---- rung 2: its inputs are the native window's pinned constants.
    let f = |k: &str| v[k].as_f64().expect(k) as f32;
    assert!(
        (f("pitch") - to_rad(FIELD_PITCH_UNITS)).abs() < 1e-4,
        "pitch {} != pinned {}",
        f("pitch"),
        to_rad(FIELD_PITCH_UNITS)
    );
    assert!(
        (f("yaw") - to_rad(FIELD_FOLLOW_YAW_UNITS)).abs() < 1e-4,
        "yaw {} != pinned {}",
        f("yaw"),
        to_rad(FIELD_FOLLOW_YAW_UNITS)
    );
    assert!((f("h") - FIELD_H).abs() < 1e-4, "H {} != {FIELD_H}", f("h"));
    assert_eq!(f("roll"), 0.0, "the field follow camera never rolls");

    // ---- rung 3: the focus is the live player anchor (X/Z), so the camera
    // tracks the same actor the page draws.
    let pt = rt.player_transform();
    let focus: Vec<f32> = v["focus"]
        .as_array()
        .expect("focus")
        .iter()
        .map(|x| x.as_f64().unwrap() as f32)
        .collect();
    assert!(
        (focus[0] - pt[0]).abs() < 1.0 && (focus[2] - pt[2]).abs() < 1.0,
        "focus {focus:?} does not track the player at ({}, {})",
        pt[0],
        pt[2]
    );

    // ---- rung 4: the uploaded matrix IS the shared retail projection of
    // those inputs - not a re-derivation, and not an orbit fitted to them.
    let vp = rt.play_camera_vp(W, H);
    assert_eq!(vp.len(), 16, "the page must get a full view-projection");
    let tr: Vec<f32> = v["tr"]
        .as_array()
        .expect("tr")
        .iter()
        .map(|x| x.as_f64().unwrap() as f32)
        .collect();
    let want = psx_camera::psx_camera_vp(
        f("pitch"),
        f("yaw"),
        f("roll"),
        f("h"),
        [tr[0], tr[1], tr[2]],
        [focus[0], focus[1], focus[2]],
        W / H,
    );
    for (i, (a, b)) in vp.iter().zip(want.iter()).enumerate() {
        assert!(
            (a - b).abs() <= 1e-4 * b.abs().max(1.0),
            "vp[{i}] = {a} != {b} (the page's matrix is not the shared kernel's)"
        );
    }

    // ---- rung 5: the player is framed, and the lens is behind and above.
    let on_screen = project(&vp, [pt[0], pt[1], pt[2]]).expect("player in front of the eye");
    assert!(
        (0.0..=320.0).contains(&on_screen.0) && (0.0..=240.0).contains(&on_screen.1),
        "player projects off-frame at {on_screen:?}"
    );
    let eye = rt.play_camera_eye();
    assert_eq!(eye.len(), 3, "the occlusion gate needs a world-space lens");
    assert!(
        eye[1] < focus[1] - 100.0,
        "eye Y {} should sit above the focus {} in Y-down world",
        eye[1],
        focus[1]
    );

    // ---- rung 6 (non-vacuity): the orbit projection the page used to build
    // for this same frame is a materially DIFFERENT picture. Without this the
    // five rungs above pass on an orbit camera fitted to the same anchor.
    let orbit = legacy_orbit_vp([focus[0], -focus[1] + 60.0, focus[2]], 0.0, 0.62, 520.0);
    let orbit_pt = project(&orbit, [pt[0], pt[1], pt[2]]);
    let differs = match orbit_pt {
        None => true,
        Some(o) => (o.0 - on_screen.0).abs() > 8.0 || (o.1 - on_screen.1).abs() > 8.0,
    };
    assert!(
        differs,
        "the shared camera and the page's old orbit put the player in the \
         same place ({on_screen:?} vs {orbit_pt:?}) - this test would pass \
         on either model"
    );
}

/// The world map's **top-view debug camera** is reachable on this host: the
/// controller, its retail toggle chord and the camera are all engine-side, so
/// the page gets the same vantage the native window has. Before this the page
/// had no top-view camera at all.
#[test]
fn the_world_map_top_view_camera_is_reachable_on_the_page() {
    let Ok(disc) = env::var("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc image");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    rt.enter_field("map01").expect("enter_field(map01)");
    for _ in 0..4 {
        rt.tick_frame().expect("tick");
    }

    // Walk mode first: the retail player-follow overworld camera.
    assert!(
        !rt.play_camera_is_top_view(),
        "the overworld starts in walk mode"
    );
    let walk = rt.play_camera_view_json();
    let wv: serde_json::Value = serde_json::from_str(&walk).unwrap();
    assert_eq!(wv["arm"].as_str(), Some("worldmap_walk"), "{walk}");
    let walk_vp = rt.play_camera_vp(W, H);
    assert_eq!(walk_vp.len(), 16);

    // Retail's top-view chord: R1 + R2 held with Cross the new press
    // (`_DAT_8007B98C` gated, armed on world-map entry like the native
    // window's). The controller compares the WHOLE held word, so the first
    // frame holds the two shoulders alone and the second adds Cross. Bits are
    // the engine's `PadButton` space - the packed retail literal `0x4A` byte-
    // swapped, not `0x4A` itself (which reads as Down | Start | L3).
    const R1: u16 = legaia_engine_core::input::PadButton::R1 as u16;
    const R2: u16 = legaia_engine_core::input::PadButton::R2 as u16;
    const CROSS: u16 = legaia_engine_core::input::PadButton::Cross as u16;
    assert_eq!(R1 | R2 | CROSS, 0x4A00, "the chord word");
    rt.set_pad(R1 | R2);
    rt.tick_frame().expect("tick");
    rt.set_pad(R1 | R2 | CROSS);
    rt.tick_frame().expect("tick");
    assert!(
        rt.play_camera_is_top_view(),
        "the retail chord must reach the top-view camera on this host too"
    );

    let top = rt.play_camera_view_json();
    let tv: serde_json::Value = serde_json::from_str(&top).unwrap();
    assert_eq!(tv["arm"].as_str(), Some("worldmap_topview"), "{top}");
    let top_vp = rt.play_camera_vp(W, H);
    assert_eq!(top_vp.len(), 16, "the top view must hand back a matrix");
    assert!(top_vp.iter().all(|v| v.is_finite()), "{top_vp:?}");
    let same = walk_vp
        .iter()
        .zip(top_vp.iter())
        .all(|(a, b)| (a - b).abs() < 1e-6);
    assert!(
        !same,
        "the top view must frame the map differently from walk mode"
    );
}

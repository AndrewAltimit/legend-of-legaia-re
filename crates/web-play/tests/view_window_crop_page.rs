//! Page ladder for retail's **visible-tile crop** - the slot-B render
//! library's cell rectangle (`FUN_801F7088`'s prologue,
//! `field_view_window::view_cells`) and the ground emitter's per-cell gate
//! (`FUN_801F6D48`, `ViewCells::ground_visible`).
//!
//! Both hosts ask the one kernel (`field_view_window::field_view_cells`)
//! through `crop_framing`. At retail framing it walks retail's window; under
//! a pulled-back distance preset alone (the page's and the native window's
//! default `far`) it walks the window widened to the preset's scale, so what
//! lies past retail's near edge stays out of shot; under a swung, tilted or
//! zoomed camera it draws the map whole.
//!
//! The ladder takes the option the way a player does - the page's distance
//! control - and scores the crop on its effect: at `retail` the stamp a host
//! re-uploads on is live and both the ground index buffer and the terrain
//! draw mask come back smaller than the whole map; at `far` the crop holds,
//! at least as wide as retail's and still short of the whole map; a dragged
//! orbit then gives the whole map back.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

fn tick(rt: &mut LegaiaRuntime, n: usize) {
    for _ in 0..n {
        rt.tick_frame().expect("tick_frame");
    }
}

/// Cycle the distance preset until it reads `want` (three presets).
fn set_distance(rt: &mut LegaiaRuntime, want: &str) {
    for _ in 0..3 {
        if rt.play_camera_distance() == want {
            return;
        }
        rt.play_camera_cycle_distance();
    }
    assert_eq!(rt.play_camera_distance(), want, "no `{want}` preset");
}

#[test]
fn the_retail_preset_crops_the_field_to_its_visible_tiles() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("town01").expect("enter town01");
    assert!(rt.retail_view_window(), "the crop option defaults on");

    set_distance(&mut rt, "retail");
    tick(&mut rt, 30);
    let whole_ground = rt.field_ground_indices().len();
    let stamp = rt.field_view_window_stamp(false);
    assert_ne!(stamp, 0, "no crop at retail framing in a field scene");
    let ground = rt.field_ground_indices_cropped(false).len();
    assert!(
        ground > 0 && ground < whole_ground,
        "the ground emitter's gate kept {ground} of {whole_ground} indices"
    );
    let live = rt.field_terrain_live(false);
    let kept = live.iter().filter(|&&b| b == 1).count();
    assert!(
        kept < live.len(),
        "the decoration pass kept every one of {} terrain draws",
        live.len()
    );
    eprintln!(
        "[ok] retail crop: ground {ground}/{whole_ground} indices, terrain {kept}/{} draws",
        live.len()
    );

    // A pulled-back preset keeps the crop, widened: no cell retail draws is
    // lost, and the map is still not drawn whole.
    set_distance(&mut rt, "far");
    tick(&mut rt, 2);
    assert_ne!(rt.field_view_window_stamp(false), 0, "far dropped the crop");
    let far_ground = rt.field_ground_indices_cropped(false).len();
    assert!(
        far_ground >= ground && far_ground < whole_ground,
        "far kept {far_ground} of {whole_ground} ground indices (retail {ground})"
    );
    let far_live = rt.field_terrain_live(false);
    for (i, (&r, &f)) in live.iter().zip(&far_live).enumerate() {
        assert!(
            r == 0 || f == 1,
            "terrain draw {i} drawn at retail, cropped at far"
        );
    }

    // A swung camera is not retail's frustum: the map draws whole.
    assert!(rt.play_camera_orbit_by(0.5), "the orbit knob is live");
    tick(&mut rt, 2);
    assert_eq!(
        rt.field_view_window_stamp(false),
        0,
        "a swung view kept the crop"
    );
    assert_eq!(rt.field_ground_indices_cropped(false).len(), whole_ground);
}

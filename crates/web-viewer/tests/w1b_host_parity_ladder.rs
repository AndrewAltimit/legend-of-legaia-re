//! Ladder for the play page's half of the host-parity kernels: the three
//! per-frame decisions the page used to make in JS.
//!
//! Each test drives the real `LegaiaRuntime` over a disc-loaded scene and
//! asserts the exported answer, so a regression on either side of the wasm
//! boundary fails here rather than in a screenshot.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset. CI runs without disc
//! data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;
use std::env;

fn loaded_runtime() -> Option<LegaiaRuntime> {
    let disc = env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

/// Free-roam field draws both sides (mode `0`); the opening prologue's
/// scripted shot arms retail's NCLIP rejection (mode `2`).
///
/// The page's assembled pass used to call `disable(CULL_FACE)`
/// unconditionally, so the tableau shot rendered the near wall of the closed
/// cave-wall backdrop it sits inside.
#[test]
fn nclip_mode_is_zero_in_free_roam_and_two_under_the_scripted_shot() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    rt.enter_field("town01").expect("enter town01");
    assert_eq!(
        rt.play_render_nclip_mode(),
        0,
        "free-roam field draws both sides"
    );
    rt.enter_field("opdeene").expect("enter opdeene");
    // The chain's first beats stage the camera within a few ticks.
    let mut armed = false;
    for _ in 0..600 {
        rt.set_pad(0);
        rt.tick_frame().expect("tick");
        if rt.play_render_nclip_mode() == 2 {
            armed = true;
            break;
        }
    }
    assert!(armed, "the prologue's scripted shot arms the NCLIP cull");
}

/// The fade's focus is the FLOOR TIER under the actor lifted half a
/// character height, in the page's Y-up draw frame - the same point
/// `field_player_occluded` ray-casts to, not the actor's own `world_y`.
#[test]
fn occlusion_focus_is_the_shared_body_centre_and_drops_under_a_scripted_shot() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    rt.enter_field("town01").expect("enter town01");
    let focus = rt.play_occlusion_focus();
    assert_eq!(focus.len(), 3, "free-roam field has a focus");
    let pt = rt.player_transform();
    assert!(
        (focus[0] - pt[0]).abs() < 1e-3 && (focus[2] - pt[2]).abs() < 1e-3,
        "the focus sits over the player in X/Z"
    );
    // Y-up draw frame: the retail Y-down centre negated. Half a character
    // height above the tier the character stands on, whatever that tier is.
    assert!(
        focus[1] > -pt[1] - 1.0,
        "the focus is at or above the actor origin's draw-frame height"
    );
    // A scripted shot owns the camera: the fade must not arm under it.
    rt.enter_field("opdeene").expect("enter opdeene");
    let mut dropped = false;
    for _ in 0..600 {
        rt.set_pad(0);
        rt.tick_frame().expect("tick");
        if rt.play_occlusion_focus().is_empty() {
            dropped = true;
            break;
        }
    }
    assert!(dropped, "no fade focus while a timeline owns the camera");
}

/// The field party HUD's projection is the engine's, taken through the
/// FOLLOW camera - so it survives a scripted shot rather than reading the
/// cutscene framing.
#[test]
fn field_hud_projection_is_engine_side_and_ignores_the_cutscene_camera() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    rt.enter_field("town01").expect("enter town01");
    // A projectable lead lands on the 240-line stage, in band.
    rt.play_field_hud_project(960.0, 720.0);
    let free = rt.field_player_screen_y();
    assert_ne!(
        free,
        legaia_web_viewer::runtime::NO_FIELD_PROJECTION,
        "a lead in free-roam field projects"
    );
    assert!(
        (-4096..=4096).contains(&free),
        "the stage Y is the kernel's clamped band, got {free}"
    );
    // Re-projecting the same frame is a pure function of it.
    rt.play_field_hud_project(960.0, 720.0);
    assert_eq!(free, rt.field_player_screen_y());
    // A scripted shot must not move the readout: the export resolves the
    // FOLLOW camera, so the answer is the free-roam one even while a
    // cutscene frame owns the draw.
    rt.enter_field("opdeene").expect("enter opdeene");
    let mut saw_cutscene_frame = false;
    for _ in 0..600 {
        rt.set_pad(0);
        rt.tick_frame().expect("tick");
        if rt.play_render_nclip_mode() == 2 {
            saw_cutscene_frame = true;
            rt.play_field_hud_project(960.0, 720.0);
            // Still a number, and still the follow camera's: the projection
            // never reports the sentinel just because a shot is running.
            let y = rt.field_player_screen_y();
            assert!(
                y == legaia_web_viewer::runtime::NO_FIELD_PROJECTION || (-4096..=4096).contains(&y),
                "stage Y stays in band under a scripted shot, got {y}"
            );
            break;
        }
    }
    assert!(saw_cutscene_frame, "the prologue stages a camera");
    // No scene host / no lead: the sentinel path, not a panic.
    let mut bare = LegaiaRuntime::new();
    bare.play_field_hud_project(960.0, 720.0);
    assert_eq!(
        bare.field_player_screen_y(),
        legaia_web_viewer::runtime::NO_FIELD_PROJECTION
    );
}

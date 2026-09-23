//! Cross-host kernels the two shipped play hosts must both read, pinned here
//! because each one replaced a decision a host had been making locally.
//!
//! See `docs/tooling/host-drift.md` - "One decision, two inputs", "A GL state
//! word the engine does not own", and the two shapes a "the page is missing
//! it" reading gets backwards.

use legaia_engine_core::camera::Camera;
use legaia_engine_core::camera_view;
use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::field_occlusion;
use legaia_engine_core::save_select::{SelectPhase, phase_layout};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::field::CameraParam;
use legaia_engine_vm::psx_camera::CutsceneCameraInterp;

/// `apply_trigger == 0` beats are banked where the events are consumed.
///
/// `route_camera_events` is the only drain of `FieldEvent::CameraConfigure`
/// and it does not restore one, so a host watching its own later drain of
/// `pending_field_events` for snap beats never sees any.
#[test]
fn snap_beats_are_banked_on_the_camera_not_left_on_the_world_queue() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    w.pending_field_events = vec![
        FieldEvent::CameraConfigure {
            params: vec![CameraParam {
                slot: 1,
                value: 1024,
            }],
            apply_trigger: 0,
            mode: 0,
        },
        FieldEvent::CameraConfigure {
            params: vec![CameraParam {
                slot: 1,
                value: 2048,
            }],
            apply_trigger: 900,
            mode: 0,
        },
    ];
    let mut cam = Camera::default();
    cam.route_camera_events(&mut w);
    // Neither beat survives on the world queue.
    assert!(
        !w.pending_field_events
            .iter()
            .any(|e| matches!(e, FieldEvent::CameraConfigure { .. })),
        "route_camera_events consumes every Configure beat"
    );
    let beats = cam.take_camera_snap_beats();
    assert_eq!(beats.len(), 1, "only the apply==0 beat banks");
    // Packed component 4 is yaw; 1024 twelve-bit units = a quarter turn.
    let (idx, val) = beats[0][0];
    assert_eq!(idx, 4);
    assert!((val - std::f32::consts::TAU / 4.0).abs() < 1e-3);
    // A second take is empty - the bank is drained, not read.
    assert!(cam.take_camera_snap_beats().is_empty());
}

/// The snap decode is `cutscene_view`'s decode: same negations, same angle
/// scale, same 6x eye-trio reduction, same degenerate filters.
#[test]
fn snap_component_decode_matches_the_cutscene_view_slot_map() {
    let comps = CutsceneCameraInterp::snap_components_for(&[
        (0, 512),              // pitch
        (1, 1024),             // yaw
        (2, 256),              // roll
        (3, 60),               // tr x
        (5, 1),                // tr z - degenerate, filtered
        (6, (-100i16) as u16), // focus x, stored negated
        (8, (-200i16) as u16), // focus z, stored negated
        (9, 1),                // H - degenerate, filtered
    ]);
    let get = |i: usize| comps.iter().find(|(c, _)| *c == i).map(|(_, v)| *v);
    use std::f32::consts::TAU;
    assert!(
        (get(3).unwrap() - TAU / 8.0).abs() < 1e-3,
        "slot 0 -> pitch"
    );
    assert!((get(4).unwrap() - TAU / 4.0).abs() < 1e-3, "slot 1 -> yaw");
    assert!(
        (get(CutsceneCameraInterp::ROLL).unwrap() - TAU / 16.0).abs() < 1e-3,
        "slot 2 -> roll"
    );
    assert!((get(6).unwrap() - 10.0).abs() < 1e-3, "slot 3 -> tr x / 6");
    assert_eq!(get(8), None, "|z| <= 1 is not an eye depth");
    assert_eq!(get(0), Some(100.0), "slot 6 -> focus x, negated back");
    assert_eq!(get(2), Some(200.0), "slot 8 -> focus z, negated back");
    assert_eq!(get(5), None, "H <= 1 is not a focal length");
}

/// The NCLIP mode word is armed for the cutscene camera and nothing else,
/// and never on the overworld.
#[test]
fn nclip_cull_mode_arms_only_for_a_non_overworld_cutscene_frame() {
    assert_eq!(camera_view::nclip_cull_mode(true, false), 2);
    assert_eq!(camera_view::nclip_cull_mode(true, true), 0);
    assert_eq!(camera_view::nclip_cull_mode(false, false), 0);
    assert_eq!(camera_view::nclip_cull_mode(false, true), 0);
}

/// The debug orbit steers the SAME field the follow camera reads, is not
/// gated by a running cutscene, and is still field-only.
#[test]
fn debug_orbit_writes_manual_orbit_ungated_but_field_only() {
    let field = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    let mut cam = Camera::default();
    assert!(cam.debug_orbit_by(&field, 0.5));
    assert!((cam.manual_orbit - 0.5).abs() < 1e-6);
    // Battle / world map keep their own cameras; the compass must not move.
    let battle = World {
        mode: SceneMode::Battle,
        ..World::default()
    };
    assert!(!cam.debug_orbit_by(&battle, 0.5));
    assert!((cam.manual_orbit - 0.5).abs() < 1e-6);
}

/// The fade's world half: field mode, no scripted shot.
#[test]
fn occlusion_fade_arms_in_field_free_roam_only() {
    let field = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    assert!(field_occlusion::fade_armed(&field, false));
    assert!(
        !field_occlusion::fade_armed(&field, true),
        "cutscene camera"
    );
    let battle = World {
        mode: SceneMode::Battle,
        ..World::default()
    };
    assert!(!field_occlusion::fade_armed(&battle, false));
    let map = World {
        mode: SceneMode::WorldMap,
        ..World::default()
    };
    assert!(!field_occlusion::fade_armed(&map, false));
}

/// With no player actor there is no body centre - the same guard that stops
/// the fade arming at all, so neither host can stage a focus at the origin.
#[test]
fn occlusion_focus_needs_a_player_actor() {
    let w = World::default();
    assert!(field_occlusion::player_body_centre(&w).is_none());
}

/// Retail raises the overwrite / delete prompt FROM the preview, so a
/// confirm is a `SlotPreview` wearing a messagebox - one pill at the
/// relocated anchor, no pill cursor, the grid and info panel still drawn.
#[test]
fn save_select_confirm_is_the_preview_plus_a_messagebox() {
    let preview = phase_layout(SelectPhase::SlotPreview { slot: 1 });
    for phase in [
        SelectPhase::ConfirmOverwrite { slot: 1, cursor: 0 },
        SelectPhase::ConfirmDelete { slot: 1, cursor: 1 },
    ] {
        let l = phase_layout(phase);
        assert_eq!(l.single_pill, preview.single_pill);
        assert_eq!(l.pill_cursor, preview.pill_cursor);
        assert_eq!(l.preview, preview.preview, "the grid stays up");
        assert!(l.confirm, "and the messagebox rides on top");
    }
    assert!(preview.preview && !preview.confirm);
    // NowChecking hides the row behind its own dialog.
    let checking = phase_layout(SelectPhase::NowChecking {
        slot: 0,
        frames_remaining: 8,
    });
    assert!(checking.single_pill && !checking.pill_cursor);
    assert!(checking.now_checking && !checking.preview);
}

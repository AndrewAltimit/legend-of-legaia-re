//! Cross-host kernels the two shipped play hosts must both read, pinned here
//! because each one replaced a decision a host had been making locally.
//!
//! See `docs/tooling/host-drift.md` - "One decision, two inputs", "A GL state
//! word the engine does not own", and the two shapes a "the page is missing
//! it" reading gets backwards.

use legaia_engine_core::world::World;
use legaia_engine_field::camera::Camera;
use legaia_engine_field::camera_frame;
use legaia_engine_field::field_events::FieldEvent;
use legaia_engine_menus::save_select::{SelectPhase, phase_layout};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_vm::field::CameraParam;

/// `route_camera_events` is the only drain of `FieldEvent::CameraConfigure`,
/// and it leaves the shot in the globals both hosts draw from: a snap beat
/// writes them, and a glide beat in the same slice sets out from the snap.
#[test]
fn a_snap_then_a_glide_arm_the_mover_from_the_snap() {
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
    assert!(
        !w.pending_field_events
            .iter()
            .any(|e| matches!(e, FieldEvent::CameraConfigure { .. })),
        "route_camera_events consumes every Configure beat"
    );
    let mv = cam.mover.as_ref().expect("the glide beat armed the mover");
    assert_eq!(mv.pairs[1], (1024, 2048));
    assert_eq!(mv.d, 900);
}

/// The NCLIP mode word is armed for the whole field pass (retail culls the
/// back faces of every field mesh - `FUN_80043390`'s mask is `0xFFFFFFFF`
/// unless the colour word carries `0x08000000`, which no field spawner sets)
/// and for a cutscene camera on any other non-overworld mode; never on the
/// overworld, and not for a free battle frame. The dance hall is a field-shaped
/// pass (its placed actors are single-sided too), so it is armed.
#[test]
fn nclip_cull_mode_arms_for_the_field_pass_and_non_overworld_cutscenes() {
    use SceneMode::*;
    assert_eq!(camera_frame::nclip_cull_mode(false, Field), 2);
    assert_eq!(camera_frame::nclip_cull_mode(true, Field), 2);
    assert_eq!(camera_frame::nclip_cull_mode(true, Cutscene), 2);
    assert_eq!(camera_frame::nclip_cull_mode(true, WorldMap), 0);
    assert_eq!(camera_frame::nclip_cull_mode(false, WorldMap), 0);
    assert_eq!(camera_frame::nclip_cull_mode(false, Battle), 0);
    assert_eq!(camera_frame::nclip_cull_mode(false, Dance), 2);
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

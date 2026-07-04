//! Disc-gated regression: the field d-pad is not inverted after a cutscene.
//!
//! The New Game prologue (`opdeene` -> `town01` opening cutscene) runs op-0x45
//! Camera Configure events that leave the [`BootSession`]'s camera controller in
//! `Cinematic` mode at the shot's yaw. The renderer frames free-roam field with
//! a fixed follow camera that never reads that yaw, but `BootSession::tick`
//! feeds the controller yaw into `World::field_camera_azimuth` to remap the pad
//! camera-relative. A leaked non-zero yaw therefore rotated the d-pad ~180deg
//! off the on-screen camera (up walked down, left walked right) once control
//! returned in Rim Elm - only on the `--boot-ui` path, since a direct boot never
//! runs those camera events.
//!
//! `Camera::reset_for_free_roam` (called from `BootSession::tick`) snaps the
//! controller back to the follow default whenever the field is in free-roam, so
//! the azimuth quantises to quadrant 0 (identity remap) and the controls match
//! the rendered camera. This drives the real `BootSession::tick` to pin that.
//!
//! Skip-passes without disc data so CI works without Sony bytes.

use std::path::PathBuf;

use legaia_engine_shell::boot::{BootConfig, BootSession};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn free_roam_field_clears_leaked_cinematic_camera_yaw() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open extracted boot session");
    let opts = legaia_engine_shell::boot::FieldLiveOpts::default();
    session
        .enter_field_live("town01", &opts)
        .expect("enter town01 live");

    // Free-roam field: plain Field mode, no opening cutscene timeline (entered
    // directly, not via the prologue hand-off).
    use legaia_engine_core::world::SceneMode;
    assert!(matches!(session.host.world.mode, SceneMode::Field));
    assert!(!session.host.world.cutscene_timeline_active());

    // Simulate the state a cutscene leaves behind: the controller parked in
    // Cinematic mode at a ~180deg yaw (the value that inverts the d-pad remap).
    session.camera.mode = legaia_engine_core::camera::CameraMode::Cinematic;
    session.camera.yaw = std::f32::consts::PI;
    session.camera.pitch = 0.5;

    // One free-roam tick: `BootSession::tick` must snap the camera back to the
    // follow default before it computes `field_camera_azimuth`.
    session.tick().expect("field tick");

    // Quadrant 0 (identity remap) - screen-up walks world +Z, not inverted.
    // (`decode_field_direction` quantises `((azimuth + 512) / 1024) & 3`.)
    let azimuth = session.host.world.field_camera_azimuth;
    let quadrant = ((azimuth as u32 + 512) / 1024) & 3;
    assert_eq!(
        quadrant, 0,
        "free-roam field azimuth stays in quadrant 0 (got azimuth={azimuth}); \
         a leaked cinematic yaw would land in quadrant 2 and invert the d-pad"
    );
    assert_eq!(
        session.camera.yaw, 0.0,
        "controller yaw reset to the follow default"
    );
}

//! Unit tests for the camera tween's environment form.

use super::*;

/// The image child's glide-origin pair survives its env round trip.
#[test]
fn cam_align_env_round_trips() {
    use legaia_engine_battle_vm::battle_cam_script::BattleCamPose;
    let live = BattleCamPose {
        pitch: 128.0,
        yaw: 3205.0,
        tr: [0.0, 972.0, 2867.0],
        focus: [-11.0, 0.0, -381.0],
    };
    let end = BattleCamPose {
        pitch: 128.0,
        yaw: 4062.0,
        tr: [0.0, 1024.0, 4915.0],
        focus: [-7.0, 0.0, -331.0],
    };
    let env = cam_align_to_env(&(live, end));
    assert_eq!(cam_align_from_env(&env), Some((live, end)));
    assert_eq!(cam_align_from_env("1,2,3"), None);
}

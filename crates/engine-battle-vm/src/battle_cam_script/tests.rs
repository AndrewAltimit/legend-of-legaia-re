use super::*;

mod action_arm;
mod cast_exit_shots;
mod glide;
mod menu_framing;
mod post_action;
mod projection;
mod shake_drive;

/// The formation behind the traced Tetsu fight. The trace pins the far
/// framing's TR.z at `7680` = `prescale(0x12C0)`, and case 9 builds that
/// raw `0x12C0` as `span * 3`, so the traced formation spanned `1600`
/// world units. Every trace-pinned menu assertion below is stated
/// against this formation - which is the point: the law reproduces the
/// measurement instead of hardcoding it.
fn traced_formation() -> FormationBox {
    FormationBox {
        min: [-800.0, -800.0],
        max: [800.0, 800.0],
    }
}

/// The traced far framing, reproduced by the case-9 law.
fn traced_menu_tr() -> [f32; 3] {
    menu_framing(Some(traced_formation()), 0.0).tr
}

/// A camera armed on the traced formation.
fn traced_cam(phase: BattleCamPhase) -> BattleCamera {
    let mut cam = BattleCamera::new(phase, 0);
    cam.set_formation(Some(traced_formation()));
    // Re-snap: `new` built the entry pose before the formation landed.
    if phase == BattleCamPhase::Menu {
        cam.pose = cam.menu_pose();
    }
    cam
}

fn steps(cam: &mut BattleCamera, n: u64) {
    for _ in 0..n {
        cam.advance_to(cam.last_frames + 2);
    }
}

/// Camera steps a re-armed `a3 = 0xC` chase needs to close any battle-scale
/// gap: each step covers at least a sixth of what remains, then the last
/// dozen units go at the walker's minimum `2` a step.
const CHASE_SETTLE_STEPS: u64 = 60;

/// Project a point through [`battle_vp`] composed with the same
/// per-draw model factors the hosts use, to PSX 320x240 screen pixels.
fn project(vp: &[f32; 16], model: &[f32; 16], v: [f32; 3]) -> Option<(f32, f32)> {
    let m = mat_mul(vp, model);
    let x = m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12];
    let y = m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13];
    let w = m[3] * v[0] + m[7] * v[1] + m[11] * v[2] + m[15];
    if w <= 1.0 {
        return None;
    }
    Some((160.0 + x / w * 160.0, 120.0 - y / w * 120.0))
}

/// The Y-flip model factor every host draw carries.
const FLIP: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, -1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

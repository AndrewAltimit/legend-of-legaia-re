//! The battle view-projection against retail screen coordinates.

use super::*;

/// The GTE origin projects to the control file's screen centre, not to
/// the geometric centre of the frame.
///
/// This is the assertion the hand-rolled comparison below cannot make on
/// its own: that test only says the matrix and the reference agree, so it
/// stayed green while both carried `120`. Here the expected value comes
/// from [`GTE_OFY`] and the projected value comes from the matrix, so a
/// matrix built on `240 / 2` fails.
#[test]
fn the_projected_origin_lands_on_the_retail_screen_centre() {
    // Eye straight down -Z at the origin: `focus` and the rotation are
    // zero, so `v = origin` lands at eye `(0, 0, TR.z)` and the retail
    // transform reduces to `screen = (OFX, OFY)`.
    let pose = BattleCamPose {
        pitch: 0.0,
        yaw: 0.0,
        tr: [0.0, 0.0, 7680.0],
        focus: [0.0, 0.0, 0.0],
    };
    let vp = battle_vp(&pose, 4.0, 4.0 / 3.0);
    let (sx, sy) = project(&vp, &FLIP, [0.0, 0.0, 0.0]).unwrap();
    assert!((sx - GTE_OFX).abs() < 0.01, "OFX: {sx} vs {GTE_OFX}");
    assert!((sy - GTE_OFY).abs() < 0.01, "OFY: {sy} vs {GTE_OFY}");
    // And the centre is genuinely off the naive one, so this is not a
    // tautology about the viewport map.
    assert!((sy - 120.0).abs() > 5.0);
}

/// [`battle_vp`] reproduces the exact retail projection
/// `screen = H*(Rx(p)*Ry(y)*(v*S - focus*S) + TR)/Ez` against a
/// hand-rolled reference, dome (raw units) and actor (4x world scale)
/// draw classes both.
#[test]
fn battle_vp_matches_the_handrolled_retail_projection() {
    let pose = BattleCamPose {
        pitch: 32.0,
        yaw: 224.0,
        tr: [0.0, 1280.0, 7680.0],
        focus: [100.0, 0.0, -50.0],
    };
    let handrolled = |v: [f32; 3], s: f32| -> Option<(f32, f32)> {
        let to_rad = |u: f32| u / 4096.0 * std::f32::consts::TAU;
        let (sy, cy) = to_rad(pose.yaw).sin_cos();
        let (sp, cp) = to_rad(pose.pitch).sin_cos();
        // World-scale the vertex and subtract the scaled focus.
        let p = [
            v[0] * s - pose.focus[0] * 4.0,
            v[1] * s - pose.focus[1] * 4.0,
            v[2] * s - pose.focus[2] * 4.0,
        ];
        let ry = [cy * p[0] + sy * p[2], p[1], -sy * p[0] + cy * p[2]];
        let e = [ry[0], cp * ry[1] - sp * ry[2], sp * ry[1] + cp * ry[2]];
        let ez = e[2] + pose.tr[2];
        if ez <= 1.0 {
            return None;
        }
        // The screen centre is the GTE control file's `(OFX, OFY)`, not
        // the geometric `(160, 120)`: writing the naive centre on both
        // sides of this comparison is what let the six-pixel error live.
        Some((
            256.0 * (e[0] + pose.tr[0]) / ez + GTE_OFX,
            256.0 * (e[1] + pose.tr[1]) / ez + GTE_OFY,
        ))
    };
    let vp = battle_vp(&pose, 4.0, 4.0 / 3.0);
    // Dome class: raw PSX vertices, model = FLIP (the vp's trailing flip
    // cancels it, so the retail chain sees the raw Y-down vertex).
    for v in [[1000.0f32, -500.0, 3000.0], [-2000.0, 0.0, 6000.0]] {
        let got = project(&vp, &FLIP, v).unwrap();
        // The dome draws unscaled but the camera focus is scaled - the
        // handrolled reference scales the vertex by 1 and focus by 4.
        let want = handrolled(v, 1.0).unwrap();
        let d = ((got.0 - want.0).powi(2) + (got.1 - want.1).powi(2)).sqrt();
        assert!(d < 0.05, "dome {v:?}: {d}px ({got:?} vs {want:?})");
    }
    // Actor class: model composes scale(4)*FLIP under the camera.
    let scale4: [f32; 16] = [
        4.0, 0.0, 0.0, 0.0, //
        0.0, 4.0, 0.0, 0.0, //
        0.0, 0.0, 4.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let model = mat_mul(&scale4, &FLIP);
    for v in [[100.0f32, -130.0, -800.0], [0.0, 0.0, 800.0]] {
        let got = project(&vp, &model, v).unwrap();
        let want = handrolled(v, 4.0).unwrap();
        let d = ((got.0 - want.0).powi(2) + (got.1 - want.1).powi(2)).sqrt();
        assert!(d < 0.05, "actor {v:?}: {d}px ({got:?} vs {want:?})");
    }
}

/// **The actor-anchored close-ups must not park the eye inside the
/// actor they frame.** The submenu and action framings are built around
/// the acting actor (`focus = actor.world`), so that actor's own
/// footprint has to stay in front of the eye and its standing height
/// has to fit the frame.
///
/// Scoped to those two deliberately: the dialogue close-up and the far
/// menu framing are anchored on the *formation*, not on the acting
/// actor - the traced dialogue pose (yaw `0`, TR.z `1638`, focus at the
/// origin) genuinely leaves the near party row behind the eye, and the
/// formation framing has its own on-screen sweep above.
///
/// This is the invariant the pose-equality tests structurally cannot
/// see. They compare two hosts' poses to each other; a pose that puts
/// the camera *inside* the geometry is equal on both hosts and passes
/// them both. Here the pose is projected against the world it frames.
#[test]
fn no_resting_framing_puts_the_eye_inside_the_acting_actor() {
    // The actor draw class: BATTLE_WORLD_SCALE + the per-model Y-flip.
    const S: f32 = 4.0;
    let scale_s: [f32; 16] = [
        S, 0.0, 0.0, 0.0, //
        0.0, S, 0.0, 0.0, //
        0.0, 0.0, S, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let model = mat_mul(&scale_s, &FLIP);
    let actor = BattleCamActor::default(); // seated at the traced (0,0,-800)
    let poses = [
        ("submenu", actor.submenu_pose()),
        ("action", action_framing(actor, ActionFraming::default())),
    ];
    for (name, pose) in poses {
        let vp = battle_vp(&pose, S, 4.0 / 3.0);
        // The ground the acting actor stands on, drawn in the same
        // scaled stage space as the actor itself.
        let foot = project(&vp, &model, actor.world).unwrap_or_else(|| {
            panic!("{name}: the acting actor's own footprint is behind the eye")
        });
        // ... and its head, so a camera parked inside the body (the
        // footprint in front but the whole mesh wrapped around the
        // lens) is caught too.
        let head = project(&vp, &model, [actor.world[0], -370.0, actor.world[2]])
            .unwrap_or_else(|| panic!("{name}: the acting actor's head is behind the eye"));
        // A standing character spans a bounded share of the 240-line
        // frame; more than a full frame means the eye is inside it.
        let span = (foot.1 - head.1).abs();
        assert!(
            span < 240.0,
            "{name}: the acting actor spans {span} of 240 scanlines - the eye is inside it"
        );
    }
}

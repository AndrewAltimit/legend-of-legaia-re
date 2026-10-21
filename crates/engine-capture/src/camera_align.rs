//! The battle camera's tween at the capture instant, in its environment
//! form (`LEGAIA_BATTLE_CAM_ALIGN`).

/// `RetailBattle::cam_tween` as the image child's `LEGAIA_BATTLE_CAM_ALIGN`:
/// sixteen comma-separated numbers, the live pose then the endpoints, each
/// `pitch, yaw, tr.x, tr.y, tr.z, focus.x, focus.y, focus.z`.
pub fn cam_align_to_env(
    tween: &(
        legaia_engine_camera::projection::BattleCamPose,
        legaia_engine_camera::projection::BattleCamPose,
    ),
) -> String {
    [tween.0, tween.1]
        .iter()
        .flat_map(|p| {
            [p.pitch, p.yaw]
                .into_iter()
                .chain(p.tr)
                .chain(p.focus)
                .collect::<Vec<_>>()
        })
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse [`cam_align_to_env`]'s string back; `None` on any malformed field.
pub fn cam_align_from_env(
    v: &str,
) -> Option<(
    legaia_engine_camera::projection::BattleCamPose,
    legaia_engine_camera::projection::BattleCamPose,
)> {
    use legaia_engine_camera::projection::BattleCamPose;
    let n: Vec<f32> = v
        .split(',')
        .map(|f| f.trim().parse().ok())
        .collect::<Option<_>>()?;
    if n.len() != 16 {
        return None;
    }
    let pose = |o: usize| BattleCamPose {
        pitch: n[o],
        yaw: n[o + 1],
        tr: [n[o + 2], n[o + 3], n[o + 4]],
        focus: [n[o + 5], n[o + 6], n[o + 7]],
    };
    Some((pose(0), pose(8)))
}

/// One pose as eight comma-separated numbers (`pitch, yaw, tr.x, tr.y,
/// tr.z, focus.x, focus.y, focus.z`) - the gate-tick framing a display-lag
/// retake carries (`LEGAIA_BATTLE_CAM_GATE_TARGET`).
pub fn cam_pose_to_env(p: &legaia_engine_camera::projection::BattleCamPose) -> String {
    [p.pitch, p.yaw]
        .into_iter()
        .chain(p.tr)
        .chain(p.focus)
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse [`cam_pose_to_env`]'s string back; `None` on any malformed field.
pub fn cam_pose_from_env(v: &str) -> Option<legaia_engine_camera::projection::BattleCamPose> {
    let n: Vec<f32> = v
        .split(',')
        .map(|f| f.trim().parse().ok())
        .collect::<Option<_>>()?;
    (n.len() == 8).then(|| legaia_engine_camera::projection::BattleCamPose {
        pitch: n[0],
        yaw: n[1],
        tr: [n[2], n[3], n[4]],
        focus: [n[5], n[6], n[7]],
    })
}

#[cfg(test)]
mod tests;

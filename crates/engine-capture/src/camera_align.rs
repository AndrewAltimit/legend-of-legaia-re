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

#[cfg(test)]
mod tests;

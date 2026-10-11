//! Field-rig roles: which battle part feeds each field bone, the field
//! skeleton's parent / child tables, role derivation and the per-group world
//! statistics it reads.

use super::super::*;
use super::FIELD_BONES;

/// Canonical battle parts feeding each field bone role.
/// Field roles: 0 head, 1 torso, 2/3 armA up/lo, 4/5 armB up/lo,
/// 6/7 legA up/lo, 8/9 legB up/lo.
pub(super) const FIELD_ROLE_SOURCES: [&[usize]; FIELD_BONES] = [
    &[0],      // head
    &[1, 2],   // torso + pelvis
    &[3],      // armA upper
    &[4, 5],   // armA fore + hand
    &[6],      // armB upper
    &[7, 8],   // armB fore + hand
    &[9],      // legA thigh
    &[10, 11], // legA shin + foot
    &[12],     // legB thigh
    &[13, 14], // legB shin + foot
];

/// Field role chain child (role space, see [`FIELD_ROLE_SOURCES`]):
/// torso -> head, each upper limb -> its lower limb; lowers and the
/// head are terminal.
pub(in super::super) const FIELD_CHILD: [Option<usize>; FIELD_BONES] = [
    None,
    Some(0),
    Some(3),
    None,
    Some(5),
    None,
    Some(7),
    None,
    Some(9),
    None,
];

/// Field role chain parent (chain-internal edges only).
pub(in super::super) const FIELD_PARENT: [Option<usize>; FIELD_BONES] = [
    Some(1),
    None,
    None,
    Some(2),
    None,
    Some(4),
    None,
    Some(6),
    None,
    Some(8),
];

/// Derived anatomy of one retail field rig: `bone_for_role[r]` = the
/// field bone index playing canonical role `r` (see
/// [`FIELD_ROLE_SOURCES`]). Derived from rest-pose world centroids per
/// slot - the group order differs between characters (Vahn's arm pair
/// orders upper/lower differently from Noa's).
pub(in super::super) fn derive_field_roles(
    model: &[ModelObject],
    rest: &[PartPose],
) -> Result<[usize; FIELD_BONES]> {
    if model.len() < FIELD_BONES || rest.len() < FIELD_BONES {
        bail!(
            "field rig has {} groups / {} channels, expected at least {FIELD_BONES}",
            model.len(),
            rest.len()
        );
    }
    #[derive(Clone, Copy)]
    struct Info {
        idx: usize,
        cx: f32,
        cy: f32,
        max_y: f32,
    }
    let mut infos = Vec::with_capacity(FIELD_BONES);
    for (i, o) in model.iter().take(FIELD_BONES).enumerate() {
        let m = rot_matrix(&rest[i]);
        let mut c = (0f32, 0f32);
        let mut max_y = f32::MIN;
        for v in &o.vertices {
            let w = apply(&m, [v[0] as f32, v[1] as f32, v[2] as f32]);
            c.0 += w[0] + rest[i].tx as f32;
            c.1 += w[1] + rest[i].ty as f32;
            max_y = max_y.max(w[1] + rest[i].ty as f32);
        }
        let n = o.vertices.len().max(1) as f32;
        infos.push(Info {
            idx: i,
            cx: c.0 / n,
            cy: c.1 / n,
            max_y,
        });
    }
    // Head = highest centroid (y-down: most negative).
    infos.sort_by(|a, b| a.cy.total_cmp(&b.cy));
    let head = infos[0].idx;
    // Torso = the most x-central of the rest.
    let torso = infos[1..]
        .iter()
        .min_by(|a, b| a.cx.abs().total_cmp(&b.cx.abs()))
        .unwrap()
        .idx;
    // Legs = the four remaining bones whose geometry reaches lowest
    // (largest max_y); arms = the other four.
    let mut rest_bones: Vec<Info> = infos
        .iter()
        .filter(|i| i.idx != head && i.idx != torso)
        .copied()
        .collect();
    rest_bones.sort_by(|a, b| b.max_y.total_cmp(&a.max_y));
    let (legs, arms) = rest_bones.split_at(4);
    let side = |set: &[Info], neg: bool| -> Vec<Info> {
        let mut v: Vec<Info> = set
            .iter()
            .filter(|i| (i.cx < 0.0) == neg)
            .copied()
            .collect();
        // Upper = higher centroid (more negative y).
        v.sort_by(|a, b| a.cy.total_cmp(&b.cy));
        v
    };
    let (arm_l, arm_r) = (side(arms, true), side(arms, false));
    let (leg_l, leg_r) = (side(legs, true), side(legs, false));
    if arm_l.len() != 2 || arm_r.len() != 2 || leg_l.len() != 2 || leg_r.len() != 2 {
        bail!(
            "field rig chains did not split 2/2/2/2 (arms {}/{}, legs {}/{})",
            arm_l.len(),
            arm_r.len(),
            leg_l.len(),
            leg_r.len()
        );
    }
    Ok([
        head,
        torso,
        arm_l[0].idx,
        arm_l[1].idx,
        arm_r[0].idx,
        arm_r[1].idx,
        leg_l[0].idx,
        leg_l[1].idx,
        leg_r[0].idx,
        leg_r[1].idx,
    ])
}

/// Rest-pose world bbox centre + per-axis extents of a set of posed
/// parts. The bbox centre (not the vertex average, which vertex density
/// skews) is the anchor: with the per-axis span, the baked part's bbox
/// coincides with the target part's, so limb chains meet exactly where
/// retail's do.
pub(in super::super) fn group_world_stats(
    parts: &[(&ModelObject, &PartPose)],
) -> ((f32, f32, f32), [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for (o, p) in parts {
        let m = rot_matrix(p);
        for v in &o.vertices {
            let w = apply(&m, [v[0] as f32, v[1] as f32, v[2] as f32]);
            let w = [w[0] + p.tx as f32, w[1] + p.ty as f32, w[2] + p.tz as f32];
            for k in 0..3 {
                min[k] = min[k].min(w[k]);
                max[k] = max[k].max(w[k]);
            }
        }
    }
    if min[0] > max[0] {
        (min, max) = ([0.0; 3], [0.0; 3]);
    }
    let center = (
        (min[0] + max[0]) / 2.0,
        (min[1] + max[1]) / 2.0,
        (min[2] + max[2]) / 2.0,
    );
    let ext = [
        (max[0] - min[0]).max(1.0),
        (max[1] - min[1]).max(1.0),
        (max[2] - min[2]).max(1.0),
    ];
    (center, ext)
}

//! Hand handling: FK-insetting the hand twins, the welded weapon fists that are
//! kept, the waist-curtain ids, and fist mirroring.

use super::super::*;

/// How much of the hands gets reversed-twin prims (fit-ladder rungs).
/// `WeaponHand` keeps the full treatment on the armB hand alone - the
/// sibling weapon-fist (Che's hammer) is the biggest open-visibility
/// surface, and halving the twin cost is what fits the Gala file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HandTwins {
    Full,
    WeaponHand,
    None,
}

#[allow(clippy::too_many_arguments)]
/// The FK hand inset: translate a baked fist IN HAND-LOCAL SPACE so its
/// posed joint overlap with the forearm matches the replaced hand's
/// across the host's own battle clips.
///
/// Why the centroid seat alone is not enough (measured on the v3 defect
/// states): matching centroid *magnitude* puts the fist in the right
/// region, but the retail hand's geometry OVERLAPS the forearm at the
/// wrist - a sibling fist of a different shape merely touches it, and
/// the joint "breathes" 3-6x wider than retail across the stance clips
/// (Gi-as-Noa armB closest-pair 2.7..9.9 vs retail's 1.0..3.2; Che's
/// hammer arm 10..18 vs Gala's steady 3) - a visible seam at the
/// command-ring closeup, the class every rest-pose metric is blind to.
///
/// The inset is a single hand-local shift `d` (posed as `R_h*d` per
/// frame) grid-searched to minimize the worst posed closest-pair
/// distance to the forearm across every record0 clip frame, aiming at
/// the host's own closure and clamped to the hand-seat law's budget.
pub(super) fn fk_inset_hand(
    hand: &mut ModelObject,
    forearm: &ModelObject,
    dst_forearm: &ModelObject,
    dst_hand: &ModelObject,
    ch_f: usize,
    ch_h: usize,
    anims: &[legaia_actor_formats::monster_archive::MonsterAnimation],
    law_budget: f32,
) -> Result<Option<[f32; 3]>> {
    use legaia_actor_formats::monster_archive::PartPose;
    if hand.vertices.is_empty() || forearm.vertices.is_empty() {
        return Ok(None);
    }
    let rot = |p: &PartPose| -> [[f32; 3]; 3] {
        let rad = |r: u16| (r as f32) * std::f32::consts::TAU / 4096.0;
        let (sx, cx) = rad(p.rx).sin_cos();
        let (sy, cy) = rad(p.ry).sin_cos();
        let (sz, cz) = rad(p.rz).sin_cos();
        [
            [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
            [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
            [-sy, sx * cy, cx * cy],
        ]
    };
    let pose = |vs: &[[i16; 3]], p: &PartPose, d: &[f32; 3]| -> Vec<[f32; 3]> {
        let r = rot(p);
        vs.iter()
            .map(|v| {
                let x = v[0] as f32 + d[0];
                let y = v[1] as f32 + d[1];
                let z = v[2] as f32 + d[2];
                [
                    r[0][0] * x + r[0][1] * y + r[0][2] * z + p.tx as f32,
                    r[1][0] * x + r[1][1] * y + r[1][2] * z + p.ty as f32,
                    r[2][0] * x + r[2][1] * y + r[2][2] * z + p.tz as f32,
                ]
            })
            .collect()
    };
    let closest = |a: &[[f32; 3]], b: &[[f32; 3]]| -> f32 {
        let mut best = f32::MAX;
        for va in a {
            for vb in b {
                let d = (va[0] - vb[0]).powi(2) + (va[1] - vb[1]).powi(2) + (va[2] - vb[2]).powi(2);
                if d < best {
                    best = d;
                }
            }
        }
        best.sqrt()
    };
    // Sample frames: every 2nd frame of every clip keeps the sweep
    // representative without an O(everything) budget.
    let mut frames: Vec<(&PartPose, &PartPose)> = Vec::new();
    for a in anims {
        for f in a.frames.iter().step_by(2) {
            if let (Some(pf), Some(ph)) = (f.get(ch_f), f.get(ch_h)) {
                frames.push((pf, ph));
            }
        }
    }
    if frames.is_empty() {
        return Ok(None);
    }
    let worst_closure = |vs: &[[i16; 3]], d: &[f32; 3]| -> f32 {
        let mut worst = 0f32;
        for (pf, ph) in &frames {
            let fa = pose(&forearm.vertices, pf, &[0.0; 3]);
            let ha = pose(vs, ph, d);
            let c = closest(&fa, &ha);
            if c > worst {
                worst = c;
            }
        }
        worst
    };
    // The host's own joint sets the target - never chase below its noise.
    let target = worst_closure(&dst_hand.vertices, &[0.0; 3]).max({
        // dst forearm/hand pair measured with the dst forearm.
        let mut worst = 0f32;
        for (pf, ph) in &frames {
            let fa = pose(&dst_forearm.vertices, pf, &[0.0; 3]);
            let ha = pose(&dst_hand.vertices, ph, &[0.0; 3]);
            let c = closest(&fa, &ha);
            if c > worst {
                worst = c;
            }
        }
        worst
    });
    let base = worst_closure(&hand.vertices, &[0.0; 3]);
    if base <= target + 1.0 {
        return Ok(None); // already as tight as the host's own joint
    }
    // Grid-search the hand-local inset, respecting the seat-law budget.
    let cen = |vs: &[[i16; 3]]| -> [f32; 3] {
        let n = vs.len() as f32;
        let s = vs.iter().fold([0f32; 3], |a, v| {
            [a[0] + v[0] as f32, a[1] + v[1] as f32, a[2] + v[2] as f32]
        });
        [s[0] / n, s[1] / n, s[2] / n]
    };
    let ch = cen(&hand.vertices);
    let dst_mag = {
        let c = cen(&dst_hand.vertices);
        (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
    };
    let mut best: ([f32; 3], f32) = ([0.0; 3], base);
    for dx in (-15i32..=15).step_by(3) {
        for dy in (-15i32..=15).step_by(3) {
            for dz in (-15i32..=15).step_by(3) {
                let d = [dx as f32, dy as f32, dz as f32];
                let nc = [ch[0] + d[0], ch[1] + d[1], ch[2] + d[2]];
                let mag = (nc[0] * nc[0] + nc[1] * nc[1] + nc[2] * nc[2]).sqrt();
                if mag > dst_mag + law_budget {
                    continue;
                }
                let w = worst_closure(&hand.vertices, &d);
                if w < best.1 {
                    best = (d, w);
                }
            }
        }
    }
    if best.1 >= base - 0.5 {
        return Ok(None); // nothing usefully better in budget
    }
    for v in hand.vertices.iter_mut() {
        *v = [
            round_coord(v[0] as f32 + best.0[0])?,
            round_coord(v[1] as f32 + best.0[1])?,
            round_coord(v[2] as f32 + best.0[2])?,
        ];
    }
    Ok(Some(best.0))
}

/// Monster fists that are really welded weapons: `(monster id, welded
/// canonical hand)`. Che's armB hammer-fist (163 part 8: 129 prims at
/// centroid magnitude 150) and Gi's armA blade-fist (162 part 5: claw
/// radii to 219 from its own centroid), against <= 38..61 for every real
/// Delilas fist. A welded weapon never closes the wrist the way a hand
/// does (Che's was the one joint the FK inset left at 5..7), and under a
/// host clip whose hand rotations assume a hand-sized part the blade
/// sweeps wildly - Gi's blade-fist under Noa's Spirit charge swept from
/// the legs to over the head, the catalogued `delilas_gi_spirit
/// _distortion_v3` streak. With the host's own weapon fused back in
/// (`weapon_fuse`) the welded copy is also redundant: the swap replaces
/// it with the sibling's other fist, mirrored.
pub(super) const WELDED_WEAPON_FISTS: [(u16, usize); 2] = [(163, 8), (162, 5)];

/// The canonical hand part whose welded weapon the swap KEEPS for this
/// source, or `None` when nothing is kept. The paired policy switch for
/// [`super::super::winpose::retarget_clip_wrist`]: a kept welded weapon's hand
/// must play the sibling's own wrist relation (its authored attitude is
/// the retail look), while a mirrored fist stays slaved to the host's.
pub fn kept_welded_hand(source_id: u16, keep_welded_fist: bool) -> Option<usize> {
    if !keep_welded_fist {
        return None;
    }
    WELDED_WEAPON_FISTS
        .iter()
        .find(|&&(id, _)| id == source_id)
        .map(|&(_, w)| w)
}

/// Monster ids whose swapped form needs the waist curtain (see the block
/// in `playerize_scaled`): Che's round torso over a narrow loincloth.
pub(super) const WAIST_CURTAIN_IDS: [u16; 1] = [163];

/// Replace the welded-weapon fist (canonical `to`) with the sibling's
/// other fist (canonical `from`) mirrored across the source body's
/// sagittal plane, re-expressed in the target hand's rest frame:
/// `v_to = R_to^T * M * R_from * v_from` with `M` the reflection about
/// the shoulder-span (lateral) axis. Rotation-only on purpose - the
/// downstream centroid seat + FK inset own the translation, so the mirror
/// only has to deliver shape + orientation. The reflection flips
/// chirality, so every primitive's winding reverses (middle-corner swap:
/// reverses both triangles of a Z-order quad, and a triangle outright).
pub(super) fn mirror_fist(
    src_model: &mut [ModelObject],
    src_rest: &[PartPose],
    to: usize,
) -> Result<()> {
    let from = if to == 8 { 5 } else { 8 };
    let pivot = |c: usize| -> [f32; 3] {
        [
            src_rest[c].tx as f32,
            src_rest[c].ty as f32,
            src_rest[c].tz as f32,
        ]
    };
    let mut lat = vsub(pivot(6), pivot(3)); // shoulder span = lateral axis
    let n = (lat[0] * lat[0] + lat[1] * lat[1] + lat[2] * lat[2]).sqrt();
    if n < 1.0 {
        bail!("degenerate shoulder span");
    }
    for v in lat.iter_mut() {
        *v /= n;
    }
    let ra = rot_matrix(&src_rest[from]);
    let rb = rot_matrix(&src_rest[to]);
    let mut fist = src_model[from].clone();
    for v in fist.vertices.iter_mut() {
        let w = apply(&ra, [v[0] as f32, v[1] as f32, v[2] as f32]);
        let d = w[0] * lat[0] + w[1] * lat[1] + w[2] * lat[2];
        let m = [
            w[0] - 2.0 * d * lat[0],
            w[1] - 2.0 * d * lat[1],
            w[2] - 2.0 * d * lat[2],
        ];
        let lb = apply_transposed(&rb, m);
        *v = [
            round_coord(lb[0])?,
            round_coord(lb[1])?,
            round_coord(lb[2])?,
        ];
    }
    for g in fist.groups.iter_mut() {
        for p in g.prims.iter_mut() {
            p.vertices.swap(1, 2);
            if p.uvs.len() >= 3 {
                p.uvs.swap(1, 2);
            }
            if p.colors.len() >= 3 {
                p.colors.swap(1, 2);
            }
        }
    }
    src_model[to] = fist;
    Ok(())
}

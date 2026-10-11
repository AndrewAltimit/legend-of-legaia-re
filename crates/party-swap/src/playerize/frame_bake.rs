//! Baking animation frames into the swapped rig: the body axes, the weapon
//! swing rotation, and the per-frame bake.

use super::super::*;

/// One rig's own body frame - `[up, lateral, forward]` - from landmarks
/// all six rigs carry: up = pelvis pivot -> head pivot, lateral = the two
/// shoulder pivots, forward = their cross. Unlike a joint's bend plane it
/// never degenerates and never depends on how one rig happens to author a
/// spine.
///
/// `pivots` is indexed in [`CANONICAL_PARTS`] order (0 head, 2 pelvis,
/// 3/6 the two shoulders) - the landmarks are those slots, not a search.
fn body_axes(pivots: &[[f32; 3]]) -> [[f32; 3]; 3] {
    let unit = |v: [f32; 3], fallback: [f32; 3]| {
        let l = vnorm(v);
        if l < 1e-3 {
            fallback
        } else {
            [v[0] / l, v[1] / l, v[2] / l]
        }
    };
    let up = unit(vsub(pivots[0], pivots[2]), [0.0, -1.0, 0.0]);
    let lat = vsub(pivots[6], pivots[3]);
    let d = vdot(lat, up);
    let lat = unit(
        [lat[0] - up[0] * d, lat[1] - up[1] * d, lat[2] - up[2] * d],
        [0.0, 0.0, 1.0],
    );
    [up, lat, unit(vcross(up, lat), [1.0, 0.0, 0.0])]
}

/// The rotation taking unit `a` onto unit `b` about their common
/// perpendicular - the swing that adds no twist of its own (Rodrigues).
/// `None` when the two are antiparallel and that axis is undefined.
fn swing_rotation(a: [f32; 3], b: [f32; 3]) -> Option<[[f32; 3]; 3]> {
    let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let v = vcross(a, b);
    let c = vdot(a, b).clamp(-1.0, 1.0);
    let s = vnorm(v);
    if s < 1e-6 {
        return (c > 0.0).then_some(ident);
    }
    let k = [v[0] / s, v[1] / s, v[2] / s];
    let kx = [[0.0, -k[2], k[1]], [k[2], 0.0, -k[0]], [-k[1], k[0], 0.0]];
    let (st, ct) = s.atan2(c).sin_cos();
    let mut r = ident;
    for (i, row) in r.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            let kk: f32 = (0..3).map(|q| kx[i][q] * kx[q][j]).sum();
            *cell = ident[i][j] + st * kx[i][j] + (1.0 - ct) * kk;
        }
    }
    Some(r)
}

/// The rest bone frames the bake aligns through, with each part's TWIST
/// about its bone referenced to the rig's own body instead of to the
/// joint's bend plane.
///
/// [`bone_frames`] pins the roll with the adjacent chain bone (the elbow /
/// knee bend plane), falling through to a world axis when that chain is
/// straight. Across two independently authored rigs that reference is not
/// comparable, in two ways that both roll the baked part about its own
/// axis:
///
/// * **Reference kinds disagree.** Che is the one sibling whose
///   pelvis->torso bone is measurable (20 units), so HIS torso takes its
///   bend plane from real anatomy while every player torso - pelvis pivot
///   sitting on the torso pivot - falls through to world Z. The two y axes
///   come out at `y . y = -0.98`, so the alignment rolled Che's chest
///   167.9 degrees about his own spine: he wore his torso backwards, and
///   the shoulder tuck, which seats the arms where the SIBLING's sockets
///   land through the torso bake, then dragged both arms round with it.
/// * **Bend planes diverge.** Even with real anatomy on both sides two
///   rigs flex their joints in unrelated directions - Gi's armA elbow
///   plane sits 122 degrees from Noa's - so his upper arm baked rolled 166
///   degrees about its own axis.
///
/// The replacement carries no per-part reference at all: turn the WHOLE
/// rig onto the host's facing (`R_body`, the one rotation taking the
/// sibling's body frame onto the host's), then swing each part minimally
/// onto its own host bone. Writing the destination frame as `R_ideal *
/// F_src` makes [`frame_align`] return exactly that composite. Nothing is
/// left free for a joint plane to get wrong, and a part's roll relative to
/// the body it hangs on is carried across the swap unchanged - which is
/// what the enemy-table preview shows and what the report compared against.
///
/// Terminal parts (head, hands, feet) keep the frame [`bone_frames`]
/// inherited for them: [`normalize_battle_rest_feet`] already pins their
/// world orientation by pre-cancelling *that* frame's alignment, and the
/// two have to describe the same rotation or the cancellation stops
/// cancelling.
///
/// Measured as excess roll - what the bake rotates a part by beyond that
/// re-facing and swing - worst non-terminal part per cast: Gi 152.5 -> 0,
/// Che 114.0 -> 0, Lu (the slot that already read right) 44.3 -> 0. The
/// rotation is right by construction, so the numbers that carry weight are
/// the geometric ones the correction was not fitted to. The per-part
/// affine fit of the bake loses the shear the old roll dragged through the
/// socket tucks - Che's armA upper went from principal scales
/// 1.01/0.75/0.24 (squashed to a quarter of its width in one direction)
/// to 0.81/0.75/0.65, its non-affine residual 0.069 -> 0.020 - and no
/// chain edge opens past BOTH the retail host's own idle envelope and the
/// sibling's, before or after.
///
/// NB [`winpose::retarget_clip`](super::super::winpose) cancels this bake by
/// rebuilding the SAME alignment out of [`bone_frames`]; it has to build
/// its frames here too, or its conjugation stops cancelling by exactly the
/// correction this applies (measured: up to 152.5 degrees on Gi, 114.0 on
/// Che, 44.3 on Lu).
///
/// Both pivot arrays are in [`CANONICAL_PARTS`] order - [`body_axes`] reads
/// its landmarks off fixed slots - so a caller on another rig topology gets
/// the plain [`bone_frames`] pair back by way of the length guard only if
/// that topology is shorter, not if it merely orders its parts differently.
pub(crate) fn bake_frames(
    src_pivots: &[[f32; 3]],
    dst_pivots: &[[f32; 3]],
    child: &[Option<usize>],
    parent: &[Option<usize>],
) -> (Vec<BoneFrame>, Vec<BoneFrame>) {
    let mut src = bone_frames(src_pivots, child, parent);
    let mut dst = bone_frames(dst_pivots, child, parent);
    if src_pivots.len() < CANONICAL_PARTS || dst_pivots.len() < CANONICAL_PARTS {
        return (src, dst);
    }
    let (bs, bd) = (body_axes(src_pivots), body_axes(dst_pivots));
    // Turn the whole rig onto the host's facing: read the vector in the
    // sibling's body coordinates, rebuild it in the host's.
    let reface = |v: [f32; 3]| -> [f32; 3] {
        let c = [vdot(v, bs[0]), vdot(v, bs[1]), vdot(v, bs[2])];
        [
            bd[0][0] * c[0] + bd[1][0] * c[1] + bd[2][0] * c[2],
            bd[0][1] * c[0] + bd[1][1] * c[1] + bd[2][1] * c[2],
            bd[0][2] * c[0] + bd[1][2] * c[1] + bd[2][2] * c[2],
        ]
    };
    for (s, d) in src.iter_mut().zip(dst.iter_mut()) {
        // Only parts with a bone of their own: a terminal's frame is
        // inherited, and is the one the feet normalisation cancels.
        if s.len.is_none() || d.len.is_none() || !s.real || !d.real {
            continue;
        }
        let xd = d.axes[0];
        // The re-faced source bone. Antiparallel to the host's leaves the
        // swing axis undefined; such a part keeps the bend-plane frame.
        let Some(swing) = swing_rotation(reface(s.axes[0]), xd) else {
            continue;
        };
        let yd = apply(&swing, reface(s.axes[1]));
        d.axes = [xd, yd, vcross(xd, yd)];
    }
    (src, dst)
}

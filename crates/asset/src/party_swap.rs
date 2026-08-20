//! Party <-> Delilas battle-model swap: rebuild a playable character's
//! assembled battle model on a Delilas monster rig (and, in the sibling
//! module direction, a Delilas model on a player rig).
//!
//! Both animation systems share one pose model: flat per-part rigid
//! transforms addressed **by part index** (`docs/formats/monster-animation.md`,
//! player streams at entry `+0xAC` per `battle_char_assembly::animation`).
//! A cross-swap therefore reduces to:
//!
//! 1. **Anatomy permutation.** Player rigs order their 15 skeleton bones
//!    `[torso, pelvis, head, armA x3, armB x3, legA x3, legB x3]`
//!    (Noa inserts a 16th hair bone at channel 3); every Delilas mesh
//!    orders its 15 parts `[head, torso, pelvis, armA x3, armB x3,
//!    legA x3, legB x3]`. Measured from the rest-pose world centroids of
//!    all six rigs - see `docs/tooling/randomizer.md` § Delilas party swap.
//! 2. **Extras merge.** The player's two equipment-visual objects ride
//!    their attach bone's pose channel exactly, so merging their geometry
//!    into the attach bone's object is pose-exact (same local frame).
//!    Noa's hair object rides its own channel and is rebased into the
//!    head's rest frame instead (rigid approximation).
//! 3. **Pivot-anchored rest-pose bake.** Local part frames are per-rig
//!    conventions (a retail player mesh even shares one mesh between its
//!    left and right limb chains, mirrored purely by pose rotations), so
//!    each part's geometry is re-expressed through `source rest pose ->
//!    target rest pose` anchored at each part's rest PIVOT - the joint
//!    the engine rotates the part about - with the bone frames aligned
//!    and the part's length scaled onto the target's joint-to-joint span
//!    (see `bake_object_pivot`). Pivot anchoring is load-bearing: any
//!    other anchor leaves a lever arm that scatters the mesh under every
//!    clip whose rotations differ from the rest pose.
//! 4. **Texture re-layout.** Both texture systems are 4bpp indices +
//!    16-colour CLUTs, so no quantization happens in either direction:
//!    used texel islands are copied bit-exact between the player band
//!    pages (`battle_char_assembly::texture`) and the monster pool page,
//!    CLUTs are carried over (player upload semantics apply the STP pass
//!    at load; the monster pool stores entries final, so the copy ORs
//!    `0x8000` onto non-zero entries), and every textured prim's UV /
//!    CBA / TSB is rewritten to the target's authoring conventions.

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;

use crate::battle_char_assembly::{self, AUTHORING_FIRST_TEXPAGE, SECTION_COUNT, TextureUpload};
use crate::battle_data_pack;
use crate::monster_archive::{self, PartPose};
use crate::monster_model::{CBA_BASE, CLUT_COUNT, CLUT_REGION_BYTES, PAGE_HEIGHT, UV_SPACE};
use legaia_tmd::encode::{ModelGroup, ModelObject, decode_model, encode};

pub mod enemy_anim;
pub mod fieldize;
pub mod moveset;
pub mod playerize;
pub mod winpose;

/// Canonical part order of the swap = the Delilas mesh order shared by all
/// three siblings: `[head, torso, pelvis, armA(u,f,h), armB(u,f,h),
/// legA(t,s,f), legB(t,s,f)]`.
pub const CANONICAL_PARTS: usize = 15;

/// A playable character's battle rig, as the swap needs it: which player
/// pose channel carries each canonical part, plus the hair channel Noa
/// alone has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerRig {
    /// `channel_for_canonical[c]` = the player skeleton channel driving
    /// canonical part `c`.
    pub channel_for_canonical: [u8; CANONICAL_PARTS],
    /// Noa's hair channel (merged into the head part); `None` elsewhere.
    pub hair_channel: Option<u8>,
}

/// Vahn (player file PROT 0863) and Gala (0865): 15 bones,
/// `[torso, pelvis, head, armA x3, armB x3, legA x3, legB x3]`.
pub const RIG_VAHN_GALA: PlayerRig = PlayerRig {
    channel_for_canonical: [2, 0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14],
    hair_channel: None,
};

/// Noa (player file PROT 0864): 16 bones - channel 3 is her hair, the
/// limb chains shift up by one.
pub const RIG_NOA: PlayerRig = PlayerRig {
    channel_for_canonical: [2, 0, 1, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    hair_channel: Some(3),
};

/// The enemy-side conversion result: a player model rebuilt on a Delilas
/// monster rig, ready for `monster_archive::replace_mesh_and_pool`.
#[derive(Debug, Clone)]
pub struct MonsterizedPlayer {
    /// Legaia TMD bytes, [`CANONICAL_PARTS`] objects in Delilas part order.
    pub tmd: Vec<u8>,
    /// Monster texture pool (`[15 CLUTs][4bpp 256x256 page]`).
    pub pool: Vec<u8>,
    /// Non-fatal notes (island downscales, dropped palettes, ...).
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Pose math (shared with the probe measurements): PSX rotation order
// Rz * Ry * Rx, angles in 1/4096 turns, y-down GTE space.

fn rot_matrix(p: &PartPose) -> [[f32; 3]; 3] {
    let rad = |r: u16| (r as f32) * std::f32::consts::TAU / 4096.0;
    let (sx, cx) = rad(p.rx).sin_cos();
    let (sy, cy) = rad(p.ry).sin_cos();
    let (sz, cz) = rad(p.rz).sin_cos();
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
}

fn apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn apply_transposed(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
    ]
}

/// Keep a battle-bake source rest's FEET at their authored WORLD
/// orientation through the stance realignment. A terminal foot rides
/// its shin's bone frame, and the shin's frame alignment encodes the
/// STANCE DIFFERENCE between the monster idle and the player idle (a
/// near-vertical sibling shin re-aims onto the player's angled
/// fighting-stance shin) - so the foot, dragged rigidly along, pitches
/// by that whole delta (Lu's flat front foot played with its toe ~50
/// units up; her authored raise is ~5). Pre-rotating the foot channel
/// by the alignment's INVERSE cancels the drag: at player idle the
/// baked foot plays back at exactly its authored world orientation.
/// Pivots (and therefore the frames themselves) are untouched.
///
/// Must be applied to the SAME rest by both the mesh bake
/// (`playerize`) and the win-pose conversion (`winpose`): the
/// conversion's conjugation cancels the source rest exactly, so
/// converted streams keep their authored look either way.
pub(crate) fn normalize_battle_rest_feet(
    src_rest: &mut [PartPose],
    dst_rest: &[PartPose],
    rig: &PlayerRig,
) {
    use winpose::{mmul, to_euler, transpose};
    if src_rest.len() < CANONICAL_PARTS {
        return;
    }
    let pivot_of = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
    let src_pivots: Vec<[f32; 3]> = src_rest
        .iter()
        .take(CANONICAL_PARTS)
        .map(pivot_of)
        .collect();
    let Some(dst_pivots) = (0..CANONICAL_PARTS)
        .map(|c| {
            dst_rest
                .get(rig.channel_for_canonical[c] as usize)
                .map(pivot_of)
        })
        .collect::<Option<Vec<[f32; 3]>>>()
    else {
        return;
    };
    let src_frames = bone_frames(&src_pivots, &CANONICAL_CHILD, &CANONICAL_PARENT);
    let dst_frames = bone_frames(&dst_pivots, &CANONICAL_CHILD, &CANONICAL_PARENT);
    // The COMPLETE terminal family: head, hands, feet - every part with
    // no child pivot drags the whole stance delta through its parent's
    // frame alignment. A foot pitched its toe up (Lu); a hand swung
    // Che's club into space above his shoulder; Che's HEAD - riding his
    // hunched torso's bone frame re-aimed onto Gala's upright one -
    // played looking sideways (the in-game screenshot's misaligned
    // face; the head was the one family member left unnormalized).
    for term in [0usize, 5, 8, 11, 14] {
        let a = frame_align(&src_frames[term], &dst_frames[term]);
        let m = mmul(&transpose(&a), &rot_matrix(&src_rest[term]));
        let (rx, ry, rz) = to_euler(&m);
        src_rest[term].rx = rx;
        src_rest[term].ry = ry;
        src_rest[term].rz = rz;
    }
}

fn round_coord(v: f32) -> Result<i16> {
    let r = v.round();
    if !(i16::MIN as f32..=i16::MAX as f32).contains(&r) {
        bail!("baked coordinate {r} out of the i16 GTE range");
    }
    Ok(r as i16)
}

/// Rest-pose world bbox centre + per-axis extents of one posed part -
/// feeds the whole-rig height ratio (the bake's radial scale).
fn part_world_stats(o: &ModelObject, p: &PartPose) -> PartStats {
    // Only prim-REFERENCED vertices count: retail objects carry stray
    // unreferenced vertices (a hand part shipping far-off orphans skewed
    // its bbox by ~50 units), and the anchor must reflect what draws.
    let mut used = vec![false; o.vertices.len()];
    for g in &o.groups {
        for prim in &g.prims {
            for &vi in &prim.vertices {
                if let Some(u) = used.get_mut(vi as usize) {
                    *u = true;
                }
            }
        }
    }
    if !used.iter().any(|&u| u) {
        used.fill(true);
    }
    let m = rot_matrix(p);
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for (v, _) in o.vertices.iter().zip(&used).filter(|&(_, &u)| u) {
        let w = apply(&m, [v[0] as f32, v[1] as f32, v[2] as f32]);
        let w = [w[0] + p.tx as f32, w[1] + p.ty as f32, w[2] + p.tz as f32];
        for k in 0..3 {
            min[k] = min[k].min(w[k]);
            max[k] = max[k].max(w[k]);
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
    PartStats { center, ext }
}

/// One posed part's rest-pose world statistics.
#[derive(Debug, Clone, Copy)]
struct PartStats {
    /// bbox centre.
    center: (f32, f32, f32),
    /// per-axis bbox extents.
    ext: [f32; 3],
}

/// Uniform battle-side scale: the height ratio of the two rigs' whole
/// rest-pose bboxes. Battle limbs scale UNIFORMLY - per-axis spans
/// visibly distort the sibling's part shapes (the user matched the
/// swapped model against the enemy-table original and saw the
/// difference), and the battle parts are large enough that mass-centred
/// placement alone keeps the chains reading connected.
fn global_height_scale(stats_src: &[PartStats], stats_dst: &[PartStats]) -> [f32; 3] {
    let span = |stats: &[PartStats]| {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for st in stats {
            lo = lo.min(st.center.1 - st.ext[1] / 2.0);
            hi = hi.max(st.center.1 + st.ext[1] / 2.0);
        }
        (hi - lo).max(1.0)
    };
    let s = (span(stats_dst) / span(stats_src)).clamp(0.25, 4.0);
    [s; 3]
}
/// Canonical chain child per part: 0=head, 1=torso, 2=pelvis, 3/4/5
/// armA upper/fore/hand, 6/7/8 armB, 9/10/11 legA thigh/shin/foot,
/// 12/13/14 legB. Terminal parts (head, hands, feet) have no child.
pub(crate) const CANONICAL_CHILD: [Option<usize>; CANONICAL_PARTS] = [
    None,
    Some(0),
    Some(1),
    Some(4),
    Some(5),
    None,
    Some(7),
    Some(8),
    None,
    Some(10),
    Some(11),
    None,
    Some(13),
    Some(14),
    None,
];

/// Chain parent per canonical part (chain-internal only - the shoulder
/// and hip attachments are not chain edges).
pub(crate) const CANONICAL_PARENT: [Option<usize>; CANONICAL_PARTS] = [
    Some(1),
    Some(2),
    None,
    None,
    Some(3),
    Some(4),
    None,
    Some(6),
    Some(7),
    None,
    Some(9),
    Some(10),
    None,
    Some(12),
    Some(13),
];

fn vsub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn vdot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn vcross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn vnorm(a: [f32; 3]) -> f32 {
    vdot(a, a).sqrt()
}

/// One part's rest bone frame: columns `[x, y, z]` with `x` along the
/// bone (pivot -> child pivot), `y` in the bend plane, plus the bone
/// length. Terminal parts inherit their chain parent's frame.
#[derive(Clone, Copy)]
pub(crate) struct BoneFrame {
    /// Column-major orthonormal frame: `axes[0]` = bone axis.
    axes: [[f32; 3]; 3],
    /// Pivot-to-child-pivot distance; `None` on terminal parts.
    len: Option<f32>,
    /// False only for the world-axes fallback of a part with neither an
    /// own bone nor an inheritable parent frame. A fallback frame's axes
    /// carry no anatomy, so aligning against one manufactures a rotation
    /// onto arbitrary world axes - Che is the one sibling whose pelvis
    /// has a measurable pelvis->torso bone (20 units) while every player
    /// pelvis is degenerate, and that asymmetry baked his pelvis ~90 deg
    /// off. [`frame_align`] refuses to align when either side is a
    /// fallback.
    real: bool,
}

/// Build per-part rest bone frames from the rig's rest **pivots** (each
/// channel's rest-pose translation - the world point the part rotates
/// about). The frame's `x` axis is the bone (pivot to child pivot); the
/// `y` reference prefers the adjacent chain bone (child's own bone,
/// else the parent bone) so the bend plane - elbow/knee - is part of
/// the frame, which is what pins the twist DOF a single-axis minimal
/// rotation leaves free (the "arm angled completely wrong" failure).
/// Degenerate references (straight chains, spines) fall back to world
/// axes, consistently on both rigs.
pub(crate) fn bone_frames(
    pivots: &[[f32; 3]],
    child: &[Option<usize>],
    parent: &[Option<usize>],
) -> Vec<BoneFrame> {
    let ident = BoneFrame {
        axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        len: None,
        real: false,
    };
    let n = pivots.len();
    let bone = |i: usize| -> Option<([f32; 3], f32)> {
        let k = child.get(i).copied().flatten()?;
        let b = vsub(pivots[k], pivots[i]);
        let l = vnorm(b);
        (l >= 2.0).then(|| ([b[0] / l, b[1] / l, b[2] / l], l))
    };
    let mut frames: Vec<Option<BoneFrame>> = vec![None; n];
    for i in 0..n {
        let Some((x, l)) = bone(i) else { continue };
        // Bend-plane reference: child's bone, else parent's bone, else
        // the world axis least parallel to the bone.
        let mut refs: Vec<[f32; 3]> = Vec::new();
        if let Some((d, _)) = child[i].and_then(&bone) {
            refs.push(d);
        }
        if let Some((d, _)) = parent[i].and_then(&bone) {
            refs.push(d);
        }
        refs.extend([[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        let mut axes = None;
        for r in refs {
            let perp = vsub(r, [x[0] * vdot(r, x), x[1] * vdot(r, x), x[2] * vdot(r, x)]);
            let pl = vnorm(perp);
            if pl > 0.25 {
                let y = [perp[0] / pl, perp[1] / pl, perp[2] / pl];
                axes = Some([x, y, vcross(x, y)]);
                break;
            }
        }
        if let Some(axes) = axes {
            frames[i] = Some(BoneFrame {
                axes,
                len: Some(l),
                real: true,
            });
        }
    }
    // Terminals (and degenerate bones) ride their chain parent's frame;
    // a part with neither gets the identity.
    (0..n)
        .map(|i| {
            frames[i]
                .or_else(|| {
                    parent
                        .get(i)
                        .copied()
                        .flatten()
                        .and_then(|p| frames[p])
                        .map(|f| BoneFrame { len: None, ..f })
                })
                .unwrap_or(ident)
        })
        .collect()
}

/// The rotation taking source frame axes onto destination frame axes:
/// `R = F_dst * F_src^T` (both proper rotations, so this is one too).
/// When either frame is the world-axes fallback the alignment is the
/// identity: a fallback's axes are not anatomy, and aligning a real
/// bone frame against one rotates the part onto arbitrary world axes
/// (the Che pelvis defect - see [`BoneFrame::real`]).
pub(crate) fn frame_align(src: &BoneFrame, dst: &BoneFrame) -> [[f32; 3]; 3] {
    if !src.real || !dst.real {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let mut r = [[0.0f32; 3]; 3];
    for (row, r_row) in r.iter_mut().enumerate() {
        for (col, cell) in r_row.iter_mut().enumerate() {
            // (F_d * F_s^T)[row][col] = sum_k F_d[row][k] * F_s[col][k];
            // axes are stored column-major (axes[k] = column k).
            *cell = (0..3).map(|k| dst.axes[k][row] * src.axes[k][col]).sum();
        }
    }
    r
}

/// Per-part pivot-anchored bake parameters: the alignment rotation, the
/// destination bone axis (the axial-scale direction), and the two scale
/// factors.
pub(crate) struct PivotBake {
    r_align: [[f32; 3]; 3],
    x_dst: [f32; 3],
    axial: f32,
    radial: f32,
}

/// Compute one part's [`PivotBake`]: frames aligned, axial scale =
/// destination bone length over source bone length (so the part's far
/// end lands exactly on the destination's child joint - the joint gaps
/// were parts keeping the source's limb proportions), radial scale =
/// the uniform whole-rig ratio (the sibling's shapes survive). Terminal
/// parts scale uniformly.
// NB a torso-uniform variant (axial = radial) was tried and REVERTED:
// pretty at rest (Che's slab kept its proportions, and the idle-frame
// enemy-table diff scored it closer), but the host's head and shoulder
// pivots are authored at the torso BONE's end - a uniform long torso
// towers past them, so in-game the head poked out mid-hump with the
// shoulder pads above it. Attachment beats shape: the torso keeps the
// axial fit.
pub(crate) fn pivot_bake_params(src: &BoneFrame, dst: &BoneFrame, radial: f32) -> PivotBake {
    let axial = match (src.len, dst.len) {
        (Some(ls), Some(ld)) if ls >= 2.0 => (ld / ls).clamp(0.25, 4.0),
        _ => radial,
    };
    PivotBake {
        r_align: frame_align(src, dst),
        x_dst: dst.axes[0],
        axial,
        radial,
    }
}

/// Where a source-rig WORLD point (a joint pivot riding a baked part)
/// lands at destination rest, through the same pivot-anchored
/// transform [`bake_object_pivot`] applies to geometry (align about
/// the anchor, axial/radial scale, re-anchor on the destination
/// pivot). The `md^T` un-pose and rest-playback `md` cancel, so the
/// destination-rest world position is direct.
pub(crate) fn bake_point_pivot(
    w: [f32; 3],
    anchor_src: [f32; 3],
    dst_pivot: [f32; 3],
    pb: &PivotBake,
) -> [f32; 3] {
    let d = vsub(w, anchor_src);
    let e = apply(&pb.r_align, d);
    let t = vdot(e, pb.x_dst);
    [
        dst_pivot[0] + pb.x_dst[0] * (pb.axial * t) + pb.radial * (e[0] - pb.x_dst[0] * t),
        dst_pivot[1] + pb.x_dst[1] * (pb.axial * t) + pb.radial * (e[1] - pb.x_dst[1] * t),
        dst_pivot[2] + pb.x_dst[2] * (pb.axial * t) + pb.radial * (e[2] - pb.x_dst[2] * t),
    ]
}

/// Prim-referenced vertex mask (retail objects carry stray orphan
/// vertices that must not influence any measurement).
fn used_verts(o: &ModelObject) -> Vec<bool> {
    let mut used = vec![false; o.vertices.len()];
    for g in &o.groups {
        for prim in &g.prims {
            for &vi in &prim.vertices {
                if let Some(u) = used.get_mut(vi as usize) {
                    *u = true;
                }
            }
        }
    }
    if !used.iter().any(|&u| u) {
        used.fill(true);
    }
    used
}

/// Seat a baked TERMINAL part along its bone axis: slide the baked
/// geometry (already in `dst`'s local space) so its near edge along the
/// destination bone axis sits where the destination part's own near
/// edge sat. Shape-preserving - a head whose chin starts higher above
/// its neck pivot than the replaced head's did otherwise leaves a bare
/// neck gap no scale can close without distorting it.
pub(crate) fn seat_terminal_axial(
    baked: &mut ModelObject,
    dst_obj: &ModelObject,
    dst_pose: &PartPose,
    x_dst: [f32; 3],
) -> Result<()> {
    let md = rot_matrix(dst_pose);
    // The bone axis expressed in the destination part's local space.
    let a = apply_transposed(&md, x_dst);
    let near = |o: &ModelObject| -> Option<f32> {
        let used = used_verts(o);
        o.vertices
            .iter()
            .zip(&used)
            .filter(|&(_, &u)| u)
            .map(|(v, _)| a[0] * v[0] as f32 + a[1] * v[1] as f32 + a[2] * v[2] as f32)
            .min_by(|x, y| x.partial_cmp(y).unwrap())
    };
    let (Some(b), Some(d)) = (near(baked), near(dst_obj)) else {
        return Ok(());
    };
    // Sink slightly PAST the destination's near edge (a tenth of the
    // destination part's own axial span): a transplant with a slimmer
    // skull base bares the neck during pitch clips even with the near
    // edges flush - the pose-worst neck gap measured ~1.7x retail's at
    // flush, retail-equivalent with the extra overlap.
    let far = {
        let used = used_verts(dst_obj);
        dst_obj
            .vertices
            .iter()
            .zip(&used)
            .filter(|&(_, &u)| u)
            .map(|(v, _)| a[0] * v[0] as f32 + a[1] * v[1] as f32 + a[2] * v[2] as f32)
            .max_by(|x, y| x.partial_cmp(y).unwrap())
            .unwrap_or(d)
    };
    let shift = (d - b) - 0.10 * (far - d).abs();
    for v in baked.vertices.iter_mut() {
        *v = [
            round_coord(v[0] as f32 + a[0] * shift)?,
            round_coord(v[1] as f32 + a[1] * shift)?,
            round_coord(v[2] as f32 + a[2] * shift)?,
        ];
    }
    Ok(())
}

/// Pivot-anchored bake: the part's geometry, expressed relative to its
/// SOURCE rest pivot, is re-aimed into the destination's rest bone
/// frame, scaled (axial along the destination bone, radial across it),
/// and written into the destination part's local space:
/// `v' = R_dst^T * S(R_align * ((R_src * v + T_src) - anchor_src))`.
/// The pivot is the point the engine rotates the part about, so
/// anchoring there (not at a bbox or mass centre) is what makes every
/// clip move the baked part exactly like the geometry it replaced.
/// `anchor_src` is the owning bone's pivot - for a part merged into
/// another bone's role it differs from the part's own `T_src`.
pub(crate) fn bake_object_pivot(
    o: &mut ModelObject,
    src: &PartPose,
    anchor_src: [f32; 3],
    dst: &PartPose,
    pb: &PivotBake,
) -> Result<()> {
    let ms = rot_matrix(src);
    let md = rot_matrix(dst);
    for v in o.vertices.iter_mut() {
        let w = apply(&ms, [v[0] as f32, v[1] as f32, v[2] as f32]);
        let d = [
            w[0] + src.tx as f32 - anchor_src[0],
            w[1] + src.ty as f32 - anchor_src[1],
            w[2] + src.tz as f32 - anchor_src[2],
        ];
        let e = apply(&pb.r_align, d);
        let t = vdot(e, pb.x_dst);
        let e = [
            pb.x_dst[0] * (pb.axial * t) + pb.radial * (e[0] - pb.x_dst[0] * t),
            pb.x_dst[1] * (pb.axial * t) + pb.radial * (e[1] - pb.x_dst[1] * t),
            pb.x_dst[2] * (pb.axial * t) + pb.radial * (e[2] - pb.x_dst[2] * t),
        ];
        let l = apply_transposed(&md, e);
        *v = [round_coord(l[0])?, round_coord(l[1])?, round_coord(l[2])?];
    }
    Ok(())
}

/// Lossless object compaction: dedup identical vertex positions (prim
/// indices remapped) and merge same-render-state groups. The section
/// splice + extras merge leaves both kinds of slack, and the compressed
/// archive slot is a hard 0x14000 budget.
fn compact_object(o: &mut ModelObject) {
    // Vertex dedup by exact position.
    let mut first: BTreeMap<[i16; 3], u16> = BTreeMap::new();
    let mut remap: Vec<u16> = Vec::with_capacity(o.vertices.len());
    let mut kept: Vec<[i16; 3]> = Vec::new();
    for v in &o.vertices {
        let id = *first.entry(*v).or_insert_with(|| {
            kept.push(*v);
            (kept.len() - 1) as u16
        });
        remap.push(id);
    }
    o.vertices = kept;
    for g in o.groups.iter_mut() {
        for p in g.prims.iter_mut() {
            for vi in p.vertices.iter_mut() {
                *vi = remap[*vi as usize];
            }
        }
    }
    // Group merge: same (shape, semi) groups concatenate.
    let mut merged: Vec<ModelGroup> = Vec::new();
    for g in o.groups.drain(..) {
        if let Some(last) = merged
            .last_mut()
            .filter(|m| m.shape == g.shape && m.semi_transparent == g.semi_transparent)
        {
            last.prims.extend(g.prims);
        } else {
            merged.push(g);
        }
    }
    // Non-adjacent same-state groups too (order across groups does not
    // matter for opaque draws; semi groups keep their relative order).
    let mut out: Vec<ModelGroup> = Vec::new();
    for g in merged {
        if let Some(prev) = out.iter_mut().find(|m| {
            m.shape == g.shape && m.semi_transparent == g.semi_transparent && !g.semi_transparent
        }) {
            prev.prims.extend(g.prims);
        } else {
            out.push(g);
        }
    }
    o.groups = out;
}

/// Append `src`'s geometry into `dst` (vertex indices rebased).
fn merge_object(dst: &mut ModelObject, src: &ModelObject) {
    let base = dst.vertices.len() as u16;
    dst.vertices.extend_from_slice(&src.vertices);
    for g in &src.groups {
        let mut g2 = g.clone();
        for p in g2.prims.iter_mut() {
            for vi in p.vertices.iter_mut() {
                *vi += base;
            }
        }
        dst.groups.push(g2);
    }
}

/// Rebase `src` (posed by `src_pose`) into `dst_pose`'s local frame and
/// merge it into `dst` - the rigid approximation used for Noa's hair:
/// `v' = R_dst^T (R_src v + T_src - T_dst)`.
fn rebase_merge(
    dst: &mut ModelObject,
    dst_pose: &PartPose,
    src: &ModelObject,
    src_pose: &PartPose,
) -> Result<()> {
    let ms = rot_matrix(src_pose);
    let md = rot_matrix(dst_pose);
    let mut moved = src.clone();
    for v in moved.vertices.iter_mut() {
        let w = apply(&ms, [v[0] as f32, v[1] as f32, v[2] as f32]);
        let rel = [
            w[0] + (src_pose.tx - dst_pose.tx) as f32,
            w[1] + (src_pose.ty - dst_pose.ty) as f32,
            w[2] + (src_pose.tz - dst_pose.tz) as f32,
        ];
        let l = apply_transposed(&md, rel);
        *v = [round_coord(l[0])?, round_coord(l[1])?, round_coord(l[2])?];
    }
    merge_object(dst, &moved);
    Ok(())
}

// ---------------------------------------------------------------------------
// Source texel space: the player band's two authoring pages.

/// One 256x256 4bpp page as decoded texel indices + presence.
struct Page {
    indices: Vec<u8>,
    present: Vec<bool>,
}

impl Page {
    fn new() -> Self {
        Page {
            indices: vec![0; UV_SPACE * PAGE_HEIGHT],
            present: vec![false; UV_SPACE * PAGE_HEIGHT],
        }
    }
}

/// The player band in authoring space: page 0x15 / 0x16 bitmaps plus the
/// CLUT row keyed by CBA column (`cba & 0x3F`).
struct BandTexels {
    pages: [Page; 2],
    /// Palette per CBA column, STP pass applied (`e |= 0x8000` on
    /// non-zero entries - the retail upload semantics).
    palettes: BTreeMap<u8, [u16; 16]>,
}

fn band_texels(uploads: &[TextureUpload]) -> BandTexels {
    let mut pages = [Page::new(), Page::new()];
    let mut palettes: BTreeMap<u8, [u16; 16]> = BTreeMap::new();
    for u in uploads {
        // Pixel half: band-relative rect, page split at halfword 0x40.
        let hw_w = u.rect.w as usize;
        for row in 0..u.rect.h as usize {
            for hw in 0..hw_w {
                let abs_hw = u.rect.x0 as usize + hw;
                let (page, page_hw) = if abs_hw < 0x40 {
                    (0usize, abs_hw)
                } else {
                    (1usize, abs_hw - 0x40)
                };
                let src = (row * hw_w + hw) * 2;
                let Some(&lo) = u.pixels.get(src) else {
                    continue;
                };
                let hi = u.pixels.get(src + 1).copied().unwrap_or(0);
                let y = u.rect.y0 as usize + row;
                if y >= PAGE_HEIGHT || page_hw >= 0x40 {
                    continue;
                }
                let word = u16::from_le_bytes([lo, hi]);
                for t in 0..4 {
                    let x = page_hw * 4 + t;
                    let idx = ((word >> (t * 4)) & 0xF) as u8;
                    let at = y * UV_SPACE + x;
                    pages[page].indices[at] = idx;
                    pages[page].present[at] = true;
                }
            }
        }
        // CLUT half: entries land at clut_x on the runtime row; the CBA
        // column addresses the same x in 16-halfword steps.
        if !u.clut.is_empty() && u.clut_x % 16 == 0 {
            let first_col = (u.clut_x / 16) as u8;
            for (chunk_i, chunk) in u.clut.chunks(16).enumerate() {
                let mut pal = [0u16; 16];
                for (i, &e) in chunk.iter().enumerate() {
                    pal[i] = if e == 0 { 0 } else { e | 0x8000 };
                }
                palettes.insert(first_col + chunk_i as u8, pal);
            }
        }
    }
    BandTexels { pages, palettes }
}

// ---------------------------------------------------------------------------
// Texture re-layout: cluster textured faces by UV-bbox overlap, shelf-pack
// the clusters into the target page, copy indices, rewrite UV/CBA/TSB.

struct FaceRef {
    obj: usize,
    group: usize,
    prim: usize,
    page: usize,
    bbox: (u8, u8, u8, u8), // (x0, y0, x1, y1) inclusive
}

fn face_bbox(uvs: &[(u8, u8)]) -> (u8, u8, u8, u8) {
    let mut b = (u8::MAX, u8::MAX, 0u8, 0u8);
    for &(u, v) in uvs {
        b.0 = b.0.min(u);
        b.1 = b.1.min(v);
        b.2 = b.2.max(u);
        b.3 = b.3.max(v);
    }
    b
}

fn boxes_touch(a: (u8, u8, u8, u8), b: (u8, u8, u8, u8)) -> bool {
    // Inflate by 1 texel so the importer-style dilation ring can't split
    // an island across two clusters.
    let (ax0, ay0, ax1, ay1) = (
        a.0 as i32 - 1,
        a.1 as i32 - 1,
        a.2 as i32 + 1,
        a.3 as i32 + 1,
    );
    let (bx0, by0, bx1, by1) = (b.0 as i32, b.1 as i32, b.2 as i32, b.3 as i32);
    ax0 <= bx1 && bx0 <= ax1 && ay0 <= by1 && by0 <= ay1
}

struct Cluster {
    faces: Vec<usize>,
    page: usize,
    bbox: (u8, u8, u8, u8),
    /// Source-space downscale divisor (1 = 1:1, 2 = half resolution).
    scale: u32,
    /// Placement in the target page.
    dst: (usize, usize),
}

fn union_bbox(a: (u8, u8, u8, u8), b: (u8, u8, u8, u8)) -> (u8, u8, u8, u8) {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

/// Shelf-pack `w x h` rects (sorted tallest-first by the caller) into a
/// `UV_SPACE x PAGE_HEIGHT` page. Returns placements or `None` on overflow.
fn shelf_pack(sizes: &[(usize, usize)]) -> Option<Vec<(usize, usize)>> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i].1));
    let mut out = vec![(0usize, 0usize); sizes.len()];
    let (mut x, mut y, mut shelf_h) = (0usize, 0usize, 0usize);
    for &i in &order {
        let (w, h) = sizes[i];
        if w > UV_SPACE {
            return None;
        }
        if x + w > UV_SPACE {
            y += shelf_h;
            x = 0;
            shelf_h = 0;
        }
        if y + h > PAGE_HEIGHT {
            return None;
        }
        out[i] = (x, y);
        x += w;
        shelf_h = shelf_h.max(h);
    }
    Some(out)
}

/// Re-layout every textured prim of `objects` from the player band into a
/// fresh 256x256 monster page. Rewrites UVs and CBA/TSB in place; returns
/// the finished pool.
fn relayout_to_monster_pool(
    objects: &mut [ModelObject],
    band: &BandTexels,
    base_scale: u32,
    warnings: &mut Vec<String>,
) -> Result<Vec<u8>> {
    // Collect textured faces.
    let mut faces: Vec<FaceRef> = Vec::new();
    for (oi, o) in objects.iter().enumerate() {
        for (gi, g) in o.groups.iter().enumerate() {
            if !g.shape.is_textured() {
                continue;
            }
            for (pi, p) in g.prims.iter().enumerate() {
                let page = if p.tsb & 0x1F == AUTHORING_FIRST_TEXPAGE {
                    0
                } else {
                    1
                };
                faces.push(FaceRef {
                    obj: oi,
                    group: gi,
                    prim: pi,
                    page,
                    bbox: face_bbox(&p.uvs),
                });
            }
        }
    }

    // Union-find clustering by inflated-bbox overlap, per page.
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn find(parent: &mut Vec<usize>, i: usize) -> usize {
        if parent[i] != i {
            let r = find(parent, parent[i]);
            parent[i] = r;
        }
        parent[i]
    }
    // O(n^2) over ~1k faces - fine.
    for i in 0..faces.len() {
        for j in (i + 1)..faces.len() {
            if faces[i].page == faces[j].page && boxes_touch(faces[i].bbox, faces[j].bbox) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut clusters: BTreeMap<usize, Cluster> = BTreeMap::new();
    #[allow(clippy::needless_range_loop)] // `find` needs `&mut parent` alongside the index
    for i in 0..faces.len() {
        let root = find(&mut parent, i);
        let e = clusters.entry(root).or_insert(Cluster {
            faces: Vec::new(),
            page: faces[i].page,
            bbox: faces[i].bbox,
            scale: base_scale,
            dst: (0, 0),
        });
        e.bbox = union_bbox(e.bbox, faces[i].bbox);
        e.faces.push(i);
    }
    let mut clusters: Vec<Cluster> = clusters.into_values().collect();

    // Pack; on overflow halve the largest clusters until it fits.
    loop {
        let sizes: Vec<(usize, usize)> = clusters
            .iter()
            .map(|c| {
                let w = (c.bbox.2 - c.bbox.0) as usize + 1;
                let h = (c.bbox.3 - c.bbox.1) as usize + 1;
                (w.div_ceil(c.scale as usize), h.div_ceil(c.scale as usize))
            })
            .collect();
        if let Some(placed) = shelf_pack(&sizes) {
            for (c, p) in clusters.iter_mut().zip(placed) {
                c.dst = p;
            }
            break;
        }
        // Halve the largest un-halved cluster (cap at base/4).
        let cap = base_scale * 4;
        let Some(big) = clusters
            .iter_mut()
            .filter(|c| c.scale < cap)
            .max_by_key(|c| {
                let w = (c.bbox.2 - c.bbox.0) as usize + 1;
                let h = (c.bbox.3 - c.bbox.1) as usize + 1;
                (w / c.scale as usize) * (h / c.scale as usize)
            })
        else {
            bail!("texture islands exceed the 256x256 monster page even at quarter resolution");
        };
        big.scale *= 2;
        warnings.push(format!(
            "texture island {}x{} downscaled to 1/{} to fit the monster page",
            big.bbox.2 - big.bbox.0 + 1,
            big.bbox.3 - big.bbox.1 + 1,
            big.scale
        ));
    }

    // Palette mapping: CBA columns used by textured faces -> pool slots.
    let mut col_to_slot: BTreeMap<u8, u8> = BTreeMap::new();
    for f in &faces {
        let col = (objects[f.obj].groups[f.group].prims[f.prim].cba & 0x3F) as u8;
        let next = col_to_slot.len() as u8;
        col_to_slot.entry(col).or_insert(next);
    }
    if col_to_slot.len() > CLUT_COUNT {
        bail!(
            "player mesh samples {} palettes - the monster pool holds {CLUT_COUNT}",
            col_to_slot.len()
        );
    }

    // Copy indices + rewrite prims.
    let mut page_indices = vec![0u8; UV_SPACE * PAGE_HEIGHT];
    for c in &clusters {
        let (sw, sh) = (
            (c.bbox.2 - c.bbox.0) as usize + 1,
            (c.bbox.3 - c.bbox.1) as usize + 1,
        );
        let s = c.scale as usize;
        for dy in 0..sh.div_ceil(s) {
            for dx in 0..sw.div_ceil(s) {
                let sx = c.bbox.0 as usize + dx * s;
                let sy = c.bbox.1 as usize + dy * s;
                let (tx, ty) = (c.dst.0 + dx, c.dst.1 + dy);
                if sx >= UV_SPACE || sy >= PAGE_HEIGHT || tx >= UV_SPACE || ty >= PAGE_HEIGHT {
                    continue;
                }
                page_indices[ty * UV_SPACE + tx] = band.pages[c.page].indices[sy * UV_SPACE + sx];
            }
        }
        for &fi in &c.faces {
            let f = &faces[fi];
            let p = &mut objects[f.obj].groups[f.group].prims[f.prim];
            for uv in p.uvs.iter_mut() {
                let nx = c.dst.0 + (uv.0 as usize - c.bbox.0 as usize) / s;
                let ny = c.dst.1 + (uv.1 as usize - c.bbox.1 as usize) / s;
                *uv = (nx.min(UV_SPACE - 1) as u8, ny.min(PAGE_HEIGHT - 1) as u8);
            }
            let col = (p.cba & 0x3F) as u8;
            let slot = col_to_slot[&col] as u16;
            p.cba = CBA_BASE | slot;
            // Keep the ABR bits, force page 5 (x 320) + 4bpp.
            p.tsb = (p.tsb & 0x0060) | 0x0005;
        }
    }

    // Pool: 15 CLUTs then the 4bpp page.
    let mut pool = vec![0u8; CLUT_REGION_BYTES];
    for (&col, &slot) in &col_to_slot {
        let pal = band.palettes.get(&col).copied().unwrap_or_else(|| {
            warnings.push(format!("CBA column {col} has no uploaded palette; black"));
            [0u16; 16]
        });
        for (i, e) in pal.iter().enumerate() {
            let at = slot as usize * 32 + i * 2;
            pool[at..at + 2].copy_from_slice(&e.to_le_bytes());
        }
    }
    for y in 0..PAGE_HEIGHT {
        for xb in 0..UV_SPACE / 2 {
            let lo = page_indices[y * UV_SPACE + xb * 2] & 0xF;
            let hi = page_indices[y * UV_SPACE + xb * 2 + 1] & 0xF;
            pool.push(lo | (hi << 4));
        }
    }
    Ok(pool)
}

// ---------------------------------------------------------------------------
// The enemy-side conversion.

/// Rebuild a playable character's **default-equipment** battle model on a
/// Delilas monster rig. `player_file` is the character's `PLAYERn` bytes
/// (extraction PROT 863..865), `rig` the matching [`PlayerRig`];
/// `archive_entry` + `target_id` name the monster block whose rig (and
/// rest-pose height) the model is rebuilt for.
pub fn monsterize_player(
    player_file: &[u8],
    rig: &PlayerRig,
    archive_entry: &[u8],
    target_id: u16,
) -> Result<MonsterizedPlayer> {
    monsterize_player_scaled(player_file, rig, archive_entry, target_id, 1)
}

/// [`monsterize_player`] with an explicit global texture downscale (`1` =
/// full resolution). [`swap_into_block`] raises it only when the rebuilt
/// block misses the compressed archive-slot budget.
fn monsterize_player_scaled(
    player_file: &[u8],
    rig: &PlayerRig,
    archive_entry: &[u8],
    target_id: u16,
    texture_downscale: u32,
) -> Result<MonsterizedPlayer> {
    let mut warnings = Vec::new();
    if texture_downscale > 1 {
        warnings.push(format!(
            "texture at 1/{texture_downscale} resolution to fit the compressed archive slot"
        ));
    }
    // The bake context - rest poses (terminals normalized), the whole-rig
    // `bake_frames` alignment, the radial scale, and the merged canonical
    // objects - is shared with the enemy-side clip retarget
    // (`enemy_anim`): the retargeted hero clips cancel this bake's
    // `R_align` by conjugation, so both sides must read identical data.
    let ctx = enemy_anim::monster_bake_ctx(player_file, rig, archive_entry, target_id)?;
    let mut objects = ctx.objects.clone();
    // Bake each part through source-channel rest -> target-part rest,
    // pivot-anchored (see `bake_object_pivot`): the pivot is the joint
    // the engine rotates the part about, and axial length matching puts
    // the part's far end on the target's child joint - so the target's
    // own clips move each baked part exactly like the geometry it
    // replaced, joints staying closed, not just the rest pose.
    for (c, o) in objects.iter_mut().enumerate() {
        let ch = rig.channel_for_canonical[c] as usize;
        let pb = pivot_bake_params(&ctx.src_frames[c], &ctx.dst_frames[c], ctx.radial);
        bake_object_pivot(
            o,
            &ctx.rest[ch],
            ctx.src_pivots[c],
            &ctx.target_rest[c],
            &pb,
        )
        .with_context(|| format!("bake canonical part {c}"))?;
        if c == 0 {
            // Seat the head on the neck: its near edge along the bone
            // axis lands where the replaced head's sat.
            seat_terminal_axial(o, &ctx.target_model[0], &ctx.target_rest[0], pb.x_dst)?;
        }
    }

    // Texture re-layout from the player band into the monster page.
    let pack = battle_data_pack::parse(player_file).context("parse player battle file")?;
    let equipped = [0u8; SECTION_COUNT];
    let uploads = battle_char_assembly::character_texture_uploads(player_file, &pack, &equipped, 0)
        .context("decode player texture uploads")?;
    let band = band_texels(&uploads);
    let pool = relayout_to_monster_pool(&mut objects, &band, texture_downscale, &mut warnings)?;

    let tmd = encode(&objects).context("encode swapped TMD")?;
    Ok(MonsterizedPlayer {
        tmd,
        pool,
        warnings,
    })
}

/// The full enemy-side block swap: convert, splice into the retail block,
/// and encode the archive slot - retrying at half / quarter texture
/// resolution when the compressed stream misses the fixed `0x14000` slot.
#[derive(Debug, Clone)]
pub struct SwappedBlock {
    /// The rebuilt decoded block (retail head / entries / tail, swapped
    /// mesh + pool).
    pub block: Vec<u8>,
    /// The re-encoded `[u32 len][LZS]` archive slot, `SLOT_STRIDE` bytes.
    pub slot: Vec<u8>,
    pub warnings: Vec<String>,
}

pub fn swap_into_block(
    player_file: &[u8],
    rig: &PlayerRig,
    archive_entry: &[u8],
    target_id: u16,
) -> Result<SwappedBlock> {
    let retail_block = monster_archive::decode_block(archive_entry, target_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {target_id}: empty / filler slot"))?;
    let mut last_err = None;
    for downscale in [1u32, 2, 4] {
        let out = monsterize_player_scaled(player_file, rig, archive_entry, target_id, downscale)?;
        let block =
            monster_archive::replace_mesh_and_pool(&retail_block, Some(&out.tmd), Some(&out.pool))?;
        match monster_archive::encode_slot(&block) {
            Ok(slot) => {
                return Ok(SwappedBlock {
                    block,
                    slot,
                    warnings: out.warnings,
                });
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("slot encode failed"))).context(format!(
        "swap for monster id {target_id} does not fit the archive slot"
    ))
}

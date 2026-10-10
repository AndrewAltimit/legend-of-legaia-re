//! Retargeting a hero clip onto a monster rig: the carried-part and chain FK
//! state, the retarget itself, and pose resampling.

use super::super::*;
use super::bake_ctx::MonsterBakeCtx;
use winpose::{mmul, to_euler, transpose};

// ---------------------------------------------------------------------------
// Hero -> monster clip retarget.

struct Carried {
    part: usize,
    root: usize,
    /// The monster's own rest offset, in the root's rest frame.
    rest_local: [f32; 3],
    /// The player's rest offset, in the player root's rest frame.
    src_rest_local: [f32; 3],
    /// Maps a player-authored root-local displacement onto the monster.
    transfer: [[f32; 3]; 3],
}

struct ChainFk {
    chain: [usize; 3],
    root: usize,
    /// The chain root's rest offset from its carrier, in the carrier's
    /// monster rest frame. The enemy-side bake anchors every part at the
    /// monster's own rest pivots and applies NO socket tuck, so the
    /// baked chain-root geometry hangs at the monster's rest socket -
    /// which is exactly where the FK has to put the pivot.
    socket_local: [f32; 3],
    /// Bone vectors (joint-to-joint) in each carrying part's monster
    /// rest frame.
    bv_local: [[f32; 3]; 2],
}

/// Converts clips that pose the HERO's player channels into canonical
/// monster-part space - the inverse direction of `winpose::retarget_clip`,
/// built over the same [`MonsterBakeCtx`] the mesh bake reads so the
/// per-part conjugation cancels the bake exactly.
pub(crate) struct HeroRetarget {
    rig: PlayerRig,
    radial: f32,
    /// Per canonical part: `A = R_p_rest^T * R_align^T * R_m_rest`, so a
    /// hero channel pose `R_h` plays the baked part as `R_h * A`.
    conj: Vec<[[f32; 3]; 3]>,
    carried: Vec<Carried>,
    chains: Vec<ChainFk>,
}

impl HeroRetarget {
    pub(crate) fn new(ctx: &MonsterBakeCtx, rig: &PlayerRig) -> Self {
        let conj: Vec<[[f32; 3]; 3]> = (0..CANONICAL_PARTS)
            .map(|c| {
                let ch = rig.channel_for_canonical[c] as usize;
                let r_align = frame_align(&ctx.src_frames[c], &ctx.dst_frames[c]);
                let a = mmul(&transpose(&rot_matrix(&ctx.rest[ch])), &transpose(&r_align));
                mmul(&a, &rot_matrix(&ctx.target_rest[c]))
            })
            .collect();
        let m_rest_local = |part: usize, root: usize| -> [f32; 3] {
            apply_transposed(
                &rot_matrix(&ctx.target_rest[root]),
                vsub(ctx.dst_pivots[part], ctx.dst_pivots[root]),
            )
        };
        let p_rest_local = |part: usize, root: usize| -> [f32; 3] {
            let rch = rig.channel_for_canonical[root] as usize;
            apply_transposed(
                &rot_matrix(&ctx.rest[rch]),
                vsub(ctx.src_pivots[part], ctx.src_pivots[root]),
            )
        };
        let carried = [(0usize, 1usize), (2, 1)]
            .iter()
            .map(|&(part, root)| Carried {
                part,
                root,
                rest_local: m_rest_local(part, root),
                src_rest_local: p_rest_local(part, root),
                transfer: transpose(&conj[root]),
            })
            .collect();
        let chains = [
            ([3usize, 4, 5], 1usize),
            ([6, 7, 8], 1),
            ([9, 10, 11], 2),
            ([12, 13, 14], 2),
        ]
        .iter()
        .map(|&(chain, root)| ChainFk {
            chain,
            root,
            socket_local: m_rest_local(chain[0], root),
            bv_local: [
                m_rest_local(chain[1], chain[0]),
                m_rest_local(chain[2], chain[1]),
            ],
        })
        .collect();
        HeroRetarget {
            rig: *rig,
            radial: ctx.radial,
            conj,
            carried,
            chains,
        }
    }

    /// Retarget hero-channel pose rows into canonical monster-part rows
    /// (same frame count; resample the source first if needed).
    pub(crate) fn retarget_frames(&self, frames: &[Vec<PartPose>]) -> Vec<Vec<PartPose>> {
        let mut out = Vec::with_capacity(frames.len());
        for sf in frames {
            let hero = |ch: usize| sf.get(ch).copied().unwrap_or_default();
            let mut row = vec![PartPose::default(); CANONICAL_PARTS];
            for (c, slot) in row.iter_mut().enumerate() {
                let pose = hero(self.rig.channel_for_canonical[c] as usize);
                let r = mmul(&rot_matrix(&pose), &self.conj[c]);
                let (rx, ry, rz) = to_euler(&r);
                let t = |v: i16| ((v as f32) * self.radial).round().clamp(-2048.0, 2047.0) as i16;
                *slot = PartPose {
                    tx: t(pose.tx),
                    ty: t(pose.ty),
                    tz: t(pose.tz),
                    rx,
                    ry,
                    rz,
                };
            }
            let set = |p: &mut PartPose, w: [f32; 3]| {
                p.tx = w[0].round().clamp(-2048.0, 2047.0) as i16;
                p.ty = w[1].round().clamp(-2048.0, 2047.0) as i16;
                p.tz = w[2].round().clamp(-2048.0, 2047.0) as i16;
            };
            let world = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
            // Carried parts first (the leg chains socket onto the pelvis).
            for cr in &self.carried {
                let root_ch = self.rig.channel_for_canonical[cr.root] as usize;
                let part_ch = self.rig.channel_for_canonical[cr.part] as usize;
                let (hp, hr) = (hero(part_ch), hero(root_ch));
                // This frame's deviation from the player's rest
                // attachment, in the player root's local frame.
                let u = apply_transposed(
                    &rot_matrix(&hr),
                    vsub(
                        [hp.tx as f32, hp.ty as f32, hp.tz as f32],
                        [hr.tx as f32, hr.ty as f32, hr.tz as f32],
                    ),
                );
                let dev = apply(&cr.transfer, vsub(u, cr.src_rest_local));
                let local = [
                    cr.rest_local[0] + dev[0] * self.radial,
                    cr.rest_local[1] + dev[1] * self.radial,
                    cr.rest_local[2] + dev[2] * self.radial,
                ];
                let o = apply(&rot_matrix(&row[cr.root]), local);
                let t = world(&row[cr.root]);
                let target = [t[0] + o[0], t[1] + o[1], t[2] + o[2]];
                set(&mut row[cr.part], target);
            }
            for fk in &self.chains {
                let rt = world(&row[fk.root]);
                let s = apply(&rot_matrix(&row[fk.root]), fk.socket_local);
                let mut pos = [rt[0] + s[0], rt[1] + s[1], rt[2] + s[2]];
                set(&mut row[fk.chain[0]], pos);
                let b0 = apply(&rot_matrix(&row[fk.chain[0]]), fk.bv_local[0]);
                pos = [pos[0] + b0[0], pos[1] + b0[1], pos[2] + b0[2]];
                set(&mut row[fk.chain[1]], pos);
                let b1 = apply(&rot_matrix(&row[fk.chain[1]]), fk.bv_local[1]);
                pos = [pos[0] + b1[0], pos[1] + b1[1], pos[2] + b1[2]];
                set(&mut row[fk.chain[2]], pos);
            }
            out.push(row);
        }
        out
    }
}

/// Linear pose resampling: `out` frames over the input's full span,
/// translations lerped, rotations shortest-path lerped. Endpoint
/// preserving (frame 0 and the last frame survive exactly).
pub(crate) fn resample_poses(frames: &[Vec<PartPose>], out_frames: usize) -> Vec<Vec<PartPose>> {
    let n = frames.len();
    if n == 0 || out_frames == 0 {
        return Vec::new();
    }
    let lerp_angle = |a: u16, b: u16, t: f32| -> u16 {
        let mut d = (b as i32 - a as i32).rem_euclid(4096);
        if d > 2048 {
            d -= 4096;
        }
        ((a as i32 + (d as f32 * t).round() as i32).rem_euclid(4096)) as u16
    };
    let lerp_i =
        |a: i16, b: i16, t: f32| -> i16 { (a as f32 + (b as f32 - a as f32) * t).round() as i16 };
    (0..out_frames)
        .map(|j| {
            let pos = if out_frames == 1 {
                0.0
            } else {
                j as f32 * (n as f32 - 1.0) / (out_frames as f32 - 1.0)
            };
            let i0 = (pos.floor() as usize).min(n - 1);
            let i1 = (i0 + 1).min(n - 1);
            let t = pos - i0 as f32;
            let (fa, fb) = (&frames[i0], &frames[i1]);
            (0..fa.len().min(fb.len()))
                .map(|p| PartPose {
                    tx: lerp_i(fa[p].tx, fb[p].tx, t),
                    ty: lerp_i(fa[p].ty, fb[p].ty, t),
                    tz: lerp_i(fa[p].tz, fb[p].tz, t),
                    rx: lerp_angle(fa[p].rx, fb[p].rx, t),
                    ry: lerp_angle(fa[p].ry, fb[p].ry, t),
                    rz: lerp_angle(fa[p].rz, fb[p].rz, t),
                })
                .collect()
        })
        .collect()
}

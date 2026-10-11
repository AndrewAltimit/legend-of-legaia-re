//! Retargeting a win-pose clip onto the swapped rig, with and without the wrist
//! correction.

use super::super::*;
use super::pose_codec::{mmul, to_euler, transpose};

/// Retarget the sibling's victory clip onto the player rig, resampled
/// (nearest frame) to `frame_count` frames of `part_count` player
/// channels. Noa's extra hair channel rides the head's pose.
pub fn retarget_clip(
    clip: &legaia_actor_formats::monster_archive::MonsterAnimation,
    rig: &PlayerRig,
    player_file: &[u8],
    archive_entry: &[u8],
    source_id: u16,
    part_count: usize,
    frame_count: usize,
) -> Result<Vec<Vec<PartPose>>> {
    retarget_clip_wrist(
        clip,
        rig,
        player_file,
        archive_entry,
        source_id,
        part_count,
        frame_count,
        None,
    )
}

/// [`retarget_clip`] with an explicit wrist policy: `natural_wrist_hand`
/// names a canonical hand part (5 / 8) whose played rotations keep the
/// SIBLING's own hand-to-forearm relation instead of being slaved to the
/// host's retail rest wrist attitude.
///
/// The slaving below exists for the HOST's welded equipped weapon: the
/// per-part conjugation preserves neither rig's wrist relation, and the
/// host's blade (correct under every host clip) pointed across the
/// torso under a played sibling pose. When the swap KEEPS the sibling's
/// own welded weapon on that hand instead (`--delilas-che-hammer`), the
/// premise inverts: the geometry in the hand is the SIBLING's, its
/// authored wrist relation is the retail look (the enemy-table ground
/// truth), and the natural conjugated rotation is exactly what pairs
/// with the mesh bake to reproduce it - measured 73 degrees of hammer
/// long-axis error under the slaved wrist, ~0 by construction here.
#[allow(clippy::too_many_arguments)]
pub fn retarget_clip_wrist(
    clip: &legaia_actor_formats::monster_archive::MonsterAnimation,
    rig: &PlayerRig,
    player_file: &[u8],
    archive_entry: &[u8],
    source_id: u16,
    part_count: usize,
    frame_count: usize,
    natural_wrist_hand: Option<usize>,
) -> Result<Vec<Vec<PartPose>>> {
    // The same rest data the playerize bake uses.
    let src_idle = monster_archive::idle_animation(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no idle"))?;
    let mut src_rest = src_idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("monster idle empty"))?
        .clone();
    let idle = battle_char_assembly::idle_battle_animation(player_file)?
        .ok_or_else(|| anyhow::anyhow!("player file has no idle"))?;
    let dst_rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("player idle empty"))?
        .clone();
    // Player-shaped ankles - MUST match the playerize mesh bake's rest,
    // or the conjugation stops cancelling and every converted pose's
    // feet skew by the ankle delta.
    normalize_battle_rest_feet(&mut src_rest, &dst_rest, rig);
    let pack = battle_data_pack::parse(player_file)?;
    let asm = battle_char_assembly::assemble_character(player_file, &pack, &[0; SECTION_COUNT])?;
    let dst_tmd = legaia_tmd::parse(&asm.tmd)?;
    let dst_model = decode_model(&dst_tmd, &asm.tmd)?;
    let src_mesh = monster_archive::mesh(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: empty slot"))?;
    let src_tmd = legaia_tmd::parse(src_mesh.tmd_bytes())?;
    let src_model = decode_model(&src_tmd, src_mesh.tmd_bytes())?;

    let dst_stats: Vec<PartStats> = (0..CANONICAL_PARTS)
        .map(|c| {
            let ch = rig.channel_for_canonical[c] as usize;
            Ok(part_world_stats(
                dst_model
                    .get(ch)
                    .ok_or_else(|| anyhow::anyhow!("player model missing channel {ch}"))?,
                &dst_rest[ch],
            ))
        })
        .collect::<Result<_>>()?;
    let src_stats: Vec<PartStats> = src_model
        .iter()
        .take(CANONICAL_PARTS)
        .enumerate()
        .map(|(c, o)| part_world_stats(o, &src_rest[c]))
        .collect();
    let radial = global_height_scale(&src_stats, &dst_stats)[0];
    let pivot_of = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
    let src_pivots: Vec<[f32; 3]> = src_rest
        .iter()
        .take(CANONICAL_PARTS)
        .map(pivot_of)
        .collect();
    let dst_pivots: Vec<[f32; 3]> = (0..CANONICAL_PARTS)
        .map(|c| pivot_of(&dst_rest[rig.channel_for_canonical[c] as usize]))
        .collect();
    // MUST be the bake's own frames, not raw `bone_frames`: the played
    // pose cancels the bake's `R_align`, so the two have to be built the
    // same way or the conjugation stops cancelling and every converted
    // pose inherits the difference (measured: up to 152 degrees on Gi's
    // shin, 114 on Che's torso).
    let (src_frames, dst_frames) = playerize::bake_frames(
        &src_pivots,
        &dst_pivots,
        &CANONICAL_CHILD,
        &CANONICAL_PARENT,
    );

    // Per-part constant conjugation A = R_src_rest^T * R_align^T * R_dst_rest.
    let conj: Vec<[[f32; 3]; 3]> = (0..CANONICAL_PARTS)
        .map(|c| {
            let ch = rig.channel_for_canonical[c] as usize;
            let r_align = frame_align(&src_frames[c], &dst_frames[c]);
            let a = mmul(&transpose(&rot_matrix(&src_rest[c])), &transpose(&r_align));
            mmul(&a, &rot_matrix(&dst_rest[ch]))
        })
        .collect();

    // Skeleton FK data. The baked mesh wears HOST-proportioned parts at
    // the HOST's joint spacing (plus the shoulder and hip tucks), so a
    // uniformly radial-scaled sibling translation puts every pivot where
    // the SIBLING's joint sat, not where the baked part is. The error is
    // `|radial * src_bone - dst_bone|` per joint: harmless where the two
    // rigs agree, and a hole where they do not - Che's torso bone is 164
    // against Gala's 90, which floated his head 38 units clear of the
    // neck for the whole victory flourish.
    //
    // So per frame the skeleton re-derives its pivots by forward
    // kinematics from the BAKED rig's own bone vectors: the torso keeps
    // the clip's world placement, head and pelvis ride it at the baked
    // offsets, and each limb chain hangs off its socket (minus the
    // played tuck, so the tucked near-edge - not the pivot - meets the
    // socket) along the baked parts' bone vectors. At destination-rest
    // rotations every term reduces exactly to the player's retail pivot.
    struct ChainFk {
        /// Canonical parts of the limb chain, root joint first.
        chain: [usize; 3],
        /// Canonical part carrying the chain's socket (1 torso, 2 pelvis).
        root: usize,
        socket_local: [f32; 3],
        tuck_local: [f32; 3],
        bv_local: [[f32; 3]; 2],
    }
    let torso_ch = rig.channel_for_canonical[1] as usize;
    let pb_torso = pivot_bake_params(&src_frames[1], &dst_frames[1], radial);
    let pb_pelvis = pivot_bake_params(&src_frames[2], &dst_frames[2], radial);
    // A part's baked offset from its carrier, in the carrier's rest frame.
    let rest_local = |c: usize, root: usize| -> [f32; 3] {
        let rch = rig.channel_for_canonical[root] as usize;
        apply_transposed(
            &rot_matrix(&dst_rest[rch]),
            vsub(dst_pivots[c], dst_pivots[root]),
        )
    };
    // Head and pelvis carry no socket of their own: the head's geometry is
    // seated on the player's neck by `seat_terminal_axial`, so the played
    // head pivot has to BE the player's neck.
    //
    // These two keep the clip's AUTHORED motion, unlike the limb chains.
    // A victory clip bobs the head against the torso, and a rigid re-seat
    // throws that away - it cost Lu more (her authored bob) than the pivot
    // error it removed (2.9 units). So only the constant rest offset is
    // replaced: the frame's deviation from the sibling's own rest
    // attachment is carried across into the player's torso frame through
    // the torso's own conjugation, and scaled with the rig.
    struct Carried {
        part: usize,
        root: usize,
        /// The player's own rest offset, in the root's rest frame.
        rest_local: [f32; 3],
        /// The sibling's rest offset, in ITS root's rest frame.
        src_rest_local: [f32; 3],
        /// Maps a sibling-authored root-local displacement onto the player.
        transfer: [[f32; 3]; 3],
    }
    let carried: Vec<Carried> = [(0usize, 1usize), (2, 1)]
        .iter()
        .map(|&(part, root)| Carried {
            part,
            root,
            rest_local: rest_local(part, root),
            src_rest_local: apply_transposed(
                &rot_matrix(&src_rest[root]),
                vsub(src_pivots[part], src_pivots[root]),
            ),
            transfer: transpose(&conj[root]),
        })
        .collect();
    let chain_fk: Vec<ChainFk> = [
        ([3usize, 4, 5], 1usize),
        ([6, 7, 8], 1),
        ([9, 10, 11], 2),
        ([12, 13, 14], 2),
    ]
    .iter()
    .map(|&(chain, root)| {
        let pb = if root == 1 { &pb_torso } else { &pb_pelvis };
        let socket = bake_point_pivot(src_pivots[chain[0]], src_pivots[root], dst_pivots[root], pb);
        let ch0 = rig.channel_for_canonical[chain[0]] as usize;
        let md0 = rot_matrix(&dst_rest[ch0]);
        let mdr = rot_matrix(&dst_rest[rig.channel_for_canonical[root] as usize]);
        let bv = |c: usize| {
            let chp = rig.channel_for_canonical[c] as usize;
            apply_transposed(
                &rot_matrix(&dst_rest[chp]),
                vsub(dst_pivots[c + 1], dst_pivots[c]),
            )
        };
        ChainFk {
            chain,
            root,
            socket_local: apply_transposed(&mdr, vsub(socket, dst_pivots[root])),
            tuck_local: apply_transposed(&md0, vsub(socket, dst_pivots[chain[0]])),
            bv_local: [bv(chain[0]), bv(chain[1])],
        }
    })
    .collect();

    // Retail rest wrist attitude per arm: hand rotation relative to its
    // forearm in the PLAYER's own idle rest. The per-part conjugation
    // maps each part's ABSOLUTE orientation independently, so a played
    // sibling pose preserves neither rig's hand-to-forearm relation -
    // measured 85 degrees off retail on Gi's weapon hand, which points
    // the welded equipped-weapon blade (correct under every host art
    // clip) straight across the torso for the whole idle. Hands carry no
    // authored detail worth keeping (unlike the head bob), so slave each
    // hand to its forearm at the retail rest attitude - the same
    // terminal law the feet get via `normalize_battle_rest_feet`. Hand
    // rotations feed no FK translation term, so this changes orientation
    // only, about the wrist pivot the fist is anchored at.
    let wrist_attitude: Vec<(usize, usize, [[f32; 3]; 3])> = [(4usize, 5usize), (7, 8)]
        .iter()
        .filter(|&&(_, hand_c)| natural_wrist_hand != Some(hand_c))
        .filter_map(|&(fore_c, hand_c)| {
            let fch = rig.channel_for_canonical[fore_c] as usize;
            let hch = rig.channel_for_canonical[hand_c] as usize;
            let (fr, hr) = (dst_rest.get(fch)?, dst_rest.get(hch)?);
            let k = mmul(&transpose(&rot_matrix(fr)), &rot_matrix(hr));
            Some((fch, hch, k))
        })
        .collect();

    let n_src = clip.frames.len();
    let mut out = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let sf = &clip.frames[(f * n_src / frame_count).min(n_src - 1)];
        let mut row = vec![PartPose::default(); part_count];
        for c in 0..CANONICAL_PARTS {
            let ch = rig.channel_for_canonical[c] as usize;
            if ch >= part_count {
                continue;
            }
            let pose = &sf[c];
            let r_play = mmul(&rot_matrix(pose), &conj[c]);
            let (rx, ry, rz) = to_euler(&r_play);
            let t = |v: i16| -> i16 { ((v as f32) * radial).round().clamp(-2048.0, 2047.0) as i16 };
            row[ch] = PartPose {
                tx: t(pose.tx),
                ty: t(pose.ty),
                tz: t(pose.tz),
                rx,
                ry,
                rz,
            };
        }
        for &(fch, hch, k) in &wrist_attitude {
            if fch >= part_count || hch >= part_count {
                continue;
            }
            let m = mmul(&rot_matrix(&row[fch]), &k);
            let (rx, ry, rz) = to_euler(&m);
            row[hch].rx = rx;
            row[hch].ry = ry;
            row[hch].rz = rz;
        }
        if torso_ch < part_count {
            let set = |p: &mut PartPose, w: [f32; 3]| {
                p.tx = w[0].round().clamp(-2048.0, 2047.0) as i16;
                p.ty = w[1].round().clamp(-2048.0, 2047.0) as i16;
                p.tz = w[2].round().clamp(-2048.0, 2047.0) as i16;
            };
            let world = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
            // Carried parts first: the leg chains socket onto the pelvis,
            // so the pelvis has to be placed before they hang off it.
            for cr in &carried {
                let ch = rig.channel_for_canonical[cr.part] as usize;
                let rch = rig.channel_for_canonical[cr.root] as usize;
                if ch >= part_count || rch >= part_count {
                    continue;
                }
                // This frame's deviation from the sibling's rest attachment,
                // in the sibling's root-local frame.
                let u = apply_transposed(
                    &rot_matrix(&sf[cr.root]),
                    vsub(
                        [
                            sf[cr.part].tx as f32,
                            sf[cr.part].ty as f32,
                            sf[cr.part].tz as f32,
                        ],
                        [
                            sf[cr.root].tx as f32,
                            sf[cr.root].ty as f32,
                            sf[cr.root].tz as f32,
                        ],
                    ),
                );
                let dev = apply(&cr.transfer, vsub(u, cr.src_rest_local));
                let local = [
                    cr.rest_local[0] + dev[0] * radial,
                    cr.rest_local[1] + dev[1] * radial,
                    cr.rest_local[2] + dev[2] * radial,
                ];
                let o = apply(&rot_matrix(&row[rch]), local);
                let t = world(&row[rch]);
                set(&mut row[ch], [t[0] + o[0], t[1] + o[1], t[2] + o[2]]);
            }
            for fk in &chain_fk {
                let chs = fk.chain.map(|c| rig.channel_for_canonical[c] as usize);
                let rch = rig.channel_for_canonical[fk.root] as usize;
                if rch >= part_count || chs.iter().any(|&ch| ch >= part_count) {
                    continue;
                }
                let rt = world(&row[rch]);
                let s = apply(&rot_matrix(&row[rch]), fk.socket_local);
                let socket = [s[0] + rt[0], s[1] + rt[1], s[2] + rt[2]];
                let r0 = rot_matrix(&row[chs[0]]);
                let tk = apply(&r0, fk.tuck_local);
                let mut pos = [socket[0] - tk[0], socket[1] - tk[1], socket[2] - tk[2]];
                set(&mut row[chs[0]], pos);
                let b0 = apply(&r0, fk.bv_local[0]);
                pos = [pos[0] + b0[0], pos[1] + b0[1], pos[2] + b0[2]];
                set(&mut row[chs[1]], pos);
                let b1 = apply(&rot_matrix(&row[chs[1]]), fk.bv_local[1]);
                pos = [pos[0] + b1[0], pos[1] + b1[1], pos[2] + b1[2]];
                set(&mut row[chs[2]], pos);
            }
        }
        if let Some(hair) = rig.hair_channel {
            let head_ch = rig.channel_for_canonical[0] as usize;
            if (hair as usize) < part_count {
                row[hair as usize] = row[head_ch];
            }
        }
        out.push(row);
    }
    Ok(out)
}

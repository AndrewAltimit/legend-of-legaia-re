//! Win-pose replacement: the party's victory animations (the eight
//! streams of each character's **base "ME" archive**, readef.DAT slot
//! `3*char + 2` - see `docs/formats/battle-data-pack.md` § "ME" stream
//! archives) rebuild from the mapped sibling's own victory clip, so a
//! swapped character celebrates like the Delilas they depict.
//!
//! The sibling's monster clip poses canonical parts; the baked player
//! model wears those parts on the player rig via the pivot bake, so the
//! clip retargets with the bake's own per-part conjugation:
//! `R_play = R_sib * R_src_rest^T * R_align^T * R_dst_rest` (the bake's
//! linear map cancelled out of the played pose), `T_play = r * T_sib`.
//! Each retail entry keeps its exact frame count (the art records'
//! timing fields stay retail) by nearest-frame resampling, and the
//! rebuilt entries re-encode with the retail channel-delta codec.

use super::*;
use crate::me_archive;

/// readef.DAT slot stride.
pub const READEF_SLOT: usize = 0x10800;

/// The base ("ME") archive slot carrying character `char_index`'s win
/// poses (0 = Vahn, 1 = Noa, 2 = Gala).
pub fn base_slot_index(char_index: usize) -> usize {
    3 * char_index + 2
}

pub(crate) fn mmul(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut m = [[0.0f32; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

pub(crate) fn transpose(a: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut m = [[0.0f32; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = a[j][i];
        }
    }
    m
}

/// Extract PSX euler angles (1/4096 turns, `R = Rz * Ry * Rx`) from a
/// rotation matrix - the inverse of `rot_matrix`. Near gimbal lock
/// (`|cos y| ~ 0`) the x/z split is degenerate; z folds to 0 there.
pub(crate) fn to_euler(m: &[[f32; 3]; 3]) -> (u16, u16, u16) {
    let sy = (-m[2][0]).clamp(-1.0, 1.0);
    let y = sy.asin();
    let (x, z) = if y.cos().abs() > 1e-4 {
        (m[2][1].atan2(m[2][2]), m[1][0].atan2(m[0][0]))
    } else {
        ((-m[1][2]).atan2(m[1][1]), 0.0)
    };
    let unit = |a: f32| -> u16 {
        let u = (a / std::f32::consts::TAU * 4096.0).round() as i32;
        (u.rem_euclid(4096)) as u16
    };
    (unit(x), unit(y), unit(z))
}

/// Pack one part pose into the 9-byte stream record (six 12-bit fields;
/// the `FUN_8004998C` unpack layout in reverse).
fn pack_part(p: &PartPose) -> [u8; 9] {
    let f = [
        (p.tx as u16) & 0xFFF,
        (p.ty as u16) & 0xFFF,
        (p.tz as u16) & 0xFFF,
        p.rx & 0xFFF,
        p.ry & 0xFFF,
        p.rz & 0xFFF,
    ];
    let mut out = [0u8; 9];
    for pair in 0..3 {
        let (a, b) = (f[pair * 2], f[pair * 2 + 1]);
        out[pair * 3] = a as u8;
        out[pair * 3 + 1] = b as u8;
        out[pair * 3 + 2] = ((a >> 8) as u8 & 0x0F) | ((b >> 4) as u8 & 0xF0);
    }
    out
}

/// Encode a decoded stream (`[parts][frames][9-byte records]`) with the
/// retail channel-delta codec - the exact inverse of
/// [`me_archive::decode_channel_delta`], choosing the cheapest selector
/// arm per value. Round-trips bit-exact through the decoder.
pub fn encode_channel_delta(decoded: &[u8]) -> Result<Vec<u8>> {
    if decoded.len() < 2 {
        bail!("stream shorter than its parts/frames head");
    }
    let parts = decoded[0] as usize;
    let frames = decoded[1] as usize;
    let channels = parts * 6;
    if decoded.len() < 2 + parts * frames * 9 {
        bail!("stream shorter than its frame data");
    }

    // Target 12-bit channel values per frame, from the packed records.
    let mut target: Vec<Vec<u16>> = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut row = Vec::with_capacity(channels);
        for p in 0..parts {
            let o = 2 + (f * parts + p) * 9;
            let b = &decoded[o..o + 9];
            for pair in 0..3 {
                let a = b[pair * 3] as u16 | ((b[pair * 3 + 2] as u16 & 0x0F) << 8);
                let v = b[pair * 3 + 1] as u16 | ((b[pair * 3 + 2] as u16 & 0xF0) << 4);
                row.push(a);
                row.push(v);
            }
        }
        target.push(row);
    }

    let mut bits: Vec<u8> = Vec::new(); // one selector bit per push
    let mut nibs: Vec<u8> = Vec::new(); // 4-bit operands
    let mut bytes: Vec<u8> = vec![parts as u8, frames as u8];

    let mut delta = vec![0u16; channels];
    let mut acc = vec![0u16; channels];
    for (f, row) in target.iter().enumerate() {
        for c in 0..channels {
            // The delta value the decoder must produce for this slot.
            let v = if f == 0 {
                if c < 6 {
                    row[c]
                } else {
                    row[c].wrapping_sub(row[c - 6]) & 0xFFF
                }
            } else {
                row[c].wrapping_sub(acc[c]) & 0xFFF
            };
            let prev = delta[if c < 6 { c + channels - 6 } else { c - 6 }];
            let dp = v.wrapping_sub(prev) & 0xFFF;
            if dp < 0x10 {
                // `01`: previous-part delta + nibble.
                bits.extend_from_slice(&[0, 1]);
                nibs.push(dp as u8);
            } else if dp >= 0xFF0 {
                // `001`: previous-part delta + negative nibble.
                bits.extend_from_slice(&[0, 0, 1]);
                nibs.push((dp & 0xF) as u8);
            } else if v < 0x10 {
                // `0001`: literal nibble.
                bits.extend_from_slice(&[0, 0, 0, 1]);
                nibs.push(v as u8);
            } else if v >= 0xFF0 {
                // `0000`: literal negative nibble.
                bits.extend_from_slice(&[0, 0, 0, 0]);
                nibs.push((v & 0xF) as u8);
            } else {
                // `1`: 12-bit literal (nibble high, byte low).
                bits.push(1);
                nibs.push((v >> 8) as u8);
                bytes.push(v as u8);
            }
            delta[c] = v;
        }
        if f == 0 {
            acc[..6].copy_from_slice(&delta[..6]);
            for c in 6..channels {
                acc[c] = delta[c].wrapping_add(acc[c - 6]) & 0xFFF;
            }
        } else {
            for c in 0..channels {
                acc[c] = acc[c].wrapping_add(delta[c]) & 0xFFF;
            }
        }
    }

    let mut bit_bytes = vec![0u8; bits.len().div_ceil(8)];
    for (i, b) in bits.iter().enumerate() {
        bit_bytes[i >> 3] |= b << (7 - (i & 7));
    }
    let mut nib_bytes = vec![0u8; nibs.len().div_ceil(2)];
    for (i, n) in nibs.iter().enumerate() {
        nib_bytes[i >> 1] |= if i & 1 == 0 { n << 4 } else { *n };
    }
    let nibble_off = bit_bytes.len();
    let byte_off = nibble_off + nib_bytes.len();
    if nibble_off > u16::MAX as usize || byte_off > u16::MAX as usize {
        bail!("codec streams exceed the 16-bit header offsets");
    }
    let mut out = Vec::with_capacity(5 + byte_off + bytes.len());
    out.push(0x40);
    out.extend_from_slice(&(nibble_off as u16).to_le_bytes());
    out.extend_from_slice(&(byte_off as u16).to_le_bytes());
    out.extend_from_slice(&bit_bytes);
    out.extend_from_slice(&nib_bytes);
    out.extend_from_slice(&bytes);
    Ok(out)
}

/// The mapped sibling's victory clip: the LAST tag-`0x22` entry, else
/// the last tag-`0x23` flourish (Che ships no `0x22`), else the idle.
pub fn victory_clip(
    archive_entry: &[u8],
    source_id: u16,
) -> Result<crate::monster_archive::MonsterAnimation> {
    let tags = monster_archive::action_tags(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: empty slot"))?;
    let anims = monster_archive::animations(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no animations"))?;
    for want in [0x22u8, 0x23, 0x00] {
        if let Some(a) = tags
            .iter()
            .rposition(|&t| t == want)
            .and_then(|i| anims.get(i))
            && a.part_count == CANONICAL_PARTS
            && !a.frames.is_empty()
        {
            return Ok(a.clone());
        }
    }
    bail!("monster id {source_id}: no usable victory clip")
}

/// Retarget the sibling's victory clip onto the player rig, resampled
/// (nearest frame) to `frame_count` frames of `part_count` player
/// channels. Noa's extra hair channel rides the head's pose.
pub fn retarget_clip(
    clip: &crate::monster_archive::MonsterAnimation,
    rig: &PlayerRig,
    player_file: &[u8],
    archive_entry: &[u8],
    source_id: u16,
    part_count: usize,
    frame_count: usize,
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
    let src_frames = bone_frames(&src_pivots, &CANONICAL_CHILD, &CANONICAL_PARENT);
    let dst_frames = bone_frames(&dst_pivots, &CANONICAL_CHILD, &CANONICAL_PARENT);

    // Per-part constant conjugation A = R_src_rest^T * R_align^T * R_dst_rest.
    let conj: Vec<[[f32; 3]; 3]> = (0..CANONICAL_PARTS)
        .map(|c| {
            let ch = rig.channel_for_canonical[c] as usize;
            let r_align = frame_align(&src_frames[c], &dst_frames[c]);
            let a = mmul(&transpose(&rot_matrix(&src_rest[c])), &transpose(&r_align));
            mmul(&a, &rot_matrix(&dst_rest[ch]))
        })
        .collect();

    // Arm-chain FK data: the baked mesh carries VAHN-proportioned arm
    // parts (and the shoulder tuck), so uniformly radial-scaled sibling
    // translations leave the arms off the baked torso's sockets at
    // victory time. Per frame the arm chains re-derive their pivots by
    // forward kinematics instead: shoulder = the baked torso's socket
    // (minus the played tuck, so the tucked near-edge - not the pivot -
    // meets the socket), elbow/hand = along the baked parts' own bone
    // vectors. At destination-rest rotations this reduces exactly to
    // the player's retail pivots.
    struct ArmFk {
        chain: [usize; 3],
        socket_local: [f32; 3],
        tuck_local: [f32; 3],
        bv_local: [[f32; 3]; 2],
    }
    let torso_ch = rig.channel_for_canonical[1] as usize;
    let pb_torso = pivot_bake_params(&src_frames[1], &dst_frames[1], radial);
    let md_t = rot_matrix(&dst_rest[torso_ch]);
    let arm_fk: Vec<ArmFk> = [[3usize, 4, 5], [6usize, 7, 8]]
        .iter()
        .map(|&chain| {
            let socket = bake_point_pivot(
                src_pivots[chain[0]],
                src_pivots[1],
                dst_pivots[1],
                &pb_torso,
            );
            let ch0 = rig.channel_for_canonical[chain[0]] as usize;
            let md0 = rot_matrix(&dst_rest[ch0]);
            let bv = |c: usize| {
                let chp = rig.channel_for_canonical[c] as usize;
                apply_transposed(
                    &rot_matrix(&dst_rest[chp]),
                    vsub(dst_pivots[c + 1], dst_pivots[c]),
                )
            };
            ArmFk {
                chain,
                socket_local: apply_transposed(&md_t, vsub(socket, dst_pivots[1])),
                tuck_local: apply_transposed(&md0, vsub(socket, dst_pivots[chain[0]])),
                bv_local: [bv(chain[0]), bv(chain[1])],
            }
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
        if torso_ch < part_count {
            let rt = rot_matrix(&row[torso_ch]);
            let tw = [
                row[torso_ch].tx as f32,
                row[torso_ch].ty as f32,
                row[torso_ch].tz as f32,
            ];
            for fk in &arm_fk {
                let set = |p: &mut PartPose, w: [f32; 3]| {
                    p.tx = w[0].round().clamp(-2048.0, 2047.0) as i16;
                    p.ty = w[1].round().clamp(-2048.0, 2047.0) as i16;
                    p.tz = w[2].round().clamp(-2048.0, 2047.0) as i16;
                };
                let chs = fk.chain.map(|c| rig.channel_for_canonical[c] as usize);
                if chs.iter().any(|&ch| ch >= part_count) {
                    continue;
                }
                let s = apply(&rt, fk.socket_local);
                let socket = [s[0] + tw[0], s[1] + tw[1], s[2] + tw[2]];
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

/// Rebuild a base "ME" archive slot: every entry becomes the retargeted
/// sibling victory clip at that entry's RETAIL frame count, re-encoded
/// with the retail codec. Returns the full `READEF_SLOT`-byte slot
/// (zero-padded past the archive).
pub fn rebuild_base_slot(
    slot: &[u8],
    clip: &crate::monster_archive::MonsterAnimation,
    rig: &PlayerRig,
    player_file: &[u8],
    archive_entry: &[u8],
    source_id: u16,
) -> Result<Vec<u8>> {
    if slot.len() != READEF_SLOT {
        bail!("base slot is {} bytes, expected {READEF_SLOT}", slot.len());
    }
    let ar = me_archive::parse(slot).context("parse base ME archive")?;
    let n = ar.len();
    // Entries 4/5 back the WEAK victory actions (0x15/0x16), which the
    // results sequencer LOOPS - retail authors them as near-static
    // breathing so the loop is invisible. A looping victory flourish
    // visibly replays, and a frozen final pose reads as the animation
    // skipping to its end - so those entries carry the sibling's IDLE
    // clip instead: authored as a seamless cycle, it survives the loop
    // the way retail's breathing does.
    let idle = monster_archive::idle_animation(archive_entry, source_id)
        .context("weak-entry idle clip")?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no idle for weak entries"))?;
    let mut bodies: Vec<Vec<u8>> = Vec::with_capacity(n);
    for i in 0..n {
        let retail = ar.entry(i).with_context(|| format!("decode entry {i}"))?;
        let (parts, frames) = (retail[0] as usize, retail[1] as usize);
        let source = if (4..=5).contains(&i) { &idle } else { clip };
        let frames_out = retarget_clip(
            source,
            rig,
            player_file,
            archive_entry,
            source_id,
            parts,
            frames,
        )?;
        let mut decoded = Vec::with_capacity(2 + parts * frames * 9);
        decoded.push(parts as u8);
        decoded.push(frames as u8);
        for row in &frames_out {
            for p in row {
                decoded.extend_from_slice(&pack_part(p));
            }
        }
        let encoded = encode_channel_delta(&decoded)?;
        // Self-check: the retail decoder must reproduce the stream
        // bit-exact (the codec's delta state is subtle; never ship an
        // entry the game would mis-decode).
        let back = me_archive::decode_channel_delta(&encoded)
            .with_context(|| format!("re-decode entry {i}"))?;
        if back != decoded {
            bail!("entry {i}: codec round-trip mismatch");
        }
        bodies.push(encoded);
    }
    let total = 3 + 2 * n + bodies.iter().map(|b| b.len()).sum::<usize>();
    if total > READEF_SLOT {
        bail!("rebuilt base archive ({total} bytes) exceeds the slot");
    }
    let mut out = Vec::with_capacity(READEF_SLOT);
    out.extend_from_slice(&me_archive::MAGIC);
    out.push(n as u8);
    for b in &bodies {
        out.extend_from_slice(&((b.len() as u16) | 0x8000).to_le_bytes());
    }
    for b in &bodies {
        out.extend_from_slice(b);
    }
    out.resize(READEF_SLOT, 0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_delta_roundtrips() {
        // A synthetic 3-part x 4-frame stream exercising every selector
        // arm: literals, small deltas, negative deltas, holds.
        let mut poses = Vec::new();
        for f in 0..4i16 {
            let mut row = Vec::new();
            for p in 0..3i16 {
                row.push(PartPose {
                    tx: 100 * p + f,
                    ty: -50 * p,
                    tz: 2000 - 13 * f * p,
                    rx: (300 * p as u16 + 7 * f as u16) & 0xFFF,
                    ry: 4095 - (p as u16 * 2),
                    rz: (f as u16 * 1000) & 0xFFF,
                });
            }
            poses.push(row);
        }
        let mut decoded = vec![3u8, 4u8];
        for row in &poses {
            for p in row {
                decoded.extend_from_slice(&pack_part(p));
            }
        }
        let enc = encode_channel_delta(&decoded).expect("encode");
        let dec = me_archive::decode_channel_delta(&enc).expect("decode");
        assert_eq!(dec, decoded, "codec round-trip");
    }
}

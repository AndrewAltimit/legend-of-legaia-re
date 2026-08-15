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
//! linear map cancelled out of the played pose). Translations do NOT
//! carry over: only the torso keeps the clip's own placement
//! (`T_play = r * T_sib`) and the rest of the skeleton hangs off it by
//! forward kinematics over the baked rig's bone vectors, because the
//! baked parts sit at the HOST's joint spacing, not the sibling's.
//! Each retail entry keeps its exact frame count (the art records'
//! timing fields stay retail) by nearest-frame resampling, and the
//! rebuilt entries re-encode with the retail channel-delta codec.
//!
//! A win pose is **not** a one-shot stream. Every one of the 24 retail
//! base records carries a loop window: entry `+0x84 = 0xFF` seeds the
//! hold counter `actor +0x176` and `+0x85`/`+0x86` bound the frames the
//! tick replays (`FUN_80047430` at `0x80047768`: once the cursor reaches
//! `+0x86 << 4` it subtracts `(+0x86 - +0x85) << 4` and decrements the
//! counter, so the stream cycles frames `[+0x85, +0x86]` up to 255 times
//! before the results sequencer moves on). Retail authors those frames
//! as a seamless celebration cycle - measured over all 24 records, the
//! pose gap across the wrap is 0.1-1.0 model units and 0.35-2.01 degrees
//! per part, at or below the window's own mean frame step.
//!
//! Uniformly resampling a one-shot flourish into that shape points the
//! window at the flourish's own tail, so the last second of the clip
//! replays for as long as the results panel is up - measured on the
//! rebuilt streams, a 4.0-164.1 unit / 5.4-122.9 degree snap at every
//! wrap against retail's 0.1-1.0 / 0.35-2.01.
//!
//! So the rebuilt stream is composed, not uniformly resampled: the
//! sibling's flourish plays over the lead-in `[0, +0x85)` and what the
//! sibling itself replays after winning fills `[+0x85, +0x86]`,
//! phase-mapped so the frame the wrap lands on is the frame it wraps to.
//! See [`victory_cycle`] and [`compose_base_stream`].

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

/// One base "ME" entry's retail loop window, read off the character's own
/// art bank: `count` is entry `+0x84` (the hold-counter seed - `0` means
/// the stream never loops), `start`/`end` are `+0x85`/`+0x86`.
///
/// `end` may equal the stream's frame count: the tick's window test runs
/// **before** its natural-end test (`0x80047768` vs `0x80047a48`), so a
/// window ending one past the last frame simply loops the whole tail and
/// the clip never reaches its commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopWindow {
    /// Entry `+0x84`, the `actor +0x176` seed (`0xFF` on every retail
    /// base record). Also the byte [`battle_char_assembly::ArtAnimRecord::uses_base_archive`]
    /// keys on, which is why the rebuild never rewrites it.
    pub count: u8,
    /// Entry `+0x85` - the frame the wrap jumps back to.
    pub start: usize,
    /// Entry `+0x86` - the frame the wrap fires on.
    pub end: usize,
}

impl LoopWindow {
    /// Whether this window actually replays frames of a stream `frames`
    /// long (retail: true on all 24 base records).
    pub fn loops(&self, frames: usize) -> bool {
        self.count != 0 && self.end > self.start && self.start < frames
    }
}

/// The loop window of every base "ME" entry of a character, keyed by the
/// entry index (= the art record's `stream_source`, a bijection over
/// `0..=7` in all three retail player files).
///
/// The window bytes live in the player file's own art bank, not in the
/// readef archive, so the rebuild has to go back to `record[0]` for them.
pub fn base_loop_windows(player_file: &[u8]) -> Result<BTreeMap<usize, LoopWindow>> {
    let rec0 = battle_char_assembly::decode_record0(player_file)
        .context("decode record[0] for the base loop windows")?;
    let bank = battle_char_assembly::art_animation_bank(&rec0).context("art bank")?;
    let mut out = BTreeMap::new();
    for r in bank.iter().filter(|r| r.uses_base_archive()) {
        let e = r.entry_offset;
        let Some(w) = rec0.get(e + 0x84..e + 0x87) else {
            continue;
        };
        out.insert(
            r.stream_source as usize,
            LoopWindow {
                count: w[0],
                start: w[1] as usize,
                end: w[2] as usize,
            },
        );
    }
    Ok(out)
}

/// The frames the sibling's OWN victory entry replays after it wins,
/// as a half-open range into `clip.frames`.
///
/// Retail declares one on two of the three siblings: Lu replays her
/// `[16, 24]` celebration sway, Gi declares the single-frame hold
/// `[30, 30]` - a freeze on his last pose - and Che's stand-in `0x23`
/// flourish declares none (her other flourishes hold at their last
/// frame). A one-frame range is therefore a legitimate answer, not a
/// degenerate one: it is exactly what the retail heroes' own win-pose
/// windows look like, which carry 0.07-0.64 degrees of drift per frame.
/// `None` = the sibling declares nothing and the caller holds the
/// flourish's own last frame.
///
/// The entry is matched by tag **and** stream shape rather than by index:
/// [`victory_clip`] indexes `animations()` with a position from
/// `action_tags()`, and `animations()` drops entries whose stream fails to
/// parse, so the two index spaces are not guaranteed to agree.
pub fn victory_cycle(
    archive_entry: &[u8],
    source_id: u16,
    clip: &crate::monster_archive::MonsterAnimation,
) -> Result<Option<std::ops::Range<usize>>> {
    let Some(block) = monster_archive::decode_block(archive_entry, source_id)? else {
        return Ok(None);
    };
    let Some(&magic_count) = block.get(0x4a) else {
        return Ok(None);
    };
    let mut found = None;
    for i in 0..magic_count as usize {
        let Some(off) = legaia_bytes::u32_le(&block, 0x4c + i * 4).map(|v| v as usize) else {
            break;
        };
        // `+0x8c` is the stream head (`[parts][frames]`); `+0x84..+0x87`
        // the loop fields.
        let Some(head) = block.get(off..off + 0x8e) else {
            continue;
        };
        if head[0] != clip.action_id
            || head[0x8c] as usize != clip.part_count
            || head[0x8d] as usize != clip.frame_count
        {
            continue;
        }
        found = Some(LoopWindow {
            count: head[0x84],
            start: head[0x85] as usize,
            end: head[0x86] as usize,
        });
    }
    let Some(w) = found else { return Ok(None) };
    // `start == 0` would leave no flourish at all, only the loop.
    if w.count == 0 || w.start == 0 || w.start >= clip.frame_count {
        return Ok(None);
    }
    // `end == start` is retail's single-frame hold arm (the tick snaps the
    // cursor back to `start` instead of subtracting a window length), so a
    // range of one frame is the faithful rendering of it.
    Ok(Some(w.start..w.end.clamp(w.start + 1, clip.frame_count)))
}

/// Pose distance between two frames, in model units: translations
/// directly, rotations as shortest-path angle converted to the arc a
/// nominal 100-unit limb sweeps. Used only to phase-align a cycle
/// against the pose the lead-in ends on.
fn pose_dist(a: &[PartPose], b: &[PartPose]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut d = 0.0f64;
    for i in 0..n {
        d += (a[i].tx - b[i].tx).abs() as f64
            + (a[i].ty - b[i].ty).abs() as f64
            + (a[i].tz - b[i].tz).abs() as f64;
        for (x, y) in [(a[i].rx, b[i].rx), (a[i].ry, b[i].ry), (a[i].rz, b[i].rz)] {
            let mut t = (x as i32 - y as i32).rem_euclid(4096);
            if t > 2048 {
                t = 4096 - t;
            }
            d += t as f64 / 4096.0 * std::f64::consts::TAU * 100.0;
        }
    }
    d / n as f64
}

/// Compose one base entry's source frame sequence: `lead` resampled over
/// the lead-in `[0, w.start)`, then `cycle` mapped over the loop window
/// so the wrap is seamless.
///
/// The tick wraps by subtracting `(+0x86 - +0x85)` frames, so host frame
/// `w.start + j` carries cycle position `(k + j * cycle.len() / L) %
/// cycle.len()` with `L = +0x86 - +0x85`: at `j == L` - the frame the
/// wrap fires on - that is position `0`, which is exactly the pose at
/// `j == 0` that the cursor lands back on. The seam is therefore an
/// identity, not an approximation, whenever the window ends inside the
/// stream; where retail put `+0x86` one past the last frame the seam is
/// one authored step of `cycle`, which is a cycle and closes on itself.
///
/// `k` is the cycle phase whose pose sits closest to the one the lead-in
/// ends on, so the single handoff into the loop is as quiet as the cycle
/// allows (and is the natural `0` when `cycle` is the continuation of
/// `lead` in the same clip).
pub fn compose_base_stream(
    lead: &[Vec<PartPose>],
    cycle: &[Vec<PartPose>],
    frames: usize,
    w: LoopWindow,
) -> Vec<Vec<PartPose>> {
    let hw0 = w.start.min(frames);
    let span = w.end.saturating_sub(w.start).max(1);
    let mut out: Vec<Vec<PartPose>> = Vec::with_capacity(frames);
    for h in 0..hw0 {
        out.push(lead[(h * lead.len() / hw0).min(lead.len() - 1)].clone());
    }
    let k = match out.last() {
        Some(tail) => (0..cycle.len())
            .min_by(|&i, &j| pose_dist(tail, &cycle[i]).total_cmp(&pose_dist(tail, &cycle[j])))
            .unwrap_or(0),
        None => 0,
    };
    for h in hw0..frames {
        let j = h - w.start;
        out.push(cycle[(k + j * cycle.len() / span) % cycle.len()].clone());
    }
    out
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
    // What fills each entry's loop window, for the six flourish entries:
    // the sibling's OWN post-win loop (Lu's `[16, 24]` sway, Gi's
    // single-frame hold), else a hold on the flourish's last frame -
    // which is also Che's own house style on the flourishes she does
    // annotate. Retail's 24 hero windows carry 0.07-0.64 degrees of drift
    // per frame, i.e. a held pose with a breath in it, so a hold is the
    // retail-shaped filler and not a compromise; taking it out of the
    // clip also means both boundaries of the composed stream - the
    // handoff into the window and the wrap inside it - are exact.
    let cycle_range =
        victory_cycle(archive_entry, source_id, clip).context("sibling victory loop window")?;
    let windows = base_loop_windows(player_file).context("host base loop windows")?;
    let mut bodies: Vec<Vec<u8>> = Vec::with_capacity(n);
    for i in 0..n {
        let retail = ar.entry(i).with_context(|| format!("decode entry {i}"))?;
        let (parts, frames) = (retail[0] as usize, retail[1] as usize);
        let source = if (4..=5).contains(&i) { &idle } else { clip };
        // Compose against the retail loop window when the entry has one
        // (all 24 retail base records do); otherwise the plain resample.
        let composed = windows.get(&i).filter(|w| w.loops(frames)).map(|&w| {
            let (lead, cycle) = match (i, &cycle_range) {
                // Weak-victory entries: idle in and idle round - the
                // idle is authored as a cycle, so the window closes on
                // itself and the whole entry stays the near-static
                // breathing retail gives these two.
                (4..=5, _) => (&idle.frames[..], &idle.frames[..]),
                (_, Some(r)) => (&clip.frames[..r.start], &clip.frames[r.clone()]),
                _ => (&clip.frames[..], &clip.frames[clip.frames.len() - 1..]),
            };
            let frames_in = compose_base_stream(lead, cycle, frames, w);
            crate::monster_archive::MonsterAnimation {
                frame_count: frames_in.len(),
                frames: frames_in,
                ..source.clone()
            }
        });
        let frames_out = retarget_clip(
            composed.as_ref().unwrap_or(source),
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

/// Rebuild ONE entry of a (main art) "ME" archive slot: entry
/// `entry_index` becomes the retargeted sibling `clip` at the entry's
/// RETAIL `(parts, frames)` shape - so the art record's timing, effect
/// script and cue track stay valid - and every other entry's stored
/// body is carried over byte-identical (size-table flags included).
/// Returns the full `READEF_SLOT`-byte slot, zero-padded.
/// The `(parts, frames)` shape a readef art slot's entry is authored at.
///
/// A retarget writes into this shape, so the caller needs it before it
/// can work out what playback rate keeps the source clip's pace.
pub fn art_entry_shape(slot: &[u8], entry_index: usize) -> Result<(usize, usize)> {
    let ar = me_archive::parse(slot).context("parse art ME archive")?;
    let decoded = ar
        .entry(entry_index)
        .with_context(|| format!("decode art entry {entry_index}"))?;
    if decoded.len() < 2 {
        bail!("art entry {entry_index} is empty");
    }
    Ok((decoded[0] as usize, decoded[1] as usize))
}

/// The rate byte that makes `src_frames` of source choreography, authored
/// at `src_rate`, take the same wall time once resampled into a
/// `host_frames` stream.
///
/// The cursor advances `rate / 8` keyframes per 60 Hz tick at the normal
/// `actor[+0x21D] == 4` (`FUN_80047430`), so a clip runs for
/// `frames * 8 / rate` ticks and holding that constant across the
/// resample gives `rate' = host_frames * rate / src_frames`. Retail rates
/// are not all 2 - Noa's Vulture Blade stream is authored at 6 - so this
/// cannot be a constant.
pub fn retimed_rate(host_frames: usize, src_frames: usize, src_rate: u8) -> u8 {
    if src_frames == 0 {
        return src_rate.max(1);
    }
    let scaled = (host_frames * src_rate as usize * 2)
        .div_ceil(src_frames)
        .div_ceil(2);
    scaled.clamp(1, u8::MAX as usize) as u8
}

/// A rebuilt art slot, plus the frame count the new entry ended up with.
///
/// The caller needs the count: the art record's own frame-indexed fields
/// (hit events, effect-script gates, the loop window) are relative to it,
/// so they have to be rescaled by whatever ratio the rebuild chose.
pub struct RebuiltArtSlot {
    pub bytes: Vec<u8>,
    /// Frames in the rebuilt entry.
    pub frames: usize,
    /// Frames the retail entry carried.
    pub retail_frames: usize,
    /// How many chain stages made it in (the front is dropped first when
    /// the slot is tight).
    pub stages: usize,
    /// The rate byte the concatenated stream wants.
    pub rate: u8,
}

/// Rebuild one entry of a readef art slot around a monster clip.
///
/// The entry is written at the **clip's own length** when the slot has
/// room for it, and only falls back to the retail frame count when it
/// does not. Resampling a 39-frame clip down to a 21-frame stream throws
/// away 18 of its poses and forces a compensating rate edit; keeping the
/// authored length costs about 2 KB in a slot that has 20 KB free, needs
/// no rate edit at all, and doubles the pose rate the player sees.
pub fn rebuild_art_slot_entry(
    slot: &[u8],
    entry_index: usize,
    chain: &[&crate::monster_archive::MonsterAnimation],
    rig: &PlayerRig,
    player_file: &[u8],
    archive_entry: &[u8],
    source_id: u16,
) -> Result<RebuiltArtSlot> {
    if slot.len() != READEF_SLOT {
        bail!("art slot is {} bytes, expected {READEF_SLOT}", slot.len());
    }
    if chain.is_empty() {
        bail!("no clip to retarget");
    }
    let ar = me_archive::parse(slot).context("parse art ME archive")?;
    let n = ar.len();
    if entry_index >= n {
        bail!("art archive has {n} entries, wanted {entry_index}");
    }
    let retail = ar
        .entry(entry_index)
        .with_context(|| format!("decode art entry {entry_index}"))?;
    let (parts, retail_frames) = (retail[0] as usize, retail[1] as usize);
    // Everything but the rebuilt entry is carried verbatim, so the room
    // available is the slot's free space plus what this entry gives back.
    let others: usize = (0..n)
        .filter(|&i| i != entry_index)
        .map(|i| ar.raw_body(i).map_or(0, |b| b.len()))
        .sum();
    let headroom = READEF_SLOT.saturating_sub(3 + 2 * n + others);

    // Try the whole chain, then drop stages from the FRONT until it fits
    // (the last stage is the payoff - a move that loses its wind-up still
    // reads; one that loses its strike does not), and only then fall back
    // to the retail shape.
    for start in 0..chain.len() {
        let stages = &chain[start..];
        let Some((frames, per_stage)) = chain_frames(stages) else {
            continue;
        };
        let built = build_chain(
            stages,
            &per_stage,
            rig,
            player_file,
            archive_entry,
            source_id,
            parts,
        )?;
        if built.len() <= headroom {
            return Ok(RebuiltArtSlot {
                bytes: assemble_slot(&ar, n, entry_index, &built)?,
                frames,
                retail_frames,
                stages: stages.len(),
                rate: chain_rate(stages),
            });
        }
    }
    // Last resort: the final stage squeezed into the retail frame count.
    let last = chain[chain.len() - 1];
    let built = build_chain(
        &[last],
        &[retail_frames],
        rig,
        player_file,
        archive_entry,
        source_id,
        parts,
    )?;
    Ok(RebuiltArtSlot {
        bytes: assemble_slot(&ar, n, entry_index, &built)?,
        frames: retail_frames,
        retail_frames,
        stages: 1,
        rate: winpose_rate(last),
    })
}

/// The playback rate a concatenated chain runs at: the fastest stage's,
/// so every slower stage can be stretched up to it rather than any stage
/// being decimated down.
fn chain_rate(stages: &[&crate::monster_archive::MonsterAnimation]) -> u8 {
    stages.iter().map(|c| winpose_rate(c)).max().unwrap_or(1)
}

fn winpose_rate(clip: &crate::monster_archive::MonsterAnimation) -> u8 {
    clip.rate.max(1)
}

/// Per-stage frame counts that preserve each stage's authored duration
/// once the whole chain plays at one rate, and their total.
///
/// A clip runs for `frames * 8 / rate` ticks, so a stage authored at
/// `rate_i` needs `frames_i * R / rate_i` frames to last as long at the
/// common rate `R`. `None` when the total will not fit the stream head's
/// `u8` frame count.
fn chain_frames(
    stages: &[&crate::monster_archive::MonsterAnimation],
) -> Option<(usize, Vec<usize>)> {
    let r = chain_rate(stages) as usize;
    let per: Vec<usize> = stages
        .iter()
        .map(|c| {
            (c.frame_count * r)
                .div_ceil(winpose_rate(c) as usize)
                .max(1)
        })
        .collect();
    let total: usize = per.iter().sum();
    (total <= u8::MAX as usize).then_some((total, per))
}

/// Retarget each stage at its own resampled length and concatenate them
/// into one stream.
#[allow(clippy::too_many_arguments)]
fn build_chain(
    stages: &[&crate::monster_archive::MonsterAnimation],
    per_stage: &[usize],
    rig: &PlayerRig,
    player_file: &[u8],
    archive_entry: &[u8],
    source_id: u16,
    parts: usize,
) -> Result<Vec<u8>> {
    let mut rows: Vec<Vec<PartPose>> = Vec::new();
    for (clip, &frames) in stages.iter().zip(per_stage) {
        rows.extend(retarget_clip(
            clip,
            rig,
            player_file,
            archive_entry,
            source_id,
            parts,
            frames,
        )?);
    }
    let frames = rows.len();
    let mut decoded = Vec::with_capacity(2 + parts * frames * 9);
    decoded.push(parts as u8);
    decoded.push(frames as u8);
    for row in &rows {
        for p in row {
            decoded.extend_from_slice(&pack_part(p));
        }
    }
    let encoded = encode_channel_delta(&decoded)?;
    let back = me_archive::decode_channel_delta(&encoded).context("re-decode rebuilt art entry")?;
    if back != decoded {
        bail!("rebuilt art entry: codec round-trip mismatch");
    }
    Ok(encoded)
}

/// Re-emit the whole slot with `entry_index`'s body replaced; every other
/// entry is carried verbatim, flag bit included.
fn assemble_slot(
    ar: &me_archive::MeArchive<'_>,
    n: usize,
    entry_index: usize,
    encoded: &[u8],
) -> Result<Vec<u8>> {
    let mut sizes: Vec<u16> = Vec::with_capacity(n);
    let mut bodies: Vec<&[u8]> = Vec::with_capacity(n);
    for i in 0..n {
        if i == entry_index {
            sizes.push((encoded.len() as u16) | 0x8000);
            bodies.push(encoded);
        } else {
            let body = ar
                .raw_body(i)
                .ok_or_else(|| anyhow::anyhow!("art entry {i} body missing"))?;
            let flag = if ar.is_compressed(i) == Some(true) {
                0x8000
            } else {
                0
            };
            sizes.push((body.len() as u16) | flag);
            bodies.push(body);
        }
    }
    let total = 3 + 2 * n + bodies.iter().map(|b| b.len()).sum::<usize>();
    if total > READEF_SLOT {
        bail!("rebuilt art archive ({total} bytes) exceeds the slot");
    }
    let mut out = Vec::with_capacity(READEF_SLOT);
    out.extend_from_slice(&me_archive::MAGIC);
    out.push(n as u8);
    for s in &sizes {
        out.extend_from_slice(&s.to_le_bytes());
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

    fn marker(v: i16) -> Vec<PartPose> {
        vec![PartPose {
            tx: v,
            ..PartPose::default()
        }]
    }

    /// The property the whole composition exists for: the frame the tick's
    /// wrap fires on carries the same pose as the frame it wraps back to,
    /// so the loop cannot show a seam. Not observable on the disc (retail
    /// ships no swapped stream), so a synthetic case holds it.
    #[test]
    fn the_composed_window_wraps_onto_its_own_first_frame() {
        let lead: Vec<Vec<PartPose>> = (0..7).map(marker).collect();
        let cycle: Vec<Vec<PartPose>> = (100..104).map(marker).collect();
        // `end < frames`: the wrap frame exists in the stream.
        let w = LoopWindow {
            count: 0xFF,
            start: 10,
            end: 29,
        };
        let out = compose_base_stream(&lead, &cycle, 30, w);
        assert_eq!(out.len(), 30);
        assert_eq!(out[0], lead[0], "lead-in starts on the clip's first frame");
        assert_eq!(out[9], lead[6], "lead-in ends on the clip's last frame");
        assert_eq!(out[29], out[10], "the wrap frame IS the wrap target");
        // Every window frame comes out of the cycle, none out of the lead.
        for f in &out[10..30] {
            assert!(cycle.contains(f), "window frame outside the cycle");
        }
    }

    /// A one-frame cycle - what a sibling declaring retail's single-frame
    /// hold arm (`+0x85 == +0x86`) contributes - fills the window with a
    /// motionless pose rather than replaying anything.
    #[test]
    fn a_one_frame_cycle_holds_the_window_still() {
        let lead: Vec<Vec<PartPose>> = (0..12).map(marker).collect();
        let cycle = vec![marker(99)];
        // `end == frames`: retail's other window shape, one past the last
        // frame, where the tick wraps before the natural end can commit.
        let w = LoopWindow {
            count: 0xFF,
            start: 5,
            end: 20,
        };
        let out = compose_base_stream(&lead, &cycle, 20, w);
        assert_eq!(out.len(), 20);
        assert!(
            out[5..].iter().all(|f| f == &cycle[0]),
            "the held window moved"
        );
    }
}

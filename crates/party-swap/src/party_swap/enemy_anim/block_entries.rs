//! The monster block's animation entry table: vertex scaling, the anchor,
//! parsing the entry spans, transforming a stream head, and rebuilding the
//! entries.

use super::super::*;
use std::collections::BTreeSet;

/// The single whole-body translation that seats the retargeted hero
/// streams on the monster block's own rest - the canonical-space mirror
/// of `winpose::idle_anchor`: x/z from the torso (canonical part 1, the
/// FK root), y from the DEEPEST ankle pivot over both cycles (canonical
/// feet 11/14; GTE y-down, so the largest `ty` is the floor). Battle
/// poses are flat absolute transforms, so this MUST be applied rigidly to
/// every part of every frame.
/// Scale every TMD vertex of the block's embedded mesh by `s`, in
/// place. Vertices are pivot-relative SVECTORs, so a uniform vertex
/// scale paired with the same scale on every clip translation is a
/// whole-figure uniform scale. Size-neutral: the vertex table is fixed
/// 8-byte records.
pub(super) fn scale_block_tmd_vertices(block: &mut [u8], s: f32) -> Result<()> {
    let tmd_off = legaia_bytes::u32_le(block, 4).context("tmd offset")? as usize;
    let tmd = legaia_tmd::parse(
        block
            .get(tmd_off..)
            .ok_or_else(|| anyhow::anyhow!("tmd offset {tmd_off:#x} out of range"))?,
    )
    .context("parse block TMD for vertex scale")?;
    const TMD_HEADER_SIZE: usize = 0xC;
    const VECTOR_SIZE: usize = 8;
    for o in &tmd.objects {
        let base = tmd_off + TMD_HEADER_SIZE + o.header.vert_top as usize;
        for v in 0..o.header.n_vert as usize {
            let at = base + v * VECTOR_SIZE;
            for k in 0..3 {
                let off = at + k * 2;
                let raw = i16::from_le_bytes(
                    block
                        .get(off..off + 2)
                        .ok_or_else(|| anyhow::anyhow!("vertex word out of range"))?
                        .try_into()
                        .unwrap(),
                );
                let scaled = ((raw as f32) * s).round().clamp(-32768.0, 32767.0) as i16;
                block[off..off + 2].copy_from_slice(&scaled.to_le_bytes());
            }
        }
    }
    Ok(())
}

pub(crate) fn monster_anchor(rows: &[Vec<PartPose>], host: &[Vec<PartPose>]) -> [i16; 3] {
    let floor = |frames: &[Vec<PartPose>]| -> Option<i16> {
        frames
            .iter()
            .filter_map(|f| {
                [11usize, 14]
                    .iter()
                    .filter_map(|&c| f.get(c))
                    .map(|p| p.ty)
                    .max()
            })
            .max()
    };
    let (Some(first), Some(rest)) = (
        rows.first().and_then(|f| f.get(1)),
        host.first().and_then(|f| f.get(1)),
    ) else {
        return [0; 3];
    };
    let dy = match (floor(rows), floor(host)) {
        (Some(a), Some(b)) => b - a,
        _ => rest.ty - first.ty,
    };
    [rest.tx - first.tx, dy, rest.tz - first.tz]
}

pub(super) fn apply_anchor(rows: &mut [Vec<PartPose>], d: [i16; 3]) {
    let put = |v: i16, d: i16| (v as i32 + d as i32).clamp(-2048, 2047) as i16;
    for row in rows.iter_mut() {
        for p in row.iter_mut() {
            p.tx = put(p.tx, d[0]);
            p.ty = put(p.ty, d[1]);
            p.tz = put(p.tz, d[2]);
        }
    }
}

// ---------------------------------------------------------------------------
// In-place entry rewriting.

/// One rewritten entry: the retargeted rows and the rate byte that keeps
/// the source clip's wall-clock duration (`frames * 8 / rate`).
pub(crate) struct EntryRewrite {
    pub rows: Vec<Vec<PartPose>>,
    pub rate: u8,
}

pub(super) const COUNT_OFF: usize = 0x4A;
pub(super) const ARRAY_OFF: usize = 0x4C;
pub(super) const ENTRY_STREAM_OFF: usize = 0x8C;
pub(super) const TABLE_WORD_BIAS: usize = 0x12;

pub(super) struct EntrySpan {
    pub(super) off: usize,
    span: usize,
    pub(super) parts: usize,
    pub(super) frames: usize,
}

/// Parse a block's entry table, requiring the ascending-contiguous layout
/// (the same law `slim` and `replace_mesh_and_pool` verify).
pub(super) fn parse_entry_table(block: &[u8]) -> Result<(Vec<EntrySpan>, usize, usize, usize)> {
    let pool_off = legaia_bytes::u32_le(block, 8).context("pool offset")? as usize;
    if pool_off == 0 || pool_off > block.len() {
        bail!("texture-pool offset {pool_off:#x} out of range");
    }
    let count = *block
        .get(COUNT_OFF)
        .ok_or_else(|| anyhow::anyhow!("block too short for the entry count"))?
        as usize;
    if count == 0 {
        bail!("block has no action entries");
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let off = legaia_bytes::u32_le(block, ARRAY_OFF + i * 4)
            .with_context(|| format!("entry {i} offset"))? as usize;
        let head = block
            .get(off..off + ENTRY_STREAM_OFF + 2)
            .ok_or_else(|| anyhow::anyhow!("entry {i} at +{off:#x} out of range"))?;
        let parts = head[ENTRY_STREAM_OFF] as usize;
        let frames = head[ENTRY_STREAM_OFF + 1] as usize;
        let span = ENTRY_STREAM_OFF + 2 + frames * parts * 9;
        if off + span > pool_off {
            bail!("entry {i} stream runs past the texture pool");
        }
        entries.push(EntrySpan {
            off,
            span,
            parts,
            frames,
        });
    }
    for w in entries.windows(2) {
        let gap = w[1].off as i64 - (w[0].off + w[0].span) as i64;
        if !(0..=3).contains(&gap) {
            bail!(
                "entries not ascending-contiguous: gap {gap} between +{:#x} and +{:#x}",
                w[0].off,
                w[1].off
            );
        }
    }
    let region_start = entries[0].off;
    let last = entries.last().expect("count >= 1");
    let tail_start = (last.off + last.span).div_ceil(4) * 4;
    if tail_start > pool_off {
        bail!("entry region overlaps the texture pool");
    }
    Ok((entries, region_start, tail_start, pool_off))
}

/// Round `v * new / old` to the nearest frame index.
fn rescale_frame_value(v: usize, old: usize, new: usize) -> usize {
    if old == 0 {
        return v;
    }
    (v * new + old / 2) / old
}

/// Rewrite the frame-indexed fields of a retail entry head for a stream
/// resized `old_frames` → `new_frames`: the playback-rate byte, the
/// `+0x10..+0x13` event-frame list (count preserved - it is how many
/// times damage applies - strictly ascending, capped inside the clip),
/// the `+0x14..` effect-script frame gates, and the `+0x84..+0x86` loop
/// window when the `+0x84` seed is non-zero (a zero-length hold window
/// stays zero-length). Everything else - tag, AGL cost, effect indices,
/// root-motion words, `+0x76`/`+0x77`, the `+0x87` sound cue - survives
/// byte for byte.
pub(super) fn transform_head(
    head: &[u8],
    old_frames: usize,
    new_frames: usize,
    rate: u8,
) -> Vec<u8> {
    let mut h = head[..ENTRY_STREAM_OFF].to_vec();
    h[0x78] = rate;
    // Event-frame list: zero-terminated, strictly ascending.
    let mut prev = 0usize;
    for j in 0..4 {
        let v = h[0x10 + j] as usize;
        if v == 0 {
            break;
        }
        let mut nv = rescale_frame_value(v, old_frames, new_frames).max(prev + 1);
        nv = nv.min(new_frames.saturating_sub(1).max(prev + 1)).min(255);
        h[0x10 + j] = nv as u8;
        prev = nv;
    }
    // Effect-script gates: 8 records at +0x14, gate 0 ends the walk.
    for r in 0..8 {
        let at = 0x14 + r * 8;
        let gate = h[at] as usize;
        if gate == 0 {
            break;
        }
        h[at] = rescale_frame_value(gate, old_frames, new_frames).clamp(1, 255) as u8;
    }
    // Loop window.
    if h[0x84] != 0 {
        let s = h[0x85] as usize;
        let e = h[0x86] as usize;
        let ns = rescale_frame_value(s, old_frames, new_frames).min(new_frames.saturating_sub(1));
        let len = e.saturating_sub(s);
        let nlen = if len == 0 {
            0
        } else {
            rescale_frame_value(len, old_frames, new_frames).max(1)
        };
        h[0x85] = ns.min(255) as u8;
        h[0x86] = (ns + nlen).min(new_frames).min(255) as u8;
    }
    h
}

/// Rebuild `current_block`'s entry region: every entry's head comes from
/// the RETAIL block (so the rewrite is idempotent - a second pass over an
/// already-mirrored block reproduces it byte for byte), streams come from
/// `rewrites` where present and from the retail block otherwise, and the
/// tail + pool of the current block shift by the region's size delta.
pub(crate) fn rebuild_block_entries(
    current_block: &[u8],
    retail_block: &[u8],
    rewrites: &BTreeMap<usize, EntryRewrite>,
) -> Result<Vec<u8>> {
    let (cur, cur_region_start, cur_tail_start, cur_pool_off) = parse_entry_table(current_block)?;
    let (ret, _, _, _) = parse_entry_table(retail_block)?;
    if cur.len() != ret.len() {
        bail!(
            "entry count mismatch: current {} vs retail {}",
            cur.len(),
            ret.len()
        );
    }
    let count = cur.len();
    if let Some(&bad) = rewrites.keys().find(|&&i| i >= count) {
        bail!("rewrite index {bad} out of range ({count} entries)");
    }
    let name_off = legaia_bytes::u32_le(current_block, 0).context("name offset")? as usize;
    let tmd_off = legaia_bytes::u32_le(current_block, 4).context("tmd offset")? as usize;
    if name_off >= cur_region_start || tmd_off >= cur_region_start {
        bail!("name/TMD offsets inside the entry region - layout not understood");
    }

    let mut out = current_block[..cur_region_start].to_vec();
    let mut new_offs = Vec::with_capacity(count);
    for (i, r) in ret.iter().enumerate() {
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        new_offs.push(out.len());
        match rewrites.get(&i) {
            Some(rw) => {
                let frames = rw.rows.len();
                if frames == 0 || frames > 255 {
                    bail!("entry {i}: rewritten stream has {frames} frames");
                }
                if rw.rows.iter().any(|row| row.len() != r.parts) {
                    bail!(
                        "entry {i}: rewritten rows do not carry the retail part count {}",
                        r.parts
                    );
                }
                let head = &retail_block[r.off..r.off + ENTRY_STREAM_OFF];
                out.extend_from_slice(&transform_head(head, r.frames, frames, rw.rate));
                out.push(r.parts as u8);
                out.push(frames as u8);
                for row in &rw.rows {
                    for p in row {
                        out.extend_from_slice(&winpose::pack_part(p));
                    }
                }
            }
            None => out.extend_from_slice(&retail_block[r.off..r.off + r.span]),
        }
    }
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    let new_tail_start = out.len();
    let delta = new_tail_start as i64 - cur_tail_start as i64;
    out.extend_from_slice(&current_block[cur_tail_start..cur_pool_off]);
    let new_pool_off = out.len();
    out.extend_from_slice(&current_block[cur_pool_off..]);

    // Fixups: pool offset, entry-offset array, effect-table words whose
    // values point into the moved tail (each word visited once).
    out[8..12].copy_from_slice(&(new_pool_off as u32).to_le_bytes());
    for (i, &off) in new_offs.iter().enumerate() {
        out[ARRAY_OFF + i * 4..ARRAY_OFF + i * 4 + 4].copy_from_slice(&(off as u32).to_le_bytes());
    }
    let mut effect_indices: BTreeSet<u32> = BTreeSet::new();
    for r in &ret {
        for at in [r.off + 4, r.off + 8] {
            let idx = legaia_bytes::u32_le(retail_block, at).context("effect index")?;
            if idx != 0 {
                if idx >= 0x100 {
                    bail!("effect index {idx:#x} is not a table index");
                }
                effect_indices.insert(idx);
            }
        }
    }
    for &idx in &effect_indices {
        let word_off = (idx as usize + count + TABLE_WORD_BIAS) * 4;
        let val = legaia_bytes::u32_le(current_block, word_off)
            .with_context(|| format!("effect table word {idx}"))? as usize;
        if val >= cur_tail_start {
            out[word_off..word_off + 4]
                .copy_from_slice(&((val as i64 + delta) as u32).to_le_bytes());
        } else if val >= cur_region_start {
            bail!("effect descriptor +{val:#x} lives inside the entry region");
        }
    }
    Ok(out)
}

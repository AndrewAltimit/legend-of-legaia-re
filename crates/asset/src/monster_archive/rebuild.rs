//! Monster-block mesh / texture-pool replacement: rebuild a decoded block
//! with a **different-sized** embedded TMD and/or texture pool, fixing every
//! offset the size change moves.
//!
//! The layout law (shared with [`super::slim`], which compacts the *entry*
//! region instead): the block is
//!
//! ```text
//! [head: name/tmd/pool offsets, stats, entry count @0x4A, offset array @0x4C,
//!  effect-offset table words, name string]
//! [TMD @ block[+0x04]]
//! [action entries - one contiguous ascending run, 1-3 byte alignment pad]
//! [tail: effect descriptors]
//! [texture pool @ block[+0x08] .. end of block]
//! ```
//!
//! Replacing the TMD shifts everything after it (entries, tail, pool) by one
//! 4-aligned delta; replacing the pool changes only the block's total length
//! (the pool is the last section). Fixed up: the `+0x08` pool offset, all
//! `+0x4C` entry offsets, and the effect-offset table word values that point
//! at the moved tail. Entry *contents* need no fixups - their `+0x04`/`+0x08`
//! effect references are table indices, not offsets, and the keyframe streams
//! are inline.
//!
//! The entry count, entry index space, and entry byte contents are preserved
//! verbatim - the streamed special-move modules stage raw entry indices (see
//! `super::slim`'s module docs for the softlock that follows from breaking
//! that), and a replacement mesh that keeps the retail object count animates
//! through the retail streams untouched.

use anyhow::{Context, Result, bail};

use std::collections::BTreeSet;

const TMD_OFF_FIELD: usize = 0x04;
const POOL_OFF_FIELD: usize = 0x08;
const COUNT_OFF: usize = 0x4A;
const ARRAY_OFF: usize = 0x4C;
const ENTRY_STREAM_OFF: usize = 0x8C;
/// The loader's effect-offset-table word formula: `word = idx + count + 0x12`
/// (`FUN_800542C8` fixup loop).
const TABLE_WORD_BIAS: usize = 0x12;

fn u32_at(b: &[u8], off: usize) -> Result<u32> {
    legaia_bytes::u32_le(b, off).with_context(|| format!("read u32 at +{off:#x}"))
}

/// Replace the embedded TMD and/or the texture pool of a decoded monster
/// block, returning the rebuilt block. `None` keeps the original section.
///
/// The new TMD must parse (`legaia_tmd::parse`) and is validated to keep the
/// retail **object count** - the animation streams address TMD objects by
/// index and a mismatched part count would pose garbage. The new pool is
/// taken verbatim (its internal layout is the caller's responsibility; see
/// [`super::MonsterTexture`] for the shape).
pub fn replace_mesh_and_pool(
    block: &[u8],
    new_tmd: Option<&[u8]>,
    new_pool: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let name_off = u32_at(block, 0)? as usize;
    let tmd_off = u32_at(block, TMD_OFF_FIELD)? as usize;
    let pool_off = u32_at(block, POOL_OFF_FIELD)? as usize;
    if pool_off == 0 || pool_off > block.len() {
        bail!("texture-pool offset {pool_off:#x} out of range");
    }
    if tmd_off == 0 || tmd_off >= pool_off {
        bail!("TMD offset {tmd_off:#x} out of range");
    }

    // Old TMD extent, cross-checked against the parser.
    let old_tmd = legaia_tmd::parse(&block[tmd_off..]).context("original TMD unparseable")?;
    let Some(old_last) = old_tmd.objects.last() else {
        bail!("original TMD has no objects");
    };
    let old_tmd_len = legaia_tmd::HEADER_SIZE + old_last.header.normal_top as usize;

    // Entry region: contiguous ascending run right after the TMD.
    let count = *block
        .get(COUNT_OFF)
        .ok_or_else(|| anyhow::anyhow!("block too short for the entry count"))?
        as usize;
    if count == 0 {
        bail!("block has no action entries");
    }
    let mut entry_offs = Vec::with_capacity(count);
    let mut effect_indices: BTreeSet<u32> = BTreeSet::new();
    for i in 0..count {
        let off = u32_at(block, ARRAY_OFF + i * 4)? as usize;
        let head = block
            .get(off..off + ENTRY_STREAM_OFF + 2)
            .ok_or_else(|| anyhow::anyhow!("entry {i} at +{off:#x} out of range"))?;
        let parts = head[ENTRY_STREAM_OFF] as usize;
        let frames = head[ENTRY_STREAM_OFF + 1] as usize;
        let span = ENTRY_STREAM_OFF + 2 + frames * parts * 9;
        if off + span > pool_off {
            bail!("entry {i} stream runs past the texture pool");
        }
        for idx in [u32_at(block, off + 4)?, u32_at(block, off + 8)?] {
            if idx != 0 {
                if idx >= 0x100 {
                    bail!("entry {i} effect index {idx:#x} is not a table index");
                }
                effect_indices.insert(idx);
            }
        }
        entry_offs.push((off, span));
    }
    // Unlike `slim` (which compacts the run and needs strict contiguity),
    // the rebuild copies the whole `[first entry .. pool)` region verbatim -
    // inter-entry gaps (some blocks carry 100-200 spare bytes between two
    // entries) shift with it and stay valid. Only the region's start matters.
    let region_start = entry_offs
        .iter()
        .map(|&(off, _)| off)
        .min()
        .expect("count >= 1");
    if name_off >= region_start || tmd_off >= region_start {
        bail!("name/TMD offsets inside the entry region - layout not understood");
    }
    // The TMD must end exactly at the entry region (modulo 0-3 pad bytes).
    let tmd_gap = region_start as i64 - (tmd_off + old_tmd_len) as i64;
    if !(0..=3).contains(&tmd_gap) {
        bail!(
            "TMD extent +{:#x}..+{:#x} does not abut the entry region at +{:#x}",
            tmd_off,
            tmd_off + old_tmd_len,
            region_start
        );
    }

    // Validate the replacement TMD.
    let tmd_bytes = match new_tmd {
        Some(b) => {
            let parsed = legaia_tmd::parse(b).context("replacement TMD unparseable")?;
            if parsed.objects.len() != old_tmd.objects.len() {
                bail!(
                    "replacement TMD has {} objects, retail has {} - the animation \
                     streams pose objects by index, the part count must match",
                    parsed.objects.len(),
                    old_tmd.objects.len()
                );
            }
            let Some(last) = parsed.objects.last() else {
                bail!("replacement TMD has no objects");
            };
            let true_len = legaia_tmd::HEADER_SIZE + last.header.normal_top as usize;
            if true_len != b.len() {
                bail!(
                    "replacement TMD carries {} trailing bytes past its own extent",
                    b.len() - true_len
                );
            }
            b
        }
        None => &block[tmd_off..tmd_off + old_tmd_len],
    };

    // Splice: head + new TMD (padded to 4) + [entries..pool) + new pool.
    let mut out = block[..tmd_off].to_vec();
    out.extend_from_slice(tmd_bytes);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    let new_region_start = out.len();
    let delta = new_region_start as i64 - region_start as i64;
    out.extend_from_slice(&block[region_start..pool_off]);
    let new_pool_off = out.len();
    debug_assert_eq!(new_pool_off as i64, pool_off as i64 + delta);
    out.extend_from_slice(new_pool.unwrap_or(&block[pool_off..]));

    // Fixups: pool offset, entry offset array, effect-table words into the
    // moved tail. Each table word is visited once (BTreeSet) - two entries
    // sharing an effect index must not shift its word twice.
    let shift = |v: usize| -> usize { (v as i64 + delta) as usize };
    out[POOL_OFF_FIELD..POOL_OFF_FIELD + 4]
        .copy_from_slice(&(shift(pool_off) as u32).to_le_bytes());
    for (i, &(off, _)) in entry_offs.iter().enumerate() {
        out[ARRAY_OFF + i * 4..ARRAY_OFF + i * 4 + 4]
            .copy_from_slice(&(shift(off) as u32).to_le_bytes());
    }
    for &idx in &effect_indices {
        let word_off = (idx as usize + count + TABLE_WORD_BIAS) * 4;
        let val = u32_at(block, word_off)? as usize;
        if val == 0 {
            continue;
        }
        if val >= region_start {
            out[word_off..word_off + 4].copy_from_slice(&(shift(val) as u32).to_le_bytes());
        } else if val > tmd_off {
            bail!("effect descriptor +{val:#x} points inside the TMD - layout not understood");
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_tmd::descriptor::PacketShape;
    use legaia_tmd::encode::{LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, encode};

    fn tiny_tmd(nobj: usize, verts_per_obj: usize) -> Vec<u8> {
        let objects: Vec<ModelObject> = (0..nobj)
            .map(|o| ModelObject {
                vertices: (0..verts_per_obj)
                    .map(|v| [v as i16, o as i16, 0])
                    .collect(),
                groups: vec![ModelGroup {
                    shape: PacketShape::F3,
                    semi_transparent: false,
                    prims: vec![ModelPrim {
                        vertices: vec![0, 1, 2],
                        uvs: vec![],
                        cba: 0,
                        tsb: 0,
                        colors: vec![[0x80; 3]],
                    }],
                }],
                scale: LEGAIA_OBJECT_SCALE,
            })
            .collect();
        encode(&objects).unwrap()
    }

    /// Synthetic block mirroring the retail layout: head, TMD, two entries,
    /// an effect descriptor in the tail, a texture pool.
    fn synthetic(tmd: &[u8]) -> Vec<u8> {
        let count = 2usize;
        let name_off = 0xA0usize;
        let tmd_off = 0xB0usize;
        let mk_entry = |id: u8, idx4: u32, frames: u8| {
            let mut e = vec![0u8; ENTRY_STREAM_OFF + 2 + frames as usize * 9];
            e[0] = id;
            e[4..8].copy_from_slice(&idx4.to_le_bytes());
            e[ENTRY_STREAM_OFF] = 1;
            e[ENTRY_STREAM_OFF + 1] = frames;
            e
        };
        let e0 = mk_entry(0x01, 0, 2);
        let e1 = mk_entry(0x23, 1, 3);
        let o0 = (tmd_off + tmd.len()).div_ceil(4) * 4;
        let o1 = (o0 + e0.len()).div_ceil(4) * 4;
        let tail = (o1 + e1.len()).div_ceil(4) * 4;
        let desc_off = tail;
        let pool = tail + 8;
        let total = pool + 32;
        let mut b = vec![0u8; total];
        b[0..4].copy_from_slice(&(name_off as u32).to_le_bytes());
        b[4..8].copy_from_slice(&(tmd_off as u32).to_le_bytes());
        b[8..12].copy_from_slice(&(pool as u32).to_le_bytes());
        b[COUNT_OFF] = count as u8;
        for (k, o) in [o0, o1].iter().enumerate() {
            b[ARRAY_OFF + k * 4..ARRAY_OFF + k * 4 + 4].copy_from_slice(&(*o as u32).to_le_bytes());
        }
        let word = (1 + count + TABLE_WORD_BIAS) * 4;
        b[word..word + 4].copy_from_slice(&(desc_off as u32).to_le_bytes());
        b[name_off..name_off + 4].copy_from_slice(b"Foo\0");
        b[tmd_off..tmd_off + tmd.len()].copy_from_slice(tmd);
        b[o0..o0 + e0.len()].copy_from_slice(&e0);
        b[o1..o1 + e1.len()].copy_from_slice(&e1);
        b[desc_off..desc_off + 8].copy_from_slice(&[0xAA; 8]);
        b[pool..].fill(0x55);
        b
    }

    #[test]
    fn identity_replacement_is_byte_identical() {
        let tmd = tiny_tmd(2, 4);
        let b = synthetic(&tmd);
        let out = replace_mesh_and_pool(&b, None, None).unwrap();
        assert_eq!(out, b);
        let out2 = replace_mesh_and_pool(&b, Some(&tmd), None).unwrap();
        assert_eq!(out2, b);
    }

    #[test]
    fn bigger_tmd_shifts_entries_tail_and_pool() {
        let tmd = tiny_tmd(2, 4);
        let b = synthetic(&tmd);
        let bigger = tiny_tmd(2, 16);
        assert!(bigger.len() > tmd.len());
        let out = replace_mesh_and_pool(&b, Some(&bigger), None).unwrap();
        let delta = (bigger.len() as i64 - tmd.len() as i64) as usize;
        assert_eq!(out.len(), b.len() + delta);
        // Entry array shifted; entry bytes intact.
        for k in 0..2 {
            let old = u32::from_le_bytes(
                b[ARRAY_OFF + k * 4..ARRAY_OFF + k * 4 + 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
            let new = u32::from_le_bytes(
                out[ARRAY_OFF + k * 4..ARRAY_OFF + k * 4 + 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
            assert_eq!(new, old + delta);
            assert_eq!(out[new], b[old], "entry {k} id byte");
        }
        // Pool offset + bytes intact.
        let new_pool = u32::from_le_bytes(out[8..12].try_into().unwrap()) as usize;
        let old_pool = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        assert_eq!(new_pool, old_pool + delta);
        assert!(out[new_pool..].iter().all(|&x| x == 0x55));
        // Effect descriptor reachable through the shifted table word.
        let word = (1 + 2 + TABLE_WORD_BIAS) * 4;
        let desc = u32::from_le_bytes(out[word..word + 4].try_into().unwrap()) as usize;
        assert_eq!(&out[desc..desc + 8], &[0xAA; 8]);
        // The new TMD is where the header says.
        let tmd_off = u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize;
        assert_eq!(&out[tmd_off..tmd_off + bigger.len()], &bigger[..]);
    }

    #[test]
    fn smaller_tmd_shrinks_the_block() {
        let tmd = tiny_tmd(2, 16);
        let b = synthetic(&tmd);
        let smaller = tiny_tmd(2, 4);
        let out = replace_mesh_and_pool(&b, Some(&smaller), None).unwrap();
        assert_eq!(out.len(), b.len() - (tmd.len() - smaller.len()));
        let new_pool = u32::from_le_bytes(out[8..12].try_into().unwrap()) as usize;
        assert!(out[new_pool..].iter().all(|&x| x == 0x55));
    }

    #[test]
    fn pool_replacement_changes_only_the_tail() {
        let tmd = tiny_tmd(2, 4);
        let b = synthetic(&tmd);
        let new_pool = vec![0x77u8; 64];
        let out = replace_mesh_and_pool(&b, None, Some(&new_pool)).unwrap();
        let pool_off = u32::from_le_bytes(out[8..12].try_into().unwrap()) as usize;
        assert_eq!(
            pool_off,
            u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize
        );
        assert_eq!(&out[..pool_off], &b[..pool_off]);
        assert_eq!(&out[pool_off..], &new_pool[..]);
    }

    #[test]
    fn rejects_object_count_mismatch() {
        let tmd = tiny_tmd(2, 4);
        let b = synthetic(&tmd);
        let wrong = tiny_tmd(3, 4);
        let err = replace_mesh_and_pool(&b, Some(&wrong), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("part count"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_tmd_with_trailing_garbage() {
        let tmd = tiny_tmd(2, 4);
        let b = synthetic(&tmd);
        let mut padded = tiny_tmd(2, 4);
        padded.extend_from_slice(&[0u8; 8]);
        assert!(replace_mesh_and_pool(&b, Some(&padded), None).is_err());
    }
}

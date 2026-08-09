//! Monster-block slimming: rebuild a decoded block minus its generic-AI
//! castable spell entries, shrinking the block's **battle-heap footprint**
//! (`block[+0x08]` bytes - the pre-texture region the loader `FUN_800542C8`
//! copies into the heap) without touching the mesh, the reactions, the
//! approach/special entries, or the stat record.
//!
//! The one consumer is the Delilas Challenge double-team round: the two slim
//! clones written into battle-unreachable archive slots so a `[163,164]`
//! formation fits the ~145 KB distinct-monster budget (the arithmetic lives
//! in `docs/subsystems/battle.md`, heap-budget section). The originals are
//! never modified.
//!
//! ## What is dropped, and why it is safe
//!
//! An entry is dropped iff it is a **castable** (`id` in `0x0C..=0x1F`) with
//! `agl_cost != 0xFF`. That set is exactly the enemy-AI spell picker's menu
//! (`overlay_0898_801e9fd4` only considers castables whose cost the actor's
//! AGL can pay; `0xFF` = never considered) - removing them narrows what the
//! generic AI can roll to basic attacks, which is invisible to everything
//! else:
//!
//! - The bespoke Delilas arm (`case 0xa2..0xa4` in the AI switch) writes only
//!   the every-3rd-phase signature-special action id; it references no record
//!   entry.
//! - Reaction staging (`FUN_80054CB0` caches tags `2,3,4,5,0xB`), the
//!   approach/victory first-byte searches (`0x20/0x21/0x22`), and the `0x23`
//!   special entries all survive untouched.
//! - `agl_cost == 0xFF` castables are kept: they are never AI-rolled but may
//!   be referenced as choreography by the streamed special modules (Lu's two
//!   `0x0C` entries carry effect-table links).
//!
//! ## Relocation rules
//!
//! Entries sit in one contiguous ascending run (1-3 byte alignment padding)
//! between the TMD and the block tail (effect descriptors, then the texture
//! pool). Dropping entries compacts that run; everything after shifts down
//! by a 4-aligned delta. **The entry count and the array's index space are
//! preserved**: the engine addresses animations by raw entry index (actor
//! `+0x1DA`; the streamed special-move modules were authored against the
//! retail layout), so a kept entry keeps its index and a dropped slot is
//! aliased to the basic-attack entry - harmless to the AI picker (id `0x01`
//! is not castable), invisible to the tag-search/reaction consumers, and a
//! valid animation for any stray index reference. Keeping the count also
//! pins the loader's effect-table word formula (`idx + count + 0x12`), so
//! entry effect indices stay verbatim. Fixed up: the `+0x08` texture-pool
//! offset, the `+0x4C` offset array, and the referenced table words' values
//! where they point into the moved tail. The name and TMD offsets precede
//! the entries and never move.

use anyhow::{Context, Result, bail};

/// One dropped entry, for reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedEntry {
    /// Spell/action id (entry `+0x00`).
    pub id: u8,
    /// AGL cost (entry `+0x74`).
    pub agl_cost: u8,
    /// Bytes removed from the block (entry head + packed keyframe stream).
    pub bytes: usize,
}

/// A slimmed block plus what was removed.
#[derive(Debug, Clone)]
pub struct SlimBlock {
    /// The rebuilt block.
    pub bytes: Vec<u8>,
    /// The dropped entries, in original entry order.
    pub dropped: Vec<DroppedEntry>,
    /// Heap-footprint reduction in bytes (old `+0x08` minus new `+0x08`).
    pub heap_saved: usize,
}

const COUNT_OFF: usize = 0x4A;
const ARRAY_OFF: usize = 0x4C;
const ENTRY_AGL_OFF: usize = 0x74;
const ENTRY_STREAM_OFF: usize = 0x8C;
/// The loader's effect-offset-table word formula: `word = idx + count + 0x12`
/// (`FUN_800542C8` fixup loop).
const TABLE_WORD_BIAS: usize = 0x12;

fn u32_at(b: &[u8], off: usize) -> Result<u32> {
    legaia_bytes::u32_le(b, off).with_context(|| format!("read u32 at +{off:#x}"))
}

fn is_dropped(id: u8, agl: u8) -> bool {
    (0x0C..=0x1F).contains(&id) && agl != 0xFF
}

/// Rebuild `block` without its generic-AI castable entries. Errors (rather
/// than producing a broken block) on any layout the relocation rules cannot
/// prove safe.
pub fn slim_castables(block: &[u8]) -> Result<SlimBlock> {
    let tex_off = u32_at(block, 8)? as usize;
    let name_off = u32_at(block, 0)? as usize;
    let tmd_off = u32_at(block, 4)? as usize;
    if tex_off == 0 || tex_off > block.len() {
        bail!("texture-pool offset {tex_off:#x} out of range");
    }
    let count = *block
        .get(COUNT_OFF)
        .ok_or_else(|| anyhow::anyhow!("block too short for the entry count"))?
        as usize;
    if count == 0 {
        bail!("block has no action entries");
    }

    // Parse the entries; require the ascending-contiguous layout.
    struct Entry {
        off: usize,
        span: usize, // unpadded: 0x8E + frames*parts*9
        id: u8,
        agl: u8,
        raw4: u32,
        raw8: u32,
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let off = u32_at(block, ARRAY_OFF + i * 4)? as usize;
        let head = block
            .get(off..off + ENTRY_STREAM_OFF + 2)
            .ok_or_else(|| anyhow::anyhow!("entry {i} at +{off:#x} out of range"))?;
        let parts = head[ENTRY_STREAM_OFF] as usize;
        let frames = head[ENTRY_STREAM_OFF + 1] as usize;
        let span = ENTRY_STREAM_OFF + 2 + frames * parts * 9;
        if off + span > tex_off {
            bail!("entry {i} stream runs past the texture pool");
        }
        entries.push(Entry {
            off,
            span,
            id: head[0],
            agl: block[off + ENTRY_AGL_OFF],
            raw4: u32_at(block, off + 4)?,
            raw8: u32_at(block, off + 8)?,
        });
    }
    for w in entries.windows(2) {
        let gap = w[1].off as i64 - (w[0].off + w[0].span) as i64;
        if !(0..=3).contains(&gap) {
            bail!(
                "entries not contiguous: gap {gap} between +{:#x} and +{:#x}",
                w[0].off,
                w[1].off
            );
        }
    }
    let region_start = entries[0].off;
    if name_off >= region_start || tmd_off >= region_start {
        bail!("name/TMD offsets inside the entry region - layout not understood");
    }
    let last = &entries[count - 1];
    let tail_start = (last.off + last.span).div_ceil(4) * 4;
    if tail_start > tex_off {
        bail!("entry region overlaps the texture pool");
    }

    // Indices must be small table indices, and none may resolve inside the
    // entry region (a descriptor inside a dropped span would dangle).
    for (i, e) in entries.iter().enumerate() {
        for idx in [e.raw4, e.raw8] {
            if idx == 0 {
                continue;
            }
            if idx >= 0x100 {
                bail!("entry {i} effect index {idx:#x} is not a table index");
            }
            let word_off = (idx as usize + count + TABLE_WORD_BIAS) * 4;
            let val = u32_at(block, word_off)? as usize;
            if (region_start..tail_start).contains(&val) {
                bail!("entry {i} effect descriptor +{val:#x} lives inside the entry region");
            }
        }
    }

    // Rebuild: head verbatim, kept entries compacted, tail shifted down.
    //
    // The entry COUNT and the array's INDEX SPACE are preserved: the engine
    // addresses animations by raw entry index (actor `+0x1DA`, and the
    // streamed special-move modules were authored against the retail layout),
    // so a kept entry must keep its index. A dropped castable's array slot is
    // aliased to the block's basic-attack entry instead of being removed -
    // the AI spell picker skips it (id `0x01` is not a castable), the
    // tag-search and reaction-map consumers never match it, and any stray
    // index reference lands on a harmless, always-valid animation. Keeping
    // the count also keeps the loader's effect-table word formula
    // (`idx + count + 0x12`) fixed, so entry effect indices need no bumping.
    let kept: Vec<usize> = (0..count)
        .filter(|&i| !is_dropped(entries[i].id, entries[i].agl))
        .collect();
    let dropped_n = count - kept.len();
    if dropped_n == 0 {
        bail!("nothing to drop - block has no AI-rollable castables");
    }
    let alias_idx = *kept
        .iter()
        .find(|&&i| entries[i].id == 0x01)
        .or_else(|| kept.first())
        .expect("at least one kept entry");
    let mut out = block[..region_start].to_vec();
    let mut new_off_by_index = vec![0usize; count];
    for &i in &kept {
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        new_off_by_index[i] = out.len();
        let e = &entries[i];
        out.extend_from_slice(&block[e.off..e.off + e.span]);
    }
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    let new_tail_start = out.len();
    let delta = tail_start - new_tail_start;
    if delta == 0 || !delta.is_multiple_of(4) {
        bail!("compaction delta {delta} not a positive 4-multiple");
    }
    out.extend_from_slice(&block[tail_start..]);

    // Fixups: the texture-pool offset, the offset array (kept entries at
    // their original indices, dropped slots aliased), and the effect-table
    // word values where they point into the moved tail.
    out[8..12].copy_from_slice(&((tex_off - delta) as u32).to_le_bytes());
    for i in 0..count {
        let target = if new_off_by_index[i] != 0 || kept.contains(&i) {
            new_off_by_index[i]
        } else {
            new_off_by_index[alias_idx]
        };
        out[ARRAY_OFF + i * 4..ARRAY_OFF + i * 4 + 4]
            .copy_from_slice(&(target as u32).to_le_bytes());
    }
    for &i in &kept {
        let e = &entries[i];
        for idx in [e.raw4, e.raw8] {
            if idx == 0 {
                continue;
            }
            let word_off = (idx as usize + count + TABLE_WORD_BIAS) * 4;
            let val = u32_at(&out, word_off)? as usize;
            if val >= tail_start {
                out[word_off..word_off + 4].copy_from_slice(&((val - delta) as u32).to_le_bytes());
            }
        }
    }

    let dropped = (0..count)
        .filter(|i| !kept.contains(i))
        .map(|i| DroppedEntry {
            id: entries[i].id,
            agl_cost: entries[i].agl,
            bytes: entries[i].span,
        })
        .collect();
    Ok(SlimBlock {
        bytes: out,
        dropped,
        heap_saved: delta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny synthetic block: head, 3 entries (attack 0x01, castable
    /// 0x0D @28, special 0x23 with effect idx 1), a one-word effect table, a
    /// descriptor in the tail, and a texture pool.
    fn synthetic() -> Vec<u8> {
        let count = 3usize;
        // Head is large enough for the offset array + the effect table word
        // for idx=1: word (1 + 3 + 0x12) = 22 -> byte 0x58.
        let entry_area = 0x60usize;
        let mk_entry = |id: u8, agl: u8, idx4: u32, frames: u8| {
            let mut e = vec![0u8; ENTRY_STREAM_OFF + 2 + frames as usize * 9];
            e[0] = id;
            e[4..8].copy_from_slice(&idx4.to_le_bytes());
            e[ENTRY_AGL_OFF] = agl;
            e[ENTRY_STREAM_OFF] = 1; // parts
            e[ENTRY_STREAM_OFF + 1] = frames;
            e
        };
        let e0 = mk_entry(0x01, 0, 0, 2);
        let e1 = mk_entry(0x0D, 28, 0, 4);
        let e2 = mk_entry(0x23, 0, 1, 3);
        let o0 = entry_area;
        let o1 = (o0 + e0.len()).div_ceil(4) * 4;
        let o2 = (o1 + e1.len()).div_ceil(4) * 4;
        let tail = (o2 + e2.len()).div_ceil(4) * 4;
        let desc_off = tail; // 8-byte descriptor
        let tex = tail + 8;
        let total = tex + 16;
        let mut b = vec![0u8; total];
        b[0..4].copy_from_slice(&0x40u32.to_le_bytes()); // name
        b[4..8].copy_from_slice(&0x48u32.to_le_bytes()); // "tmd"
        b[8..12].copy_from_slice(&(tex as u32).to_le_bytes());
        b[COUNT_OFF] = count as u8;
        for (k, o) in [o0, o1, o2].iter().enumerate() {
            b[ARRAY_OFF + k * 4..ARRAY_OFF + k * 4 + 4].copy_from_slice(&(*o as u32).to_le_bytes());
        }
        // Effect table word for idx=1 -> descriptor in the tail.
        let word = (1 + count + TABLE_WORD_BIAS) * 4;
        b[word..word + 4].copy_from_slice(&(desc_off as u32).to_le_bytes());
        b[o0..o0 + e0.len()].copy_from_slice(&e0);
        b[o1..o1 + e1.len()].copy_from_slice(&e1);
        b[o2..o2 + e2.len()].copy_from_slice(&e2);
        b[desc_off..desc_off + 8].copy_from_slice(&[0xAA; 8]);
        b[tex..].fill(0x55);
        b
    }

    #[test]
    fn drops_the_castable_and_relocates() {
        let b = synthetic();
        let slim = slim_castables(&b).unwrap();
        assert_eq!(slim.dropped.len(), 1);
        assert_eq!(slim.dropped[0].id, 0x0D);
        // Count and index space preserved; texture offset shifted by the
        // saved bytes.
        assert_eq!(slim.bytes[COUNT_OFF], 3);
        let old_tex = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        let new_tex = u32::from_le_bytes(slim.bytes[8..12].try_into().unwrap()) as usize;
        assert_eq!(old_tex - new_tex, slim.heap_saved);
        // Texture bytes intact at the new offset.
        assert!(slim.bytes[new_tex..].iter().all(|&x| x == 0x55));
        // The dropped castable's slot (index 1) aliases the attack entry
        // (index 0): same offset, id 0x01 at the target.
        let o0 =
            u32::from_le_bytes(slim.bytes[ARRAY_OFF..ARRAY_OFF + 4].try_into().unwrap()) as usize;
        let o1 = u32::from_le_bytes(slim.bytes[ARRAY_OFF + 4..ARRAY_OFF + 8].try_into().unwrap())
            as usize;
        assert_eq!(o1, o0, "dropped slot aliases the attack entry");
        assert_eq!(slim.bytes[o0], 0x01);
        // The special keeps its ORIGINAL index (2) and its verbatim effect
        // index (1): the table word position is count-stable and its value
        // still resolves to the descriptor, now shifted with the tail.
        let o23 = u32::from_le_bytes(
            slim.bytes[ARRAY_OFF + 8..ARRAY_OFF + 12]
                .try_into()
                .unwrap(),
        ) as usize;
        assert_eq!(slim.bytes[o23], 0x23);
        let idx = u32::from_le_bytes(slim.bytes[o23 + 4..o23 + 8].try_into().unwrap());
        assert_eq!(idx, 1, "effect index verbatim - no bumping");
        let word = (idx as usize + 3 + TABLE_WORD_BIAS) * 4;
        let desc = u32::from_le_bytes(slim.bytes[word..word + 4].try_into().unwrap()) as usize;
        assert_eq!(&slim.bytes[desc..desc + 8], &[0xAA; 8]);
        // The kept attack entry is byte-identical and unmoved.
        let old_o0 = u32::from_le_bytes(b[ARRAY_OFF..ARRAY_OFF + 4].try_into().unwrap()) as usize;
        assert_eq!(o0, old_o0);
        let span = ENTRY_STREAM_OFF + 2 + 2 * 9;
        assert_eq!(&slim.bytes[o0..o0 + span], &b[old_o0..old_o0 + span]);
    }

    #[test]
    fn refuses_a_block_with_nothing_to_drop() {
        let mut b = synthetic();
        // Make the castable unavailable (0xFF): nothing left to drop.
        let o1 = u32::from_le_bytes(b[ARRAY_OFF + 4..ARRAY_OFF + 8].try_into().unwrap()) as usize;
        b[o1 + ENTRY_AGL_OFF] = 0xFF;
        assert!(slim_castables(&b).is_err());
    }
}

//! PSX GPU primitive-pool decoder.
//!
//! The PSX renderer composes each frame by walking an ordering-table (OT) of
//! linked primitive packets. PsyQ packs each primitive with a 4-byte "chain
//! tag" at offset 0 (`u8 len_words | u24 next_addr`), followed by the raw
//! GP0 command word and the per-vertex payload. Every prim in the chain is
//! word-aligned and contiguous in the prim pool, so we can recover the full
//! set by scanning the pool for valid tags + cmd-byte pairs.
//!
//! This module is the building block for "replay the in-game top-down view
//! in WebGL": we extract the live prim pool from a mednafen save state,
//! decode each primitive into a structured record (screen-space vertices,
//! UVs, CLUT/tpage, color), and the engine-side renderer rasterises them
//! against the save state's VRAM. The output is pixel-perfectly equivalent
//! to what the PSX GPU drew at capture time.
//!
//! ### Pool location
//!
//! For the Drake-Kingdom top-view save state the pool starts at `0x800AD400` and runs ~341 KB. Decoded
//! corpus: ~4400 `POLY_FT4`, ~250 `POLY_GT4`, ~150 `SPRT_16` ≈ 4800 prims
//! per frame. The per-region RAM layout is in `docs/subsystems/world-map.md`.
//!
//! ### Tag validation
//!
//! A 4-byte word is treated as a candidate chain tag when:
//! - `len = high_byte` is in `[1, 12]` (PSX limit; longest standard prim
//!   is `POLY_GT4` = 12 payload words),
//! - `next_addr = low_24_bits` is either `0xFFFFFF` (terminator) or sits
//!   inside the pool bounds. The next-addr is a kuseg-stripped pointer.
//!
//! If the cmd byte at `tag_offset + 4` matches a known opcode AND the
//! recorded length matches what that opcode expects, we accept it as a
//! prim and skip past its payload to the next candidate position.

use serde::Serialize;

mod packet_decode;
mod ram_scan;
#[cfg(test)]
mod tests;

use packet_decode::{decode_in, decode_packet, read_u32};
pub use ram_scan::{OtArray, PoolRegion, find_ot_arrays, find_pools};

/// One decoded primitive packet (subset of PSX GPU commands the top-view
/// renderer uses). Vertex coords are screen-space (post-GTE), UVs are in
/// PSX framebuffer halfword units, color is the GP0 modulator RGB.
#[derive(Debug, Clone, Serialize)]
pub enum Prim {
    /// Textured 4-vertex quad with single modulator color. Cmd 0x2C..0x2F.
    /// 10 u32 words on disc (1 tag + 9 payload).
    PolyFt4 {
        cmd: u8,
        color: [u8; 3],
        verts: [(i16, i16); 4],
        uvs: [(u8, u8); 4],
        clut: u16,
        tpage: u16,
    },
    /// Textured 4-vertex quad with per-vertex Gouraud colors. Cmd 0x3C..0x3F.
    /// 13 u32 words (1 tag + 12 payload).
    PolyGt4 {
        cmd: u8,
        colors: [[u8; 3]; 4],
        verts: [(i16, i16); 4],
        uvs: [(u8, u8); 4],
        clut: u16,
        tpage: u16,
    },
    /// Textured 3-vertex tri with single modulator color. Cmd 0x24..0x27.
    /// 7 u32 words (1 tag + 6 payload).
    PolyFt3 {
        cmd: u8,
        color: [u8; 3],
        verts: [(i16, i16); 3],
        uvs: [(u8, u8); 3],
        clut: u16,
        tpage: u16,
    },
    /// Textured 3-vertex tri with Gouraud colors. Cmd 0x34..0x37.
    /// 10 u32 words (1 tag + 9 payload).
    PolyGt3 {
        cmd: u8,
        colors: [[u8; 3]; 3],
        verts: [(i16, i16); 3],
        uvs: [(u8, u8); 3],
        clut: u16,
        tpage: u16,
    },
    /// Fixed 16x16 textured sprite. Cmd **0x7C..0x7F**. 4 u32 words
    /// (1 tag + 3 payload).
    Sprt16 {
        cmd: u8,
        color: [u8; 3],
        pos: (i16, i16),
        uv: (u8, u8),
        clut: u16,
    },
    /// Fixed 8x8 textured sprite. Cmd **0x74..0x77**. 4 u32 words.
    Sprt8 {
        cmd: u8,
        color: [u8; 3],
        pos: (i16, i16),
        uv: (u8, u8),
        clut: u16,
    },
    /// Free-size textured sprite (libgpu `SPRT`). Cmd **0x64..0x67**, 5 u32
    /// words - the fixed-size arms above plus a trailing `w | h<<16`.
    ///
    /// Retail draws every window nine-slice with this opcode (4x4 corners,
    /// 24x4 / 4x24 edges), so a decoder without this arm reports a frame with
    /// no chrome at all rather than an error.
    Sprt {
        cmd: u8,
        color: [u8; 3],
        pos: (i16, i16),
        uv: (u8, u8),
        clut: u16,
        size: (u16, u16),
    },
    /// Flat-shaded untextured triangle. Cmd 0x20..0x23. 4 payload words.
    PolyF3 {
        cmd: u8,
        color: [u8; 3],
        verts: [(i16, i16); 3],
    },
    /// Flat-shaded untextured quad. Cmd 0x28..0x2B. 5 payload words.
    PolyF4 {
        cmd: u8,
        color: [u8; 3],
        verts: [(i16, i16); 4],
    },
    /// Gouraud-shaded untextured triangle. Cmd 0x30..0x33. 6 payload words.
    PolyG3 {
        cmd: u8,
        colors: [[u8; 3]; 3],
        verts: [(i16, i16); 3],
    },
    /// Gouraud-shaded untextured quad. Cmd 0x38..0x3B. 8 payload words.
    PolyG4 {
        cmd: u8,
        colors: [[u8; 3]; 4],
        verts: [(i16, i16); 4],
    },
}

impl Prim {
    /// Returns the cmd byte (high byte of the GP0 packet header).
    pub fn cmd(&self) -> u8 {
        match self {
            Prim::PolyFt4 { cmd, .. }
            | Prim::PolyGt4 { cmd, .. }
            | Prim::PolyFt3 { cmd, .. }
            | Prim::PolyGt3 { cmd, .. }
            | Prim::Sprt16 { cmd, .. }
            | Prim::Sprt8 { cmd, .. }
            | Prim::Sprt { cmd, .. }
            | Prim::PolyF3 { cmd, .. }
            | Prim::PolyF4 { cmd, .. }
            | Prim::PolyG3 { cmd, .. }
            | Prim::PolyG4 { cmd, .. } => *cmd,
        }
    }

    /// Short opcode name, e.g. `"POLY_FT4"`.
    pub fn kind(&self) -> &'static str {
        match self {
            Prim::PolyFt4 { .. } => "POLY_FT4",
            Prim::PolyGt4 { .. } => "POLY_GT4",
            Prim::PolyFt3 { .. } => "POLY_FT3",
            Prim::PolyGt3 { .. } => "POLY_GT3",
            Prim::Sprt16 { .. } => "SPRT_16",
            Prim::Sprt8 { .. } => "SPRT_8",
            Prim::Sprt { .. } => "SPRT",
            Prim::PolyF3 { .. } => "POLY_F3",
            Prim::PolyF4 { .. } => "POLY_F4",
            Prim::PolyG3 { .. } => "POLY_G3",
            Prim::PolyG4 { .. } => "POLY_G4",
        }
    }

    /// True when the primitive samples a texture page (so it can carry a
    /// ground/wall atlas). Untextured flat and Gouraud polys cannot.
    pub fn is_textured(&self) -> bool {
        matches!(
            self,
            Prim::PolyFt4 { .. }
                | Prim::PolyGt4 { .. }
                | Prim::PolyFt3 { .. }
                | Prim::PolyGt3 { .. }
                | Prim::Sprt16 { .. }
                | Prim::Sprt8 { .. }
                | Prim::Sprt { .. }
        )
    }

    /// `(clut, tpage)` for textured primitives. Sprites carry no tpage of their
    /// own (they inherit the last `DR_TPAGE`), so their tpage reports `None`.
    pub fn clut_tpage(&self) -> Option<(u16, Option<u16>)> {
        match self {
            Prim::PolyFt4 { clut, tpage, .. }
            | Prim::PolyGt4 { clut, tpage, .. }
            | Prim::PolyFt3 { clut, tpage, .. }
            | Prim::PolyGt3 { clut, tpage, .. } => Some((*clut, Some(*tpage))),
            Prim::Sprt16 { clut, .. } | Prim::Sprt8 { clut, .. } | Prim::Sprt { clut, .. } => {
                Some((*clut, None))
            }
            _ => None,
        }
    }

    /// Screen-space vertices of the primitive. Sprites expand to their
    /// top-left corner plus the implied extent.
    pub fn verts(&self) -> Vec<(i16, i16)> {
        match self {
            Prim::PolyFt4 { verts, .. }
            | Prim::PolyGt4 { verts, .. }
            | Prim::PolyF4 { verts, .. }
            | Prim::PolyG4 { verts, .. } => verts.to_vec(),
            Prim::PolyFt3 { verts, .. }
            | Prim::PolyGt3 { verts, .. }
            | Prim::PolyF3 { verts, .. }
            | Prim::PolyG3 { verts, .. } => verts.to_vec(),
            Prim::Sprt16 { pos, .. } => vec![*pos, (pos.0 + 16, pos.1 + 16)],
            Prim::Sprt8 { pos, .. } => vec![*pos, (pos.0 + 8, pos.1 + 8)],
            Prim::Sprt { pos, size, .. } => vec![
                *pos,
                (
                    pos.0.saturating_add(size.0 as i16),
                    pos.1.saturating_add(size.1 as i16),
                ),
            ],
        }
    }

    /// Screen-space axis-aligned bounds `(min_x, min_y, max_x, max_y)`.
    pub fn bounds(&self) -> (i16, i16, i16, i16) {
        let vs = self.verts();
        let mut b = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
        for (x, y) in vs {
            b.0 = b.0.min(x);
            b.1 = b.1.min(y);
            b.2 = b.2.max(x);
            b.3 = b.3.max(y);
        }
        b
    }
}

/// The pool's runtime base address (kuseg). Tag `next_addr` fields use the
/// kuseg-stripped form (`0x00Axxxxx`) but in-pool offsets are byte-relative
/// to this base. Verified at `0x800AD400` for the Drake/Sebucus/Karisto
/// world-map top-view save states; the address is consistent across them
/// because PsyQ's `GsGetNextWorkBuf` returns the same heap slot each frame.
pub const POOL_BASE_DEFAULT: u32 = 0x800AD400;

/// Decode every primitive packet found in the pool buffer. Tags overlap
/// with their own payload words in the brute-force scan; we mark every
/// word consumed by an accepted primitive so a later tag inside the same
/// payload doesn't cause double-emit.
pub fn decode(pool: &[u8], pool_base: u32) -> Vec<Prim> {
    decode_in(pool, pool_base, &mut Vec::new())
}

/// One in-pool tag record: byte offset within the pool buffer + the
/// decoded chain-tag fields. Useful for walking the OT-link graph.
#[derive(Debug, Clone, Copy)]
pub struct TagRec {
    /// Byte offset of the tag word inside the pool buffer.
    pub offset: usize,
    /// Number of payload words this tag advertises (high byte of word 0).
    pub length: u8,
    /// Kuseg-stripped 24-bit "next packet" pointer.
    pub next_addr: u32,
}

/// Walk every tag that produces an accepted primitive and return its
/// chain-tag fields. The output is in pool-offset order (ascending).
pub fn decode_tags(pool: &[u8], pool_base: u32) -> Vec<TagRec> {
    let pool_lo = pool_base & 0x00FF_FFFF;
    let pool_hi = pool_lo + pool.len() as u32;
    let mut consumed = vec![false; pool.len() / 4];
    let mut out = Vec::new();
    let n_words = pool.len() / 4;
    for w in 0..n_words {
        if consumed[w] {
            continue;
        }
        let i = w * 4;
        if i + 8 > pool.len() {
            break;
        }
        let tag = read_u32(pool, i);
        let length = ((tag >> 24) & 0xFF) as usize;
        let next_addr = tag & 0x00FF_FFFF;
        if !(1..=12).contains(&length) {
            continue;
        }
        if next_addr != 0x00FF_FFFF && !(pool_lo..pool_hi).contains(&next_addr) {
            continue;
        }
        let payload_end = i + 4 + length * 4;
        if payload_end > pool.len() {
            continue;
        }
        let cmd_word = read_u32(pool, i + 4);
        let cmd = ((cmd_word >> 24) & 0xFF) as u8;
        let (kind_ok, prim) = decode_packet(pool, i, cmd, length);
        if !kind_ok {
            continue;
        }
        if prim.is_some() {
            for k in 0..=length {
                let cw = w + k;
                if cw < consumed.len() {
                    consumed[cw] = true;
                }
            }
            out.push(TagRec {
                offset: i,
                length: length as u8,
                next_addr,
            });
        }
    }
    out
}

/// Result of finding the chain head (the tag that no other tag's `next_addr`
/// points at) in a pool buffer. `heads` is the set of head candidates - in a
/// well-formed pool there's exactly one. `terminators` is the count of tags
/// whose `next_addr == 0xFFFFFF` (chain tails). `linked` is the number of
/// tags that ARE referenced by some other tag's `next_addr`.
#[derive(Debug, Clone)]
pub struct ChainTopology {
    pub total_tags: usize,
    pub heads: Vec<usize>,
    pub terminators: usize,
    pub linked: usize,
}

/// Identify the chain head(s) of an OT-linked prim pool.
///
/// The OT layout is: every accepted-prim tag has a `next_addr` pointing at
/// the next tag in chain order, or `0xFFFFFF` for the tail. The head is the
/// unique tag whose offset doesn't appear in any other tag's `next_addr`.
/// Used to verify `POOL_BASE_DEFAULT` is correctly placed: if the head's
/// pool-offset is 0, the pool starts exactly at `pool_base`; otherwise the
/// real pool starts at `pool_base + head_offset`.
pub fn chain_topology(pool: &[u8], pool_base: u32) -> ChainTopology {
    let tags = decode_tags(pool, pool_base);
    let pool_lo = pool_base & 0x00FF_FFFF;
    let mut linked_offsets: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut terminators = 0usize;
    for t in &tags {
        if t.next_addr == 0x00FF_FFFF {
            terminators += 1;
            continue;
        }
        let next_off = t.next_addr.wrapping_sub(pool_lo) as usize;
        linked_offsets.insert(next_off);
    }
    let mut heads = Vec::new();
    for t in &tags {
        if !linked_offsets.contains(&t.offset) {
            heads.push(t.offset);
        }
    }
    ChainTopology {
        total_tags: tags.len(),
        heads,
        terminators,
        linked: linked_offsets.len(),
    }
}

/// One unique "tile type" found by clustering POLY_FT4 packets by their
/// texture-immutable fingerprint: the `(clut, tpage, sorted uvs)` tuple.
/// `count` is how many tiles in this frame share this fingerprint - tile
/// types reused 100+ times across the continent terrain are the prime
/// candidates for a per-tile descriptor table in source data.
#[derive(Debug, Clone, Serialize)]
pub struct TileSignature {
    pub clut: u16,
    pub tpage: u16,
    /// UVs in their packet-order tuple `[(u0,v0),(u1,v1),(u2,v2),(u3,v3)]`.
    /// Sorted lexicographically across the four vertices to make rotated
    /// copies of the same tile collapse into one cluster.
    pub uvs: [(u8, u8); 4],
    pub count: usize,
    /// Multiple candidate byte fingerprints, ordered from richest to
    /// poorest. The search picks the first one that hits in a window
    /// and reports stride. Variants:
    ///
    /// 0. **Rich**: full 12 bytes `[u0,v0,u1,v1,u2,v2,u3,v3,clut,tpage]`.
    /// 1. **Packet-template**: 8 bytes `[u0,v0,clut.lo,clut.hi,u_diag,v_diag,tpage.lo,tpage.hi]`
    ///    matching the layout of the two halfword-packed UV+CLUT and
    ///    UV+TPAGE words from the live FT4 packet.
    /// 2. **UV+CLUT-only**: 4 bytes `[u0,v0,clut.lo,clut.hi]`.
    /// 3. **UV+TPAGE-only**: 4 bytes `[u_diag,v_diag,tpage.lo,tpage.hi]`.
    /// 4. **UV-only**: 2 bytes `[u_min,v_min]` of the lex-min vertex.
    pub fingerprints: Vec<Vec<u8>>,
}

/// Cluster POLY_FT4 prims by `(clut, tpage, sorted uvs)`. Output sorted by
/// descending count. The continent terrain is ~10k POLY_FT4 per frame, but
/// reuses a small number of source tile descriptors - this clustering
/// surfaces the per-tile palette directly.
pub fn tile_signatures(prims: &[Prim]) -> Vec<TileSignature> {
    use std::collections::HashMap;
    type TileKey = (u16, u16, [(u8, u8); 4]);
    let mut bucket: HashMap<TileKey, usize> = HashMap::new();
    for p in prims {
        if let Prim::PolyFt4 {
            clut, tpage, uvs, ..
        } = p
        {
            let mut sorted = *uvs;
            sorted.sort_by_key(|&(u, v)| ((u as u32) << 8) | v as u32);
            *bucket.entry((*clut, *tpage, sorted)).or_insert(0) += 1;
        }
    }
    let mut out: Vec<TileSignature> = bucket
        .into_iter()
        .map(|((clut, tpage, uvs), count)| {
            // 0: Rich 12-byte fingerprint.
            let mut rich = Vec::with_capacity(12);
            for (u, v) in &uvs {
                rich.push(*u);
                rich.push(*v);
            }
            rich.extend_from_slice(&clut.to_le_bytes());
            rich.extend_from_slice(&tpage.to_le_bytes());
            // 1: Packet-template (FT4's uv0+clut word followed by uv-diag+tpage word).
            // `uvs[0]` is the lex-min vertex; the diagonal-opposite vertex
            // shares the orthogonal coords of the FT4's emit order, which
            // is `uvs[3]` for a fully-sorted square tile.
            let mut packet = Vec::with_capacity(8);
            packet.push(uvs[0].0);
            packet.push(uvs[0].1);
            packet.extend_from_slice(&clut.to_le_bytes());
            packet.push(uvs[3].0);
            packet.push(uvs[3].1);
            packet.extend_from_slice(&tpage.to_le_bytes());
            // 2: UV+CLUT only (4 bytes).
            let mut uv_clut = Vec::with_capacity(4);
            uv_clut.push(uvs[0].0);
            uv_clut.push(uvs[0].1);
            uv_clut.extend_from_slice(&clut.to_le_bytes());
            // 3: UV+TPAGE only (4 bytes).
            let mut uv_tpage = Vec::with_capacity(4);
            uv_tpage.push(uvs[3].0);
            uv_tpage.push(uvs[3].1);
            uv_tpage.extend_from_slice(&tpage.to_le_bytes());
            // 4: CLUT+TPAGE pair only (4 bytes), useful for locating
            // a per-frame tile palette table when the source's per-tile
            // record stores texpage/clut together.
            let mut ct = Vec::with_capacity(4);
            ct.extend_from_slice(&clut.to_le_bytes());
            ct.extend_from_slice(&tpage.to_le_bytes());
            TileSignature {
                clut,
                tpage,
                uvs,
                count,
                fingerprints: vec![rich, packet, uv_clut, uv_tpage, ct],
            }
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.count));
    out
}

/// One primitive in submission order, with the pool offset it came from.
#[derive(Debug, Clone, Serialize)]
pub struct ChainedPrim {
    /// Position along the walked chain - this **is** the effective draw order.
    pub order: usize,
    /// Byte offset of the packet's tag inside the pool buffer.
    pub offset: usize,
    pub prim: Prim,
}

/// Walk the OT chain from `head_offset` and return the primitives in the order
/// the GPU consumes them.
///
/// This is the part that answers "which copy wins". The PSX has no depth
/// buffer: the ordering table *is* the depth policy, and within one frame the
/// packet drawn **later** in the chain overwrites the pixels of the one drawn
/// earlier. So for two coincident surfaces, the winner is simply the one with
/// the higher `order`. Walking the chain rather than scanning the pool in
/// address order is what makes that readable - libgpu appends packets to
/// whichever OT bucket their z lands in, so address order and draw order are
/// unrelated.
///
/// The walk is cycle-guarded: a corrupt or misidentified pool can produce a
/// link loop, and a loop must terminate the walk rather than hang it.
pub fn chain_walk(pool: &[u8], pool_base: u32, head_offset: usize) -> Vec<ChainedPrim> {
    let pool_lo = pool_base & 0x00FF_FFFF;
    let mut seen = vec![false; pool.len() / 4 + 1];
    let mut out = Vec::new();
    let mut cursor = head_offset;
    let mut order = 0usize;
    loop {
        if cursor + 8 > pool.len() {
            break;
        }
        let wi = cursor / 4;
        if wi >= seen.len() || seen[wi] {
            break; // terminator, out of range, or a link cycle
        }
        seen[wi] = true;
        let tag = read_u32(pool, cursor);
        let length = ((tag >> 24) & 0xFF) as usize;
        let next_addr = tag & 0x00FF_FFFF;
        if (1..=12).contains(&length) && cursor + 4 + length * 4 <= pool.len() {
            let cmd = ((read_u32(pool, cursor + 4) >> 24) & 0xFF) as u8;
            if let (_, Some(prim)) = decode_packet(pool, cursor, cmd, length) {
                out.push(ChainedPrim {
                    order,
                    offset: cursor,
                    prim,
                });
                order += 1;
            }
        }
        if next_addr == 0x00FF_FFFF {
            break;
        }
        let next_off = next_addr.wrapping_sub(pool_lo) as usize;
        if next_off >= pool.len() {
            break;
        }
        cursor = next_off;
    }
    out
}

/// Census of the chain links [`chain_walk`] walks past **without** decoding,
/// keyed by `(GP0 command byte, tag payload length)`.
///
/// This is the instrument's own blind spot, reported rather than hidden. A
/// "retail draws none of family X" reading off a decoded list is only as good
/// as the decoder's acceptance table: until `POLY_FT3` was accepted at its
/// real seven payload words, every textured flat triangle walked into this
/// bucket and every list read as "no triangles". Empty links (length `0`, the
/// OT bucket heads) are not counted; everything else that fails to decode is,
/// including GP0 attribute packets (`0xE1..=0xE6`), which are expected here.
pub fn chain_undecoded(
    pool: &[u8],
    pool_base: u32,
    head_offset: usize,
) -> std::collections::BTreeMap<(u8, usize), usize> {
    let pool_lo = pool_base & 0x00FF_FFFF;
    let mut seen = vec![false; pool.len() / 4 + 1];
    let mut out = std::collections::BTreeMap::new();
    let mut cursor = head_offset;
    loop {
        if cursor + 8 > pool.len() {
            break;
        }
        let wi = cursor / 4;
        if wi >= seen.len() || seen[wi] {
            break;
        }
        seen[wi] = true;
        let tag = read_u32(pool, cursor);
        let length = ((tag >> 24) & 0xFF) as usize;
        let next_addr = tag & 0x00FF_FFFF;
        if length > 0 && cursor + 4 + length * 4 <= pool.len() {
            let cmd = ((read_u32(pool, cursor + 4) >> 24) & 0xFF) as u8;
            let decoded = (1..=12).contains(&length)
                && matches!(decode_packet(pool, cursor, cmd, length), (_, Some(_)));
            if !decoded {
                *out.entry((cmd, length)).or_insert(0) += 1;
            }
        }
        if next_addr == 0x00FF_FFFF {
            break;
        }
        let next_off = next_addr.wrapping_sub(pool_lo) as usize;
        if next_off >= pool.len() {
            break;
        }
        cursor = next_off;
    }
    out
}

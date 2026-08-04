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
//! For the Drake-Kingdom top-view save state (`mc1` per the user's
//! convention) the pool starts at `0x800AD400` and runs ~341 KB. Decoded
//! corpus: ~4400 `POLY_FT4`, ~250 `POLY_GT4`, ~150 `SPRT_16` ≈ 4800 prims
//! per frame. See `memory/project_world_map_top_view_findings.md` for the
//! per-region RAM layout summary.
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
            Prim::Sprt16 { clut, .. } | Prim::Sprt8 { clut, .. } => Some((*clut, None)),
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

/// One contiguous run of primitive packets found by [`find_pools`].
#[derive(Debug, Clone, Serialize)]
pub struct PoolRegion {
    /// KSEG0 address of the first accepted packet's tag word.
    pub start: u32,
    /// KSEG0 address one past the last accepted packet's payload.
    pub end: u32,
    /// How many packets were accepted inside the run.
    pub prims: usize,
}

impl PoolRegion {
    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// Locate the frame's primitive pool(s) inside a whole main-RAM image.
///
/// [`POOL_BASE_DEFAULT`] is a world-map constant, so anything that wants the
/// display list of an arbitrary scene has to *find* the pool rather than assume
/// it. libgpu builds each frame's packets contiguously in one work buffer, so
/// the pool shows up as a dense run of packets whose chain tags point at each
/// other. Two extra constraints keep the whole-RAM scan from drowning in false
/// positives (a scan bounded only by "somewhere in 2 MiB" accepts far too much):
///
/// - a tag's `next_addr` must be the terminator **or** land within `NEAR_WINDOW`
///   bytes of the tag itself - real OT links are short-range because the pool is
///   one buffer, whereas a random word that happens to look like a tag points
///   anywhere;
/// - a run must hold at least `min_prims` accepted packets to be reported.
///
/// Runs are separated when the gap between consecutive accepted packets exceeds
/// `MAX_GAP` bytes.
pub fn find_pools(ram: &[u8], ram_base: u32, min_prims: usize) -> Vec<PoolRegion> {
    /// How far a legitimate chain link may reach. The PSX work buffer for one
    /// frame is a few hundred KB at most.
    const NEAR_WINDOW: u32 = 512 * 1024;
    /// Bytes of non-primitive slack tolerated inside one run.
    const MAX_GAP: usize = 4096;

    let mut accepted: Vec<(usize, usize)> = Vec::new(); // (offset, total_bytes)
    let n_words = ram.len() / 4;
    let mut w = 0usize;
    while w < n_words {
        let i = w * 4;
        if i + 8 > ram.len() {
            break;
        }
        let tag = read_u32(ram, i);
        let length = ((tag >> 24) & 0xFF) as usize;
        let next_addr = tag & 0x00FF_FFFF;
        if !(1..=12).contains(&length) || i + 4 + length * 4 > ram.len() {
            w += 1;
            continue;
        }
        let here = (ram_base + i as u32) & 0x00FF_FFFF;
        let near = next_addr == 0x00FF_FFFF || here.abs_diff(next_addr) <= NEAR_WINDOW;
        if !near {
            w += 1;
            continue;
        }
        let cmd = ((read_u32(ram, i + 4) >> 24) & 0xFF) as u8;
        let (_, prim) = decode_packet(ram, i, cmd, length);
        if prim.is_some() {
            accepted.push((i, 4 + length * 4));
            w += length + 1;
        } else {
            w += 1;
        }
    }

    let mut out: Vec<PoolRegion> = Vec::new();
    let mut run_start: Option<usize> = None;
    let mut run_end = 0usize;
    let mut run_count = 0usize;
    for (off, size) in accepted {
        match run_start {
            Some(_) if off.saturating_sub(run_end) <= MAX_GAP => {
                run_end = off + size;
                run_count += 1;
            }
            Some(s) => {
                if run_count >= min_prims {
                    out.push(PoolRegion {
                        start: ram_base + s as u32,
                        end: ram_base + run_end as u32,
                        prims: run_count,
                    });
                }
                run_start = Some(off);
                run_end = off + size;
                run_count = 1;
            }
            None => {
                run_start = Some(off);
                run_end = off + size;
                run_count = 1;
            }
        }
    }
    if let Some(s) = run_start.filter(|_| run_count >= min_prims) {
        out.push(PoolRegion {
            start: ram_base + s as u32,
            end: ram_base + run_end as u32,
            prims: run_count,
        });
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.prims));
    out
}

/// A located libgpu ordering table: the bucket array itself, not the packets.
#[derive(Debug, Clone, Serialize)]
pub struct OtArray {
    /// KSEG0 address of the lowest bucket (`ot[0]`).
    pub start: u32,
    /// KSEG0 address one past the highest bucket.
    pub end: u32,
    /// Number of `u32` buckets.
    pub buckets: usize,
    /// KSEG0 address of the entry `DrawOTag` is handed - the chain head. With
    /// `ClearOTagR` this is the **highest** bucket, because the reverse-cleared
    /// table threads `ot[N-1] -> ot[N-2] -> ... -> ot[0] -> terminator`.
    pub head: u32,
}

/// Find the frame's ordering-table array(s) in a main-RAM image.
///
/// The packet pool and the ordering table are different objects, and the chain
/// head lives in the **table**, not the pool - which is why walking a pool in
/// isolation reports hundreds of spurious "heads" (every packet whose
/// predecessor link sits outside the window looks like a head). To get real
/// draw order the walk has to start at the table.
///
/// `ClearOTagR` leaves a recognisable signature: an empty bucket at address `A`
/// holds `0x00` in its length byte and `A - 4` in its 24-bit next field, so a
/// cleared table is a run of words each pointing at its own predecessor. Buckets
/// that received primitives break the pattern by pointing into the pool instead,
/// so the run is detected on the empty ones and then extended across the
/// occupied ones.
pub fn find_ot_arrays(ram: &[u8], ram_base: u32, min_buckets: usize) -> Vec<OtArray> {
    let n_words = ram.len() / 4;
    let is_empty_bucket = |w: usize| -> bool {
        let i = w * 4;
        if i + 4 > ram.len() {
            return false;
        }
        let word = read_u32(ram, i);
        if (word >> 24) != 0 {
            return false;
        }
        let here = (ram_base + i as u32) & 0x00FF_FFFF;
        (word & 0x00FF_FFFF) == here.wrapping_sub(4) & 0x00FF_FFFF
    };
    // A bucket that received prims points somewhere else entirely; accept it as
    // part of the table so an occupied bucket does not split one array in two.
    let is_occupied_bucket = |w: usize| -> bool {
        let i = w * 4;
        if i + 4 > ram.len() {
            return false;
        }
        let word = read_u32(ram, i);
        (word >> 24) == 0 && (word & 0x00FF_FFFF) != 0
    };

    let mut out = Vec::new();
    let mut w = 0usize;
    while w < n_words {
        if !is_empty_bucket(w) {
            w += 1;
            continue;
        }
        // Walk back over any occupied buckets that precede this empty one.
        let mut lo = w;
        while lo > 0 && (is_empty_bucket(lo - 1) || is_occupied_bucket(lo - 1)) {
            lo -= 1;
        }
        // Walk forward likewise.
        let mut hi = w;
        while hi + 1 < n_words && (is_empty_bucket(hi + 1) || is_occupied_bucket(hi + 1)) {
            hi += 1;
        }
        let buckets = hi - lo + 1;
        if buckets >= min_buckets {
            out.push(OtArray {
                start: ram_base + (lo * 4) as u32,
                end: ram_base + ((hi + 1) * 4) as u32,
                buckets,
                head: ram_base + (hi * 4) as u32,
            });
        }
        w = hi + 1;
    }
    out.sort_by_key(|o| std::cmp::Reverse(o.buckets));
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

fn decode_in(pool: &[u8], pool_base: u32, _scratch: &mut Vec<u8>) -> Vec<Prim> {
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
        // Payload starts at `i + 4`. Need at least `length` payload words.
        let payload_end = i + 4 + length * 4;
        if payload_end > pool.len() {
            continue;
        }
        // Read cmd byte (high byte of payload word 0).
        let cmd_word = read_u32(pool, i + 4);
        let cmd = ((cmd_word >> 24) & 0xFF) as u8;
        let (kind_ok, prim) = decode_packet(pool, i, cmd, length);
        if !kind_ok {
            continue;
        }
        if let Some(p) = prim {
            // Mark the tag + payload words as consumed so an inner false-positive
            // tag (e.g. a vertex word that looks like a chain link) can't emit
            // a phantom prim.
            for k in 0..=length {
                let cw = w + k;
                if cw < consumed.len() {
                    consumed[cw] = true;
                }
            }
            out.push(p);
        }
    }
    out
}

/// Try to decode the packet starting at offset `i` in `pool` as the given
/// `cmd` with payload `length` words. Returns `(kind_matches, prim)`:
/// `kind_matches=false` ⇒ the cmd byte didn't match any known opcode (skip
/// without consuming); `kind_matches=true` with `Some(prim)` ⇒ accepted;
/// `kind_matches=true` with `None` ⇒ matched cmd but length disagreed
/// (also skip).
fn decode_packet(pool: &[u8], i: usize, cmd: u8, length: usize) -> (bool, Option<Prim>) {
    match cmd {
        // POLY_FT4: 9 payload words.
        0x2C..=0x2F if length == 9 => {
            let (color, verts, uvs, clut, tpage) = decode_ft4(pool, i);
            (
                true,
                Some(Prim::PolyFt4 {
                    cmd,
                    color,
                    verts,
                    uvs,
                    clut,
                    tpage,
                }),
            )
        }
        // POLY_GT4: 12 payload words.
        0x3C..=0x3F if length == 12 => {
            let (colors, verts, uvs, clut, tpage) = decode_gt4(pool, i);
            (
                true,
                Some(Prim::PolyGt4 {
                    cmd,
                    colors,
                    verts,
                    uvs,
                    clut,
                    tpage,
                }),
            )
        }
        // POLY_FT3: 6 payload words.
        0x24..=0x27 if length == 6 => {
            let (color, verts, uvs, clut, tpage) = decode_ft3(pool, i);
            (
                true,
                Some(Prim::PolyFt3 {
                    cmd,
                    color,
                    verts,
                    uvs,
                    clut,
                    tpage,
                }),
            )
        }
        // POLY_GT3: 9 payload words.
        0x34..=0x37 if length == 9 => {
            let (colors, verts, uvs, clut, tpage) = decode_gt3(pool, i);
            (
                true,
                Some(Prim::PolyGt3 {
                    cmd,
                    colors,
                    verts,
                    uvs,
                    clut,
                    tpage,
                }),
            )
        }
        // GP0 rectangle commands encode size in bits 4-3 of the opcode:
        // `00`=variable, `01`=1x1, `10`=8x8, `11`=16x16, with bit 2 = textured.
        // So 0x74..0x77 is the **8x8** textured sprite and 0x7C..0x7F the
        // **16x16** one - the reverse of an earlier reading here, which made
        // every 8x8 sprite decode as `Sprt16` and render at double size in the
        // web-viewer's `sprite_to_quad` call.
        //
        // SPRT_8 (fixed 8x8): 3 payload words.
        0x74..=0x77 if length == 3 => {
            let (color, pos, uv, clut) = decode_sprt(pool, i);
            (
                true,
                Some(Prim::Sprt8 {
                    cmd,
                    color,
                    pos,
                    uv,
                    clut,
                }),
            )
        }
        // SPRT_16 (fixed 16x16): 3 payload words.
        0x7C..=0x7F if length == 3 => {
            let (color, pos, uv, clut) = decode_sprt(pool, i);
            (
                true,
                Some(Prim::Sprt16 {
                    cmd,
                    color,
                    pos,
                    uv,
                    clut,
                }),
            )
        }
        // POLY_F3 (flat untextured tri): 4 payload words.
        0x20..=0x23 if length == 4 => {
            let p = i + 4;
            let color = rgb(read_u32(pool, p));
            let verts = [
                vert(read_u32(pool, p + 4)),
                vert(read_u32(pool, p + 8)),
                vert(read_u32(pool, p + 12)),
            ];
            (true, Some(Prim::PolyF3 { cmd, color, verts }))
        }
        // POLY_F4 (flat untextured quad): 5 payload words.
        0x28..=0x2B if length == 5 => {
            let p = i + 4;
            let color = rgb(read_u32(pool, p));
            let verts = [
                vert(read_u32(pool, p + 4)),
                vert(read_u32(pool, p + 8)),
                vert(read_u32(pool, p + 12)),
                vert(read_u32(pool, p + 16)),
            ];
            (true, Some(Prim::PolyF4 { cmd, color, verts }))
        }
        // POLY_G3 (Gouraud untextured tri): 6 payload words.
        0x30..=0x33 if length == 6 => {
            let p = i + 4;
            let colors = [
                rgb(read_u32(pool, p)),
                rgb(read_u32(pool, p + 8)),
                rgb(read_u32(pool, p + 16)),
            ];
            let verts = [
                vert(read_u32(pool, p + 4)),
                vert(read_u32(pool, p + 12)),
                vert(read_u32(pool, p + 20)),
            ];
            (true, Some(Prim::PolyG3 { cmd, colors, verts }))
        }
        // POLY_G4 (Gouraud untextured quad): 8 payload words.
        0x38..=0x3B if length == 8 => {
            let p = i + 4;
            let colors = [
                rgb(read_u32(pool, p)),
                rgb(read_u32(pool, p + 8)),
                rgb(read_u32(pool, p + 16)),
                rgb(read_u32(pool, p + 24)),
            ];
            let verts = [
                vert(read_u32(pool, p + 4)),
                vert(read_u32(pool, p + 12)),
                vert(read_u32(pool, p + 20)),
                vert(read_u32(pool, p + 28)),
            ];
            (true, Some(Prim::PolyG4 { cmd, colors, verts }))
        }
        // Other known cmds we don't handle yet (POLY_F4, POLY_G4, tiles,
        // lines, GP0 control words). Treat as "known kind, wrong length"
        // so we skip without consuming.
        0x20..=0x3F | 0x40..=0x5F | 0x60..=0x7F | 0x80..=0x9F | 0xA0..=0xCF | 0xE0..=0xE6 => {
            (true, None)
        }
        _ => (false, None),
    }
}

fn read_u32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

fn vert(w: u32) -> (i16, i16) {
    let x = (w & 0xFFFF) as i16; // sign-extended via i16 cast
    let y = ((w >> 16) & 0xFFFF) as i16;
    (x, y)
}

fn rgb(w: u32) -> [u8; 3] {
    [
        (w & 0xFF) as u8,
        ((w >> 8) & 0xFF) as u8,
        ((w >> 16) & 0xFF) as u8,
    ]
}

/// Decoder return shapes for polygon variants. Aliased here to satisfy
/// the `type_complexity` lint (each decoder returns the per-prim fields
/// minus the shared `cmd` byte). `(color(s), verts, uvs, clut, tpage)`.
type Ft4Fields = ([u8; 3], [(i16, i16); 4], [(u8, u8); 4], u16, u16);
type Gt4Fields = ([[u8; 3]; 4], [(i16, i16); 4], [(u8, u8); 4], u16, u16);
type Ft3Fields = ([u8; 3], [(i16, i16); 3], [(u8, u8); 3], u16, u16);
type Gt3Fields = ([[u8; 3]; 3], [(i16, i16); 3], [(u8, u8); 3], u16, u16);

/// POLY_FT4 layout (payload, 9 words starting at offset 4 past the tag):
///
/// ```text
/// +0  [cmd  | R0 | G0 | B0]
/// +1  [x0   | y0]
/// +2  [u0   | v0 | clut15]
/// +3  [x1   | y1]
/// +4  [u1   | v1 | tpage15]
/// +5  [x2   | y2]
/// +6  [u2   | v2 | pad]
/// +7  [x3   | y3]
/// +8  [u3   | v3 | pad]
/// ```
fn decode_ft4(pool: &[u8], i: usize) -> Ft4Fields {
    let p = i + 4;
    let cmd_word = read_u32(pool, p);
    let color = rgb(cmd_word);
    let v0 = vert(read_u32(pool, p + 4));
    let uv_clut = read_u32(pool, p + 8);
    let v1 = vert(read_u32(pool, p + 12));
    let uv_tpage = read_u32(pool, p + 16);
    let v2 = vert(read_u32(pool, p + 20));
    let uv2_word = read_u32(pool, p + 24);
    let v3 = vert(read_u32(pool, p + 28));
    let uv3_word = read_u32(pool, p + 32);
    let uvs = [
        ((uv_clut & 0xFF) as u8, ((uv_clut >> 8) & 0xFF) as u8),
        ((uv_tpage & 0xFF) as u8, ((uv_tpage >> 8) & 0xFF) as u8),
        ((uv2_word & 0xFF) as u8, ((uv2_word >> 8) & 0xFF) as u8),
        ((uv3_word & 0xFF) as u8, ((uv3_word >> 8) & 0xFF) as u8),
    ];
    let clut = ((uv_clut >> 16) & 0xFFFF) as u16;
    let tpage = ((uv_tpage >> 16) & 0xFFFF) as u16;
    (color, [v0, v1, v2, v3], uvs, clut, tpage)
}

/// POLY_GT4 layout (12 payload words). Per-vertex (color | xy | uv*) repeats:
///
/// ```text
/// +0  [cmd  | R0 | G0 | B0]
/// +1  [x0   | y0]
/// +2  [u0   | v0 | clut15]
/// +3  [R1   | G1 | B1 | pad]
/// +4  [x1   | y1]
/// +5  [u1   | v1 | tpage15]
/// +6  [R2   | G2 | B2 | pad]
/// +7  [x2   | y2]
/// +8  [u2   | v2 | pad]
/// +9  [R3   | G3 | B3 | pad]
/// +10 [x3   | y3]
/// +11 [u3   | v3 | pad]
/// ```
fn decode_gt4(pool: &[u8], i: usize) -> Gt4Fields {
    let p = i + 4;
    let c0 = rgb(read_u32(pool, p));
    let v0 = vert(read_u32(pool, p + 4));
    let uv0_clut = read_u32(pool, p + 8);
    let c1 = rgb(read_u32(pool, p + 12));
    let v1 = vert(read_u32(pool, p + 16));
    let uv1_tpage = read_u32(pool, p + 20);
    let c2 = rgb(read_u32(pool, p + 24));
    let v2 = vert(read_u32(pool, p + 28));
    let uv2 = read_u32(pool, p + 32);
    let c3 = rgb(read_u32(pool, p + 36));
    let v3 = vert(read_u32(pool, p + 40));
    let uv3 = read_u32(pool, p + 44);
    let uvs = [
        ((uv0_clut & 0xFF) as u8, ((uv0_clut >> 8) & 0xFF) as u8),
        ((uv1_tpage & 0xFF) as u8, ((uv1_tpage >> 8) & 0xFF) as u8),
        ((uv2 & 0xFF) as u8, ((uv2 >> 8) & 0xFF) as u8),
        ((uv3 & 0xFF) as u8, ((uv3 >> 8) & 0xFF) as u8),
    ];
    let clut = ((uv0_clut >> 16) & 0xFFFF) as u16;
    let tpage = ((uv1_tpage >> 16) & 0xFFFF) as u16;
    ([c0, c1, c2, c3], [v0, v1, v2, v3], uvs, clut, tpage)
}

fn decode_ft3(pool: &[u8], i: usize) -> Ft3Fields {
    let p = i + 4;
    let color = rgb(read_u32(pool, p));
    let v0 = vert(read_u32(pool, p + 4));
    let uv0_clut = read_u32(pool, p + 8);
    let v1 = vert(read_u32(pool, p + 12));
    let uv1_tpage = read_u32(pool, p + 16);
    let v2 = vert(read_u32(pool, p + 20));
    let uv2 = read_u32(pool, p + 24);
    let uvs = [
        ((uv0_clut & 0xFF) as u8, ((uv0_clut >> 8) & 0xFF) as u8),
        ((uv1_tpage & 0xFF) as u8, ((uv1_tpage >> 8) & 0xFF) as u8),
        ((uv2 & 0xFF) as u8, ((uv2 >> 8) & 0xFF) as u8),
    ];
    let clut = ((uv0_clut >> 16) & 0xFFFF) as u16;
    let tpage = ((uv1_tpage >> 16) & 0xFFFF) as u16;
    (color, [v0, v1, v2], uvs, clut, tpage)
}

fn decode_gt3(pool: &[u8], i: usize) -> Gt3Fields {
    let p = i + 4;
    let c0 = rgb(read_u32(pool, p));
    let v0 = vert(read_u32(pool, p + 4));
    let uv0_clut = read_u32(pool, p + 8);
    let c1 = rgb(read_u32(pool, p + 12));
    let v1 = vert(read_u32(pool, p + 16));
    let uv1_tpage = read_u32(pool, p + 20);
    let c2 = rgb(read_u32(pool, p + 24));
    let v2 = vert(read_u32(pool, p + 28));
    let uv2 = read_u32(pool, p + 32);
    let uvs = [
        ((uv0_clut & 0xFF) as u8, ((uv0_clut >> 8) & 0xFF) as u8),
        ((uv1_tpage & 0xFF) as u8, ((uv1_tpage >> 8) & 0xFF) as u8),
        ((uv2 & 0xFF) as u8, ((uv2 >> 8) & 0xFF) as u8),
    ];
    let clut = ((uv0_clut >> 16) & 0xFFFF) as u16;
    let tpage = ((uv1_tpage >> 16) & 0xFFFF) as u16;
    ([c0, c1, c2], [v0, v1, v2], uvs, clut, tpage)
}

/// Sprite (SPRT_8 / SPRT_16) layout (3 payload words):
///
/// ```text
/// +0 [cmd | R | G | B]
/// +1 [x   | y]
/// +2 [u   | v | clut15]
/// ```
fn decode_sprt(pool: &[u8], i: usize) -> ([u8; 3], (i16, i16), (u8, u8), u16) {
    let p = i + 4;
    let color = rgb(read_u32(pool, p));
    let pos = vert(read_u32(pool, p + 4));
    let uv_clut = read_u32(pool, p + 8);
    let uv = ((uv_clut & 0xFF) as u8, ((uv_clut >> 8) & 0xFF) as u8);
    let clut = ((uv_clut >> 16) & 0xFFFF) as u16;
    (color, pos, uv, clut)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build one packet: `[tag][payload...]`, returning the bytes.
    fn packet(len: u8, next: u32, words: &[u32]) -> Vec<u8> {
        assert_eq!(len as usize, words.len());
        let mut v = Vec::new();
        v.extend_from_slice(&(((len as u32) << 24) | (next & 0x00FF_FFFF)).to_le_bytes());
        for w in words {
            v.extend_from_slice(&w.to_le_bytes());
        }
        v
    }

    fn xy(x: i16, y: i16) -> u32 {
        ((y as u16 as u32) << 16) | (x as u16 as u32)
    }

    /// The GP0 rectangle opcode encodes size in bits 4-3: `0x74..0x77` is 8x8
    /// and `0x7C..0x7F` is 16x16. An earlier reading had these swapped, which
    /// made every 8x8 sprite decode as `Sprt16` and draw at double size.
    #[test]
    fn sprite_opcode_sizes_follow_the_gp0_encoding() {
        let mut buf = packet(3, 0xFFFFFF, &[0x7400_0000, xy(10, 20), 0x0000_1234]);
        assert!(matches!(
            decode(&buf, 0x8000_0000).as_slice(),
            [Prim::Sprt8 { .. }]
        ));
        buf = packet(3, 0xFFFFFF, &[0x7C00_0000, xy(10, 20), 0x0000_1234]);
        assert!(matches!(
            decode(&buf, 0x8000_0000).as_slice(),
            [Prim::Sprt16 { .. }]
        ));
    }

    #[test]
    fn sprite_bounds_use_the_real_extent() {
        let s8 = &decode(
            &packet(3, 0xFFFFFF, &[0x7400_0000, xy(10, 20), 0]),
            0x8000_0000,
        )[0]
        .bounds();
        assert_eq!(*s8, (10, 20, 18, 28));
        let s16 = &decode(
            &packet(3, 0xFFFFFF, &[0x7C00_0000, xy(10, 20), 0]),
            0x8000_0000,
        )[0]
        .bounds();
        assert_eq!(*s16, (10, 20, 26, 36));
    }

    /// Untextured polys carry no texture page at all, so a reader that only
    /// knows the textured opcodes silently drops them - and flat/Gouraud
    /// geometry is a large share of a real field frame.
    #[test]
    fn untextured_polygon_families_decode() {
        let f3 = packet(4, 0xFFFFFF, &[0x2000_0000, xy(0, 0), xy(10, 0), xy(0, 10)]);
        let f4 = packet(
            5,
            0xFFFFFF,
            &[0x2800_0000, xy(0, 0), xy(10, 0), xy(0, 10), xy(10, 10)],
        );
        let g3 = packet(
            6,
            0xFFFFFF,
            &[0x3000_0000, xy(0, 0), 0, xy(10, 0), 0, xy(0, 10)],
        );
        let g4 = packet(
            8,
            0xFFFFFF,
            &[
                0x3800_0000,
                xy(0, 0),
                0,
                xy(10, 0),
                0,
                xy(0, 10),
                0,
                xy(10, 10),
            ],
        );
        for (bytes, want) in [
            (f3, "POLY_F3"),
            (f4, "POLY_F4"),
            (g3, "POLY_G3"),
            (g4, "POLY_G4"),
        ] {
            let got = decode(&bytes, 0x8000_0000);
            assert_eq!(got.len(), 1, "{want} did not decode");
            assert_eq!(got[0].kind(), want);
            assert!(!got[0].is_textured(), "{want} must not report a texture");
            assert!(got[0].clut_tpage().is_none());
        }
    }

    /// The chain is the draw order, and it is not address order: a packet
    /// linked later wins on a machine with no depth buffer. Build a pool whose
    /// link order is the reverse of its address order and check the walk
    /// follows the links.
    #[test]
    fn chain_walk_follows_links_not_addresses() {
        let base = 0x8000_0000u32;
        // Three F3 packets at offsets 0, 20, 40; link 40 -> 0 -> 20 -> end.
        let mut pool = Vec::new();
        pool.extend_from_slice(&packet(
            4,
            base + 20,
            &[0x2000_0000, xy(0, 0), xy(9, 0), xy(0, 9)],
        ));
        pool.extend_from_slice(&packet(
            4,
            0xFFFFFF,
            &[0x2000_0000, xy(1, 1), xy(9, 1), xy(1, 9)],
        ));
        pool.extend_from_slice(&packet(
            4,
            base,
            &[0x2000_0000, xy(2, 2), xy(9, 2), xy(2, 9)],
        ));
        let walked = chain_walk(&pool, base, 40);
        assert_eq!(walked.len(), 3);
        assert_eq!(
            walked.iter().map(|c| c.offset).collect::<Vec<_>>(),
            vec![40, 0, 20],
            "walk must follow next_addr, not ascending address"
        );
        assert_eq!(walked[2].order, 2, "last walked packet wins");
    }

    /// A malformed pool can link a packet back into the chain; the walk must
    /// terminate rather than spin.
    #[test]
    fn chain_walk_terminates_on_a_link_cycle() {
        let base = 0x8000_0000u32;
        let mut pool = Vec::new();
        pool.extend_from_slice(&packet(
            4,
            base + 20,
            &[0x2000_0000, xy(0, 0), xy(9, 0), xy(0, 9)],
        ));
        pool.extend_from_slice(&packet(
            4,
            base,
            &[0x2000_0000, xy(1, 1), xy(9, 1), xy(1, 9)],
        ));
        let walked = chain_walk(&pool, base, 0);
        assert_eq!(walked.len(), 2, "cycle must stop after revisiting a packet");
    }

    /// `ClearOTagR` leaves every empty bucket pointing at its own predecessor;
    /// that signature is how the ordering table is told apart from the packet
    /// pool, and the head is the highest bucket.
    #[test]
    fn ot_array_is_found_by_its_cleared_signature() {
        let base = 0x8000_0000u32;
        let buckets = 128usize;
        let mut ram = vec![0u8; buckets * 4 + 64];
        for i in 0..buckets {
            let addr = base + (i * 4) as u32;
            let prev = (addr.wrapping_sub(4)) & 0x00FF_FFFF;
            ram[i * 4..i * 4 + 4].copy_from_slice(&prev.to_le_bytes());
        }
        let found = find_ot_arrays(&ram, base, 64);
        assert_eq!(found.len(), 1, "expected exactly one table");
        assert_eq!(found[0].buckets, buckets);
        assert_eq!(
            found[0].head,
            base + ((buckets - 1) * 4) as u32,
            "head is the highest bucket (reverse-cleared table)"
        );
    }

    #[test]
    fn ft4_packet_round_trip() {
        // Synthesise one POLY_FT4: tag = (len=9, next=0xFFFFFF), then 9 words.
        let mut buf = vec![0u8; 64];
        let tag = 0x09FFFFFF_u32;
        buf[0..4].copy_from_slice(&tag.to_le_bytes());
        let cmd = 0x2C202020_u32; // cmd=0x2C, color = (0x20, 0x20, 0x20)
        buf[4..8].copy_from_slice(&cmd.to_le_bytes());
        // 4 verts at (10,10), (20,10), (10,20), (20,20)
        buf[8..12].copy_from_slice(&((10u32) | ((10u32) << 16)).to_le_bytes());
        buf[12..16].copy_from_slice(&((0xABCDu32 << 16) | 0x1020).to_le_bytes()); // uv0 + clut
        buf[16..20].copy_from_slice(&((20u32) | ((10u32) << 16)).to_le_bytes());
        buf[20..24].copy_from_slice(&((0x0019u32 << 16) | 0x3040).to_le_bytes()); // uv1 + tpage
        buf[24..28].copy_from_slice(&((10u32) | ((20u32) << 16)).to_le_bytes());
        buf[28..32].copy_from_slice(&(0x5060u32).to_le_bytes());
        buf[32..36].copy_from_slice(&((20u32) | ((20u32) << 16)).to_le_bytes());
        buf[36..40].copy_from_slice(&(0x7080u32).to_le_bytes());
        let prims = decode(&buf, 0x800AD400);
        assert_eq!(prims.len(), 1, "expected exactly one prim");
        match &prims[0] {
            Prim::PolyFt4 {
                cmd,
                color,
                verts,
                clut,
                tpage,
                ..
            } => {
                assert_eq!(*cmd, 0x2C);
                assert_eq!(*color, [0x20, 0x20, 0x20]);
                assert_eq!(verts[0], (10, 10));
                assert_eq!(verts[3], (20, 20));
                assert_eq!(*clut, 0xABCD);
                assert_eq!(*tpage, 0x0019);
            }
            _ => panic!("wrong variant"),
        }
    }
}

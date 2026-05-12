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
    /// Fixed 16x16 sprite. Cmd 0x74..0x77. 4 u32 words (1 tag + 3 payload).
    Sprt16 {
        cmd: u8,
        color: [u8; 3],
        pos: (i16, i16),
        uv: (u8, u8),
        clut: u16,
    },
    /// Fixed 8x8 sprite. Cmd 0x7C..0x7F. 4 u32 words.
    Sprt8 {
        cmd: u8,
        color: [u8; 3],
        pos: (i16, i16),
        uv: (u8, u8),
        clut: u16,
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
            | Prim::Sprt8 { cmd, .. } => *cmd,
        }
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
        // SPRT_16 (fixed 16x16): 3 payload words.
        0x74..=0x77 if length == 3 => {
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
        // SPRT_8 (fixed 8x8): 3 payload words.
        0x7C..=0x7F if length == 3 => {
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

//! Legaia-specific primitive iteration.
//!
//! Reverse-engineered from `FUN_8002735c` (the renderer in `SCUS_942.54`)
//! and the per-mode descriptor table at `DAT_8007326c`.
//!
//! Each object's primitive section is a sequence of **groups**, each with an
//! 8-byte header followed by `count` fixed-stride primitives:
//!
//! ```text
//! group header (8 bytes):
//!   +0  u16 count           // primitives in this group
//!   +2  u16 flags           // selects per-mode descriptor (low bit of (flags>>1) = 1 => quad, else triangle)
//!   +4  u8  olen            // PSX SDK output length
//!   +5  u8  ilen            // per-prim WORD stride (per-prim bytes = ilen * 4)
//!   +6  u8  flag            // PSX SDK flag byte
//!   +7  u8  mode            // PSX SDK mode byte
//! prim data:
//!   count * (ilen * 4) bytes
//! group footer:
//!   ilen * 4 bytes (the renderer always advances one extra prim slot before
//!   reading the next group's header; treat as padding)
//! ```
//!
//! End-of-section is signaled by a u32 of zero where the next group header
//! would start (i.e. count == 0 && flags == 0).
//!
//! The per-prim layout is keyed on the group's `flags` field. For each
//! flags value, the renderer's table at `DAT_8007326c` gives the byte
//! offset (in u16 units) within the prim where the vertex indices begin.
//! See [`vertex_offset_bytes`] for the lookup.
//!
//! Vertex indices are u16 byte-offsets into the object's vertex array
//! (each vertex is 8 bytes - `SVECTOR { i16 x, y, z, pad }`), so the
//! array index is `raw_index / 8`.

use anyhow::{Result, bail};
use serde::Serialize;

use crate::descriptor::Descriptor;

/// Group header size in bytes.
pub const GROUP_HEADER_SIZE: usize = 8;

/// Decoded group header.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct GroupHeader {
    pub count: u16,
    pub flags: u16,
    pub olen: u8,
    pub ilen: u8,
    pub flag: u8,
    pub mode: u8,
}

impl GroupHeader {
    /// Parse 8 bytes at `pos` as a group header.
    fn parse(buf: &[u8], pos: usize) -> Result<Self> {
        if pos + GROUP_HEADER_SIZE > buf.len() {
            bail!("group header at {} past buffer end ({})", pos, buf.len());
        }
        Ok(Self {
            count: u16::from_le_bytes(buf[pos..pos + 2].try_into().unwrap()),
            flags: u16::from_le_bytes(buf[pos + 2..pos + 4].try_into().unwrap()),
            olen: buf[pos + 4],
            ilen: buf[pos + 5],
            flag: buf[pos + 6],
            mode: buf[pos + 7],
        })
    }

    /// Per-prim stride in bytes.
    pub fn prim_stride(&self) -> usize {
        self.ilen as usize * 4
    }

    /// Total bytes this group occupies in the section, including the
    /// trailing footer slot.
    pub fn total_bytes(&self) -> usize {
        GROUP_HEADER_SIZE + (self.count as usize + 1) * self.prim_stride()
    }

    /// True if a u32 of zero at the group-header position would be a
    /// valid section terminator (no real groups have count==0 && flags==0).
    pub fn is_terminator(&self) -> bool {
        self.count == 0 && self.flags == 0
    }

    /// Number of vertices per primitive in this group: 4 if quad, else 3.
    /// From the renderer: `if ((flags >> 1) & 1 == 0) -> 3 else 4`.
    pub fn n_vertices(&self) -> usize {
        if (self.flags >> 1) & 1 == 0 { 3 } else { 4 }
    }

    /// Semi-transparency enable (the PSX SDK mode byte's ABE bit, bit 1).
    /// The mode byte is the GP0 polygon command byte the renderer emits for
    /// the group's prims, so ABE here means "the GPU blends texels whose
    /// BGR555 STP bit (bit 15) is set". Legaia character meshes commonly
    /// carry ABE (mode `0x26`/`0x27`); the per-texel STP bit decides which
    /// pixels actually blend.
    pub fn abe(&self) -> bool {
        self.mode & 0x02 != 0
    }
}

/// Look up the byte offset within a primitive where vertex indices begin,
/// for a given group `flags` value. Thin wrapper over
/// [`Descriptor::vertex_offset`](crate::descriptor::Descriptor::vertex_offset) -
/// see that module for the table and the per-row derivation.
///
/// Returns `None` for flags outside the known range.
pub fn vertex_offset_bytes(flags: u16) -> Option<usize> {
    Descriptor::for_flags(flags).map(|d| d.vertex_offset)
}

/// One decoded primitive within a group.
#[derive(Debug, Clone, Serialize)]
pub struct Prim {
    /// Byte offset within the TMD buffer where this prim's data starts.
    pub bytes_offset: usize,
    /// Per-prim stride in bytes (= group.prim_stride()).
    pub bytes_size: usize,
    /// Vertex indices as raw u16 byte-offsets into the object's vertex array.
    /// Array index = raw / 8 (since SVECTOR = 8 bytes).
    pub vertex_indices_raw: Vec<u16>,
    /// Per-vertex `(u, v)` texture coordinates if the prim has a texture
    /// block (which Legaia's character TMDs always do - every observed mode
    /// in the 599-mesh battle_data corpus carries UVs). One entry per vertex.
    pub uvs: Vec<(u8, u8)>,
    /// CLUT base address (raw 16-bit value from the TMD). Decode with
    /// `cba_xy()` to get VRAM coordinates.
    pub cba: u16,
    /// Texture sub-base / "tpage" (raw 16-bit value). Decode with `tpage_xy()`
    /// for the VRAM page.
    pub tsb: u16,
    /// Per-vertex `[R, G, B]` colours, in the same stored order as
    /// [`vertex_indices_raw`](Self::vertex_indices_raw) - `colors[i]` pairs with
    /// `vertex_indices_raw[i]`. A **flat** prim stores one colour word at the
    /// prim start, replicated to every vertex here; a **gouraud** prim stores one
    /// colour word per vertex at a 4-byte stride. The 4th byte of each colour
    /// word is the SDK GP0 code byte and is dropped here.
    ///
    /// This is the **field's entire lighting signal**: the PSX GPU modulates a
    /// textured prim's texel by it (`texel * colour / 128`, so `0x80` is neutral,
    /// below darkens and above brightens up to ~2x), and draws an untextured
    /// prim in it directly. Populated for every prim, textured or not; the
    /// light-source-lit rows (0/1), which carry no colour block of their own, get
    /// [`MODULATION_NEUTRAL`] so they draw at the raw texel.
    pub colors: Vec<[u8; 3]>,
}

impl Prim {
    /// Vertex array indices (raw / 8).
    pub fn vertex_indices(&self) -> Vec<u16> {
        self.vertex_indices_raw.iter().map(|r| r / 8).collect()
    }

    /// Decode CBA → `(vram_x_pixels, vram_y_pixels)` for the CLUT location.
    /// PSX encoding: x = (cba & 0x3F) * 16, y = (cba >> 6) & 0x1FF.
    pub fn cba_xy(&self) -> (u16, u16) {
        ((self.cba & 0x3F) * 16, (self.cba >> 6) & 0x1FF)
    }

    /// Decode TSB → `(tpage_x_pixels, tpage_y_pixels, depth_bits, abr)`.
    /// PSX encoding: tpage_x = (tsb & 0xF) * 64, tpage_y = ((tsb >> 4) & 1) * 256,
    /// abr = (tsb >> 5) & 0x3, depth_bits = `[4, 8, 16, 4][((tsb >> 7) & 0x3)]`.
    pub fn tpage_xy(&self) -> (u16, u16, u8, u8) {
        let x = (self.tsb & 0xF) * 64;
        let y = ((self.tsb >> 4) & 1) * 256;
        let abr = ((self.tsb >> 5) & 0x3) as u8;
        let depth = match (self.tsb >> 7) & 0x3 {
            0 => 4,
            1 => 8,
            _ => 16,
        };
        (x, y, depth, abr)
    }
}

/// Layout of texture data within a Legaia primitive.
///
/// Reverse-engineered from `dump_prim_bytes` on `0866_battle_data` corpus -
/// matches the standard PSX SDK textured-primitive layout. The texture block
/// (UVs + CBA + TSB) sits between the color block(s) and the vertex indices.
///
/// For triangles the block is 10 bytes:
///   `[u0, v0, cba_lo, cba_hi, u1, v1, tsb_lo, tsb_hi, u2, v2]`
///
/// For quads the block is 12 bytes (same layout, plus `[u3, v3]`):
///   `[u0, v0, cba_lo, cba_hi, u1, v1, tsb_lo, tsb_hi, u2, v2, u3, v3]`
///
/// The block ends exactly at the vertex-index offset reported by
/// [`vertex_offset_bytes`]; the start is `vert_off - block_len`.
///
/// **Untextured prims (POLY_F3/F4/G3/G4)** carry no texture block - the
/// bytes immediately before the vertex indices are per-vertex *colors* or
/// per-vertex *normal indices*, not UV/CBA/TSB. The caller must consult
/// [`crate::descriptor::Descriptor::for_flags`] for the group's flags and
/// only invoke this function when `packet_shape.is_textured()` is `true`;
/// otherwise garbage bytes get interpreted as a texture descriptor, the
/// renderer's VRAM lookup samples some random page+CLUT slot, and the
/// resulting prim either renders as a single flat colour (the famous green
/// tint when `CLUT[0]` is non-zero) or as a transparent hole. Returns
/// `(Vec::new(), 0, 0)` when the block would run past the buffer.
///
/// `block_off` is the byte offset of the texture block within the prim
/// payload, resolved from the per-mode descriptor
/// ([`crate::descriptor::Descriptor::texture_block_offset`]) - the block
/// starts after any leading per-vertex colour words. The earlier caller
/// derived this as `vertex_offset - block_len`, which only held for the
/// baked-colour rows (byte1 = 3); the lit rows (byte1 = 0) put the block at
/// offset 0, ahead of the vertices, and the subtraction pointed into
/// geometry bytes.
fn extract_textures(
    buf: &[u8],
    prim_off: usize,
    n_verts: usize,
    block_off: usize,
) -> (Vec<(u8, u8)>, u16, u16) {
    let block_len = 4 + n_verts * 2; // CBA(2) + TSB(2) + UVs(n*2)
    let block_start = prim_off + block_off;
    if block_start + block_len > buf.len() {
        return (Vec::new(), 0, 0);
    }
    let mut uvs = Vec::with_capacity(n_verts);
    // Bytes 0-1: u0,v0; 2-3: cba; 4-5: u1,v1; 6-7: tsb; 8-9: u2,v2 [10-11: u3,v3]
    uvs.push((buf[block_start], buf[block_start + 1]));
    let cba = u16::from_le_bytes([buf[block_start + 2], buf[block_start + 3]]);
    uvs.push((buf[block_start + 4], buf[block_start + 5]));
    let tsb = u16::from_le_bytes([buf[block_start + 6], buf[block_start + 7]]);
    uvs.push((buf[block_start + 8], buf[block_start + 9]));
    if n_verts >= 4 {
        uvs.push((buf[block_start + 10], buf[block_start + 11]));
    }
    (uvs, cba, tsb)
}

/// The PSX GPU's neutral texture-modulation colour.
///
/// A textured primitive's texel is modulated by the packet colour as
/// `texel * colour / 128`, so `0x80` leaves the texel untouched, `0x00`
/// blacks it out and `0xFF` very nearly doubles it. Prims with no colour
/// block of their own (the light-source-lit rows 0/1) get this, which is
/// the same thing as drawing the raw texel.
pub const MODULATION_NEUTRAL: u8 = 0x80;

/// Extract the per-vertex `[R, G, B]` colour block of a primitive.
///
/// The colour block sits at the prim's start (before the texture block, if any,
/// and before the vertex indices). A **flat** prim (`F3`/`F4`/`FT3`/`FT4`) stores
/// one colour word at offset 0 shared by every vertex; a **gouraud** prim
/// (`G3`/`G4`/`GT3`/`GT4`) stores one colour word per vertex at a 4-byte stride.
/// The returned colours are in stored order - `colors[i]` pairs with the prim's
/// `vertex_indices_raw[i]`. The colour word's 4th byte (the SDK GP0 code -
/// `0x20`/`0x24`/`0x28`/`0x2C`/`0x30`/`0x34`/`0x38`/`0x3C`, plus `|2` for the
/// semi-transparent variants) is dropped. Returns `n_verts` entries; a colour
/// word that runs past the buffer falls back to mid-grey so a malformed tail
/// can't panic.
fn extract_colors(buf: &[u8], prim_off: usize, n_verts: usize, is_gouraud: bool) -> Vec<[u8; 3]> {
    (0..n_verts)
        .map(|corner| {
            let off = prim_off + if is_gouraud { corner * 4 } else { 0 };
            if off + 3 <= buf.len() {
                [buf[off], buf[off + 1], buf[off + 2]]
            } else {
                [128, 128, 128]
            }
        })
        .collect()
}

/// One group: header + decoded prims.
#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub header_offset: usize,
    pub header: GroupHeader,
    pub prims: Vec<Prim>,
}

/// Iterate the primitive section of an object. Walks groups until a
/// terminator (zero count+flags u32) is found OR the section is fully
/// consumed.
///
/// `section_start` and `section_size` are byte ranges within `buf`; they
/// come from `Object::primitives_byte_offset` / `primitives_byte_size`.
///
/// Returns `Err` on the first malformed group; if you'd rather collect
/// every well-formed group up to the failure point, use
/// [`iter_groups_lenient`].
pub fn iter_groups(buf: &[u8], section_start: usize, section_size: usize) -> Result<Vec<Group>> {
    let section_end = section_start
        .checked_add(section_size)
        .ok_or_else(|| anyhow::anyhow!("section size overflow"))?;
    if section_end > buf.len() {
        bail!(
            "section [{}..{}) past buffer end ({})",
            section_start,
            section_end,
            buf.len()
        );
    }
    let mut out = Vec::new();
    let mut pos = section_start;
    while pos + GROUP_HEADER_SIZE <= section_end {
        let header = GroupHeader::parse(buf, pos)?;
        if header.is_terminator() {
            break;
        }
        if header.ilen == 0 {
            bail!(
                "group at offset {} has ilen=0; cannot determine prim stride",
                pos - section_start
            );
        }
        let header_offset = pos;
        let prim_base = pos + GROUP_HEADER_SIZE;
        let stride = header.prim_stride();
        let group_total = header.total_bytes();
        if pos + group_total > section_end {
            bail!(
                "group at offset {} ({} prims of {} bytes + 1 footer slot = {} total) overruns section ({})",
                pos - section_start,
                header.count,
                stride,
                group_total,
                section_size
            );
        }
        let n_verts = header.n_vertices();
        let vert_off = vertex_offset_bytes(header.flags);
        // Per-group descriptor: the packet shape decides whether the
        // bytes immediately before the vertex indices are a texture block
        // (FT*/GT*) or per-vertex colours / normal indices (F*/G*). Reading
        // those colour bytes as a texture descriptor is what produced
        // bogus CBA/TSB values in earlier renderer output.
        let descriptor = Descriptor::for_flags(header.flags);
        let is_textured = descriptor.is_some_and(|d| d.packet_shape.is_textured());
        let is_gouraud = descriptor.is_some_and(|d| d.packet_shape.is_gouraud());
        // Texture block starts after the prim's leading colour words (if any);
        // the descriptor resolves the exact offset per row.
        let texblock_off = descriptor.and_then(|d| d.texture_block_offset);
        let mut prims = Vec::with_capacity(header.count as usize);
        for i in 0..header.count as usize {
            let prim_off = prim_base + i * stride;
            prims.push(decode_prim(
                buf,
                prim_off,
                stride,
                n_verts,
                vert_off,
                texblock_off,
                is_textured,
                is_gouraud,
            ));
        }
        out.push(Group {
            header_offset,
            header,
            prims,
        });
        pos += group_total;
    }
    Ok(out)
}

/// Decode one primitive at `prim_off`: the vertex-index list (at `vert_off`), the
/// texture block (`extract_textures`) for textured prims, or the per-vertex
/// colour block (`extract_colors`) for untextured `F*`/`G*` prims. Shared by
/// [`iter_groups`] and [`iter_groups_lenient`] so the two stay byte-identical.
#[allow(clippy::too_many_arguments)]
fn decode_prim(
    buf: &[u8],
    prim_off: usize,
    stride: usize,
    n_verts: usize,
    vert_off: Option<usize>,
    texblock_off: Option<usize>,
    is_textured: bool,
    is_gouraud: bool,
) -> Prim {
    let mut vertex_indices_raw = Vec::with_capacity(n_verts);
    if let Some(off) = vert_off
        && off + n_verts * 2 <= stride
    {
        for v in 0..n_verts {
            let o = prim_off + off + v * 2;
            vertex_indices_raw.push(u16::from_le_bytes(buf[o..o + 2].try_into().unwrap()));
        }
    }
    let (uvs, cba, tsb, colors) = if is_textured {
        let (uvs, cba, tsb) = match texblock_off {
            Some(off) => extract_textures(buf, prim_off, n_verts, off),
            None => (Vec::new(), 0, 0),
        };
        // Textured prims on the *baked-colour* rows (4/5, `byte1 = 3`) carry
        // the same leading colour block as the untextured rows - it is what
        // pushes the texture block off offset 0. The PSX GPU modulates the
        // texel by it (`texel * colour / 128`), so it is the field's entire
        // lighting signal and must not be dropped.
        //
        // The *lit* rows (0/1, `byte1 = 0`) have no colour block - the texture
        // block starts at offset 0 - so they get the neutral 0x80 the GPU
        // treats as "texel unchanged". See `MODULATION_NEUTRAL`.
        let colors = match texblock_off {
            Some(off) if off > 0 => extract_colors(buf, prim_off, n_verts, is_gouraud),
            _ => vec![[MODULATION_NEUTRAL; 3]; n_verts],
        };
        (uvs, cba, tsb, colors)
    } else {
        // Untextured (F*/G*): per-vertex colour block at the prim start.
        (
            Vec::new(),
            0,
            0,
            extract_colors(buf, prim_off, n_verts, is_gouraud),
        )
    };
    Prim {
        bytes_offset: prim_off,
        bytes_size: stride,
        vertex_indices_raw,
        uvs,
        cba,
        tsb,
        colors,
    }
}

/// Lenient sibling of [`iter_groups`]: walk groups until a terminator,
/// the section ends, or the next header would be malformed (overruns the
/// section, or has `ilen == 0` so the prim stride can't be derived).
///
/// On a malformed boundary, returns every well-formed group walked so far
/// instead of throwing the whole walk away. Mesh builders use this so a
/// single bad group near the end of an object's primitive section doesn't
/// hide every valid group that came before it - the visible symptom of
/// the strict variant was a TMD rendering only the first object's worth
/// of geometry, with the rest of the model missing.
pub fn iter_groups_lenient(buf: &[u8], section_start: usize, section_size: usize) -> Vec<Group> {
    let Some(section_end) = section_start.checked_add(section_size) else {
        return Vec::new();
    };
    if section_end > buf.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut pos = section_start;
    while pos + GROUP_HEADER_SIZE <= section_end {
        let Ok(header) = GroupHeader::parse(buf, pos) else {
            break;
        };
        if header.is_terminator() {
            break;
        }
        if header.ilen == 0 {
            break;
        }
        let header_offset = pos;
        let prim_base = pos + GROUP_HEADER_SIZE;
        let stride = header.prim_stride();
        let group_total = header.total_bytes();
        if pos + group_total > section_end {
            break;
        }
        let n_verts = header.n_vertices();
        let vert_off = vertex_offset_bytes(header.flags);
        let descriptor = Descriptor::for_flags(header.flags);
        let is_textured = descriptor.is_some_and(|d| d.packet_shape.is_textured());
        let is_gouraud = descriptor.is_some_and(|d| d.packet_shape.is_gouraud());
        // Texture block starts after the prim's leading colour words (if any);
        // the descriptor resolves the exact offset per row.
        let texblock_off = descriptor.and_then(|d| d.texture_block_offset);
        let mut prims = Vec::with_capacity(header.count as usize);
        for i in 0..header.count as usize {
            let prim_off = prim_base + i * stride;
            prims.push(decode_prim(
                buf,
                prim_off,
                stride,
                n_verts,
                vert_off,
                texblock_off,
                is_textured,
                is_gouraud,
            ));
        }
        out.push(Group {
            header_offset,
            header,
            prims,
        });
        pos += group_total;
    }
    out
}

/// Stats summarizing iter_groups output.
#[derive(Debug, Clone, Serialize)]
pub struct GroupStats {
    pub group_count: usize,
    pub total_prims: usize,
    pub triangles: usize,
    pub quads: usize,
    pub bytes_consumed: usize,
}

pub fn group_stats(section_start: usize, groups: &[Group]) -> GroupStats {
    let mut s = GroupStats {
        group_count: groups.len(),
        total_prims: 0,
        triangles: 0,
        quads: 0,
        bytes_consumed: 0,
    };
    for g in groups {
        let n = g.header.count as usize;
        s.total_prims += n;
        if g.header.n_vertices() == 4 {
            s.quads += n;
        } else {
            s.triangles += n;
        }
        s.bytes_consumed += g.header.total_bytes();
    }
    if let Some(last) = groups.last() {
        let end = last.header_offset + last.header.total_bytes();
        s.bytes_consumed = end - section_start;
    }
    s
}

#[cfg(test)]
mod tests;

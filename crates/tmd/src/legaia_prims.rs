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
//! (each vertex is 8 bytes — `SVECTOR { i16 x, y, z, pad }`), so the
//! array index is `raw_index / 8`.

use anyhow::{Result, bail};
use serde::Serialize;

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
}

/// Look up the byte offset within a primitive where vertex indices begin,
/// for a given group `flags` value.
///
/// The renderer indexes a 6-entry table at `DAT_8007326c` via
/// `((flags >> 1) - 8) >> 1`. The byte offset depends on whether the
/// primitive is a triangle or a quad, AND on the table entry's byte-3
/// "type" tag (which the renderer uses to override the offset for some
/// quad variants).
///
/// Triangle case (`(flags >> 1) & 1 == 0`):
///   `iVar2 = entry.byte4` (in u16 units)
///
/// Quad case (`(flags >> 1) & 1 == 1`):
///   `iVar2 = entry.byte4 + 2`, then override per byte 3:
///     byte3 == 0  -> iVar2 = entry.byte4   (cancel the +2)
///     byte3 == 1  -> iVar2 = 8             (override to 8)
///     byte3 == 3  -> iVar2 = 0xE           (override to 14)
///     else        -> iVar2 = entry.byte4 + 2 (no override)
///
/// Returns `None` for flags outside the known range.
pub fn vertex_offset_bytes(flags: u16) -> Option<usize> {
    let f_shifted = (flags as u32) >> 1;
    if !(8..=0x13).contains(&f_shifted) {
        return None;
    }
    let table_idx = ((f_shifted - 8) >> 1) as usize;
    // Each 8-byte table entry from DAT_8007326c. (byte3 of first u32, byte4
    // of second u32). Extracted from
    // `ghidra/scripts/funcs/data_8007325c_around_DAT_8007326c.txt`.
    //
    //   entry 0 = [04 00 00 05 07 00 00 00]  byte3=0x05  byte4=0x07
    //   entry 1 = [09 00 00 07 06 00 00 00]  byte3=0x07  byte4=0x06
    //   entry 2 = [04 00 00 00 02 00 00 00]  byte3=0x00  byte4=0x02
    //   entry 3 = [06 00 00 02 06 00 00 00]  byte3=0x02  byte4=0x06
    //   entry 4 = [07 03 00 01 07 00 00 00]  byte3=0x01  byte4=0x07
    //   entry 5 = [09 03 00 03 0B 00 00 00]  byte3=0x03  byte4=0x0B
    const TABLE: [(u8, u8); 6] = [
        (0x05, 0x07),
        (0x07, 0x06),
        (0x00, 0x02),
        (0x02, 0x06),
        (0x01, 0x07),
        (0x03, 0x0B),
    ];
    let (byte3, byte4) = TABLE[table_idx];
    let is_quad = (f_shifted & 1) == 1;
    let i_var2: u8 = if is_quad {
        match byte3 {
            0 => byte4,
            1 => 8,
            3 => 0xE,
            _ => byte4 + 2,
        }
    } else {
        byte4
    };
    Some(i_var2 as usize * 2)
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
    /// block (which Legaia's character TMDs always do — every observed mode
    /// in the 599-mesh battle_data corpus carries UVs). One entry per vertex.
    pub uvs: Vec<(u8, u8)>,
    /// CLUT base address (raw 16-bit value from the TMD). Decode with
    /// `cba_xy()` to get VRAM coordinates.
    pub cba: u16,
    /// Texture sub-base / "tpage" (raw 16-bit value). Decode with `tpage_xy()`
    /// for the VRAM page.
    pub tsb: u16,
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
/// Reverse-engineered from `dump_prim_bytes` on `0866_battle_data` corpus —
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
/// [`vertex_offset_bytes`]; the start is `vert_off - block_len`. Any prim
/// whose color section + texture block doesn't fit in `[0, vert_off)` is
/// treated as untextured (uvs empty, cba/tsb 0).
fn extract_textures(
    buf: &[u8],
    prim_off: usize,
    n_verts: usize,
    vert_off: usize,
) -> (Vec<(u8, u8)>, u16, u16) {
    let block_len = 4 + n_verts * 2; // CBA(2) + TSB(2) + UVs(n*2)
    if vert_off < block_len {
        return (Vec::new(), 0, 0);
    }
    let block_start = prim_off + vert_off - block_len;
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
        let mut prims = Vec::with_capacity(header.count as usize);
        for i in 0..header.count as usize {
            let prim_off = prim_base + i * stride;
            let mut vertex_indices_raw = Vec::with_capacity(n_verts);
            if let Some(off) = vert_off
                && off + n_verts * 2 <= stride
            {
                for v in 0..n_verts {
                    let o = prim_off + off + v * 2;
                    vertex_indices_raw.push(u16::from_le_bytes(buf[o..o + 2].try_into().unwrap()));
                }
            }
            let (uvs, cba, tsb) = if let Some(off) = vert_off {
                extract_textures(buf, prim_off, n_verts, off)
            } else {
                (Vec::new(), 0, 0)
            };
            prims.push(Prim {
                bytes_offset: prim_off,
                bytes_size: stride,
                vertex_indices_raw,
                uvs,
                cba,
                tsb,
            });
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
mod tests {
    use super::*;

    /// Build a section with one FT3 group: count=2, ilen=5, vertex_offset=14.
    /// Vertex indices are stored as raw byte-offsets (array_idx * 8).
    fn ft3_section() -> Vec<u8> {
        let mut buf = Vec::new();
        // Group header: count=2, flags=0x20, olen=7, ilen=5, flag=1, mode=0x27
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&0x0020u16.to_le_bytes());
        buf.extend_from_slice(&[7, 5, 1, 0x27]);
        // Prim 0: 20 bytes; vertex byte-offsets at bytes 14-19 for verts 0,1,2
        let mut prim0 = vec![0xAAu8; 20];
        for (vi, &raw) in [0u16, 8, 16].iter().enumerate() {
            let off = 14 + vi * 2;
            prim0[off..off + 2].copy_from_slice(&raw.to_le_bytes());
        }
        buf.extend_from_slice(&prim0);
        // Prim 1: vertex byte-offsets for verts 3,4,5
        let mut prim1 = vec![0xBBu8; 20];
        for (vi, &raw) in [24u16, 32, 40].iter().enumerate() {
            let off = 14 + vi * 2;
            prim1[off..off + 2].copy_from_slice(&raw.to_le_bytes());
        }
        buf.extend_from_slice(&prim1);
        // Footer slot (one extra ilen*4 = 20 bytes of padding)
        buf.extend_from_slice(&[0; 20]);
        // Terminator u32
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf
    }

    #[test]
    fn iterates_one_group() {
        let buf = ft3_section();
        let groups = iter_groups(&buf, 0, buf.len()).unwrap();
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.header.count, 2);
        assert_eq!(g.header.flags, 0x20);
        assert_eq!(g.header.ilen, 5);
        assert_eq!(g.header.n_vertices(), 3);
        assert_eq!(g.prims.len(), 2);
        assert_eq!(g.prims[0].vertex_indices(), vec![0, 1, 2]);
        assert_eq!(g.prims[1].vertex_indices(), vec![3, 4, 5]);
    }

    #[test]
    fn vertex_offset_lookup() {
        // entry 4 triangle (flags 0x20): byte4=0x07 -> 14 bytes
        assert_eq!(vertex_offset_bytes(0x20), Some(14));
        // entry 4 QUAD (flags 0x22): byte3==1 forces iVar2=8 -> 16 bytes
        assert_eq!(vertex_offset_bytes(0x22), Some(16));
        // entry 5 triangle (flags 0x24): byte4=0x0B -> 22 bytes
        assert_eq!(vertex_offset_bytes(0x24), Some(22));
        // entry 5 QUAD (flags 0x26): byte3==3 forces iVar2=0xE -> 28 bytes
        assert_eq!(vertex_offset_bytes(0x26), Some(28));
        // entry 2 triangle (flags 0x18): byte4=0x02 -> 4 bytes
        assert_eq!(vertex_offset_bytes(0x18), Some(4));
        // entry 2 QUAD (flags 0x1A): byte3==0 -> iVar2 = byte4 -> 4 bytes
        assert_eq!(vertex_offset_bytes(0x1A), Some(4));
        // Out-of-range flags
        assert_eq!(vertex_offset_bytes(0x00), None);
        assert_eq!(vertex_offset_bytes(0x100), None);
    }

    #[test]
    fn quad_vs_triangle_classification() {
        // Triangle (flags >> 1) & 1 == 0
        let h = GroupHeader {
            count: 1,
            flags: 0x20,
            olen: 7,
            ilen: 5,
            flag: 0,
            mode: 0,
        };
        assert_eq!(h.n_vertices(), 3);
        // Quad
        let h2 = GroupHeader {
            count: 1,
            flags: 0x22,
            olen: 9,
            ilen: 6,
            flag: 0,
            mode: 0,
        };
        assert_eq!(h2.n_vertices(), 4);
    }

    #[test]
    fn terminator_stops_iteration() {
        let mut buf = Vec::new();
        // Just a terminator
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        let groups = iter_groups(&buf, 0, buf.len()).unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn rejects_section_overrun() {
        // Truncated buffer: header says count=10 but only 8 bytes of buffer
        let buf = vec![10, 0, 0x20, 0, 7, 5, 1, 0x27];
        assert!(iter_groups(&buf, 0, buf.len()).is_err());
    }
}

//! Per-mode descriptor table for Legaia primitive groups.
//!
//! PORT: FUN_8002735C
//!
//! Reverse-engineered from `FUN_8002735c` (the renderer in `SCUS_942.54`)
//! and the in-memory table at `DAT_8007326c`. The renderer indexes a
//! 6-entry × 8-byte table to figure out, for each primitive group:
//!
//! 1. **Where the vertex indices are** (byte offset within the prim
//!    payload). Triangles use one offset; quads use a different offset
//!    derived from the same entry - see [`Descriptor::vertex_offset`].
//! 2. **Which DrawPolyXX packet shape** to emit (the GTE writes one of
//!    eight OT-packet variants, selected by entry-byte-3's low 2 bits in
//!    combination with the triangle/quad flag bit). See
//!    [`Descriptor::packet_shape`].
//!
//! The vertex-offset side is also exposed in the older
//! [`super::legaia_prims::vertex_offset_bytes`] free function; this module
//! reframes the same data as a typed struct so engine code can branch on
//! shading mode (`Flat` vs `Gouraud`) and texture presence
//! (`Untextured` vs `Textured`) without re-deriving the bit math.
//!
//! Provenance: `ghidra/scripts/funcs/data_8007325c_around_DAT_8007326c.txt`
//! (raw bytes), `ghidra/scripts/funcs/8002735C.txt` (renderer dispatch).

use serde::Serialize;

/// Per-prim packet shape - which `DrawPolyXX` SDK call the GTE emits.
///
/// The PSX SDK names these `POLY_F3`, `POLY_FT3`, `POLY_G3`, `POLY_GT3`
/// (triangles) and `POLY_F4`, `POLY_FT4`, `POLY_G4`, `POLY_GT4` (quads).
/// `F` = flat-shaded, `G` = gouraud-shaded; `T` = textured. The Legaia
/// renderer uses the entry's "byte 3" low 2 bits to select between the
/// 4 base shapes and combines that with the quad bit (`flags >> 1 & 1`)
/// to expand to 8 total shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketShape {
    /// `POLY_F3` - flat-shaded triangle.
    F3,
    /// `POLY_FT3` - flat-shaded textured triangle.
    Ft3,
    /// `POLY_G3` - gouraud-shaded triangle.
    G3,
    /// `POLY_GT3` - gouraud-shaded textured triangle.
    Gt3,
    /// `POLY_F4` - flat-shaded quad.
    F4,
    /// `POLY_FT4` - flat-shaded textured quad.
    Ft4,
    /// `POLY_G4` - gouraud-shaded quad.
    G4,
    /// `POLY_GT4` - gouraud-shaded textured quad.
    Gt4,
}

impl PacketShape {
    /// Number of vertices (3 for triangles, 4 for quads).
    pub fn n_vertices(self) -> usize {
        match self {
            Self::F3 | Self::Ft3 | Self::G3 | Self::Gt3 => 3,
            Self::F4 | Self::Ft4 | Self::G4 | Self::Gt4 => 4,
        }
    }

    /// `true` if the shape carries per-vertex texture coordinates (the
    /// `Ft*` / `Gt*` family).
    pub fn is_textured(self) -> bool {
        matches!(self, Self::Ft3 | Self::Gt3 | Self::Ft4 | Self::Gt4)
    }

    /// `true` if the shape is gouraud-shaded (per-vertex color, the `G*`
    /// family).
    pub fn is_gouraud(self) -> bool {
        matches!(self, Self::G3 | Self::Gt3 | Self::G4 | Self::Gt4)
    }

    /// `true` for quads (`*4` family).
    pub fn is_quad(self) -> bool {
        self.n_vertices() == 4
    }
}

/// Decoded per-mode descriptor entry. One of these is selected per primitive
/// group via [`Descriptor::for_flags`].
///
/// The on-disc representation is one row of the 6-row × 8-byte table at
/// `DAT_8007326C` plus the triangle/quad flag bit `(flags >> 1) & 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Descriptor {
    /// 0..=5 - which row of `DAT_8007326C` was selected.
    pub table_row: u8,
    /// Raw "byte 3" of the table entry - the OT-packet selector.
    pub raw_byte3: u8,
    /// Raw "byte 4" of the table entry - the base vertex-index offset
    /// (in u16 units).
    pub raw_byte4: u8,
    /// `true` for quads (derived from the original `flags` bit), `false`
    /// for triangles.
    pub is_quad: bool,
    /// Resolved DrawPolyXX shape.
    pub packet_shape: PacketShape,
    /// Resolved vertex-index byte offset within the prim payload.
    pub vertex_offset: usize,
    /// Byte offset of the texture block (`[u0 v0 cba][u1 v1 tsb][u2 v2 …]`)
    /// within the prim payload, for the `Ft*` / `Gt*` shapes. `None` for
    /// untextured shapes.
    ///
    /// The block sits **after** the prim's leading colour words, when
    /// present. Whether colours precede the texture block is encoded in the
    /// table entry's byte 1 (`0` = no leading colours - a light-source-lit
    /// prim carrying per-vertex normals *after* the vertex indices, so the
    /// texture block is at offset 0; non-zero = per-vertex baked colours
    /// precede it: one word for flat `Ft*`, `n_vertices` words for gouraud
    /// `Gt*`). Rows 0/1 of `DAT_8007326C` are the lit variants (byte1 = 0),
    /// rows 4/5 the baked-colour variants (byte1 = 3). The earlier
    /// `vertex_offset - block_len` heuristic only matched the byte1 = 3 rows;
    /// the byte1 = 0 rows read cba/tsb from geometry bytes and rendered as
    /// rainbow garbage.
    pub texture_block_offset: Option<usize>,
}

impl Descriptor {
    /// Decode the descriptor for a group's `flags` field. Returns `None`
    /// for `flags` values outside the documented range.
    pub fn for_flags(flags: u16) -> Option<Self> {
        let f_shifted = (flags as u32) >> 1;
        if !(8..=0x13).contains(&f_shifted) {
            return None;
        }
        let table_row = ((f_shifted - 8) >> 1) as u8;
        let is_quad = (f_shifted & 1) == 1;

        let (byte1, byte3, byte4) = TABLE[table_row as usize];
        // vertex_offset replicates the same logic as
        // legaia_prims::vertex_offset_bytes - kept here so the descriptor
        // is self-contained.
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
        let vertex_offset = i_var2 as usize * 2;

        // Packet shape is derived from byte3's low 2 bits + the quad flag.
        // Shape table from the renderer's GP0(0x20) / GP0(0x24) / GP0(0x30)
        // / GP0(0x34) etc. dispatch:
        //
        //   byte3 low 2 bits = 0  → F3  / F4   (flat)
        //   byte3 low 2 bits = 1  → FT3 / FT4  (flat textured)
        //   byte3 low 2 bits = 2  → G3  / G4   (gouraud)
        //   byte3 low 2 bits = 3  → GT3 / GT4  (gouraud textured)
        let shape_idx = byte3 & 0x3;
        let packet_shape = match (is_quad, shape_idx) {
            (false, 0) => PacketShape::F3,
            (false, 1) => PacketShape::Ft3,
            (false, 2) => PacketShape::G3,
            (false, _) => PacketShape::Gt3,
            (true, 0) => PacketShape::F4,
            (true, 1) => PacketShape::Ft4,
            (true, 2) => PacketShape::G4,
            (true, _) => PacketShape::Gt4,
        };

        // Texture-block offset: after any leading per-vertex colour words.
        // byte1 == 0 -> lit prim (normals trail the vertices), texture block
        // at offset 0. byte1 != 0 -> baked colours precede it: 1 word for a
        // flat `Ft*`, `n_vertices` words for a gouraud `Gt*`.
        let n_verts = packet_shape.n_vertices();
        let texture_block_offset = if packet_shape.is_textured() {
            let color_words = if byte1 == 0 {
                0
            } else if packet_shape.is_gouraud() {
                n_verts
            } else {
                1
            };
            Some(color_words * 4)
        } else {
            None
        };

        Some(Self {
            table_row,
            raw_byte3: byte3,
            raw_byte4: byte4,
            is_quad,
            packet_shape,
            vertex_offset,
            texture_block_offset,
        })
    }
}

/// The 6-row × 2-byte projection of `DAT_8007326C` actually consumed by
/// the renderer - `(byte3, byte4)` for each row. Bytes 0,1,2,5,6,7 of each
/// 8-byte slot are unused by `FUN_8002735c`'s fast-path dispatch.
///
/// Pulled from `ghidra/scripts/funcs/data_8007325c_around_DAT_8007326c.txt`:
///
/// ```text
/// row 0: 04 00 00 05 07 00 00 00  byte3=0x05  byte4=0x07
/// row 1: 09 00 00 07 06 00 00 00  byte3=0x07  byte4=0x06
/// row 2: 04 00 00 00 02 00 00 00  byte3=0x00  byte4=0x02
/// row 3: 06 00 00 02 06 00 00 00  byte3=0x02  byte4=0x06
/// row 4: 07 03 00 01 07 00 00 00  byte3=0x01  byte4=0x07
/// row 5: 09 03 00 03 0B 00 00 00  byte3=0x03  byte4=0x0B
/// ```
///
/// (Note: the 5th-column raw values are the *byte4*, not pulled from the
/// 4th column. The renderer treats the table as a packed
/// `{i32 first; i32 second}` struct and reads `(first >> 24)` for byte 3
/// and `(second & 0xFF)` for byte 4.)
///
/// `byte1` is `first`'s second byte - the renderer's per-vertex colour base
/// (`FUN_8002735c` reads it at `sp+0xb9`). It is `0` for rows 0-3 (lit prims,
/// no baked colours - the texture block starts at offset 0) and `3` for rows
/// 4-5 (baked-colour prims - colours precede the texture block). Consumed by
/// [`Descriptor::texture_block_offset`].
const TABLE: [(u8, u8, u8); 6] = [
    // (byte1, byte3, byte4)
    (0x00, 0x05, 0x07),
    (0x00, 0x07, 0x06),
    (0x00, 0x00, 0x02),
    (0x00, 0x02, 0x06),
    (0x03, 0x01, 0x07),
    (0x03, 0x03, 0x0B),
];

/// Number of rows in the per-mode descriptor table.
pub const TABLE_ROWS: usize = 6;

/// Iterate every flags value that decodes through the table. Real TMDs
/// only use the even values (`0x10, 0x12, …, 0x26`); the LSB of `flags`
/// is unused by the renderer's row-and-quad math.
pub fn known_flag_values() -> impl Iterator<Item = u16> {
    (0x10u16..=0x26)
        .step_by(2)
        .filter(|&f| Descriptor::for_flags(f).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_block_offset_lit_vs_baked_color_rows() {
        // Rows 0/1 (byte1 == 0) are the light-source-lit textured variants:
        // the texture block sits at offset 0, ahead of the vertices (normals
        // trail them). Byte-pinned from Rim Elm env-mesh pack 36:
        //   flags 0x13 (FT4) cba=0x7acd, 0x15 (GT3) cba=0x7ac0, 0x17 (GT4)
        //   cba=0x7ac5 - all valid CLUT(_, 491) at prim byte 2.
        assert_eq!(
            Descriptor::for_flags(0x13).unwrap().texture_block_offset,
            Some(0)
        ); // Ft4
        assert_eq!(
            Descriptor::for_flags(0x15).unwrap().texture_block_offset,
            Some(0)
        ); // Gt3
        assert_eq!(
            Descriptor::for_flags(0x17).unwrap().texture_block_offset,
            Some(0)
        ); // Gt4

        // Rows 4/5 (byte1 == 3) carry baked per-vertex colours before the
        // block: one word for flat `Ft*`, `n_vertices` words for gouraud
        // `Gt*`. These match the earlier `vertex_offset - block_len` result,
        // so the common case is unchanged.
        assert_eq!(
            Descriptor::for_flags(0x20).unwrap().texture_block_offset,
            Some(4)
        ); // Ft3: 1 colour word
        assert_eq!(
            Descriptor::for_flags(0x22).unwrap().texture_block_offset,
            Some(4)
        ); // Ft4: 1 colour word
        assert_eq!(
            Descriptor::for_flags(0x24).unwrap().texture_block_offset,
            Some(12)
        ); // Gt3: 3 colour words
        assert_eq!(
            Descriptor::for_flags(0x26).unwrap().texture_block_offset,
            Some(16)
        ); // Gt4: 4 colour words

        // Untextured shapes carry no texture block.
        assert_eq!(
            Descriptor::for_flags(0x1C).unwrap().texture_block_offset,
            None
        ); // G3
        assert_eq!(
            Descriptor::for_flags(0x1A).unwrap().texture_block_offset,
            None
        ); // F4
    }

    #[test]
    fn descriptor_decodes_row4_triangle() {
        // flags 0x20 → row 4 triangle. byte3=1 (FT*), byte4=7. Vertex
        // offset = byte4 * 2 = 14.
        let d = Descriptor::for_flags(0x20).expect("decode");
        assert_eq!(d.table_row, 4);
        assert_eq!(d.raw_byte3, 0x01);
        assert_eq!(d.raw_byte4, 0x07);
        assert!(!d.is_quad);
        assert_eq!(d.packet_shape, PacketShape::Ft3);
        assert_eq!(d.vertex_offset, 14);
        assert_eq!(d.packet_shape.n_vertices(), 3);
        assert!(d.packet_shape.is_textured());
        assert!(!d.packet_shape.is_gouraud());
    }

    #[test]
    fn descriptor_decodes_row4_quad() {
        // flags 0x22 → row 4 quad. byte3=1 forces vertex_offset = 8 * 2.
        let d = Descriptor::for_flags(0x22).expect("decode");
        assert_eq!(d.table_row, 4);
        assert!(d.is_quad);
        assert_eq!(d.packet_shape, PacketShape::Ft4);
        assert_eq!(d.vertex_offset, 16);
    }

    #[test]
    fn descriptor_decodes_row5_quad_with_byte3_3_override() {
        // flags 0x26 → row 5 quad. byte3=3 (GT*) forces vertex_offset=0xE*2=28.
        let d = Descriptor::for_flags(0x26).expect("decode");
        assert_eq!(d.table_row, 5);
        assert!(d.is_quad);
        assert_eq!(d.packet_shape, PacketShape::Gt4);
        assert_eq!(d.vertex_offset, 28);
        assert!(d.packet_shape.is_textured());
        assert!(d.packet_shape.is_gouraud());
    }

    #[test]
    fn descriptor_decodes_row2_quad_byte3_zero_no_plus_two() {
        // flags 0x1A → row 2 quad. byte3=0 cancels the +2 → vertex_offset=byte4*2=4.
        let d = Descriptor::for_flags(0x1A).expect("decode");
        assert_eq!(d.table_row, 2);
        assert!(d.is_quad);
        assert_eq!(d.packet_shape, PacketShape::F4);
        assert_eq!(d.vertex_offset, 4);
        assert!(!d.packet_shape.is_textured());
        assert!(!d.packet_shape.is_gouraud());
    }

    #[test]
    fn descriptor_decodes_row3_triangle_default_plus_two_branch() {
        // flags 0x1C → row 3 triangle. byte3=2 (G*) → triangles use byte4
        // directly, no +2 override (that's quad-only).
        let d = Descriptor::for_flags(0x1C).expect("decode");
        assert_eq!(d.table_row, 3);
        assert!(!d.is_quad);
        assert_eq!(d.packet_shape, PacketShape::G3);
        assert_eq!(d.vertex_offset, 12); // byte4=6 * 2
    }

    #[test]
    fn descriptor_decodes_row3_quad_default_plus_two_branch() {
        // flags 0x1E → row 3 quad. byte3=2 falls into the `_` branch
        // (NOT 0/1/3), so vertex_offset = (byte4 + 2) * 2 = 16.
        let d = Descriptor::for_flags(0x1E).expect("decode");
        assert_eq!(d.table_row, 3);
        assert!(d.is_quad);
        assert_eq!(d.packet_shape, PacketShape::G4);
        assert_eq!(d.vertex_offset, 16);
    }

    #[test]
    fn descriptor_rejects_out_of_range_flags() {
        assert!(Descriptor::for_flags(0x00).is_none());
        assert!(Descriptor::for_flags(0x0E).is_none());
        assert!(Descriptor::for_flags(0x28).is_none()); // > 0x27 means f_shifted > 0x13
        assert!(Descriptor::for_flags(0x100).is_none());
    }

    #[test]
    fn known_flag_values_yields_all_six_rows_both_winding() {
        let values: Vec<u16> = known_flag_values().collect();
        assert_eq!(values.len(), 12, "6 rows × 2 (tri/quad) = 12");
        // Spot-check a couple
        assert!(values.contains(&0x10));
        assert!(values.contains(&0x26));
    }

    #[test]
    fn descriptor_matches_legaia_prims_vertex_offset() {
        // The descriptor module must agree with the older free-function
        // [`super::super::legaia_prims::vertex_offset_bytes`] on every
        // valid flags value - they read from the same on-disc table.
        for flags in known_flag_values() {
            let d = Descriptor::for_flags(flags).unwrap();
            let expected = super::super::legaia_prims::vertex_offset_bytes(flags).unwrap();
            assert_eq!(
                d.vertex_offset, expected,
                "mismatch at flags 0x{flags:X}: descriptor={} vs free-fn={}",
                d.vertex_offset, expected,
            );
        }
    }

    #[test]
    fn packet_shape_classifications_consistent() {
        for flags in known_flag_values() {
            let d = Descriptor::for_flags(flags).unwrap();
            assert_eq!(d.packet_shape.is_quad(), d.is_quad);
            assert_eq!(d.packet_shape.n_vertices(), if d.is_quad { 4 } else { 3 });
        }
    }
}

//! The **widget-class table** at `SCUS_942.54` VA `0x800732A4` - the sprite
//! book every 2-D UI surface in the game draws itself out of, and the thing a
//! [`crate::screen_elements`] record's `+0x0E` *kind* byte indexes.
//!
//! Two SCUS routines consume it and between them they cover the whole chrome:
//!
//! * `FUN_8002C488(x, y, id)` - draw **one** sprite. Emits a `SPRT` whose
//!   `(u, v, w, h)` come from record `id` and whose CLUT comes from the
//!   record's palette byte, then a `DR_TPAGE` fixing the texture page.
//! * `FUN_8002C69C(x, y, w, h)` - draw a **sized** widget, with the record
//!   index in `gp+0x14C`. Dispatches on the record's class byte through the
//!   7-entry jump table at `0x80010D18`, applies the record's `(dx, dy)` to
//!   the seat, and then follows the record's signed chain delta to the next
//!   record until the delta is `0`.
//!
//! ## A record (`0x0C` bytes)
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0x00` | u8 | frame **class** - selects the arm that lays the widget out |
//! | `+0x01` | u8 | **tile-set** index (see [`FRAME_TILESET_VA`]) |
//! | `+0x02` | i8 | **chain delta** to the next record; `0` ends the run |
//! | `+0x03` | u8 | **palette** byte (bit 7 = semi-transparent, rest selects the CLUT) |
//! | `+0x04`..`+0x07` | u8 x4 | sprite source rect `u`, `v`, `w`, `h` |
//! | `+0x08` / `+0x0A` | i16 | seat bias `dx` / `dy` applied by `FUN_8002C69C` |
//!
//! `FUN_8002C488` ignores `+0x08`/`+0x0A` and seats the sprite at the caller's
//! `(x, y)` verbatim; only the sized path biases the seat. That split is why
//! the status marker lands at `pen + (0x3B, 2)` (its caller's own offset)
//! while the roster panel's `HP` label lands at `pen + (-1, 17)` (the
//! record's).
//!
//! ## The palette byte is a packed CLUT address
//!
//! Both routines decode `+0x03` the same way, and it has two forms:
//!
//! ```text
//! bit 6 clear:  CBA = 0x7FC0 + (b & 0x3F)     -> VRAM row 511, x = (b & 0x3F) * 16
//! bit 6 set:    fb_y = 498 + ((b & 0x3F) >> 2)
//!               fb_x = 896 + (b & 3) * 16
//! ```
//!
//! The first form is the resident system-UI sheet's own 16-colour sub-palette
//! strip on VRAM row 511. The second is a **separate 4x4 block of sub-palettes
//! at VRAM `(896.., 498..501)`**, and it is what makes each element badge a
//! different colour off one strip of texels: the badges are consecutive
//! records, so `b` walks `0x40, 0x41, ...` and the decode walks the block
//! row-major. See [`clut_fb`].
//!
//! ## Where the texels are
//!
//! Every record but the four portrait ids samples texture page
//! [`SHEET_TPAGE`] = VRAM `(896, 256)`, the resident system-UI TIM. The
//! portraits ([`SPRITE_PORTRAIT_FIRST`] and [`SPRITE_PORTRAIT_FRAME`]) take
//! [`PORTRAIT_TPAGE`] = `(960, 256)` and read their CLUT out of the four-word
//! side table at [`PORTRAIT_CLUT_TABLE_VA`] instead of the palette byte.

use crate::screen_elements::ExeMap;

/// RAM address of widget record 0.
pub const TABLE_VA: u32 = 0x8007_32A4;
/// Bytes per widget record.
pub const RECORD_STRIDE: usize = 0x0C;

/// Widget records `0x800732A4..0x80073A00`.
///
/// The bound is structural, not a heuristic: `TABLE_VA + 0x9D * 0x0C` is
/// exactly [`FRAME_TILESET_VA`], the frame tile-set pool the class arms read.
/// The data agrees - record `0x9C` is the last one whose `(u, v, w, h)` fits a
/// 256x256 texture page, and `0x9D` already carries a four-digit seat bias.
pub const RECORD_COUNT: usize = 0x9D;

/// The frame **tile-set** pool: eight `(u, v, w, h)` byte quads per set,
/// `0x20` bytes apart, read by the corner-framed window classes.
///
/// Quad order, as the class-0 arm emits them:
/// top-left, top-right, bottom-left, bottom-right, top edge, bottom edge,
/// left edge, right edge. Corners draw once; each edge tiles at its own size
/// with the final tile clipped to the remainder - the same clip law the plate
/// runs use.
pub const FRAME_TILESET_VA: u32 = 0x8007_3A00;
/// Bytes per tile-set.
pub const TILESET_STRIDE: usize = 0x20;

/// The **plate cap-pair** view of the same pool, `0x8` apart: the class-3 arm
/// (the rounded plate runs - chips, plaques, the status bar) reads a
/// `(left cap, right cap)` quad pair from here rather than a whole frame set.
///
/// It is the same bytes at a different origin and stride: cap pair `i` is
/// tile-set `(i + 3) / 4`'s quads `6`/`7` when `i >= 3`. Index `0` is the
/// "no caps" sentinel the arm skips.
pub const FRAME_CAP_PAIR_VA: u32 = 0x8007_3A60;

/// Texture page every widget but the portraits samples - the resident
/// system-UI TIM at VRAM `(896, 256)`, 4bpp.
pub const SHEET_TPAGE: u16 = 0x1E;
/// Texture page the portrait ids sample - VRAM `(960, 256)`, 4bpp.
pub const PORTRAIT_TPAGE: u16 = 0x1F;
/// Four-word CLUT side table the portrait ids read instead of their palette
/// byte (`FUN_8002C488`'s `param_3 - 0x86 < 3 || param_3 == 0x8A` arm).
pub const PORTRAIT_CLUT_TABLE_VA: u32 = 0x8007_3DB8;

/// The `LV` label sprite - the no-ailment arm of the status ladder.
pub const SPRITE_LEVEL_MARKER: u8 = 0x0A;
/// First status-element badge id.
pub const SPRITE_STATUS_FIRST: u8 = 0x18;
/// Last status-element badge id (the KO badge).
pub const SPRITE_STATUS_LAST: u8 = 0x20;
/// First **plain** element badge; eight consecutive records.
pub const SPRITE_ELEMENT_BADGE_FIRST: u8 = 0x8B;
/// First **winged** element badge; eight consecutive records.
pub const SPRITE_ELEMENT_BADGE_WINGED_FIRST: u8 = 0x94;
/// Number of element badges in each strip.
pub const ELEMENT_BADGE_COUNT: usize = 8;
/// First party-member portrait id; three consecutive records.
pub const SPRITE_PORTRAIT_FIRST: u8 = 0x86;
/// The empty portrait frame that shares the portraits' page and CLUT table.
pub const SPRITE_PORTRAIT_FRAME: u8 = 0x8A;

/// The nine status badges in ladder order, with the word each one has printed
/// on it. The art is a two-column block of 48x16 cells on the system-UI sheet;
/// the tenth cell of the block is other art, not a tenth badge.
///
/// The mask column is the bit `FUN_8002C2E4` tests to select the badge; `None`
/// is the KO badge, which the ladder reaches on zero HP rather than on a bit.
pub const STATUS_BADGES: [(u8, Option<u16>, &str); 9] = [
    (0x18, Some(0x0001), "Venom"),
    (0x19, Some(0x0002), "Toxic"),
    (0x1A, Some(0x0004), "Stone"),
    (0x1B, Some(0x0078), "Rot"),
    (0x1C, Some(0x0380), "Rage"),
    (0x1D, Some(0x0400), "Numb"),
    (0x1E, Some(0x0800), "Sleep"),
    (0x1F, Some(0x1000), "Curse"),
    (0x20, None, "Faint"),
];

/// Decode a widget palette byte into the VRAM `(x, y)` of its 16-entry CLUT.
///
/// Both consumers run this arithmetic inline and identically; it is the whole
/// answer to "which sub-palette does this sprite take".
// REF: FUN_8002c488 (0x8002c5d4..0x8002c608), FUN_8002c69c (0x8002c898..0x8002c8d0)
pub const fn clut_fb(pal: u8) -> (u16, u16) {
    if pal & 0x40 != 0 {
        let k = (pal & 0x3F) as u16;
        (896 + (k & 3) * 16, 498 + (k >> 2))
    } else {
        let cba = 0x7FC0u16 + (pal & 0x7F) as u16;
        ((cba & 0x3F) * 16, (cba >> 6) & 0x1FF)
    }
}

/// The GP0 primitive code a record's palette byte selects: `0x66` (raw sprite,
/// texture unblended) when bit 7 is set, else `0x64`.
pub const fn sprite_code(pal: u8) -> u8 {
    if pal & 0x80 != 0 { 0x66 } else { 0x64 }
}

/// One `(u, v, w, h)` byte quad out of the frame tile-set pool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TileQuad {
    pub u: u8,
    pub v: u8,
    pub w: u8,
    pub h: u8,
}

/// One `0x0C`-byte widget-class record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Widget {
    /// `+0x00` - frame class (`0..=6`; the jump table has seven arms).
    pub class: u8,
    /// `+0x01` - tile-set index.
    pub tileset: u8,
    /// `+0x02` - signed chain delta to the next record; `0` ends the run.
    pub chain: i8,
    /// `+0x03` - palette byte.
    pub palette: u8,
    /// `+0x04`..`+0x07` - sprite source rect on the sheet.
    pub rect: (u8, u8, u8, u8),
    /// `+0x08` / `+0x0A` - seat bias applied by the sized draw path.
    pub bias: (i16, i16),
}

impl Widget {
    /// VRAM `(x, y)` of this record's 16-entry CLUT.
    pub const fn clut_fb(&self) -> (u16, u16) {
        clut_fb(self.palette)
    }

    /// The GP0 sprite code this record emits.
    pub const fn sprite_code(&self) -> u8 {
        sprite_code(self.palette)
    }

    /// Texture page this record samples.
    pub const fn tpage(&self) -> u16 {
        SHEET_TPAGE
    }
}

/// The decoded widget-class table plus the frame tile-set pool behind it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WidgetTable {
    records: Vec<Widget>,
    tileset_pool: Vec<u8>,
}

impl WidgetTable {
    /// Parse the table out of a `SCUS_942.54` image.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let base = map.off(TABLE_VA)?;
        let mut records = Vec::with_capacity(RECORD_COUNT);
        for i in 0..RECORD_COUNT {
            let o = base + i * RECORD_STRIDE;
            let r = scus.get(o..o + RECORD_STRIDE)?;
            let i16at = |k: usize| i16::from_le_bytes([r[k], r[k + 1]]);
            records.push(Widget {
                class: r[0],
                tileset: r[1],
                chain: r[2] as i8,
                palette: r[3],
                rect: (r[4], r[5], r[6], r[7]),
                bias: (i16at(0x08), i16at(0x0A)),
            });
        }
        // The tile-set pool starts exactly where the records stop; keep the
        // five well-formed sets plus the cap-pair tail they share.
        let pool_base = map.off(FRAME_TILESET_VA)?;
        let pool_len = TILESET_STRIDE * 5;
        let tileset_pool = scus.get(pool_base..pool_base + pool_len)?.to_vec();
        Some(Self {
            records,
            tileset_pool,
        })
    }

    /// Every record, in table order.
    pub fn records(&self) -> &[Widget] {
        &self.records
    }

    /// Record `id`, if the table is that long.
    pub fn get(&self, id: u8) -> Option<Widget> {
        self.records.get(id as usize).copied()
    }

    /// Walk the chain starting at `id`, yielding each record's `(id, record)`
    /// in draw order. Stops on a `0` delta, an out-of-range hop, or a repeat
    /// (retail's own data terminates, but a caller may hand in any byte).
    // REF: FUN_8002c69c (0x8002ff00 - `lb v1, 0x2(s7)`, add into the index, re-enter)
    pub fn chain_from(&self, id: u8) -> Vec<(u8, Widget)> {
        let mut out = Vec::new();
        let mut seen = [false; 256];
        let mut cur = id;
        while let Some(w) = self.get(cur) {
            if seen[cur as usize] {
                break;
            }
            seen[cur as usize] = true;
            out.push((cur, w));
            if w.chain == 0 {
                break;
            }
            let Some(next) = (cur as i16).checked_add(w.chain as i16) else {
                break;
            };
            if !(0..RECORD_COUNT as i16).contains(&next) {
                break;
            }
            cur = next as u8;
        }
        out
    }

    /// The eight quads of frame tile-set `index`, if it is one of the
    /// well-formed sets.
    pub fn tileset(&self, index: u8) -> Option<[TileQuad; 8]> {
        let base = index as usize * TILESET_STRIDE;
        let r = self.tileset_pool.get(base..base + TILESET_STRIDE)?;
        let mut quads = [TileQuad::default(); 8];
        for (i, q) in quads.iter_mut().enumerate() {
            *q = TileQuad {
                u: r[i * 4],
                v: r[i * 4 + 1],
                w: r[i * 4 + 2],
                h: r[i * 4 + 3],
            };
        }
        Some(quads)
    }

    /// The `(left cap, right cap)` quad pair a plate run of tile-set `index`
    /// draws. Index `0` is the sentinel the class-3 arm skips.
    pub fn plate_caps(&self, index: u8) -> Option<(TileQuad, TileQuad)> {
        if index == 0 {
            return None;
        }
        let base = (FRAME_CAP_PAIR_VA - FRAME_TILESET_VA) as usize + index as usize * 8;
        let r = self.tileset_pool.get(base..base + 8)?;
        Some((
            TileQuad {
                u: r[0],
                v: r[1],
                w: r[2],
                h: r[3],
            },
            TileQuad {
                u: r[4],
                v: r[5],
                w: r[6],
                h: r[7],
            },
        ))
    }
}

/// Source rect + CLUT for element badge `index` off the plain strip.
pub fn element_badge(table: &WidgetTable, index: usize) -> Option<Widget> {
    if index >= ELEMENT_BADGE_COUNT {
        return None;
    }
    table.get(SPRITE_ELEMENT_BADGE_FIRST + index as u8)
}

/// The word printed on status badge `sprite`, if it is one of the nine.
pub fn status_badge_label(sprite: u8) -> Option<&'static str> {
    STATUS_BADGES
        .iter()
        .find(|(id, _, _)| *id == sprite)
        .map(|(_, _, label)| *label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_byte_decodes_both_clut_forms() {
        // Row-511 form: sub-palette index times 16.
        assert_eq!(clut_fb(0x00), (0, 511));
        assert_eq!(clut_fb(0x04), (64, 511));
        assert_eq!(clut_fb(0x0C), (192, 511));
        // Bit 7 only selects the raw-sprite code, never the CLUT.
        assert_eq!(clut_fb(0x83), clut_fb(0x03));
        assert_eq!(sprite_code(0x83), 0x66);
        assert_eq!(sprite_code(0x03), 0x64);
        // Badge form: a 4x4 block at (896.., 498..), walked row-major.
        assert_eq!(clut_fb(0x40), (896, 498));
        assert_eq!(clut_fb(0x41), (912, 498));
        assert_eq!(clut_fb(0x45), (912, 499));
        assert_eq!(clut_fb(0x47), (944, 499));
        assert_eq!(clut_fb(0x48), (896, 500));
        assert_eq!(clut_fb(0x4F), (944, 501));
    }

    #[test]
    fn the_record_run_stops_where_the_tileset_pool_starts() {
        assert_eq!(
            TABLE_VA as usize + RECORD_COUNT * RECORD_STRIDE,
            FRAME_TILESET_VA as usize
        );
        // The cap-pair view is the fourth tile-set's origin.
        assert_eq!(
            FRAME_TILESET_VA as usize + 3 * TILESET_STRIDE,
            FRAME_CAP_PAIR_VA as usize
        );
    }

    #[test]
    fn status_badges_cover_the_ladder_exactly() {
        assert_eq!(STATUS_BADGES.len(), 9);
        for (i, (id, _, _)) in STATUS_BADGES.iter().enumerate() {
            assert_eq!(*id, SPRITE_STATUS_FIRST + i as u8);
        }
        assert_eq!(STATUS_BADGES[8].0, SPRITE_STATUS_LAST);
        assert_eq!(status_badge_label(0x20), Some("Faint"));
        assert_eq!(status_badge_label(0x21), None);
    }

    #[test]
    fn rejects_a_non_exe() {
        assert!(WidgetTable::from_scus(b"not an exe").is_none());
    }
}

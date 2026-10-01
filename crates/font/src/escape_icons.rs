//! The sprites the `0xCE` escapes draw, decoded off the disc.
//!
//! REF: FUN_8002C488, FUN_80036888  -- the sprite decode only, for previews
//!
//! A string escape (`string_id != 0` in the table at `0x80074050`) makes
//! `FUN_80036888` call `FUN_8002C488(x, y + y_offset, string_id)`, which for
//! every id outside `0x86..=0x88` / `0x8A` emits one textured sprite from the
//! 12-byte record at `0x800732A4 + id*12`:
//!
//! | Offset | Field |
//! |---|---|
//! | `+3` | CLUT byte `b` |
//! | `+4` / `+5` | U / V |
//! | `+6` / `+7` | width / height |
//!
//! The CLUT word is `0x7FC0 + (b & 0x7F)` - palette `b & 0x7F` of VRAM row
//! 511 - unless bit `0x40` is set, in which case it is
//! `((b & 0x3F) >> 2) + 0x1F2` rows down and `0x38 + (b & 3)` columns across:
//! VRAM `(896 + (b & 3) * 16, 498 + (b & 0x3F) / 4)`.
//!
//! The sprite carries no texture page of its own. Its UVs resolve to the
//! icons only against the **system-UI sheet** at VRAM `(896, 256)`; against
//! the font page `(896, 0)` they land on accent-glyph cells. Every texel and
//! palette involved sits in the boot-resident TIMs at the head of `PROT.DAT`
//! (before the first TOC entry):
//!
//! | `PROT.DAT` offset | Upload |
//! |---|---|
//! | `0x018E0` | system-UI sheet, 256x192 at `(896, 256)`; 16 palettes on row 511 |
//! | `0x07B00` | one palette at `(304, 511)` (the button icons' `b = 0x13`) |
//! | `0x10178` / `0x100D0` / `0x10028` / `0x0FF80` | four palettes each on rows 498 / 499 / 500 / 501; the first also uploads the 256x32 strip at `(896, 448)` |
//!
//! A TIM's CLUT block is laid down as one row of `w * h` colours at its
//! origin - the layout under which palette `k` of the sheet is VRAM
//! `(16k, 511)`, as the CLUT words above address it.

use crate::{EscapeEntry, EscapeTable};
use anyhow::{Context, Result, bail};

/// First byte of `PROT.DAT` [`EscapeIcons::from_disc`] needs.
pub const ICON_PROT_DAT_OFFSET: u64 = 0x18E0;
/// Byte length from [`ICON_PROT_DAT_OFFSET`] that covers every TIM the icons
/// read (through the end of the TIM at `0x10178`).
pub const ICON_PROT_DAT_LEN: usize = 0x11218 - 0x18E0;

/// `PROT.DAT` offsets of the TIMs uploaded, in upload order.
const ICON_TIMS: [usize; 6] = [0x18E0, 0x7B00, 0x10178, 0x100D0, 0x10028, 0xFF80];

/// SCUS VA of the 38-entry escape table.
const ESCAPE_TABLE_RAM: u32 = 0x8007_4050;
/// Entries in the escape table.
pub const ESCAPE_COUNT: usize = 38;
/// SCUS VA of the 12-byte sprite records `FUN_8002C488` draws from.
const SPRITE_TABLE_RAM: u32 = 0x8007_32A4;
/// Texture page the escape sprites decode against (the system-UI sheet).
const PAGE_X: usize = 896;
const PAGE_Y: usize = 256;

/// One escape's sprite, as RGBA8 (transparent where the texel's colour is
/// `0x0000`, the PSX transparency rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscapeIcon {
    /// The escape operand.
    pub index: u8,
    pub w: u32,
    pub h: u32,
    /// Vertical offset from the text line's top (`y_offset` of the entry).
    pub y_offset: i8,
    /// Pen advance after the sprite.
    pub advance: u8,
    pub rgba: Vec<u8>,
}

/// Every escape's sprite, indexed by operand (`None` for the numeric
/// escapes `0x0B..=0x0E` and anything `FUN_8002C488` routes elsewhere).
#[derive(Debug, Clone)]
pub struct EscapeIcons {
    pub icons: Vec<Option<EscapeIcon>>,
}

fn scus_off(scus: &[u8], va: u32) -> Option<usize> {
    let t_addr = if scus.len() >= 0x40 && &scus[0..8] == b"PS-X EXE" {
        u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?)
    } else {
        0x8001_0000
    };
    Some(va.checked_sub(t_addr)? as usize + 0x800)
}

/// The escape table read straight from `SCUS_942.54`.
pub fn escape_table_from_scus(scus: &[u8]) -> Result<EscapeTable> {
    let off = scus_off(scus, ESCAPE_TABLE_RAM).context("escape table below t_addr")?;
    let bytes = scus
        .get(off..off + ESCAPE_COUNT * 4)
        .context("SCUS too short for the escape table")?;
    Ok(EscapeTable {
        entries: bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|e| EscapeEntry {
                string_id: i16::from_le_bytes([e[0], e[1]]),
                advance_px: e[2],
                y_offset: e[3] as i8,
            })
            .collect(),
    })
}

struct Vram(Vec<u16>);

impl Vram {
    const W: usize = 1024;
    const H: usize = 512;
    fn put(&mut self, x: usize, y: usize, v: u16) {
        if x < Self::W && y < Self::H {
            self.0[y * Self::W + x] = v;
        }
    }
    fn get(&self, x: usize, y: usize) -> u16 {
        if x < Self::W && y < Self::H {
            self.0[y * Self::W + x]
        } else {
            0
        }
    }
    /// Upload one TIM (CLUT as a single row, image as its rect).
    fn load_tim(&mut self, tim: &[u8]) -> Result<()> {
        let u32_at = |o: usize| -> Result<u32> {
            Ok(u32::from_le_bytes(
                tim.get(o..o + 4).context("TIM truncated")?.try_into()?,
            ))
        };
        let u16_at = |o: usize| -> Result<u16> {
            Ok(u16::from_le_bytes(
                tim.get(o..o + 2).context("TIM truncated")?.try_into()?,
            ))
        };
        if u32_at(0)? != 0x10 {
            bail!("not a TIM");
        }
        let flags = u32_at(4)?;
        let mut p = 8usize;
        let block = |p: &mut usize, as_row: bool, vram: &mut Vram| -> Result<()> {
            let len = u32_at(*p)? as usize;
            let (x, y) = (u16_at(*p + 4)? as usize, u16_at(*p + 6)? as usize);
            let (w, h) = (u16_at(*p + 8)? as usize, u16_at(*p + 10)? as usize);
            for k in 0..w * h {
                let v = u16_at(*p + 12 + k * 2)?;
                if as_row {
                    vram.put(x + k, y, v);
                } else {
                    vram.put(x + k % w, y + k / w, v);
                }
            }
            *p += len;
            Ok(())
        };
        if flags & 8 != 0 {
            block(&mut p, true, self)?;
        }
        block(&mut p, false, self)
    }
}

impl EscapeIcons {
    /// Decode every escape sprite. `prot_head` is `PROT.DAT` from
    /// [`ICON_PROT_DAT_OFFSET`] (at least [`ICON_PROT_DAT_LEN`] bytes);
    /// `scus` is the whole executable.
    pub fn from_disc(prot_head: &[u8], scus: &[u8]) -> Result<Self> {
        let mut vram = Vram(vec![0; Vram::W * Vram::H]);
        for off in ICON_TIMS {
            let rel = off - ICON_PROT_DAT_OFFSET as usize;
            let tim = prot_head.get(rel..).context("PROT.DAT head too short")?;
            vram.load_tim(tim)
                .with_context(|| format!("icon TIM at PROT.DAT 0x{off:X}"))?;
        }
        let table = escape_table_from_scus(scus)?;
        let sprites = scus_off(scus, SPRITE_TABLE_RAM).context("sprite table below t_addr")?;
        let mut icons = Vec::with_capacity(ESCAPE_COUNT);
        for (i, e) in table.entries.iter().enumerate() {
            let id = e.string_id;
            if id <= 0 || matches!(id, 0x86..=0x88 | 0x8A) {
                icons.push(None);
                continue;
            }
            let r = scus
                .get(sprites + id as usize * 12..sprites + id as usize * 12 + 12)
                .context("sprite record past SCUS")?;
            let b = r[3];
            let (u, v, w, h) = (r[4] as usize, r[5] as usize, r[6] as usize, r[7] as usize);
            let (cx, cy) = if b & 0x40 != 0 {
                let n = (b & 0x3F) as usize;
                (PAGE_X + (n & 3) * 16, 498 + (n >> 2))
            } else {
                (((b & 0x7F) as usize) * 16, 511)
            };
            let mut rgba = vec![0u8; w * h * 4];
            for y in 0..h {
                for x in 0..w {
                    let (tu, tv) = ((u + x) & 0xFF, (v + y) & 0xFF);
                    let hw = vram.get(PAGE_X + tu / 4, PAGE_Y + tv);
                    let texel = ((hw >> ((tu % 4) * 4)) & 0xF) as usize;
                    let c = vram.get(cx + texel, cy);
                    if c == 0 {
                        continue;
                    }
                    let ch = |s: u16| {
                        let v = (s & 0x1F) as u8;
                        (v << 3) | (v >> 2)
                    };
                    let o = (y * w + x) * 4;
                    rgba[o..o + 4].copy_from_slice(&[ch(c), ch(c >> 5), ch(c >> 10), 255]);
                }
            }
            icons.push(Some(EscapeIcon {
                index: i as u8,
                w: w as u32,
                h: h as u32,
                y_offset: e.y_offset,
                advance: e.advance_px,
                rgba,
            }));
        }
        Ok(Self { icons })
    }

    /// The sprite for escape operand `index`.
    pub fn get(&self, index: u8) -> Option<&EscapeIcon> {
        self.icons.get(index as usize).and_then(|i| i.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic disc head: every TIM a 4bpp page whose texels are all `1`,
    /// palettes whose colour 1 is opaque; SCUS with a two-entry table.
    #[test]
    fn decodes_a_synthetic_sprite_and_skips_numbers() {
        let mut head = vec![0u8; ICON_PROT_DAT_LEN];
        let tim = |clut: (u16, u16, u16, u16), img: (u16, u16, u16, u16), fill: u16| {
            let mut t = Vec::new();
            t.extend_from_slice(&0x10u32.to_le_bytes());
            t.extend_from_slice(&8u32.to_le_bytes());
            let cn = (clut.2 * clut.3) as usize;
            t.extend_from_slice(&((12 + cn * 2) as u32).to_le_bytes());
            for v in [clut.0, clut.1, clut.2, clut.3] {
                t.extend_from_slice(&v.to_le_bytes());
            }
            for k in 0..cn {
                let c: u16 = if k % 16 == 1 { 0x7FFF } else { 0 };
                t.extend_from_slice(&c.to_le_bytes());
            }
            let n = (img.2 * img.3) as usize;
            t.extend_from_slice(&((12 + n * 2) as u32).to_le_bytes());
            for v in [img.0, img.1, img.2, img.3] {
                t.extend_from_slice(&v.to_le_bytes());
            }
            for _ in 0..n {
                t.extend_from_slice(&fill.to_le_bytes());
            }
            t
        };
        for off in ICON_TIMS {
            let t = if off == 0x18E0 {
                tim((0, 511, 16, 16), (896, 256, 64, 192), 0x1111)
            } else {
                tim((896, 498, 16, 1), (896, 448, 4, 4), 0)
            };
            let rel = off - 0x18E0;
            head[rel..rel + t.len()].copy_from_slice(&t);
        }
        // SCUS: raw image loaded at 0x80010000 (no EXE header).
        let mut scus = vec![0u8; 0x80000];
        let esc = scus_off(&scus, ESCAPE_TABLE_RAM).unwrap();
        scus[esc..esc + 4].copy_from_slice(&[55, 0, 16, 0xFE]);
        scus[esc + 4..esc + 8].copy_from_slice(&[0, 0, 32, 1]);
        let spr = scus_off(&scus, SPRITE_TABLE_RAM).unwrap() + 55 * 12;
        scus[spr..spr + 8].copy_from_slice(&[5, 0, 0, 2, 0, 0, 4, 2]);
        let icons = EscapeIcons::from_disc(&head, &scus).unwrap();
        let a = icons.get(0).unwrap();
        assert_eq!((a.w, a.h, a.y_offset, a.advance), (4, 2, -2, 16));
        assert_eq!(&a.rgba[0..4], &[255, 255, 255, 255]);
        assert!(icons.get(1).is_none(), "numeric escape draws no sprite");
    }
}

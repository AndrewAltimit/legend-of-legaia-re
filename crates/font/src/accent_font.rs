//! The accent font: accented Latin glyphs added to the dialog-font page at
//! patch time, built from the user's own disc.
//!
//! The dialog font is one 4bpp 256x256 TIM (`PROT.DAT` offset
//! [`crate::FONT_TIM_PROT_DAT_OFFSET`], member 3 of the boot system-UI pack)
//! indexed by byte, plus the 256-byte advance table in `SCUS_942.54`
//! (`0x80073F1C`). Every cell of the [`crate::latin`] layout that has a
//! [`Recipe`] is rebuilt here: the base letter is copied out of the same page
//! (the disc's own `e`, `A`, `?`), and the diacritic is drawn from the small
//! fill masks below, in the page's two ink indices - `15` fill and `14` drop
//! shadow, the shadow being the fill dilated one pixel right, down and
//! diagonally (the rule every retail glyph but three follows). The cell's
//! advance is the base letter's own width-table entry.
//!
//! Nothing here carries glyph bytes: the output is a function of the input
//! page, so the only pixels a patched disc gains are the user's own letters
//! plus marks drawn from the masks in this file. See
//! `docs/formats/dialog-font.md#the-accent-font`.

use anyhow::{Context, Result, bail};

use crate::latin::{LATIN_CELLS, LatinCell, Mark, Recipe};

/// Page width / height in pixels.
const PAGE: usize = 256;
/// Cell pitch in pixels.
const CELL: usize = 16;
/// Drawn columns / rows of a cell (the GP0 sprite is 14x15).
const DRAW_W: usize = crate::GLYPH_W as usize;
const DRAW_H: usize = crate::GLYPH_H as usize;
/// Palette indices the font page draws with.
const FILL: u8 = 15;
const SHADOW: u8 = crate::FONT_SHADOW_INDEX;
/// Lowest fill row of a descender (`g`, `p`, `y` end here).
const DESCENDER_ROW: usize = 12;
/// Top fill row of the retail capitals.
const CAP_TOP_ROW: usize = 2;

/// One 16x16 cell of palette indices.
type Cell = [[u8; CELL]; CELL];

/// The decoded font page: 256x256 palette indices.
#[derive(Clone, PartialEq, Eq)]
pub struct FontPage {
    px: Vec<u8>,
}

impl std::fmt::Debug for FontPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontPage").finish_non_exhaustive()
    }
}

/// Byte offset of the image block's pixel data inside the font TIM.
fn pixel_offset(tim: &[u8]) -> Result<usize> {
    let rd = |o: usize| -> Result<u32> {
        tim.get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| anyhow::anyhow!("font TIM truncated at 0x{o:X}"))
    };
    if rd(0)? != 0x10 {
        bail!("not a TIM (bad magic)");
    }
    let flags = rd(4)?;
    let mut p = 8usize;
    if flags & 0x8 != 0 {
        p += rd(p)? as usize;
    }
    let off = p + 12;
    if tim.len() < off + PAGE * PAGE / 2 {
        bail!("font TIM pixel data truncated");
    }
    Ok(off)
}

impl FontPage {
    /// Decode the page from the font TIM (as read from `PROT.DAT`).
    pub fn from_tim(tim: &[u8]) -> Result<Self> {
        Ok(Self {
            px: crate::decode_font_tim(tim).context("decode dialog-font TIM")?,
        })
    }

    /// Re-pack the page into `tim`'s image block in place (the header and
    /// CLUT are left alone, so the TIM keeps its size).
    pub fn write_into_tim(&self, tim: &mut [u8]) -> Result<()> {
        let off = pixel_offset(tim)?;
        for y in 0..PAGE {
            for x in (0..PAGE).step_by(2) {
                let lo = self.px[y * PAGE + x] & 0xF;
                let hi = self.px[y * PAGE + x + 1] & 0xF;
                tim[off + y * (PAGE / 2) + x / 2] = lo | (hi << 4);
            }
        }
        Ok(())
    }

    fn origin(byte: u8) -> (usize, usize) {
        (
            ((byte & 0x0F) as usize) * CELL,
            ((byte & 0xF0) as usize) - 0x20,
        )
    }

    fn cell(&self, byte: u8) -> Cell {
        let (u, v) = Self::origin(byte);
        let mut c = [[0u8; CELL]; CELL];
        for (y, row) in c.iter_mut().enumerate() {
            for (x, p) in row.iter_mut().enumerate() {
                *p = self.px[(v + y) * PAGE + u + x];
            }
        }
        c
    }

    fn set_cell(&mut self, byte: u8, c: &Cell) {
        let (u, v) = Self::origin(byte);
        for (y, row) in c.iter().enumerate() {
            for (x, p) in row.iter().enumerate() {
                self.px[(v + y) * PAGE + u + x] = *p;
            }
        }
    }

    /// `true` when the cell has any ink in its drawn 14x15 area.
    pub fn has_ink(&self, byte: u8) -> bool {
        if byte < crate::FIRST_CHAR {
            return false;
        }
        let c = self.cell(byte);
        c.iter()
            .take(DRAW_H)
            .any(|r| r.iter().take(DRAW_W).any(|&p| p != 0))
    }
}

/// Fill-pixel bounding box `(min_x, max_x, min_y, max_y)`.
fn fill_bbox(c: &Cell) -> Option<(usize, usize, usize, usize)> {
    let mut bb: Option<(usize, usize, usize, usize)> = None;
    for (y, row) in c.iter().enumerate() {
        for (x, &p) in row.iter().enumerate() {
            if p == FILL {
                bb = Some(match bb {
                    None => (x, x, y, y),
                    Some((a, b, cc, d)) => (a.min(x), b.max(x), cc.min(y), d.max(y)),
                });
            }
        }
    }
    bb
}

/// Paint fill pixels `(x, y)` and give each the drop shadow the page uses,
/// without covering ink already there.
fn paint(c: &mut Cell, pts: &[(i32, i32)]) {
    let inside =
        |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < DRAW_W && (y as usize) < DRAW_H;
    for &(x, y) in pts {
        if inside(x, y) {
            c[y as usize][x as usize] = FILL;
        }
    }
    for &(x, y) in pts {
        for (dx, dy) in [(1, 0), (0, 1), (1, 1)] {
            let (sx, sy) = (x + dx, y + dy);
            if inside(sx, sy) && c[sy as usize][sx as usize] == 0 {
                c[sy as usize][sx as usize] = SHADOW;
            }
        }
    }
}

/// Fill pixels of a `#`-mask placed at `(x0, y0)`.
fn mask_points(rows: &[&str], x0: i32, y0: i32) -> Vec<(i32, i32)> {
    let mut pts = Vec::new();
    for (dy, row) in rows.iter().enumerate() {
        for (dx, ch) in row.bytes().enumerate() {
            if ch == b'#' {
                pts.push((x0 + dx as i32, y0 + dy as i32));
            }
        }
    }
    pts
}

fn mark_mask(m: Mark) -> &'static [&'static str] {
    match m {
        Mark::Acute => &[".##", "#.."],
        Mark::Grave => &["##.", "..#"],
        Mark::Circumflex => &[".#.", "#.#"],
        Mark::Diaeresis => &["#.#"],
        Mark::Tilde => &[".#.#", "#.#."],
        Mark::Ring => &["###", "#.#", "###"],
        Mark::Cedilla => &[".#", "##"],
    }
}

/// Shift a cell's ink down by `dy` rows.
fn shift_down(c: &Cell, dy: usize) -> Cell {
    let mut out = [[0u8; CELL]; CELL];
    for y in 0..CELL {
        if y + dy < CELL {
            out[y + dy] = c[y];
        }
    }
    out
}

/// Drop the dot of an `i`/`j`: everything above the first ink-free row that
/// follows the top ink row.
fn dotless(c: &Cell) -> Cell {
    let Some((_, _, top, bottom)) = fill_bbox(c) else {
        return *c;
    };
    let row_has_fill = |y: usize| c[y].contains(&FILL);
    let Some(gap) = (top..=bottom).find(|&y| !row_has_fill(y)) else {
        return *c;
    };
    let mut out = *c;
    for row in out.iter_mut().take(gap) {
        *row = [0; CELL];
    }
    out
}

/// Build one glyph cell. `widths` is the (unpatched) advance table.
/// Returns the cell and its advance-table entry.
fn build(page: &FontPage, widths: &[u8; 256], recipe: Recipe) -> Option<(Cell, u8)> {
    match recipe {
        Recipe::Accent(base, mark) => {
            let mut c = page.cell(base);
            if matches!(base, b'i' | b'j') && mark != Mark::Cedilla {
                c = dotless(&c);
            }
            let (min_x, max_x, top, bottom) = fill_bbox(&c)?;
            let mask = mark_mask(mark);
            let mw = mask.iter().map(|r| r.len()).max().unwrap_or(0) as i32;
            let mh = mask.len() as i32;
            let bw = (max_x - min_x + 1) as i32;
            let x0 = (min_x as i32 + (bw - mw + 1).div_euclid(2)).clamp(0, DRAW_W as i32 - mw);
            if mark == Mark::Cedilla {
                paint(&mut c, &mask_points(mask, x0, bottom as i32 + 1));
            } else {
                // Mark rows, its shadow row, then the letter.
                let y0 = top as i32 - mh - 1;
                if y0 < 0 {
                    c = shift_down(&c, (-y0) as usize);
                }
                paint(&mut c, &mask_points(mask, x0, y0.max(0)));
            }
            Some((c, widths[base as usize]))
        }
        Recipe::Ligature(a, b) => {
            let ca = page.cell(a);
            let cb = page.cell(b);
            let (_, max_a, _, _) = fill_bbox(&ca)?;
            let (min_b, _, _, _) = fill_bbox(&cb)?;
            let shift = max_a.checked_sub(min_b)?;
            let mut c = ca;
            for (row, src_row) in c.iter_mut().zip(cb.iter()) {
                for (x, &p) in src_row.iter().enumerate() {
                    let tx = x + shift;
                    if p == 0 || tx >= DRAW_W {
                        continue;
                    }
                    if p == FILL || row[tx] == 0 {
                        row[tx] = p;
                    }
                }
            }
            let (_, max_x, _, _) = fill_bbox(&c)?;
            if max_x + 1 >= DRAW_W {
                return None;
            }
            Some((c, (max_x + 1) as u8))
        }
        Recipe::Inverted(base) => {
            let src = page.cell(base);
            let (min_x, max_x, top, bottom) = fill_bbox(&src)?;
            let h = bottom - top + 1;
            let dst_top = (DESCENDER_ROW + 1).checked_sub(h)?;
            // Turned half a turn about the ink's box, then dropped so its
            // last row sits on the descender line.
            let mut pts = Vec::new();
            for (y, row) in src.iter().enumerate().take(bottom + 1).skip(top) {
                for (x, &p) in row.iter().enumerate().take(max_x + 1).skip(min_x) {
                    if p == FILL {
                        let fy = dst_top + (bottom - y);
                        let fx = min_x + (max_x - x);
                        pts.push((fx as i32, fy as i32));
                    }
                }
            }
            let mut c = [[0u8; CELL]; CELL];
            paint(&mut c, &pts);
            Some((c, widths[base as usize]))
        }
        Recipe::Drawn(rows) => {
            let mut c = [[0u8; CELL]; CELL];
            paint(&mut c, &mask_points(rows, 0, CAP_TOP_ROW as i32));
            let (_, max_x, _, _) = fill_bbox(&c)?;
            Some((c, (max_x + 1) as u8))
        }
    }
}

/// What [`build_accent_font`] produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccentFont {
    /// The page with every drawable layout cell rebuilt.
    pub page: FontPage,
    /// The advance table with every rebuilt cell's entry set.
    pub widths: [u8; 256],
    /// Bytes whose cell was rebuilt, ascending.
    pub cells: Vec<u8>,
}

/// Build the accent font from a disc's font page and advance table.
///
/// Only the layout's recipe cells change; every ASCII cell and every other
/// byte's advance is left as the disc has it. Deterministic: the same input
/// page gives the same output, which is what [`accent_font_state`] relies on.
pub fn build_accent_font(page: &FontPage, widths: &[u8; 256]) -> AccentFont {
    let mut out = page.clone();
    let mut w = *widths;
    let mut cells = Vec::new();
    for cell in LATIN_CELLS {
        let Some(recipe) = cell.recipe else { continue };
        if let Some((c, adv)) = build(page, widths, recipe) {
            out.set_cell(cell.byte, &c);
            w[cell.byte as usize] = adv;
            cells.push(cell.byte);
        }
    }
    AccentFont {
        page: out,
        widths: w,
        cells,
    }
}

/// Whether a disc already carries the accent font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccentFontState {
    /// Every layout cell and advance is the accent font's.
    Applied,
    /// None of them is.
    Absent,
    /// Some are: another font patch, or a different revision of this one.
    Partial,
}

/// Compare a disc's page + advance table against the accent font rebuilt
/// from that same page. The ASCII cells the recipes read are never touched
/// by the patch, so rebuilding from a patched page reproduces the patch.
pub fn accent_font_state(page: &FontPage, widths: &[u8; 256]) -> AccentFontState {
    let built = build_accent_font(page, widths);
    let mut same = 0usize;
    for &b in &built.cells {
        let adv_ok = widths[b as usize] == built.widths[b as usize];
        if adv_ok && page.cell(b) == built.page.cell(b) {
            same += 1;
        }
    }
    if same == built.cells.len() && same > 0 {
        AccentFontState::Applied
    } else if same == 0 {
        AccentFontState::Absent
    } else {
        AccentFontState::Partial
    }
}

/// How one byte draws in a given font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellDraw {
    /// Ink and a non-zero advance: draws as a letter.
    Draws,
    /// Ink, but a zero advance-table entry: the glyph draws and the next
    /// letter lands one pixel to its right, on top of it.
    Overprints,
    /// No ink: draws nothing (the pen still moves by the advance).
    Blank,
}

/// Classify how `byte` draws with this page and advance table.
pub fn cell_draw(page: &FontPage, widths: &[u8; 256], byte: u8) -> CellDraw {
    if !page.has_ink(byte) {
        CellDraw::Blank
    } else if widths[byte as usize] == 0 {
        CellDraw::Overprints
    } else {
        CellDraw::Draws
    }
}

/// File offset of the advance table inside `SCUS_942.54`.
pub fn width_table_file_offset(scus: &[u8]) -> Result<usize> {
    let t_addr = if scus.len() >= 0x40 && &scus[0..8] == b"PS-X EXE" {
        u32::from_le_bytes(scus[0x18..0x1C].try_into().unwrap())
    } else {
        0x8001_0000
    };
    let off = crate::WIDTH_TABLE_RAM
        .checked_sub(t_addr)
        .map(|v| v as usize + 0x800)
        .ok_or_else(|| anyhow::anyhow!("width table below t_addr"))?;
    if off + 256 > scus.len() {
        bail!("width table past SCUS end");
    }
    Ok(off)
}

/// Read the advance table out of `SCUS_942.54`.
pub fn read_widths(scus: &[u8]) -> Result<[u8; 256]> {
    crate::read_scus_widths(scus)
}

/// The layout cells a typed character can land on, for callers that want
/// the table without importing [`crate::latin`] too.
pub fn drawn_cells() -> impl Iterator<Item = &'static LatinCell> {
    LATIN_CELLS.iter().filter(|c| c.recipe.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic page: every printable ASCII byte gets a block glyph whose
    /// shape depends on the byte (no disc bytes involved).
    fn synthetic() -> (FontPage, [u8; 256]) {
        let mut page = FontPage {
            px: vec![0; PAGE * PAGE],
        };
        let mut widths = [0u8; 256];
        for b in 0x21u8..=0x7E {
            let lower = b.is_ascii_lowercase();
            let (top, bottom): (usize, usize) = if lower { (5, 10) } else { (2, 10) };
            let w = 3 + (b as usize % 4);
            let mut c = [[0u8; CELL]; CELL];
            let mut pts = Vec::new();
            for y in top..=bottom {
                for x in 0..w {
                    if x == 0 || x == w - 1 || y == top || y == bottom {
                        pts.push((x as i32, y as i32));
                    }
                }
            }
            if b == b'i' {
                // A dot, a gap, then the stem.
                pts.retain(|&(_, y)| y > 6);
                pts.push((1, 4));
            }
            paint(&mut c, &pts);
            page.set_cell(b, &c);
            widths[b as usize] = w as u8;
        }
        (page, widths)
    }

    #[test]
    fn builds_every_recipe_cell() {
        let (page, widths) = synthetic();
        let f = build_accent_font(&page, &widths);
        let expected = LATIN_CELLS.iter().filter(|c| c.recipe.is_some()).count();
        assert_eq!(f.cells.len(), expected);
        for &b in &f.cells {
            assert!(f.page.has_ink(b), "{b:02x}");
            assert!(f.widths[b as usize] > 0, "{b:02x}");
            assert_eq!(cell_draw(&f.page, &f.widths, b), CellDraw::Draws);
        }
    }

    #[test]
    fn ascii_cells_and_other_widths_untouched() {
        let (page, widths) = synthetic();
        let f = build_accent_font(&page, &widths);
        for b in 0x20u8..=0x7E {
            assert_eq!(f.page.cell(b), page.cell(b), "{b:02x}");
        }
        for b in 0..=255u8 {
            if !f.cells.contains(&b) {
                assert_eq!(f.widths[b as usize], widths[b as usize]);
            }
        }
    }

    #[test]
    fn accent_takes_base_advance_and_sits_above() {
        let (page, widths) = synthetic();
        let f = build_accent_font(&page, &widths);
        assert_eq!(f.widths[0x82], widths[b'e' as usize]);
        let c = f.page.cell(0x82);
        let (_, _, top, _) = fill_bbox(&c).unwrap();
        assert!(top < 5, "mark above the x-height");
        // A capital shifts down to make room, staying inside the cell.
        let c = f.page.cell(0x90);
        let (_, _, top, bottom) = fill_bbox(&c).unwrap();
        assert_eq!(top, 0);
        assert!(bottom < DRAW_H);
    }

    #[test]
    fn dotless_i_under_marks() {
        let (page, widths) = synthetic();
        let f = build_accent_font(&page, &widths);
        let c = f.page.cell(0xA1); // i-acute
        // The synthetic dot sat at row 4; the mark replaces it higher up.
        assert_ne!(c[4][1], FILL);
    }

    #[test]
    fn state_round_trips() {
        let (page, widths) = synthetic();
        assert_eq!(accent_font_state(&page, &widths), AccentFontState::Absent);
        let f = build_accent_font(&page, &widths);
        assert_eq!(
            accent_font_state(&f.page, &f.widths),
            AccentFontState::Applied
        );
        let mut half = f.widths;
        half[0x82] = 0;
        assert_eq!(accent_font_state(&f.page, &half), AccentFontState::Partial);
    }

    #[test]
    fn tim_repack_round_trips() {
        let (page, _) = synthetic();
        // Minimal 4bpp TIM with a CLUT block and the 64x256 image block.
        let mut tim = vec![0u8; 8 + 44 + 12 + PAGE * PAGE / 2];
        tim[0] = 0x10;
        tim[4] = 0x08;
        tim[8..12].copy_from_slice(&44u32.to_le_bytes());
        let img = 52;
        tim[img..img + 4].copy_from_slice(&((12 + PAGE * PAGE / 2) as u32).to_le_bytes());
        tim[img + 4..img + 6].copy_from_slice(&896u16.to_le_bytes());
        tim[img + 8..img + 10].copy_from_slice(&64u16.to_le_bytes());
        tim[img + 10..img + 12].copy_from_slice(&256u16.to_le_bytes());
        page.write_into_tim(&mut tim).unwrap();
        assert_eq!(FontPage::from_tim(&tim).unwrap(), page);
    }
}

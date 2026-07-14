//! Minigame **art packs** - the textures the minigame overlays upload to VRAM.
//!
//! The three playable minigame rules engines ([`crate::slot_payout`],
//! [`crate::baka_opponents`], [`crate::dance_chart`]) decode the *numbers* a
//! minigame plays with out of its code overlay. This module decodes the
//! *pictures*: the reel symbols, the marquee, the digit font, the paytable
//! panel. The machine's *geometry* - it is a 3D scene, not a sprite collage -
//! lives next door in [`crate::minigame_slot_scene`].
//!
//! ## Loader provenance
//!
//! The casino slot machine's overlay init `FUN_801CEC94`
//! (`overlay_slot_machine_801cec94.txt`) issues three asset loads before the
//! reel state machine runs. Each is a `li a0, <raw TOC index>` feeding a loader
//! call; the extraction entry is `raw - 2` (see `docs/formats/cdname.md`):
//!
//! | disasm | raw | extraction entry | role |
//! |---|---|---|---|
//! | `li a0,0x4b2` -> `jal 0x8003eb98` | `0x4B2` | **1200** | the texture pack (this module) |
//! | `li a0,0x4b1` -> `jal 0x8003eb98` | `0x4B1` | 1199 | sibling art entry |
//! | `li a0,0x4b0` -> `jal 0x8001fc00` | `0x4B0` | 1198 | sibling art entry |
//!
//! Entry 1200 is a standard descriptor container ([`crate::parse_player_lzs`]):
//! descriptor 0 is a **`TIM_LIST`** (type byte `0x01`) whose LZS-decoded body is
//! a [`crate::pack`] of five standard PSX TIMs. Their framebuffer destinations
//! are exactly the texture pages and CLUT rows the reel renderer
//! `FUN_801d0fa8` and the HUD rasteriser `FUN_801d2cc0` sample:
//!
//! ```text
//! pack | image fb   | CLUT row | texpage attr | role
//! -----|------------|----------|--------------|-----------------------------------------
//!   0  | (768,   0) |   490    | 0x0C         | reel symbols + digit font + medallion
//!   1  | (832,   0) |   491    | 0x0D         | bonus-strip multiplier numerals (1..10)
//!   2  | (768, 256) |   492    | 0x1C         | marquee panel, mascots, pedestals, lamps
//!   3  | (832, 256) |   493    | 0x1D         | dot-matrix message bank + lamp swatches + cursor
//!   4  | (640,   0) |   494    | 0x8A         | the paytable / coin info panel (8bpp - see below)
//! ```
//!
//! Page 4 is the odd one: its texpage attribute has the GPU's **8-bit** colour
//! bit set, so the block is 128 texels wide and its CLUT is one 256-entry
//! palette. Decoding it as the TIM header's `4bpp` claims yields noise. It is
//! *not* the machine's cabinet - the cabinet is not in this pack at all (see
//! [`crate::minigame_slot_scene`]).
//!
//! Every image block is **byte-identical** to a retail VRAM dump taken at the
//! slot machine (the `minigame_slot_machine` capture), which is what pins the
//! `texpage 0x0C = (768,0)` / `0x0D = (832,0)` split the slot-machine page
//! previously carried as an open question.
//!
//! ## Sprite geometry (all Confirmed from the disassembly)
//!
//! - **Reel symbols** - `FUN_801d0fa8` computes UVs arithmetically from the
//!   symbol id, with no descriptor table: a 4x4 grid of 64x64 cells on the
//!   `0x0C` page, `U = (sym & 3) * 0x40`, `V = (sym & 0xC) * 0x10`, and a
//!   **per-symbol CLUT** at id `0x7A80 + sym` (row 490, column `sym`). Symbol
//!   ids `0..=9` are the ten reel symbols. The per-symbol CLUT is load-bearing:
//!   several symbols share one cell of artwork and are told apart *only* by
//!   their palette.
//! - **Bonus numerals** - the same three lines of `FUN_801d0fa8`, rebased. When
//!   the strip value clears `0x10` (a bonus round swaps the reels onto the
//!   `0x10..=0x19` strip) the renderer bumps the texpage to `0x0D` and the CLUT
//!   base to `0x7AC0`, so the `1..=10` faces are **their own 64x64 artwork on
//!   page 1**, each with its own palette column - which is why every numeral on
//!   the retail bonus reels is a different colour. [`slot_bonus_number`].
//! - **Digit font** - `FUN_801d2914` draws the coin readout from the same
//!   `0x0C` page: `U = 0x40 + digit * 0x10`, `V = 0xC0`, 16x16 per glyph, CLUT
//!   `0x7A8D` (row 490, column 13). The 64x16 cell at `(0, 0xC0)` immediately
//!   left of digit `0` is the **"COIN"** label.
//! - **HUD widgets** - the 3-record table at `DAT_801d347c` (PROT 0975 file
//!   offset [`SLOT_HUD_TABLE_OFFSET`], 20-byte stride) that `FUN_801d2cc0`
//!   indexes. Parsed by [`parse_slot_hud`]; the records resolve to the paytable
//!   info panel ([`slot_info_panel`]), the "COIN" label and the cash-out cursor.
//!   Unlike everything else the machine draws, these three are **screen-space**:
//!   `FUN_801d2cc0` takes a pixel position and writes the quad's XY directly,
//!   with no GTE projection.

use anyhow::{Context, Result, bail};
use legaia_tim::{PixelMode, Tim, decode_rgba8, parse as parse_tim};

use crate::{DecodeMode, decode, pack, parse_player_lzs};

/// Extraction PROT entry carrying the slot machine's texture pack (raw TOC
/// index `0x4B2`, loaded by `FUN_801CEC94`).
pub const SLOT_ART_PROT_INDEX: usize = 1200;

/// Asset type byte for a `TIM_LIST` descriptor.
const TYPE_TIM_LIST: u8 = 0x01;

/// Index into the decoded pack of the page the reel renderer samples
/// (texpage `0x0C`, fb `(768, 0)`).
pub const SLOT_SYMBOL_PAGE: usize = 0;
/// Index of the **bonus numeral** page - the `1..=10` reel faces a bonus round
/// swaps the reels onto (texpage `0x0D`, fb `(832, 0)`, CLUT row 491). See
/// [`slot_bonus_number`].
pub const SLOT_BONUS_PAGE: usize = 1;
/// Index of the banner/cursor page (texpage `0x1D`, fb `(832, 256)`).
pub const SLOT_BANNER_PAGE: usize = 3;
/// Index of the paytable / coin info-panel page (texpage `0x8A`, fb `(640, 0)`).
/// Sampled as **8bpp** - see [`slot_info_panel`].
pub const SLOT_PANEL_PAGE: usize = 4;
/// The info panel's cell on its page: `uv (0, 16)`, 127x239 (HUD record 0).
pub const SLOT_PANEL_CELL: (u8, u8, u8, u8) = (0, 16, 127, 239);

/// Reel symbol ids `0..=9` (`FUN_801d13e8` indexes the payout table with these).
pub const SLOT_SYMBOL_COUNT: usize = 10;
/// Edge of one reel-symbol cell, in texels (`FUN_801d0fa8`).
pub const SLOT_SYMBOL_CELL: usize = 64;

/// CLUT id base the reel renderer samples: `0x7A80 + sym`.
pub const SLOT_SYMBOL_CLUT_BASE: u16 = 0x7A80;
/// CLUT id base for a **bonus strip value** (`>= 0x10`): `0x7AC0 + (value & 0xF)`
/// - CLUT row 491, one palette column per numeral (`FUN_801d0fa8`).
pub const SLOT_BONUS_CLUT_BASE: u16 = 0x7AC0;
/// Numerals on the bonus strip (`1..=10`).
pub const SLOT_BONUS_NUMBER_COUNT: usize = 10;
/// CLUT id the digit font samples (`FUN_801d2914`).
pub const SLOT_DIGIT_CLUT: u16 = 0x7A8D;

/// `V` row of the digit strip on the `0x0C` page (`FUN_801d2914`).
pub const SLOT_DIGIT_V: usize = 0xC0;
/// `U` of digit `0`; digit `d` sits at `SLOT_DIGIT_U0 + d * SLOT_DIGIT_W`.
pub const SLOT_DIGIT_U0: usize = 0x40;
/// Per-glyph cell of the digit font.
pub const SLOT_DIGIT_W: usize = 0x10;
/// Height of the digit / "COIN" strip.
pub const SLOT_DIGIT_H: usize = 0x10;

/// File offset of the 3-record HUD widget descriptor table (`DAT_801d347c`)
/// inside the raw slot overlay entry (PROT 0975).
pub const SLOT_HUD_TABLE_OFFSET: usize = 0x4C64;
/// Stride of one HUD widget descriptor.
pub const SLOT_HUD_STRIDE: usize = 0x14;
/// Records in the HUD widget table.
pub const SLOT_HUD_RECORDS: usize = 3;

/// A CLUT id, split into the VRAM cell it addresses.
///
/// PSX encoding: bits 0..=5 are `x / 16`, bits 6..=14 are `y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClutId(pub u16);

impl ClutId {
    /// VRAM column of the palette's first entry.
    pub fn x(self) -> u16 {
        (self.0 & 0x3F) * 16
    }
    /// VRAM row.
    pub fn y(self) -> u16 {
        self.0 >> 6
    }
    /// Palette index *within* a TIM whose CLUT block starts at column 0 of the
    /// same row - i.e. the `clut_idx` to hand [`legaia_tim::decode_rgba8`].
    pub fn palette_index(self) -> usize {
        (self.0 & 0x3F) as usize
    }
}

/// A texpage attribute, split into the VRAM page origin it addresses.
///
/// PSX encoding: bits 0..=3 are `x / 64`, bit 4 is `y / 256`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TexPage(pub u16);

impl TexPage {
    pub fn x(self) -> u16 {
        (self.0 & 0xF) * 64
    }
    pub fn y(self) -> u16 {
        ((self.0 >> 4) & 1) * 256
    }
}

/// One record of the slot machine's HUD widget descriptor table
/// (`DAT_801d347c`, read by `FUN_801d2cc0`). Field offsets are those documented
/// on `docs/subsystems/minigame-slot-machine.md`.
#[derive(Debug, Clone, Copy)]
pub struct SlotHudWidget {
    /// `+0x00` base-size scale, 20.12 fixed point.
    pub scale: i32,
    /// `+0x04` texpage attribute.
    pub texpage: TexPage,
    /// `+0x06` CLUT id.
    pub clut: ClutId,
    /// `+0x08` texture origin within the page.
    pub u: u8,
    pub v: u8,
    /// `+0x0A` cell size.
    pub w: u8,
    pub h: u8,
}

/// Parse the 3 HUD widget descriptors out of the **raw** slot overlay entry
/// (PROT 0975). Returns the cabinet panel, the "COIN" label and the cash-out
/// cursor, in table order.
pub fn parse_slot_hud(overlay: &[u8]) -> Result<Vec<SlotHudWidget>> {
    let end = SLOT_HUD_TABLE_OFFSET + SLOT_HUD_RECORDS * SLOT_HUD_STRIDE;
    if overlay.len() < end {
        bail!(
            "slot overlay too small ({}b) for the HUD table at 0x{:X}",
            overlay.len(),
            SLOT_HUD_TABLE_OFFSET
        );
    }
    let rd16 = |o: usize| u16::from_le_bytes([overlay[o], overlay[o + 1]]);
    Ok((0..SLOT_HUD_RECORDS)
        .map(|i| {
            let o = SLOT_HUD_TABLE_OFFSET + i * SLOT_HUD_STRIDE;
            SlotHudWidget {
                scale: i32::from_le_bytes(overlay[o..o + 4].try_into().unwrap()),
                texpage: TexPage(rd16(o + 4)),
                clut: ClutId(rd16(o + 6)),
                u: overlay[o + 8],
                v: overlay[o + 9],
                w: overlay[o + 0x0A],
                h: overlay[o + 0x0B],
            }
        })
        .collect())
}

/// A decoded RGBA8 sprite lifted out of an art page.
#[derive(Debug, Clone)]
pub struct Sprite {
    pub width: usize,
    pub height: usize,
    /// Row-major RGBA8. Palette entry `0x0000` decodes to alpha `0`.
    pub rgba: Vec<u8>,
}

/// Decode the five-TIM art pack out of a **raw** PROT entry 1200.
///
/// The entry is a descriptor container; descriptor 0 (`TIM_LIST`) LZS-decodes to
/// a [`crate::pack`] of standard PSX TIMs.
pub fn parse_art_pack(entry: &[u8]) -> Result<Vec<Tim>> {
    if entry.len() < 8 {
        bail!(
            "art entry too small ({}b) for a container header",
            entry.len()
        );
    }
    // The container's first meta word is the descriptor count.
    let count = u32::from_le_bytes(entry[0..4].try_into().unwrap()) as usize;
    if count == 0 || count > 16 {
        bail!("implausible descriptor count {count} in art entry");
    }
    let container = parse_player_lzs(entry, count)?;
    let desc = container
        .descriptors
        .iter()
        .find(|d| d.type_byte == TYPE_TIM_LIST)
        .context("art entry carries no TIM_LIST descriptor")?;
    let body = decode(entry, desc, DecodeMode::Lzs).context("LZS-decoding the TIM_LIST")?;
    let bodies = pack::extract_pack(&body).context("parsing the TIM_LIST pack")?;
    bodies
        .iter()
        .enumerate()
        .map(|(i, b)| parse_tim(b).with_context(|| format!("pack TIM {i}")))
        .collect()
}

/// Crop a `w x h` rect out of a full-page RGBA8 decode.
fn crop(page: &[u8], page_w: usize, x: usize, y: usize, w: usize, h: usize) -> Result<Sprite> {
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        let start = ((y + row) * page_w + x) * 4;
        let end = start + w * 4;
        let slice = page
            .get(start..end)
            .context("sprite crop runs past the decoded page")?;
        rgba.extend_from_slice(slice);
    }
    Ok(Sprite {
        width: w,
        height: h,
        rgba,
    })
}

/// The 64x64 cell a reel **strip value** occupies on its art page, as the reel
/// renderer computes it - there is no descriptor table (`FUN_801d0fa8`):
/// `U = (value & 3) * 0x40`, `V = (value & 0xC) * 0x10`. A 4x4 grid, walked
/// row-major, and the same arithmetic serves both strips: the symbol ids
/// `0..=9` on the `0x0C` page and the bonus values `0x10..=0x19` on the `0x0D`
/// page (whose low nibble is `0..=9` again).
fn reel_cell(value: usize) -> (usize, usize) {
    ((value & 3) * 0x40, (value & 0x0C) * 0x10)
}

/// Decode reel symbol `sym` (`0..=9`) at its retail cell and **its own** CLUT.
///
/// Port of the UV + CLUT arithmetic in `FUN_801d0fa8`.
pub fn slot_symbol(art: &[Tim], sym: usize) -> Result<Sprite> {
    if sym >= SLOT_SYMBOL_COUNT {
        bail!("reel symbol {sym} out of range (0..{SLOT_SYMBOL_COUNT})");
    }
    let tim = art
        .get(SLOT_SYMBOL_PAGE)
        .context("art pack has no symbol page")?;
    // CLUT id 0x7A80 + sym -> palette column `sym` of the page's CLUT block.
    let page = decode_rgba8(
        tim,
        ClutId(SLOT_SYMBOL_CLUT_BASE + sym as u16).palette_index(),
    )?;
    let (u, v) = reel_cell(sym);
    crop(
        &page,
        tim.pixel_width(),
        u,
        v,
        SLOT_SYMBOL_CELL,
        SLOT_SYMBOL_CELL,
    )
}

/// Decode the **bonus reel numeral** `number` (`1..=10`) - the big coloured
/// digit a bonus round swaps the reels onto - at its retail cell and its own
/// CLUT.
///
/// These are not a scaled-up coin font and not drawn glyphs: they are ten 64x64
/// cells of their own artwork on art-pack page [`SLOT_BONUS_PAGE`], which is
/// exactly the page the reel renderer switches to when a strip value clears
/// `0x10`. `FUN_801d0fa8` reads the strip value `v`, and *the same three lines*
/// that serve the symbol strip serve this one - only rebased:
///
/// ```text
/// texpage = 0x0C + (v >= 0x10)      // 0x0D = the (832, 0) page
/// clut    = (v >= 0x10 ? 0x7AC0 : 0x7A80) + (v & 0xF)
/// U, V    = (v & 3) * 0x40, (v & 0xC) * 0x10
/// ```
///
/// The per-numeral CLUT column is load-bearing exactly as it is for the symbols:
/// the ten cells are drawn once and **recoloured per numeral** (that is why every
/// digit on the retail bonus reels is a different colour), so decoding page 1
/// through a single palette gives ten same-coloured digits.
pub fn slot_bonus_number(art: &[Tim], number: usize) -> Result<Sprite> {
    if number == 0 || number > SLOT_BONUS_NUMBER_COUNT {
        bail!("bonus number {number} out of range (1..={SLOT_BONUS_NUMBER_COUNT})");
    }
    let tim = art
        .get(SLOT_BONUS_PAGE)
        .context("art pack has no bonus-numeral page")?;
    // The strip value that carries this numeral: `number + 0xF` (0x10..=0x19).
    let value = number + crate::slot_payout::BONUS_VALUE_BIAS as usize;
    let page = decode_rgba8(
        tim,
        ClutId(SLOT_BONUS_CLUT_BASE + (value & 0xF) as u16).palette_index(),
    )?;
    let (u, v) = reel_cell(value);
    crop(
        &page,
        tim.pixel_width(),
        u,
        v,
        SLOT_SYMBOL_CELL,
        SLOT_SYMBOL_CELL,
    )
}

/// Decode the coin readout's font strip: the `"COIN"` label followed by digits
/// `0..=9`, as one `224 x 16` sprite (`FUN_801d2914`'s row, CLUT `0x7A8D`).
///
/// Digit `d` occupies `x = SLOT_DIGIT_U0 + d * SLOT_DIGIT_W`, and the 64px
/// `"COIN"` label sits at `x = 0`.
pub fn slot_digit_strip(art: &[Tim]) -> Result<Sprite> {
    let tim = art
        .get(SLOT_SYMBOL_PAGE)
        .context("art pack has no symbol page")?;
    let page = decode_rgba8(tim, ClutId(SLOT_DIGIT_CLUT).palette_index())?;
    let w = SLOT_DIGIT_U0 + SLOT_SYMBOL_COUNT * SLOT_DIGIT_W;
    crop(&page, tim.pixel_width(), 0, SLOT_DIGIT_V, w, SLOT_DIGIT_H)
}

/// Decode one HUD widget (cabinet panel / "COIN" label / cursor) through its own
/// descriptor - the record's texpage selects the pack page, its CLUT id the
/// palette, and its `u/v/w/h` the cell.
pub fn slot_hud_sprite(art: &[Tim], widget: &SlotHudWidget) -> Result<Sprite> {
    let tim = art
        .iter()
        .find(|t| t.image.fb_x == widget.texpage.x() && t.image.fb_y == widget.texpage.y())
        .with_context(|| {
            format!(
                "no art page at texpage ({}, {})",
                widget.texpage.x(),
                widget.texpage.y()
            )
        })?;
    let page = decode_rgba8(tim, widget.clut.palette_index())?;
    crop(
        &page,
        tim.pixel_width(),
        widget.u as usize,
        widget.v as usize,
        widget.w as usize,
        widget.h as usize,
    )
}

/// Texpage attribute the HUD rasteriser writes for the info panel (`0x8A`). Bit
/// 7 is the GPU's colour-depth field: it selects **8-bit** texels, not 4.
pub const SLOT_PANEL_TEXPAGE: u16 = 0x8A;

/// Decode the machine's **paytable / coin info panel** - HUD record 0, the
/// 127x239 sprite `FUN_801cfff0` draws at screen `(560, 128)` on the right of
/// the machine (the "x30 back" / "x9 back" / "Bonus games" board with the coin
/// readout under it).
///
/// The pack's TIM *header* declares this page 4bpp, but the texpage attribute
/// the rasteriser actually writes ([`SLOT_PANEL_TEXPAGE`]) has the 8-bit colour
/// bit set, so the GPU samples the same block as **8bpp**: the 64-halfword-wide
/// image is 128 texels across, and the CLUT's 256 entries are one 8-bit palette
/// rather than sixteen 4-bit ones. Decoding it as the header claims yields
/// noise, which is why this has its own entry point rather than going through
/// [`slot_page`].
pub fn slot_info_panel(art: &[Tim]) -> Result<Sprite> {
    let tim = art
        .get(SLOT_PANEL_PAGE)
        .context("art pack has no info-panel page")?;
    let clut = tim.clut.as_ref().context("the info panel needs a CLUT")?;
    let pal: Vec<u16> = clut.entries.iter().take(256).copied().collect();
    if pal.len() < 256 {
        bail!("the info panel's CLUT is not a 256-entry 8bpp palette");
    }
    // 8bpp: one byte per texel, `fb_w` halfwords per row.
    let stride = tim.image.fb_w as usize * 2;
    let (w, h) = (SLOT_PANEL_CELL.2 as usize, SLOT_PANEL_CELL.3 as usize);
    let (u0, v0) = (SLOT_PANEL_CELL.0 as usize, SLOT_PANEL_CELL.1 as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        for col in 0..w {
            let idx = tim
                .image
                .data
                .get((v0 + row) * stride + u0 + col)
                .copied()
                .unwrap_or(0) as usize;
            let e = pal[idx];
            let (r, g, b) = (
                ((e & 0x1F) as u32 * 255 / 31) as u8,
                (((e >> 5) & 0x1F) as u32 * 255 / 31) as u8,
                (((e >> 10) & 0x1F) as u32 * 255 / 31) as u8,
            );
            rgba.extend_from_slice(&[r, g, b, if e == 0 { 0 } else { 255 }]);
        }
    }
    Ok(Sprite {
        width: w,
        height: h,
        rgba,
    })
}

/// The **palette indices** of a 4bpp art page - one nibble per texel, row-major.
///
/// The dot-matrix marquee ([`crate::minigame_slot_scene`]) needs the raw index,
/// not a colour: the retail init reads each message rect back out of VRAM and
/// keeps the nibble, and the nibble is what selects the lamp swatch a dot draws.
pub fn slot_page_indices(art: &[Tim], page: usize) -> Result<(Vec<u8>, usize, usize)> {
    let tim = art.get(page).context("art page index out of range")?;
    if tim.mode != PixelMode::Bpp4 {
        bail!("art page {page} is not 4bpp");
    }
    let (w, h) = (tim.pixel_width(), tim.pixel_height());
    let stride = tim.image.fb_w as usize * 2;
    let mut out = Vec::with_capacity(w * h);
    for row in 0..h {
        for col in 0..w {
            let byte = tim
                .image
                .data
                .get(row * stride + col / 2)
                .copied()
                .unwrap_or(0);
            out.push(if col % 2 == 0 { byte & 0x0F } else { byte >> 4 });
        }
    }
    Ok((out, w, h))
}

/// Decode a whole art page through one of its palettes - the escape hatch for
/// art whose on-screen rect this module does not trace to a renderer.
pub fn slot_page(art: &[Tim], page: usize, palette: usize) -> Result<Sprite> {
    let tim = art.get(page).context("art page index out of range")?;
    if tim.mode != PixelMode::Bpp4 {
        bail!("art page {page} is not 4bpp");
    }
    let rgba = decode_rgba8(tim, palette)?;
    Ok(Sprite {
        width: tim.pixel_width(),
        height: tim.pixel_height(),
        rgba,
    })
}

// ---------------------------------------------------------------------------
// Baka Fighter: the roster's names + the ladder that orders them
// ---------------------------------------------------------------------------

/// Bytes of the ASCII fighter name at roster record `+0x00`.
///
/// The Baka Fighter roster record ([`crate::baka_opponents`], 17 records of
/// `0x6C` at `0x801D769C`) opens with a **32-byte NUL-padded ASCII name**. The
/// stat parser starts at `+0x20`, so the names sit in the bytes ahead of it.
pub const BAKA_NAME_LEN: usize = 0x20;

/// The stage counter `DAT_801DC10C` is seeded to **2**, not 0 (`FUN_801CF00C`).
pub const BAKA_FIRST_STAGE: u32 = 2;

/// Roster id of the fighter faced at a given stage: `roster = stage + 3`
/// (every consumer of the stage counter applies this fold).
pub const BAKA_STAGE_TO_ROSTER: u32 = 3;

/// Reaching this stage sets the all-clear flag and wraps the counter to `0`
/// (`FUN_801D0748`'s `0xE` compare).
pub const BAKA_STAGE_CLEAR: u32 = 0xE;

/// Read the 17 fighter names out of the **as-loaded** Baka overlay (PROT 0976).
///
/// Names are the labels the site shows; the stats come from
/// [`crate::baka_opponents::parse`] over the same image.
pub fn baka_roster_names(overlay: &[u8]) -> Result<Vec<String>> {
    use crate::baka_opponents::{
        OPPONENT_COUNT, OPPONENT_RECORD_STRIDE, OPPONENT_TABLE_FILE_OFFSET,
    };
    let end = OPPONENT_TABLE_FILE_OFFSET + OPPONENT_COUNT * OPPONENT_RECORD_STRIDE;
    if overlay.len() < end {
        bail!(
            "baka overlay too small ({}b) for the roster at 0x{:X}",
            overlay.len(),
            OPPONENT_TABLE_FILE_OFFSET
        );
    }
    Ok((0..OPPONENT_COUNT)
        .map(|i| {
            let o = OPPONENT_TABLE_FILE_OFFSET + i * OPPONENT_RECORD_STRIDE;
            let raw = &overlay[o..o + BAKA_NAME_LEN];
            let n = raw.iter().position(|&b| b == 0).unwrap_or(BAKA_NAME_LEN);
            String::from_utf8_lossy(&raw[..n]).trim().to_string()
        })
        .collect())
}

/// The ladder, in the order the arcade cabinet actually serves it.
///
/// The stage counter starts at [`BAKA_FIRST_STAGE`] and runs to
/// [`BAKA_STAGE_CLEAR`], so the *first* twelve fighters are roster ids `5..=16`,
/// and their prize gold is strictly monotonic across them. Only after the
/// all-clear does the counter wrap to `0`, putting roster ids `3` and `4` up as
/// the second-lap opponents - which is the reason a naive read of the roster's
/// gold column looks out of order.
///
/// Returns `(stage, roster_id)` pairs for one full lap plus the two wrap-around
/// rungs.
pub fn baka_ladder() -> Vec<(u32, usize)> {
    let mut out: Vec<(u32, usize)> = (BAKA_FIRST_STAGE..BAKA_STAGE_CLEAR)
        .map(|s| (s, (s + BAKA_STAGE_TO_ROSTER) as usize))
        .collect();
    // The post-clear wrap: stages 0 and 1 are only reachable on the second lap.
    out.push((0, BAKA_STAGE_TO_ROSTER as usize));
    out.push((1, BAKA_STAGE_TO_ROSTER as usize + 1));
    out
}

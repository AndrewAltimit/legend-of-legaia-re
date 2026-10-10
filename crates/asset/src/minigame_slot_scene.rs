//! The casino slot machine's **3D scene** - the geometry the overlay projects
//! through the GTE, and the dot-matrix marquee message bank it scrolls.
//!
//! The machine is not a sprite collage. Every element on screen is a quad in a
//! 3D scene, projected by the GTE and depth-sorted into the ordering table:
//!
//! - the **reels** are textured cylinders (`FUN_801d0fa8`): each visible face is
//!   a `POLY_GT4` whose four corners are `(x, y(a), z(a))` for two adjacent reel
//!   angles, with `y` and `z` read out of the SCUS sine / cosine tables and the
//!   quad's gouraud shade depth-cued off `z`. The four corners go through
//!   `RotTransPers4` (`FUN_8005bac8` = `RTPT` + `RTPS`);
//! - the **paylines** are `LINE_F2` segments whose two endpoints are each
//!   `RTPS`-projected (`FUN_801d3380` -> `FUN_8003d368`, `cop2 0x180001`);
//! - the **payline medallions**, **payline lamps**, **reel-stop pedestals**,
//!   **marquee panel** and **mascots** are billboards: a 3D centre transformed
//!   into view space, four corners built around it at a view-space half-extent,
//!   and the quad perspective-projected (`FUN_801d08e4` -> `FUN_800195a8`);
//! - the **marquee** is a 78 x 13 dot-matrix display: 1014 individually
//!   `RTPS`-projected 2x2 sprites (`FUN_801d0e1c`), each sampling a lamp colour
//!   selected by a byte in the dot buffer.
//!
//! The overlay contains no `cop2` instruction of its own - it reaches the GTE
//! entirely through the SCUS wrappers - which is why a naive "are there GTE ops
//! in the overlay?" sweep reports the machine as 2D. It is not.
//!
//! ## Coordinate system
//!
//! The scene camera is set every frame by `FUN_800172c0` from the rotation
//! angles at `_DAT_8007b790` (which the slot's init clears to zero) and the
//! scale matrix at `_DAT_8007bf10`, which the init writes as
//! `diag(0x6000, 0x3000, 0x3000)` = `diag(6, 3, 3)` in 4.12 fixed point. The
//! rotation being identity is what makes the machine face the camera; the 2:1
//! x:y scale is the 640-wide hi-res video mode's pixel aspect.
//!
//! **`-z` is toward the viewer.** The glass (paylines, medallions, lamps,
//! pedestals, marquee) sits at `z = -768` / `-800`; the reel cylinders are
//! centred on `z = 0` with the symbol on the payline at `z = -512`, i.e. behind
//! the glass. The depth-cue shade peaks at `z = -0x200`, which is exactly that
//! payline face - the rest of the cylinder fades to black, and that fade is what
//! hides the near half of each reel without a backface cull.
//!
//! ## Projection
//!
//! [`project`] is the screen mapping, on the retail 640x240 framebuffer: the
//! GTE's own, with the register file read out of the `minigame_slot_machine`
//! mednafen state (its `GTE` section carries COP2) - rotation
//! `diag(6, 3, 3)`, translation `(-1440, 20, 24480)`, `OFX 320`, `OFY 114`,
//! `H 1024` ([`GTE_OFX`] and siblings). The exporters' perspective-about-a-
//! vanishing-point form ([`PROJ_Z0`] and siblings) is the same mapping
//! rearranged, derived from those registers. An earlier projection fitted to
//! the same frame sat `3.6` rows low and is gone.
//!
//! ## Provenance
//!
//! Table file offsets are into the **raw** slot overlay entry (PROT 0975). The
//! overlay's load base is `0x801C_E818`, so `file = VA - 0x801C_E818`. The four
//! geometry tables tile contiguously from `0x4E68` to `0x4F38`.

use anyhow::{Result, bail};

use crate::minigame_art::ClutId;

mod cabinet;
mod marquee;
mod projection;
mod reel;
mod sprite_cells;
#[cfg(test)]
mod tests;

pub use cabinet::*;
pub use marquee::*;
pub use projection::*;
pub use reel::*;
pub use sprite_cells::*;

/// Runtime load base of the slot overlay (PROT 0975), slot A.
pub const OVERLAY_BASE_VA: u32 = 0x801C_E818;

// ---------------------------------------------------------------------------
// Table offsets (file offsets into the raw PROT 0975 image)
// ---------------------------------------------------------------------------

/// `DAT_801d34f0` - the marquee **message bank**: 21 records of
/// `[u8 u, u8 v, u8 w, u8 h, u32 runtime_ptr]`. `FUN_801CEC94` `StoreImage`s
/// each rect back out of VRAM page 3 and expands its 4bpp nibbles into a
/// byte-per-texel bitmap; `FUN_801d069c` / `FUN_801d3230` blit those bitmaps
/// into the dot buffer.
pub const MESSAGE_TABLE_OFFSET: usize = 0x4CD8;
/// Records in the marquee message bank.
pub const MESSAGE_COUNT: usize = 21;
/// Stride of one message record.
pub const MESSAGE_STRIDE: usize = 8;

/// `DAT_801d3680` - the 5 paylines, `[SVECTOR a, SVECTOR b]` per record
/// (`FUN_801d3380`).
pub const PAYLINE_TABLE_OFFSET: usize = 0x4E68;
/// `DAT_801d36d0` - the 5 left payline medallions, `[SVECTOR pos]` whose `pad`
/// word doubles as the CLUT column (`FUN_801d08e4`).
pub const MEDALLION_TABLE_OFFSET: usize = 0x4EB8;
/// `DAT_801d36f8` - the 5 right payline lamps (`FUN_801d08e4`).
pub const LAMP_TABLE_OFFSET: usize = 0x4EE0;
/// `DAT_801d3720` - the marquee panel + the two mascots (`FUN_801d08e4`).
pub const MARQUEE_TABLE_OFFSET: usize = 0x4F08;

/// Byte stride of one medallion / lamp record: a bare `SVECTOR`, which is how
/// far apart the two tables sit (`MEDALLION_TABLE_OFFSET` to
/// `LAMP_TABLE_OFFSET` to `MARQUEE_TABLE_OFFSET` is `0x28` each, and each holds
/// [`LAMP_COUNT`] records).
pub const LAMP_RECORD_STRIDE: usize = 8;

/// Paylines: 3 horizontal + 2 diagonal.
pub const PAYLINE_COUNT: usize = 5;
/// Payline medallions / lamps: one per payline.
pub const LAMP_COUNT: usize = PAYLINE_COUNT;
/// Marquee billboards: the panel and the two mascots flanking it.
pub const MARQUEE_COUNT: usize = 3;

// ---------------------------------------------------------------------------
// Paylines
// ---------------------------------------------------------------------------

/// Per-reel display-row offset of each payline, relative to the reel's payline
/// row. Read off `FUN_801d13e8`, whose absolute row reads are `+0x11 / +0x10 /
/// +0x0F` (lines 0/1/2, the same offset on all three reels) and then the two
/// diagonals `+0x0F / +0x10 / +0x11` and `+0x11 / +0x10 / +0x0F`. The centre row
/// is `+0x10`, so the offsets below are relative to it:
///
/// | line | reels | on screen |
/// |---|---|---|
/// | 0 | `+1 +1 +1` | top row |
/// | 1 | ` 0  0  0` | middle row (the payline proper) |
/// | 2 | `-1 -1 -1` | bottom row |
/// | 3 | `-1  0 +1` | diagonal, bottom-left to top-right |
/// | 4 | `+1  0 -1` | diagonal, top-left to bottom-right |
///
/// The line index is also the medallion / lamp index: line 3's medallion is the
/// one at `y = +336` and line 4's at `y = -336`, which is exactly where the two
/// diagonal segments in the payline table terminate.
pub const PAYLINE_ROW_OFFSETS: [[i32; 3]; PAYLINE_COUNT] =
    [[1, 1, 1], [0, 0, 0], [-1, -1, -1], [-1, 0, 1], [1, 0, -1]];

/// Row bias of a reel's payline row: retail reads the display strip at
/// `((pos >> 8) + 0x10) % 0x14` (`FUN_801d0554`, `FUN_801d13e8`).
pub const PAYLINE_CENTRE_ROW_BIAS: i32 = 0x10;

// ---------------------------------------------------------------------------
// Landing-line table (FUN_801d2440)
// ---------------------------------------------------------------------------

/// `DAT_801d3630` - which payline a forced stop lands its target on: five rows
/// of four `u32` (three reels plus an unread fourth word), selected by the
/// per-spin `rand % 5` at `DAT_801d4134` (`FUN_801d258c`, `0x801D25A0..
/// 0x801D25D0`). `FUN_801d2440` reads `table[jitter * 16 + reel * 4]`
/// (`0x801D2444..0x801D2464`) and, on a hit at raw strip row `R`, returns the
/// stop position `(R + 3 + word) % 20` (`0x801D24F4..0x801D2520`). The display
/// reads the payline centre at `stop + 0x10`, so the target lands at centre
/// offset `-(19 + word) mod 20` - word `0` puts it on the top row, `21` on the
/// middle, `22` on the bottom ([`landing_offset_for_word`]).
pub const LANDING_LINE_TABLE_OFFSET: usize = 0x4E18;
/// Rows of the landing-line table: the `rand % 5` domain.
pub const LANDING_LINE_ROWS: usize = 5;
/// Byte stride of one landing-line row (`sll v0,v0,0x4` at `0x801D2458`).
pub const LANDING_LINE_STRIDE: usize = 16;

/// The payline ([`PAYLINE_ROW_OFFSETS`] index) each landing-line row puts a
/// forced target on, decoded from the retail table: rows 0..2 are the three
/// horizontal lines and rows 3 / 4 the two diagonals (line 4, then line 3).
/// Pinned against the disc by `slot_landing_lines_match_the_retail_table`.
pub const LANDING_LINE_BY_JITTER: [usize; LANDING_LINE_ROWS] = [0, 1, 2, 4, 3];

/// The centre-row offset a landing-line word places the target at.
pub fn landing_offset_for_word(word: u32) -> i32 {
    let off = (-(19 + word as i64)).rem_euclid(20) as i32;
    if off > 10 { off - 20 } else { off }
}

/// Decode the landing-line table into payline indices, one per jitter value,
/// or `None` when a row's three offsets are not one of the five paylines.
pub fn parse_landing_lines(overlay: &[u8]) -> Option<[usize; LANDING_LINE_ROWS]> {
    let mut out = [0usize; LANDING_LINE_ROWS];
    for (row, slot) in out.iter_mut().enumerate() {
        let base = LANDING_LINE_TABLE_OFFSET + row * LANDING_LINE_STRIDE;
        let mut offs = [0i32; 3];
        for (reel, o) in offs.iter_mut().enumerate() {
            let at = base + reel * 4;
            let w = u32::from_le_bytes(overlay.get(at..at + 4)?.try_into().ok()?);
            *o = landing_offset_for_word(w);
        }
        *slot = PAYLINE_ROW_OFFSETS.iter().position(|l| *l == offs)?;
    }
    Some(out)
}

/// `0x801d35a8` - seven sixteen-byte NUL-padded developer labels for the
/// machine's feature states (flag and big-game names) between the payout table
/// and the landing-line table. No instruction in any image forms an address in
/// them, in any of the forms the two address scans cover; they are dead data.
pub const DEV_LABELS_OFFSET: usize = 0x4D90;
/// Labels in the dead developer-label run.
pub const DEV_LABEL_COUNT: usize = 7;
/// Byte stride of one developer label.
pub const DEV_LABEL_STRIDE: usize = 16;

// ---------------------------------------------------------------------------
// Parsed scene
// ---------------------------------------------------------------------------

/// A model-space point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos3 {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

/// One payline: a 3D segment whose two endpoints retail `RTPS`-projects.
#[derive(Debug, Clone, Copy)]
pub struct PayLine {
    pub a: Pos3,
    pub b: Pos3,
}

/// One payline medallion / lamp billboard.
#[derive(Debug, Clone, Copy)]
pub struct LampBillboard {
    pub pos: Pos3,
    /// The record's `pad` word. For the left medallions it is the CLUT column
    /// (`0x7A80 + art`); the right lamps carry the same value, but their
    /// renderer ignores it and uses the fixed [`LAMP_CLUT`].
    pub art: i16,
}

/// The marquee panel / mascot billboards.
#[derive(Debug, Clone, Copy)]
pub struct MarqueeBillboard {
    pub pos: Pos3,
    /// CLUT column offset (`0x7B00 + clut_off`).
    pub clut_off: i16,
    /// View-space half-width.
    pub half_w: i16,
    /// View-space half-height.
    pub half_h: i16,
    /// Texture cell on page 2.
    pub u: u8,
    pub v: u8,
    pub w: u8,
    pub h: u8,
}

/// The whole scene, as parsed off the disc.
#[derive(Debug, Clone)]
pub struct SlotScene {
    pub paylines: Vec<PayLine>,
    pub medallions: Vec<LampBillboard>,
    pub lamps: Vec<LampBillboard>,
    pub marquee: Vec<MarqueeBillboard>,
    pub messages: Vec<MarqueeMessage>,
}

fn rd_i16(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([b[o], b[o + 1]])
}

fn rd_pos(b: &[u8], o: usize) -> Pos3 {
    Pos3 {
        x: rd_i16(b, o),
        y: rd_i16(b, o + 2),
        z: rd_i16(b, o + 4),
    }
}

/// Parse **only** the five payline segments out of the raw PROT 0975 image.
///
/// The payline geometry table is the one part of the scene graph that needs
/// no art: [`parse_scene`] demands the decoded banner page for the marquee
/// message bank, which a caller that wants nothing but the paylines would
/// otherwise have to build. Split out so a host can take the segments alone.
pub fn parse_paylines(overlay: &[u8]) -> Result<Vec<PayLine>> {
    let need = PAYLINE_TABLE_OFFSET + PAYLINE_COUNT * 16;
    if overlay.len() < need {
        bail!(
            "slot overlay too small ({}b) for the payline table at 0x{:X}",
            overlay.len(),
            PAYLINE_TABLE_OFFSET
        );
    }
    Ok((0..PAYLINE_COUNT)
        .map(|i| {
            let o = PAYLINE_TABLE_OFFSET + i * 16;
            PayLine {
                a: rd_pos(overlay, o),
                b: rd_pos(overlay, o + 8),
            }
        })
        .collect())
}

/// What the machine's screen shows this frame, for a host's draw pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotScreen {
    /// The machine alone.
    Machine,
    /// The cash-out picker over the machine (state `0x32`), cursor on `row`.
    Picker { row: u32 },
    /// An instruction page (320-wide mode): `0` = the attract text
    /// (state `0x35`), `1` = the symbol payout chart (states `0x36` / `0x38`).
    Instructions { page: u8 },
    /// The not-enough-coins prompt over the machine (state `0x5A`).
    NoCoins,
}

/// Runtime VA of the attract-text pointer table `FUN_801D30F8` walks: 14
/// pointers into the string block at the head of the overlay.
pub const RULES_LINE_TABLE_VA: u32 = 0x801D_34B8;
/// Lines in [`RULES_LINE_TABLE_VA`] (`sltiu v0,s2,0xe` at `0x801D3154`).
pub const RULES_LINE_COUNT: usize = 14;
/// The first rules page's footer string (`0x801CFB7C` loads it for state
/// `0x35`).
pub const RULES_NEXT_VA: u32 = 0x801C_EAAC;
/// The second rules page's footer string (state `0x36` / `0x38`).
pub const RULES_END_VA: u32 = 0x801C_EAB4;
/// Runtime VA of the payout chart's symbol order `FUN_801D2AA4` reads: ten
/// bytes, five rows per column.
pub const RULES_CHART_VA: u32 = 0x801D_3784;

/// The text and chart order of the machine's two rules pages - the strings
/// are the overlay's own, read at runtime (never shipped).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotRules {
    /// The fourteen attract-text lines, NUL stripped (MES bytes; a `0xCE`
    /// escape is a button glyph).
    pub lines: Vec<Vec<u8>>,
    /// Page 0's footer.
    pub next: Vec<u8>,
    /// Page 1's footer.
    pub end: Vec<u8>,
    /// The payout chart's symbol ids, column-major: `chart[col * 5 + row]`.
    pub chart: [u8; 10],
}

/// Read [`SlotRules`] out of the **raw** PROT 0975 image.
pub fn parse_rules(overlay: &[u8]) -> Result<SlotRules> {
    let off = |va: u32| va.checked_sub(OVERLAY_BASE_VA).map(|o| o as usize);
    let cstr = |va: u32| -> Result<Vec<u8>> {
        let o = off(va).filter(|&o| o < overlay.len());
        let Some(o) = o else {
            bail!("rules string 0x{va:08X} outside the overlay");
        };
        // A `0xCE` escape's argument byte may be zero; keep it with its escape.
        let mut out = Vec::new();
        let mut i = o;
        while i < overlay.len() {
            let b = overlay[i];
            if b == 0xCE && i + 1 < overlay.len() {
                out.extend_from_slice(&overlay[i..i + 2]);
                i += 2;
                continue;
            }
            if b == 0 {
                break;
            }
            out.push(b);
            i += 1;
        }
        Ok(out)
    };
    let table = off(RULES_LINE_TABLE_VA).unwrap_or(usize::MAX);
    if table.saturating_add(RULES_LINE_COUNT * 4) > overlay.len() {
        bail!("slot overlay too small for the rules line table");
    }
    let lines = (0..RULES_LINE_COUNT)
        .map(|i| {
            let o = table + i * 4;
            cstr(u32::from_le_bytes(overlay[o..o + 4].try_into().unwrap()))
        })
        .collect::<Result<Vec<_>>>()?;
    let c = off(RULES_CHART_VA).unwrap_or(usize::MAX);
    if c.saturating_add(10) > overlay.len() {
        bail!("slot overlay too small for the rules chart");
    }
    let mut chart = [0u8; 10];
    chart.copy_from_slice(&overlay[c..c + 10]);
    Ok(SlotRules {
        lines,
        next: cstr(RULES_NEXT_VA)?,
        end: cstr(RULES_END_VA)?,
        chart,
    })
}

/// Parse the slot machine's 3D scene graph out of the **raw** PROT 0975 image.
///
/// `page3` is the decoded banner page of the art pack (pack index 3, fb
/// `(832, 256)`) as one palette *index* per texel, row-major, `page3_w` wide -
/// the message bank's bitmaps are cut out of it.
pub fn parse_scene(overlay: &[u8], page3: &[u8], page3_w: usize) -> Result<SlotScene> {
    let need = MARQUEE_TABLE_OFFSET + MARQUEE_COUNT * 16;
    if overlay.len() < need {
        bail!(
            "slot overlay too small ({}b) for the scene tables at 0x{:X}",
            overlay.len(),
            PAYLINE_TABLE_OFFSET
        );
    }

    let paylines = parse_paylines(overlay)?;

    let lamp_at = |base: usize| -> Vec<LampBillboard> {
        (0..LAMP_COUNT)
            .map(|i| {
                let o = base + i * 8;
                LampBillboard {
                    pos: rd_pos(overlay, o),
                    art: rd_i16(overlay, o + 6),
                }
            })
            .collect()
    };
    let medallions = lamp_at(MEDALLION_TABLE_OFFSET);
    let lamps = lamp_at(LAMP_TABLE_OFFSET);

    let marquee = (0..MARQUEE_COUNT)
        .map(|i| {
            let o = MARQUEE_TABLE_OFFSET + i * 16;
            MarqueeBillboard {
                pos: rd_pos(overlay, o),
                clut_off: rd_i16(overlay, o + 6),
                half_w: rd_i16(overlay, o + 8),
                half_h: rd_i16(overlay, o + 10),
                u: overlay[o + 12],
                v: overlay[o + 13],
                w: overlay[o + 14],
                h: overlay[o + 15],
            }
        })
        .collect();

    let messages = parse_messages(overlay, page3, page3_w)?;

    Ok(SlotScene {
        paylines,
        medallions,
        lamps,
        marquee,
        messages,
    })
}

/// The CLUT the medallion whose record carries `art` samples.
pub fn medallion_clut(art: i16) -> ClutId {
    ClutId(MEDALLION_CLUT_BASE.wrapping_add(art as u16))
}

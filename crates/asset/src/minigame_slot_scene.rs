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
//! [`project`] is the screen mapping, on the retail 640x240 framebuffer. Its
//! **shape** is derived (a perspective divide of a view-space point whose x:y
//! scale ratio is exactly 2, read out of the camera matrix); its four
//! **scalars** are *fitted* to a retail framebuffer captured at the machine
//! (the `minigame_slot_machine` capture), because the GTE control words
//! (`OFX` / `OFY` / `H`) do not live in main RAM and so are not in the save
//! state.
//!
//! The fit is over-determined and independently checked: it was solved on the
//! five payline lamps alone, and then *predicted* - to about a pixel each - the
//! on-screen rect of every other element, none of which entered the fit: the
//! medallion column, the marquee panel, the two mascots, the three reel windows,
//! the reel-stop pedestals, and the dot-matrix grid.
//!
//! ## Provenance
//!
//! Table file offsets are into the **raw** slot overlay entry (PROT 0975). The
//! overlay's load base is `0x801C_E818`, so `file = VA - 0x801C_E818`. The four
//! geometry tables tile contiguously from `0x4E68` to `0x4F38`.

use anyhow::{Result, bail};

use crate::minigame_art::ClutId;

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
// Reel cylinder (FUN_801d0fa8)
// ---------------------------------------------------------------------------

/// Model-space `x` of reel 0's left edge (`FUN_801cf0d8`: `iVar13 = -0x200`).
pub const REEL_X0: i32 = -0x200;
/// Model-space `x` step between reels (`iVar13 += 0x180`).
pub const REEL_X_STEP: i32 = 0x180;
/// Model-space width of one reel (each face spans `x .. x + 0x100`).
pub const REEL_WIDTH: i32 = 0x100;
/// Reels on the machine.
pub const REEL_COUNT: usize = 3;
/// Symbols on one reel strip.
pub const STRIP_LEN: i32 = 0x14;

/// Base angle of the first emitted reel face (`param_2 + 0x380 + frac`).
pub const REEL_ANGLE_BASE: i32 = 0x380;
/// Angle subtended by one symbol (`0x1000` = a full turn).
pub const REEL_ANGLE_STEP: i32 = 0x100;
/// A full turn, in the 12-bit angle the SCUS trig tables are indexed by.
pub const ANGLE_FULL: i32 = 0x1000;
/// Reel faces emitted per reel, per frame.
pub const REEL_FACES: usize = 8;

/// The reel cylinder's `y` radius: `y(a) = (sin(a) * -0x249) >> 12`.
pub const REEL_Y_RADIUS: i32 = 0x249;
/// The reel cylinder's `z` radius shift: `z(a) = cos(a) >> 3`, i.e. radius 512.
pub const REEL_Z_SHIFT: u32 = 3;

/// Peak gouraud shade of a reel face (`0xB4`). The retail blend is
/// `texel * shade / 128`, so `0xB4` is a 1.41x brighten, not a clamp at 1.0.
pub const REEL_SHADE_MAX: i32 = 0xB4;
/// Depth-cue bias: the shade peaks at `z = -REEL_SHADE_Z_BIAS`.
pub const REEL_SHADE_Z_BIAS: i32 = 0x200;
/// Depth-cue gain (`(z + bias) * 0x21C >> 9`).
pub const REEL_SHADE_Z_GAIN: i32 = 0x21C;
/// The blend divisor of a `POLY_GT4`'s vertex colour.
pub const SHADE_NEUTRAL: i32 = 128;

/// SCUS virtual address of the 4096-entry **sine** table (4.12 fixed, amplitude
/// `0x1000`) the reel renderer reaches through the pointer `_DAT_8007b81c`.
pub const SIN_TABLE_VA: u32 = 0x8007_0A2C;
/// SCUS virtual address of the matching **cosine** table (`_DAT_8007b7f8`).
pub const COS_TABLE_VA: u32 = 0x8007_122C;

/// `sin(angle)` in 4.12 fixed point, amplitude `0x1000` - the SCUS table at
/// [`SIN_TABLE_VA`], reproduced (its entries are `round(0x1000 * sin)`).
// REF: FUN_801d0fa8 (the reel renderer's trig-table reads)
pub fn sin_4096(angle: i32) -> i32 {
    let a = angle.rem_euclid(ANGLE_FULL) as f64 * (core::f64::consts::TAU / ANGLE_FULL as f64);
    (4096.0 * a.sin()).round() as i32
}

/// `cos(angle)` in 4.12 fixed point - the SCUS table at [`COS_TABLE_VA`].
pub fn cos_4096(angle: i32) -> i32 {
    let a = angle.rem_euclid(ANGLE_FULL) as f64 * (core::f64::consts::TAU / ANGLE_FULL as f64);
    (4096.0 * a.cos()).round() as i32
}

/// Model-space `y` of the reel cylinder at `angle` (`FUN_801d0fa8`).
// PORT: FUN_801d0fa8 (reel cylinder y: sin(a) * -0x249 >> 12)
pub fn reel_y(angle: i32) -> i32 {
    let v = sin_4096(angle) * -REEL_Y_RADIUS;
    // Retail biases negatives before the arithmetic shift (round toward zero).
    if v < 0 { (v + 0xFFF) >> 12 } else { v >> 12 }
}

/// Model-space `z` of the reel cylinder at `angle` (`FUN_801d0fa8`).
// PORT: FUN_801d0fa8 (reel cylinder z: cos(a) >> 3)
pub fn reel_z(angle: i32) -> i32 {
    let v = cos_4096(angle);
    if v < 0 {
        (v + 7) >> REEL_Z_SHIFT
    } else {
        v >> REEL_Z_SHIFT
    }
}

/// The depth-cued gouraud shade of a reel vertex at model-space `z`, clamped to
/// `0 ..= 0xB4` (`FUN_801d0fa8`). Feed it to a `texel * shade / 128` blend.
// PORT: FUN_801d0fa8 (reel depth-cue shade)
pub fn reel_shade(z: i32) -> i32 {
    let v = (z + REEL_SHADE_Z_BIAS) * REEL_SHADE_Z_GAIN;
    let v = if v < 0 { (v + 0x1FF) >> 9 } else { v >> 9 };
    (REEL_SHADE_MAX - v).clamp(0, REEL_SHADE_MAX)
}

/// Model-space left edge of reel `r`.
pub fn reel_x(r: usize) -> i32 {
    REEL_X0 + (r as i32) * REEL_X_STEP
}

// ---------------------------------------------------------------------------
// Sprite cells (the rects FUN_801d08e4 writes into its prims)
// ---------------------------------------------------------------------------

/// Art-pack page of the payline medallions (texpage `0x0C`).
pub const MEDALLION_PAGE: usize = 0;
/// Medallion cell `(u, v, w, h)`: `FUN_801d08e4`'s second loop draws 32x32 at
/// `uv (0xA8, 0x80)` with CLUT `0x7A80 + id`, `id` being the record's own `pad`
/// field - so the five medallions are one cell of artwork recoloured.
pub const MEDALLION_CELL: (u8, u8, u8, u8) = (0xA8, 0x80, 32, 32);
/// CLUT base of the medallions.
pub const MEDALLION_CLUT_BASE: u16 = 0x7A80;
/// View-space half-extent of a medallion billboard (`0x1A0 x 0xD0`).
pub const MEDALLION_HALF: (i32, i32) = (0x1A0, 0xD0);

/// Art-pack page of the payline lamps (texpage `0x1C`).
pub const LAMP_PAGE: usize = 2;
/// Unlit lamp cell.
pub const LAMP_CELL_UNLIT: (u8, u8, u8, u8) = (0x10, 0xE0, 16, 16);
/// Lit lamp cell (the winning payline's).
pub const LAMP_CELL_LIT: (u8, u8, u8, u8) = (0x00, 0xE0, 16, 16);
/// The lamps' fixed CLUT.
pub const LAMP_CLUT: u16 = 0x7B09;
/// View-space half-extent of a lamp billboard (`0xB4 x 0xA0`).
pub const LAMP_HALF: (i32, i32) = (0xB4, 0xA0);

/// Art-pack page of the reel-stop pedestals.
pub const PEDESTAL_PAGE: usize = 2;
/// Pedestal cell size (32x32).
pub const PEDESTAL_SIZE: u8 = 32;
/// `V` of pedestal `r`'s cell: `PEDESTAL_V0 + r * PEDESTAL_V_STEP`. Both the
/// spinning and the stopped cell sit on this row - the stop branch of
/// `FUN_801d08e4` overrides **only the `U`s**, which is exactly the trap: the
/// pedestals stay on their own row and slide left to the "taken" column.
pub const PEDESTAL_V0: u8 = 0x80;
/// Row step of the pedestal cell, per reel.
pub const PEDESTAL_V_STEP: u8 = 0x20;
/// `U` of the pedestal cell while the reel spins.
pub const PEDESTAL_U_SPINNING: u8 = 0x60;
/// `U` of the pedestal cell once the reel is stopped.
pub const PEDESTAL_U_STOPPED: u8 = 0x00;
/// CLUT base while spinning (`+ reel`).
pub const PEDESTAL_CLUT_SPINNING: u16 = 0x7B03;
/// CLUT base once stopped (`+ reel`).
pub const PEDESTAL_CLUT_STOPPED: u16 = 0x7B06;

/// The cell pedestal `reel` draws, as `(u, v, w, h)`.
pub fn pedestal_cell(reel: usize, stopped: bool) -> (u8, u8, u8, u8) {
    let u = if stopped {
        PEDESTAL_U_STOPPED
    } else {
        PEDESTAL_U_SPINNING
    };
    (
        u,
        PEDESTAL_V0 + reel as u8 * PEDESTAL_V_STEP,
        PEDESTAL_SIZE,
        PEDESTAL_SIZE,
    )
}
/// View-space half-extent of a pedestal billboard (`0x230 x 0x120`).
pub const PEDESTAL_HALF: (i32, i32) = (0x230, 0x120);
/// Model-space `x` of pedestal 0.
pub const PEDESTAL_X0: i32 = -0x180;
/// Model-space `x` step between pedestals.
pub const PEDESTAL_X_STEP: i32 = 0x180;
/// Model-space `y` of the pedestal row.
pub const PEDESTAL_Y: i32 = 0x1E0;
/// The plane all the machine's glass furniture sits on.
pub const GLASS_Z: i32 = -800;

/// Art-pack page of the marquee panel + mascots.
pub const MARQUEE_PAGE: usize = 2;
/// Marquee CLUT base (`0x7B00 + record.clut_off`).
pub const MARQUEE_CLUT_BASE: u16 = 0x7B00;

// ---------------------------------------------------------------------------
// Dot-matrix marquee (FUN_801d0e1c)
// ---------------------------------------------------------------------------

/// Dot-matrix columns.
pub const DOT_COLS: usize = 0x4E;
/// Dot-matrix rows.
pub const DOT_ROWS: usize = 0x0D;
/// Model-space `x` of dot column 0.
pub const DOT_X0: i32 = -0x1AD;
/// Model-space `y` of dot row 0.
pub const DOT_Y0: i32 = -0x280;
/// Model-space `x` step between dot columns.
pub const DOT_X_STEP: i32 = 0x0B;
/// Model-space `y` step between dot rows.
pub const DOT_Y_STEP: i32 = 0x0C;
/// The dot plane.
pub const DOT_Z: i32 = -800;
/// The dots sample page 3 (texpage `0x1D`, set by the trailing `DR_TPAGE`).
pub const DOT_PAGE: usize = 3;
/// Each dot is a 2x2 texel sprite.
pub const DOT_SIZE: u32 = 2;
/// CLUT the dots sample (`0x7B4F` - row 493, column 15).
///
/// That column is **empty on the disc** and written every frame: the reel SM
/// `MoveImage`s a 16x1 rect from `((tick & 1) * 16, 493)` to column 15, so the
/// dots' palette alternates between page 3's CLUT columns
/// [`DOT_BLINK_PALETTES`] - the marquee's blink. Decoding column 15 straight off
/// the disc gives a fully transparent, invisible marquee.
pub const DOT_CLUT: u16 = 0x7B4F;
/// The two page-3 CLUT columns the marquee alternates between (the source of the
/// per-frame copy into column 15). Pick with `tick & 1`.
pub const DOT_BLINK_PALETTES: [usize; 2] = [0, 1];
/// The dot buffer's stride: `DAT_801d37a0[col * 0x10 + row]`.
pub const DOT_STRIDE: usize = 0x10;
/// The dot buffer holds a texel `u`, and the init writes `nibble << 2`, so a
/// nibble `n` selects the lamp swatch at `u = n * 4` on page 3's top row.
pub const DOT_U_PER_NIBBLE: u32 = 4;

// ---------------------------------------------------------------------------
// The marquee's message bank - what the 21 records ARE (FUN_801cfff0)
// ---------------------------------------------------------------------------
//
// The bank is not 21 anonymous bitmaps. `FUN_801cfff0` composes the marquee out
// of it every frame, and the ids it indexes give every record its role. The
// arithmetic is the pin: the payout caption prints its digits with
// `FUN_801d3230(n / 1000 + 6)`, `(n % 1000) / 100 + 6`, `/ 10 + 6`, `% 10 + 6`,
// so records `6..=15` are the glyphs `"0".."9"` - and record `16` is the extra
// one-record glyph **"10"**, because the bonus tally indexes `claimed - 0x10 + 6`
// over a `claimed` of `0x10..=0x1A` (see [`crate::slot_payout`]).
//
// The bank decodes off the disc as exactly that: 6..=16 are the eleven numerals
// `"0"`..`"10"`, 0x11 is the multiplication sign, 0x12 / 0x13 the filled /
// hollow round pips, and 0x14 the word "coin".

/// First numeral record of the message bank. Record `MSG_NUMBER_BASE + n` is the
/// glyph for `n`, for `n` in `0..=10` - eleven records, because **"10"** gets a
/// glyph of its own (a bonus reel can land on 10).
pub const MSG_NUMBER_BASE: usize = 6;
/// The largest numeral the bank has a glyph for (`MSG_NUMBER_BASE + 10`).
pub const MSG_NUMBER_MAX: usize = 10;
/// The multiplication sign the bonus tally separates its columns with.
pub const MSG_TIMES: usize = 0x11;
/// A bonus round still owed (filled pip).
pub const MSG_ROUND_PIP_ON: usize = 0x12;
/// A bonus round already played (hollow pip).
pub const MSG_ROUND_PIP_OFF: usize = 0x13;
/// The word "coin", printed after the payout figure.
pub const MSG_COINS: usize = 0x14;

/// Dot columns the **bonus tally**'s three numerals are blitted at - one per
/// reel (`FUN_801cfff0`: `FUN_801d3230(msg, reel << 5, 0)`).
///
/// The tally is the strip across the top of the machine that reads `0 x 0 x 0`
/// at the start of a bonus round and fills in each column's landed number as
/// that reel's stop is claimed. It is drawn *only* in feature modes 4..=6 and
/// only in the reel states (3 = stopping, 4 = payout); in the normal game the
/// same dot matrix scrolls the attract legend instead.
pub const TALLY_NUMBER_COLS: [usize; 3] = [0x00, 0x20, 0x40];
/// Dot columns the two [`MSG_TIMES`] signs sit at, between the numerals.
pub const TALLY_TIMES_COLS: [usize; 2] = [0x10, 0x30];
/// Dot columns the **payout caption**'s four digits are blitted at (thousands →
/// units; a digit is only drawn once the figure reaches its place).
pub const PAYOUT_DIGIT_COLS: [usize; 4] = [0x00, 0x0D, 0x1A, 0x27];
/// Dot column the [`MSG_COINS`] word follows the payout figure at. The word runs
/// past the matrix's 78 columns, so its tail clips - as it does in retail.
pub const PAYOUT_COINS_COL: usize = 0x34;
/// The payout caption slides down into place over its first frames: it is drawn
/// at dot row `min(frame - 0xD, 0)` (`FUN_801cfff0`), i.e. from 13 rows above the
/// matrix down to row 0.
pub const PAYOUT_SLIDE_ROWS: i32 = 0x0D;
/// Dot columns the three bonus-round pips sit at while a round is idle / spinning
/// up (states 1-2) - the tally strip's other face, counting the rounds still owed.
pub const ROUND_PIP_COLS: [usize; 3] = [0x00, 0x20, 0x40];

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Width of the retail framebuffer the machine draws into: `FUN_801CEC94` sets
/// video mode `0x280` = 640 wide. Horizontal pixels are half-width, which is why
/// [`PROJ_ASPECT`] is 2.
pub const SCREEN_W: f32 = 640.0;
/// Height of the retail framebuffer.
pub const SCREEN_H: f32 = 240.0;

/// Screen x of the machine's model-space origin. **Fitted.** The machine sits
/// left of centre to clear the coin panel the HUD rasteriser draws at x 560.
pub const PROJ_OFX: f32 = 253.0;
/// Screen y of the machine's model-space origin. **Fitted.**
pub const PROJ_OFY: f32 = 118.5;
/// View-space depth offset. **Fitted** from the ratio of the on-screen scale at
/// `z = -800` (the glass) to that at `z = -512` (the reel's payline face).
pub const PROJ_Z0: f32 = 9324.0;
/// Screen x scale at `z = 0`. **Fitted.**
pub const PROJ_SX0: f32 = 0.2547;
/// x:y scale ratio. **Derived**, not fitted: the camera matrix the init writes
/// to `_DAT_8007bf10` is `diag(6, 3, 3)`.
pub const PROJ_ASPECT: f32 = 2.0;
/// The camera matrix's x scale. A billboard's view-space half-extent divides by
/// this to reach screen pixels: `FUN_800195a8` builds the corners *after* the
/// matrix multiply, so they carry no model scale.
pub const PROJ_X_SCALE: f32 = 6.0;

/// Screen x-scale at model-space depth `z` (`-z` is toward the viewer).
pub fn view_scale(z: i32) -> f32 {
    PROJ_SX0 * PROJ_Z0 / (PROJ_Z0 + z as f32)
}

/// Project a model-space point onto the retail 640x240 framebuffer.
pub fn project(x: i32, y: i32, z: i32) -> (f32, f32) {
    let s = view_scale(z);
    (
        PROJ_OFX + s * x as f32,
        PROJ_OFY + (s / PROJ_ASPECT) * y as f32,
    )
}

/// Screen half-extent of a billboard whose view-space half-extent is `(hw, hh)`
/// and whose centre is at depth `z`. Both axes divide by the same `H / vz`, so a
/// billboard is *not* aspect-corrected: a 2:1 view-space extent is a 2:1 screen
/// extent, which on the half-width hi-res pixel grid renders square.
pub fn billboard_half(hw: i32, hh: i32, z: i32) -> (f32, f32) {
    let k = view_scale(z) / PROJ_X_SCALE;
    (hw as f32 * k, hh as f32 * k)
}

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

/// One record of the dot-matrix message bank.
#[derive(Debug, Clone)]
pub struct MarqueeMessage {
    /// 4bpp texel x within page 3 (the rect's VRAM x is `832 + u / 4`).
    pub u: u8,
    /// Texel y within page 3 (the VRAM row is `256 + v`).
    pub v: u8,
    /// Width, in texels.
    pub w: u8,
    /// Height, in texels (every record is 13 - the dot matrix's row count).
    pub h: u8,
    /// `w * h` palette nibbles, row-major - the expanded bitmap the retail init
    /// builds by reading the rect back out of VRAM. A zero nibble is an unlit
    /// dot; a non-zero nibble `n` lights the dot with the swatch at page-3
    /// `u = n * 4, v = 0`.
    pub bitmap: Vec<u8>,
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

    let paylines = (0..PAYLINE_COUNT)
        .map(|i| {
            let o = PAYLINE_TABLE_OFFSET + i * 16;
            PayLine {
                a: rd_pos(overlay, o),
                b: rd_pos(overlay, o + 8),
            }
        })
        .collect();

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

/// Cut the 21 dot-matrix message bitmaps out of the banner page, as
/// `FUN_801CEC94` does at run time (it `StoreImage`s each rect back out of VRAM
/// and expands the nibbles; reading the decoded page directly is the same bytes
/// without the round trip).
// PORT: FUN_801cec94 (the 21-record message-bank StoreImage + nibble expansion)
pub fn parse_messages(overlay: &[u8], page3: &[u8], page3_w: usize) -> Result<Vec<MarqueeMessage>> {
    let end = MESSAGE_TABLE_OFFSET + MESSAGE_COUNT * MESSAGE_STRIDE;
    if overlay.len() < end {
        bail!("slot overlay too small for the marquee message bank");
    }
    Ok((0..MESSAGE_COUNT)
        .map(|i| {
            let o = MESSAGE_TABLE_OFFSET + i * MESSAGE_STRIDE;
            let (u, v, w, h) = (overlay[o], overlay[o + 1], overlay[o + 2], overlay[o + 3]);
            let (uw, uh) = (w as usize, h as usize);
            let mut bitmap = Vec::with_capacity(uw * uh);
            for row in 0..uh {
                for col in 0..uw {
                    let idx = (v as usize + row) * page3_w + u as usize + col;
                    bitmap.push(page3.get(idx).copied().unwrap_or(0));
                }
            }
            MarqueeMessage { u, v, w, h, bitmap }
        })
        .collect())
}

/// The dot buffer `FUN_801d069c` fills: a `DOT_COLS x DOT_ROWS` grid of palette
/// nibbles at the retail stride, `buf[col * DOT_STRIDE + row]`.
///
/// Blits `msg` scrolled to `(x, y)`. A dot whose source column falls outside the
/// message stays unlit, which is what makes the message scroll in and out.
// PORT: FUN_801d069c (marquee dot-buffer composer)
pub fn compose_marquee(msg: &MarqueeMessage, x: i32, y: i32) -> Vec<u8> {
    let mut buf = vec![0u8; DOT_COLS * DOT_STRIDE];
    for row in 0..DOT_ROWS {
        let sy = y + row as i32;
        if sy < 0 || sy >= msg.h as i32 {
            continue;
        }
        for col in 0..DOT_COLS {
            let sx = x + col as i32;
            if sx < 0 || sx >= msg.w as i32 {
                continue;
            }
            buf[col * DOT_STRIDE + row] = msg.bitmap[sy as usize * msg.w as usize + sx as usize];
        }
    }
    buf
}

/// The CLUT the medallion whose record carries `art` samples.
pub fn medallion_clut(art: i16) -> ClutId {
    ClutId(MEDALLION_CLUT_BASE.wrapping_add(art as u16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trig_matches_the_scus_tables() {
        // Spot values read out of the SCUS tables at SIN_TABLE_VA / COS_TABLE_VA.
        assert_eq!(sin_4096(0), 0);
        assert_eq!(sin_4096(1024), 4096);
        assert_eq!(sin_4096(128), 799);
        assert_eq!(cos_4096(0), 4096);
        assert_eq!(cos_4096(1024), 0);
        assert_eq!(cos_4096(128), 4017);
    }

    #[test]
    fn the_payline_face_is_brightest_and_the_near_half_is_black() {
        // Angle 0x800 carries the symbol on the payline: z = -512, peak shade.
        assert_eq!(reel_z(0x800), -512);
        assert_eq!(reel_shade(reel_z(0x800)), REEL_SHADE_MAX);
        // The near half of the cylinder (z >= 0) shades to black, which is what
        // hides it without a backface cull.
        assert_eq!(reel_shade(reel_z(0)), 0);
    }

    #[test]
    fn the_glass_sits_in_front_of_the_reels() {
        // -z is toward the viewer, so the glass (z = -800) is nearer than the
        // reel's payline face (z = -512) and therefore renders larger.
        assert!(view_scale(GLASS_Z) > view_scale(-512));
    }

    #[test]
    fn the_projection_puts_the_reels_where_retail_does() {
        // Reel centres measured off the retail framebuffer: 150 / 253 / 357.
        for (r, want) in [(0usize, 149.5f32), (1, 253.0), (2, 356.5)] {
            let cx = reel_x(r) + REEL_WIDTH / 2;
            let (sx, _) = project(cx, 0, reel_z(0x800));
            assert!(
                (sx - want).abs() < 1.5,
                "reel {r} centre projected to {sx}, retail has it at {want}"
            );
        }
    }
}

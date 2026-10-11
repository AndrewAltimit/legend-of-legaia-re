//! The dot-matrix marquee: its grid geometry, the message bank cut out of
//! the banner page, the tally / payout / round-pip composition and the
//! attract legend that scrolls when no tally is showing.

use super::*;

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

/// Feature mode (`DAT_801d3cac`) of the **bonus round**: the reels carry the
/// numerals, every spin pays a product, and the tally strip is what the dot
/// matrix says. The engine-side twin is
/// `legaia_engine_minigames::slot_machine::FEATURE_MODE_BONUS`.
pub const FEATURE_MODE_BONUS_ROUND: u8 = 6;
/// The feature modes whose reel states put the tally / round-pip strip on the
/// marquee at all (`FUN_801cfff0`'s `4..=6` gate). Outside them the same matrix
/// scrolls the attract legend instead.
pub const MARQUEE_TALLY_MODES: core::ops::RangeInclusive<u8> = 4..=FEATURE_MODE_BONUS_ROUND;

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
//
// The native window owns the dot buffer: `slot_cabinet::SlotMarqueeClock`
// composes it every frame (the attract legend through this scroll, the
// caption / tally through `render_marquee`) and the dot pass rasterises it.
// The browser play page still exports the 21 message bitmaps as JSON and
// blits them in JavaScript.
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

/// Clear the whole dot buffer.
///
/// A **negative** message id is `FUN_801d069c`'s clear command, not a lookup: the
/// `bgez $a0` at its head skips the scroll body entirely and falls into a
/// `DOT_COLS x DOT_ROWS` store loop that zeroes `DAT_801d37a0`. The per-frame
/// composer opens with exactly that call, so the marquee is rebuilt from scratch
/// every frame rather than diffed against the previous one.
// PORT: FUN_801d069c (negative-id clear path)
// The blank buffer every composed frame starts from, natively through
// `slot_cabinet::SlotMarqueeClock`.
pub fn clear_dots() -> Vec<u8> {
    vec![0u8; DOT_COLS * DOT_STRIDE]
}

/// Blit `msg` into `buf` with its top-left at dot cell `(col, row)`.
///
/// This is the **placement** sibling of [`compose_marquee`], and the difference
/// between them is which end of the copy gets clipped. [`compose_marquee`]
/// (`FUN_801d069c`) offsets the *source* and clips it signed, which scrolls one
/// message through a fixed window. `FUN_801d3230` offsets the *destination* and
/// clips it **unsigned** - `sltiu $v0, $t0, 0x4e` on the column and
/// `sltiu $v0, $a2, 0xd` on the row.
///
/// The unsigned compare is how one instruction enforces *both* bounds: a
/// negative offset fails `sltiu` as a huge unsigned value, so retail needs no
/// separate `< 0` test. Negative offsets are not a corner case here - the payout
/// caption starts at `row = -PAYOUT_SLIDE_ROWS` and counts up to `0`, and it is
/// this clip that hides the rows still above the matrix while it slides in. Port
/// the bound as a bare `dr < DOT_ROWS` and every one of those rows lands.
///
/// Retail walks the source at the decoded page's `0x100` stride from a pointer
/// the message record caches at `+4`; [`MarqueeMessage::bitmap`] is that same
/// sub-rect already tightened to `w`, so indexing by `w` reads the same texels -
/// the equivalence [`compose_marquee`] already relies on.
// PORT: FUN_801d3230 (dot-buffer placement blit)
// Reached natively through `render_marquee` from
// `slot_cabinet::SlotMarqueeClock`.
pub fn place_message(buf: &mut [u8], msg: &MarqueeMessage, col: i32, row: i32) {
    for r in 0..msg.h as i32 {
        let dr = row + r;
        if (dr as u32) >= DOT_ROWS as u32 {
            continue;
        }
        for c in 0..msg.w as i32 {
            let dc = col + c;
            if (dc as u32) >= DOT_COLS as u32 {
                continue;
            }
            let src = r as usize * msg.w as usize + c as usize;
            if let Some(&texel) = msg.bitmap.get(src) {
                buf[dc as usize * DOT_STRIDE + dr as usize] = texel;
            }
        }
    }
}

/// The per-frame inputs of the marquee composer - the six overlay globals
/// `FUN_801cfff0` reads before it decides what the dot matrix says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarqueeFrame {
    /// `DAT_801d3d3c`: the coin figure the payout caption prints. `0` = no caption.
    pub payout: i32,
    /// `DAT_801d3c94`: frames since the caption started. `0` = no caption.
    pub payout_frame: i32,
    /// `DAT_801d3cac`: feature mode. The tally / pip strip draws only in `4..=6`.
    pub feature_mode: u8,
    /// `DAT_801d3c84`: the reel state machine's state word.
    pub reel_state: i32,
    /// `DAT_801d3cb0`: bonus rounds still owed.
    pub bonus_rounds: i32,
    /// `DAT_801d3d20[]`: per-reel claimed value (payline value + 1); `0` = unclaimed.
    pub claimed: [i32; 3],
}

/// One message blitted into the dot matrix this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarqueePlacement {
    /// Index into the 21-record message bank.
    pub msg: usize,
    /// Dot column of the blit's left edge.
    pub col: i32,
    /// Dot row of the blit's top edge; negative while a caption slides in.
    pub row: i32,
}

/// Decide what the dot matrix shows this frame.
///
/// The matrix has one occupant at a time and the priority is fixed: while the
/// payout caption is up it owns the whole strip, and only once it is down does
/// the bonus tally / round-pip strip get to draw. Retail gates the caption on
/// **both** the figure and the frame counter being non-zero, and falls straight
/// through to the tally when either is `0`.
///
/// The caption is a right-aligned four-digit figure at [`PAYOUT_DIGIT_COLS`]
/// followed by [`MSG_COINS`]. Its leading-zero suppression tests the **whole
/// figure** at every place rather than the running remainder - all four guards
/// re-read `DAT_801d3d3c` - while the digit *values* come off a remainder chain
/// that runs whether or not its own place was drawn.
// PORT: FUN_801cfff0 (per-frame marquee composition)
// The caption / tally half; the attract-legend tail is [`attract_legend`].
// Composed natively every frame by `slot_cabinet::SlotMarqueeClock`.
pub fn compose_marquee_frame(frame: &MarqueeFrame) -> Vec<MarqueePlacement> {
    let mut out = Vec::new();
    if frame.payout != 0 && frame.payout_frame != 0 {
        // `min(frame - 0xD, 0)`: composed one row per frame from 13 rows above
        // the matrix down to row 0, then held there.
        let row = (frame.payout_frame - PAYOUT_SLIDE_ROWS).min(0);
        let n = frame.payout;
        let digits = [n / 1000, (n % 1000) / 100, (n % 100) / 10, n % 10];
        // Place 3 (the units) is unconditional; the rest need the figure to
        // reach their own decade.
        let thresholds = [1000, 100, 10, 0];
        for ((&digit, &col), &threshold) in digits
            .iter()
            .zip(PAYOUT_DIGIT_COLS.iter())
            .zip(thresholds.iter())
        {
            if n >= threshold {
                out.push(MarqueePlacement {
                    msg: MSG_NUMBER_BASE + digit as usize,
                    col: col as i32,
                    row,
                });
            }
        }
        out.push(MarqueePlacement {
            msg: MSG_COINS,
            col: PAYOUT_COINS_COL as i32,
            row,
        });
        return out;
    }
    if !MARQUEE_TALLY_MODES.contains(&frame.feature_mode) {
        return out;
    }
    match frame.reel_state {
        // States 1-2 (idle / spinning up): the pips count the rounds still owed.
        1 | 2 => {
            for (i, &col) in ROUND_PIP_COLS.iter().enumerate() {
                let lit = frame.bonus_rounds > i as i32;
                out.push(MarqueePlacement {
                    msg: if lit {
                        MSG_ROUND_PIP_ON
                    } else {
                        MSG_ROUND_PIP_OFF
                    },
                    col: col as i32,
                    row: 0,
                });
            }
        }
        // States 3-4 (stopping / payout): the tally reads `n x n x n`.
        3 | 4 => {
            for (reel, &col) in TALLY_NUMBER_COLS.iter().enumerate() {
                let claimed = frame.claimed[reel];
                // An unclaimed reel - and any value below the bonus base - prints
                // a plain "0" rather than being skipped.
                let number = if claimed < 0x10 { 0 } else { claimed - 0x10 };
                out.push(MarqueePlacement {
                    msg: MSG_NUMBER_BASE + number as usize,
                    col: col as i32,
                    row: 0,
                });
            }
            for &col in TALLY_TIMES_COLS.iter() {
                out.push(MarqueePlacement {
                    msg: MSG_TIMES,
                    col: col as i32,
                    row: 0,
                });
            }
        }
        _ => {}
    }
    out
}

/// Rasterise a frame's placements into a dot buffer. A placement whose record is
/// outside the bank is dropped rather than panicking: retail's digit arithmetic
/// is unclamped, so a figure past the bank's range would index off the end of it.
pub fn render_marquee(placements: &[MarqueePlacement], messages: &[MarqueeMessage]) -> Vec<u8> {
    let mut buf = clear_dots();
    for placement in placements {
        if let Some(msg) = messages.get(placement.msg) {
            place_message(&mut buf, msg, placement.col, placement.row);
        }
    }
    buf
}

// ---------------------------------------------------------------------------
// The attract legend - the dot matrix outside the caption and the tally
// ---------------------------------------------------------------------------

/// What the legend counter `DAT_801d3ca0` is reset to the frame the legend's
/// message id changes (`FUN_801d069c`, `0x801D06F8..0x801D0704`: `li v0,0x64`;
/// the same call also scrolls from source column `0`, not from the counter).
pub const LEGEND_COUNTER_RESET: i32 = 0x64;

/// The scrolling legend `FUN_801cfff0` puts on the dot matrix when neither the
/// payout caption nor the tally strip owns it: `(message id, source column)`
/// for one `FUN_801d069c` scroll call, or `None` when that frame's matrix is
/// left clear.
///
/// The tail runs only outside the tally modes (`feature_mode` not in `4..=6`,
/// the `sltiu 3` gate at `0x801D027C`). It first reads the bonus-anticipation
/// latch `DAT_801d3ca4` (`FUN_801d1af4` writes `1` / `2` when an `8` / `9`
/// pair lines up while the reels settle): a raised latch scrolls message `4` /
/// `5` at `counter % 200 - 100` (`0x801D04BC..0x801D0530`). Otherwise the
/// feature mode indexes a 7-entry jump table at `0x801CEC58`: mode `0` places
/// message `0` still at column `0`, modes `1..=3` scroll message `1..=3` at
/// `counter % 168 - 84`, and every other entry is the function's epilogue.
///
/// `counter` is the free-running `DAT_801d3ca0`, which the reel renderer's
/// tail (`0x801CFF00..0x801CFF24`) advances once a frame; a caller also owns
/// the reset to [`LEGEND_COUNTER_RESET`] on a message change.
// PORT: FUN_801cfff0 (the attract-legend tail, 0x801D038C..0x801D0538)
pub fn attract_legend(feature_mode: u8, anticipation: i32, counter: i32) -> Option<(usize, i32)> {
    if MARQUEE_TALLY_MODES.contains(&feature_mode) {
        return None;
    }
    match anticipation {
        1 => return Some((4, counter % 200 - 100)),
        2 => return Some((5, counter % 200 - 100)),
        0 => {}
        _ => return None,
    }
    match feature_mode {
        0 => Some((0, 0)),
        1..=3 => Some((feature_mode as usize, counter % 168 - 84)),
        _ => None,
    }
}

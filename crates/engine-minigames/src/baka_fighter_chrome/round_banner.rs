//! The round banner timeline.

use super::*;

// ---------------------------------------------------------------------------
// Round banner
// ---------------------------------------------------------------------------

/// Frame thresholds of the round banner timeline.
pub const BANNER_SLIDE_IN: i32 = 30;
pub const BANNER_SLIDE_OUT: i32 = 90;
/// Screen x the two banner halves converge on.
pub const BANNER_CENTRE_X: i16 = 0x90;

/// PORT: FUN_801D5C7C - the "ROUND n" banner slide.
///
/// One call per frame with the elapsed frame count `t` and the round index
/// `round` (retail's `DAT_801DBF8C`, 0-based). The banner is two mirrored
/// halves that converge on [`BANNER_CENTRE_X`], hold, then part again:
///
/// - `t == 0` fires the round-announce voice line - clip `XA32`, and the
///   **channel is the round index itself**, duration `0x48`.
/// - `t < 30`: offset `0xB4 - 6t`, level held at `0x80`, halves drawn at
///   half brightness (the "parted" pose, both sprite flags set).
/// - `30 <= t < 90`: offset `0`, level `0x80 + (t - 30) * 127 / 30` (so it
///   reaches `0xFF` at `t = 60`), halves drawn at full level with the flags
///   cleared (the "joined" pose).
/// - `t >= 90`: offset `6 * (t - 90)`, level `0xC8 - (t - 90) * 127 / 30`,
///   back to the parted pose.
///
/// The level clamps to `0..=0xFF` after the ramps. Each pose draws the
/// caption widget `3` and the round digit (glyph cell `round + 1`) twice,
/// mirrored about [`BANNER_CENTRE_X`].
pub fn round_banner_frame(t: i32, round: i32) -> ChromeFrame {
    let mut out = ChromeFrame::default();
    if t == 0 {
        out.xa = Some(round_banner_xa(round));
    }

    let mut level = 0x80;
    let mut offset;
    let mut parted = true;
    if t < BANNER_SLIDE_IN {
        offset = 0xB4 - t * 6;
    } else {
        level = 0x80 + div30((t - BANNER_SLIDE_IN) * 127);
        offset = 0;
        parted = false;
    }
    if t >= BANNER_SLIDE_OUT {
        offset = (t - BANNER_SLIDE_OUT) * 6;
        level = 0xC8 - div30((t - BANNER_SLIDE_OUT) * 127);
        parted = true;
    }
    level = level.clamp(0, 0xFF);

    let x = offset as i16;
    let mirror = BANNER_CENTRE_X - x;
    if parted {
        let lv = half(level);
        out.draws
            .push(ChromeDraw::plain(3, x + BANNER_CENTRE_X, 0x77, lv, 0x1000));
        out.draws
            .push(glyph_draw(x + 0xE8, 0x78, round + 1, lv, 0x1000));
        out.draws
            .push(ChromeDraw::plain(3, mirror, 0x77, lv, 0x1000));
        out.draws
            .push(glyph_draw(mirror + 0x58, 0x77, round + 1, lv, 0x1000));
    } else {
        out.draws
            .push(glyph_draw(x + 0xE8, 0x78, round + 1, level, 0x1000));
        out.draws
            .push(glyph_draw(x + 0xE8, 0x78, round + 1, level, 0x1000));
        out.draws
            .push(ChromeDraw::plain(3, mirror, 0x78, level, 0x1000));
        out.draws
            .push(glyph_draw(mirror + 0x58, 0x78, round + 1, level, 0x1000));
    }
    out
}

/// Whether the banner's two sprite-actor visibility flags are set this frame
/// (`DAT_801D71AB` / `DAT_801D71D3`, byte `+0x0F` of widget records 2 and 4).
pub fn round_banner_flags(t: i32) -> bool {
    !(BANNER_SLIDE_IN..BANNER_SLIDE_OUT).contains(&t)
}

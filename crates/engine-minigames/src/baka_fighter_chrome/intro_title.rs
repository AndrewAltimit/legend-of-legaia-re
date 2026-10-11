//! The intro title card timeline.

use super::*;

// ---------------------------------------------------------------------------
// Intro title card
// ---------------------------------------------------------------------------

/// Frame thresholds of the intro title timeline.
pub const INTRO_LOGO_IN: i32 = 30;
pub const INTRO_LOGO_HOLD: i32 = 100;
pub const INTRO_SUBTITLE_END: i32 = 140;

/// The intro title card's announcer latch (`DAT_801DBE8C`): `0` before the
/// first line, `1` after it, `2` after the second.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IntroTitle {
    pub announced: i32,
}

impl IntroTitle {
    /// PORT: FUN_801D59D4 - the Baka Fighter intro title card.
    ///
    /// One call per frame with the elapsed frame count `t`. Three segments,
    /// each an independent range test on the same counter (they do not chain -
    /// retail re-tests `t` at every stage):
    ///
    /// - `30 <= t < 100`: the logo (widget `0x28`) fades up at `(0xA0, 0x80)`,
    ///   brightness `(t - 30) * 8` clamped by holding the multiplier at `0x10`.
    ///   The first announcer line (`XA33` channel `0x0E`) fires once, latched
    ///   by [`IntroTitle::announced`].
    /// - `100 <= t < 140`: the logo holds at brightness `0x80`; the second
    ///   line (`XA33` channel `0x0F`) fires once; the first four frames push a
    ///   white screen tint; the subtitle (widget `0x22`) shrinks in from
    ///   `size = 0x1000 + (0x10 - k) << 11` where `k = min(t - 100, 0x10)`,
    ///   and the banner CLUT flips to [`BANNER_CLUT_FLASH`] until `k` reaches
    ///   its clamp.
    /// - `t >= 140`: the CLUT returns to [`BANNER_CLUT_IDLE`] and the full
    ///   card assembles - a four-cell caption ramp (widgets `0x24..=0x27`,
    ///   one per four frames from `t = 147`), the sweep bar (widget `0x32`)
    ///   whose brightness ramps `4` per frame from `-0x80`, a screen tint
    ///   fading white to black over the same 64 frames, then the logo, the
    ///   subtitle at `0x80 + ramp`, the two side ornaments (`0x2A` at
    ///   `x = 0x86`, `0x2B` at `x = 0xBB`) and the underline (`0x23`).
    ///
    /// The caption index is `(e - 8) >> 2` for `e >= 8` and `(e - 5) >> 2`
    /// below it (retail's round-toward-zero shift), tested **unsigned** so the
    /// negative early values fall through undrawn.
    pub fn frame(&mut self, t: i32) -> ChromeFrame {
        let mut out = ChromeFrame::default();

        if (INTRO_LOGO_IN..INTRO_LOGO_HOLD).contains(&t) {
            let step = if t - INTRO_LOGO_IN >= 0x11 {
                0x10
            } else {
                t - INTRO_LOGO_IN
            };
            out.draws
                .push(ChromeDraw::plain(0x28, 0xA0, 0x80, step * 8, 0x1000));
            if self.announced == 0 {
                self.announced = 1;
                out.xa = Some(INTRO_LOGO_XA);
            }
        }

        if (INTRO_LOGO_HOLD..INTRO_SUBTITLE_END).contains(&t) {
            let e = t - INTRO_LOGO_HOLD;
            if self.announced == 1 && e >= 0 {
                self.announced = 2;
                out.xa = Some(INTRO_SUBTITLE_XA);
            }
            out.draws
                .push(ChromeDraw::plain(0x28, 0xA0, 0x80, 0x80, 0x1000));
            if e < 4 {
                out.tint = Some(ScreenTint { grey: 0xFF });
            }
            let k = 0x10 - if e >= 0x11 { 0x10 } else { e };
            out.banner_clut = Some(if k != 0 {
                BANNER_CLUT_FLASH
            } else {
                BANNER_CLUT_IDLE
            });
            out.draws.push(ChromeDraw::plain(
                0x22,
                0xA0,
                0x98,
                0x80,
                (k << 11) + 0x1000,
            ));
        }

        if t >= INTRO_SUBTITLE_END {
            out.banner_clut = Some(BANNER_CLUT_IDLE);
            let e = t - INTRO_SUBTITLE_END;
            // Round-toward-zero `(e - 8) / 4`.
            let mut caption = if e - 8 >= 0 {
                (e - 8) >> 2
            } else {
                (e - 5) >> 2
            };
            if (caption as u32) < 4 {
                out.draws.push(ChromeDraw::plain(
                    0x24 + caption as u8,
                    0xA0,
                    0x98,
                    0x80,
                    0x1000,
                ));
            }
            // Sweep ramp: 4 per frame, saturating at 0xFF. Retail's saturated
            // arm also drops the caption ramp to zero.
            let mut ramp = e << 2;
            let mut sweep = ramp - 0x80;
            if ramp >= 0x100 {
                ramp = 0xFF;
                caption = 0;
                sweep = ramp - 0x80;
            }
            if sweep < 0 {
                sweep = 0;
            }
            out.draws
                .push(ChromeDraw::plain(0x32, 0xA0, 0xB8, sweep, 0x1000));
            out.tint = Some(ScreenTint {
                grey: (0xFF - ramp) as u8,
            });
            out.draws
                .push(ChromeDraw::plain(0x28, 0xA0, 0x80, 0x80, 0x1000));
            out.draws
                .push(ChromeDraw::plain(0x22, 0xA0, 0x98, caption + 0x80, 0x1000));
            out.draws
                .push(ChromeDraw::plain(0x2A, 0x86, 0x64, 0x80, 0x1000));
            out.draws
                .push(ChromeDraw::plain(0x2B, 0xBB, 0x64, 0x80, 0x1000));
            out.draws
                .push(ChromeDraw::plain(0x23, 0xA0, 0x80, 0x80, 0x1000));
        }

        out
    }
}

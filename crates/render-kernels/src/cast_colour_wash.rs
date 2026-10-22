//! PROT 0898's **cast colour wash**: the full-screen gradient backdrop the
//! two cast dispatchers draw from their shared tail while a module has armed
//! it.
//!
//! PORT: FUN_801F2410 (PROT 0898; the band mode, `ctx[+0x27D] == 0`, whole -
//! the scroll step and the packet chain)
//!
//! # Where it runs
//!
//! `FUN_801F1ED4` (the Seru band) and `FUN_801F2160` (the capture band) end in
//! one epilogue each (`0x801F2128` / `0x801F23D8`) that calls
//! `FUN_801F2410` when `ctx[+0x27A] != 0` (`see ghidra/scripts/funcs/
//! overlay_muscle_dome_801f2410.txt`, dumped under that prefix; the routine is
//! PROT 0898's). It is drawn on every frame the resident module ticks with a
//! non-zero alpha - the modules ramp that byte themselves - and on no other.
//!
//! # The parameter block
//!
//! | `ctx` | meaning |
//! |---|---|
//! | `+0x27A` | alpha: every colour is scaled `c * alpha / 255` |
//! | `+0x27B` | the scanline modes' wave amplitude |
//! | `+0x27C` | band height `h` |
//! | `+0x27D` | mode: `0` bands, `1` / `2` scanline strips, `3+` nothing |
//! | `+0x27E..+0x280` | colour A |
//! | `+0x281..+0x283` | colour B |
//! | `+0x285` / `+0x286` | the scanline modes' `u` / `v` bases |
//! | `+0x32A` | scroll (`i16`) |
//! | `+0x32C` | scroll rate (`i16`) |
//!
//! # The scroll (`0x801F2444..0x801F24DC`)
//!
//! With `h != 0` the scroll advances `step * rate` and is wrapped into the
//! band once each way (`h < scroll` takes `h` off; a negative scroll gets `h`
//! back); with `h == 0` it advances by the bare frame step.
//!
//! # The band mode (`0x801F2520..0x801F2954`)
//!
//! From `y = -scroll`, while `y < 0xE0`: two opaque `POLY_G4` (`0x38`,
//! tag length `8`) across the whole width (`x` `0..=0x13F`), the first from
//! `y` to `y + h/2` shading A (top) to B, the second from there to `y + h`
//! shading B back to A; then `y += h`. Both are linked at `ot + 0x3FFC` - the
//! last word of the 4096-entry table, the **farthest** bucket - so the wash
//! is drawn before everything else in the frame and every scene primitive
//! covers it. [`band_prims`] marks each quad with the far-plane
//! [`CornerDepth`] for the same reason: it lands only where the scene left
//! the depth clear.
//!
//! # The scanline modes (`0x801F2958..0x801F2D20`)
//!
//! Modes `1` and `2` draw a textured backdrop instead: for every screen line
//! `y` in `0..0xE0`, one-line `POLY_FT4`s (`0x2C`, opaque, colour
//! `(alpha, alpha, alpha)`, so each texel is `texel * alpha / 128`) that
//! resample texture page `0xA8` (8bpp at VRAM `(512, 0)`) through CLUT
//! `0x7A00` (`(0, 488)`) - the image the module uploaded there. Line `y`
//! reads texture row `v = v_base + y + hi(row_v[y])` and starts at
//! `u = wobble(y) + u_base + hi(row_u[y])`, where `hi` is the signed high
//! byte of the line's halfword in the two per-line arrays `ctx[+0x32E]` /
//! `ctx[+0x4EE]`, and the wobble is a sine over the band
//! (`m = (scroll + y) % h`):
//!
//! ```text
//! m <  h/2:  wobble = !trunc(sin[(m << 12) / h]         * amp / 0x2000)
//! m >= h/2:  wobble =  trunc(sin[((m - h/2) << 12) / h] * amp / 0x2000)
//! ```
//!
//! The 256-texel row wraps across the 320-pixel line in two or three strips:
//! `[0, 0xFF - u]` over `u..=0xFF`, then from `s = 0xFF - u` either the
//! whole row again and a third strip to `0x13F` (`s < 0x40`), or the row's
//! head out to `0x13F`. Mode `2` mirrors the second strip's `u` (the
//! `0x801F2C34..0x801F2C50` swap). Every strip is linked at `ot + 0x3FFC`,
//! the farthest bucket, like the bands. No routine on the disc stores into
//! the two per-line arrays, so the port reads them as zero
//! ([`scanline_prims`]).

use crate::screen_prim::{CornerDepth, FlatQuad, ScreenPrim, ScreenQuad};

/// The scanline modes' texture page (`li t0, 0xa8`): 8bpp at `(512, 0)`.
pub const SCANLINE_TPAGE: u16 = 0xA8;
/// The scanline modes' CLUT (`li s8, 0x7a00`): `(0, 488)`.
pub const SCANLINE_CLUT: u16 = 0x7A00;

/// The ordering-table bucket both band quads link at (`ot + 0x3FFC`).
pub const WASH_OT: u32 = 0xFFF;

/// The screen line the band loop stops at (`slti v0, s1, 0xE0`).
pub const WASH_BOTTOM: i32 = 0xE0;

/// The right edge every band quad spans to (`li s4, 0x13F`).
pub const WASH_RIGHT: i16 = 0x13F;

/// The colour-wash block of the battle context, as the cast modules write it
/// (`ctx[+0x27A..+0x286]`, `ctx[+0x32A]`, `ctx[+0x32C]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CastColourWash {
    /// `+0x27A`.
    pub alpha: u8,
    /// `+0x27B`.
    pub amplitude: u8,
    /// `+0x27C`.
    pub height: u8,
    /// `+0x27D`.
    pub mode: u8,
    /// `+0x27E..+0x280`.
    pub colour_a: [u8; 3],
    /// `+0x281..+0x283`.
    pub colour_b: [u8; 3],
    /// `+0x284` (the unreferenced pulse routine's phase byte).
    pub pulse: u8,
    /// `+0x285`.
    pub u_base: u8,
    /// `+0x286`.
    pub v_base: u8,
    /// `+0x32A`.
    pub scroll: i16,
    /// `+0x32C`.
    pub scroll_rate: i16,
}

impl CastColourWash {
    /// Write one byte of the block by its `ctx` offset (a halfword field
    /// takes `value` whole). Offsets outside the block are ignored.
    pub fn store(&mut self, field: u16, value: i32) {
        let b = value as u8;
        match field {
            0x27A => self.alpha = b,
            0x27B => self.amplitude = b,
            0x27C => self.height = b,
            0x27D => self.mode = b,
            0x27E..=0x280 => self.colour_a[usize::from(field - 0x27E)] = b,
            0x281..=0x283 => self.colour_b[usize::from(field - 0x281)] = b,
            0x284 => self.pulse = b,
            0x285 => self.u_base = b,
            0x286 => self.v_base = b,
            0x32A => self.scroll = value as i16,
            0x32C => self.scroll_rate = value as i16,
            _ => {}
        }
    }

    /// Read one field back, as [`Self::store`] addresses it.
    pub fn load(&self, field: u16) -> i32 {
        match field {
            0x27A => i32::from(self.alpha),
            0x27B => i32::from(self.amplitude),
            0x27C => i32::from(self.height),
            0x27D => i32::from(self.mode),
            0x27E..=0x280 => i32::from(self.colour_a[usize::from(field - 0x27E)]),
            0x281..=0x283 => i32::from(self.colour_b[usize::from(field - 0x281)]),
            0x284 => i32::from(self.pulse),
            0x285 => i32::from(self.u_base),
            0x286 => i32::from(self.v_base),
            0x32A => i32::from(self.scroll),
            0x32C => i32::from(self.scroll_rate),
            _ => 0,
        }
    }

    /// Whether the dispatchers' tail calls the emitter this frame.
    pub fn armed(&self) -> bool {
        self.alpha != 0
    }

    /// The scroll step the emitter opens with, for a frame step of `step`
    /// vsyncs (`0x801F2444..0x801F24DC`).
    pub fn step_scroll(&mut self, step: u8) {
        let step = i32::from(step);
        if self.height != 0 {
            let h = i32::from(self.height);
            let mut s = i32::from(self.scroll) + step * i32::from(self.scroll_rate);
            s = i32::from(s as i16);
            if h < s {
                s -= h;
            }
            let s16 = s as i16;
            self.scroll = if s16 < 0 {
                (i32::from(s16) + h) as i16
            } else {
                s16
            };
        } else {
            self.scroll = (i32::from(self.scroll) + step) as i16;
        }
    }

    /// One channel scaled by the alpha: `c * alpha / 255`, the
    /// `mult 0x80808081` / `sra 7` divide (`0x801F257C..0x801F25A0`).
    pub fn scaled(&self, c: u8) -> u8 {
        ((u32::from(c) * u32::from(self.alpha)) / 255) as u8
    }
}

/// The band mode's packets for this frame, in link order (each band's
/// upper quad first). Empty for an unarmed wash, a zero band height, or a
/// scanline mode.
pub fn band_prims(w: &CastColourWash) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    if !w.armed() || w.mode != 0 || w.height == 0 {
        return out;
    }
    let a = w.colour_a.map(|c| w.scaled(c));
    let b = w.colour_b.map(|c| w.scaled(c));
    let rgba = |c: [u8; 3]| [c[0], c[1], c[2], 0xFF];
    let h = i32::from(w.height);
    let mut y = -i32::from(w.scroll);
    let quad = |top: i32, bottom: i32, upper: [u8; 3], lower: [u8; 3]| {
        let (t, bm) = (top as i16, bottom as i16);
        ScreenPrim::Flat(FlatQuad {
            xy: [(0, t), (WASH_RIGHT, t), (0, bm), (WASH_RIGHT, bm)],
            color: rgba(upper),
            gouraud: Some([rgba(upper), rgba(upper), rgba(lower), rgba(lower)]),
            semi_transparent: false,
            abr_mode: 0,
            ot_index: WASH_OT,
            depth: Some(CornerDepth::new([1.0; 4])),
        })
    };
    while y < WASH_BOTTOM {
        let mid = (i32::from(w.height) >> 1) + y;
        out.push(quad(y, mid, a, b));
        out.push(quad(mid, h + y, b, a));
        y += h;
    }
    out
}

/// The scanline modes' start offset for line `y`: the band wobble
/// (`0x801F298C..0x801F2AA4`), before the bases and the per-line arrays.
pub fn scanline_wobble(w: &CastColourWash, y: i32) -> u8 {
    let h = i32::from(w.height);
    if h == 0 {
        return 0;
    }
    let m = (i32::from(w.scroll) + y) % h;
    let half = h >> 1;
    let amp = i32::from(w.amplitude);
    let sin = |t: i32| crate::gte::psx_sin(t as u16);
    if m < half {
        !((sin((m << 12) / h) * amp) / 0x2000) as u8
    } else {
        ((sin(((m - half) << 12) / h) * amp) / 0x2000) as u8
    }
}

/// The scanline modes' strips for this frame (module docs), in link order.
/// Empty for an unarmed wash, a band mode or a zero band height (whose
/// divide traps in retail). The per-line arrays read as zero.
pub fn scanline_prims(w: &CastColourWash) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    if !w.armed() || !matches!(w.mode, 1 | 2) || w.height == 0 {
        return out;
    }
    let a = u32::from(w.alpha);
    let color = a << 16 | a << 8 | a;
    let strip = |x0: i16, x1: i16, y: i16, u0: u8, u1: u8, v: u8| {
        ScreenPrim::Textured(ScreenQuad {
            xy: [(x0, y), (x1, y), (x0, y + 1), (x1, y + 1)],
            uv: [(u0, v), (u1, v), (u0, v), (u1, v)],
            clut: SCANLINE_CLUT,
            tpage: SCANLINE_TPAGE,
            color,
            gouraud: None,
            semi_transparent: false,
            ot_index: WASH_OT,
            depth: Some(CornerDepth::new([1.0; 4])),
        })
    };
    for y in 0..WASH_BOTTOM {
        let u = scanline_wobble(w, y).wrapping_add(w.u_base);
        let v = w.v_base.wrapping_add(y as u8);
        let s = 0xFF - i16::from(u);
        let yy = y as i16;
        out.push(strip(0, s, yy, u, 0xFF, v));
        let (x1, u1) = if s < 0x40 {
            (s + 0xFF, 0xFF)
        } else {
            (WASH_RIGHT, (0x3F - s) as u8)
        };
        out.push(if w.mode == 2 {
            strip(s, x1, yy, u1, 0, v)
        } else {
            strip(s, x1, yy, 0, u1, v)
        });
        if s < 0x40 {
            out.push(strip(s + 0xFF, WASH_RIGHT, yy, 0, (0x3F - s) as u8, v));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kemaro() -> CastColourWash {
        // PROT 0918 arm 5's block (`0x801F729C..0x801F7304`).
        CastColourWash {
            alpha: 0x80,
            height: 0xFF,
            colour_a: [0x3F, 0x3F, 0xFF],
            colour_b: [0xBF, 0xBF, 0xFF],
            ..Default::default()
        }
    }

    #[test]
    fn unarmed_or_scanline_draws_nothing() {
        let mut w = kemaro();
        w.alpha = 0;
        assert!(band_prims(&w).is_empty());
        let mut w = kemaro();
        w.mode = 1;
        assert!(band_prims(&w).is_empty());
        assert!(scanline_prims(&kemaro()).is_empty(), "a band mode");
    }

    fn mule() -> CastColourWash {
        // PROT 0929's block (`0x801F6EC0..0x801F6F1C`): mode 1, amplitude 8,
        // band 0x80, rate 1, u 0, v 0x20.
        CastColourWash {
            alpha: 0x80,
            amplitude: 8,
            height: 0x80,
            mode: 1,
            u_base: 0,
            v_base: 0x20,
            scroll_rate: 1,
            ..Default::default()
        }
    }

    #[test]
    fn the_scanline_wobble_is_a_sine_over_the_band() {
        let w = mule();
        // m = 0: !(0) = 0xFF; m = h/4: !(trunc(sin(0x800) * 8 / 0x2000)) -
        // sin of a half turn truncates to 0.
        assert_eq!(scanline_wobble(&w, 0), 0xFF);
        // m = 0x20: sin[0x400] = 4096 -> 4096 * 8 / 0x2000 = 4 -> !4.
        assert_eq!(scanline_wobble(&w, 0x20), !4u8);
        // m = 0x60: the second half, sin[(0x20 << 12) / 0x80] = 4096 -> 4.
        assert_eq!(scanline_wobble(&w, 0x60), 4);
    }

    #[test]
    fn each_line_wraps_the_row_across_the_screen() {
        let p = scanline_prims(&mule());
        // Line 0: u = 0xFF, s = 0 -> [0, 0] then [0, 0xFF] then [0xFF, 0x13F].
        let quad = |i: usize| match p[i] {
            ScreenPrim::Textured(q) => q,
            _ => panic!("textured"),
        };
        let a = quad(0);
        assert_eq!(a.xy, [(0, 0), (0, 0), (0, 1), (0, 1)]);
        assert_eq!(a.uv[0], (0xFF, 0x20));
        let b = quad(1);
        assert_eq!((b.xy[0].0, b.xy[1].0), (0, 0xFF));
        assert_eq!((b.uv[0].0, b.uv[1].0), (0, 0xFF));
        let c = quad(2);
        assert_eq!((c.xy[0].0, c.xy[1].0), (0xFF, 0x13F));
        assert_eq!((c.uv[0].0, c.uv[1].0), (0, 0x3F));
        assert_eq!(a.tpage, SCANLINE_TPAGE);
        assert_eq!(a.clut, SCANLINE_CLUT);
        assert_eq!(a.color, 0x808080);
        assert!(!a.semi_transparent);
        // Every line is covered, two or three strips each.
        let lines: std::collections::BTreeSet<i16> = p
            .iter()
            .map(|q| match q {
                ScreenPrim::Textured(q) => q.xy[0].1,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(lines.len(), 0xE0);
        // Mode 2 mirrors the second strip.
        let mut w2 = mule();
        w2.mode = 2;
        let p2 = scanline_prims(&w2);
        let ScreenPrim::Textured(m) = p2[1] else {
            panic!("textured")
        };
        assert_eq!((m.uv[0].0, m.uv[1].0), (0xFF, 0));
    }

    #[test]
    fn one_tall_band_covers_the_frame_a_to_b_to_a() {
        let p = band_prims(&kemaro());
        assert_eq!(p.len(), 2, "h = 0xFF reaches past 0xE0 in one band");
        let ScreenPrim::Flat(q) = p[0] else {
            panic!("flat")
        };
        assert_eq!(q.xy, [(0, 0), (0x13F, 0), (0, 0x7F), (0x13F, 0x7F)]);
        // 0x3F * 0x80 / 255 = 31, 0xBF * 0x80 / 255 = 95, 0xFF -> 0x80.
        let g = q.gouraud.unwrap();
        assert_eq!(g[0], [31, 31, 128, 255]);
        assert_eq!(g[2], [95, 95, 128, 255]);
        assert!(!q.semi_transparent);
        assert_eq!(q.ot_index, WASH_OT);
        let ScreenPrim::Flat(q2) = p[1] else {
            panic!("flat")
        };
        assert_eq!(q2.xy[0].1, 0x7F);
        assert_eq!(q2.xy[2].1, 0xFF);
        assert_eq!(q2.gouraud.unwrap()[2], [31, 31, 128, 255]);
    }

    #[test]
    fn scroll_wraps_into_the_band() {
        // PROT 0922: h = 40, rate 1.
        let mut w = CastColourWash {
            alpha: 0x80,
            height: 40,
            scroll_rate: 1,
            ..Default::default()
        };
        for _ in 0..41 {
            w.step_scroll(1);
        }
        assert_eq!(w.scroll, 1, "41 steps of 1 over a 40-line band");
        let p = band_prims(&w);
        let ScreenPrim::Flat(q) = p[0] else {
            panic!("flat")
        };
        assert_eq!(q.xy[0].1, -1, "the first band starts at -scroll");
        // Bands until y reaches 0xE0: -1, 39, ... 199 -> 6 bands.
        assert_eq!(p.len(), 12);
    }
}

//! The PROT 0977 (`other_game` / Muscle Dome arena door-init) overlay's HUD
//! primitive layer: a table-driven textured-Gouraud-quad emitter and the
//! decimal readout built on top of it.
//!
//! Three routines share one descriptor table at overlay VA `0x801D170C`,
//! stride `0x14` ([`HUD_SPRITE_STRIDE`]). Each emits a PSX `POLY_GT4`
//! (Gouraud-shaded, textured four-point polygon, GP0 command `0x3C`, or
//! `0x3E` when the record's semi-transparency byte is set) into the
//! scratchpad primitive pool and links it into the ordering table at the
//! depth held in `DAT_801D1AA8`, which the emitter then resets to `3`.
//!
//! The two emitters differ only in how the quad is placed and scaled:
//!
//! * [`hud_quad_centred`] (`FUN_801D050C`) treats `(x, y)` as the **centre**
//!   and halves the extent (`>> 13` instead of `>> 12`), so the quad spans
//!   `x - half ..= x + half - 1`.
//! * [`hud_quad_corner`] (`FUN_801D08EC`) treats `(x, y)` as the **top-left**
//!   corner and spans `x ..= x + extent`, and clamps its brightness argument
//!   to `0..=0xFF` first - the centred emitter does not clamp.
//!
//! [`decimal_slots`] / [`decimal_quads`] (`FUN_801D1308`) render an unsigned
//! decimal readout of up to eight digits through the centred emitter, using
//! record index [`DIGIT_SPRITE_INDEX`] as the glyph and stepping the glyph's
//! texture-U column per digit.
//!
//! Provenance: `ghidra/scripts/funcs/overlay_0977_other_game_801d050c.txt`,
//! `..._801d08ec.txt`, `..._801d1308.txt`; ported from the disassembly, not
//! the decompiled C.
//!
//! # NOT WIRED
//!
//! Nothing in the engine hosts the 0977 arena HUD. The overlay is the mode-24
//! sub-id-5 door/init slot for the Muscle Dome arena, whose match state
//! machine lives in the battle overlay; the engine's `muscle_dome` session has
//! no HUD surface and never loads 0977's sprite table, so there is no source
//! for the [`HudSprite`] records these builders consume. Wiring needs the
//! arena HUD screen plus a parser for the overlay-resident descriptor table -
//! in that order.

/// Byte stride of one sprite descriptor in the table at `0x801D170C`.
pub const HUD_SPRITE_STRIDE: usize = 0x14;

/// Low-bit width of the emitter's `sel` argument that selects a table row.
/// The remaining high bits are the *variant* (`sel >> 10`, truncating).
pub const HUD_SEL_INDEX_BITS: u32 = 10;

/// Table row the decimal renderer draws every digit from.
pub const DIGIT_SPRITE_INDEX: usize = 9;

/// Digit slots the decimal renderer walks (`10^7 .. 10^0`).
pub const DECIMAL_SLOTS: usize = 8;

/// Texture-U column of decimal digit `0`. Each digit steps `+8` from here
/// (`u = digit * 8 - 0x80`, stored as a byte).
pub const DIGIT_U_BASE: i32 = -0x80;

/// Horizontal advance between two digit cells, in screen pixels.
pub const DIGIT_ADVANCE: i32 = 8;

/// CLUT the digit record carries at rest; the renderer offsets it by its
/// palette argument for the duration of the call and restores it after.
pub const DIGIT_CLUT_BASE: u16 = 0x7D86;

/// Quad scale the decimal renderer passes to the centred emitter (1.0 in
/// 12.12 fixed point).
pub const DIGIT_SCALE: i32 = 0x1000;

/// One `0x14`-byte record of the sprite descriptor table at `0x801D170C`.
///
/// Field offsets are the retail ones; the emitters *mutate* two of them
/// (`semi_transparent` and `page`) whenever they are called with a non-zero
/// variant, and that mutation persists in the table for later calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HudSprite {
    /// `+0x00` - texel-to-world size scalar, applied before the caller's
    /// scale.
    pub size: i32,
    /// `+0x04` - base tpage word; the emitter adds `page * 0x20`.
    pub tpage: u16,
    /// `+0x06` - CLUT word.
    pub clut: u16,
    /// `+0x08` - texture U of the top-left texel.
    pub u0: u8,
    /// `+0x09` - texture V of the top-left texel.
    pub v0: u8,
    /// `+0x0A` - texel width.
    pub w: u8,
    /// `+0x0B` - texel height.
    pub h: u8,
    /// `+0x0C..0x0E` - colour of the two **top** vertices.
    pub rgb_top: [u8; 3],
    /// `+0x0F` - non-zero selects the semi-transparent command (`0x3E`).
    pub semi_transparent: u8,
    /// `+0x10..0x12` - colour of the two **bottom** vertices, which is what
    /// makes every quad a vertical two-stop gradient.
    pub rgb_bottom: [u8; 3],
    /// `+0x13` - tpage page offset, multiplied by `0x20` into the tpage word.
    pub page: u8,
}

/// A resolved `POLY_GT4` packet, renderer-agnostic.
///
/// Vertex order is the PSX one: `0` top-left, `1` top-right, `2` bottom-left,
/// `3` bottom-right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudQuad {
    /// Screen-space vertex positions.
    pub xy: [(i16, i16); 4],
    /// Per-vertex texture coordinates.
    pub uv: [(u8, u8); 4],
    /// Per-vertex colour, already scaled by the brightness argument.
    pub rgb: [[u8; 3]; 4],
    /// Texture-page word (`record.tpage + page * 0x20`).
    pub tpage: u16,
    /// CLUT word (`record.clut`, `+1` when the variant is `2`).
    pub clut: u16,
    /// `true` selects GP0 `0x3E` over `0x3C`.
    pub semi_transparent: bool,
}

/// GP0 command byte of an opaque Gouraud-textured quad.
pub const GP0_POLY_GT4: u8 = 0x3C;

/// Split an emitter `sel` argument into `(table index, variant)`.
///
/// The retail code divides by `0x400` truncating toward zero
/// (`if (sel < 0) sel += 0x3FF; sel >>= 10`) and masks the index with
/// `0x3FF`, so a negative `sel` yields a negative variant and a *positive*
/// index.
#[inline]
pub fn hud_sel_split(sel: i32) -> (usize, i32) {
    let biased = if sel < 0 {
        sel.wrapping_add(0x3FF)
    } else {
        sel
    };
    ((sel & 0x3FF) as usize, biased >> HUD_SEL_INDEX_BITS)
}

/// Scale one colour channel by the emitter's brightness argument
/// (`c * brightness / 256`, truncating toward zero, stored as a byte).
#[inline]
fn scale_channel(c: u8, brightness: i32) -> u8 {
    let p = (c as i32).wrapping_mul(brightness);
    let p = if p < 0 { p.wrapping_add(0xFF) } else { p };
    (p >> 8) as u8
}

/// Apply the variant side effects the retail emitters perform on the shared
/// table before building the packet, and return the CLUT the packet uses.
fn apply_variant(rec: &mut HudSprite, variant: i32) -> u16 {
    if variant != 0 {
        rec.semi_transparent = 1;
        rec.page = variant as u8;
    }
    if variant == 2 {
        rec.clut.wrapping_add(1)
    } else {
        rec.clut
    }
}

/// Fill the parts of a quad that both emitters share: colours, texture
/// coordinates, tpage, CLUT and the transparency flag.
fn shared_quad(rec: &HudSprite, brightness: i32, clut: u16) -> HudQuad {
    let top = [
        scale_channel(rec.rgb_top[0], brightness),
        scale_channel(rec.rgb_top[1], brightness),
        scale_channel(rec.rgb_top[2], brightness),
    ];
    let bottom = [
        scale_channel(rec.rgb_bottom[0], brightness),
        scale_channel(rec.rgb_bottom[1], brightness),
        scale_channel(rec.rgb_bottom[2], brightness),
    ];
    let u1 = rec.u0.wrapping_add(rec.w).wrapping_sub(1);
    let v1 = rec.v0.wrapping_add(rec.h).wrapping_sub(1);
    HudQuad {
        xy: [(0, 0); 4],
        uv: [(rec.u0, rec.v0), (u1, rec.v0), (rec.u0, v1), (u1, v1)],
        rgb: [top, top, bottom, bottom],
        tpage: rec.tpage.wrapping_add((rec.page as u16).wrapping_mul(0x20)),
        clut,
        semi_transparent: rec.semi_transparent != 0,
    }
}

/// Half-extent of the centred emitter: `((texels * size) >> 13) * scale >> 12`,
/// each shift truncating toward zero.
fn centred_half(texels: u8, size: i32, scale: i32) -> i32 {
    let p = (texels as i32).wrapping_mul(size);
    let p = if p < 0 { p.wrapping_add(0x1FFF) } else { p };
    let q = (p >> 13).wrapping_mul(scale);
    let q = if q < 0 { q.wrapping_add(0xFFF) } else { q };
    q >> 12
}

/// Full extent of the corner emitter: the same chain with `>> 12` first.
fn corner_span(texels: u8, size: i32, scale: i32) -> i32 {
    let p = (texels as i32).wrapping_mul(size);
    let p = if p < 0 { p.wrapping_add(0xFFF) } else { p };
    let q = (p >> 12).wrapping_mul(scale);
    let q = if q < 0 { q.wrapping_add(0xFFF) } else { q };
    q >> 12
}

/// Emit the quad **centred** on `(x, y)`.
///
/// `brightness` is applied to every colour channel unclamped (a value above
/// `0x100` overflows the byte exactly as retail does); `scale` is 12.12 fixed
/// point. `variant` is `sel >> 10` and, when non-zero, is written back into
/// the shared record as its transparency flag and tpage page.
///
/// PORT: FUN_801d050c
pub fn hud_quad_centred(
    rec: &mut HudSprite,
    x: i16,
    y: i16,
    variant: i32,
    brightness: i32,
    scale: i32,
) -> HudQuad {
    let clut = apply_variant(rec, variant);
    let mut q = shared_quad(rec, brightness, clut);
    let hw = centred_half(rec.w, rec.size, scale);
    let hh = centred_half(rec.h, rec.size, scale);
    let x0 = (x as i32).wrapping_sub(hw) as i16;
    let x1 = (x as i32).wrapping_add(hw).wrapping_sub(1) as i16;
    let y0 = (y as i32).wrapping_sub(hh) as i16;
    let y1 = (y as i32).wrapping_add(hh).wrapping_sub(1) as i16;
    q.xy = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
    q
}

/// Emit the quad with `(x, y)` as its **top-left** corner.
///
/// Unlike [`hud_quad_centred`] this clamps `brightness` into `0..=0xFF`
/// before scaling, and its span uses one shift less, so the same record
/// covers twice the pixels for the same `scale`.
///
/// PORT: FUN_801d08ec
pub fn hud_quad_corner(
    rec: &mut HudSprite,
    x: i16,
    y: i16,
    variant: i32,
    brightness: i32,
    scale: i32,
) -> HudQuad {
    let brightness = brightness.clamp(0, 0xFF);
    let clut = apply_variant(rec, variant);
    let mut q = shared_quad(rec, brightness, clut);
    let w = corner_span(rec.w, rec.size, scale);
    let h = corner_span(rec.h, rec.size, scale);
    let x1 = (x as i32).wrapping_add(w) as i16;
    let y1 = (y as i32).wrapping_add(h) as i16;
    q.xy = [(x, y), (x1, y), (x, y1), (x1, y1)];
    q
}

/// The eight decimal slots of a readout, most significant first.
///
/// A slot holds `Some(digit)` when retail would draw it. The rule is retail's
/// own and is not plain leading-zero suppression: the slot array starts at
/// `-1` everywhere, the **units** slot is pre-seeded with `0`, and slot `i`
/// is then written with `value / 10^(7-i)` only when that quotient is
/// non-zero. A slot whose stored quotient is negative is skipped at draw
/// time, so a **negative `value` renders nothing at all**.
///
/// PORT: FUN_801d1308 (slot fill)
pub fn decimal_slots(value: i32) -> [Option<u8>; DECIMAL_SLOTS] {
    let mut raw = [-1i32; DECIMAL_SLOTS];
    raw[DECIMAL_SLOTS - 1] = 0;
    let mut divisor = 10_000_000i32;
    for slot in raw.iter_mut() {
        let q = value / divisor;
        if q != 0 {
            *slot = q;
        }
        divisor /= 10;
    }
    let mut out = [None; DECIMAL_SLOTS];
    for (o, q) in out.iter_mut().zip(raw) {
        if q >= 0 {
            *o = Some((q % 10) as u8);
        }
    }
    out
}

/// Texture-U column of one decimal glyph (`digit * 8 - 0x80`, byte-wrapped).
///
/// PORT: FUN_801d1308 (glyph column)
#[inline]
pub fn digit_column(digit: u8) -> u8 {
    ((digit as i32) * 8 + DIGIT_U_BASE) as u8
}

/// Build the quads of a decimal readout starting at `(x, y)`.
///
/// `digit` is the table's digit record ([`DIGIT_SPRITE_INDEX`]); it is
/// mutated exactly as retail mutates it - the CLUT is offset by `palette` for
/// the duration of the call, each drawn digit rewrites the record's `u0`, and
/// the CLUT is restored to [`DIGIT_CLUT_BASE`] on return. Every glyph goes
/// through the centred emitter at [`DIGIT_SCALE`], and the pen advances
/// [`DIGIT_ADVANCE`] per slot **including** the slots that draw nothing.
///
/// PORT: FUN_801d1308
pub fn decimal_quads(
    digit: &mut HudSprite,
    x: i16,
    y: i16,
    value: i32,
    brightness: i32,
    palette: i16,
) -> Vec<HudQuad> {
    digit.clut = DIGIT_CLUT_BASE.wrapping_add(palette as u16);
    let mut out = Vec::new();
    let mut pen = x;
    for slot in decimal_slots(value) {
        if let Some(d) = slot {
            digit.u0 = digit_column(d);
            out.push(hud_quad_centred(digit, pen, y, 0, brightness, DIGIT_SCALE));
        }
        pen = pen.wrapping_add(DIGIT_ADVANCE as i16);
    }
    digit.clut = DIGIT_CLUT_BASE;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite() -> HudSprite {
        HudSprite {
            size: 0x1000,
            tpage: 0x0040,
            clut: 0x7D86,
            u0: 0x10,
            v0: 0x20,
            w: 8,
            h: 16,
            rgb_top: [0x80, 0x40, 0x20],
            semi_transparent: 0,
            rgb_bottom: [0x40, 0x20, 0x10],
            page: 0,
        }
    }

    #[test]
    fn sel_splits_index_and_variant() {
        assert_eq!(hud_sel_split(9), (9, 0));
        assert_eq!(hud_sel_split(0x409), (9, 1));
        assert_eq!(hud_sel_split(0x809), (9, 2));
        // The index mask keeps the low ten bits even for a negative sel.
        assert_eq!(hud_sel_split(-1).0, 0x3FF);
    }

    #[test]
    fn full_brightness_passes_the_record_colours_through() {
        let mut r = sprite();
        let q = hud_quad_centred(&mut r, 100, 50, 0, 0x100, 0x1000);
        assert_eq!(q.rgb[0], [0x80, 0x40, 0x20]);
        assert_eq!(q.rgb[1], q.rgb[0], "both top vertices share a colour");
        assert_eq!(q.rgb[2], [0x40, 0x20, 0x10]);
        assert_eq!(q.rgb[3], q.rgb[2], "both bottom vertices share a colour");
    }

    #[test]
    fn half_brightness_halves_every_channel() {
        let mut r = sprite();
        let q = hud_quad_centred(&mut r, 0, 0, 0, 0x80, 0x1000);
        assert_eq!(q.rgb[0], [0x40, 0x20, 0x10]);
    }

    #[test]
    fn the_centred_emitter_brackets_the_anchor() {
        let mut r = sprite();
        // size 0x1000 and scale 0x1000 make the half-extent w/2 and h/2.
        let q = hud_quad_centred(&mut r, 100, 50, 0, 0x100, 0x1000);
        assert_eq!(q.xy[0], (100 - 4, 50 - 8));
        assert_eq!(q.xy[3], (100 + 4 - 1, 50 + 8 - 1));
    }

    #[test]
    fn the_corner_emitter_spans_the_full_extent_from_the_anchor() {
        let mut r = sprite();
        let q = hud_quad_corner(&mut r, 100, 50, 0, 0x100, 0x1000);
        assert_eq!(q.xy[0], (100, 50));
        assert_eq!(q.xy[3], (100 + 8, 50 + 16));
    }

    #[test]
    fn only_the_corner_emitter_clamps_brightness() {
        let mut a = sprite();
        let mut b = sprite();
        // 0x200 doubles: 0x80 * 0x200 >> 8 = 0x100, which truncates to 0.
        assert_eq!(
            hud_quad_centred(&mut a, 0, 0, 0, 0x200, 0x1000).rgb[0][0],
            0
        );
        // The corner emitter clamps to 0xFF first, so nothing overflows.
        assert_eq!(
            hud_quad_corner(&mut b, 0, 0, 0, 0x200, 0x1000).rgb[0][0],
            0x7F
        );
    }

    #[test]
    fn texture_coordinates_span_the_record_rect() {
        let mut r = sprite();
        let q = hud_quad_centred(&mut r, 0, 0, 0, 0x100, 0x1000);
        assert_eq!(q.uv[0], (0x10, 0x20));
        assert_eq!(q.uv[3], (0x10 + 8 - 1, 0x20 + 16 - 1));
    }

    #[test]
    fn a_non_zero_variant_writes_back_into_the_shared_record() {
        let mut r = sprite();
        let q = hud_quad_centred(&mut r, 0, 0, 1, 0x100, 0x1000);
        assert!(q.semi_transparent);
        assert_eq!(q.tpage, 0x0040 + 0x20);
        assert_eq!(r.page, 1, "the mutation persists in the table");
        assert_eq!(r.semi_transparent, 1);
        // A later variant-0 call still sees the mutated record.
        let q2 = hud_quad_centred(&mut r, 0, 0, 0, 0x100, 0x1000);
        assert!(q2.semi_transparent);
        assert_eq!(q2.tpage, 0x0040 + 0x20);
    }

    #[test]
    fn variant_two_also_bumps_the_clut() {
        let mut r = sprite();
        let q = hud_quad_centred(&mut r, 0, 0, 2, 0x100, 0x1000);
        assert_eq!(q.clut, 0x7D86 + 1);
        assert_eq!(r.page, 2);
        // The record's own CLUT is not mutated - only the packet's.
        assert_eq!(r.clut, 0x7D86);
    }

    #[test]
    fn zero_renders_a_single_units_digit() {
        let s = decimal_slots(0);
        assert_eq!(s[..7].iter().filter(|d| d.is_some()).count(), 0);
        assert_eq!(s[7], Some(0));
    }

    #[test]
    fn leading_zeros_are_suppressed() {
        let s = decimal_slots(1000);
        assert_eq!(
            s,
            [None, None, None, None, Some(1), Some(0), Some(0), Some(0)]
        );
    }

    #[test]
    fn eight_digits_fill_every_slot() {
        let s = decimal_slots(12_345_678);
        let got: Vec<u8> = s.iter().map(|d| d.unwrap()).collect();
        assert_eq!(got, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn a_negative_value_draws_nothing() {
        // Every stored quotient is negative, including the pre-seeded units
        // slot, so retail's `bltz` skip drops all eight.
        assert!(decimal_slots(-42).iter().all(|d| d.is_none()));
    }

    #[test]
    fn digit_columns_step_eight_from_the_base() {
        assert_eq!(digit_column(0), 0x80);
        assert_eq!(digit_column(1), 0x88);
        assert_eq!(digit_column(9), 0xC8);
    }

    #[test]
    fn the_readout_advances_over_suppressed_slots_too() {
        let mut d = sprite();
        let quads = decimal_quads(&mut d, 100, 60, 42, 0x100, 0);
        assert_eq!(quads.len(), 2);
        // Slots 6 and 7 draw; the pen has already stepped six cells.
        assert_eq!(quads[0].xy[0].0, 100 + 6 * 8 - 4);
        assert_eq!(quads[1].xy[0].0, 100 + 7 * 8 - 4);
    }

    #[test]
    fn the_readout_restores_the_digit_clut() {
        let mut d = sprite();
        let quads = decimal_quads(&mut d, 0, 0, 7, 0x100, 3);
        assert_eq!(quads[0].clut, DIGIT_CLUT_BASE + 3);
        assert_eq!(d.clut, DIGIT_CLUT_BASE, "restored on return");
    }
}

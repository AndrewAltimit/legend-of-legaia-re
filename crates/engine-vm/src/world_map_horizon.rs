//! World-map horizon / sky band emitter, ported clean-room from
//! `FUN_801D7EA0` (world-map overlay) and its byte-identical 0897
//! field-overlay relocation copy `FUN_801C9688`.
//!
//! PORT: FUN_801d7ea0, FUN_801c9688
//!
//! The emitter is one-shot: it runs only when the gate `_DAT_801F351C` is
//! set, and self-clears it. The gate plus its three staged params live in
//! [`engine_core::world_map::EmitterGate`]; this module is the consumer that
//! turns one armed gate into a frame's worth of prims.
//!
//! ## What it draws
//!
//! Retail walks a row counter `iVar11` from `4` to `0xE3` - **224
//! iterations**, one per visible scanline of the 224-line NTSC draw area.
//! Each iteration emits a horizontal **band** one pixel tall spanning
//! `y_top = i - 4` to `y_bottom = i - 3`:
//!
//! | Prim | Retail packet | Role |
//! |---|---|---|
//! | 2 x `POLY_FT4` | tag `0x09000000`, code+colour `0x2C808080` | The band's left and right halves, textured. |
//! | 1 x `LINE_F2` | tag `0x03000000`, code+colour `0x40010101` | A full-width (`0..0x140`) near-black scanline at `y_top`. |
//! | 1 x `MoveImage` | 6-word packet via the libgpu `DR_MOVE` filler | VRAM row blit of a `0x140 x 1` source strip. |
//!
//! So ~896 prims per call across the 224 bands. The band's horizontal
//! extents are warped per-row by a trig-table lookup, which is what makes
//! the plane appear to rotate with the camera - consistent with a horizon /
//! sky / animated-background plane rather than a fixed continent mesh.
//!
//! ## Horizontal extents
//!
//! Three x coordinates per band, derived from the staged `scale` and the
//! per-row trig sample `c` (4.12 fixed point):
//!
//! ```text
//! half = scale >> 1
//! lo   = scale / 5
//! hi   = scale - lo
//! c    = trig[(angle & 0xFFF)]
//!
//! x_a = -half - ((half * c) >> 12)
//! x_b = x_a + ((hi * c) >> 12) + hi + 0xFF
//! x_c = x_b + lo + ((lo * c) >> 12) + 0x40
//! ```
//!
//! Quad 0 spans `x_a..x_b`, quad 1 spans `x_b..x_c`. Retail accumulates
//! these in full 32-bit registers and truncates **once**, at the `sh`
//! that stores each vertex X into the packet - not per term. The port
//! computes in `i32` and narrows at the same point, so the two agree
//! including how an overflowing `scale` wraps.
//!
//! ## Angle advance
//!
//! The persisted angle (`_DAT_801F3518`) advances **once per call** by
//! `tick_delta * angle_step`, and is stored *before* the loop runs. The
//! extra `+= 0x10` each iteration is loop-local scratch and is **not**
//! written back - so the angle the next frame starts from does not include
//! the 224 per-row steps.
//!
//! ## Trig table
//!
//! `_DAT_8007B81C` is a **pointer** to a `0x1000`-entry `i16` trig table,
//! indexed `(angle & 0xFFF)`. It is the same pointer the move VM and
//! effect VM index (see [`crate::move_vm::host`]); the caller supplies the
//! samples so this module stays disc-free and unit-testable.
//!
//! ## Source
//!
//! `ghidra/scripts/funcs/overlay_world_map_801d7ea0.txt` and
//! `overlay_0897_xxx_dat_801c9688.txt` (the two overlay-resident copies).
//! See [`docs/subsystems/world-map.md`](../../../docs/subsystems/world-map.md#fun_801d7ea0---world-map-poly_ft4-batch-emitter-832-bytes).

/// First value of the retail row counter `iVar11`.
const ROW_FIRST: i32 = 4;
/// Exclusive limit of the retail row counter (`while (iVar11 < 0xE4)`).
const ROW_LIMIT: i32 = 0xE4;

/// Screen width the emitter spans, in pixels (`0x140` = 320).
pub const SCREEN_WIDTH: i16 = 0x140;

/// `POLY_FT4` GP0 command byte OR'd with the neutral grey the emitter uses
/// for every band quad (retail literal `local_30`).
pub const QUAD_CODE_COLOUR: u32 = 0x2C80_8080;

/// `LINE_F2` GP0 command byte OR'd with the near-black scanline colour
/// (retail literal `0x40010101`).
pub const LINE_CODE_COLOUR: u32 = 0x4001_0101;

/// Source-row offset applied to the per-band VRAM blit when the alternate
/// band select (`_DAT_8007B74C`) is non-zero.
pub const ALT_BAND_OFFSET: i16 = 0xF0;

/// Per-vertex `(u, v)` texture coordinates of the band's **left** quad
/// (retail writes `u` 0/0xFF and `v` 1/2).
const QUAD0_UV: [(u8, u8); 4] = [(0x00, 1), (0xFF, 1), (0x00, 2), (0xFF, 2)];
/// Texture page of the band's left quad.
const QUAD0_TPAGE: u16 = 0x0100;

/// Per-vertex `(u, v)` coordinates of the band's **right** quad.
const QUAD1_UV: [(u8, u8); 4] = [(0x3F, 1), (0x7F, 1), (0x3F, 2), (0x7F, 2)];
/// Texture page of the band's right quad.
const QUAD1_TPAGE: u16 = 0x0103;

/// One textured band quad (`POLY_FT4`).
///
/// Retail leaves the CLUT halfword (`packet + 0x0E`) **unwritten**, so the
/// quad inherits whatever CLUT the previous packet in the pool left there.
/// The port models that by not carrying a CLUT field at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandQuad {
    /// Left edge (vertices 0 and 2).
    pub x_left: i16,
    /// Right edge (vertices 1 and 3).
    pub x_right: i16,
    /// Top edge (vertices 0 and 1).
    pub y_top: i16,
    /// Bottom edge (vertices 2 and 3).
    pub y_bottom: i16,
    /// GP0 command byte + flat colour, as one word.
    pub code_colour: u32,
    /// Per-vertex texture coordinates, in vertex order.
    pub uv: [(u8, u8); 4],
    /// Texture page word.
    pub tpage: u16,
}

/// The full-width dark scanline (`LINE_F2`) drawn at the band's top edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandLine {
    pub x0: i16,
    pub x1: i16,
    pub y: i16,
    pub code_colour: u32,
}

/// The per-band VRAM row blit (`MoveImage` / `DR_MOVE`).
///
/// Retail builds it as `MoveImage(rect, 0, 1)` - source rect at
/// `(0, row)` sized `0x140 x 1`, destination `(0, 1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandBlit {
    /// Source row in VRAM (the raw row counter plus the alternate-band
    /// offset - **not** the band's screen `y`).
    pub src_y: i16,
    pub width: i16,
    pub height: i16,
    pub dst_x: i16,
    pub dst_y: i16,
}

/// One emitted scanline band: two textured quads, a dark line, a row blit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HorizonBand {
    pub quads: [BandQuad; 2],
    pub line: BandLine,
    pub blit: BandBlit,
}

/// The result of one armed emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HorizonBatch {
    /// The 224 emitted bands, in draw order (top scanline first).
    pub bands: Vec<HorizonBand>,
    /// The angle to persist back into `_DAT_801F3518`. This is the
    /// *pre-loop* value - the per-row `+= 0x10` is scratch.
    pub angle_after: u32,
    /// OT layer / draw priority the packets chain into (`_DAT_801F3528`).
    pub ot_layer: u32,
}

impl HorizonBatch {
    /// Total GPU prims this batch represents (4 per band).
    pub fn prim_count(&self) -> usize {
        self.bands.len() * 4
    }
}

/// Number of bands one emission produces - a fixed retail loop bound.
pub const BAND_COUNT: usize = (ROW_LIMIT - ROW_FIRST) as usize;

/// Emit one frame of horizon bands.
///
/// * `scale` - staged `_DAT_801F3520`.
/// * `angle` - persisted `_DAT_801F3518` from the previous call.
/// * `angle_step` - staged `_DAT_801F3524`.
/// * `tick_delta` - the per-frame tick count `DAT_1F800393`.
/// * `ot_layer` - staged `_DAT_801F3528`.
/// * `alt_band` - `_DAT_8007B74C != 0`; shifts every blit source row by
///   [`ALT_BAND_OFFSET`].
/// * `trig` - samples the `0x1000`-entry table behind `_DAT_8007B81C`.
///
/// The caller is responsible for the gate itself: retail runs this only
/// when `_DAT_801F351C != 0` and clears the flag first. Pair it with
/// `EmitterGate::take()`.
///
/// NOT WIRED: reached only from tests. `World::tick_world_map` does call
/// `run_horizon_emitter` every world-map frame, but the gate it consults
/// is never armed in production - `EmitterGate::arm` (the port of
/// `FUN_801D8258`) has no non-test caller because retail's param-prep
/// wrappers `FUN_801D1344` / `FUN_801C2B2C` are not ported. Nothing
/// consumes the resulting `HorizonBatch` either: no renderer reads
/// `WorldMapController::horizon`. So the arithmetic below is verified
/// against the disassembly and exercised by unit tests, but no frame the
/// engine draws depends on it.
///
/// `FUN_801C9688` is a relocation copy of `FUN_801D7EA0`: the two bodies
/// are instruction-identical, differing only in three branch targets
/// that shift by the `0xE818` relocation delta, so one port covers both.
// PORT: FUN_801d7ea0
// PORT: FUN_801c9688
pub fn emit_horizon(
    scale: i32,
    angle: u32,
    angle_step: u32,
    tick_delta: u8,
    ot_layer: u32,
    alt_band: bool,
    trig: &dyn Fn(u16) -> i16,
) -> HorizonBatch {
    // Retail advances and stores the angle once, before the row loop.
    let base_angle = angle.wrapping_add(u32::from(tick_delta).wrapping_mul(angle_step));

    let half = scale >> 1;
    let lo = scale / 5;
    let hi = scale - lo;

    let band_offset = if alt_band { ALT_BAND_OFFSET } else { 0 };

    let mut bands = Vec::with_capacity(BAND_COUNT);
    let mut sweep = base_angle;

    for row in ROW_FIRST..ROW_LIMIT {
        let row16 = row as i16;
        let y_top = row16 - 4;
        let y_bottom = row16 - 3;

        let c = i32::from(trig((sweep & 0xFFF) as u16));

        // Each term is truncated to i16 on its own, then added - matching
        // the per-term `(short)` casts in the retail C.
        let x_a = (-(half as i16)).wrapping_sub(((half * c) >> 12) as i16);
        let x_b = x_a
            .wrapping_add(((hi * c) >> 12) as i16)
            .wrapping_add(hi as i16)
            .wrapping_add(0xFF);
        let x_c = x_b
            .wrapping_add(lo as i16)
            .wrapping_add(((lo * c) >> 12) as i16)
            .wrapping_add(0x40);

        bands.push(HorizonBand {
            quads: [
                BandQuad {
                    x_left: x_a,
                    x_right: x_b,
                    y_top,
                    y_bottom,
                    code_colour: QUAD_CODE_COLOUR,
                    uv: QUAD0_UV,
                    tpage: QUAD0_TPAGE,
                },
                BandQuad {
                    x_left: x_b,
                    x_right: x_c,
                    y_top,
                    y_bottom,
                    code_colour: QUAD_CODE_COLOUR,
                    uv: QUAD1_UV,
                    tpage: QUAD1_TPAGE,
                },
            ],
            line: BandLine {
                x0: 0,
                x1: SCREEN_WIDTH,
                y: y_top,
                code_colour: LINE_CODE_COLOUR,
            },
            blit: BandBlit {
                src_y: row16.wrapping_add(band_offset),
                width: SCREEN_WIDTH,
                height: 1,
                dst_x: 0,
                dst_y: 1,
            },
        });

        // Loop-local sweep; deliberately not folded back into `angle_after`.
        sweep = sweep.wrapping_add(0x10);
    }

    HorizonBatch {
        bands,
        angle_after: base_angle,
        ot_layer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat trig table - every sample zero. Keeps the x algebra to its
    /// scale-only terms so the geometry is checkable by hand.
    fn flat(_: u16) -> i16 {
        0
    }

    #[test]
    fn emits_224_bands_and_896_prims() {
        let b = emit_horizon(0x500, 0, 0x10, 1, 4, false, &flat);
        assert_eq!(b.bands.len(), BAND_COUNT);
        assert_eq!(BAND_COUNT, 224, "retail loop runs rows 4..0xE4");
        assert_eq!(b.prim_count(), 896);
    }

    #[test]
    fn bands_cover_consecutive_scanlines_from_zero() {
        let b = emit_horizon(0x500, 0, 0, 0, 0, false, &flat);
        assert_eq!(b.bands[0].line.y, 0, "first band sits at scanline 0");
        assert_eq!(b.bands[0].quads[0].y_top, 0);
        assert_eq!(b.bands[0].quads[0].y_bottom, 1);
        let last = b.bands.last().unwrap();
        assert_eq!(last.quads[0].y_top, 223, "224 scanlines, 0..=223");
        assert_eq!(last.quads[0].y_bottom, 224);
        // Every band is exactly one pixel tall and steps by one.
        for (i, band) in b.bands.iter().enumerate() {
            assert_eq!(band.quads[0].y_top, i as i16);
            assert_eq!(band.quads[0].y_bottom, i as i16 + 1);
            assert_eq!(band.quads[1].y_top, band.quads[0].y_top);
        }
    }

    #[test]
    fn quads_tile_horizontally_without_a_gap() {
        let b = emit_horizon(0x500, 0, 0, 0, 0, false, &flat);
        for band in &b.bands {
            assert_eq!(
                band.quads[0].x_right, band.quads[1].x_left,
                "the right quad starts exactly where the left one ends"
            );
        }
    }

    #[test]
    fn flat_trig_gives_the_scale_only_extents() {
        // c = 0 kills every `* c >> 12` term:
        //   x_a = -(scale >> 1)
        //   x_b = x_a + hi + 0xFF
        //   x_c = x_b + lo + 0x40
        let scale = 0x500;
        let half = scale >> 1;
        let lo = scale / 5;
        let hi = scale - lo;

        let b = emit_horizon(scale, 0, 0, 0, 0, false, &flat);
        let band = &b.bands[0];
        assert_eq!(band.quads[0].x_left, -(half as i16));
        assert_eq!(band.quads[0].x_right, -(half as i16) + hi as i16 + 0xFF);
        assert_eq!(
            band.quads[1].x_right,
            band.quads[0].x_right + lo as i16 + 0x40
        );
    }

    #[test]
    fn trig_is_sampled_per_row_stepping_by_0x10() {
        use std::cell::RefCell;
        let seen = RefCell::new(Vec::new());
        let probe = |i: u16| -> i16 {
            seen.borrow_mut().push(i);
            0
        };
        emit_horizon(0x100, 0x40, 0, 0, 0, false, &probe);
        let seen = seen.into_inner();
        assert_eq!(seen.len(), BAND_COUNT);
        assert_eq!(seen[0], 0x40, "first row samples the base angle");
        assert_eq!(seen[1], 0x50, "each row advances the sweep by 0x10");
        assert_eq!(seen[2], 0x60);
        // The sweep wraps within the 0x1000-entry table.
        assert!(seen.iter().all(|&i| i < 0x1000));
    }

    #[test]
    fn angle_persists_the_pre_loop_value_only() {
        // tick_delta * angle_step is applied once; the per-row 0x10 steps
        // are scratch and must not leak into the persisted angle.
        let b = emit_horizon(0x100, 0x1000, 0x20, 3, 0, false, &flat);
        assert_eq!(b.angle_after, 0x1000 + 3 * 0x20);
    }

    #[test]
    fn zero_tick_delta_leaves_the_angle_put() {
        let b = emit_horizon(0x100, 0x777, 0x20, 0, 0, false, &flat);
        assert_eq!(b.angle_after, 0x777);
    }

    #[test]
    fn alt_band_shifts_only_the_blit_source_row() {
        let normal = emit_horizon(0x100, 0, 0, 0, 0, false, &flat);
        let alt = emit_horizon(0x100, 0, 0, 0, 0, true, &flat);
        for (n, a) in normal.bands.iter().zip(alt.bands.iter()) {
            assert_eq!(a.blit.src_y, n.blit.src_y + ALT_BAND_OFFSET);
            // Geometry is untouched by the band select.
            assert_eq!(a.quads, n.quads);
            assert_eq!(a.line, n.line);
        }
        // The blit source row is the raw counter, not the screen y.
        assert_eq!(normal.bands[0].blit.src_y, ROW_FIRST as i16);
    }

    #[test]
    fn blit_is_a_full_width_single_row_to_the_fixed_destination() {
        let b = emit_horizon(0x100, 0, 0, 0, 0, false, &flat);
        for band in &b.bands {
            assert_eq!(band.blit.width, SCREEN_WIDTH);
            assert_eq!(band.blit.height, 1);
            assert_eq!((band.blit.dst_x, band.blit.dst_y), (0, 1));
        }
    }

    #[test]
    fn line_spans_the_full_screen_at_the_band_top() {
        let b = emit_horizon(0x100, 0, 0, 0, 0, false, &flat);
        for band in &b.bands {
            assert_eq!(band.line.x0, 0);
            assert_eq!(band.line.x1, SCREEN_WIDTH);
            assert_eq!(band.line.y, band.quads[0].y_top);
            assert_eq!(band.line.code_colour, LINE_CODE_COLOUR);
        }
    }

    #[test]
    fn quad_texture_pages_and_uvs_are_the_retail_literals() {
        let b = emit_horizon(0x100, 0, 0, 0, 0, false, &flat);
        let band = &b.bands[0];
        assert_eq!(band.quads[0].tpage, 0x0100);
        assert_eq!(band.quads[1].tpage, 0x0103);
        assert_eq!(
            band.quads[0].uv,
            [(0x00, 1), (0xFF, 1), (0x00, 2), (0xFF, 2)]
        );
        assert_eq!(
            band.quads[1].uv,
            [(0x3F, 1), (0x7F, 1), (0x3F, 2), (0x7F, 2)]
        );
        assert_eq!(band.quads[0].code_colour, QUAD_CODE_COLOUR);
    }

    #[test]
    fn non_zero_trig_warps_the_extents_per_row() {
        // A row-varying sample must produce row-varying geometry, which is
        // what makes the plane appear to rotate.
        let ramp = |i: u16| -> i16 { (i & 0xFF) as i16 };
        let b = emit_horizon(0x800, 0, 0, 0, 0, false, &ramp);
        let widths: Vec<i16> = b
            .bands
            .iter()
            .map(|b| b.quads[0].x_right - b.quads[0].x_left)
            .collect();
        assert!(
            widths.windows(2).any(|w| w[0] != w[1]),
            "trig sample must modulate the band width"
        );
    }

    #[test]
    fn ot_layer_is_carried_through() {
        let b = emit_horizon(0x100, 0, 0, 0, 7, false, &flat);
        assert_eq!(b.ot_layer, 7);
    }
}

//! Landing a drawn frame back in the software PSX VRAM.
//!
//! On the console the framebuffer *is* VRAM: the display area is a rect inside
//! the same 1024x512 halfword page textures are read from, so a primitive can
//! sample pixels the GPU drew moments earlier. Retail leans on that for the
//! whole field-to-battle transition family - the curtain style
//! (`legaia_engine_vm::battle_intro_styles::tick_curtain`) slices a *captured
//! field frame* into 240 row strips and 320 column strips and stretches them
//! apart, and the strips are ordinary textured quads whose texture page is the
//! capture.
//!
//! The port's renderer draws through wgpu into a colour attachment that has no
//! relationship to [`legaia_tim::Vram`]: `Renderer::upload_vram` pushes the
//! software page *to* the GPU (`TEXTURE_BINDING | COPY_DST`) and nothing ever
//! comes back, so a frame the engine just drew was not addressable as a
//! texture. This module is that missing direction.
//!
//! # Where retail parks the capture
//!
//! The curtain's four texture-page words pin the destination without needing a
//! capture: `0x105` / `0x108` decode to 15-bpp pages at VRAM `(320, 0)` and
//! `(512, 0)`, and the column pass' `0x115` / `0x118` to `(320, 256)` and
//! `(512, 256)`. A page is 256 halfwords wide, and the row pass draws a
//! `0xC0`-wide strip from the first page followed by a `0x80`-wide strip from
//! the second: `0xC0 + 0x80 = 0x140`, so together they span VRAM columns
//! `320..=639` - one 320-pixel scanline. The capture is therefore a plain
//! 320x240 15-bpp image at `(320, 0)`, with a second copy at `(320, 256)`,
//! parked to the right of the two display buffers (which sit at `(0, 0)` and
//! `(0, 240)` - see `legaia_engine_vm::vram_rect_copy::BACK_BUFFER_Y_BIAS`).
//! [`FIELD_CAPTURE_ROWS`] / [`FIELD_CAPTURE_COLS`] name those two rects.
//!
//! # What the port does differently, and why it is still exact
//!
//! Retail captures at the native 320x240; the port renders at the window size
//! (960x720 by default), so [`blit_rgba_into_vram`] point-samples the readback
//! down into the destination rect. Nothing else is approximated: the engine's
//! colour pipeline is display-referred PSX framebuffer bytes end to end (see
//! `docs/subsystems/renderer.md` § "Colour space"), and the last stage of every
//! 3D shader expands a 5-bit channel as `(c5 << 3) | (c5 >> 2)`, so `byte >> 3`
//! recovers `c5` exactly. A dithered frame therefore round-trips into VRAM
//! bit-for-bit, and an undithered one takes the same 24->15 bit truncation the
//! PSX GPU applies on store.
//!
//! # The mask bit is a choice the caller has to make
//!
//! A 15-bpp texel of `0x0000` is *transparent* when sampled, and black
//! framebuffer pixels are exactly `0x0000`. Retail's draw environment decides
//! this with the GP0(E6) mask setting; here it is [`CaptureOpts::set_mask_bit`].
//! Leaving it set (the default) makes every captured pixel opaque, which is
//! what a full-screen transition wants; clearing it reproduces the
//! "black reads as a hole" behaviour a sprite capture relies on.

use legaia_tim::{VRAM_HEIGHT, VRAM_WIDTH, Vram};

/// Native PSX display width the engine's screen-space primitives are authored
/// in (every retail emitter clamps against `0x140`).
pub const PSX_SCREEN_WIDTH: u16 = 320;
/// Native PSX display height (`0xF0`).
pub const PSX_SCREEN_HEIGHT: u16 = 240;

/// A rectangle of the 1024x512 software VRAM page, in halfwords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VramRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl VramRect {
    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }

    /// `true` when the rect has a non-zero extent and lies wholly inside the
    /// 1024x512 page. A partially-outside rect is still writable - the blit
    /// clips - but a caller staging a texture page wants to know.
    pub fn fits_in_vram(&self) -> bool {
        self.w != 0
            && self.h != 0
            && (self.x as usize + self.w as usize) <= VRAM_WIDTH
            && (self.y as usize + self.h as usize) <= VRAM_HEIGHT
    }
}

/// The rect the curtain style's **row** pass samples (texture pages `0x105` /
/// `0x108`).
pub const FIELD_CAPTURE_ROWS: VramRect = VramRect::new(320, 0, PSX_SCREEN_WIDTH, PSX_SCREEN_HEIGHT);
/// The rect its **column** pass samples (texture pages `0x115` / `0x118`).
pub const FIELD_CAPTURE_COLS: VramRect =
    VramRect::new(320, 256, PSX_SCREEN_WIDTH, PSX_SCREEN_HEIGHT);

/// How a frame is quantised on its way into VRAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureOpts {
    /// Set bit 15 (the PSX mask / STP bit) on every written texel, so a black
    /// pixel samples as opaque black instead of as a transparent hole. See the
    /// module docs.
    pub set_mask_bit: bool,
}

impl Default for CaptureOpts {
    fn default() -> Self {
        Self { set_mask_bit: true }
    }
}

/// Quantise one display-referred RGB8 triple to a PSX BGR555 framebuffer word.
///
/// This is the PSX GPU's own 24-to-15-bit store: three independent `>> 3`
/// truncations, red in bits `0..=4`. `mask` becomes bit 15.
pub fn rgb8_to_bgr555(r: u8, g: u8, b: u8, mask: bool) -> u16 {
    let r5 = (r >> 3) as u16;
    let g5 = (g >> 3) as u16;
    let b5 = (b >> 3) as u16;
    r5 | (g5 << 5) | (b5 << 10) | if mask { 0x8000 } else { 0 }
}

/// Point-sample an RGBA8 frame (`src_w` x `src_h`, row-major, no padding) into
/// `dst` inside `vram`, quantising each pixel with [`rgb8_to_bgr555`].
///
/// Returns the number of VRAM cells written. Rows and columns that fall
/// outside the page are skipped (`Vram::write_block` clips), and a zero-extent
/// source or destination writes nothing.
///
/// Sampling is nearest-neighbour with the source index taken as
/// `col * src_w / dst.w`, which is an identity map when the two agree - so a
/// capture at the native 320x240 is not resampled at all.
pub fn blit_rgba_into_vram(
    rgba: &[u8],
    src_w: u32,
    src_h: u32,
    vram: &mut Vram,
    dst: VramRect,
    opts: CaptureOpts,
) -> usize {
    if dst.w == 0 || dst.h == 0 || src_w == 0 || src_h == 0 {
        return 0;
    }
    let needed = (src_w as usize) * (src_h as usize) * 4;
    if rgba.len() < needed {
        return 0;
    }
    let mut words = Vec::with_capacity(dst.w as usize * dst.h as usize);
    for row in 0..dst.h as u32 {
        let sy = (row * src_h / dst.h as u32).min(src_h - 1) as usize;
        for col in 0..dst.w as u32 {
            let sx = (col * src_w / dst.w as u32).min(src_w - 1) as usize;
            let off = (sy * src_w as usize + sx) * 4;
            words.push(rgb8_to_bgr555(
                rgba[off],
                rgba[off + 1],
                rgba[off + 2],
                opts.set_mask_bit,
            ));
        }
    }
    vram.write_block(dst.x, dst.y, dst.w, dst.h, bytemuck::cast_slice(&words));
    words.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
        px.iter()
            .cycle()
            .take((w * h * 4) as usize)
            .copied()
            .collect()
    }

    #[test]
    fn bgr555_is_the_gpu_truncation_with_red_in_the_low_bits() {
        assert_eq!(rgb8_to_bgr555(0xFF, 0, 0, false), 0x001F);
        assert_eq!(rgb8_to_bgr555(0, 0xFF, 0, false), 0x03E0);
        assert_eq!(rgb8_to_bgr555(0, 0, 0xFF, false), 0x7C00);
        assert_eq!(rgb8_to_bgr555(0, 0, 0, true), 0x8000);
        // The low three bits of each byte are dropped, not rounded.
        assert_eq!(rgb8_to_bgr555(0x07, 0x07, 0x07, false), 0x0000);
        assert_eq!(rgb8_to_bgr555(0x08, 0, 0, false), 0x0001);
    }

    #[test]
    fn a_dithered_channel_round_trips_bit_exactly() {
        // The 3D shaders' final stage expands a 5-bit channel as
        // `(c5 << 3) | (c5 >> 2)`; every one of the 32 values must come back
        // out of the capture unchanged, or a captured frame is not the frame
        // that was drawn.
        for c5 in 0u16..32 {
            let byte = ((c5 << 3) | (c5 >> 2)) as u8;
            let word = rgb8_to_bgr555(byte, byte, byte, false);
            assert_eq!(word & 0x1F, c5, "r c5={c5}");
            assert_eq!((word >> 5) & 0x1F, c5, "g c5={c5}");
            assert_eq!((word >> 10) & 0x1F, c5, "b c5={c5}");
        }
    }

    #[test]
    fn a_native_resolution_capture_is_not_resampled() {
        // 4x2 gradient, captured 1:1 - every destination cell must be its own
        // source pixel, in order.
        let mut rgba = Vec::new();
        for i in 0..8u8 {
            rgba.extend_from_slice(&[i << 3, 0, 0, 255]);
        }
        let mut vram = Vram::new();
        let dst = VramRect::new(320, 0, 4, 2);
        assert_eq!(
            blit_rgba_into_vram(&rgba, 4, 2, &mut vram, dst, CaptureOpts::default()),
            8
        );
        for i in 0..8usize {
            let (x, y) = (320 + i % 4, i / 4);
            assert_eq!(vram.pixel(x, y), 0x8000 | i as u16, "cell {i}");
        }
    }

    #[test]
    fn a_window_sized_frame_point_samples_down_to_the_psx_rect() {
        // 960x720 (the play window's default) into the retail 320x240 rect:
        // every third source pixel survives, and the rect is fully written.
        let src = solid(960, 720, [0xFF, 0xFF, 0xFF, 0xFF]);
        let mut vram = Vram::new();
        let written = blit_rgba_into_vram(
            &src,
            960,
            720,
            &mut vram,
            FIELD_CAPTURE_ROWS,
            CaptureOpts::default(),
        );
        assert_eq!(written, 320 * 240);
        assert_eq!(vram.pixel(320, 0), 0xFFFF);
        assert_eq!(vram.pixel(639, 239), 0xFFFF);
        // Nothing spilled into the display buffer to its left, or past the
        // bottom edge.
        assert_eq!(vram.pixel(319, 0), 0);
        assert_eq!(vram.pixel(320, 240), 0);
    }

    #[test]
    fn the_mask_bit_decides_whether_black_samples_as_a_hole() {
        let black = solid(2, 2, [0, 0, 0, 255]);
        let dst = VramRect::new(0, 0, 2, 2);

        let mut opaque = Vram::new();
        blit_rgba_into_vram(&black, 2, 2, &mut opaque, dst, CaptureOpts::default());
        assert_eq!(opaque.pixel(0, 0), 0x8000);
        assert!(opaque.region_has_data(0, 0, 2, 2));

        let mut holed = Vram::new();
        blit_rgba_into_vram(
            &black,
            2,
            2,
            &mut holed,
            dst,
            CaptureOpts {
                set_mask_bit: false,
            },
        );
        assert_eq!(holed.pixel(0, 0), 0x0000);
        // 0x0000 is the codebase-wide "unpopulated" word, so an unmasked black
        // capture is indistinguishable from never having been written.
        assert!(!holed.region_has_data(0, 0, 2, 2));
    }

    #[test]
    fn a_short_or_empty_source_writes_nothing() {
        let mut vram = Vram::new();
        let dst = VramRect::new(0, 0, 4, 4);
        assert_eq!(
            blit_rgba_into_vram(&[], 0, 0, &mut vram, dst, CaptureOpts::default()),
            0
        );
        // Claims 4x4 but only carries 3 pixels.
        let short = vec![0xFFu8; 12];
        assert_eq!(
            blit_rgba_into_vram(&short, 4, 4, &mut vram, dst, CaptureOpts::default()),
            0
        );
        assert!(!vram.region_has_data(0, 0, 4, 4));
    }

    #[test]
    fn the_retail_capture_rects_fit_and_clear_the_display_buffers() {
        for r in [FIELD_CAPTURE_ROWS, FIELD_CAPTURE_COLS] {
            assert!(r.fits_in_vram(), "{r:?}");
            // Both start past the 320-wide display buffers at x = 0.
            assert!(r.x >= PSX_SCREEN_WIDTH);
        }
        // The row capture stops short of the column capture, so the two do
        // not overlap.
        const {
            assert!(FIELD_CAPTURE_ROWS.y + FIELD_CAPTURE_ROWS.h <= FIELD_CAPTURE_COLS.y);
        }
    }

    #[test]
    fn a_rect_running_off_the_page_is_rejected_by_fits_but_still_clips() {
        let off = VramRect::new(1020, 510, 8, 8);
        assert!(!off.fits_in_vram());
        let mut vram = Vram::new();
        let src = solid(8, 8, [0xFF, 0xFF, 0xFF, 0xFF]);
        blit_rgba_into_vram(&src, 8, 8, &mut vram, off, CaptureOpts::default());
        // The in-page corner landed; the rest was dropped rather than wrapping
        // onto the next row.
        assert_eq!(vram.pixel(1020, 510), 0xFFFF);
        assert_eq!(vram.pixel(0, 511), 0);
    }
}

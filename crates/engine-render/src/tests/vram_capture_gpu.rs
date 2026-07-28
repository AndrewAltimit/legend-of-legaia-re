//! End-to-end proof that a frame the renderer *drew* lands back in the
//! software PSX VRAM and is then samplable as a texture page.
//!
//! The chain a transition needs is: draw -> read back -> quantise into
//! [`legaia_tim::Vram`] -> re-upload -> sample. `Renderer::capture_into_vram`
//! is `capture_rgba` plus [`crate::vram_capture::blit_rgba_into_vram`], and
//! `Renderer` cannot be built without a window - so this drives the same two
//! halves through the headless harness instead: the real
//! `SCREEN_OVERLAY_SHADER_SRC` pipeline draws the frame, the readback goes
//! through the real blit, and the result is read out of the real `Vram`.
//!
//! Skips (passes vacuously) with no GPU adapter, like its sibling.

use super::*;
use crate::screen_overlay::{FlatQuad, ScreenPrim, ScreenQuad};
use crate::tests::screen_overlay_gpu::{TARGET, build_harness, headless_device, render_frame_rgba};
use crate::vram_capture::{CaptureOpts, VramRect, blit_rgba_into_vram, rgb8_to_bgr555};
use legaia_tim::Vram;

fn full_quad() -> [(i16, i16); 4] {
    [
        (0, 0),
        (TARGET as i16, 0),
        (0, TARGET as i16),
        (TARGET as i16, TARGET as i16),
    ]
}

#[test]
fn a_drawn_frame_lands_in_vram_as_the_colour_that_was_drawn() {
    let Some((device, queue)) = headless_device() else {
        eprintln!(
            "no GPU adapter; skipping a_drawn_frame_lands_in_vram_as_the_colour_that_was_drawn"
        );
        return;
    };
    let h = build_harness(device, queue);

    // Draw a solid quad in a colour that is exactly representable in 5 bits
    // per channel, so the capture must reproduce it with no rounding slack:
    // c5 = 16 expands to byte 132 (see `psx_dither`).
    let byte = ((16u16 << 3) | (16u16 >> 2)) as u8;
    let frame = render_frame_rgba(
        &h,
        &[ScreenPrim::Flat(FlatQuad {
            xy: full_quad(),
            color: [byte, byte, byte, 255],
            semi_transparent: false,
            abr_mode: 0,
            ot_index: 10,
        })],
        [0.0, 0.0, 0.0, 1.0],
    );

    let mut vram = Vram::new();
    let dst = VramRect::new(320, 0, TARGET as u16, TARGET as u16);
    let written = blit_rgba_into_vram(
        &frame,
        TARGET as u32,
        TARGET as u32,
        &mut vram,
        dst,
        CaptureOpts::default(),
    );
    assert_eq!(written, TARGET * TARGET);

    let expected = rgb8_to_bgr555(byte, byte, byte, true);
    assert_eq!(expected, 0x8000 | 16 | (16 << 5) | (16 << 10));
    for y in 0..TARGET {
        for x in 0..TARGET {
            assert_eq!(
                vram.pixel(320 + x, y),
                expected,
                "captured cell ({x}, {y}) is not the drawn colour"
            );
        }
    }
    // And the capture is a *populated* region as far as every VRAM consumer
    // is concerned - which is what makes it usable as a texture page.
    assert!(vram.region_has_data(320, 0, TARGET, TARGET));
    assert!(!vram.region_has_data(0, 0, TARGET, TARGET));
}

#[test]
fn the_captured_page_reads_back_through_the_15bpp_texture_path() {
    let Some((device, queue)) = headless_device() else {
        eprintln!(
            "no GPU adapter; skipping the_captured_page_reads_back_through_the_15bpp_texture_path"
        );
        return;
    };

    // Capture a red frame, then draw a quad that samples the captured page
    // through the real shader's 15-bpp branch. This is the curtain's whole
    // move: the strips' texture page IS the capture.
    let h = build_harness(device, queue);
    let frame = render_frame_rgba(
        &h,
        &[ScreenPrim::Flat(FlatQuad {
            xy: full_quad(),
            color: [255, 0, 0, 255],
            semi_transparent: false,
            abr_mode: 0,
            ot_index: 10,
        })],
        [0.0, 0.0, 0.0, 1.0],
    );

    // The harness' VRAM is 64x64, so land the capture inside texture page 0
    // at (5, 5) - the texel the harness' own textured case samples.
    let mut vram = Vram::new();
    blit_rgba_into_vram(
        &frame,
        TARGET as u32,
        TARGET as u32,
        &mut vram,
        VramRect::new(5, 5, TARGET as u16, TARGET as u16),
        CaptureOpts::default(),
    );
    assert_eq!(vram.pixel(5, 5), 0x801F);

    // Rebuild the harness with that captured texel in place and sample it.
    let Some((device2, queue2)) = headless_device() else {
        return;
    };
    let h2 = crate::tests::screen_overlay_gpu::build_harness_with(
        device2,
        queue2,
        wgpu::TextureFormat::Rgba8Unorm,
        vram.pixel(5, 5),
    );
    let px = crate::tests::screen_overlay_gpu::render_center_pixel(
        &h2,
        &[ScreenPrim::Textured(ScreenQuad {
            xy: full_quad(),
            uv: [(5, 5); 4],
            clut: 0,
            tpage: 2 << 7, // 15bpp
            color: 0x0080_8080,
            gouraud: None,
            semi_transparent: false,
            ot_index: 10,
        })],
        [0.0, 0.0, 0.0, 1.0],
    );
    assert!(
        px[0] > 200 && px[1] < 40 && px[2] < 40,
        "captured red must survive the round trip through the texture path: {px:?}"
    );
}

#[test]
fn a_gouraud_quad_gradients_across_the_frame_on_gpu() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping a_gouraud_quad_gradients_across_the_frame_on_gpu");
        return;
    };
    let h = build_harness(device, queue);

    // A flat quad cannot express the transition styles' top-edge/bottom-edge
    // gradient. Draw one whose top corners are black and bottom corners full
    // brightness, and check the shader really interpolates: the top row must
    // come out darker than the bottom row.
    let frame = render_frame_rgba(
        &h,
        &[ScreenPrim::Textured(ScreenQuad {
            xy: full_quad(),
            uv: [(5, 5); 4],
            clut: 0,
            tpage: 2 << 7,
            color: 0x00FF_00FF, // ignored when `gouraud` is set
            gouraud: Some([0, 0, 0x0080_8080, 0x0080_8080]),
            semi_transparent: false,
            ot_index: 10,
        })],
        [0.0, 0.0, 0.0, 1.0],
    );
    let row = |y: usize| frame[(y * TARGET) * 4] as u32;
    assert!(
        row(TARGET - 1) > row(0) + 40,
        "gouraud modulation must ramp down the quad: top={} bottom={}",
        row(0),
        row(TARGET - 1)
    );
}

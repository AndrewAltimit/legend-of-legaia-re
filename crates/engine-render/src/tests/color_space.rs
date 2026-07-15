//! The engine renders in **PSX framebuffer space**: a colour that reaches a
//! shader's `return` is the byte the console would clock out to the display,
//! and no stage of the pipeline may re-encode it.
//!
//! That invariant is easy to break silently. The renderer used to present
//! through an sRGB swapchain, which treats each shader output as *linear* and
//! applies the linear->sRGB transfer on store: every midtone came out lifted
//! (retail's mid-grey `132` presented as `190`), the 5-bit quantisation
//! [`psx_dither`](crate::psx_dither) exists to model was rounded straight back
//! out, and the PSX semi-transparency blends ran in linear space where retail
//! blends raw 5-bit values. The symptom was washed-out, over-bright scenes -
//! and, because it was global, it had been papered over locally: the status
//! screen's ink was stored pre-linearised (`srgb_to_linear(206/255)`) while
//! the options screen's *same* retail ink was stored raw, so the two presented
//! at different brightnesses.
//!
//! These tests pin both halves: the attachment is never sRGB
//! (`choose_surface_format`), and a known VRAM texel presents at the byte
//! retail puts on the wire. The second is checked against an sRGB target too,
//! so it fails loudly if the policy ever regresses rather than passing
//! vacuously.

use super::*;
use crate::renderer::choose_surface_format;
use crate::screen_overlay::{ScreenPrim, ScreenQuad};
use wgpu::TextureFormat as Tf;

/// PSX 5-bit -> 8-bit expansion: `(c5 << 3) | (c5 >> 2)`. What the console
/// puts on the wire for a BGR555 component.
fn psx_5_to_8(c5: u32) -> u8 {
    ((c5 << 3) | (c5 >> 2)) as u8
}

#[test]
fn presented_attachment_is_never_srgb() {
    // Typical desktop swapchain: both twins offered, sRGB listed first.
    let (fmt, view) = choose_surface_format(&[Tf::Bgra8UnormSrgb, Tf::Bgra8Unorm]);
    assert_eq!(fmt, Tf::Bgra8Unorm, "prefer the UNORM surface outright");
    assert_eq!(view, Tf::Bgra8Unorm);

    // Browser/canvas swapchain: only UNORM offered.
    let (fmt, view) = choose_surface_format(&[Tf::Bgra8Unorm, Tf::Rgba8Unorm]);
    assert_eq!(fmt, Tf::Bgra8Unorm);
    assert_eq!(view, Tf::Bgra8Unorm);

    // Degenerate surface that offers nothing but sRGB: keep the surface
    // format (it's all we may configure) but render through the UNORM twin.
    let (fmt, view) = choose_surface_format(&[Tf::Rgba8UnormSrgb]);
    assert_eq!(fmt, Tf::Rgba8UnormSrgb);
    assert_eq!(view, Tf::Rgba8Unorm, "render through the UNORM twin");

    // The invariant that actually matters, over every surface we might meet.
    for offered in [
        vec![Tf::Bgra8UnormSrgb, Tf::Bgra8Unorm],
        vec![Tf::Bgra8Unorm],
        vec![Tf::Rgba8UnormSrgb],
        vec![Tf::Rgba8UnormSrgb, Tf::Rgba8Unorm, Tf::Rgba16Float],
    ] {
        let (_, view) = choose_surface_format(&offered);
        assert!(
            !view.is_srgb(),
            "shaders emit PSX framebuffer bytes; the attachment must not \
             re-encode them (offered {offered:?} -> view {view:?})"
        );
    }
}

/// A 15bpp VRAM texel presents at the exact byte retail would output - and
/// the same texel through an sRGB attachment does not. Renders the *real*
/// overlay shader (neutral `0x80` modulation = pass-through) via the
/// `screen_overlay_gpu` harness.
#[test]
fn psx_texel_presents_at_its_retail_byte() {
    let Some((device, queue)) = super::screen_overlay_gpu::headless_device() else {
        eprintln!("no GPU adapter; skipping psx_texel_presents_at_its_retail_byte");
        return;
    };

    // Mid-grey: 5-bit 16 in each channel -> retail byte 132. The most
    // diagnostic value there is; the sRGB lift is worst at the midtones.
    const C5: u32 = 16;
    let expected = psx_5_to_8(C5); // 132
    let word = (C5 | (C5 << 5) | (C5 << 10)) as u16;

    let quad = |target: usize| {
        ScreenPrim::Textured(ScreenQuad {
            xy: [
                (0, 0),
                (target as i16, 0),
                (0, target as i16),
                (target as i16, target as i16),
            ],
            uv: [(5, 5); 4],
            clut: 0,
            tpage: 2 << 7,      // 15bpp
            color: 0x0080_8080, // neutral /128 modulation: texel passes through
            semi_transparent: false,
            ot_index: 10,
        })
    };

    // The pipeline as configured: UNORM attachment.
    let h = super::screen_overlay_gpu::build_harness_with(device, queue, Tf::Rgba8Unorm, word);
    let px = super::screen_overlay_gpu::render_center_pixel(
        &h,
        &[quad(super::screen_overlay_gpu::TARGET)],
        [0.0, 0.0, 0.0, 1.0],
    );
    for (i, c) in px[..3].iter().enumerate() {
        // +/-1: the shader reconstructs the component as c5/31 and the UNORM
        // conversion rounds, which lands within one of the console's
        // bit-replication expansion.
        assert!(
            c.abs_diff(expected) <= 1,
            "channel {i} presented {c}, retail puts {expected} on the wire \
             (whole pixel {px:?}) - something on the path is converting \
             colour spaces"
        );
    }

    // Non-vacuity: the attachment we deliberately do NOT use lifts that same
    // texel far out of tolerance. This is the bug the policy prevents.
    let Some((device, queue)) = super::screen_overlay_gpu::headless_device() else {
        return;
    };
    let h_srgb =
        super::screen_overlay_gpu::build_harness_with(device, queue, Tf::Rgba8UnormSrgb, word);
    let px_srgb = super::screen_overlay_gpu::render_center_pixel(
        &h_srgb,
        &[quad(super::screen_overlay_gpu::TARGET)],
        [0.0, 0.0, 0.0, 1.0],
    );
    eprintln!(
        "[color-space] BGR555 5-bit {C5}: retail wire byte {expected}, \
         UNORM attachment {}, sRGB attachment {} (the wash-out)",
        px[0], px_srgb[0]
    );
    assert!(
        px_srgb[0] > expected + 40,
        "an sRGB attachment should visibly lift the midtone (that's the \
         wash-out this policy exists to prevent), but it presented \
         {px_srgb:?} against retail's {expected}"
    );
}

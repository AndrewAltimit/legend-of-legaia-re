//! The field-to-battle transition emitter's renderer-bound half.
//!
//! The emitter itself - working sets, projection, packet builders, the
//! curtain's CPU two-pass composition - is wgpu-free and lives in
//! [`legaia_render_kernels::battle_intro`], where the browser play page can link
//! it.
//!
//! What cannot move is the way the native window obtains the field frame the
//! styles sample: [`update_field_capture`] re-renders the host's frame
//! offscreen through [`crate::Renderer::capture_scene_rgba`] and hands the
//! readback to the shared [`BattleIntro::land_capture_rgba`] /
//! [`BattleIntro::refresh_captured_page`] pair. The target is the one the
//! host presents - its screen layers and every enhancement pass included -
//! less the transition's own primitives. (The play page's equivalent is a
//! `gl.readPixels` of its own finished frame - each host owns its readback,
//! neither owns the emitter.)

use legaia_render_kernels::battle_intro::*;

/// Bring the transition's private VRAM page up to date for this frame, and
/// say whether the host has to re-upload it.
///
/// Two jobs, and only the first is a one-shot: on the frame the emitter arms,
/// the drawn field frame is read back ([`crate::Renderer::capture_rgba`]) and
/// landed via [`BattleIntro::land_capture_rgba`]; every frame after that,
/// [`BattleIntro::refresh_captured_page`] advances the curtain's CPU two-pass
/// composition. Returns `Some(page)` on any frame whose contents changed -
/// the capture frame for every style, and every frame for the curtain.
///
/// `on_capture` sees the readback on the one frame it is taken (the
/// `LEGAIA_DIAG_INTRO_CAPTURE` dump hangs off it).
pub fn update_field_capture<'a>(
    intro: &'a mut BattleIntro,
    renderer: &crate::Renderer,
    target: crate::RenderTarget<'_>,
    base: &legaia_tim::Vram,
    on_capture: impl FnOnce(&crate::CaptureImage),
) -> anyhow::Result<Option<&'a legaia_tim::Vram>> {
    if intro.needs_capture() {
        let img = renderer.capture_scene_rgba(target)?;
        on_capture(&img);
        intro.land_capture_rgba(&img.rgba, img.width, img.height, base);
    }
    Ok(intro.refresh_captured_page())
}

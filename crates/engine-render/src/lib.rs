//! Minimal wgpu renderer for the Phase 1 asset viewer.
//!
//! PORT: FUN_80034b78, FUN_80034e4c, FUN_8002C69C, FUN_8002C488, FUN_8002B994, FUN_8003C310
//! PORT: FUN_80031D00, FUN_800337B0, FUN_80035CB8, FUN_80035DA0, FUN_80035E44, FUN_800349EC, FUN_80035EA8
//! PORT: FUN_8003C1F8 (per-glyph dialog-font sprite emit; the engine renders
//! in-game proportional dialog glyphs via the legaia-font atlas + textured-quad
//! overlay instead of the retail GP0 cell-UV push)
//!
//! Owns a wgpu device + surface, plus two render pipelines:
//!
//! * **Textured-quad** (Phase 1 TIM viewer) - `upload_texture` +
//!   `render(RenderTarget::Texture(...))`. Letterbox-preserves aspect ratio.
//! * **Flat-shaded mesh** (Phase 1 TMD viewer) - `upload_mesh` +
//!   `render(RenderTarget::Mesh { ... })`. Lit by a single directional
//!   light, depth-tested. Uses the `glam::Mat4` MVP supplied per-frame so
//!   the host can spin the model without re-uploading.
//!
//! Both pipelines share the same surface + depth attachment. PSX-faithful
//! rasterisation (affine UV warp, sub-pixel vertex jitter, 15-bit ordered
//! dithering) is opt-in via [`Renderer::set_psx_mode`]; GTE emulation and
//! batched draws are future phases.
//! REF: FUN_801D0148, FUN_801D5DE0, FUN_801D84D0, FUN_801E08D8, FUN_801E1C1C, FUN_801E36C4
//! REF: FUN_801E3EE0, FUN_801E3FF0

pub mod actor_cull;
pub mod afterimage;
pub mod billboard;
pub mod gte;
pub mod gte_trace;
pub mod window;

pub use glam;
pub use legaia_font;
pub use legaia_tim;
pub use wgpu;

// The pure, wgpu-free UI draw-list layer lives in `legaia-engine-ui`. Re-export
// every item (`TextDraw`, `SpriteDraw`, `SpriteRequest`, and all the
// `*_draws_for` / view-struct builders) at its historical crate-root path so
// native shell code, the asset-viewer, and tests compile unchanged.
pub use legaia_engine_ui::*;

pub mod dyn_light;
pub mod profile;
pub mod psx_blend;
pub mod psx_dither;
pub mod psx_light;
mod renderer;
pub mod screen_overlay;
mod shaders;

pub use renderer::*;

/// Batch of [`TextDraw`]s to render in one pass against a shared font atlas.
/// Cheap to construct each frame; the renderer copies the geometry into a
/// reusable dynamic buffer before drawing.
pub struct TextOverlay<'a> {
    pub atlas: &'a UploadedFontAtlas,
    pub draws: &'a [TextDraw],
}

/// GPU-resident aliases of the moved [`SpriteDraw`] / [`TextOverlay`] shapes.
/// [`SpriteDraw`] itself lives in `legaia-engine-ui`; these two hold a wgpu
/// [`UploadedFontAtlas`] handle so they stay in this crate.
pub type UploadedSpriteAtlas = UploadedFontAtlas;
pub type SpriteOverlay<'a> = TextOverlay<'a>;

#[cfg(test)]
mod tests;

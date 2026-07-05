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

pub mod afterimage;
pub mod billboard;
pub mod gte;
pub mod gte_trace;
pub mod window;

pub use glam;
pub use legaia_font;
pub use legaia_tim;
pub use wgpu;

pub mod psx_blend;
pub mod psx_dither;
mod renderer;
pub mod screen_overlay;
mod shaders;
mod ui_menu;
mod ui_overlay;
mod ui_title_save;

pub use renderer::*;
pub use ui_menu::*;
pub use ui_overlay::*;
pub use ui_title_save::*;

/// One textured quad in screen space. Coordinates are pixel-space relative
/// to the top-left of the surface; the renderer converts to NDC at draw
/// time. Atlas coordinates are pixel-space inside the source font atlas.
#[derive(Debug, Clone, Copy)]
pub struct TextDraw {
    /// Destination rectangle: `(x, y, w, h)` in surface pixels.
    pub dst: (i32, i32, u32, u32),
    /// Source rectangle: `(x, y, w, h)` in atlas pixels.
    pub src: (u32, u32, u32, u32),
    /// RGBA tint multiplied with the sampled atlas texel.
    pub color: [f32; 4],
}

/// Batch of [`TextDraw`]s to render in one pass against a shared font atlas.
/// Cheap to construct each frame; the renderer copies the geometry into a
/// reusable dynamic buffer before drawing.
pub struct TextOverlay<'a> {
    pub atlas: &'a UploadedFontAtlas,
    pub draws: &'a [TextDraw],
}

/// Sprite types are semantic aliases of the text-quad types - both are
/// just textured quads sampled with nearest-neighbour filtering and alpha
/// blending. Sharing the GPU pipeline keeps engine-render small; the
/// distinct names keep call-sites readable.
pub type SpriteDraw = TextDraw;
pub type UploadedSpriteAtlas = UploadedFontAtlas;
pub type SpriteOverlay<'a> = TextOverlay<'a>;

/// One sprite request emitted by the World→sprite-batch glue. Holds the
/// renderer-agnostic shape of "draw glyph from this atlas page at this
/// world pixel position" so engine-core (which doesn't link wgpu) can
/// produce them and engine-render can consume them.
///
/// Engines that want richer per-sprite state (subpixel offset, rotation,
/// scale) should branch off this type - it's intentionally minimal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteRequest {
    /// Top-left of the sprite in world / screen pixels.
    pub world_x: i32,
    pub world_y: i32,
    /// Atlas source rect: `(x, y, w, h)` in atlas pixels.
    pub atlas_src: (u32, u32, u32, u32),
    /// RGBA tint multiplied with the sampled atlas texel.
    pub color: [f32; 4],
}

#[cfg(test)]
mod tests;

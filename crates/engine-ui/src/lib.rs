//! Pure, renderer-agnostic UI draw-list builders for the Legaia engine port.
//!
//! This crate holds the wgpu-free layer of the in-game UI: every function here
//! projects a renderer-agnostic *view* struct (built by the shell from the live
//! [`World`](../../engine-core)) into a `Vec` of [`TextDraw`] / [`SpriteDraw`]
//! primitives - screen rectangles plus a font-atlas or VRAM-sprite source. A
//! host renderer (native wgpu in `legaia-engine-render`, or the WebGL play page
//! in `legaia-web-viewer`) consumes those primitives; neither the geometry nor
//! the navigation logic depends on the GPU backend.
//!
//! Modules:
//! * [`ui_overlay`] - dialog box, cutscene narration, battle HUD, encounter
//!   banner, stage-scale text, per-glyph sprite emit helpers.
//! * [`ui_fishing`] - fishing-minigame HUD: the ported draw-list layout plus
//!   the consumer that renders it.
//! * [`ui_menu`] - pause-menu field/status/spell/inventory/equipment panels,
//!   options + key-rebind, name entry, game-over, tactical-arts editor.
//! * [`ui_title_save`] - title menu, 9-slice window chrome, save-select,
//!   save-slot grid + info panel, "Now checking" dialog.
//!
//! Extracted from `legaia-engine-render`, which re-exports every item here at
//! its old path so native code + tests compile unchanged. The GPU-resident
//! batch wrappers ([`TextOverlay`]/`SpriteOverlay`/`UploadedSpriteAtlas`) stay
//! in `legaia-engine-render` because they hold wgpu handles.
//!
//! [`TextOverlay`]: https://docs.rs/legaia-engine-render

pub use legaia_font;
pub use legaia_tim;

pub mod battle_name_banner;
mod ui_fishing;
mod ui_menu;
mod ui_overlay;
mod ui_title_save;

pub use ui_fishing::*;
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

/// Sprite draws are a semantic alias of the text-quad type - both are just
/// textured quads sampled with nearest-neighbour filtering and alpha
/// blending. Sharing the shape keeps call-sites readable while a single host
/// pipeline services both.
pub type SpriteDraw = TextDraw;

/// One sprite request emitted by the World->sprite-batch glue. Holds the
/// renderer-agnostic shape of "draw glyph from this atlas page at this
/// world pixel position" so engine-core (which doesn't link wgpu) can
/// produce them and a host renderer can consume them.
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

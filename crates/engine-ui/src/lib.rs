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
//! * `battle_hud` (items at the crate root) - sprite / text batching, shop
//!   rows, level-up and capture banners, the battle HUD, encounter banner,
//!   stage-scale text, per-glyph sprite emit helpers. The dialog and
//!   notice boxes sit beside it in their own root-level modules.
//! * [`battle_command_ui`] - the battle command menu's packet-pinned chip
//!   cluster: the plate 3-slice, the four-arm diamond and its D-pad glyph,
//!   and the `-` chip an unavailable command draws.
//! * [`battle_hud_chrome`] - the battle surfaces that are widget-table
//!   records rather than plate runs: the class-0 frame of the message
//!   banner and the intro enemy-name labels (a marbled fill under a gold
//!   border; the banner shares its seat with the actor-name plaque) and the
//!   status / element badge cells.
//! * [`fishing`] - fishing-minigame HUD: the ported draw-list layout plus
//!   the consumer that renders it.
//! * [`menu`] - pause-menu field/status/spell/inventory/equipment panels,
//!   options + key-rebind, name entry, game-over, tactical-arts editor.
//! * [`menu::window_painters`] - content painters for the menu-overlay
//!   window-descriptor table's prompt / counter / tab / shop-panel block, and
//!   [`menu::equip_stat_compare`] for the equip screen's two stat-compare
//!   painters (descriptor ids 25 and 41).
//! * [`menu::window_dispatch`] - which painter draws a given descriptor,
//!   keyed on its `renderer_va` the way the retail window walker keys on the
//!   live window's `+0x28`.
//! * `title_save` (items at the crate root) - title menu, 9-slice window
//!   chrome, save-select, save-slot grid + info panel, "Now checking" dialog.
//! * [`minigame_fx`] - the minigame effect-part pool's draw builder (the
//!   pool itself is `legaia_engine_minigames::minigame_fx`) plus the dance run's
//!   own sprite-part emit, both shared by every host.
//!
//! The wgpu-free render kernels both hosts share - fixed-point GTE
//! arithmetic, the screen-space PSX primitive record with its OT sort and
//! vertex builder, the RGBA-to-BGR555 frame blit, the field-to-battle
//! transition emitter - are `legaia-render-kernels`, one crate down. The
//! GPU-resident batch wrappers ([`TextOverlay`]/`SpriteOverlay`/
//! `UploadedSpriteAtlas`) are in `legaia-engine-render` because they hold
//! wgpu handles.
//!
//! [`TextOverlay`]: https://docs.rs/legaia-engine-render

pub mod arts_input;
pub mod baka_strips;
pub mod battle_chrome;
pub mod battle_command_ui;
mod battle_hud;
pub mod battle_hud_chrome;
pub mod battle_item_ui;
pub mod battle_name_banner;
pub mod battle_numerals;
pub mod battle_stage_clear;
mod battle_timed_fight_strip;
mod battle_tutorial_box;
pub mod boot_logos;
pub mod cast_theeder;
pub mod dance;
mod dialog_reading_box;
pub mod field_party_hud;
pub mod fishing;
mod incense_notice_box;
pub mod menu;
pub mod minigame_fx;
pub mod muscle_dome_hud;
pub mod pause_menu;
pub mod prize_exchange;
pub mod ringside_backdrop;
pub mod shop_screen;
pub mod slot_cabinet;
pub mod slot_paylines;
mod text_balloon_box;
pub mod text_lines;
mod tile_board_prompt;
mod title_save;
mod world_map_labels;

pub use battle_hud::*;
pub use battle_timed_fight_strip::*;
pub use battle_tutorial_box::*;
pub use dialog_reading_box::*;
pub use fishing::hud::*;
pub use incense_notice_box::*;
pub use menu::equip_stat_compare::*;
pub use menu::screens::*;
pub use menu::window_dispatch::*;
pub use text_balloon_box::*;
pub use tile_board_prompt::*;
pub use title_save::*;
pub use world_map_labels::*;

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

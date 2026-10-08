//! The shop / prize-exchange / inn / banner **screens** both play hosts draw,
//! composed once.
//!
//! `engine-ui` holds the renderer-agnostic builders and deliberately does not
//! link `engine-core`; `engine-core` holds the state and the retail kernels
//! and draws nothing. Something has to project the one into the other, and
//! that projection used to live twice - once in the native window
//! (`engine-shell/src/window/`) and once on the browser play page
//! (`web-viewer/src/play_shop.rs`), line for line. This crate is that
//! projection: [`ScreenInputs`] in, [`ShopOverlayFrame`] out.
//!
//! What stays with a host: assembling [`ScreenInputs`] from its own holders,
//! the stage-to-surface scale of the stage texts, its layer order, and its
//! upload (wgpu on the native window, JSON on the page).
//!
//! Every text this crate returns is in the retail 320x240 **stage**; every
//! sprite is in surface pixels for the `surface_w x surface_h` the caller
//! passed.

use legaia_asset::menu_windows::MenuWindowTable;
use legaia_engine_core::field_submode_screen::SubmodeLine;
use legaia_engine_core::menu_runtime::MenuRuntime;
use legaia_engine_core::world::World;
use legaia_engine_ui::{SaveMenuAtlasRects, SpriteDraw, TextDraw};

mod banners;
pub mod field_frame;
mod panel;
mod prize;
mod shop;

pub use banners::banner_stage_draws;
pub use panel::{FallbackPanel, fallback_panel_draws, panel_frame_rect};
pub use prize::{coin_counter_window_draws, prize_window_draws};
pub use shop::{
    ShopWindowDraws, gold_shop_screen, recipient_window_draws, sell_detail_window_draws,
    shop_staged_item, shop_vendor_name, shop_window_draws,
};

/// Stage pen of the field shop / inn panel (`shop_draws_for`'s `pen`), and
/// the anchor its plain-text stand-in lines share.
pub const SHOP_PEN: (i32, i32) = (8, 140);
/// Stage pen of the chrome-less post-battle level-up banner
/// (`level_up_draws_for`).
pub const LEVEL_UP_PEN: (i32, i32) = (8, 60);
/// Stage pen of the chrome-less Seru-capture banner
/// (`capture_banner_draws_for`).
pub const CAPTURE_PEN: (i32, i32) = (8, 40);

/// Vendor-name plate (`0x21`): the record-sourced title tab.
pub const WIN_VENDOR_PLATE: usize = 33;
/// Purse (`0x20`): the party-gold counter.
pub const WIN_PURSE: usize = 32;
/// Item info (`0x22`): name + owned count + description.
pub const WIN_ITEM_INFO: usize = 34;
/// Buy quantity (`0x23`): held count, prompt, qty x unit = total.
pub const WIN_BUY_QUANTITY: usize = 35;
/// Equip-target recipient list (`0x24`).
pub const WIN_EQUIP_TARGET: usize = 36;
/// Sell quantity (`0x25`): quantity, held count, halved total.
pub const WIN_SELL_QUANTITY: usize = 37;
/// Item detail / sell (`0x27`): name, description, sell price, passive lines.
pub const WIN_SELL_DETAIL: usize = 39;
/// Point Card toast (`0x1F`, `FUN_801DCE20`): heading, 8-digit bank, unit
/// label, cursor. Both openers hand the widget VM a script whose whole body
/// is `[open 0x1F]` + terminator, so the beat is the only gate.
pub const WIN_POINT_CARD: usize = 31;

/// Renderer VA of window 35 (`FUN_801D5510`). Its port
/// (`engine_core::shop::shop_buy_quantity_panel`) returns pens rather than a
/// draw list, so no `MenuWindowPainter` names it and `painter_at` does not
/// resolve the id; it is checked against the descriptor's own renderer word
/// instead - the same authority-over-id rule `painter_at` applies.
pub const RENDERER_BUY_QUANTITY: u32 = 0x801D_5510;
/// Renderer VA of window 39 (`FUN_801D5AE8`), the pens-only sibling of
/// window 35 (`engine_core::shop::shop_sell_detail_panel`).
pub const RENDERER_SELL_DETAIL: u32 = 0x801D_5AE8;

/// Window 39's price label. Retail's own string is a menu-overlay rodata
/// literal; staging it here keeps the translation layer owning the text.
pub const SELL_DETAIL_PRICE_LABEL: &str = "Price";
/// What window 39 prints in place of the price row when the item's `+2` buy
/// price is zero - retail's quest-item / unsellable arm.
pub const SELL_DETAIL_CANNOT_SELL: &str = "Cannot sell";

/// Everything the screens read, borrowed from whichever host holds it.
#[derive(Clone, Copy)]
pub struct ScreenInputs<'a> {
    /// The live scene world: purse, bag, text tables, shop catalog.
    pub world: &'a World,
    /// The menu runtime that owns the shop / prize / inn sessions.
    pub menu: &'a MenuRuntime,
    pub font: &'a legaia_font::Font,
    /// The menu overlay's disc-parsed window table. The retail descriptor
    /// windows draw at its rects or not at all.
    pub table: Option<&'a MenuWindowTable>,
    /// The system-UI chrome atlas rects. Without them frames are skipped and
    /// cursors / pictograms fall back to ASCII glyphs.
    pub chrome: Option<&'a SaveMenuAtlasRects>,
    /// The boot executable's spell / Seru name table (the seru-trade rows).
    pub seru_names: Option<&'a legaia_asset::spell_names::SpellNameTable>,
    /// The casino coin counter's laid-out lines
    /// (`SceneHost::coin_counter_lines`, which needs the scene's PROT index).
    pub coin_counter: &'a [SubmodeLine],
}

/// One frame of the shop-family overlay group.
#[derive(Debug, Clone, Default)]
pub struct ShopOverlayFrame {
    /// Stage-pixel texts: the fallback panel, the shop's retail windows, the
    /// prize exchange, the coin counter. The host scales them to the surface.
    pub stage_texts: Vec<TextDraw>,
    /// Surface-pixel sprites: the panel frame, then the gold shop's and the
    /// prize exchange's frames and atlas markers.
    pub sprites: Vec<SpriteDraw>,
    /// Stage-pixel level-up / Seru-capture banner rows (outside battle).
    pub banner_texts: Vec<TextDraw>,
}

impl ShopOverlayFrame {
    /// Nothing to draw this frame.
    pub fn is_empty(&self) -> bool {
        self.stage_texts.is_empty() && self.sprites.is_empty() && self.banner_texts.is_empty()
    }
}

/// The shop-family overlay for one frame - built once, so a host's text and
/// sprite passes read the same composition.
///
/// The gold shop's retail screen wins whenever a gold-shop phase is up;
/// otherwise the fallback panel (seru trade, inn, the `[label]` stand-in)
/// draws, with the retail windows around it and its gold frame sized off the
/// panel alone. The prize exchange and the coin counter draw beside either.
pub fn shop_overlay_frame(
    inputs: &ScreenInputs<'_>,
    surface_w: u32,
    surface_h: u32,
) -> ShopOverlayFrame {
    let mut frame = ShopOverlayFrame::default();
    let (origin, scale) = legaia_engine_ui::pause_menu::stage_transform(surface_w, surface_h);
    match gold_shop_screen(inputs, surface_w, surface_h) {
        Some(screen) => {
            frame.stage_texts.extend(screen.texts);
            frame.sprites.extend(screen.sprites);
        }
        None => {
            let fallback = fallback_panel_draws(inputs, surface_w, surface_h);
            if let (Some(rects), Some(rect)) = (inputs.chrome, panel_frame_rect(&fallback.panel)) {
                frame
                    .sprites
                    .extend(legaia_engine_ui::menu_window_chrome_draws_for(
                        rects, rect, origin, scale,
                    ));
            }
            frame.stage_texts.extend(fallback.panel);
            frame.stage_texts.extend(fallback.windows.texts);
            frame.sprites.extend(fallback.windows.sprites);
        }
    }
    let prize = prize_window_draws(inputs, surface_w, surface_h);
    frame.stage_texts.extend(prize.texts);
    frame.sprites.extend(prize.sprites);
    frame
        .stage_texts
        .extend(coin_counter_window_draws(inputs.font, inputs.coin_counter));
    frame.banner_texts = banner_stage_draws(inputs);
    frame
}

/// Display label for item `id` off the disc item table
/// (`MenuState::item_label`, which spells the nameless-id fallback).
pub fn shop_item_label(world: &World, id: u8) -> String {
    world.menu.item_label(id)
}

/// The description line window 34 draws.
///
/// `FUN_801D4A80` routes an **accessory** (item record kind byte `2`) through
/// the passive table instead of the item's own description word, and draws
/// nothing at all when that passive index is the `>= 0x40` sentinel.
/// `MenuTextTables::item_passive_text` resolves the same chain
/// (`legaia_asset::accessory_passive`, which applies the sentinel bound), so a
/// `Some` there is the accessory arm and a `None` is the item arm.
pub fn shop_item_description(world: &World, id: u8) -> String {
    let Some(text) = world.menu.text.as_ref() else {
        return String::new();
    };
    if let Some((_, desc)) = text.item_passive_text(id) {
        return desc;
    }
    text.item_desc(id).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests;

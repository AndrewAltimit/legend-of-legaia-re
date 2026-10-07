//! The casino's two counters: the prize exchange (menu-overlay windows
//! 43 / 44 / 45 / 46) and the coin counter (the field overlay's op-`0x49`
//! sub-op 6 screen).

use crate::{ScreenInputs, shop_item_label};
use legaia_engine_core::field_submode_screen::SubmodeLine;
use legaia_engine_ui::{self as ui, TextDraw, shop_screen as ss, ui_prize_exchange as px};

/// The casino **prize-exchange** windows while a
/// `legaia_engine_core::prize_exchange::PrizeExchangeSession` owns the pad -
/// the shared `engine-ui` composition, framed, with its hand / coin
/// pictogram resolved to atlas sprites (`shop_screen::prize_screen_draws`).
/// Texts in stage pixels, sprites in surface pixels. Empty without a session
/// or the window table, and while the field is still fading out.
pub fn prize_window_draws(
    inputs: &ScreenInputs<'_>,
    surface_w: u32,
    surface_h: u32,
) -> ss::ShopScreenDraws {
    let menu = inputs.menu;
    let (Some(session), Some(table)) = (menu.prize_session.as_ref(), inputs.table) else {
        return Default::default();
    };
    if menu.shop_fade_level().is_some() {
        return Default::default();
    }
    let world = inputs.world;
    let view = px::PrizeExchangeView {
        rows: session
            .rows()
            .map(|r| {
                let held = *world.party.inventory.get(&r.item_id).unwrap_or(&0);
                px::PrizeRow {
                    name: shop_item_label(world, r.item_id),
                    price: r.price,
                    held,
                    // FUN_801D5DE0's ink arm, over the coin bank.
                    ink: legaia_engine_core::shop::shop_stock_row_ink(
                        i16::from(held),
                        r.gate as i16,
                        world.minigames.casino_coins as i32,
                        r.price as i32,
                    ),
                }
            })
            .collect(),
        cursor: session.cursor(),
        coins: world.minigames.casino_coins,
        confirm_cursor: session.confirming().then(|| session.confirm_cursor()),
    };
    let (text, marks, pict) = px::prize_exchange_draws_for(inputs.font, table, &view);
    let (origin, scale) = ui::pause_menu::stage_transform(surface_w, surface_h);
    let ctx = ss::ShopScreenCtx {
        font: inputs.font,
        rects: ui::pause_menu::MenuRects::new(Some(table)),
        chrome: inputs.chrome,
        origin,
        scale,
    };
    ss::prize_screen_draws(&ctx, session.confirming(), text, &marks, pict)
}

/// The casino **coin counter** (op-`0x49` sub-op 6): the field overlay's
/// entry panel (record 10, `FUN_801E6F70`) and, during the confirm, the
/// three-line panel over it (record 11) - laid out by the engine
/// (`SceneHost::coin_counter_lines`, labels read off the disc) and drawn
/// through the shared pen composition. Stage pixels.
pub fn coin_counter_window_draws(font: &legaia_font::Font, lines: &[SubmodeLine]) -> Vec<TextDraw> {
    ui::ui_text_lines::pen_line_draws_for(
        font,
        lines
            .iter()
            .map(|l| (&l.text[..], i32::from(l.x), i32::from(l.y), l.pen)),
    )
}

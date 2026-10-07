//! The **fallback panel**: what the shop-family overlay draws when no
//! gold-shop screen is up - the seru-trade offer list / confirm, the inn's
//! cost prompt and resting caption, and the `[label]` stand-in for a menu
//! state nothing else renders - plus the gold frame sized around it.
//!
//! `MenuRuntime::shop_screen_phase` answers for every gold-shop state
//! (`ShopMenu` / `ShopBuy` / `ShopSell` / `ShopQuantity` / `ShopConfirm`), so
//! [`crate::gold_shop_screen`] always wins those and the panel never carries
//! their rows. A shop session that reaches this module is in the seru trade
//! or the transient exit beat.

use crate::{SHOP_PEN, ScreenInputs, shop_window_draws};
use legaia_engine_core::menu_runtime::MenuState;
use legaia_engine_ui::{self as ui, ShopRow, TextDraw, shop_screen as ss};

/// The fallback panel's output for one frame.
#[derive(Debug, Clone, Default)]
pub struct FallbackPanel {
    /// The panel itself at [`SHOP_PEN`], stage pixels - the only texts the
    /// gold frame ([`panel_frame_rect`]) is sized from.
    pub panel: Vec<TextDraw>,
    /// The shop's retail descriptor windows still up around a trade screen
    /// (vendor plate, purse, the Point Card toast), their cursor and
    /// pictogram requests resolved through [`ss::shop_marker_draws`]: atlas
    /// sprites with the chrome, the shared `>` / `G` / `C` glyphs without.
    pub windows: ss::ShopScreenDraws,
}

/// The fallback panel for one frame. Empty while no menu-runtime screen is
/// open, and while the prize exchange (which draws its own retail windows)
/// owns the pad.
pub fn fallback_panel_draws(
    inputs: &ScreenInputs<'_>,
    surface_w: u32,
    surface_h: u32,
) -> FallbackPanel {
    let mut out = FallbackPanel::default();
    let menu = inputs.menu;
    if !menu.is_open() {
        return out;
    }
    let font = inputs.font;
    let state = MenuState::from_byte(menu.ctx_state());
    let cursor = menu.cursor() as usize;
    if let Some(shop) = menu.shop_session.as_ref() {
        let (origin, scale) = ui::pause_menu::stage_transform(surface_w, surface_h);
        let ctx = ss::ShopScreenCtx {
            font,
            rects: ui::pause_menu::MenuRects::new(inputs.table),
            chrome: inputs.chrome,
            origin,
            scale,
        };
        let win = shop_window_draws(inputs, shop, state, cursor);
        out.windows.texts.extend(win.texts);
        let marks = ss::shop_marker_draws(&ctx, &win.marks, &win.pictograms);
        out.windows.texts.extend(marks.texts);
        out.windows.sprites.extend(marks.sprites);
        // The Point Card toast over the rest.
        out.windows.texts.extend(win.toast_texts);
        let toast = ss::shop_marker_draws(&ctx, &win.toast_marks, &[]);
        out.windows.texts.extend(toast.texts);
        out.windows.sprites.extend(toast.sprites);
        if matches!(
            state,
            Some(MenuState::ShopTrade) | Some(MenuState::ShopTradeConfirm)
        ) {
            out.panel = shop_trade_draws(inputs, state, cursor);
        }
        return out;
    }
    if let Some(inn) = menu.inn_session.as_ref() {
        out.panel = match state {
            Some(MenuState::InnConfirm) => {
                let title = format!("INN  Rest for {}G?", inn.cost);
                let rows = [ShopRow::new("Yes", None), ShopRow::new("No", None)];
                ui::shop_draws_for(
                    font,
                    &title,
                    &rows,
                    cursor,
                    Some(inputs.world.party.money),
                    SHOP_PEN,
                )
            }
            Some(MenuState::InnSleep) => label_draws(font, "Resting..."),
            _ => label_draws(font, &format!("[{}]", menu.current_label())),
        };
        return out;
    }
    if menu.prize_session.is_none() {
        // A diagnostic row: the difference between "this screen has no draw
        // yet" and a black frame with the pad captured.
        out.panel = label_draws(font, &format!("[{}]", menu.current_label()));
    }
    out
}

/// One plain line at [`SHOP_PEN`] in the retail menu ink.
fn label_draws(font: &legaia_font::Font, text: &str) -> Vec<TextDraw> {
    ui::text_draws_for(&font.layout_ascii(text), SHOP_PEN, ui::MENU_TEXT_WHITE)
}

/// The shop menu's **seru-trade** screens: the offer list (`ShopTrade`) or
/// the yes/no confirm (`ShopTradeConfirm`). The text is the engine's
/// (`seru_trade::trade_screen_text`): "give (owner) -> receive" rows built
/// from the boot executable's spell / Seru name table.
///
/// Seru trading is a patcher feature - retail's config ships disabled, so
/// `shop_menu_rows` hides the row and neither screen is reachable on a
/// vanilla disc.
fn shop_trade_draws(
    inputs: &ScreenInputs<'_>,
    state: Option<MenuState>,
    cursor: usize,
) -> Vec<TextDraw> {
    let menu = inputs.menu;
    let pending = menu.pending_trade_offer();
    let text = legaia_engine_core::seru_trade::trade_screen_text(
        menu.trade_session.as_ref(),
        pending.as_ref(),
        state == Some(MenuState::ShopTradeConfirm),
        &inputs.world.party.roster.members,
        inputs.seru_names,
    );
    let rows: Vec<ShopRow<'_>> = text
        .rows
        .iter()
        .map(|l| ShopRow::new(l.as_str(), None))
        .collect();
    ui::shop_draws_for(inputs.font, &text.title, &rows, cursor, None, SHOP_PEN)
}

/// The gold 9-slice frame rect (stage pixels) around the fallback panel, or
/// `None` when no panel draws. Sized from the panel's own rows only - never
/// from the floor window, the code lock or the retail windows beside it,
/// which carry no panel.
pub fn panel_frame_rect(panel: &[TextDraw]) -> Option<(i32, i32, i32, i32)> {
    if panel.is_empty() {
        return None;
    }
    Some(ui::shop_panel_frame_rect(
        SHOP_PEN,
        ui::shop_panel_rows(panel),
    ))
}

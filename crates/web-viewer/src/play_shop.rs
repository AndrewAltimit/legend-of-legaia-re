//! Browser **field shop** + post-action **banner overlays**.
//!
//! Both halves are pure wiring: the state machine is
//! [`legaia_engine_core::menu_runtime::MenuRuntime`] and the geometry is
//! [`legaia_engine_ui::shop_draws_for`] / [`legaia_engine_ui::level_up_draws_for`]
//! / [`legaia_engine_ui::capture_banner_draws_for`] - the same builders the
//! native `play-window` calls. Nothing here re-implements a screen; it
//! projects the shared draw lists into the page's `{ sprites, texts }` quad
//! JSON, exactly as [`crate::play_menu`] and [`crate::play_dialog`] do.
//!
//! # Why the shop had to land with the catalog
//!
//! A field-VM op-`0x49` sub-0 merchant record arms the shop through
//! `World::try_arm_field_shop`, which sets **both** `field_shop_armed` and
//! `field_shop_open`. The op-`0x49` tristate then reports `Armed`, and the
//! field VM *suspends* until the host calls `World::finish_field_shop`. So a
//! host that installs the shop catalog but never opens a shop UI does not
//! merely lack a screen - it hangs the script on the first merchant.
//!
//! That is why [`crate::runtime`] installs `item_shop_data` and this module
//! lands together: before, the browser had no catalog, so `try_arm_field_shop`
//! failed its priced-record validation and every merchant was inert. Now the
//! catalog resolves, the shop opens, and closing it resumes the VM past the
//! merchant op.
//!
//! # Divergence from the native window (deliberate)
//!
//! * **Edge-triggered input.** The native window feeds `MenuRuntime::tick`
//!   the *held* pad each frame; `menu_runtime::step` does no edge detection
//!   of its own, so a held direction walks the cursor at 60 rows/second.
//!   The browser page feeds **edges**, matching its own pause-menu
//!   convention ([`crate::play_menu::play_menu_input`]) and retail's
//!   behaviour.
//!
//! And one that is **not** a divergence, though it once was: both hosts now
//! resolve their row labels from the disc item table (`World::menu.text`), so
//! a name that appears on one appears on the other.
//!
//! Row inks come from the retail kernels
//! `legaia_engine_core::shop::{shop_root_command_rows, shop_buy_row_ink,
//! shop_stock_row_ink}` (`FUN_801D4868`; the list kernel `FUN_80032A44`'s
//! shop-row arm; `FUN_801D5DE0`), so an empty bag greys the Sell row, a full
//! stack / unaffordable price greys a stock row, and the Platinum Card band a
//! shop builds (`ShopInventory::featured_rows`) draws in the teal pen on this
//! host too. `shop_stock_row_ink` is the **casino prize list's** kernel - it
//! gates on the coin bank - and inks the prize-exchange rows, not the shop's.
//!
//! # The retail descriptor windows
//!
//! Retail's shop is not one panel. The window-script runner slides in five
//! separate windows and each one's content comes from the routine its
//! descriptor names, so the shop the page draws is the engine's interactive
//! list **plus** the four painters this host feeds off the disc-parsed
//! descriptor table - the same set, at the same rects, that the native
//! `play-window` paints (`window/shop_windows.rs`):
//!
//! | Id | Renderer | Content |
//! |---|---|---|
//! | 33 (`0x21`) | `FUN_801DCF14` | vendor plate - the scene MAN shop record's trailing name |
//! | 32 (`0x20`) | `FUN_801DCF84` | purse - `World::party.money` (retail `_DAT_8008459C`) |
//! | 34 (`0x22`) | `FUN_801D4A80` | hovered item's name / owned count / description |
//! | 37 (`0x25`) | `FUN_801D5944` | sell quantity, held count, halved gold total |
//!
//! Each resolves through [`ui::painter_at`], so an id whose descriptor names a
//! different renderer is skipped rather than mis-drawn. They draw only when
//! the real descriptor table parsed: these windows exist at their disc rects
//! or not at all, and [`crate::play_menu`]'s pinned-rect fallback cannot
//! invent a `renderer_va`.
//!
//! Two further screens ride over the parked list, each drawn by the shared
//! builder its native twin uses: the equipment-buy **recipient picker**
//! (windows 36 / 25 / 41, `ui::recipient_picker_draws_for`) and the
//! **seru-trade** offer list + confirm ([`LegaiaRuntime::shop_trade_draws`],
//! the twin of `window/menu_draws.rs::draw_shop_trade`).
//!
//! REF: FUN_801d5de0
//! REF: FUN_801d4868

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::menu_runtime::{MenuInput, MenuState};
use legaia_engine_core::shop::ShopSession;
use legaia_engine_ui::ui_menu_window_painters::{
    SELL_QUANTITY_HEADING, buy_quantity_draws_for, counter_panel_draws_for,
    item_description_draws_for, record_title_tab_draws_for, sell_quantity_draws_for,
};
use legaia_engine_ui::{self as ui, ShopRow, SpriteDraw, TextDraw};
use wasm_bindgen::prelude::*;

/// The per-window painters' output for one shop frame: texts in stage pixels
/// plus the sprite requests `ui::shop_screen::shop_marker_draws` resolves.
/// The Point Card toast is kept apart because it draws over every other
/// window, text included. Twin of the native window's `ShopWindowDraws`.
#[derive(Default)]
struct ShopWindowDraws {
    texts: Vec<TextDraw>,
    marks: Vec<ui::ui_menu_window_painters::PainterSprite>,
    pictograms: Vec<ui::ui_menu_window_painters::PainterPictogram>,
    toast_texts: Vec<TextDraw>,
    toast_marks: Vec<ui::ui_menu_window_painters::PainterSprite>,
    toast_frame: Option<ui::ui_menu_window_painters::PainterRect>,
}

/// One shop panel row before it is turned into a borrowing [`ShopRow`]:
/// owned label, optional price, retail `_DAT_8007B454` ink.
type ShopRowSpec = (String, Option<u32>, u8);

/// Stage-pixel pen for the shop panel, matching the native window's `(8, 140)`.
const SHOP_PEN: (i32, i32) = (8, 140);
/// Vendor-name plate (`0x21`): the record-sourced title tab.
const WIN_VENDOR_PLATE: usize = 33;
/// Purse (`0x20`): the party-gold counter.
const WIN_PURSE: usize = 32;
/// Item info (`0x22`): name + owned count + description.
const WIN_ITEM_INFO: usize = 34;
/// Buy quantity (`0x23`): held count, prompt, qty x unit = total.
const WIN_BUY_QUANTITY: usize = 35;
/// Equip-target recipient list (`0x24`).
const WIN_EQUIP_TARGET: usize = 36;
/// Sell quantity (`0x25`): quantity, held count, halved total.
const WIN_SELL_QUANTITY: usize = 37;
/// Sell-list item detail (`0x27`): name / desc / price row / passive lines.
const WIN_SELL_DETAIL: usize = 39;
// Window 25 (`0x19`, the active-character stat compare) is not a shop window:
// its only opener in the whole menu overlay is the Equip screen's script.
// Window 41 (`0x29`, the party compare) draws through `ui::shop_screen`.
/// Window 31 (`FUN_801DCE20`) - the Point Card toast retail raises after a buy
/// commit credits the counter. Both openers hand the widget VM a script whose
/// whole body is `[open 0x1F]` + terminator, so the beat is the only gate.
const WIN_POINT_CARD: usize = 31;
/// Renderer VA of window 35 (`FUN_801D5510`). No `MenuWindowPainter`
/// variant names it, so `painter_at` does not resolve the id; it is verified
/// against the descriptor's own renderer here instead.
const RENDERER_BUY_QUANTITY: u32 = 0x801D_5510;
/// Renderer VA of window 39 (`FUN_801D5AE8`), the pens-only sibling.
const RENDERER_SELL_DETAIL: u32 = 0x801D_5AE8;
// Windows 35 and 37's own lines are `engine-ui`'s `BUY_QUANTITY_PROMPT` /
// `BUY_QUANTITY_HELD_TAIL` / `BUY_QUANTITY_NONE_HELD` / `SELL_QUANTITY_HEADING`,
// imported with their painters: both hosts draw both windows, so a page-local
// copy is exactly the divergence the drift gate pairs constants to catch.
/// Window 39's two engine-authored labels, paired with the native window's
/// constants of the same names. Retail's own strings are menu-overlay rodata
/// literals; staging them here keeps the translation layer owning the text.
const SELL_DETAIL_PRICE_LABEL: &str = "Price";
/// What window 39 prints in place of the price row when the item's `+2` buy
/// price is zero - retail's quest-item / unsellable arm.
const SELL_DETAIL_CANNOT_SELL: &str = "Cannot sell";
/// Stage-pixel pen for the level-up banner (native `(8, 60)`).
const LEVEL_UP_PEN: (i32, i32) = (8, 60);
/// Stage-pixel pen for the Seru-capture banner (native `(8, 40)`).
const CAPTURE_PEN: (i32, i32) = (8, 40);

/// Pack a pad word into the `MenuInput` the menu VM steps on.
fn menu_input(edge: u16) -> MenuInput {
    legaia_engine_core::menu_runtime::menu_input_from_pad_edges(edge)
}

impl LegaiaRuntime {
    /// Hand a field-VM-armed shop to the menu runtime. Called once per
    /// [`LegaiaRuntime::tick_frame`], mirroring the native window's
    /// `take_pending_field_shop` drain.
    pub(crate) fn poll_field_shop(&mut self) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        if let Some(shop) = host.world.take_pending_field_shop() {
            self.menu.open_shop_menu(shop);
        }
        // The casino prize counter (op-0x49 sub-7), same drain shape.
        if let Some(exchange) = host.world.take_pending_prize_exchange() {
            self.menu.open_prize_exchange(exchange);
        }
    }

    /// Display label for item `id` off the SCUS item table, falling back to
    /// the raw id when no executable was loaded (PROT.DAT-only session).
    fn shop_item_label(&self, id: u8) -> String {
        // `MenuState::item_label`, the label the native shop prints.
        self.scene_host
            .as_ref()
            .map(|h| h.world.menu.item_label(id))
            .unwrap_or_else(|| format!("Item {id:02X}"))
    }

    /// Build the shop panel's text draws in **stage** pixels, or `None` when
    /// no shop is up. Row labels + prices come from the live session; the
    /// geometry is `engine-ui`'s.
    ///
    /// `purse_drawn` is whether the retail descriptor windows are drawing
    /// this frame: window 34 IS the gold readout, so the engine panel drops
    /// its own footer whenever it draws, the way the native window's arm does
    /// (`window/hud.rs` recomputes `show_gold` off `retail_windows`). Without
    /// it this page printed the purse twice.
    fn shop_stage_draws(
        &self,
        font: &legaia_font::Font,
        purse_drawn: bool,
    ) -> Option<Vec<TextDraw>> {
        let shop = self.menu.shop_session.as_ref()?;
        let state = MenuState::from_byte(self.menu.ctx_state());
        let cursor = self.menu.cursor() as usize;
        let world = self.scene_host.as_ref().map(|h| &h.world)?;
        let gold = world.party.money;

        // Owned label storage: the ShopRow view borrows &str, so the
        // resolved names have to outlive the row vector.
        let mut labels: Vec<String> = Vec::new();
        let bag = legaia_engine_core::menu_runtime::MenuRuntime::inventory_items(world);
        let held_of = |id: u8| -> i16 {
            bag.iter()
                .find(|(i, _)| *i == id)
                .map(|(_, q)| *q as i16)
                .unwrap_or(0)
        };
        // The seru-trade screens carry dynamic, owned labels and their own
        // title, so they short-circuit the `(rows, gold)` table below - the
        // same split the native window makes in `draw_shop_trade`.
        if matches!(
            state,
            Some(MenuState::ShopTrade) | Some(MenuState::ShopTradeConfirm)
        ) {
            return Some(self.shop_trade_draws(font, state, cursor));
        }

        let (rows_spec, show_gold): (Vec<ShopRowSpec>, Option<i32>) = match state {
            // Top picker: Buy / Sell / (Trade) / Exit - labels and retail's
            // bag-scan ink from `menu_runtime::shop_root_labels`, the native
            // window's call too (an empty bag greys every row below Buy).
            Some(MenuState::ShopMenu) => (
                legaia_engine_core::menu_runtime::shop_root_labels(
                    world.seru_trade_enabled(),
                    !bag.is_empty(),
                )
                .into_iter()
                .map(|(label, ink)| (label.to_string(), None, ink))
                .collect(),
                Some(gold),
            ),
            Some(MenuState::ShopBuy) => (
                shop.inventory
                    .items
                    .iter()
                    .enumerate()
                    .map(|(row, item)| {
                        // FUN_80032A44's buy-row arm: the hoisted band keeps
                        // its featured pen even when dim.
                        let ink = legaia_engine_core::shop::shop_buy_row_ink(
                            row < shop.inventory.featured_rows,
                            held_of(item.item_id),
                            gold,
                            item.price as i32,
                            false,
                        );
                        (self.shop_item_label(item.item_id), Some(item.price), ink)
                    })
                    .collect(),
                Some(gold),
            ),
            // Retail's sell list is the price-gated slot walk, not the
            // id-sorted bag: an unsellable row dims and sorts last
            // (`MenuRuntime::sell_list_rows`). Twin of the native window's
            // arm in `window::hud`.
            Some(MenuState::ShopSell) => (
                legaia_engine_core::menu_runtime::MenuRuntime::sell_list_rows(world)
                    .iter()
                    .map(|r| {
                        (
                            format!("{} x{}", self.shop_item_label(r.id), r.count),
                            None,
                            if r.dim {
                                ui::SHOP_INK_GREY
                            } else {
                                ui::SHOP_INK_NORMAL
                            },
                        )
                    })
                    .collect(),
                Some(gold),
            ),
            // Retail's quantity screen has no list: one number steps in
            // place inside window 35 / 37 while the list it came from stays
            // parked behind it, so this screen contributes a title and no
            // rows. Twin of the native window's arm in `window::hud`.
            Some(MenuState::ShopQuantity) => (Vec::new(), None),
            Some(MenuState::ShopConfirm) => (
                vec![
                    ("Yes".to_string(), None, ui::SHOP_INK_NORMAL),
                    ("No".to_string(), None, ui::SHOP_INK_NORMAL),
                ],
                Some(gold),
            ),
            _ => (Vec::new(), None),
        };
        if rows_spec.is_empty() {
            return None;
        }
        for (label, _, _) in &rows_spec {
            labels.push(label.clone());
        }
        let rows: Vec<ShopRow<'_>> = labels
            .iter()
            .zip(rows_spec.iter())
            .map(|(label, (_, price, ink))| ShopRow {
                label: label.as_str(),
                price: *price,
                ink: *ink,
            })
            .collect();
        let title = self.menu.current_label();
        let show_gold = if purse_drawn { None } else { show_gold };
        Some(ui::shop_draws_for(
            font, title, &rows, cursor, show_gold, SHOP_PEN,
        ))
    }

    /// The **inn** overlay's stage draws: the cost prompt with its Yes / No
    /// cursor (`InnConfirm`) and the resting caption (`InnSleep`), or `None`
    /// when no inn session is up.
    ///
    /// A leg-for-leg port of the native window's inn arm in `window/hud.rs`,
    /// down to the pen and the title format. `docs/subsystems/inn.md` used to
    /// record that the browser page "deliberately mirrors" the native window
    /// by not drawing these - which was false in the direction that matters,
    /// because the native window does draw them. `InnSession` still has no
    /// production caller on either host (the reachable retail inn is an
    /// ordinary field-VM dialogue), so this is the same test / tooling entry
    /// on both, not a new invented path.
    fn inn_stage_draws(&self, font: &legaia_font::Font) -> Option<Vec<TextDraw>> {
        let cost = self.menu.inn_session.as_ref()?.cost;
        let state = MenuState::from_byte(self.menu.ctx_state());
        let cursor = self.menu.cursor() as usize;
        match state {
            Some(MenuState::InnConfirm) => {
                let gold = self
                    .scene_host
                    .as_ref()
                    .map(|h| h.world.party.money)
                    .unwrap_or(0);
                let title = format!("INN  Rest for {cost}G?");
                let rows = vec![ShopRow::new("Yes", None), ShopRow::new("No", None)];
                Some(ui::shop_draws_for(
                    font,
                    &title,
                    &rows,
                    cursor,
                    Some(gold),
                    SHOP_PEN,
                ))
            }
            Some(MenuState::InnSleep) => Some(ui::text_draws_for(
                &font.layout_ascii("Resting..."),
                SHOP_PEN,
                ui::MENU_TEXT_WHITE,
            )),
            _ => None,
        }
    }

    /// `[Label]` stand-in for a menu-runtime state neither the shop arm nor
    /// the inn arm renders - the native window's `else` arm (and the inn's
    /// own `_` arm) in `window/hud.rs`.
    ///
    /// It is a diagnostic row, and it is the difference between "this screen
    /// has no draw yet" and a black frame with the pad captured. The native
    /// window has always drawn it; this page drew nothing, so a menu state
    /// with no renderer looked like a hang. A live SHOP session is excluded
    /// on purpose: `ShopQuantity` legitimately contributes no rows, because
    /// the retail descriptor windows carry that screen.
    fn menu_label_stand_in(&self, font: &legaia_font::Font) -> Option<Vec<TextDraw>> {
        if !self.menu.is_open() || self.menu.shop_session.is_some() {
            return None;
        }
        Some(ui::text_draws_for(
            &font.layout_ascii(&format!("[{}]", self.menu.current_label())),
            SHOP_PEN,
            ui::MENU_TEXT_WHITE,
        ))
    }

    /// The shop menu's **seru-trade** screens: the offer list (`ShopTrade`)
    /// or the yes/no confirm (`ShopTradeConfirm`).
    ///
    /// The twin of the native window's `draw_shop_trade`. Both screens carry
    /// owned labels ("give (owner) -> receive", the confirm's question) built
    /// from the boot executable's spell/seru name table, so they cannot ride
    /// the `'static`-label row table the rest of the shop uses.
    ///
    /// Seru trading is a patcher feature - retail's config ships disabled, so
    /// `shop_menu_rows` hides the row and neither screen is reachable on a
    /// vanilla disc. On a patched one they are, and the page drew nothing at
    /// all for both states while the session held the pad.
    fn shop_trade_draws(
        &self,
        font: &legaia_font::Font,
        state: Option<MenuState>,
        cursor: usize,
    ) -> Vec<TextDraw> {
        // The screen's text is the engine's (`seru_trade::trade_screen_text`,
        // the native window's call too).
        let pending = self.menu.pending_trade_offer();
        let party = self
            .scene_host
            .as_ref()
            .map(|h| h.world.party.roster.members.as_slice())
            .unwrap_or(&[]);
        let text = legaia_engine_core::seru_trade::trade_screen_text(
            self.menu.trade_session.as_ref(),
            pending.as_ref(),
            state == Some(MenuState::ShopTradeConfirm),
            party,
            self.seru_names.as_ref(),
        );
        let rows: Vec<ShopRow<'_>> = text
            .rows
            .iter()
            .map(|l| ShopRow::new(l.as_str(), None))
            .collect();
        ui::shop_draws_for(font, &text.title, &rows, cursor, None, SHOP_PEN)
    }

    /// The live shop's vendor name, recovered the way the native host does.
    ///
    /// Retail's window 33 reads it out of the armed op-`0x49` record
    /// (`_DAT_8007B450` -> `record + record[2] + 3`, one past the last item
    /// id). `ShopSession` keeps the priced stock but not that name, so match
    /// the session's stock against the scene's decoded shops; a scene with a
    /// single merchant resolves on the first entry.
    ///
    /// REF: FUN_801DCF14
    fn shop_vendor_name<'a>(&'a self, shop: &ShopSession) -> Option<&'a str> {
        let shops = &self.scene_host.as_ref()?.world.shops.scene_shops;
        shops
            .iter()
            .find(|s| {
                s.inventory.items.len() == shop.inventory.items.len()
                    && s.inventory
                        .items
                        .iter()
                        .zip(shop.inventory.items.iter())
                        .all(|(a, b)| a.item_id == b.item_id)
            })
            .or_else(|| shops.first().filter(|_| shops.len() == 1))
            .map(|s| s.name.as_str())
            .filter(|n| !n.is_empty())
    }

    /// The item retail's staged-id word `DAT_801E46B0` would hold: the hovered
    /// row while a list has focus, the pending item once quantity / confirm
    /// owns the flow. `None` is the "not positive" case every painter in this
    /// family draws nothing for - hence `Option`, not a default of id 0.
    fn shop_staged_item(
        &self,
        shop: &ShopSession,
        state: Option<MenuState>,
        cursor: usize,
    ) -> Option<u8> {
        match state {
            Some(MenuState::ShopBuy) => shop.inventory.items.get(cursor).map(|i| i.item_id),
            Some(MenuState::ShopSell) => {
                let world = &self.scene_host.as_ref()?.world;
                legaia_engine_core::menu_runtime::MenuRuntime::sell_list_rows(world)
                    .get(cursor)
                    .map(|r| r.id)
            }
            Some(MenuState::ShopQuantity) | Some(MenuState::ShopConfirm) => shop.pending_item_id,
            _ => None,
        }
        .filter(|id| *id != 0)
    }

    /// The description line window 34 draws.
    ///
    /// `FUN_801D4A80` routes an **accessory** through the passive table rather
    /// than the item's own description word, and draws nothing when that
    /// passive index is the `>= 0x40` sentinel.
    /// `MenuTextTables::item_passive_lines` resolves the same chain, so a
    /// `Some` there is the accessory arm and a `None` the item arm.
    fn shop_item_description(&self, id: u8) -> String {
        let Some(text) = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.menu.text.as_ref())
        else {
            return String::new();
        };
        if let Some((_, desc)) = text.item_passive_lines(id) {
            return desc;
        }
        text.item_desc(id).unwrap_or_default().to_string()
    }

    /// ASCII stand-in for a painter's pictogram / cursor request, until the
    /// UI-icon atlas page carrying the currency glyphs is uploaded on this
    /// host. Same substitution the native window makes, so neither host drops
    /// a request silently.
    fn painter_glyph_stand_in(
        &self,
        font: &legaia_font::Font,
        glyph: &str,
        xy: (i32, i32),
    ) -> Vec<TextDraw> {
        ui::text_draws_for(&font.layout_ascii(glyph), xy, ui::MENU_TEXT_GOLD)
    }

    /// The casino **prize-exchange** windows (43 / 44 / 45 / 46) while a
    /// session owns the pad - the shared `engine-ui` composition, framed, with
    /// its hand / coin pictogram resolved to atlas sprites
    /// (`shop_screen::prize_screen_draws`, the native window's call too).
    /// Texts in stage pixels, sprites in surface pixels. Empty without the
    /// window table.
    fn prize_window_draws(
        &self,
        font: &legaia_font::Font,
        surface_w: u32,
        surface_h: u32,
    ) -> ui::shop_screen::ShopScreenDraws {
        let Some(session) = self.menu.prize_session.as_ref() else {
            return Default::default();
        };
        let Some(assets) = self.menu_assets.as_ref() else {
            return Default::default();
        };
        let Some(table) = assets.window_table() else {
            return Default::default();
        };
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
            return Default::default();
        };
        use legaia_engine_ui::ui_prize_exchange as px;
        let view = px::PrizeExchangeView {
            rows: session
                .rows()
                .map(|r| {
                    let held = *world.party.inventory.get(&r.item_id).unwrap_or(&0);
                    px::PrizeRow {
                        name: self.shop_item_label(r.item_id),
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
        let (text, marks, pict) = px::prize_exchange_draws_for(font, table, &view);
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        let ctx = ui::shop_screen::ShopScreenCtx {
            font,
            rects: ui::pause_menu::MenuRects::new(Some(table)),
            chrome: assets.chrome_rects(),
            origin,
            scale,
        };
        ui::shop_screen::prize_screen_draws(&ctx, session.confirming(), text, &marks, pict)
    }

    /// The casino **coin counter** (op-0x49 sub-6): the field overlay's entry
    /// panel (record 10, `FUN_801E6F70`) and the confirm's three-line panel
    /// over it (record 11), laid out by the engine
    /// (`SceneHost::coin_counter_lines`) and drawn through the pen
    /// composition the native window shares. Stage pixels.
    fn coin_counter_window_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let lines = host.coin_counter_lines();
        legaia_engine_ui::ui_text_lines::pen_line_draws_for(
            font,
            lines
                .iter()
                .map(|l| (&l.text[..], i32::from(l.x), i32::from(l.y), l.pen)),
        )
    }

    /// The shop's four **retail descriptor windows** for the current phase, in
    /// stage pixels. Empty when the menu-overlay window table did not parse:
    /// these windows exist only at their disc-parsed rects.
    fn shop_window_draws(
        &self,
        font: &legaia_font::Font,
        shop: &ShopSession,
        state: Option<MenuState>,
        cursor: usize,
    ) -> ShopWindowDraws {
        let mut draws = ShopWindowDraws::default();
        let Some(assets) = self.menu_assets.as_ref() else {
            return draws;
        };
        let Some(table) = assets.window_table() else {
            return draws;
        };
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
            return draws;
        };
        let bag = legaia_engine_core::menu_runtime::MenuRuntime::inventory_items(world);
        let held_of = |id: u8| -> u32 {
            bag.iter()
                .find(|(i, _)| *i == id)
                .map(|(_, q)| u32::from(*q))
                .unwrap_or(0)
        };
        let out = &mut draws.texts;
        let marks = &mut draws.marks;
        let pics = &mut draws.pictograms;

        // Window 33 - the vendor plate.
        if let (Some(name), Some((d, _))) = (
            self.shop_vendor_name(shop),
            ui::painter_at(
                table,
                WIN_VENDOR_PLATE,
                ui::MenuWindowPainter::RecordTitleTab,
            ),
        ) {
            out.extend(record_title_tab_draws_for(font, ui::painter_rect(d), name));
        }

        // Window 32 - the purse. Both the pictogram id and which live total
        // the digits print come out of the dispatch, because retail's two
        // counter renderers are one routine with two literals changed.
        let purse = table.window(WIN_PURSE);
        if let (Some(d), Some(ui::MenuWindowPainter::Counter { pictogram, source })) =
            (purse, purse.and_then(ui::painter_for))
        {
            let value = match source {
                ui::CounterSource::PartyGold => world.party.money.max(0) as u64,
                ui::CounterSource::CasinoCoins => world.minigames.casino_coins as u64,
            };
            let rect = ui::painter_rect(d);
            let (digits, pic) = counter_panel_draws_for(font, rect, pictogram, value);
            out.extend(digits);
            pics.push(pic);
        }

        // Window 34 - the hovered item's info panel. The sell list draws
        // window 39 (the price + passive detail panel) instead - retail's
        // `FUN_801D5AE8` is the sell-family renderer, and the two windows
        // print the same name/description head at overlapping rects.
        let staged = self.shop_staged_item(shop, state, cursor);
        // The sell family (list or its quantity stepper) shows window 39 in
        // place of 34.
        let selling_list = matches!(state, Some(MenuState::ShopSell))
            || self.menu.quantity_view().is_some_and(|v| !v.buying);
        if !selling_list
            && let Some((d, _)) =
                ui::painter_at(table, WIN_ITEM_INFO, ui::MenuWindowPainter::ItemDescription)
        {
            let id = staged.unwrap_or(0);
            out.extend(item_description_draws_for(
                font,
                ui::painter_rect(d),
                staged.is_some(),
                &self.shop_item_label(id),
                held_of(id).min(u32::from(u8::MAX)) as u8,
                &self.shop_item_description(id),
            ));
        }
        if selling_list {
            let (text, pic) = self.sell_detail_window_draws(font, table, staged);
            out.extend(text);
            pics.extend(pic);
        }

        // Windows 37 / 35 - the two quantity steppers. Retail runs the
        // quantity screen as one number moving in place over a parked list,
        // so both arms read the live `QuantityPicker`
        // (`MenuRuntime::quantity_view`) rather than a list cursor. Both
        // draws are `engine-ui` painters the native window calls too.
        let quantity = self.menu.quantity_view();
        if let Some(view) = quantity.filter(|v| !v.buying)
            && let Some((d, _)) = ui::painter_at(
                table,
                WIN_SELL_QUANTITY,
                ui::MenuWindowPainter::SellQuantity,
            )
        {
            let (text, pic, cur) = sell_quantity_draws_for(
                font,
                ui::painter_rect(d),
                true,
                SELL_QUANTITY_HEADING,
                u32::from(view.quantity),
                u32::from(view.max),
                u32::from(view.price),
            );
            out.extend(text);
            pics.extend(pic);
            marks.extend(cur);
        }
        if let Some(view) = quantity.filter(|v| v.buying)
            && let Some(d) = table
                .window(WIN_BUY_QUANTITY)
                .filter(|d| d.renderer_va == RENDERER_BUY_QUANTITY)
        {
            let held = bag
                .iter()
                .find(|(i, _)| *i == view.item_id)
                .map(|(_, q)| u32::from(*q));
            let (text, pic, cur) = buy_quantity_draws_for(
                font,
                ui::painter_rect(d),
                held,
                u32::from(view.quantity),
                u32::from(view.max),
                u32::from(view.price),
            );
            out.extend(text);
            pics.extend(pic);
            marks.extend(cur);
        }

        // Window 31 - the Point Card toast, the browser twin of the native
        // host's arm in `window/shop_windows.rs`. Same beat
        // (`MenuRuntime::point_card_toast`), same disc-parsed rect, same
        // shared labels, so the two hosts cannot drift on the content.
        if self.menu.point_card_toast().is_some()
            && let Some((d, _)) =
                ui::painter_at(table, WIN_POINT_CARD, ui::MenuWindowPainter::AmountPrompt)
        {
            use ui::ui_menu_window_painters as painters;
            let (text, cur) = painters::amount_prompt_draws_for(
                font,
                ui::painter_rect(d),
                painters::POINT_CARD_HEADING,
                world.minigames.point_card.max(0) as u64,
                painters::POINT_CARD_UNIT_LABEL,
            );
            draws.toast_texts.extend(text);
            draws.toast_marks.push(cur);
            draws.toast_frame = Some(ui::painter_rect(d));
        }
        draws
    }

    /// Window 39 - the sell list's item detail panel (`FUN_801D5AE8` via
    /// `engine_core::shop::shop_sell_detail_panel`): name, description, the
    /// halved price row (or "Cannot sell"), and the accessory passive lines
    /// resolved through the renderer's own double-table chain
    /// (`engine_core::shop::item_passive_index`).
    fn sell_detail_window_draws(
        &self,
        font: &legaia_font::Font,
        table: &legaia_asset::menu_windows::MenuWindowTable,
        staged: Option<u8>,
    ) -> (
        Vec<TextDraw>,
        Option<ui::ui_menu_window_painters::PainterPictogram>,
    ) {
        let mut out = Vec::new();
        let Some(d) = table
            .window(WIN_SELL_DETAIL)
            .filter(|d| d.renderer_va == RENDERER_SELL_DETAIL)
        else {
            return (out, None);
        };
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
            return (out, None);
        };
        let rect = ui::painter_rect(d);
        let id = staged.unwrap_or(0);
        let price = world
            .shops
            .item_shop_data
            .as_ref()
            .map(|t| t.price(id))
            .unwrap_or(0);
        // The renderer's passive chain: item kind picks which table the
        // subtype indexes (equip record `+5` vs effect record `+3`).
        let passive = world.tables.item_effects.as_ref().and_then(|effects| {
            legaia_engine_core::shop::item_passive_index(
                effects.kind(id),
                effects.subtype(id),
                |sub| {
                    self.equip_stats
                        .as_ref()
                        .and_then(|t| t.rows().get(sub as usize))
                        .map(|b| b.passive_index())
                        .unwrap_or(legaia_asset::equip_stats::PASSIVE_NONE)
                },
                |sub| {
                    world
                        .tables
                        .item_effects
                        .as_ref()
                        .and_then(|t| t.descriptor(sub))
                        .map(|e| e.marker)
                        .unwrap_or(0x41)
                },
            )
        });
        let panel = legaia_engine_core::shop::shop_sell_detail_panel(
            (rect.x as i16, rect.y as i16),
            i32::from(id),
            price,
            passive,
        );
        if staged.is_none() {
            // Retail leaves only the shade box when nothing is staged; the
            // shop screen frames that box, so no text renders.
            return (out, None);
        }
        let text = |out: &mut Vec<TextDraw>, s: &str, pen: (i16, i16), ink: [f32; 4]| {
            out.extend(ui::text_draws_for(
                &font.layout_ascii(s),
                (i32::from(pen.0), i32::from(pen.1)),
                ink,
            ));
        };
        text(
            &mut out,
            &self.shop_item_label(id),
            panel.name_pen,
            ui::MENU_TEXT_GOLD,
        );
        let desc = self.shop_item_description(id);
        if !desc.is_empty() {
            text(&mut out, &desc, panel.desc_pen, ui::MENU_TEXT_WHITE);
        }
        match panel.sell {
            Some(row) => {
                text(
                    &mut out,
                    SELL_DETAIL_PRICE_LABEL,
                    row.label_pen,
                    ui::MENU_TEXT_TEAL,
                );
                // A 5-digit field at `WX + 0x64`, right-packed.
                out.extend(ui::shop_screen::shop_digit_field_draws(
                    font,
                    u32::from(row.price),
                    (i32::from(row.value_pen.0), i32::from(row.value_pen.1)),
                    ui::shop_screen::SELL_DETAIL_PRICE_CELLS,
                    ui::MENU_TEXT_WHITE,
                ));
            }
            None => text(
                &mut out,
                SELL_DETAIL_CANNOT_SELL,
                panel.cannot_sell_pen,
                ui::MENU_TEXT_ORANGE,
            ),
        }
        if let Some((name, line)) = panel.passive.and_then(|_| {
            self.scene_host
                .as_ref()
                .and_then(|h| h.world.menu.text.as_ref())
                .and_then(|t| t.item_passive_lines(id))
        }) {
            text(&mut out, &name, panel.passive_name_pen, ui::MENU_TEXT_GREEN);
            text(&mut out, &line, panel.passive_desc_pen, ui::MENU_TEXT_WHITE);
        }
        // The currency pictogram beside the price.
        let pic = panel
            .sell
            .map(|row| ui::ui_menu_window_painters::PainterPictogram {
                id: ui::COUNTER_PICTOGRAM_GOLD,
                x: i32::from(row.icon_pen.0),
                y: i32::from(row.icon_pen.1),
            });
        (out, pic)
    }

    /// Window 36 (`0x24`, `FUN_801D56FC`) of the **equipment-buy recipient
    /// flow** (menu-overlay sub-screen `0x1C`): the bag row plus one row per
    /// member, greyed by the character mask, drawn while
    /// [`legaia_engine_core::menu_runtime::MenuRuntime::recipient_session`]
    /// owns the pad. Window 41, the party compare beside it, belongs to the
    /// whole buy flow's window set and draws through
    /// [`ui::shop_screen::shop_screen_draws`].
    ///
    /// The layout is [`ui::recipient_picker_draws_for`], the shared
    /// composition the native window calls too. Returns the texts and the
    /// hand request, which the shop screen resolves to a sprite.
    fn recipient_window_draws(
        &self,
        font: &legaia_font::Font,
    ) -> (
        Vec<TextDraw>,
        Vec<ui::ui_menu_window_painters::PainterSprite>,
    ) {
        let Some(session) = self.menu.recipient_session.as_ref() else {
            return (Vec::new(), Vec::new());
        };
        let Some(table) = self.menu_assets.as_ref().and_then(|a| a.window_table()) else {
            return (Vec::new(), Vec::new());
        };
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
            return (Vec::new(), Vec::new());
        };
        let members = legaia_engine_core::shop::party_compare_members(
            world,
            self.menu.equip_info.as_ref(),
            session.item_id,
        );
        let rows: Vec<ui::RecipientMemberView<'_>> = members
            .iter()
            .take(session.can_equip.len())
            .enumerate()
            .map(|(i, m)| ui::RecipientMemberView {
                name: m.name.as_str(),
                equippable: session.can_equip.get(i).copied().unwrap_or(false),
                already_equipped: m.already_equipped,
                current: Default::default(),
                candidate: Default::default(),
                hp_max: 0,
                mp_max: 0,
            })
            .collect();
        let rects = ui::RecipientWindowRects {
            target_list: ui::painter_at(
                table,
                WIN_EQUIP_TARGET,
                ui::MenuWindowPainter::EquipTargetList,
            )
            .map(|(d, _)| ui::painter_rect(d)),
            party_compare: None,
        };
        let view = ui::RecipientPickerView {
            heading: ui::RECIPIENT_HEADING,
            cursor: session.cursor,
            members: &rows,
            staged_category: ui::CATEGORY_DEFAULT,
        };
        ui::recipient_picker_draws_for(font, rects, &view)
    }

    /// The whole gold-shop screen for one frame, or `None` when no gold-shop
    /// screen is up (inn, seru trade, the exit beat). The twin of the native
    /// window's `gold_shop_screen` (`window/shop_windows.rs`): the phase's
    /// window set framed, the picker / paged list / party column off
    /// [`ui::shop_screen::shop_screen_draws`], every per-window painter on
    /// top, and their hand / pictogram requests resolved to atlas sprites.
    /// Texts are stage pixels; sprites surface pixels.
    fn gold_shop_screen(
        &self,
        font: &legaia_font::Font,
        surface_w: u32,
        surface_h: u32,
    ) -> Option<ui::shop_screen::ShopScreenDraws> {
        use legaia_engine_core::shop::ShopScreenPhase as P;
        use ui::shop_screen as ss;
        let phase = self.menu.shop_screen_phase()?;
        let shop = self.menu.shop_session.as_ref()?;
        let assets = self.menu_assets.as_ref()?;
        let world = &self.scene_host.as_ref()?.world;
        let state = MenuState::from_byte(self.menu.ctx_state());
        let cursor = self.menu.cursor() as usize;
        // The windows on screen this frame with their slide progress
        // (`ShopSlides`, stepped by the menu tick): the phase's set plus any
        // window still sliding out. The toast's frame is laid last by hand
        // below, after the markers of the windows it covers.
        let mut slides = self.menu.shop_slides();
        if slides.is_empty() {
            slides = legaia_engine_core::shop::shop_screen_windows(phase, false)
                .into_iter()
                .map(|id| (id, legaia_engine_core::shop::SHOP_SLIDE_FRAMES))
                .collect();
        }
        let windows: Vec<usize> = slides.iter().map(|(id, _)| *id).collect();
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        let ctx = ss::ShopScreenCtx {
            font,
            rects: ui::pause_menu::MenuRects::new(assets.window_table()),
            chrome: assets.chrome_rects(),
            origin,
            scale,
        };
        let bag = legaia_engine_core::menu_runtime::MenuRuntime::inventory_items(world);
        let held_of = |id: u8| -> i16 {
            bag.iter()
                .find(|(i, _)| *i == id)
                .map(|(_, q)| *q as i16)
                .unwrap_or(0)
        };

        // Window 42 - the picker, with the hand only while it has the pad.
        let picker_rows = legaia_engine_core::menu_runtime::shop_root_labels(
            world.seru_trade_enabled(),
            !bag.is_empty(),
        );
        let picker = ss::ShopPickerView {
            rows: &picker_rows,
            cursor: (phase == P::Root).then_some(cursor),
        };

        // Window 40 / 38 - the list for this phase.
        let gold = world.party.money;
        let (labels, values, inks, kind, list_cursor, browsing): (
            Vec<String>,
            Vec<u32>,
            Vec<u8>,
            _,
            usize,
            bool,
        ) = match phase {
            P::SellList | P::SellQuantity => {
                let rows = legaia_engine_core::menu_runtime::MenuRuntime::sell_list_rows(world);
                (
                    rows.iter().map(|r| self.shop_item_label(r.id)).collect(),
                    rows.iter().map(|r| u32::from(r.count)).collect(),
                    rows.iter()
                        .map(|r| {
                            if r.dim {
                                ui::SHOP_INK_GREY
                            } else {
                                ui::SHOP_INK_NORMAL
                            }
                        })
                        .collect(),
                    ss::ShopListKind::Sell,
                    cursor,
                    true,
                )
            }
            _ => {
                let parked = phase == P::Root;
                let items = &shop.inventory.items;
                (
                    items
                        .iter()
                        .map(|i| self.shop_item_label(i.item_id))
                        .collect(),
                    items.iter().map(|i| i.price).collect(),
                    items
                        .iter()
                        .enumerate()
                        .map(|(row, i)| {
                            legaia_engine_core::shop::shop_buy_row_ink(
                                row < shop.inventory.featured_rows,
                                held_of(i.item_id),
                                gold,
                                i.price as i32,
                                parked,
                            )
                        })
                        .collect(),
                    ss::ShopListKind::Buy,
                    if parked { 0 } else { cursor },
                    phase == P::BuyList,
                )
            }
        };
        let rows: Vec<ss::ShopListRow<'_>> = labels
            .iter()
            .zip(values.iter().zip(inks.iter()))
            .map(|(l, (v, i))| ss::ShopListRow {
                label: l.as_str(),
                value: *v,
                ink: *i,
            })
            .collect();
        let list = ss::ShopListView {
            kind,
            rows: &rows,
            cursor: list_cursor,
            browsing,
        };

        // Window 41 - the party column for the staged item.
        let staged = match phase {
            P::BuyRecipient => self.menu.recipient_session.as_ref().map(|r| r.item_id),
            P::BuyQuantity => self.menu.quantity_view().map(|v| v.item_id),
            _ => self.shop_staged_item(shop, state, cursor),
        };
        let members = staged
            .map(|id| {
                legaia_engine_core::shop::party_compare_members(
                    world,
                    self.menu.equip_info.as_ref(),
                    id,
                )
            })
            .unwrap_or_default();
        let party: Vec<ss::ShopCompareMember<'_>> = members
            .iter()
            .map(|m| ss::ShopCompareMember {
                name: m.name.as_str(),
                already_equipped: m.already_equipped,
                equippable: m.equippable,
                current: m.current,
                candidate: m.candidate,
            })
            .collect();

        let view = ss::ShopScreenView {
            windows: &windows,
            picker: Some(picker),
            list: Some(list),
            party: &party,
        };
        let mut out = ss::shop_screen_draws(&ctx, &view);

        // The per-window painters (vendor plate, purse, item info, the
        // quantity steppers, the sell detail, the recipient list, the toast).
        let mut win = self.shop_window_draws(font, shop, state, cursor);
        let (recip_text, recip_marks) = self.recipient_window_draws(font);
        win.texts.extend(recip_text);
        win.marks.extend(recip_marks);
        out.texts.extend(win.texts);
        let marks = ss::shop_marker_draws(&ctx, &win.marks, &win.pictograms);
        out.texts.extend(marks.texts);
        out.sprites.extend(marks.sprites);
        // Every window's draws ride its open / close slide.
        ss::apply_shop_slides(
            &ctx,
            &slides,
            legaia_engine_core::shop::SHOP_SLIDE_FRAMES,
            &mut out.texts,
            &mut out.sprites,
        );
        // The Point Card toast draws over everything: its frame after every
        // other window's sprites, its text after every other window's text,
        // and no text it covers.
        if let Some(r) = win.toast_frame {
            ss::occlude_texts(&mut out.texts, (r.x - 8, r.y - 8, r.w + 16, r.h + 16));
            out.sprites
                .extend(ss::shop_window_frames(&ctx, &[ss::WIN_SHOP_POINT_CARD]));
            out.texts.extend(win.toast_texts);
            let t = ss::shop_marker_draws(&ctx, &win.toast_marks, &[]);
            out.texts.extend(t.texts);
            out.sprites.extend(t.sprites);
        }
        Some(out)
    }

    /// Post-action banner draws in **stage** pixels: the level-up summary and
    /// the Seru-capture line, both ticked down by `World::tick`.
    fn banner_stage_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let mut out = Vec::new();
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
            return out;
        };
        // The native window's arm (`window/hud.rs`): with the system-UI
        // chrome loaded the message rides retail's framed top-of-screen
        // banner, and the loose pens only fire on a chrome-less host. The
        // two framed paths are mutually exclusive by mode - in battle
        // `battle_hud_draws_for` already emitted the banner (and yielded the
        // plaque's seat to it), outside battle the rows are emitted here,
        // because the port raises both messages a mode-tick after the fight
        // has handed the frame back to the field.
        //
        // This arm used to `return` on the framed case, which drew the
        // message in battle and NOTHING at all outside it: every level-up
        // and every Seru capture on the field was silent on this page while
        // the native window framed both.
        let framed = self
            .menu_assets
            .as_ref()
            .and_then(|a| self.battle_banner_message(a));
        if let Some(message) = framed {
            if world.mode != legaia_engine_core::world::SceneMode::Battle {
                out.extend(ui::battle_hud_chrome::message_banner_text_draws_for(
                    font, &message,
                ));
            }
            return out;
        }
        if let Some(b) = world.party.current_level_up_banner.as_ref() {
            out.extend(ui::level_up_draws_for(
                font,
                b.char_id,
                b.new_level,
                b.hp_gained,
                b.mp_gained,
                LEVEL_UP_PEN,
            ));
        }
        if let Some(b) = world.party.current_capture_banner.as_ref()
            && let Some(text) = b.current_banner()
        {
            out.extend(ui::capture_banner_draws_for(font, &text, CAPTURE_PEN));
        }
        out
    }
}

/// Test-only probes for the disc-gated shop oracle
/// (`tests/shop_overlay_parity.rs`). Native-only so the wasm export surface
/// the page consumes stays exactly the player-facing API.
#[cfg(not(target_arch = "wasm32"))]
impl LegaiaRuntime {
    /// Did the gold-shop catalog resolve off `SCUS_942.54`? With no catalog
    /// `try_arm_field_shop` rejects every merchant record.
    pub fn debug_has_shop_catalog(&self) -> bool {
        self.scene_host
            .as_ref()
            .is_some_and(|h| h.world.shops.item_shop_data.is_some())
    }

    /// Is the op-`0x49` shop gate still held (i.e. the field VM suspended)?
    pub fn debug_field_shop_gate_held(&self) -> bool {
        self.scene_host
            .as_ref()
            .is_some_and(|h| h.world.shops.shop_open)
    }

    /// Arm + open a shop the way a merchant's op-`0x49` sub-0 record would,
    /// stocked from the real price table. Returns `false` when no catalog is
    /// installed (nothing to price a stock list with).
    pub fn debug_open_test_shop(&mut self) -> bool {
        let Some(host) = self.scene_host.as_mut() else {
            return false;
        };
        let Some(data) = host.world.shops.item_shop_data.as_ref() else {
            return false;
        };
        // First few genuinely priced ids - enough rows to prove the panel
        // renders stock rather than an empty frame.
        let items: Vec<legaia_engine_core::shop::ShopItem> = (1u8..=255)
            .filter(|&id| data.price(id) > 0)
            .take(4)
            .map(|id| legaia_engine_core::shop::ShopItem {
                item_id: id,
                price: data.price(id) as u32,
            })
            .collect();
        if items.is_empty() {
            return false;
        }
        let inv = legaia_engine_core::shop::ShopInventory::new(0, items);
        // Mirror the arm the field VM performs, so closing the shop has a
        // gate to release.
        host.world.shops.shop_armed = true;
        host.world.shops.shop_open = true;
        self.menu
            .open_shop_menu(legaia_engine_core::shop::ShopSession::new(inv));
        true
    }

    /// Arm + open a shop stocked with **equipment** ids, the rows whose
    /// buy-list confirm takes the retail `RecipientPicker` route
    /// (`shop::buy_list_confirm_route` kind `1`) instead of the quantity
    /// picker. The affordability test runs against the live purse, so the
    /// party is topped up first - a refused row buzzes and never opens the
    /// picker. `false` when the disc tables that decide the route are
    /// missing.
    pub fn debug_open_equipment_shop(&mut self) -> bool {
        let Some(table) = self.equip_stats.clone() else {
            return false;
        };
        let Some(host) = self.scene_host.as_mut() else {
            return false;
        };
        let Some(data) = host.world.shops.item_shop_data.as_ref() else {
            return false;
        };
        let items: Vec<legaia_engine_core::shop::ShopItem> = (1u8..=255)
            .filter(|&id| table.is_equipment(id) && data.price(id) > 0)
            .take(4)
            .map(|id| legaia_engine_core::shop::ShopItem {
                item_id: id,
                price: data.price(id) as u32,
            })
            .collect();
        if items.is_empty() {
            return false;
        }
        host.world.party.money = legaia_engine_core::shop::GOLD_CAP;
        let inv = legaia_engine_core::shop::ShopInventory::new(0, items);
        host.world.shops.shop_armed = true;
        host.world.shops.shop_open = true;
        self.menu
            .open_shop_menu(legaia_engine_core::shop::ShopSession::new(inv));
        true
    }

    /// Is the equipment-buy recipient picker (retail sub-screen `0x1C`)
    /// currently the screen that owns the pad?
    pub fn debug_recipient_picker_open(&self) -> bool {
        self.menu.recipient_session.is_some()
    }

    /// Raw menu-VM state byte, for asserting which shop screen owns the pad.
    pub fn debug_menu_state_byte(&self) -> u8 {
        self.menu.ctx_state()
    }

    /// Install an **enabled** seru-trade config, the way a `--seru-trade`
    /// patched disc's rodata blob does at `load_disc`. Retail ships the
    /// config disabled, so this is the only way a test reaches the shop's
    /// Trade Seru row without a patched image. `false` with no scene host.
    pub fn debug_enable_seru_trade(&mut self, seed: u64) -> bool {
        let Some(host) = self.scene_host.as_mut() else {
            return false;
        };
        host.world.tables.seru_trade_config = Some(legaia_asset::seru_trade::SeruTradeConfig {
            enabled: true,
            seed,
            ..Default::default()
        });
        true
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// `true` while a field-VM merchant shop is up. The page freezes field
    /// input and routes pad edges to [`Self::play_shop_input`] while this
    /// holds, the same way it defers to the pause menu.
    ///
    /// The answer is [`legaia_engine_core::menu_runtime::MenuRuntime::is_open`],
    /// the predicate the native window feeds the field a neutral pad on. The
    /// page used to spell out `shop_session || prize_session` here, which
    /// agreed only while no other menu-runtime screen could be up.
    pub fn play_shop_is_open(&self) -> bool {
        self.menu.is_open()
    }

    /// Drive the open shop one frame from an edge-triggered PSX pad word
    /// (same bit layout as [`Self::set_pad`]).
    ///
    /// When the session ends (the player picked **Exit**, clearing
    /// `shop_session`), this calls `World::finish_field_shop` so the
    /// suspended op-`0x49` flips Armed -> Done and the field VM advances past
    /// the merchant op on its next step. Without that call the script would
    /// stay parked forever.
    pub fn play_shop_input(&mut self, edge: u16) {
        if !self.menu.is_open() {
            return;
        }
        let input = menu_input(edge);
        // Disjoint field borrows: the menu runtime and the scene host are
        // separate fields, so the live scene world (not the disc-free
        // scaffold) can be ticked in place - the shop spends the player's
        // real gold and stocks their real bag.
        let menu = &mut self.menu;
        if let Some(host) = self.scene_host.as_mut() {
            menu.tick(&mut host.world, input);
        }
        // The shop's own blip (`MenuRuntime::take_ui_cue`), keyed through
        // the page's SFX channel - the native window keys the same one off
        // `tick_menu_runtime_session`.
        if let Some(cue) = self.menu.take_ui_cue() {
            self.play_sfx(u32::from(cue));
        }
        if self.menu.shop_session.is_none()
            && let Some(host) = self.scene_host.as_mut()
            && host.world.shops.shop_open
        {
            host.world.finish_field_shop();
        }
        // The prize exchange's own Exit already unparks through the runtime
        // tick; this is the same safety net the shop keeps.
        if self.menu.prize_session.is_none()
            && let Some(host) = self.scene_host.as_mut()
            && host.world.shops.prize_exchange_open
        {
            host.world.finish_prize_exchange();
        }
    }

    /// Draw lists for the field shop panel and the post-action banners over a
    /// `surface_w` x `surface_h` canvas.
    ///
    /// Same shape as [`Self::play_menu_draws_json`] and
    /// [`Self::play_dialog_draws_json`]: `{ "open", "sprites", "texts" }`,
    /// sampling the atlases the `play_menu_*` accessors upload. `open` is
    /// `false` when neither a shop nor a banner is up this frame.
    ///
    /// Like the dialog box (and unlike the pause menu) these composite over
    /// the live field - retail draws both over the running scene.
    pub fn play_overlay_draws_json(&mut self, surface_w: u32, surface_h: u32) -> String {
        const CLOSED: &str = r#"{"open":false,"sprites":[],"texts":[]}"#;
        // A party wipe holds the frozen battle frame and adds nothing to it:
        // retail's next frame after the wipe store is the title overlay
        // fading in. The native window's boot-UI arm owns the whole HUD for
        // the hold (`build_hud` returns the empty game-over list, and the
        // battle chrome pass returns on `boot_ui.is_active()`); this page
        // silenced only its post-battle list, so the party strip, the plaque
        // and the command chips stayed painted over the wipe.
        if self.game_over.is_some() {
            return CLOSED.to_string();
        }
        if !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        let Some(assets) = self.menu_assets.as_ref() else {
            return CLOSED.to_string();
        };
        let font = assets.font_ref();
        let chrome = assets.chrome_rects();
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));

        // Shop first, inn second: the two are mutually exclusive (one menu
        // context byte) and both draw through `shop_draws_for` at the same
        // pen, which is what the native window's `if shop … else if inn …`
        // arm expresses.
        // The retail descriptor windows ride alongside the engine's own
        // interactive list, exactly as they do in the native window: the list
        // is the control, these are the readouts around it. Built FIRST
        // because whether they draw decides whether the panel keeps its own
        // gold footer (window 34 is the purse).
        // The gold shop's retail screen (window set, picker, paged list,
        // party column, per-window painters) - the composition the native
        // window draws too. Outside it (inn, seru trade) the engine panel and
        // the loose painters below keep the screen.
        let gold_shop = self.gold_shop_screen(font, surface_w, surface_h);
        let mut gold_shop_sprites = Vec::new();
        let (mut windows, shop) = if let Some(screen) = gold_shop {
            gold_shop_sprites = screen.sprites;
            (screen.texts, None)
        } else {
            let mut windows = Vec::new();
            if let Some(session) = self.menu.shop_session.as_ref() {
                let w = self.shop_window_draws(
                    font,
                    session,
                    MenuState::from_byte(self.menu.ctx_state()),
                    self.menu.cursor() as usize,
                );
                windows.extend(w.texts);
                for m in w.marks.iter().chain(w.toast_marks.iter()) {
                    windows.extend(self.painter_glyph_stand_in(font, ">", (m.x, m.y)));
                }
                for p in &w.pictograms {
                    windows.extend(self.painter_glyph_stand_in(font, "G", (p.x, p.y)));
                }
                windows.extend(w.toast_texts);
            }
            let shop = self
                .shop_stage_draws(font, !windows.is_empty())
                .or_else(|| self.inn_stage_draws(font))
                .or_else(|| self.menu_label_stand_in(font));
            (windows, shop)
        };
        // Casino prize exchange (windows 43/44/45/46) + the coin counter's
        // digit entry, both shared engine-ui compositions.
        let prize = self.prize_window_draws(font, surface_w, surface_h);
        gold_shop_sprites.extend(prize.sprites);
        windows.extend(prize.texts);
        windows.extend(self.coin_counter_window_draws(font));
        // The field floor window (op-0x49 sub-op 4, the Uru Mais warp pads), through
        // the engine layout + shared line composition the native window
        // draws (`SceneHost::flag_window_lines`).
        if let Some(host) = self.scene_host.as_ref() {
            let mut floor = host.flag_window_lines();
            // The code lock (op-0x49 sub-op 2, slot 0x21), same line
            // composition as the native window (`SceneHost::code_lock_lines`).
            floor.extend(host.code_lock_lines());
            windows.extend(legaia_engine_ui::ui_text_lines::text_line_draws_for(
                font,
                floor
                    .iter()
                    .map(|l| (&l.text[..], i32::from(l.x), i32::from(l.y), l.marked)),
                [1.0, 1.0, 1.0, 1.0],
                legaia_engine_ui::ui_text_lines::FLOOR_WINDOW_MARKED_INK,
            ));
        }
        let banners = self.banner_stage_draws(font);
        // In-battle overlay (HUD rows / encounter banner / command menus),
        // already in surface pixels - appended after the stage-space scale
        // below ([`crate::play_battle`]). Empty outside battle.
        let mut battle = self.battle_overlay_draws(assets, surface_w, surface_h);
        // Post-battle spoils / game over / "no encounters here" hint: all
        // three are OUTSIDE battle mode, so they append here rather than
        // inside `battle_overlay_draws` (which returns early off battle).
        // The native window draws the same three from the same shared
        // builders + world model.
        battle.extend(self.post_battle_overlay_draws(assets, surface_w, surface_h));
        // Sparring-tutorial prompt box: unlike the rest of the battle overlay
        // its rect is in 320x240 stage space (the retail emitter's own
        // coordinates), so it joins the stage-scaled group below and gets the
        // window skin the emitter's measured rect implies.
        let tutorial = self.battle_tutorial_stage_draws(font, chrome.is_some());
        // Retail's field party-status readout (name / LV / HP / MP per present
        // member over a translucent plate). Already in surface pixels, like
        // `battle`, and empty whenever anything else owns the screen
        // ([`crate::play_field_hud`]).
        let (field_hud_sprites, mut field_hud_texts) =
            self.field_party_hud_draws(surface_w, surface_h);
        // The passive-ability badge column rides the same font layer; it is
        // independent of the party readout's idle gate, so it is appended
        // rather than folded into the builder above.
        field_hud_texts.extend(self.passive_hud_draws(surface_w, surface_h));
        // In-world minigame screens (casino / dance / arena), surface
        // pixels ([`crate::play_minigames`]). Empty outside one.
        let (minigame_sprites, minigame_texts) =
            self.minigame_overlay_draws(font, surface_w, surface_h);
        if shop.is_none()
            && gold_shop_sprites.is_empty()
            && windows.is_empty()
            && banners.is_empty()
            && battle.is_empty()
            && tutorial.is_empty()
            && field_hud_sprites.is_empty()
            && field_hud_texts.is_empty()
            && minigame_sprites.is_empty()
            && minigame_texts.is_empty()
        {
            return CLOSED.to_string();
        }

        // The field HUD leads the sprite array so its plate lands under its
        // own label / numeral cells: within one array the draw order is the
        // vec order, and the two never coexist with the chrome below.
        let mut sprites: Vec<SpriteDraw> = field_hud_sprites;
        // Battle HUD chrome (party-strip + plaque lozenges and the gold HP /
        // green MP label cells) samples the same system-UI atlas as the shop
        // frame, so it rides the same sprite array. Empty outside battle.
        sprites.extend(self.battle_chrome_sprite_draws(assets, surface_w, surface_h));
        // The post-battle report's two framed windows (level-up above,
        // spoils below) - same atlas, drawn outside battle mode.
        sprites.extend(self.battle_spoils_chrome_sprite_draws(assets, surface_w, surface_h));
        // The gold shop's frames and atlas sprites.
        sprites.extend(gold_shop_sprites);
        let mut texts: Vec<TextDraw> = Vec::new();
        if let Some(draws) = shop {
            // Frame the panel in the same gold 9-slice the pause menu uses,
            // sized off the text through the shared `shop_panel_rows` /
            // `shop_panel_frame_rect` pair - the same two calls the native
            // window's sprite pass makes, so the inset, the width and the row
            // pitch are one set of numbers rather than two.
            if let Some(rects) = chrome {
                let rows = ui::shop_panel_rows(&draws);
                sprites.extend(ui::menu_window_chrome_draws_for(
                    rects,
                    ui::shop_panel_frame_rect(SHOP_PEN, rows),
                    origin,
                    scale,
                ));
            }
            texts.extend(draws);
        }
        texts.extend(windows);
        texts.extend(banners);
        if let Some(rects) = chrome {
            sprites.extend(self.battle_tutorial_chrome_draws(font, rects, origin, scale));
        }
        // Stage-space, so it joins `texts` before the scale pass below.
        texts.extend(tutorial);
        // Arts command-input chrome (direction chips + D-pad, the pennant
        // input bar, the AP plate). Emitted in stage space by the shared
        // `arts_input` builders off the same baked atlas the menu chrome
        // samples, so this page and the native window draw one geometry.
        // Its own text (the Begin | Reselect pick) rides the stage-scaled
        // text pass below.
        let (arts_sprites, arts_texts) = self.arts_input_stage_draws(font, chrome, origin, scale);
        sprites.extend(arts_sprites);
        ui::scale_stage_text_draws(&mut texts, origin, scale);
        texts.extend(arts_texts);
        // Battle draws stay in surface pixels: the shared HUD's measured
        // column offsets span wider than the 320-px menu stage, exactly as
        // drawn by the native window (surface-space HUD).
        texts.extend(battle);
        // The field HUD's names, likewise already in surface pixels.
        texts.extend(field_hud_texts);
        sprites.extend(minigame_sprites);
        texts.extend(minigame_texts);

        serde_json::json!({
            "open": true,
            // A shop is a menu-overlay session: the field overlay is swapped
            // out and the screen behind its windows is black
            // (`MenuRuntime::covers_field`). The page paints the backdrop
            // before the quads.
            "backdrop": if self.menu.covers_field() { "black" } else { "none" },
            "sprites": sprites.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

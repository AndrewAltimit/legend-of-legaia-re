//! The shop's **retail descriptor windows**, painted through the
//! `renderer_va` dispatch instead of a hard-coded screen.
//!
//! Retail's shop is not one panel: the open script the window-script runner
//! `FUN_801D6628` interprets slides in five separate windows, and each one's
//! content comes from the routine its descriptor names (see
//! `docs/subsystems/shop.md` - the script's descriptor words are byte-verified
//! by the patcher's seru-trading vendor, which edits exactly those seams).
//! Three of the five have painters in `engine-ui`, plus the sell flow's own
//! quantity panel:
//!
//! | Id | Renderer | Content this host feeds it |
//! |---|---|---|
//! | 33 (`0x21`) | `FUN_801DCF14` | the vendor plate - the scene MAN shop record's trailing name |
//! | 32 (`0x20`) | `FUN_801DCF84` | the purse - `World::party.money` (retail `_DAT_8008459C`) |
//! | 34 (`0x22`) | `FUN_801D4A80` | the hovered item's name / owned count / description |
//! | 35 (`0x23`) | `FUN_801D5510` | the buy quantity, held count, unit price and running total |
//! | 37 (`0x25`) | `FUN_801D5944` | the sell quantity, held count and halved gold total |
//! | 39 (`0x27`) | `FUN_801D5AE8` | the sell list's detail panel - name, description, halved price, passive lines |
//!
//! The remaining two are the Buy / Sell / Quit picker (id 42, `FUN_801D4868`,
//! whose rows + ink are `engine-core::shop::shop_root_command_rows`) and the
//! renderer-less list container (id 40), whose content is the host's list.
//!
//! Windows 34 and 39 are alternatives, not siblings: `FUN_801D5AE8` is the
//! sell-family renderer and prints the same name/description head at an
//! overlapping rect, so this host draws 34 for the buy list and 39 for the
//! sell list, matching the browser page.
//!
//! Each window resolves through
//! [`legaia_engine_render::painter_at`], so an id whose descriptor names a
//! different renderer is skipped rather than mis-drawn: the id is the lookup
//! key and the renderer is the authority.
//!
//! Windows 35 and 39 are the exceptions, and they are exceptions about the
//! *port*, not about the table: `FUN_801D5510` and `FUN_801D5AE8` are ported as
//! pens-returning kernels rather than draw-list builders, so no
//! `MenuWindowPainter` variant names either and `painter_at` cannot resolve
//! them. The same authority rule still holds - each id is filtered on the
//! descriptor's own `renderer_va` ([`RENDERER_BUY_QUANTITY`],
//! [`RENDERER_SELL_DETAIL`]), which
//! `crates/engine-shell/tests/menu_window_dispatch_real.rs` pins against the
//! disc's table.
//!
//! One further sub-screen rides over the parked buy list:
//! [`PlayWindowApp::recipient_window_draws`] paints the equipment-buy
//! recipient picker (windows 36 / 25 / 41) through
//! `engine-ui::recipient_picker_draws_for`, the same shared composition the
//! browser play page calls.
//!
//! ## What is a stand-in
//!
//! The painters also return pictogram + cursor **sprite** requests (retail
//! `FUN_8002C488` / `FUN_8002B994` UI-icon-atlas draws). The atlas page
//! holding the currency pictograms is not uploaded yet, so this pass renders
//! them as the ASCII stand-ins the rest of the menu UI uses while an atlas
//! page is missing, and drops nothing silently.

use super::*;

use legaia_engine_core::shop::ShopSession;
use legaia_engine_render::MenuWindowPainter;
use legaia_engine_render::ui_menu_window_painters::{
    POINT_CARD_HEADING, POINT_CARD_UNIT_LABEL, PainterPictogram, PainterSprite,
    SELL_QUANTITY_HEADING, amount_prompt_draws_for, buy_quantity_draws_for,
    counter_panel_draws_for, item_description_draws_for, record_title_tab_draws_for,
    sell_quantity_draws_for,
};

/// Vendor-name plate (`0x21`): the record-sourced title tab.
const WIN_VENDOR_PLATE: usize = 33;
/// Purse (`0x20`): the party-gold counter.
const WIN_PURSE: usize = 32;
/// Item info (`0x22`): name + owned count + description.
const WIN_ITEM_INFO: usize = 34;
/// Buy quantity (`0x23`): held count, prompt, qty x unit = total.
const WIN_BUY_QUANTITY: usize = 35;
/// Sell quantity (`0x25`): quantity, held count, halved total.
const WIN_SELL_QUANTITY: usize = 37;
/// Point Card toast (`0x1F`): heading, 8-digit bank, unit label, cursor.
const WIN_POINT_CARD: usize = 31;
/// Item detail / sell (`0x27`): name, description, sell price, passive lines.
const WIN_SELL_DETAIL: usize = 39;
/// Renderer VA of window 39 (`FUN_801D5AE8`). Its port
/// (`engine_core::shop::shop_sell_detail_panel`) returns pens like window
/// 35's, so the id is filtered on the descriptor's own renderer word here
/// rather than through `painter_at`.
const RENDERER_SELL_DETAIL: u32 = 0x801D_5AE8;
/// Renderer VA of window 35 (`FUN_801D5510`). Its port
/// (`engine_core::shop::shop_buy_quantity_panel`) returns pens rather than a
/// draw list, so `painter_at` deliberately does not resolve it - the id is
/// checked against the descriptor's own renderer word here instead, which is
/// the same authority-over-id rule the `painter_at` windows above use.
const RENDERER_BUY_QUANTITY: u32 = 0x801D_5510;

impl PlayWindowApp {
    /// The live shop's vendor name.
    ///
    /// Retail's window 33 reads it out of the armed op-`0x49` record:
    /// `_DAT_8007B450` points at the opcode's **sub-op byte** (opcode `+1`,
    /// pinned in `docs/subsystems/boot.md` and `tile-board.md`), and
    /// `FUN_801DCF14` starts the string at `record + record[2] + 3`. With the
    /// shop record's payload `[count][count x id][name\0]`, `record[2]` is
    /// `count`, so the string lands exactly one past the last item id - the
    /// trailing ASCII name `legaia_asset::shop_stock` decodes.
    ///
    /// The engine's `ShopSession` keeps the priced stock but not that name, so
    /// the host recovers it by matching the session's stock against the
    /// scene's decoded shops (`World::shops.scene_shops`); a scene with one merchant
    /// resolves on the first entry.
    ///
    /// REF: FUN_801DCF14
    fn shop_vendor_name(&self, shop: &ShopSession) -> Option<&str> {
        let shops = &self.session.host.world.shops.scene_shops;
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

    /// The item the shop's staged-id word would hold: the hovered row while a
    /// list has focus, the pending item once a quantity / confirm phase owns
    /// the flow.
    ///
    /// Retail's `DAT_801E46B0` is a *positive* item id, and every painter in
    /// this family draws nothing when it is not - which is why this returns
    /// `Option` rather than defaulting to id 0.
    fn shop_staged_item(
        &self,
        shop: &ShopSession,
        state: Option<MenuState>,
        cursor: usize,
    ) -> Option<u8> {
        match state {
            Some(MenuState::ShopBuy) => shop.inventory.items.get(cursor).map(|i| i.item_id),
            Some(MenuState::ShopSell) => {
                legaia_engine_core::menu_runtime::MenuRuntime::sell_list_rows(
                    &self.session.host.world,
                )
                .get(cursor)
                .map(|r| r.id)
            }
            Some(MenuState::ShopQuantity) | Some(MenuState::ShopConfirm) => shop.pending_item_id,
            _ => None,
        }
        .filter(|id| *id != 0)
    }

    /// Paint the shop's retail descriptor windows for the current phase.
    ///
    /// Empty when the menu overlay's window table did not parse (no disc):
    /// these windows exist only at their disc-parsed rects, and the engine's
    /// own shop panel already carries the interactive list.
    pub(super) fn shop_window_draws(
        &self,
        shop: &ShopSession,
        state: Option<MenuState>,
        cursor: usize,
    ) -> ShopWindowDraws {
        let mut draws = ShopWindowDraws::default();
        let Some(table) = self.menu_window_table.as_ref() else {
            return draws;
        };
        let world = &self.session.host.world;
        let bag = MenuRuntime::inventory_items(world);
        let out = &mut draws.texts;
        let marks = &mut draws.marks;
        let pics = &mut draws.pictograms;

        // Window 33 - the vendor plate.
        if let (Some(name), Some((d, _))) = (
            self.shop_vendor_name(shop),
            legaia_engine_render::painter_at(
                table,
                WIN_VENDOR_PLATE,
                MenuWindowPainter::RecordTitleTab,
            ),
        ) {
            out.extend(record_title_tab_draws_for(
                &self.font,
                legaia_engine_render::painter_rect(d),
                name,
            ));
        }

        // Window 32 - the purse. The pictogram id + which total the digits
        // print both come out of the dispatch, because retail's two counter
        // renderers differ in exactly those two literals.
        let purse = table.window(WIN_PURSE);
        if let (Some(d), Some(MenuWindowPainter::Counter { pictogram, source })) =
            (purse, purse.and_then(legaia_engine_render::painter_for))
        {
            let value = match source {
                legaia_engine_render::CounterSource::PartyGold => world.party.money.max(0) as u64,
                legaia_engine_render::CounterSource::CasinoCoins => {
                    world.minigames.casino_coins as u64
                }
            };
            let rect = legaia_engine_render::painter_rect(d);
            let (digits, pic) = counter_panel_draws_for(&self.font, rect, pictogram, value);
            out.extend(digits);
            pics.push(pic);
        }

        // Window 34 - the hovered item's info panel. The sell list draws
        // window 39 instead: `FUN_801D5AE8` is the sell-family renderer, and
        // the two windows print the same name/description head at overlapping
        // rects, so drawing both would double the text.
        let staged = self.shop_staged_item(shop, state, cursor);
        // The sell family (list or its quantity stepper) shows window 39 in
        // place of 34.
        let selling_list = matches!(state, Some(MenuState::ShopSell))
            || self.menu_runtime.quantity_view().is_some_and(|v| !v.buying);
        if !selling_list
            && let Some((d, _)) = legaia_engine_render::painter_at(
                table,
                WIN_ITEM_INFO,
                MenuWindowPainter::ItemDescription,
            )
        {
            let id = staged.unwrap_or(0);
            let name = self.shop_item_name(id);
            let owned = bag
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, q)| *q)
                .unwrap_or(0);
            out.extend(item_description_draws_for(
                &self.font,
                legaia_engine_render::painter_rect(d),
                staged.is_some(),
                &name,
                owned,
                &self.shop_item_description(id),
            ));
        }
        if selling_list {
            let (text, pic) = self.sell_detail_window_draws(table, staged);
            out.extend(text);
            pics.extend(pic);
        }

        // Windows 37 / 35 - the two quantity steppers. Retail runs the
        // quantity screen as one number moving in place over a parked list,
        // so both arms read the live `QuantityPicker`
        // (`MenuRuntime::quantity_view`) rather than a list cursor: the
        // number on screen is the number the pad is stepping, and the second
        // number beside it is the picker's own bound. Both draws are
        // `engine-ui` painters the browser page calls too.
        let quantity = self.menu_runtime.quantity_view();
        if let Some(view) = quantity.filter(|v| !v.buying)
            && let Some((d, _)) = legaia_engine_render::painter_at(
                table,
                WIN_SELL_QUANTITY,
                MenuWindowPainter::SellQuantity,
            )
        {
            let (text, pic, cur) = sell_quantity_draws_for(
                &self.font,
                legaia_engine_render::painter_rect(d),
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
                &self.font,
                legaia_engine_render::painter_rect(d),
                held,
                u32::from(view.quantity),
                u32::from(view.max),
                u32::from(view.price),
            );
            out.extend(text);
            pics.extend(pic);
            marks.extend(cur);
        }

        // Window 31 - the Point Card toast. Retail's buy commit hands the
        // widget VM a one-command script (`0x801E4EDC` from the quantity
        // commit, `0x801E4EA8` from the recipient picker; both decode to
        // `[open 0x1F]` + terminator) and then stalls for a press, so this
        // draws exactly while `MenuRuntime` reports the beat.
        let toast = self
            .menu_runtime
            .point_card_toast()
            .and_then(|_| {
                legaia_engine_render::painter_at(
                    table,
                    WIN_POINT_CARD,
                    MenuWindowPainter::AmountPrompt,
                )
            })
            .map(|(d, _)| legaia_engine_render::painter_rect(d));
        if let Some(rect) = toast {
            let points = world.minigames.point_card.max(0) as u64;
            let (text, cur) = amount_prompt_draws_for(
                &self.font,
                rect,
                POINT_CARD_HEADING,
                points,
                POINT_CARD_UNIT_LABEL,
            );
            draws.toast_texts.extend(text);
            draws.toast_marks.push(cur);
            draws.toast_frame = Some(rect);
        }
        draws
    }

    /// Window 36 (`0x24`, `FUN_801D56FC`) of the **equipment-buy recipient
    /// flow** (menu-overlay sub-screen `0x1C`, `FUN_801DB380`): the bag row
    /// plus one row per member, greyed by the character mask, drawn while
    /// [`legaia_engine_core::menu_runtime::MenuRuntime::recipient_session`]
    /// owns the pad. Retail's picker script `0x801E4E84` opens window 36 and
    /// nothing else; window 41, the party compare beside it, belongs to the
    /// whole buy flow's window set and draws through
    /// [`legaia_engine_render::shop_screen::shop_screen_draws`].
    ///
    /// The layout is [`legaia_engine_render::recipient_picker_draws_for`],
    /// the same shared composition the browser play page calls
    /// (`web-viewer::play_shop::recipient_window_draws`). Returns the texts
    /// and the hand request, which the shop screen resolves to a sprite.
    pub(super) fn recipient_window_draws(&self) -> (Vec<TextDraw>, Vec<PainterSprite>) {
        use legaia_engine_render::{
            MenuWindowPainter, RecipientMemberView, RecipientPickerView, RecipientWindowRects,
            painter_at, painter_rect, recipient_picker_draws_for,
        };
        /// Equip-target recipient list (`0x24`).
        const WIN_EQUIP_TARGET: usize = 36;

        let Some(session) = self.menu_runtime.recipient_session.as_ref() else {
            return (Vec::new(), Vec::new());
        };
        let Some(table) = self.menu_window_table.as_ref() else {
            return (Vec::new(), Vec::new());
        };
        let world = &self.session.host.world;
        let members = legaia_engine_core::shop::party_compare_members(
            world,
            self.menu_runtime.equip_info.as_ref(),
            session.item_id,
        );
        let rows: Vec<RecipientMemberView<'_>> = members
            .iter()
            .take(session.can_equip.len())
            .enumerate()
            .map(|(i, m)| RecipientMemberView {
                name: m.name.as_str(),
                equippable: session.can_equip.get(i).copied().unwrap_or(false),
                already_equipped: m.already_equipped,
                current: Default::default(),
                candidate: Default::default(),
                hp_max: 0,
                mp_max: 0,
            })
            .collect();
        // Window 36 only: window 41 is part of the shop screen's set for the
        // whole buy flow and draws through `shop_screen_draws`.
        let rects = RecipientWindowRects {
            target_list: painter_at(table, WIN_EQUIP_TARGET, MenuWindowPainter::EquipTargetList)
                .map(|(d, _)| painter_rect(d)),
            party_compare: None,
        };
        let view = RecipientPickerView {
            heading: legaia_engine_render::RECIPIENT_HEADING,
            cursor: session.cursor,
            members: &rows,
            staged_category: legaia_engine_render::CATEGORY_DEFAULT,
        };
        recipient_picker_draws_for(&self.font, rects, &view)
    }

    /// Item display name, falling back to the id when the disc text tables
    /// are unavailable.
    /// Paint the casino prize-exchange screen (windows 43 / 44 / 45 / 46)
    /// while a [`legaia_engine_core::prize_exchange::PrizeExchangeSession`]
    /// owns the pad - the shared `engine-ui` composition, framed and with its
    /// hand / coin pictogram resolved to atlas sprites through
    /// `shop_screen::prize_screen_draws`, the browser page's call too. Texts
    /// in stage pixels, sprites in surface pixels. Empty without the disc
    /// window table.
    pub(super) fn prize_window_draws(
        &self,
        session: &legaia_engine_core::prize_exchange::PrizeExchangeSession,
        surface_w: u32,
        surface_h: u32,
    ) -> legaia_engine_render::shop_screen::ShopScreenDraws {
        let Some(table) = self.menu_window_table.as_ref() else {
            return Default::default();
        };
        let world = &self.session.host.world;
        use legaia_engine_render::ui_prize_exchange as px;
        let view = px::PrizeExchangeView {
            rows: session
                .rows()
                .map(|r| {
                    let held = *world.party.inventory.get(&r.item_id).unwrap_or(&0);
                    px::PrizeRow {
                        name: self.shop_item_name(r.item_id),
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
        let (text, marks, pict) = px::prize_exchange_draws_for(&self.font, table, &view);
        let (origin, scale) =
            legaia_engine_render::pause_menu::stage_transform(surface_w, surface_h);
        let ctx = legaia_engine_render::shop_screen::ShopScreenCtx {
            font: &self.font,
            rects: legaia_engine_render::pause_menu::MenuRects::new(Some(table)),
            chrome: self.save_menu.as_ref().map(|m| &m.rects),
            origin,
            scale,
        };
        legaia_engine_render::shop_screen::prize_screen_draws(
            &ctx,
            session.confirming(),
            text,
            &marks,
            pict,
        )
    }

    /// Paint the casino **coin counter** (op-`0x49` sub-op 6): the field
    /// overlay's entry panel (record 10, `FUN_801E6F70`) and, during the
    /// confirm, the three-line panel over it (record 11) - laid out by the
    /// engine off the live submode screen, labels read off the disc, drawn
    /// through the pen composition the browser play page shares.
    pub(super) fn coin_counter_window_draws(&self) -> Vec<TextDraw> {
        let lines = self.session.host.coin_counter_lines();
        legaia_engine_render::ui_text_lines::pen_line_draws_for(
            &self.font,
            lines
                .iter()
                .map(|l| (&l.text[..], i32::from(l.x), i32::from(l.y), l.pen)),
        )
    }

    fn shop_item_name(&self, id: u8) -> String {
        self.session.host.world.menu.item_label(id)
    }

    /// The description line window 34 draws.
    ///
    /// `FUN_801D4A80` routes an **accessory** (item record kind byte `2`)
    /// through the passive table instead of the item's own description word,
    /// and draws nothing at all when that passive index is the `>= 0x40`
    /// sentinel. `MenuTextTables::item_passive_lines` resolves the same chain
    /// (`legaia_asset::accessory_passive`, which applies the sentinel bound),
    /// so a `Some` there is the accessory arm and a `None` is the item arm.
    fn shop_item_description(&self, id: u8) -> String {
        let Some(text) = self.session.host.world.menu.text.as_ref() else {
            return String::new();
        };
        if let Some((_, desc)) = text.item_passive_lines(id) {
            return desc;
        }
        text.item_desc(id).unwrap_or_default().to_string()
    }

    /// Window 39 - the **item detail / sell panel** (`FUN_801D5AE8`): the
    /// hovered item's name and description, its halved sell price (or the
    /// "cannot sell" line), and the accessory-passive name + description
    /// rows. It replaces window 34 while the sell list has focus.
    ///
    /// Line-for-line twin of `web-viewer::play_shop::sell_detail_window_draws`:
    /// same shared pen kernel
    /// ([`legaia_engine_core::shop::shop_sell_detail_panel`]), same paired
    /// label constants, so the two hosts cannot drift on layout or text.
    /// Without it a native-window seller saw the buy-side info window, with no
    /// price at all on the screen where the price is the decision.
    ///
    /// The passive chain is the renderer's own (`0x801D5C5C..0x801D5CC8`): the
    /// item record's `+0` class byte picks which table the `+1` subtype
    /// indexes - equipment records' `+5`, everything else the item-effect
    /// record's `+3`. The equipment arm reads through
    /// [`legaia_engine_core::equipment::DiscEquipInfo::row_passive_index`],
    /// which is row-keyed exactly as the renderer's
    /// `0x80074F68 + subtype*8` is.
    fn sell_detail_window_draws(
        &self,
        table: &legaia_asset::menu_windows::MenuWindowTable,
        staged: Option<u8>,
    ) -> (Vec<TextDraw>, Option<PainterPictogram>) {
        let mut out = Vec::new();
        let Some(d) = table
            .window(WIN_SELL_DETAIL)
            .filter(|d| d.renderer_va == RENDERER_SELL_DETAIL)
        else {
            return (out, None);
        };
        let world = &self.session.host.world;
        let rect = legaia_engine_render::painter_rect(d);
        let id = staged.unwrap_or(0);
        let price = world
            .shops
            .item_shop_data
            .as_ref()
            .map(|t| t.price(id))
            .unwrap_or(0);
        let passive = world.tables.item_effects.as_ref().and_then(|effects| {
            legaia_engine_core::shop::item_passive_index(
                effects.kind(id),
                effects.subtype(id),
                |sub| {
                    self.menu_runtime
                        .equip_info
                        .as_ref()
                        .map(|i| i.row_passive_index(sub))
                        .unwrap_or(legaia_engine_core::shop::PASSIVE_NONE)
                },
                |sub| {
                    effects
                        .descriptor(sub)
                        .map(|e| e.marker)
                        .unwrap_or(legaia_engine_core::shop::PASSIVE_NONE + 1)
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
            // Retail leaves only the shade box when nothing is staged, and
            // this host draws no colour-fill primitives - same as the browser.
            return (out, None);
        }
        let mut out_digits: Option<(u16, (i16, i16))> = None;
        let mut text = |s: &str, pen: (i16, i16), ink: [f32; 4]| {
            out.extend(legaia_engine_render::text_draws_for(
                &self.font.layout_ascii(s),
                (i32::from(pen.0), i32::from(pen.1)),
                ink,
            ));
        };
        text(
            &self.shop_item_name(id),
            panel.name_pen,
            legaia_engine_render::MENU_TEXT_GOLD,
        );
        let desc = self.shop_item_description(id);
        if !desc.is_empty() {
            text(&desc, panel.desc_pen, legaia_engine_render::MENU_TEXT_WHITE);
        }
        match panel.sell {
            Some(row) => {
                text(
                    SELL_DETAIL_PRICE_LABEL,
                    row.label_pen,
                    legaia_engine_render::MENU_TEXT_TEAL,
                );
                // A 5-digit field at `WX + 0x64`, right-packed.
                out_digits = Some((row.price, row.value_pen));
            }
            None => text(
                SELL_DETAIL_CANNOT_SELL,
                panel.cannot_sell_pen,
                legaia_engine_render::MENU_TEXT_ORANGE,
            ),
        }
        if let Some((name, line)) = panel
            .passive
            .and(world.menu.text.as_ref())
            .and_then(|t| t.item_passive_lines(id))
        {
            text(
                &name,
                panel.passive_name_pen,
                legaia_engine_render::MENU_TEXT_GREEN,
            );
            text(
                &line,
                panel.passive_desc_pen,
                legaia_engine_render::MENU_TEXT_WHITE,
            );
        }
        if let Some((price, pen)) = out_digits {
            out.extend(legaia_engine_render::shop_screen::shop_digit_field_draws(
                &self.font,
                u32::from(price),
                (i32::from(pen.0), i32::from(pen.1)),
                legaia_engine_render::shop_screen::SELL_DETAIL_PRICE_CELLS,
                legaia_engine_render::MENU_TEXT_WHITE,
            ));
        }
        // The currency pictogram beside the price.
        let pic = panel.sell.map(|row| PainterPictogram {
            id: legaia_engine_render::COUNTER_PICTOGRAM_GOLD,
            x: i32::from(row.icon_pen.0),
            y: i32::from(row.icon_pen.1),
        });
        (out, pic)
    }

    /// ASCII stand-in for a painter's pictogram request until the UI-icon
    /// atlas page carrying the currency glyphs is uploaded.
    pub(super) fn painter_pictogram_stand_in(
        &self,
        pic: legaia_engine_render::ui_menu_window_painters::PainterPictogram,
    ) -> Vec<TextDraw> {
        let glyph = match pic.id {
            legaia_engine_render::COUNTER_PICTOGRAM_GOLD => "G",
            legaia_engine_render::COUNTER_PICTOGRAM_COINS => "C",
            _ => "*",
        };
        legaia_engine_render::text_draws_for(
            &self.font.layout_ascii(glyph),
            (pic.x, pic.y),
            legaia_engine_render::MENU_TEXT_GOLD,
        )
    }

    /// ASCII stand-in for a painter's cursor / marker sprite request - the
    /// twin of the browser play page's `painter_glyph_stand_in(font, ">", ..)`
    /// at the same six call sites.
    ///
    /// It used to withhold the glyph whenever the system-UI atlas was
    /// resident, on the reasoning that "the sprite pass owns the hand cursor
    /// then". No sprite pass does: `PainterSprite` has no consumer outside
    /// this function anywhere in the workspace, and the one pass that draws a
    /// hand from that atlas (`field_menu_chrome_sprite_draws`) runs only
    /// under `BootUiState::FieldMenu`, which is not the state any of these
    /// windows draw in. So on every disc run - the only run where the atlas
    /// IS resident - the shop, prize-exchange and equip-recipient windows had
    /// no cursor at all in this window while the page drew one.
    pub(super) fn painter_cursor_stand_in(
        &self,
        sprite: legaia_engine_render::ui_menu_window_painters::PainterSprite,
    ) -> Vec<TextDraw> {
        legaia_engine_render::text_draws_for(
            &self.font.layout_ascii(">"),
            (sprite.x, sprite.y),
            legaia_engine_render::MENU_TEXT_GOLD,
        )
    }
}

// Windows 35 and 37's own lines are `engine-ui`'s
// `BUY_QUANTITY_PROMPT` / `BUY_QUANTITY_HELD_TAIL` / `BUY_QUANTITY_NONE_HELD`
// / `SELL_QUANTITY_HEADING`, imported with their painters: both hosts draw
// both windows, so a host-local copy is exactly the divergence the drift
// gate pairs constants to catch.

/// Window 39's two engine-authored labels, paired with the browser page's
/// constants of the same names. Retail's own strings are menu-overlay rodata
/// literals; staging them here keeps the translation layer owning the text.
const SELL_DETAIL_PRICE_LABEL: &str = "Price";
/// What window 39 prints in place of the price row when the item's `+2` buy
/// price is zero - retail's quest-item / unsellable arm.
const SELL_DETAIL_CANNOT_SELL: &str = "Cannot sell";

// Window 31's heading + unit label are `engine-ui`'s
// `POINT_CARD_HEADING` / `POINT_CARD_UNIT_LABEL` (imported above): both hosts
// draw this window, so a host-local copy here would be exactly the kind of
// silent divergence `check-ui-host-drift.py` has to pair constants to catch.

/// The per-window painters' output for one shop frame: their texts in stage
/// pixels, plus the sprite requests [`shop_marker_draws`] resolves against
/// the chrome atlas. The Point Card toast is kept apart because it draws
/// over every other window, text included.
///
/// [`shop_marker_draws`]: legaia_engine_render::shop_screen::shop_marker_draws
#[derive(Default)]
pub(super) struct ShopWindowDraws {
    pub texts: Vec<TextDraw>,
    pub marks: Vec<PainterSprite>,
    pub pictograms: Vec<PainterPictogram>,
    pub toast_texts: Vec<TextDraw>,
    pub toast_marks: Vec<PainterSprite>,
    pub toast_frame: Option<legaia_engine_render::ui_menu_window_painters::PainterRect>,
}

impl PlayWindowApp {
    /// The whole gold-shop screen for one frame, or `None` when no gold-shop
    /// screen is up (inn, seru trade, the exit beat): the frames of the
    /// phase's window set, the picker / list / party column
    /// (`engine-ui::shop_screen`), then every per-window painter above, the
    /// painters' hand and pictogram requests resolved to atlas sprites.
    ///
    /// Texts are stage pixels (the caller scales them with the rest of the
    /// shop text); sprites are surface pixels for `surface_w x surface_h`.
    /// The browser page composes the same screen through the same calls
    /// (`web-viewer::play_shop::gold_shop_screen`).
    pub(super) fn gold_shop_screen(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Option<legaia_engine_render::shop_screen::ShopScreenDraws> {
        use legaia_engine_render::shop_screen as ss;
        let phase = self.menu_runtime.shop_screen_phase()?;
        let shop = self.menu_runtime.shop_session.as_ref()?;
        let world = &self.session.host.world;
        let state = MenuState::from_byte(self.menu_runtime.ctx_state());
        let cursor = self.menu_runtime.cursor() as usize;
        // The windows on screen this frame with their slide progress
        // (`ShopSlides`, stepped by the menu tick): the phase's set plus any
        // window still sliding out. The toast's frame is laid last by hand
        // below, after the markers of the windows it covers.
        let mut slides = self.menu_runtime.shop_slides();
        if slides.is_empty() {
            slides = legaia_engine_core::shop::shop_screen_windows(phase, false)
                .into_iter()
                .map(|id| (id, legaia_engine_core::shop::SHOP_SLIDE_FRAMES))
                .collect();
        }
        let windows: Vec<usize> = slides.iter().map(|(id, _)| *id).collect();
        let (origin, scale) =
            legaia_engine_render::pause_menu::stage_transform(surface_w, surface_h);
        let ctx = ss::ShopScreenCtx {
            font: &self.font,
            rects: legaia_engine_render::pause_menu::MenuRects::new(
                self.menu_window_table.as_ref(),
            ),
            chrome: self.save_menu.as_ref().map(|m| &m.rects),
            origin,
            scale,
        };
        let bag = MenuRuntime::inventory_items(world);
        let held_of = |id: u8| -> i16 {
            bag.iter()
                .find(|(i, _)| *i == id)
                .map(|(_, q)| *q as i16)
                .unwrap_or(0)
        };
        use legaia_engine_core::shop::ShopScreenPhase as P;

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
                let rows = MenuRuntime::sell_list_rows(world);
                (
                    rows.iter().map(|r| self.shop_item_name(r.id)).collect(),
                    rows.iter().map(|r| u32::from(r.count)).collect(),
                    rows.iter()
                        .map(|r| {
                            if r.dim {
                                legaia_engine_render::SHOP_INK_GREY
                            } else {
                                legaia_engine_render::SHOP_INK_NORMAL
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
                        .map(|i| self.shop_item_name(i.item_id))
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
        let staged = self.shop_staged_item(shop, state, cursor);
        let staged = match phase {
            P::BuyRecipient => self
                .menu_runtime
                .recipient_session
                .as_ref()
                .map(|r| r.item_id),
            P::BuyQuantity => self.menu_runtime.quantity_view().map(|v| v.item_id),
            _ => staged,
        };
        let members = staged
            .map(|id| {
                legaia_engine_core::shop::party_compare_members(
                    world,
                    self.menu_runtime.equip_info.as_ref(),
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
        let mut win = self.shop_window_draws(shop, state, cursor);
        let (recip_text, recip_marks) = self.recipient_window_draws();
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
}

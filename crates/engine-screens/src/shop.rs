//! The gold shop's **retail descriptor windows** and the screen around them.
//!
//! Retail's shop is not one panel: the open script the window-script runner
//! `FUN_801D6628` interprets slides in separate windows, and each one's
//! content comes from the routine its descriptor names (see
//! `docs/subsystems/shop.md`):
//!
//! | Id | Renderer | Content fed to it |
//! |---|---|---|
//! | 33 (`0x21`) | `FUN_801DCF14` | the vendor plate - the scene MAN shop record's trailing name |
//! | 32 (`0x20`) | `FUN_801DCF84` | the purse - `World::party.money` (retail `_DAT_8008459C`) |
//! | 34 (`0x22`) | `FUN_801D4A80` | the hovered item's name / owned count / description |
//! | 35 (`0x23`) | `FUN_801D5510` | the buy quantity, held count, unit price and running total |
//! | 36 (`0x24`) | `FUN_801D56FC` | the equipment-buy recipient list |
//! | 37 (`0x25`) | `FUN_801D5944` | the sell quantity, held count and halved gold total |
//! | 39 (`0x27`) | `FUN_801D5AE8` | the sell list's detail panel - name, description, halved price, passive lines |
//! | 31 (`0x1F`) | `FUN_801DCE20` | the Point Card toast after a buy commit |
//!
//! The Buy / Sell / Quit picker (id 42, `FUN_801D4868`), the lists (40 / 38)
//! and the party compare (41) are `engine-ui::shop_screen`'s.
//!
//! Windows 34 and 39 are alternatives, not siblings: `FUN_801D5AE8` is the
//! sell-family renderer and prints the same name/description head at an
//! overlapping rect, so 34 draws for the buy list and 39 for the sell list.
//!
//! Each window resolves through [`ui::painter_at`], so an id whose descriptor
//! names a different renderer is skipped rather than mis-drawn. Windows 35
//! and 39 are ported as pens-returning kernels rather than draw-list
//! builders, so they are filtered on the descriptor's own `renderer_va`
//! ([`RENDERER_BUY_QUANTITY`], [`RENDERER_SELL_DETAIL`]) instead.
//!
//! REF: FUN_801d5de0
//! REF: FUN_801d4868

use crate::{
    RENDERER_BUY_QUANTITY, RENDERER_SELL_DETAIL, SELL_DETAIL_CANNOT_SELL, SELL_DETAIL_PRICE_LABEL,
    ScreenInputs, WIN_BUY_QUANTITY, WIN_EQUIP_TARGET, WIN_ITEM_INFO, WIN_POINT_CARD, WIN_PURSE,
    WIN_SELL_DETAIL, WIN_SELL_QUANTITY, WIN_VENDOR_PLATE, shop_item_description, shop_item_label,
};
use legaia_asset::menu_windows::MenuWindowTable;
use legaia_engine_core::menu_runtime::{MenuRuntime, MenuState};
use legaia_engine_core::shop::ShopSession;
use legaia_engine_core::world::World;
use legaia_engine_ui::ui_menu_window_painters::{
    POINT_CARD_HEADING, POINT_CARD_UNIT_LABEL, PainterPictogram, PainterRect, PainterSprite,
    SELL_QUANTITY_HEADING, amount_prompt_draws_for, buy_quantity_draws_for,
    counter_panel_draws_for, item_description_draws_for, record_title_tab_draws_for,
    sell_quantity_draws_for,
};
use legaia_engine_ui::{self as ui, TextDraw, shop_screen as ss};

/// The per-window painters' output for one shop frame: their texts in stage
/// pixels, plus the sprite requests [`ss::shop_marker_draws`] resolves
/// against the chrome atlas. The Point Card toast is kept apart because it
/// draws over every other window, text included.
#[derive(Default)]
pub struct ShopWindowDraws {
    pub texts: Vec<TextDraw>,
    pub marks: Vec<PainterSprite>,
    pub pictograms: Vec<PainterPictogram>,
    pub toast_texts: Vec<TextDraw>,
    pub toast_marks: Vec<PainterSprite>,
    pub toast_frame: Option<PainterRect>,
}

/// The live shop's vendor name.
///
/// Retail's window 33 reads it out of the armed op-`0x49` record:
/// `_DAT_8007B450` points at the opcode's **sub-op byte**, and `FUN_801DCF14`
/// starts the string at `record + record[2] + 3`. With the shop record's
/// payload `[count][count x id][name\0]`, `record[2]` is `count`, so the
/// string lands one past the last item id - the trailing ASCII name
/// `legaia_asset::shop_stock` decodes.
///
/// `ShopSession` keeps the priced stock but not that name, so the name is
/// recovered by matching the session's stock against the scene's decoded
/// shops (`World::shops.scene_shops`); a scene with one merchant resolves on
/// the first entry.
///
/// REF: FUN_801DCF14
pub fn shop_vendor_name<'w>(world: &'w World, shop: &ShopSession) -> Option<&'w str> {
    let shops = &world.shops.scene_shops;
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
/// row while a list has focus, the pending item once a quantity / confirm
/// phase owns the flow. `None` is the "not positive" case every painter in
/// this family draws nothing for - hence `Option`, not a default of id 0.
pub fn shop_staged_item(
    world: &World,
    shop: &ShopSession,
    state: Option<MenuState>,
    cursor: usize,
) -> Option<u8> {
    match state {
        Some(MenuState::ShopBuy) => shop.inventory.items.get(cursor).map(|i| i.item_id),
        Some(MenuState::ShopSell) => MenuRuntime::sell_list_rows(world).get(cursor).map(|r| r.id),
        Some(MenuState::ShopQuantity) | Some(MenuState::ShopConfirm) => shop.pending_item_id,
        _ => None,
    }
    .filter(|id| *id != 0)
}

/// The shop's retail descriptor windows for the current phase, in stage
/// pixels. Empty when the menu overlay's window table did not parse: these
/// windows exist only at their disc-parsed rects.
pub fn shop_window_draws(
    inputs: &ScreenInputs<'_>,
    shop: &ShopSession,
    state: Option<MenuState>,
    cursor: usize,
) -> ShopWindowDraws {
    let mut draws = ShopWindowDraws::default();
    let Some(table) = inputs.table else {
        return draws;
    };
    let world = inputs.world;
    let font = inputs.font;
    let menu = inputs.menu;
    let bag = MenuRuntime::inventory_items(world);
    let out = &mut draws.texts;
    let marks = &mut draws.marks;
    let pics = &mut draws.pictograms;

    // Window 33 - the vendor plate.
    if let (Some(name), Some((d, _))) = (
        shop_vendor_name(world, shop),
        ui::painter_at(
            table,
            WIN_VENDOR_PLATE,
            ui::MenuWindowPainter::RecordTitleTab,
        ),
    ) {
        out.extend(record_title_tab_draws_for(font, ui::painter_rect(d), name));
    }

    // Window 32 - the purse. The pictogram id + which total the digits print
    // both come out of the dispatch, because retail's two counter renderers
    // differ in exactly those two literals.
    let purse = table.window(WIN_PURSE);
    if let (Some(d), Some(ui::MenuWindowPainter::Counter { pictogram, source })) =
        (purse, purse.and_then(ui::painter_for))
    {
        let value = match source {
            ui::CounterSource::PartyGold => world.party.money.max(0) as u64,
            ui::CounterSource::CasinoCoins => world.minigames.casino_coins as u64,
        };
        let (digits, pic) = counter_panel_draws_for(font, ui::painter_rect(d), pictogram, value);
        out.extend(digits);
        pics.push(pic);
    }

    // Window 34 - the hovered item's info panel; the sell family (list or
    // its quantity stepper) shows window 39 in its place.
    let staged = shop_staged_item(world, shop, state, cursor);
    let selling_list = matches!(state, Some(MenuState::ShopSell))
        || menu.quantity_view().is_some_and(|v| !v.buying);
    if !selling_list
        && let Some((d, _)) =
            ui::painter_at(table, WIN_ITEM_INFO, ui::MenuWindowPainter::ItemDescription)
    {
        let id = staged.unwrap_or(0);
        let owned = bag
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, q)| *q)
            .unwrap_or(0);
        out.extend(item_description_draws_for(
            font,
            ui::painter_rect(d),
            staged.is_some(),
            &shop_item_label(world, id),
            owned,
            &shop_item_description(world, id),
        ));
    }
    if selling_list {
        let (text, pic) = sell_detail_window_draws(inputs, table, staged);
        out.extend(text);
        pics.extend(pic);
    }

    // Windows 37 / 35 - the two quantity steppers. Retail runs the quantity
    // screen as one number moving in place over a parked list, so both arms
    // read the live `QuantityPicker` (`MenuRuntime::quantity_view`) rather
    // than a list cursor.
    let quantity = menu.quantity_view();
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

    // Window 31 - the Point Card toast. Retail's buy commit hands the widget
    // VM a one-command script (`0x801E4EDC` from the quantity commit,
    // `0x801E4EA8` from the recipient picker; both decode to `[open 0x1F]` +
    // terminator) and then stalls for a press, so this draws exactly while
    // `MenuRuntime` reports the beat.
    if menu.point_card_toast().is_some()
        && let Some((d, _)) =
            ui::painter_at(table, WIN_POINT_CARD, ui::MenuWindowPainter::AmountPrompt)
    {
        let rect = ui::painter_rect(d);
        let (text, cur) = amount_prompt_draws_for(
            font,
            rect,
            POINT_CARD_HEADING,
            world.minigames.point_card.max(0) as u64,
            POINT_CARD_UNIT_LABEL,
        );
        draws.toast_texts.extend(text);
        draws.toast_marks.push(cur);
        draws.toast_frame = Some(rect);
    }
    draws
}

/// Window 39 - the **item detail / sell panel** (`FUN_801D5AE8`): the
/// hovered item's name and description, its halved sell price (or the
/// "cannot sell" line), and the accessory-passive name + description rows.
/// It replaces window 34 while the sell list has focus.
///
/// The pens are the shared kernel's
/// ([`legaia_engine_core::shop::shop_sell_detail_panel`]). The passive chain
/// is the renderer's own (`0x801D5C5C..0x801D5CC8`): the item record's `+0`
/// class byte picks which table the `+1` subtype indexes - equipment records'
/// `+5`, everything else the item-effect record's `+3`. The equipment arm
/// reads through [`legaia_engine_core::equipment::DiscEquipInfo::row_passive_index`]
/// off `MenuRuntime::equip_info`, which is row-keyed exactly as the
/// renderer's `0x80074F68 + subtype*8` is.
///
/// Returns the texts and the currency pictogram beside the price.
pub fn sell_detail_window_draws(
    inputs: &ScreenInputs<'_>,
    table: &MenuWindowTable,
    staged: Option<u8>,
) -> (Vec<TextDraw>, Option<PainterPictogram>) {
    use legaia_engine_core::shop::PASSIVE_NONE;
    let mut out = Vec::new();
    let Some(d) = table
        .window(WIN_SELL_DETAIL)
        .filter(|d| d.renderer_va == RENDERER_SELL_DETAIL)
    else {
        return (out, None);
    };
    // Retail leaves only the shade box when nothing is staged; the shop
    // screen frames that box, so no text renders.
    let Some(id) = staged else {
        return (out, None);
    };
    let world = inputs.world;
    let rect = ui::painter_rect(d);
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
                inputs
                    .menu
                    .equip_info
                    .as_ref()
                    .map(|i| i.row_passive_index(sub))
                    .unwrap_or(PASSIVE_NONE)
            },
            |sub| {
                effects
                    .descriptor(sub)
                    .map(|e| e.marker)
                    .unwrap_or(PASSIVE_NONE + 1)
            },
        )
    });
    let panel = legaia_engine_core::shop::shop_sell_detail_panel(
        (rect.x as i16, rect.y as i16),
        i32::from(id),
        price,
        passive,
    );
    let font = inputs.font;
    let text = |out: &mut Vec<TextDraw>, s: &str, pen: (i16, i16), ink: [f32; 4]| {
        out.extend(ui::text_draws_for(
            &font.layout_ascii(s),
            (i32::from(pen.0), i32::from(pen.1)),
            ink,
        ));
    };
    text(
        &mut out,
        &shop_item_label(world, id),
        panel.name_pen,
        ui::MENU_TEXT_GOLD,
    );
    // Retail hands the description words to the line-breaking printer
    // (`FUN_800337B0` / `FUN_8003CD00` -> `FUN_80036888`), so a `|` puts the
    // rest on the next row.
    let broken = |out: &mut Vec<TextDraw>, s: &str, pen: (i16, i16), ink: [f32; 4]| {
        out.extend(ui::ui_menu_window_painters::broken_text_draws_for(
            font,
            s,
            (i32::from(pen.0), i32::from(pen.1)),
            ink,
        ));
    };
    let desc = shop_item_description(world, id);
    if !desc.is_empty() {
        broken(&mut out, &desc, panel.desc_pen, ui::MENU_TEXT_WHITE);
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
            out.extend(ss::shop_digit_field_draws(
                font,
                u32::from(row.price),
                (i32::from(row.value_pen.0), i32::from(row.value_pen.1)),
                ss::SELL_DETAIL_PRICE_CELLS,
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
    if let Some((name, line)) = panel
        .passive
        .and(world.menu.text.as_ref())
        .and_then(|t| t.item_passive_text(id))
    {
        text(&mut out, &name, panel.passive_name_pen, ui::MENU_TEXT_GREEN);
        broken(&mut out, &line, panel.passive_desc_pen, ui::MENU_TEXT_WHITE);
    }
    let pic = panel.sell.map(|row| PainterPictogram {
        id: ui::COUNTER_PICTOGRAM_GOLD,
        x: i32::from(row.icon_pen.0),
        y: i32::from(row.icon_pen.1),
    });
    (out, pic)
}

/// Window 36 (`0x24`, `FUN_801D56FC`) of the **equipment-buy recipient
/// flow** (menu-overlay sub-screen `0x1C`, `FUN_801DB380`): the bag row plus
/// one row per member, greyed by the character mask, drawn while
/// `MenuRuntime::recipient_session` owns the pad. Retail's picker script
/// `0x801E4E84` opens window 36 and nothing else; window 41, the party
/// compare beside it, belongs to the whole buy flow's window set and draws
/// through [`ss::shop_screen_draws`].
///
/// Returns the texts and the hand request, which the shop screen resolves to
/// a sprite.
pub fn recipient_window_draws(inputs: &ScreenInputs<'_>) -> (Vec<TextDraw>, Vec<PainterSprite>) {
    let Some(session) = inputs.menu.recipient_session.as_ref() else {
        return (Vec::new(), Vec::new());
    };
    let Some(table) = inputs.table else {
        return (Vec::new(), Vec::new());
    };
    let members = legaia_engine_core::shop::party_compare_members(
        inputs.world,
        inputs.menu.equip_info.as_ref(),
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
    ui::recipient_picker_draws_for(inputs.font, rects, &view)
}

/// The whole gold-shop screen for one frame, or `None` when no gold-shop
/// screen is up (inn, seru trade, the exit beat): the frames of the phase's
/// window set, the picker / list / party column (`engine-ui::shop_screen`),
/// then every per-window painter above, the painters' hand and pictogram
/// requests resolved to atlas sprites.
///
/// Texts are stage pixels; sprites are surface pixels for
/// `surface_w x surface_h`.
pub fn gold_shop_screen(
    inputs: &ScreenInputs<'_>,
    surface_w: u32,
    surface_h: u32,
) -> Option<ss::ShopScreenDraws> {
    use legaia_engine_core::shop::ShopScreenPhase as P;
    let menu = inputs.menu;
    let phase = menu.shop_screen_phase()?;
    // The field is still fading to black: no window is up yet.
    if menu.shop_fade_level().is_some() {
        return Some(Default::default());
    }
    let shop = menu.shop_session.as_ref()?;
    let world = inputs.world;
    let state = MenuState::from_byte(menu.ctx_state());
    let cursor = menu.cursor() as usize;
    // The windows on screen this frame with their slide progress
    // (`ShopSlides`, stepped by the menu tick): the phase's set plus any
    // window still sliding out. The toast's frame is laid last by hand below,
    // after the markers of the windows it covers.
    let mut slides = menu.shop_slides();
    if slides.is_empty() {
        slides = legaia_engine_core::shop::shop_screen_windows(phase, false)
            .into_iter()
            .map(|id| (id, legaia_engine_core::shop::SHOP_SLIDE_FRAMES))
            .collect();
    }
    let windows: Vec<usize> = slides.iter().map(|(id, _)| *id).collect();
    let (origin, scale) = ui::pause_menu::stage_transform(surface_w, surface_h);
    let ctx = ss::ShopScreenCtx {
        font: inputs.font,
        rects: ui::pause_menu::MenuRects::new(inputs.table),
        chrome: inputs.chrome,
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
                rows.iter().map(|r| shop_item_label(world, r.id)).collect(),
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
                    .map(|i| shop_item_label(world, i.item_id))
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
    // The page triangles: only while the list itself has the pad, and
    // through the kernel's blink gate.
    let arrows =
        matches!(phase, P::BuyList | P::SellList) && ss::page_arrows_blink_on(menu.ui_frame());
    let list = ss::ShopListView {
        kind,
        rows: &rows,
        cursor: list_cursor,
        browsing,
        arrows,
    };

    // Window 41 - the party column for the staged item.
    let staged = match phase {
        P::BuyRecipient => menu.recipient_session.as_ref().map(|r| r.item_id),
        P::BuyQuantity => menu.quantity_view().map(|v| v.item_id),
        _ => shop_staged_item(world, shop, state, cursor),
    };
    let members = staged
        .map(|id| {
            legaia_engine_core::shop::party_compare_members(world, menu.equip_info.as_ref(), id)
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

    // The per-window painters (vendor plate, purse, item info, the quantity
    // steppers, the sell detail, the recipient list, the toast).
    let mut win = shop_window_draws(inputs, shop, state, cursor);
    let (recip_text, recip_marks) = recipient_window_draws(inputs);
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
    // other window's sprites, its text after every other window's text, and
    // no text it covers.
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

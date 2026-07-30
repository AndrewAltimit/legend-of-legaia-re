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
//! | 32 (`0x20`) | `FUN_801DCF84` | the purse - `World::money` (retail `_DAT_8008459C`) |
//! | 34 (`0x22`) | `FUN_801D4A80` | the hovered item's name / owned count / description |
//! | 37 (`0x25`) | `FUN_801D5944` | the sell quantity, held count and halved gold total |
//!
//! The remaining two are the Buy / Sell / Quit picker (id 42, `FUN_801D4868`,
//! whose rows + ink are `engine-core::shop::shop_root_command_rows`) and the
//! renderer-less list container (id 40), whose content is the host's list.
//!
//! Each window resolves through
//! [`legaia_engine_render::painter_at`], so an id whose descriptor names a
//! different renderer is skipped rather than mis-drawn: the id is the lookup
//! key and the renderer is the authority.
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
    POINT_CARD_HEADING, POINT_CARD_UNIT_LABEL, amount_prompt_draws_for, counter_panel_draws_for,
    item_description_draws_for, record_title_tab_draws_for, sell_quantity_draws_for,
};

/// Vendor-name plate (`0x21`): the record-sourced title tab.
const WIN_VENDOR_PLATE: usize = 33;
/// Purse (`0x20`): the party-gold counter.
const WIN_PURSE: usize = 32;
/// Item info (`0x22`): name + owned count + description.
const WIN_ITEM_INFO: usize = 34;
/// Sell quantity (`0x25`): quantity, held count, halved total.
const WIN_SELL_QUANTITY: usize = 37;
/// Point Card toast (`0x1F`): heading, 8-digit bank, unit label, cursor.
const WIN_POINT_CARD: usize = 31;

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
    /// scene's decoded shops (`World::scene_shops`); a scene with one merchant
    /// resolves on the first entry.
    ///
    /// REF: FUN_801DCF14
    fn shop_vendor_name(&self, shop: &ShopSession) -> Option<&str> {
        let shops = &self.session.host.world.scene_shops;
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
        bag: &[(u8, u8)],
    ) -> Option<u8> {
        match state {
            Some(MenuState::ShopBuy) => shop.inventory.items.get(cursor).map(|i| i.item_id),
            Some(MenuState::ShopSell) => bag.get(cursor).map(|(id, _)| *id),
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
    ) -> Vec<TextDraw> {
        let Some(table) = self.menu_window_table.as_ref() else {
            return Vec::new();
        };
        let world = &self.session.host.world;
        let bag = MenuRuntime::inventory_items(world);
        let mut out = Vec::new();

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
                legaia_engine_render::CounterSource::PartyGold => world.money.max(0) as u64,
                legaia_engine_render::CounterSource::CasinoCoins => world.casino_coins as u64,
            };
            let rect = legaia_engine_render::painter_rect(d);
            let (digits, pic) = counter_panel_draws_for(&self.font, rect, pictogram, value);
            out.extend(digits);
            out.extend(self.painter_pictogram_stand_in(pic));
        }

        // Window 34 - the hovered item's info panel.
        let staged = self.shop_staged_item(shop, state, cursor, &bag);
        if let Some((d, _)) = legaia_engine_render::painter_at(
            table,
            WIN_ITEM_INFO,
            MenuWindowPainter::ItemDescription,
        ) {
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

        // Window 37 - the sell quantity panel, while the sell flow is sizing
        // a stack.
        let selling = matches!(state, Some(MenuState::ShopQuantity)) && !shop.pending_is_buying;
        if let Some((d, _)) = legaia_engine_render::painter_at(
            table,
            WIN_SELL_QUANTITY,
            MenuWindowPainter::SellQuantity,
        ) {
            let id = staged.unwrap_or(0);
            let held = bag
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, q)| u32::from(*q))
                .unwrap_or(0);
            // Retail reads the item record's own `+2` buy price and halves
            // the product; the shop's stock list is not consulted, so a bag
            // item the merchant does not stock still prices correctly.
            let unit_price = world
                .item_shop_data
                .as_ref()
                .map(|d| u32::from(d.price(id)))
                .unwrap_or(0);
            let rect = legaia_engine_render::painter_rect(d);
            let (text, pic, cur) = sell_quantity_draws_for(
                &self.font,
                rect,
                selling && staged.is_some(),
                SELL_QUANTITY_HEADING,
                u32::from(shop.pending_quantity),
                held,
                unit_price,
            );
            out.extend(text);
            if let Some(pic) = pic {
                out.extend(self.painter_pictogram_stand_in(pic));
            }
            if let Some(cur) = cur {
                out.extend(self.painter_cursor_stand_in(cur));
            }
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
            let points = world.point_card.max(0) as u64;
            let (text, cur) = amount_prompt_draws_for(
                &self.font,
                rect,
                POINT_CARD_HEADING,
                points,
                POINT_CARD_UNIT_LABEL,
            );
            out.extend(text);
            out.extend(self.painter_cursor_stand_in(cur));
        }
        out
    }

    /// The three retail windows of the **equipment-buy recipient flow**
    /// (menu-overlay sub-screen `0x1C`, `FUN_801DB380`), drawn while
    /// [`legaia_engine_core::menu_runtime::MenuRuntime::recipient_session`]
    /// owns the pad and the buy list stays parked behind it:
    ///
    /// | Id | Renderer | Content |
    /// |---|---|---|
    /// | 36 (`0x24`) | `FUN_801D56FC` | bag row + one row per member, greyed by the character mask |
    /// | 25 (`0x19`) | `FUN_801D1290` | the highlighted member's stat compare - **an engine addition**, see below |
    /// | 41 (`0x29`) | `FUN_801D4C28` | the party-wide ATK / UDF / LDF compare |
    ///
    /// Retail's picker script `0x801E4E84` opens window 36 and nothing else,
    /// over the shop set the entry script `0x801E4E64` already put up - which
    /// includes window 41 but not 25. Window `0x19` is named by one open
    /// command in the whole menu overlay, the Equip screen's, so this host
    /// paints one panel more than retail does; the divergence is recorded on
    /// `engine-ui::recipient_picker_draws_for` and is symmetric across hosts.
    ///
    /// The layout is [`legaia_engine_render::recipient_picker_draws_for`],
    /// the same shared composition the browser play page calls
    /// (`web-viewer::play_shop::recipient_window_draws`) - this method only
    /// resolves the rects off the disc window table and builds the model.
    pub(super) fn recipient_window_draws(&self) -> Vec<TextDraw> {
        use legaia_engine_render::{
            EquipStatBlock, MenuWindowPainter, RecipientMemberView, RecipientPickerView,
            RecipientWindowRects, painter_at, painter_rect, recipient_picker_draws_for,
        };
        /// Equip-target recipient list (`0x24`).
        const WIN_EQUIP_TARGET: usize = 36;
        // Window 25 (`0x19`, the active-character stat compare) is not a shop
        // window: its only opener in the menu overlay is the Equip screen's.
        /// Party-wide stat compare (`0x29`).
        const WIN_COMPARE_PARTY: usize = 41;

        let Some(session) = self.menu_runtime.recipient_session.as_ref() else {
            return Vec::new();
        };
        let Some(table) = self.menu_window_table.as_ref() else {
            return Vec::new();
        };
        let world = &self.session.host.world;
        let item_id = session.item_id;
        let members: Vec<&legaia_save::CharacterRecord> = world
            .roster
            .members
            .iter()
            .take(session.can_equip.len())
            .collect();
        let labels: Vec<String> = members
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let n = r.name();
                if n.is_empty() {
                    format!("Member {}", i + 1)
                } else {
                    n
                }
            })
            .collect();
        // The slot the disc category names, and what currently sits in it -
        // both feed the trial-equip (candidate) stat blocks.
        let slot_idx = self
            .menu_runtime
            .equip_info
            .as_ref()
            .and_then(|i| i.entry(item_id))
            .map(|e| {
                use legaia_asset::equip_stats::EquipSlot as Disc;
                use legaia_engine_core::equipment::EquipSlot;
                match e.category {
                    Disc::Weapon => EquipSlot::Weapon,
                    Disc::Body => EquipSlot::BodyArmor,
                    Disc::Head => EquipSlot::Helmet,
                    Disc::Footwear => EquipSlot::Boot,
                }
                .as_index() as usize
            });
        let candidate_for =
            |rec: &legaia_save::CharacterRecord, current: EquipStatBlock| -> EquipStatBlock {
                let displaced = slot_idx.map(|idx| rec.equipment().slots[idx]).unwrap_or(0);
                let old_m = world
                    .equipment_table
                    .get(displaced)
                    .copied()
                    .unwrap_or_default();
                let new_m = world
                    .equipment_table
                    .get(item_id)
                    .copied()
                    .unwrap_or_default();
                let mut cand = current;
                cand.atk += i32::from(new_m.atk) - i32::from(old_m.atk);
                cand.udf += i32::from(new_m.udf) - i32::from(old_m.udf);
                cand.ldf += i32::from(new_m.ldf) - i32::from(old_m.ldf);
                cand.spd += i32::from(new_m.spd) - i32::from(old_m.spd);
                cand.int += i32::from(new_m.int) - i32::from(old_m.int);
                cand
            };

        let rows: Vec<RecipientMemberView<'_>> = members
            .iter()
            .zip(labels.iter())
            .enumerate()
            .map(|(i, (rec, label))| {
                let current = EquipStatBlock::from_character_record(&rec.raw).unwrap_or_default();
                let hms = rec.hp_mp_sp();
                RecipientMemberView {
                    name: label.as_str(),
                    equippable: session.can_equip.get(i).copied().unwrap_or(false),
                    already_equipped: rec.equipment().slots.contains(&item_id),
                    current,
                    candidate: candidate_for(rec, current),
                    hp_max: hms.hp_max,
                    mp_max: hms.mp_max,
                }
            })
            .collect();

        let rects = RecipientWindowRects {
            target_list: painter_at(table, WIN_EQUIP_TARGET, MenuWindowPainter::EquipTargetList)
                .map(|(d, _)| painter_rect(d)),
            // No window 25: retail's picker script opens only window 36, and
            // the shop's stat compare is window 41 below.
            party_compare: painter_at(
                table,
                WIN_COMPARE_PARTY,
                MenuWindowPainter::PartyStatCompare,
            )
            .map(|(d, _)| painter_rect(d)),
        };
        let view = RecipientPickerView {
            heading: legaia_engine_render::RECIPIENT_HEADING,
            cursor: session.cursor,
            members: &rows,
            // The picker only ever opens on the buy list's **equipment**
            // route (`shop::buy_list_confirm_route` kind `1`), and every
            // equipment bonus record on the disc carries the `0x40`
            // no-passive sentinel in its `+5` compare-category byte - so
            // this constant is the byte, not a fallback. The browser host
            // looks the byte up in its parsed `EquipStatTable`; the two
            // agree by construction, and `engine-ui`'s
            // `the_equipment_category_sentinel_selects_the_atk_triple`
            // pins the equivalence.
            staged_category: legaia_engine_render::CATEGORY_DEFAULT,
        };
        let (mut out, sprites) = recipient_picker_draws_for(&self.font, rects, &view);
        for s in sprites {
            out.extend(self.painter_cursor_stand_in(s));
        }
        out
    }

    /// Item display name, falling back to the id when the disc text tables
    /// are unavailable.
    fn shop_item_name(&self, id: u8) -> String {
        self.session
            .host
            .world
            .menu_text
            .as_ref()
            .and_then(|t| t.item_name(id))
            .map(str::to_string)
            .unwrap_or_else(|| format!("item {id:02}"))
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
        let Some(text) = self.session.host.world.menu_text.as_ref() else {
            return String::new();
        };
        if let Some((_, desc)) = text.item_passive_lines(id) {
            return desc;
        }
        text.item_desc(id).unwrap_or_default().to_string()
    }

    /// ASCII stand-in for a painter's pictogram request until the UI-icon
    /// atlas page carrying the currency glyphs is uploaded.
    fn painter_pictogram_stand_in(
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

    /// ASCII stand-in for a painter's cursor / marker sprite request.
    /// (`pub(super)`: the window-7 spell level-up notice in `menu_draws`
    /// shares it.)
    pub(super) fn painter_cursor_stand_in(
        &self,
        sprite: legaia_engine_render::ui_menu_window_painters::PainterSprite,
    ) -> Vec<TextDraw> {
        if self.save_menu.is_some() {
            // The sprite pass owns the hand cursor whenever the atlas is
            // resident; drawing both would double it.
            return Vec::new();
        }
        legaia_engine_render::text_draws_for(
            &self.font.layout_ascii(">"),
            (sprite.x, sprite.y),
            legaia_engine_render::MENU_TEXT_GOLD,
        )
    }
}

/// Heading window 37 draws above its quantity row. Retail's own string is a
/// menu-overlay rodata literal (`0x801CEC38`); the port stages an
/// engine-authored line in the same slot so the translation layer owns the
/// text.
const SELL_QUANTITY_HEADING: &str = "How many?";

// Window 31's heading + unit label are `engine-ui`'s
// `POINT_CARD_HEADING` / `POINT_CARD_UNIT_LABEL` (imported above): both hosts
// draw this window, so a host-local copy here would be exactly the kind of
// silent divergence `check-ui-host-drift.py` has to pair constants to catch.

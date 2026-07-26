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
//! * **Real item names.** The native shop rows are placeholder `"Item"`
//!   labels; the page resolves the SCUS item table it already parses at
//!   `load_disc`. Same draw builder, populated row labels.
//!
//! Row inks come from the retail kernels
//! `legaia_engine_core::shop::{shop_root_command_rows, shop_stock_row_ink}`
//! (`FUN_801D4868` / `FUN_801D5DE0`), so an empty bag greys the Sell row and a
//! full stack / unaffordable price greys a stock row on this host too.
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
//! | 32 (`0x20`) | `FUN_801DCF84` | purse - `World::money` (retail `_DAT_8008459C`) |
//! | 34 (`0x22`) | `FUN_801D4A80` | hovered item's name / owned count / description |
//! | 37 (`0x25`) | `FUN_801D5944` | sell quantity, held count, halved gold total |
//!
//! Each resolves through [`ui::painter_at`], so an id whose descriptor names a
//! different renderer is skipped rather than mis-drawn. They draw only when
//! the real descriptor table parsed: these windows exist at their disc rects
//! or not at all, and [`crate::play_menu`]'s pinned-rect fallback cannot
//! invent a `renderer_va`.
//!
//! REF: FUN_801d5de0
//! REF: FUN_801d4868

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::menu_runtime::{MenuInput, MenuState, shop_menu_rows};
use legaia_engine_core::shop::ShopSession;
use legaia_engine_ui::ui_menu_window_painters::{
    counter_panel_draws_for, item_description_draws_for, record_title_tab_draws_for,
    sell_quantity_draws_for,
};
use legaia_engine_ui::{self as ui, ShopRow, SpriteDraw, TextDraw};
use wasm_bindgen::prelude::*;

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
/// Sell quantity (`0x25`): quantity, held count, halved total.
const WIN_SELL_QUANTITY: usize = 37;
/// Heading window 37 draws above its quantity row. Retail's own string is a
/// menu-overlay rodata literal (`0x801CEC38`); both hosts stage the same
/// engine-authored line so the translation layer owns the text.
const SELL_QUANTITY_HEADING: &str = "How many?";
/// Stage-pixel pen for the level-up banner (native `(8, 60)`).
const LEVEL_UP_PEN: (i32, i32) = (8, 60);
/// Stage-pixel pen for the Seru-capture banner (native `(8, 40)`).
const CAPTURE_PEN: (i32, i32) = (8, 40);

/// Pack a pad word into the `MenuInput` the menu VM steps on.
fn menu_input(edge: u16) -> MenuInput {
    MenuInput {
        cross: edge & 0x4000 != 0,
        circle: edge & 0x2000 != 0,
        triangle: edge & 0x1000 != 0,
        square: edge & 0x8000 != 0,
        up: edge & 0x0010 != 0,
        down: edge & 0x0040 != 0,
        left: edge & 0x0080 != 0,
        right: edge & 0x0020 != 0,
    }
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
    }

    /// Display label for item `id` off the SCUS item table, falling back to
    /// the raw id when no executable was loaded (PROT.DAT-only session).
    fn shop_item_label(&self, id: u8) -> String {
        self.scene_host
            .as_ref()
            .and_then(|h| h.world.menu_text.as_ref())
            .and_then(|t| t.item_name(id))
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Item {id:02X}"))
    }

    /// Build the shop panel's text draws in **stage** pixels, or `None` when
    /// no shop is up. Row labels + prices come from the live session; the
    /// geometry is `engine-ui`'s.
    fn shop_stage_draws(&self, font: &legaia_font::Font) -> Option<Vec<TextDraw>> {
        let shop = self.menu.shop_session.as_ref()?;
        let state = MenuState::from_byte(self.menu.ctx_state());
        let cursor = self.menu.cursor() as usize;
        let world = self.scene_host.as_ref().map(|h| &h.world)?;
        let gold = world.money;

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
        let (rows_spec, show_gold): (Vec<ShopRowSpec>, Option<i32>) = match state {
            // Top picker: Buy / Sell / (Trade) / Exit, matching the runtime's
            // dynamic row layout. The Sell row's ink follows retail's bag scan
            // (`shop_root_command_rows`): an empty bag greys it.
            Some(MenuState::ShopMenu) => {
                let sellable = !bag.is_empty();
                let ink =
                    legaia_engine_core::shop::shop_root_command_rows((0, 0), 0x4000, sellable);
                (
                    shop_menu_rows(world.seru_trade_enabled())
                        .iter()
                        .map(|s| {
                            let (label, ink) = match s {
                                MenuState::ShopBuy => ("Buy", ink[0].ink),
                                MenuState::ShopSell => ("Sell", ink[1].ink),
                                MenuState::ShopTrade => ("Trade Seru", ink[0].ink),
                                _ => ("Exit", ink[0].ink),
                            };
                            (label.to_string(), None, ink)
                        })
                        .collect(),
                    Some(gold),
                )
            }
            Some(MenuState::ShopBuy) => (
                shop.inventory
                    .items
                    .iter()
                    .map(|item| {
                        let ink = legaia_engine_core::shop::shop_stock_row_ink(
                            held_of(item.item_id),
                            0,
                            gold,
                            item.price as i32,
                        );
                        (self.shop_item_label(item.item_id), Some(item.price), ink)
                    })
                    .collect(),
                Some(gold),
            ),
            Some(MenuState::ShopSell) => (
                bag.iter()
                    .map(|(id, qty)| {
                        (
                            format!("{} x{}", self.shop_item_label(*id), qty),
                            None,
                            ui::SHOP_INK_NORMAL,
                        )
                    })
                    .collect(),
                Some(gold),
            ),
            Some(MenuState::ShopQuantity) => (
                (1u32..=9)
                    .map(|n| (n.to_string(), None, ui::SHOP_INK_NORMAL))
                    .collect(),
                None,
            ),
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
        Some(ui::shop_draws_for(
            font, title, &rows, cursor, show_gold, SHOP_PEN,
        ))
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
        let shops = &self.scene_host.as_ref()?.world.scene_shops;
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
            .and_then(|h| h.world.menu_text.as_ref())
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

    /// The shop's four **retail descriptor windows** for the current phase, in
    /// stage pixels. Empty when the menu-overlay window table did not parse:
    /// these windows exist only at their disc-parsed rects.
    fn shop_window_draws(
        &self,
        font: &legaia_font::Font,
        shop: &ShopSession,
        state: Option<MenuState>,
        cursor: usize,
    ) -> Vec<TextDraw> {
        let Some(assets) = self.menu_assets.as_ref() else {
            return Vec::new();
        };
        let Some(table) = assets.window_table() else {
            return Vec::new();
        };
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
            return Vec::new();
        };
        let bag = legaia_engine_core::menu_runtime::MenuRuntime::inventory_items(world);
        let held_of = |id: u8| -> u32 {
            bag.iter()
                .find(|(i, _)| *i == id)
                .map(|(_, q)| u32::from(*q))
                .unwrap_or(0)
        };
        let mut out = Vec::new();

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
                ui::CounterSource::PartyGold => world.money.max(0) as u64,
                ui::CounterSource::CasinoCoins => world.casino_coins as u64,
            };
            let rect = ui::painter_rect(d);
            let (digits, pic) = counter_panel_draws_for(font, rect, pictogram, value);
            out.extend(digits);
            let glyph = match pic.id {
                ui::COUNTER_PICTOGRAM_GOLD => "G",
                ui::COUNTER_PICTOGRAM_COINS => "C",
                _ => "*",
            };
            out.extend(self.painter_glyph_stand_in(font, glyph, (pic.x, pic.y)));
        }

        // Window 34 - the hovered item's info panel.
        let staged = self.shop_staged_item(shop, state, cursor, &bag);
        if let Some((d, _)) =
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

        // Window 37 - the sell quantity panel, while the sell flow is sizing
        // a stack.
        let selling = matches!(state, Some(MenuState::ShopQuantity)) && !shop.pending_is_buying;
        if let Some((d, _)) = ui::painter_at(
            table,
            WIN_SELL_QUANTITY,
            ui::MenuWindowPainter::SellQuantity,
        ) {
            let id = staged.unwrap_or(0);
            // Retail reads the item record's own `+2` buy price and halves the
            // product; the merchant's stock list is not consulted, so a bag
            // item the shop does not sell still prices correctly.
            let unit_price = world
                .item_shop_data
                .as_ref()
                .map(|t| u32::from(t.price(id)))
                .unwrap_or(0);
            let (text, pic, cur) = sell_quantity_draws_for(
                font,
                ui::painter_rect(d),
                selling && staged.is_some(),
                SELL_QUANTITY_HEADING,
                u32::from(shop.pending_quantity),
                held_of(id),
                unit_price,
            );
            out.extend(text);
            if let Some(pic) = pic {
                out.extend(self.painter_glyph_stand_in(font, "G", (pic.x, pic.y)));
            }
            if let Some(cur) = cur {
                out.extend(self.painter_glyph_stand_in(font, ">", (cur.x, cur.y)));
            }
        }
        out
    }

    /// Post-action banner draws in **stage** pixels: the level-up summary and
    /// the Seru-capture line, both ticked down by `World::tick`.
    fn banner_stage_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let mut out = Vec::new();
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
            return out;
        };
        if let Some(b) = world.current_level_up_banner.as_ref() {
            out.extend(ui::level_up_draws_for(
                font,
                b.char_id,
                b.new_level,
                b.hp_gained,
                b.mp_gained,
                LEVEL_UP_PEN,
            ));
        }
        if let Some(b) = world.current_capture_banner.as_ref()
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
            .is_some_and(|h| h.world.item_shop_data.is_some())
    }

    /// Is the op-`0x49` shop gate still held (i.e. the field VM suspended)?
    pub fn debug_field_shop_gate_held(&self) -> bool {
        self.scene_host
            .as_ref()
            .is_some_and(|h| h.world.field_shop_open)
    }

    /// Arm + open a shop the way a merchant's op-`0x49` sub-0 record would,
    /// stocked from the real price table. Returns `false` when no catalog is
    /// installed (nothing to price a stock list with).
    pub fn debug_open_test_shop(&mut self) -> bool {
        let Some(host) = self.scene_host.as_mut() else {
            return false;
        };
        let Some(data) = host.world.item_shop_data.as_ref() else {
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
        host.world.field_shop_armed = true;
        host.world.field_shop_open = true;
        self.menu
            .open_shop_menu(legaia_engine_core::shop::ShopSession::new(inv));
        true
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// `true` while a field-VM merchant shop is up. The page freezes field
    /// input and routes pad edges to [`Self::play_shop_input`] while this
    /// holds, the same way it defers to the pause menu.
    pub fn play_shop_is_open(&self) -> bool {
        self.menu.shop_session.is_some()
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
        if self.menu.shop_session.is_none() {
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
        if self.menu.shop_session.is_none()
            && let Some(host) = self.scene_host.as_mut()
            && host.world.field_shop_open
        {
            host.world.finish_field_shop();
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
        if !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        let Some(assets) = self.menu_assets.as_ref() else {
            return CLOSED.to_string();
        };
        let font = assets.font_ref();
        let chrome = assets.chrome_rects();
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));

        let shop = self.shop_stage_draws(font);
        // The retail descriptor windows ride alongside the engine's own
        // interactive list, exactly as they do in the native window: the list
        // is the control, these are the readouts around it.
        let windows = match self.menu.shop_session.as_ref() {
            Some(session) => self.shop_window_draws(
                font,
                session,
                MenuState::from_byte(self.menu.ctx_state()),
                self.menu.cursor() as usize,
            ),
            None => Vec::new(),
        };
        let banners = self.banner_stage_draws(font);
        if shop.is_none() && windows.is_empty() && banners.is_empty() {
            return CLOSED.to_string();
        }

        let mut sprites: Vec<SpriteDraw> = Vec::new();
        let mut texts: Vec<TextDraw> = Vec::new();
        if let Some(draws) = shop {
            // Frame the panel in the same gold 9-slice the pause menu uses,
            // sized to the row count (title row + one row per entry at the
            // builder's 14-px pitch).
            if let Some(rects) = chrome {
                let rows = draws
                    .iter()
                    .map(|d| d.dst.1)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    .max(1) as i32;
                sprites.extend(ui::menu_window_chrome_draws_for(
                    rects,
                    (SHOP_PEN.0 - 8, SHOP_PEN.1 - 8, 200, rows * 14 + 12),
                    origin,
                    scale,
                ));
            }
            texts.extend(draws);
        }
        texts.extend(windows);
        texts.extend(banners);
        ui::scale_stage_text_draws(&mut texts, origin, scale);

        serde_json::json!({
            "open": true,
            "sprites": sprites.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

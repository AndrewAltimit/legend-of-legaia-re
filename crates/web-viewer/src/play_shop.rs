//! Browser **field shop** + post-action **banner overlays**.
//!
//! Both halves are pure wiring: the state machine is
//! [`legaia_engine_core::menu_runtime::MenuRuntime`] and the screens are
//! [`legaia_engine_screens::shop_overlay_frame`] - the composition the native
//! `play-window` calls. Nothing here re-implements a screen; it projects the
//! shared draw lists into the page's `{ sprites, texts }` quad JSON, exactly
//! as [`crate::play_menu`] and [`crate::play_dialog`] do.
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
//! # Divergence from the native window
//!
//! None. Both hosts step the open session through the one per-tick kernel
//! `MenuRuntime::step_field_session` on the tick's pad **edges** (the native
//! window once fed the held word, which walked the cursor at 60 rows a
//! second), once per sim tick (this page once stepped it per display frame,
//! which ran the tick-counted fade and window slides at the monitor's rate),
//! and release the merchant op on the same test (this page once released it
//! while the buy list's quantity / recipient sub-screens were still up). Both
//! resolve their row labels from the disc item table (`World::menu.text`).
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
//! # The screens themselves
//!
//! The shop's retail descriptor windows, the gold-shop screen, the prize
//! exchange, the coin counter, the fallback panel (seru trade, inn, the
//! `[label]` stand-in) with its gold frame, and the level-up / capture
//! banners all come from one composition the native window calls too:
//! [`legaia_engine_screens::shop_overlay_frame`]. This page only assembles its
//! inputs, scales its stage texts and serialises the quads.

use crate::runtime::LegaiaRuntime;
use legaia_engine_screens::{ScreenInputs, ShopOverlayFrame};
use legaia_engine_ui::{self as ui, SpriteDraw, TextDraw};
use wasm_bindgen::prelude::*;

impl LegaiaRuntime {
    /// Hand a field-VM-armed shop to the menu runtime. Called once per
    /// [`LegaiaRuntime::tick_frame`], mirroring the native window's
    /// `take_pending_field_shop` drain.
    pub(crate) fn poll_field_shop(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let mut opened = false;
        if let Some(shop) = host.world.take_pending_field_shop() {
            self.menu.open_shop_menu(shop);
            opened = true;
        }
        // The casino prize counter (op-0x49 sub-7), same drain shape.
        if let Some(exchange) = host.world.take_pending_prize_exchange() {
            self.menu.open_prize_exchange(exchange);
            opened = true;
        }
        // The screen takes its first step on the tick that opened it, on no
        // edge - the native window's `menu_edge = 0` beat. The press this
        // tick carried already went to the field (the Cross that closed the
        // merchant's line), and handing it to the screen too would commit
        // the picker's first row on the same press. Without the step the
        // page's fade and window slides ran one tick behind the window's.
        if !opened {
            return;
        }
        let cue = self.menu.step_field_session(&mut host.world, 0);
        if let Some(cue) = cue {
            self.play_sfx(u32::from(cue));
        }
        self.rebuild_screen_geom();
    }

    /// The shop-family overlay for this frame over a `surface_w x surface_h`
    /// canvas - the shared composition (`legaia_engine_screens`) over this
    /// page's holders. Empty without a scene host.
    fn shop_overlay_frame(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> ShopOverlayFrame {
        let Some(host) = self.scene_host.host() else {
            return ShopOverlayFrame::default();
        };
        let coin_counter = host.coin_counter_lines();
        let inputs = ScreenInputs {
            world: &host.world,
            menu: &self.menu,
            font: assets.font_ref(),
            table: assets.window_table(),
            chrome: assets.chrome_rects(),
            seru_names: self.seru_names.as_ref(),
            coin_counter: &coin_counter,
        };
        legaia_engine_screens::shop_overlay_frame(&inputs, surface_w.max(1), surface_h.max(1))
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
            .host()
            .is_some_and(|h| h.world.shops.item_shop_data.is_some())
    }

    /// Is the op-`0x49` shop gate still held (i.e. the field VM suspended)?
    pub fn debug_field_shop_gate_held(&self) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| h.world.shops.shop_open)
    }

    /// Arm + open a shop the way a merchant's op-`0x49` sub-0 record would,
    /// stocked from the real price table. Returns `false` when no catalog is
    /// installed (nothing to price a stock list with).
    pub fn debug_open_test_shop(&mut self) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
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

    /// Arm the casino prize counter exactly as a clerk's `49 07 <block>`
    /// does (`World::try_arm_prize_exchange` on the instruction bytes), so
    /// the page's own drain opens it on the next frame. `false` when the
    /// scene carries no prize block `block` (the blocks are read off the
    /// disc at boot) or a counter is already armed.
    pub fn debug_arm_prize_exchange(&mut self, block: u8) -> bool {
        self.scene_host
            .host_mut()
            .is_some_and(|h| h.world.try_arm_prize_exchange(&[0x49, 0x07, block]))
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
        let Some(host) = self.scene_host.host_mut() else {
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
        let Some(host) = self.scene_host.host_mut() else {
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

    /// Drive the open shop **one sim tick** from an edge-triggered PSX pad
    /// word (same bit layout as [`Self::set_pad`]). The page calls it once per
    /// sim step it drains, with the frame's edges on the first step and `0`
    /// on the rest - the shop's fade and window slides are tick-counted, and
    /// one call per display frame ran them at the monitor's rate.
    ///
    /// The step is the shared
    /// [`MenuRuntime::step_field_session`](legaia_engine_core::menu_runtime::MenuRuntime::step_field_session),
    /// the native window's call too: the tick, the shop's blip, and - once the
    /// whole runtime has closed - `World::finish_field_shop`, so the suspended
    /// op-`0x49` flips Armed -> Done and the field VM advances past the
    /// merchant op on its next step.
    pub fn play_shop_input(&mut self, edge: u16) {
        // Disjoint field borrows: the menu runtime and the scene host are
        // separate fields, so the live scene world (not the disc-free
        // scaffold) is ticked in place - the shop spends the player's real
        // gold and stocks their real bag.
        let menu = &mut self.menu;
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        if let Some(cue) = menu.step_field_session(&mut host.world, edge) {
            self.play_sfx(u32::from(cue));
        }
        // The opening fade steps with the shop, so its quad is re-placed
        // into the frame's screen geometry here rather than on a field tick
        // the page does not run under a shop.
        self.rebuild_screen_geom();
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

        // The shop-family overlay (gold shop, prize exchange, coin counter,
        // the fallback panel with its frame, the banners) - the composition
        // the native window draws too, built once for both arrays.
        let screens = self.shop_overlay_frame(assets, surface_w, surface_h);
        let mut windows = screens.stage_texts;
        // The end-of-game soft reset (op-0x49 sub-op 0xC, slot 0x33): the
        // records screen at the engine's pen, as the native window draws it.
        if let Some(pen) = self
            .scene_host
            .host()
            .and_then(|h| h.world.soft_reset_records_pen())
        {
            windows.extend(self.records_draws_at(font, pen));
        }
        // The field floor window (op-0x49 sub-op 4, the Uru Mais warp pads), through
        // the engine layout + shared line composition the native window
        // draws (`SceneHost::flag_window_lines`).
        if let Some(host) = self.scene_host.host() {
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
        let banners = screens.banner_texts;
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
        if screens.sprites.is_empty()
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
        // The shop-family frames and atlas sprites (the fallback panel's gold
        // frame first, under its own rows).
        sprites.extend(screens.sprites);
        let mut texts: Vec<TextDraw> = windows;
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

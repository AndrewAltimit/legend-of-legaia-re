//! Opening the menu's sessions (field overlay, shop, inn, prize exchange) and the
//! open / suspend / cover state queries hosts read.

use super::*;

impl MenuRuntime {
    /// Take the UI cue the last [`Self::tick`] raised (a shop blip), if any.
    /// Both hosts key it through their SFX channel right after the tick -
    /// the shop's cues are the engine's decision, the voice is the host's.
    pub fn take_ui_cue(&mut self) -> Option<u8> {
        self.ui_cue.take()
    }

    /// Open whichever menu-overlay screen the field VM requested this tick -
    /// an op-`0x49` sub-0 gold shop (`World::take_pending_field_shop`) or the
    /// sub-7 casino prize counter (`World::take_pending_prize_exchange`) -
    /// and answer whether one opened.
    ///
    /// The tick that opens a screen steps it on **no** edge
    /// ([`Self::session_edge`]): the press that tick carried already went to
    /// the field - typically the Cross that closed the merchant's line and
    /// ran the script into op `0x49` - and handing it to the screen too
    /// commits the picker's first row on the same press. Retail's screen
    /// comes up on a later frame, once the menu overlay has swapped in. Both
    /// play hosts drain the requests through this and step the opening tick
    /// through [`Self::session_edge`]; each used to spell the drain itself.
    pub fn open_field_overlay_requests(&mut self, world: &mut World) -> bool {
        let mut opened = false;
        if let Some(shop) = world.take_pending_field_shop() {
            self.open_shop_menu(shop);
            opened = true;
        }
        if let Some(exchange) = world.take_pending_prize_exchange() {
            self.open_prize_exchange(exchange);
            opened = true;
        }
        opened
    }

    /// The field-overlay poll both play hosts run once per sim tick: open
    /// whichever screen the field VM requested
    /// ([`Self::open_field_overlay_requests`]) and take that screen's first
    /// step, on no edge ([`Self::session_edge`]).
    ///
    /// `None` when nothing opened. `Some(cue)` on the tick a screen opened,
    /// carrying the blip its first step raised for the host to key.
    ///
    /// The first step belongs to the opening tick rather than to the next
    /// one: the shop's field fade and window slides are tick-counted from it,
    /// so a host that deferred it ran them a tick behind the other. A host
    /// that also steps open sessions every tick ([`Self::step_field_session`])
    /// skips that step on a tick this returned `Some`.
    pub fn poll_field_overlay(&mut self, world: &mut World) -> Option<Option<u8>> {
        if !self.open_field_overlay_requests(world) {
            return None;
        }
        Some(self.step_field_session(world, Self::session_edge(true, 0)))
    }

    /// The pad edge a session step takes this tick: none on the tick a
    /// screen opened ([`Self::open_field_overlay_requests`]), else the
    /// tick's own.
    pub const fn session_edge(opened_this_tick: bool, pressed_edge: u16) -> u16 {
        if opened_this_tick { 0 } else { pressed_edge }
    }

    /// One sim tick of an open field session (shop, prize exchange, inn
    /// prompt) on that tick's pad **edges**, then the unpark of whatever
    /// field script a closed session left suspended - the whole per-tick
    /// step both play hosts run, so neither can tick the screen on a
    /// different clock or release the merchant op on a different test.
    ///
    /// Returns the screen's blip for the host to key.
    ///
    /// Call once per sim tick while [`Self::is_open`] holds, and on the tick
    /// a session closes: the shop's fade ([`Self::shop_fade_level`]) and
    /// window slides are tick-counted, so a host that stepped this once per
    /// display frame ran them at its monitor's rate rather than retail's.
    /// The edge goes to the first tick of a frame only; catch-up ticks pass
    /// `0`.
    pub fn step_field_session(&mut self, world: &mut World, edge: u16) -> Option<u8> {
        let mut cue = None;
        if self.is_open() {
            self.tick(world, menu_input_from_pad_edges(edge));
            cue = self.take_ui_cue();
        }
        // A field-VM-armed shop the player has now closed: the suspended
        // op-`0x49` resumes (Armed -> Done) and the field VM advances past
        // the merchant op next tick. Keyed on the whole runtime being
        // closed, not on the shop session alone: the buy list's sub-screens
        // (quantity, recipient picker, Point Card toast) outlive a cleared
        // `shop_session` for a beat, and the merchant must not resume under
        // them.
        if world.shops.shop_open && !self.is_open() {
            world.finish_field_shop();
        }
        // Safety net for the prize exchange (its own Exit already calls
        // `finish_prize_exchange` through the tick): if the screen closed by
        // any other path, unpark the counter script rather than wedge it.
        if world.shops.prize_exchange_open && !self.is_open() {
            world.finish_prize_exchange();
        }
        cue
    }

    /// Install the disc-pinned equip restrictions the retail buy dispatch
    /// reads ([`legaia_engine_menus::equipment::DiscEquipInfo`], built from the static
    /// `SCUS_942.54` equipment stat-bonus table).
    pub fn install_equip_info(&mut self, info: DiscEquipInfo) {
        self.equip_info = Some(info);
    }

    /// Install a shop session and prepare for `ShopBuy` entry. Engines call
    /// this when the field VM triggers a shop transition.
    pub fn open_shop(&mut self, session: ShopSession) {
        self.shop_session = Some(session);
        self.shop_slides.reset();
        // A direct open (tooling / tests) skips the field fade; the merchant
        // path (`open_shop_menu`) runs it.
        self.shop_age = SHOP_FADE_FRAMES;
    }

    /// Open a shop into its **top-level Buy / Sell / Trade picker**
    /// ([`MenuState::ShopMenu`]) - the field-VM op-`0x49` merchant trigger path.
    /// The Trade row appears only when the disc enabled seru trading; selecting
    /// it opens this vendor's [`legaia_engine_battle::seru_trade::SeruTradeSession`].
    pub fn open_shop_menu(&mut self, session: ShopSession) {
        self.shop_session = Some(session);
        self.shop_slides.reset();
        self.shop_age = 0;
        self.trade_session = None;
        self.recipient_session = None;
        self.ctx.state = MenuState::ShopMenu.as_byte();
        self.ctx.cursor = 0;
    }

    /// Open a shop directly into its **buy list** - the field-VM op-`0x49`
    /// merchant trigger path (distinct from the pause-menu [`Self::open`]).
    /// Installs the session and enters `ShopBuy` at the top of the list, so a
    /// host that drained [`legaia_engine_core::world::World::take_pending_field_shop`] can
    /// hand the player straight into the store.
    pub fn open_shop_buy(&mut self, session: ShopSession) {
        self.shop_session = Some(session);
        self.shop_slides.reset();
        // A direct open (tooling / tests) skips the field fade; the merchant
        // path (`open_shop_menu`) runs it.
        self.shop_age = SHOP_FADE_FRAMES;
        self.ctx.state = MenuState::ShopBuy.as_byte();
        self.ctx.cursor = 0;
    }

    /// Install an inn session and prepare for `InnConfirm` entry. `cost` is
    /// the gold required for a rest at this inn - in production the scene's
    /// scripted gold-gate literal (see [`Self::open_scene_inn`], which
    /// resolves it from the loaded scene); passing a constant directly is
    /// the test / tooling path.
    pub fn open_inn(&mut self, cost: u32) {
        self.inn_session = Some(InnSession::new(cost));
    }

    /// Open the inn prompt with the **current scene's scripted cost** - the
    /// op-`0x4E` gold-gate literal scanned from the scene MAN at load
    /// ([`legaia_engine_scene_host::scene_host::SceneHost::scene_inn_cost`]). Installs the session
    /// and enters `InnConfirm` at the Yes slot, mirroring the
    /// [`Self::open_shop_buy`] field-trigger entry shape. Returns the
    /// resolved cost, or `None` (no session installed, state untouched)
    /// when the scene charges nothing - free rests (Rim Elm's bed, Biron)
    /// have no gate + debit pair in their scripts.
    pub fn open_scene_inn(
        &mut self,
        host: &legaia_engine_scene_host::scene_host::SceneHost,
    ) -> Option<u32> {
        let cost = host.scene_inn_cost()?;
        self.inn_session = Some(InnSession::new(cost));
        self.ctx.state = MenuState::InnConfirm.as_byte();
        self.ctx.cursor = 0;
        Some(cost)
    }

    /// Open the menu (entry-point - typically called when the field VM
    /// requests menu via op `0x4C` sub-1).
    pub fn open(&mut self) {
        open(&mut self.ctx);
    }

    /// `true` while the menu is active (ctx state != Closed), or while a
    /// prize-exchange session owns the pad (it runs outside the
    /// [`MenuState`] graph, so `ctx.state` stays `Closed` under it).
    pub fn is_open(&self) -> bool {
        self.ctx.state != MenuState::Closed.as_byte() || self.prize_session.is_some()
    }

    /// `true` while the open screen is one retail runs as a **menu-overlay
    /// session**, under which the field does not advance at all.
    ///
    /// A gold shop and the casino prize exchange are both menu-overlay
    /// sessions: their UI and tables live in the menu overlay (PROT 0899),
    /// which shares slot A with the field overlay (PROT 0897) and replaces it
    /// while it runs. A save state taken inside the ticket-counter prize shop
    /// holds game mode `0x17` with 0899's clean prefix resident and the field
    /// overlay swapped out (`scripts/scenarios.toml` `casino_prize_shop`), so
    /// no field code - locomotion, NPC motion, the field VM - can run under
    /// the screen. The inn is not one of these: retail's inn is an ordinary
    /// field-VM dialogue ([`legaia_engine_menus::inn`]), so the field keeps its tick under
    /// the prompt.
    ///
    /// Hosts skip the world tick while this holds. The browser page always
    /// froze the field under a shop; the native window used to tick it with a
    /// neutral pad, so NPCs kept walking and scene programs kept running
    /// behind the buy list.
    pub fn suspends_field(&self) -> bool {
        self.prize_session.is_some() || (self.is_open() && self.inn_session.is_none())
    }

    /// `true` while the screen behind the menu is **black** rather than the
    /// field: the same menu-overlay sessions as [`Self::suspends_field`].
    /// With the field overlay swapped out nothing draws the scene, so retail
    /// fades the field to black before the shop's windows slide in (Retock
    /// Items Shop capture: the scene darkens over the first frames of the
    /// open, then every shop screen sits on black). Hosts skip the 3D pass
    /// and clear black while this holds.
    pub fn covers_field(&self) -> bool {
        self.suspends_field() && self.shop_fade_level().is_none()
    }

    /// While a shop or the prize counter is opening, how far the field has
    /// faded toward black: `Some(level)` with `level` `0..=255` for the
    /// first [`SHOP_FADE_FRAMES`] ticks, `None` once the fade is done (the
    /// screen is then black - [`Self::covers_field`]) or with no shop up.
    ///
    /// Retail (Retock Items Shop, stepped per vsync) darkens the field
    /// linearly from the last dialogue frame to black over fourteen frames
    /// before the shop's windows appear. Hosts draw it as a subtractive
    /// full-screen quad over the frozen field
    /// (`screen_prim::fade_prim(level * 0x010101, 2, 0)`).
    pub fn shop_fade_level(&self) -> Option<u8> {
        if self.shop_session.is_none() && self.prize_session.is_none() {
            return None;
        }
        (self.shop_age < SHOP_FADE_FRAMES)
            .then(|| (255 * (self.shop_age + 1) / SHOP_FADE_FRAMES).min(255) as u8)
    }

    /// Open the casino prize-exchange screen (field-VM op-`0x49` sub-op 7) -
    /// the counterpart of [`Self::open_shop_menu`] for the session drained
    /// from `World::take_pending_prize_exchange`.
    pub fn open_prize_exchange(
        &mut self,
        session: legaia_engine_minigames::prize_exchange::PrizeExchangeSession,
    ) {
        self.prize_session = Some(session);
        self.shop_age = 0;
    }

    /// Raw state byte of the underlying [`MenuCtx`].
    pub fn ctx_state(&self) -> u8 {
        self.ctx.state
    }

    /// Cursor position within the current screen.
    pub fn cursor(&self) -> u8 {
        self.ctx.cursor
    }

    /// The seru-trade offer currently being confirmed at `ShopTradeConfirm`
    /// (the one picked in `ShopTrade`), for the host to label the prompt.
    pub fn pending_trade_offer(&self) -> Option<legaia_game_tables::seru_trade::OwnerTrade> {
        self.trade_session
            .as_ref()
            .and_then(|t| t.offers.get(self.trade_pending_offer).copied())
    }
}

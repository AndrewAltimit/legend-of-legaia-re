//! The in-field pause menu on the session: open / press / close, the pause
//! wipe, the save rack, and the per-tick menu and sub-screen steps.

use super::*;

impl<S: AudioSink> BootSession<S> {
    /// Open the in-field pause menu (the retail Start-press path into the
    /// CARD mode pair, `game_mode 0x17`). Builds a [`FieldMenuSession`]
    /// seeded with the world's money + play time - the same construction the
    /// windowed host uses - then remembers the current
    /// [`SceneMode`] and switches the world into [`SceneMode::Menu`], so
    /// field dispatch suspends while the menu owns the frame. Idempotent
    /// while a menu is already open.
    ///
    /// This is the **builder**, and it enforces only the refusal retail puts
    /// inside the controller itself (the engaged bit). A host's *Start edge*
    /// must additionally consult
    /// [`World::field_menu_open_allowed`](legaia_engine_core::world::World::field_menu_open_allowed),
    /// which adds the mode test, instead of spelling one out locally. That
    /// separation is deliberate: headless drivers and oracles legitimately
    /// build the session from any mode, but a pad route that hard-codes
    /// `SceneMode::Field` silently drops the three kingdom overworlds, and
    /// those are the only scenes where the Save row is legal at all.
    pub fn open_field_menu(&mut self) {
        if self.field_menu.is_some() {
            return;
        }
        // Start is inert while a dialogue engagement owns the player. Retail's
        // menu-open accept lives in `FUN_801D01B0`'s pre-movement header,
        // *after* the engaged-bit branch at `0x801D01F0` - so with
        // `player+0x10 & 0x80000` raised the pad never reaches it and no menu
        // (and no deny buzz) happens at all.
        // REF: FUN_801D01B0
        // A script's own menu press (a save point's `49 01`) comes from the
        // interaction the engagement belongs to, so it is not refused.
        if self.host.world.dialogue_owns_input() && !self.host.world.scripted_menu_open_pending() {
            return;
        }
        let world = &mut self.host.world;
        // Menu-open state 0 (`FUN_801DC6B4`, `0x801DC7EC..0x801DC800`): a
        // non-zero casino coin bank raises the system flag the money box's
        // coin row tests. The flag is never cleared.
        if world.minigames.casino_coins != 0 {
            world.system_flag_set(legaia_engine_menus::field_menu::SYSTEM_FLAG_CASINO_COIN_ROW);
        }
        let mut session = FieldMenuSession::new();
        session.money = world.party.money.max(0) as u32;
        session.play_time_seconds = world.clock.play_time_seconds;
        // Sample the two row gates retail keeps as globals and reads at every
        // draw: the op-`0x49` entry context (`*_DAT_8007B450`, which blocks
        // Load) and the scene's save permission (`_DAT_8007B6A8`, seeded at
        // scene load from the MAN header bit, which blocks Save). Both are
        // scene-scoped and the menu suspends the field, so sampling once at
        // open is equivalent to retail's per-frame re-read.
        session.set_gate(FieldMenuGate {
            entry_context_kind: world.menu_entry_context_kind(),
            save_allowed: world.party.scene_save_allowed,
        });
        // Retail's driver picks the *starting* sub-screen off that same kind
        // byte, so a locked context opens on the notice panel rather than on
        // the root picker (`FUN_801DC6B4`, `0x801dc8d0..0x801dc8e4`).
        session.open_entry_screen();
        self.field_menu_resume = world.mode;
        world.mode = SceneMode::Menu;
        self.field_menu = Some(session);
        // Retail opens the menu by writing the mode word, not by calling the
        // menu: `CARD INIT` (22) stages the menu overlay and hands the word to
        // `CARD MODE` (23) at `0x80025974`, which is the mode every menu-open
        // capture holds. Going through the seat is what runs the mode-change
        // edge - in particular the pad-edge swallow, so the Start press that
        // opened the menu is not also delivered as the menu's first input.
        self.mode_seat.request_card_mode();
        let plan = self
            .mode_seat
            .enter(legaia_engine_mode::mode::GameMode::CardInit, world);
        debug_assert!(
            plan.is_none(),
            "CARD INIT stages no overlay-A request in the port's model"
        );
    }

    /// One menu-button press, as every host answers it: the pause menu's whole
    /// open rule in one place, so the native window, the browser page and a
    /// headless [`Self::tick`] cannot answer the same press differently.
    ///
    /// - A script's own menu press (an op-`0x49` save point / ready check,
    ///   [`World::scripted_menu_open_pending`](legaia_engine_core::world::World::scripted_menu_open_pending))
    ///   opens the menu with no Start edge and past the engagement gate.
    /// - A Start edge opens it when
    ///   [`World::field_menu_open_allowed`](legaia_engine_core::world::World::field_menu_open_allowed)
    ///   says so - the scene mode, the system / menu locks, the warp hold, the
    ///   opening chain, a narration crawl or title card, a shop, and a dialogue
    ///   engagement (retail's engaged-bit branch at `0x801D01F0` sits ahead of
    ///   the accept, so Start mid-dialogue opens nothing and buzzes nothing).
    /// - A Start edge the menu lock refuses queues the deny buzz (`0x23`);
    ///   every other refusal is silent.
    ///
    /// No-op ([`PauseMenuPress::None`]) while a menu is already open: what
    /// Start does inside the menu is the picker's own rule.
    ///
    /// REF: FUN_801D01B0 (`0x801D0250`, the menu-open accept)
    pub fn press_field_menu(&mut self, start_edge: bool) -> PauseMenuPress {
        if self.field_menu.is_some() {
            return PauseMenuPress::None;
        }
        let scripted = self.host.world.scripted_menu_open_pending();
        if scripted || (start_edge && self.host.world.field_menu_open_allowed()) {
            self.open_field_menu();
            if self.field_menu.is_none() {
                return PauseMenuPress::None;
            }
            if scripted {
                self.host.world.note_scripted_menu_opened();
            }
            // The field darkens before the menu exists (`FUN_801ED308`
            // phase 1): hosts draw the field under the wipe and route no
            // input to the menu until `pause_wipe().menu_spawned()`.
            self.pause_wipe.open();
            return PauseMenuPress::Opened { scripted };
        }
        if start_edge && self.host.world.field_menu_press_denied() {
            return PauseMenuPress::Denied;
        }
        PauseMenuPress::None
    }

    /// Close the pause menu and restore the suspended scene mode (the mode
    /// the world ran when [`Self::open_field_menu`] fired). No-op when no
    /// menu is open.
    pub fn close_field_menu(&mut self) {
        if self.field_menu.take().is_some() {
            // The field brightens back under the lifting wipe (phase 4).
            self.pause_wipe.close();
            // The sub-session goes with it: a menu closed out from under an
            // open screen must not leave that screen holding the pad the next
            // time the menu opens.
            self.field_menu_sub = None;
            self.save_flow.reset();
            self.host.world.mode = self.field_menu_resume;
            // The word follows the world back out of `CARD MODE`; the seat
            // takes the edge (and the pad swallow) on the next frame, so the
            // confirm that closed the menu does not walk the player.
            self.mode_seat.adopt_scene_mode(self.field_menu_resume);
            // A scripted menu press (a save point's `49 01`, a `49 0D` ready
            // check) parks its op until the menu it opened closes; the close
            // resumes it once. Twin of `play_menu_close` on the browser host.
            self.host.world.release_menu_entry_context_park();
        }
    }

    /// The pause menu's field wipe: whether the menu has spawned yet and the
    /// fade quad to draw over the field while it ramps.
    pub fn pause_wipe(&self) -> &legaia_engine_system::pause_wipe::PauseWipe {
        &self.pause_wipe
    }

    /// Advance the pause wipe one frame. [`Self::tick`] does this itself; a
    /// host that runs the menu frames without ticking the session (both play
    /// hosts) calls it on those frames until the menu has spawned.
    pub fn step_pause_wipe(&mut self) {
        let step = self.host.world.clock.frame_step;
        self.pause_wipe.tick(step);
    }

    /// Whether the in-field pause menu is open (the engine equivalent of
    /// retail `game_mode 0x17`; [`World::mode`](legaia_engine_core::world::World::mode)
    /// is [`SceneMode::Menu`] while `true`).
    pub fn field_menu_is_open(&self) -> bool {
        self.field_menu.is_some()
    }

    /// Install the rack the Load / Save rows build their sub-session against,
    /// plus the block list behind each of its ports.
    ///
    /// The rack's kind decides the flow ([`SaveRack::CardPorts`] = retail's
    /// two-stage pill-row -> block-grid screen), which is why it arrives as
    /// one value instead of a rack and a mode flag: a host cannot make that
    /// call differently from another host. `port_blocks[p]` answers the
    /// card-read beat for port `p`; ports past its end read as unmounted.
    pub fn set_save_rack(&mut self, rack: SaveRack, port_blocks: Vec<Vec<SlotSnapshot>>) {
        self.save_rack = rack;
        self.save_port_blocks = port_blocks;
        self.save_flow.reset();
    }

    /// The rack currently behind the Load / Save rows.
    pub fn save_rack(&self) -> &SaveRack {
        &self.save_rack
    }

    /// The two-stage card flow driving an open Save / Load sub-session -
    /// the block grid, its cursor, and which port was read.
    pub fn save_flow(&self) -> &SaveScreenFlow {
        &self.save_flow
    }

    /// Drive one frame of the open pause menu, both levels. Returns `true`
    /// when the root session reached an outcome and the caller should close.
    ///
    /// Retail's pause menu suspends the root list on confirm and hands the
    /// pad to the routed sub-screen; this is that second level. Before it
    /// existed here, a confirmed row was resumed on the spot, so a driver
    /// built on `set_pad` + [`Self::tick`] could move the root cursor and
    /// gate rows but could not enter Items / Equip / Save at all - while both
    /// shipped hosts implemented the stack privately. Keeping it on the
    /// session is what lets an oracle and a host walk the same screens.
    pub(super) fn tick_field_menu(&mut self) -> bool {
        // No menu exists until the wipe spawns it; the pad goes nowhere.
        if !self.pause_wipe.menu_spawned() {
            return false;
        }
        let pad = &self.host.world.input;
        // The edge word every menu surface in this subsystem reads. A held
        // mask is one event: `just_pressed` is `pad & !pad_prev`.
        let pressed = pad.pad() & !pad.pad_prev();

        if self.field_menu_sub.is_some() {
            self.tick_field_menu_sub(pressed);
        } else {
            // Root list. A confirm suspends it on the routed row; build that
            // row's sub-session and control moves there next frame.
            let menu = self.field_menu.as_mut().expect("field_menu is Some");
            if let Some(row) = tick_root_list(menu, pressed) {
                self.save_flow.reset();
                let world = &self.host.world;
                let chain_library = world.chain_library();
                self.field_menu_sub = Some(FieldMenuSubsession::build(
                    row,
                    world,
                    &self.options_state,
                    &self.save_rack,
                    &chain_library,
                    &world.tables.spell_catalog,
                    &world.tables.equipment_table,
                ));
            }
        }

        self.field_menu.as_ref().and_then(|m| m.outcome()).is_some()
    }

    /// Route one pad edge into the open sub-session and, when it finishes,
    /// drain its outcome into the world before resuming the root list -
    /// through the same engine steps both shipped hosts call
    /// ([`tick_open_subsession`], [`finish_subsession`]).
    fn tick_field_menu_sub(&mut self, pressed: u16) {
        let Some(active) = self.field_menu_sub.as_mut() else {
            return;
        };
        // The Save / Load rows run under the two-stage card flow: it
        // pre-empts the grid edges and resolves the card-read beat.
        let mut edge = pressed;
        if let FieldMenuSubsession::Save(s) = active {
            if let Some(port) = self.save_flow.pending_read(s) {
                let blocks = self
                    .save_port_blocks
                    .get(port as usize)
                    .cloned()
                    .unwrap_or_default();
                self.save_flow.install_blocks(port, blocks);
            }
            edge = self.save_flow.before_tick(s, pressed);
            // A card Save's write: `BootSession` has no save backend, so the
            // write is the latch below and always goes through.
            if self.save_flow.save_request(s).is_some() {
                self.save_flow.finish_save_request(s, true);
            }
        }
        // No key table here: a headless driver has no bindings to rebind.
        let _ = tick_open_subsession(active, edge, None, &self.host.world);
        if !self
            .field_menu_sub
            .as_ref()
            .is_some_and(FieldMenuSubsession::is_done)
        {
            return;
        }
        let finished = self.field_menu_sub.take().expect("sub was Some");
        let done = finish_subsession(finished, &mut self.host.world);
        let close_menu = done.close_menu;
        // A renderer-less driver just latches the notices.
        if done.spell_level_notice.is_some() {
            self.spell_level_notice = done.spell_level_notice;
        }
        if done.art_learned_notice.is_some() {
            self.art_learned_notice = done.art_learned_notice;
        }
        match done.handoff {
            SubsessionHandoff::Applied => {}
            // The outcome names the card port, the grid names the block.
            // Persisting it is the host's: `BootSession` has no save
            // backend, so the pick is latched for the caller.
            SubsessionHandoff::Save(s) => {
                self.last_save_commit = self
                    .save_flow
                    .commit(&s)
                    .or_else(|| s.outcome().and(self.save_flow.written_save()))
            }
            SubsessionHandoff::Options(state) => self.options_state = state,
        }
        if let Some(menu) = self.field_menu.as_mut() {
            let _ = menu.resume(close_menu);
        }
    }
}

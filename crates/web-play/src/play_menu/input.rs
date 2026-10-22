//! `play_menu_input`: one frame of pad input routed through the root list
//! and the open sub-session.

use super::*;

impl LegaiaRuntime {
    /// Whether the pause menu owns the frame: the wipe has spawned it (or a
    /// title row opened it with no wipe), so every sim tick is one menu step
    /// ([`Self::play_menu_input`]) - the native window's boot-UI arm.
    pub(crate) fn play_menu_owns_frame(&self) -> bool {
        self.play_menu.as_ref().is_some_and(|m| m.owns_frame)
    }

    /// The tick the wipe spawned the menu (`TickOwner::PauseMenuSpawned`):
    /// the menu owns the frame from the next tick, as the native arm that
    /// raises its boot-UI state does.
    pub(crate) fn play_menu_take_spawn(&mut self) {
        if let Some(m) = self.play_menu.as_mut() {
            m.owns_frame = true;
        }
    }

    /// A press opened the session's field menu (`TickOwner::PauseMenuOpened`):
    /// stand up the page's menu over it, and blip the open as a confirm - as
    /// on the native window; a scripted press blips nothing, retail's cue
    /// `0x20` belonging to the pad controller, not to the actor it spawns.
    pub(crate) fn play_menu_attach_opened(&mut self, scripted: bool) {
        if self.play_menu.is_none() {
            self.play_menu = Some(PlayMenu::new());
        }
        if !scripted {
            let _ = self.play_sfx_event("menu_confirm");
        }
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Drive the menu one frame from an edge-triggered PSX pad word (same bit
    /// layout as [`Self::set_pad`]). Navigation:
    /// - top-level: Up/Down move the cursor, Cross opens the row, Circle closes.
    /// - a sub-screen: routes the edges to its session; Circle (or the session
    ///   finishing) drops back to the top-level list.
    ///
    /// It also fires the menu's blip for the edge the refusal box and the save
    /// grid let through (the native window's order), so the page no longer
    /// blips the raw press.
    pub fn play_menu_input(&mut self, edge: u16) {
        if self.play_menu.is_none() {
            return;
        }
        // The menu spawns partway through the field's wipe to black
        // (`BootSession::pause_wipe`, retail `FUN_801ED308` phase 1): until
        // then each tick only advances the wipe, the pad goes nowhere, and
        // the page keeps drawing the field under the wipe's quad.
        if !self.pause_menu_spawned() {
            if let Some(session) = self.scene_host.session_mut() {
                session.step_pause_wipe();
            }
            self.rebuild_screen_geom();
            return;
        }
        // A save-refusal notice owns the pad while it is up - the same
        // pre-empt the native window makes, so the edge that dismisses the
        // box does not also drive the menu behind it.
        if let Some(m) = self.play_menu.as_mut()
            && m.save_flow.tick_refusal(edge)
        {
            return;
        }
        // The menu's blip, off this edge once the refusal box has had it -
        // the native window fires its cues after the same pre-empts
        // (`tick_boot_ui`). The save screen's grid filters the edge further
        // (`before_tick`, below), so its blip waits for the filtered edge:
        // a blocked Load on an empty block is silent on both hosts.
        let (start_closes_menu, save_sub) = self
            .play_menu
            .as_ref()
            .map(|m| {
                let save = matches!(
                    m.sub.as_ref(),
                    Some(PlaySub::Session(s)) if matches!(s.as_ref(), FieldMenuSubsession::Save(_))
                );
                (m.sub.is_none(), save)
            })
            .unwrap_or((true, false));
        if edge != 0 && !save_sub {
            let _ = self.play_menu_edge_blip(edge, start_closes_menu);
        }
        // Window 7 (spell level-up notice) owns the pad while armed: retail's
        // cast sub-screens stall on the confirm | cancel masks after the
        // widget-VM `[open window 7]` script (`0x801E4D50` / `0x801E4D78`),
        // and nothing else on the pad moves. Same pre-empt as the native
        // window's field-menu arm.
        if self.menu.dismiss_spell_level_notice(
            pressed(edge, PadButton::Cross),
            pressed(edge, PadButton::Circle),
            pressed(edge, PadButton::Triangle),
        ) {
            return;
        }
        // Window 8 (art learned): the Items use sub-screen's same stall.
        if self.menu.dismiss_art_learned_notice(
            pressed(edge, PadButton::Cross),
            pressed(edge, PadButton::Circle),
            pressed(edge, PadButton::Triangle),
        ) {
            return;
        }
        // Sub-screen active: route to its session, then check for exit.
        let has_sub = self
            .play_menu
            .as_ref()
            .map(|m| m.sub.is_some())
            .unwrap_or(false);
        if has_sub {
            // Answer the flow's card read off `&self` before the `&mut`
            // borrow below: reading a port lifts fifteen SC blocks, which is
            // the one thing the kernel cannot do for itself.
            self.service_card_read();
            self.service_card_save();

            let mut session_done = false;
            let mut edge = edge;
            let mut save_edge = None;
            // Consumed once per tick whether or not a rebind screen is open:
            // a key latched while none is would be stale by the next frame
            // and must not survive to be bound later. Same rule as the native
            // window's `pending_key_name`.
            let key = self.play_menu.as_mut().and_then(|m| m.pending_key.take());
            let mut rebound = None;
            if let Some(m) = self.play_menu.as_mut()
                && let Some(PlaySub::Session(session)) = m.sub.as_mut()
            {
                // Step the grid cursor and gate an empty-block Load BEFORE
                // the session ticks, so a confirm on the same edge commits
                // the cell the player is looking at.
                if let FieldMenuSubsession::Save(s) = session.as_mut() {
                    edge = m.save_flow.before_tick(s, edge);
                    save_edge = Some(edge);
                }
                // The shared step (with the Status screen's Triangle -> Arts
                // editor extension); it hands back a committed rebind. A
                // title-opened Load has no scene yet, and no Status screen
                // either, so it steps the session alone.
                rebound = match self.scene_host.host() {
                    Some(host) => field_menu_dispatch::tick_open_subsession(
                        session.as_mut(),
                        edge,
                        key.as_deref(),
                        &host.world,
                    ),
                    None => {
                        session.tick_pad_edge_with_key(edge, key.as_deref());
                        session.take_rebound_mapping()
                    }
                };
                session_done = session.is_done();
            }
            // A bind committed inside the Options sub-session's Key Config
            // screen. Adopting it here is what makes the very next keydown
            // resolve through the new table, and `store_mapping` persists it
            // to `localStorage` - the browser twin of the native window
            // rewriting `legaia-input.toml`.
            if let Some(mapping) = rebound.as_ref() {
                crate::pad_bindings::store_mapping(mapping);
            }
            // The save screen's blip, off the edge its grid let through.
            if let Some(e) = save_edge.filter(|&e| e != 0) {
                let _ = self.play_menu_edge_blip(e, false);
            }
            if session_done {
                // Fold the finished session's result into the live world
                // (equip swap / item use / spell cast / card load-save)
                // exactly as the native shell does, then drop back to the
                // top-level list on the row that opened it.
                let flow = self
                    .play_menu
                    .as_ref()
                    .map(|m| m.save_flow.clone())
                    .unwrap_or_default();
                let sub = self.play_menu.as_mut().and_then(|m| m.sub.take());
                if let Some(PlaySub::Session(session)) = sub {
                    // Fold the result into the live world through the one
                    // router every host shares (equip swap / item use /
                    // spell cast / arts / reorder); only the card rack and
                    // the options store are this host's.
                    let (handoff, close_menu) = match self.scene_host.host_mut() {
                        Some(host) => {
                            let mut done =
                                field_menu_dispatch::finish_subsession(*session, &mut host.world);
                            // Windows 7 / 8: the shared runtime holds the beat
                            // and the pre-empts at the top of this method hold
                            // the pad.
                            self.menu.arm_finished_notices(&mut done);
                            (done.handoff, done.close_menu)
                        }
                        None => (
                            field_menu_dispatch::SubsessionHandoff::without_world(*session),
                            false,
                        ),
                    };
                    match handoff {
                        field_menu_dispatch::SubsessionHandoff::Applied => {}
                        // Load / Save reach the card rack, which needs the
                        // whole runtime.
                        field_menu_dispatch::SubsessionHandoff::Save(s) => {
                            self.apply_card_outcome(&flow, &s)
                        }
                        field_menu_dispatch::SubsessionHandoff::Options(state) => {
                            self.options_state = state;
                            self.persist_and_apply_options();
                        }
                    }
                    // Hand control back to the shared picker, which parks the
                    // cursor on the row that opened the sub-screen - the same
                    // `menu.resume(false)` the native shell calls.
                    // A title-opened menu has no root screen to return to:
                    // `resume(true)` finishes the session and the menu
                    // closes on this same tick.
                    // A Door use's exit code closes the whole menu
                    // (`FinishedSubsession::close_menu`), as it does natively.
                    let close = if let Some(m) = self.play_menu.as_mut() {
                        let from_title = m.from_title;
                        let close = (from_title && m.pending_load_scene.is_none()) || close_menu;
                        if let Some(root) = self
                            .scene_host
                            .session_mut()
                            .and_then(|s| s.field_menu.as_mut())
                        {
                            let _ = root.resume(from_title || close_menu);
                        }
                        // A parked Load label must outlive this tick: the
                        // page collects it and closes the menu itself.
                        close
                    } else {
                        false
                    };
                    if close {
                        self.play_menu_close();
                    }
                }
            }
            return;
        }

        // Top-level command list: the shared retail picker, not a local
        // cursor. `tick` inks and routes off the same `root_menu_confirm_route`
        // the row renderer draws from, so a row cannot draw white and then
        // open something the gate forbids.
        let suspended_row = if self.play_menu.is_some() {
            self.root_menu_mut()
                .and_then(|m| field_menu_dispatch::tick_root_list(m, edge))
        } else {
            None
        };
        if let Some(row) = suspended_row {
            // Load / Save browse the console's two memory-card ports, so the
            // rack is `CardPorts` - which is also what puts the session in
            // the matching two-stage flow; no host flips that flag by hand
            // (`SaveSelectSession::for_rack`); the rack is the session's
            // over the two ports (`card_port_rack`), the native window's
            // too. Every other
            // row builds the real retail sub-session from the disc catalogs
            // installed on the host world at `load_disc` (spell / equipment /
            // item), matching the native shell's `FieldMenuSubsession::build`.
            let rack = legaia_engine_session::title_continue::card_port_rack(self.card_ports());
            let sub = self.scene_host.host().map(|host| {
                let world = &host.world;
                let chain = world.chain_library();
                let mut built = FieldMenuSubsession::build(
                    row,
                    world,
                    &self.options_state,
                    &rack,
                    &chain,
                    &world.tables.spell_catalog,
                    &world.tables.equipment_table,
                );
                // The Options screen's engine-only Key Config screen (Select), armed off the
                // page's live (persisted) binding table - the browser twin of
                // the native window arming it off `legaia-input.toml`.
                built.arm_key_rebind(crate::pad_bindings::live_mapping());
                PlaySub::Session(Box::new(built))
            });
            if let Some(sub) = sub
                && let Some(m) = self.play_menu.as_mut()
            {
                m.sub = Some(sub);
                m.save_flow.reset();
            }
        }
        // Circle on the root list (or a sub-session that asked to close the
        // menu entirely) finishes the session; restore the suspended scene
        // mode and drop the menu, exactly as `close_field_menu` does.
        let outcome = self.root_menu().and_then(|m| m.outcome());
        if let Some(FieldMenuOutcome::Closed | FieldMenuOutcome::Confirmed(_)) = outcome {
            self.play_menu_close();
        }
    }
}

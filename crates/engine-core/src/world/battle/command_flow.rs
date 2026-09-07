//! Player-driven battle command menu and the Arts / Magic / Item submenu
//! drivers. Split out of `battle.rs` as additional `impl World` blocks; no
//! logic change from the original inline definitions.

use super::*;

/// The first **normal**-art action constant (art ordinal `4`). The queue
/// builder `FUN_801EED1C` tokenizes only ordinals `>= 4`: the Miracle Art
/// (ordinal `0`) and the three Hyper Arts (`1..=3`) take its other arm
/// (`sltiu a1,a0,0x4` at `0x801EF330`), which writes nothing unless the
/// slot's `+0x25F` Miracle marker is armed.
const NORMAL_ART_MIN_CONSTANT: u8 = 0x1F;

impl World {
    /// Open the player-driven command menu for party member `actor` and park
    /// the action SM. The action context's `active_actor` is set now; the
    /// queued action / target is filled in by [`Self::tick_battle_command`]
    /// once the player confirms. No-op unless [`Self::battle_player_driven`].
    pub(in crate::world) fn open_battle_command(&mut self, actor: u8) {
        use crate::battle_flow::BattleFlowState as Flow;
        use crate::battle_input::BattleCommandSession;
        if !self.battle_player_driven {
            return;
        }
        self.battle_ctx.active_actor = actor;
        // **The round prompt is the phase the session opens in, not one it is
        // swapped onto a frame later.** Retail's round-start arm writes the
        // prompt state before anything is on screen - `0x14` at `0x801D0EC4`
        // seats the selector on member 0 and stores `ctx[+0x06] = 0x1E` in the
        // delay slot at `0x801D0ED4`, and the four-arm ring `0x28` is only ever
        // reached *through* `0x1E` (the confirm arm at `0x801D108C`). So the
        // ring is never the first thing a round shows.
        //
        // A session built on the ring and rewritten by the next tick's
        // [`World::arm_round_open_prompt`] is one frame of the wrong surface:
        // a host that draws between ticks draws the ring, and any observer
        // that reads the session on the frame it opens - which is the frame
        // `battle_command` becomes `Some` - reads the ring and concludes the
        // prompt never happens. That is exactly how "no battle opens the round
        // prompt" was measured, and with it "a player cannot flee at all",
        // when the prompt was in fact one tick behind the look.
        //
        // Reopening mid-round (a submenu backed out of, or a tutorial rewind)
        // is *not* a round start: the flow byte is still parked on the window
        // state it bounced from, which is where retail's own cancel arm lands
        // (the item window `0x3C` stores `0x28` at `0x801D180C`), so those
        // open on the ring.
        //
        // REF: FUN_801D0748 (states 0x14 / 0x1E / 0x28)
        let round_open = matches!(self.battle_flow, Flow::Idle | Flow::TurnPrompt);
        self.battle_command = Some(if round_open {
            BattleCommandSession::new_round_open(actor, actor, self.battle_no_escape)
        } else {
            BattleCommandSession::new(actor, actor)
        });
        if self.battle_flow == Flow::Idle {
            self.set_battle_flow(Flow::TurnPrompt);
        }
    }

    /// Drive the open command session one frame from [`World::input`]. When the
    /// session resolves, arm the action SM with the chosen command + target
    /// (v0.1: a physical Attack) and clear the session so the SM resumes.
    /// On an abort (no valid target) it falls back to the first living monster
    /// so the loop never deadlocks.
    pub(in crate::world) fn tick_battle_command(&mut self) {
        use crate::battle_input::{BattleCommandInput, Resolution};
        use crate::battle_round::PendingPartyAction;
        use crate::input::PadButton;
        use crate::target_picker::CursorRow;

        let Some(mut session) = self.battle_command.take() else {
            return;
        };

        let party_count = self.party_count.clamp(1, 3);
        // Target-row selectability: the per-slot validity byte the retail
        // validator (`FUN_8003FB10` arm `0x05`) writes, not an inline liveness
        // test - see `super::validator_host`.
        let (party, monsters) = self.battle_target_rows();

        let ev = BattleCommandInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
            // The ring's Attack arm reads the option word with the pad.
            select_attack: self.battle_select_attack,
        };
        session.input(ev, party, monsters);
        // Target-cursor tint: retail stamps the four monster slots bright /
        // dimmed while the cursor is walking them and clears the tint the
        // moment it closes.
        self.apply_target_cursor_tint(&session);

        // Sparring tutorial: the hook for the state this resolution enters can
        // reject it (the wrong-lesson rewind), in which
        // case the action is discarded and the command menu reopens. Resolved
        // phases are gated here; unresolved ones (menu cursor / target cursor)
        // just mirror onto the retail command-flow byte `ctx[+0x06]`.
        let resolution = session.resolved();
        if resolution.is_none() {
            self.sync_battle_flow(Some(&session.phase));
        }
        if self.battle_tutorial.is_some()
            && let Some(res) = resolution
        {
            use crate::battle_flow::BattleFlowState as Flow;
            let rejected = match res {
                // Attack is the only command that reaches Confirmed with a
                // target in the engine; it commits category 3.
                Resolution::Confirmed { .. } => self.battle_tutorial_commit(3),
                Resolution::SpiritGuard => self.battle_tutorial_commit(4),
                Resolution::OpenItemMenu => self.set_battle_flow(Flow::ItemWindow),
                Resolution::OpenArtsMenu => self.set_battle_flow(Flow::ArtsCommandEntry),
                Resolution::OpenSpellMenu => self.set_battle_flow(Flow::MagicWindow),
                // Retail's state-50 handler rejects Run unconditionally for the
                // whole sparring fight.
                Resolution::RunAway => self.set_battle_flow(Flow::EscapePrompt),
                Resolution::Aborted => false,
            };
            if rejected {
                self.open_battle_command(session.actor);
                return;
            }
        }

        match session.resolved() {
            Some(Resolution::Confirmed {
                // Only Attack reaches Confirmed with a target: Arts / Magic /
                // Item hand off to their own submenus above.
                command: _,
                target_row,
                target_slot,
            }) => {
                let target = match target_row {
                    CursorRow::Enemy => party_count + target_slot,
                    CursorRow::Ally => target_slot,
                };
                let actor = session.actor;
                // Retail's target confirm (`0x5A`) writes the target byte
                // `+0x1DD` and the category, then walks the ring on to the
                // next member; the swing stream is seeded when the action SM
                // dispatches the member (`FUN_801EED1C` from state `0x0C`),
                // which is where `dispatch_pending_party_action` seeds it.
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3; // Attack
                }
                self.commit_party_command(actor, PendingPartyAction::Attack { target });
            }
            Some(Resolution::OpenArtsMenu) => {
                // Player picked Arts: open the retail-model per-press command
                // input (`FUN_801D0748` state 0x50). The legacy saved-chain
                // list stays reachable behind `LEGAIA_ARTS_SAVED_LIST=1`
                // (`var_os` is a clean `None` on wasm, so the browser always
                // takes the retail path).
                self.battle_ctx.active_actor = session.actor;
                if std::env::var_os("LEGAIA_ARTS_SAVED_LIST").is_some() {
                    let rows = self.build_battle_arts_rows(session.actor);
                    self.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(
                        session.actor,
                        session.actor,
                        rows,
                    ));
                } else {
                    self.open_arts_command_input(session.actor);
                }
            }
            Some(Resolution::OpenSpellMenu) => {
                // Player picked Magic: hand off to the spell submenu (same
                // pattern as Item). `tick_battle_spell_menu` drives until the
                // player casts (turn cycles via EndOfAction) or backs out.
                self.battle_ctx.active_actor = session.actor;
                match self.build_battle_spell_session(session.actor) {
                    Some(menu) => self.battle_spell_menu = Some(menu),
                    // No caster record / no catalog - don't strand the SM;
                    // reopen the command menu so the player can pick again.
                    None => self.open_battle_command(session.actor),
                }
            }
            Some(Resolution::OpenItemMenu) => {
                // Player picked Item: hand off to the inventory submenu. The
                // command session is dropped (already taken) and the action SM
                // stays parked; `tick_battle_item_menu` drives until the player
                // uses an item (turn cycles via EndOfAction) or backs out
                // (the command menu reopens for the same actor).
                self.battle_ctx.active_actor = session.actor;
                self.battle_item_menu = Some(self.build_battle_item_session());
            }
            Some(Resolution::SpiritGuard) => {
                // Player picked Spirit: the guard stance (retail's pending
                // category `+0x1DE = 4`, the melee kernel's tripled guard
                // roll) is up from the commit - it protects against every
                // monster that dispatches ahead of this member - and lasts
                // until the next round's sweep clears the category. The AP
                // charge is the Spirit band's own, at dispatch.
                let actor = session.actor;
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.action_category = 4;
                }
                if let Some(guard) = self.battle_guarding.get_mut(actor as usize) {
                    *guard = true;
                }
                self.commit_party_command(actor, PendingPartyAction::Spirit);
            }
            Some(Resolution::RunAway) => {
                // Player picked Run: retail's `0x32` confirm stamps category
                // `5` on every party actor and begins the round at once
                // (`commit_party_command` does both); the escape roll is the
                // run band's own, at each member's dispatch.
                self.commit_party_command(session.actor, PendingPartyAction::Run);
            }
            Some(Resolution::Aborted) => {
                // No valid target the player could pick - commit a default
                // strike on the first living monster so the round progresses.
                let actor = session.actor;
                let target = (party_count..self.actors.len() as u8)
                    .find(|&i| self.actors[i as usize].battle.liveness != 0)
                    .unwrap_or(party_count);
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                }
                self.commit_party_command(actor, PendingPartyAction::Attack { target });
            }
            None => {
                // Still selecting - keep the session open for the next frame.
                self.battle_command = Some(session);
            }
        }
    }

    /// Seed party member `actor`'s action-parameter stream
    /// (`actor[+0x1DF..]`) with the swing sequence a physical Attack executes,
    /// and return how many swing bytes were written.
    ///
    /// **This is the byte stream the whole attack band runs on.** State
    /// `0x1E` walks `actor[+0x1DF + actor[+0x15]]` until it reads the `0x00`
    /// terminator, staging each byte as the next queued anim; with an
    /// all-zero stream the loop reads its terminator on the first byte and
    /// falls straight through to recovery, which is a strike-less turn - no
    /// weapon swing staged, no equipment clip committed, no effect script
    /// installed, and therefore no move-power record for the weapon-trail
    /// pass to project from ([`World::move_fx_streak`], whose `action` key is
    /// this stream's first byte).
    ///
    /// The bytes come from [`vm::battle_action::basic_attack_queue`], the port
    /// of the one queue arm retail builds **without** running the Arts command
    /// gauge (`FUN_801EED1C`'s no-directional-input arm): two independently
    /// rolled Left/Right arm swings against an ordinary target, one low swing
    /// against a [`vm::battle_action::LOW_SWING_TARGET_CLASS`] target. The
    /// engine's Attack command is exactly that situation - it resolves a
    /// target with no direction input - so it is the retail kernel that
    /// applies, and the alternative (the player's own recorded chain, retail
    /// `FUN_801DA34C` /
    /// [`vm::battle_action::preseed_action_queue`]) still has no engine-side
    /// carrier to read from.
    ///
    /// **Disclosed stand-in.** Retail picks between the two shapes on the
    /// target monster record's `+0x1E` byte, which
    /// `legaia_asset::monster_archive` does not parse and `MonsterDef` does
    /// not carry, so [`Self::attack_swing_class_of`] answers `0` and the
    /// live path always takes the two-arm-swing form. That is retail's own
    /// behaviour for every non-class-`2` target; only the low-swing collapse
    /// is unreachable until the byte is parsed.
    ///
    /// REF: FUN_801EED1C
    pub(in crate::world) fn seed_basic_attack_queue(&mut self, actor: u8, target: u8) -> usize {
        let swing_class = self.attack_swing_class_of(target);
        let mut queue = [0u8; vm::battle_action::ACTION_QUEUE_CAP];
        let written =
            vm::battle_action::basic_attack_queue(&mut queue, swing_class, &mut || self.next_rng());
        if let Some(a) = self.actors.get_mut(actor as usize) {
            let len = a.battle.params.len().min(queue.len());
            a.battle.params[..len].copy_from_slice(&queue[..len]);
            a.battle.strike_index = 0;
        }
        written
    }

    /// The acting slot's Miracle marker `ctx[+0x25F + slot]`, as retail's
    /// party battle-actor seeding writes it: `1` when the character's Ra-Seru
    /// equipment byte is occupied.
    ///
    /// Retail arms it **once per battle** in `FUN_80053CB8`; the port reads it
    /// at queue-build time instead, which is the same value - the byte it
    /// depends on is a character-record equipment slot, and equipment cannot
    /// change between battle entry and an arts commit. A roster slot with no
    /// record (a synthetic party, a zeroed roster) reads unarmed, which is
    /// retail's answer for an empty Ra-Seru slot.
    ///
    /// PORT: FUN_80053CB8 (`0x800541D0..0x80054274`, via
    /// [`vm::battle_action::miracle_marker_armed`])
    pub(in crate::world) fn miracle_marker_armed_for(&self, roster_slot: u8) -> bool {
        let Some(record) = self.roster.members.get(roster_slot as usize) else {
            return false;
        };
        // Retail's table `0x8007BD10` holds a **1-based** char id; the
        // engine's `active_party` mirror holds the 0-based roster slot.
        vm::battle_action::miracle_marker_armed(
            roster_slot.wrapping_add(1),
            &record.equipment().slots,
        )
    }

    /// The target's swing class - retail's monster record `+0x1E`, read
    /// record-direct through the `0x801C9348` pointer table by
    /// `FUN_801EED1C`'s no-input attack arm.
    ///
    /// Resolved through the slot's seated monster id and the catalog's
    /// [`crate::monster_catalog::MonsterDef::swing_class`] (record `+0x1E`).
    /// A party target, an empty slot, or a synthetic catalog with no disc
    /// record reads `0` - the class that takes the ordinary two-swing attack,
    /// which is retail's answer for every non-class-`2` target.
    pub(in crate::world) fn attack_swing_class_of(&self, target: u8) -> u8 {
        let Some(id) = self
            .actors
            .get(target as usize)
            .and_then(|a| a.battle_monster_id)
        else {
            return 0;
        };
        self.monster_catalog
            .get(id)
            .map(|d| d.swing_class)
            .unwrap_or(0)
    }

    /// Stamp (or clear) the retail target-select tint across the monster
    /// slots for the open command session.
    ///
    /// Retail's picker writes the pointed-at slot straight into the acting
    /// actor's `+0x1DD` and `FUN_801DA6B4` reads it back from there, so the
    /// port mirrors the engine picker's cursor onto `active_target` while an
    /// enemy row is live. Any other phase - the command menu, an ally row, a
    /// resolved session - runs the clear pass, which is retail's
    /// `param_1 != 0` arm.
    ///
    /// The stamps are the kernel's three render words
    /// ([`vm::battle_action::target_cursor_highlight`] - flag 5/200, the
    /// bright/dim colour words, the q12 tint blend), applied here over the
    /// **engine's** monster window: retail walks the fixed table slots
    /// `3..=6` because its monsters always seat there, but the engine
    /// compacts seating to `party_count..`, so with fewer than three party
    /// members the kernel's fixed window lands on empty slots and no monster
    /// is ever tinted. Same law, engine seat numbering.
    ///
    /// REF: FUN_801DA6B4
    fn apply_target_cursor_tint(&mut self, session: &crate::battle_input::BattleCommandSession) {
        use crate::target_picker::{CursorRow, PickerState};
        use vm::battle_action::{
            CURSOR_BLEND_ON, CURSOR_COLOR_BRIGHT, CURSOR_COLOR_DIM, CURSOR_FLAG_DIMMED,
            CURSOR_FLAG_SELECTED,
        };
        let party_count = self.party_count.clamp(1, 3);
        let enable = match session.picker().map(|p| p.state()) {
            Some(PickerState::Cursor {
                row: CursorRow::Enemy,
                slot,
            }) => {
                let abs = party_count.saturating_add(slot);
                if let Some(a) = self.actors.get_mut(session.actor as usize) {
                    a.battle.active_target = abs;
                }
                true
            }
            _ => false,
        };
        let active_target = self
            .actors
            .get(session.actor as usize)
            .map(|a| a.battle.active_target)
            .unwrap_or(0);
        // The engine's four-slot monster window (retail `3..=6` re-based to
        // the compacted seating).
        for slot in party_count..party_count.saturating_add(4) {
            let selected = slot == active_target;
            let Some(actor) = self.actors.get_mut(slot as usize).map(|a| &mut a.battle) else {
                continue;
            };
            // Dead slots keep their state (retail `+0x14C != 0` gate).
            if actor.liveness == 0 {
                continue;
            }
            if !enable {
                actor.render_flag = 0;
                actor.render_blend = 0;
                actor.render_color = CURSOR_COLOR_BRIGHT;
            } else if selected {
                actor.render_blend = CURSOR_BLEND_ON;
                actor.render_flag = CURSOR_FLAG_SELECTED;
                actor.render_color = CURSOR_COLOR_BRIGHT;
            } else {
                actor.render_blend = CURSOR_BLEND_ON;
                actor.render_flag = CURSOR_FLAG_DIMMED;
                actor.render_color = CURSOR_COLOR_DIM;
            }
        }
    }

    /// Drive the open battle Arts submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`crate::battle_arts::BattleArtsInput`] per
    /// frame. A confirmed row commits its direction string
    /// ([`Self::run_battle_art`]); the queue is built and the attack band
    /// armed at the caster's dispatch. Backing out reopens the command menu.
    pub(in crate::world) fn tick_battle_arts_menu(&mut self) {
        use crate::battle_arts::{ArtsResolution, BattleArtsInput};
        use crate::input::PadButton;

        let Some(mut menu) = self.battle_arts_menu.take() else {
            return;
        };

        // Same validator-backed target rows as the command menu.
        let (party, monsters) = self.battle_target_rows();

        let ev = BattleArtsInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        menu.input(ev, party, monsters);

        match menu.resolved() {
            Some(ArtsResolution::Confirmed {
                art_index,
                target_row,
                target_slot,
            }) => {
                let caster = menu.actor;
                // A saved-chain row is its directional string: the same
                // arrows the command input would have produced, run through
                // the same queue-builder.
                let sequence = menu
                    .arts
                    .get(art_index as usize)
                    .map(|a| a.sequence.clone())
                    .unwrap_or_default();
                self.run_battle_art(caster, &sequence, target_row, target_slot);
            }
            Some(ArtsResolution::Aborted) => {
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            None => {
                self.battle_arts_menu = Some(menu);
            }
        }
    }

    /// Open the retail-model **Arts command input** for `actor`: seed the
    /// AP pool from the acting character's AGL (retail `ctx+0x6DC` <-
    /// actor `+0x154`; [`crate::arts_command_input::DEFAULT_POOL`] without
    /// stats), the four per-direction press costs (Left = the arm command
    /// `0x0C`, carrying the per-(character, weapon) `+0x74` byte from
    /// [`Self::battle_arm_costs`]; the others at the favored base), and the
    /// Triangle arts-list page count from the caster's loaded art catalog.
    ///
    /// PORT: FUN_801D0748 (state 0x50 arm)
    /// REF: FUN_801D388C
    /// `true` while a party member owns the pad in the retail-model Arts
    /// command input. Retail parks the party **status plate off-screen**
    /// for the whole session (its draws move to `y = 230`, below the
    /// 228-line display window - `docs/subsystems/minigame-muscle-dome.md`
    /// § Arts command input), so a host's battle-HUD strip reads this and
    /// emits nothing while it holds.
    pub fn arts_input_active(&self) -> bool {
        self.battle_arts_input.is_some()
    }

    /// The actor-table index of the party member entering commands, or
    /// `None` when no session is open. The party surface has two mutually
    /// exclusive forms - roster panels, and a full-width bar for the actor
    /// that owns the pad - so a host needs the *which*, not just the
    /// *whether*.
    pub fn arts_input_actor(&self) -> Option<u8> {
        self.battle_arts_input.as_ref().map(|s| s.actor)
    }

    /// Renderer-agnostic view of the open Arts command input, or `None`
    /// when no session is up. Both hosts build the pinned chrome from
    /// this and nothing else.
    pub fn arts_input_view(&self) -> Option<crate::arts_command_input::ArtsInputView<'_>> {
        let s = self.battle_arts_input.as_ref()?;
        Some(crate::arts_command_input::ArtsInputView {
            buffer: &s.buffer,
            spent: &s.spent,
            pool: s.pool,
            pool_max: s.pool_max,
            costs: s.costs,
            // The right-hand plate reads the caster's **Spirit** gauge and
            // never moves during entry - the entry budget's visible form is
            // the bar. Without a live Spirit value the pool stands in.
            plate_value: self.spirit_gauge(s.actor).min(100) as u8,
            list_page: s.list_page,
            list_pages: s.list_pages,
            phase: (&s.phase).into(),
        })
    }

    pub(in crate::world) fn open_arts_command_input(&mut self, actor: u8) {
        use crate::arts_command_input::{
            ARTS_LIST_ROWS_PER_PAGE, ArtsCommandInputSession, DEFAULT_POOL, FAVORED_COST,
        };
        let char_slot = self.party_roster_slot(actor as usize) as u8;
        let pool = self
            .roster
            .members
            .get(char_slot as usize)
            .map(|r| r.live_stats().agl)
            .filter(|&a| a > 0)
            .unwrap_or(DEFAULT_POOL);
        // Cost order = Command byte order (Left, Right, Down, Up) = the
        // runtime action slots `0xC..=0xF` the disc bytes are keyed by.
        let costs = self
            .battle_swing_costs
            .get(char_slot as usize)
            .copied()
            .unwrap_or([FAVORED_COST; 4]);
        let character = self.caster_character(char_slot);
        let n_arts = self
            .art_records
            .iter()
            .filter(|((ch, _), rec)| *ch == character && !rec.commands.is_empty())
            .count();
        let pages = n_arts.div_ceil(ARTS_LIST_ROWS_PER_PAGE) as u8;
        self.battle_arts_input = Some(ArtsCommandInputSession::new(
            actor, actor, pool, costs, pages,
        ));
    }

    /// Drive the open Arts command input one frame from [`World::input`].
    ///
    /// On a confirmed Begin the entered sequence is committed
    /// ([`Self::run_battle_art`]); retail's queue-builder runs over it at
    /// the caster's dispatch and the attack band stages the result. Backing
    /// out (empty buffer + Circle, or no valid target) reopens the command
    /// menu.
    pub(in crate::world) fn tick_battle_arts_input(&mut self) {
        use crate::arts_command_input::{ArtsCommandPad, ArtsInputResolution};
        use crate::input::PadButton;

        let Some(mut session) = self.battle_arts_input.take() else {
            return;
        };
        let (party, monsters) = self.battle_target_rows();
        let ev = ArtsCommandPad {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
            triangle: self.input.just_pressed(PadButton::Triangle),
        };
        session.input(ev, party, monsters);

        match session.resolved() {
            Some(ArtsInputResolution::Confirmed {
                target_row,
                target_slot,
            }) => {
                let caster = session.actor;
                self.run_battle_art(caster, &session.buffer, target_row, target_slot);
            }
            Some(ArtsInputResolution::Aborted) => {
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            None => {
                self.battle_arts_input = Some(session);
            }
        }
    }

    /// Build the action queue retail's queue-builder `FUN_801EED1C` writes
    /// into `actor[+0x1DF..]` for an entered arrow string - byte-exact, not
    /// structural:
    ///
    /// 1. the tokenizer pass ([`legaia_art::tokenize`]): the arrows become
    ///    `0x0C..0x0F` swings, each matched art gets the `0x19` starter
    ///    written over its **last** arrow and its constant inserted after
    ///    it, leading arrows stay and arts overlap (`↑↓↑` -> `0F 0E 19 27`);
    /// 2. the learn-on-use check per accepted art (`FUN_801EFBFC`, `jal` at
    ///    `0x801EF44C`): a newly learned art's starter is `0x1A` instead
    ///    (`addiu v1,t3,0x18` with `t3 = 2`, `0x801EF6F0`);
    /// 3. the finish ([`vm::battle_action::finish_action_queue`]): the
    ///    Miracle replacement, the MSB-clear sweep and the Super
    ///    tail-replace, in that order.
    ///
    /// Returns the 19-byte stream window and the art constants it performs,
    /// in order (the shout-cue list). A character with no art catalog gets
    /// its arrows as plain swings, which is retail's own answer for an
    /// unmatched string.
    ///
    /// PORT: FUN_801EED1C (the player path: normalise + learn + finish;
    /// the tokenizer body is `legaia_art::tokenize`, the finish passes
    /// `legaia_engine_vm::battle_action::queue_applier`)
    pub(in crate::world) fn build_arts_action_queue(
        &mut self,
        caster: u8,
        commands: &[legaia_art::Command],
    ) -> (
        [u8; vm::battle_action::ACTION_QUEUE_CAP],
        Vec<legaia_art::ActionConstant>,
    ) {
        use legaia_art::ActionConstant;
        use vm::battle_action::ACTION_QUEUE_CAP;
        let roster = self.party_roster_slot(caster as usize) as u8;
        let character = self.caster_character(roster);
        // The character's art catalog in grid order (ascending constant),
        // the order the builder's inner loop walks (`s3 = 0xB..`).
        let mut catalog: Vec<(ActionConstant, Vec<legaia_art::Command>)> = self
            .art_records
            .iter()
            .filter(|((ch, action), rec)| {
                // Only the **normal** arts (ordinal `>= 4`, constants
                // `0x1F+`): the builder's inner loop routes the Miracle Art
                // and the three Hyper Arts (ordinals `0..=3`) through a
                // different arm (`sltiu a1,a0,0x4` at `0x801EF330`) that,
                // with the slot's `+0x25F` marker clear, writes nothing -
                // their combo bytes never tokenize as arts.
                // ... and only combos of two arrows or more: a fully matched
                // one-arrow string takes the builder's `s1 == 1` exit
                // (`0x801EF420..0x801EF434`) with no rewrite - the disc's
                // one-arrow record is the Miracle finisher's, and letting it
                // match would steal an arrow from every art containing it.
                *ch == character
                    && rec.commands.len() >= 2
                    && action.as_byte() >= NORMAL_ART_MIN_CONSTANT
            })
            .map(|((_, action), rec)| (*action, rec.commands.clone()))
            .collect();
        catalog.sort_by_key(|(a, _)| a.as_byte());
        let entries: Vec<legaia_art::tokenize::ArtEntry<'_>> =
            catalog.iter().map(|(a, c)| (*a, c.as_slice())).collect();
        let tokens = legaia_art::tokenize(&entries, commands);
        log::debug!(
            "arts queue: {:?} over {:?} -> {:02x?}",
            commands,
            catalog
                .iter()
                .map(|(a, c)| (a.as_byte(), c.as_slice()))
                .collect::<Vec<_>>(),
            &tokens[..]
        );
        let mut bytes = [0u8; ACTION_QUEUE_CAP];
        bytes[..tokens.len()].copy_from_slice(&tokens);
        // Learn-on-use per accepted art, in the builder's own **tail-first**
        // order (`s8` from 15 down, `0x801EF848`, restarting at `s8 + 1`
        // after each match): when one art appears twice in a queue, the
        // *last* occurrence is the one `FUN_801EFBFC` sees first and so the
        // one that gets the `0x1A` newly-learned starter. The marked-starter
        // reorder in `finish_action_queue` then walks it back to the art's
        // first occurrence, which is what makes that pass load-bearing rather
        // than a no-op. Retail's insert gate (`ctx[+0x266 + slot]`) has no
        // engine analogue and reads open.
        for i in (0..tokens.len().saturating_sub(1)).rev() {
            if bytes[i] != ActionConstant::RegularStarter.as_byte() {
                continue;
            }
            let Some(art) = ActionConstant::from_byte(bytes[i + 1]).filter(|a| a.is_art()) else {
                continue;
            };
            let id = art.as_byte();
            let known = self.tactical_arts.is_learned(roster, id);
            self.notify_art_used(roster, id);
            if !known && self.tactical_arts.is_learned(roster, id) {
                bytes[i] = ActionConstant::SpecialStarter.as_byte();
            }
        }
        let miracle_armed = self.miracle_marker_armed_for(roster);
        vm::battle_action::finish_action_queue(character, commands, miracle_armed, &mut bytes);
        let actions: Vec<ActionConstant> = bytes
            .iter()
            .take_while(|&&b| b != 0)
            .filter_map(|&b| ActionConstant::from_byte(b))
            .filter(|a| a.is_art())
            .collect();
        (bytes, actions)
    }

    /// Commit an entered Tactical-Arts string (`sequence` = the direction
    /// command bytes `1..=4`, the arts input's buffer or a saved chain's
    /// string) against the picked target: the arts screen's commit (`0x50`
    /// -> `0x5A` -> the ring walk). The entry executes at the caster's
    /// dispatch ([`Self::execute_battle_art`]), once every member has
    /// committed (retail `0x6E -> 0xFE`).
    pub(in crate::world) fn run_battle_art(
        &mut self,
        caster: u8,
        sequence: &[u8],
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) {
        if let Some(a) = self.actors.get_mut(caster as usize) {
            a.battle.action_category = 3;
        }
        self.commit_party_command(
            caster,
            crate::battle_round::PendingPartyAction::Art {
                sequence: sequence.to_vec(),
                target_row,
                target_slot,
            },
        );
    }

    /// Execute a committed Tactical-Arts turn at the caster's dispatch:
    /// build retail's action queue ([`Self::build_arts_action_queue`]) and
    /// arm the action SM's attack band with it
    /// ([`Self::arm_battle_art_action`]). The band then stages the queue
    /// byte by byte - each swing, starter and art constant its own clip -
    /// and the hit-event driver resolves damage on each clip's own beats,
    /// so a three-arrow art is two swings and the art, paced by the clips,
    /// exactly as retail runs it.
    fn execute_battle_art(
        &mut self,
        caster: u8,
        sequence: &[u8],
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) {
        let commands: Vec<legaia_art::Command> = sequence
            .iter()
            .filter_map(|&b| legaia_art::Command::from_byte(b))
            .collect();
        let (queue, actions) = self.build_arts_action_queue(caster, &commands);
        self.charge_art_spirit(caster, &actions);
        self.arm_battle_art_action(caster, &queue, &actions, target_row, target_slot);
    }

    /// Charge the turn's **art bodies** out of the caster's Spirit gauge
    /// (`actor[+0x170]`) - the second half of the two-gauge split. The
    /// direction commands are already paid, out of the entry pool
    /// (`ctx+0x6DC`, seeded from AGL); this is the price of the arts those
    /// directions matched, and until it was wired a turn's whole cost was its
    /// swings and an art body was free.
    ///
    /// The amount is [`crate::ap_gauge::arts_turn_spirit_cost`] over the
    /// caster's builder-order catalog ([`crate::battle_arts::spirit_catalog`])
    /// and the arts the queue actually performs. Retail accrues it into
    /// `actor[+0x224]` inside the builder and spends it once in the
    /// battle-action cleanup arm (`0x801E5D74`); the port charges it here, at
    /// the commit, which is the same turn and the same total.
    ///
    /// A plain attack charges nothing, because an unmatched arrow string
    /// performs no art and `actions` is empty.
    ///
    /// PORT: FUN_801EED1C (Spirit-cost half) / FUN_801E295C (the `+0x224` spend)
    fn charge_art_spirit(&mut self, caster: u8, actions: &[legaia_art::ActionConstant]) {
        if actions.is_empty() {
            return;
        }
        let roster = self.party_roster_slot(caster as usize) as u8;
        let character = self.caster_character(roster);
        let catalog = crate::battle_arts::spirit_catalog(&self.art_records, character);
        // Retail's halving gate is the acting actor's `0x800` flag
        // (`srl t4,t4,0x1` at `0x801EF378`); the port has no carrier for it
        // yet, so the full-price arm is the one that runs.
        let cost = crate::ap_gauge::arts_turn_spirit_cost(&catalog, actions, false);
        if let Some(a) = self.actors.get_mut(caster as usize) {
            a.battle.spirit_gauge = a.battle.spirit_gauge.saturating_sub(cost);
        }
    }

    /// Stage a built action queue on the acting actor and arm the action
    /// SM's attack band for it: category `3`, the target, `Begin`. The
    /// stream is the queue **verbatim** (retail's `+0x1DF..` window), with
    /// the `0x00` terminator the band stops on guaranteed inside the
    /// stream. An empty queue still arms: the band reads its terminator on
    /// byte 0 and drops to recovery, consuming the turn - retail's own
    /// answer to an input with nothing in it.
    ///
    /// REF: FUN_801E295C (state `0x0C` ActionSeed, which calls the builder
    /// for the acting party slot and seeds category 3)
    fn arm_battle_art_action(
        &mut self,
        caster: u8,
        queue: &[u8],
        actions: &[legaia_art::ActionConstant],
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) {
        use crate::target_picker::CursorRow;
        let party_count = self.party_count.clamp(1, 3);
        let target = match target_row {
            CursorRow::Enemy => party_count + target_slot,
            CursorRow::Ally => target_slot,
        };
        if usize::from(target) >= self.actors.len() {
            return;
        }
        // The **roster**-slot keying the queue was built under, not the
        // battle ordinal: the hit-event driver looks the art record up by
        // this key, so the two have to agree or a three-member party reads
        // the wrong character's table.
        let char_slot = self.party_roster_slot(caster as usize) as u8;
        let character = self.caster_character(char_slot);
        self.push_art_shout_cues(caster, actions);
        // A freshly-armed action starts from an empty strike script - see
        // `World::clear_action_stream` for the soft-lock a carried-over byte
        // produces.
        self.clear_action_stream(caster);
        let Some(a) = self.actors.get_mut(caster as usize) else {
            return;
        };
        let n = queue.len().min(a.battle.params.len().saturating_sub(1));
        a.battle.params[..n].copy_from_slice(&queue[..n]);
        a.battle.params[n] = 0;
        a.battle.strike_index = 0;
        a.battle.active_target = target;
        a.battle.action_category = 3;
        // The art-record lookup key. Nothing else in the port writes it, so
        // an unset slot would resolve every character's arts against Vahn's
        // table.
        a.battle.character = character;
        a.battle.chosen_art = None;
        self.battle_ctx.active_actor = caster;
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
    }

    /// Dispatch the command `actor` committed this round - the engine's
    /// counterpart of the action SM's `0x0C` seed for a party slot, run when
    /// the initiative pick lands on the member. Everything the old commit
    /// sites armed on the spot is armed here instead: the swing stream
    /// (`FUN_801EED1C`), the art profile, the cast, the item effect + its
    /// cast band, the Spirit charge, the escape roll + run band.
    ///
    /// REF: FUN_801E295C (state `0x0C`, the party-slot `jal 0x801EED1C`)
    /// REF: FUN_801EED1C
    pub(in crate::world) fn dispatch_pending_party_action(
        &mut self,
        actor: u8,
        action: crate::battle_round::PendingPartyAction,
    ) {
        use crate::battle_round::PendingPartyAction as Pending;
        use vm::battle_action::ActionState;
        self.battle_ctx.active_actor = actor;
        match action {
            Pending::Attack { target } => {
                // A freshly-armed action starts from an empty strike script -
                // see [`World::clear_action_stream`] for the soft-lock a
                // carried-over byte produces.
                self.clear_action_stream(actor);
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3; // Attack
                }
                // ... and then seeds it, which is what makes the attack band's
                // strike loop a loop instead of an immediate exit.
                self.seed_basic_attack_queue(actor, target);
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
            }
            Pending::Art {
                sequence,
                target_row,
                target_slot,
            } => self.execute_battle_art(actor, &sequence, target_row, target_slot),
            Pending::Spell {
                spell_id,
                target_row,
                target_slot,
            } => {
                // The dispatch commits the category-2 action; the action SM's
                // Magic band carries it from here - facing, the MP debit and
                // the `0x14`-frame wait at `0x28`/`0x29`, the summon band for
                // a Seru id - and the outcome folds at retail's seam
                // (`World::settle_cast_band` / the stager's strike). An
                // escape spell's success ends the encounter from the live
                // loop the frame it folds, through the escape teardown.
                match self.spell_catalog.get(spell_id).cloned() {
                    Some(def) => {
                        let targets = self.spell_targets_for(&def, target_row, target_slot);
                        self.arm_player_cast(actor, &def, targets);
                    }
                    None => {
                        // Not a catalog spell (the submenu only lists catalog
                        // ids, so this is defensive): the turn is spent.
                        self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
                        self.cycle_battle_turn();
                    }
                }
            }
            Pending::Item {
                item_id,
                used_slots,
            } => {
                // Apply to every affected slot (the copy went at the commit).
                for &target_slot in &used_slots {
                    let outcome = self.apply_battle_item(item_id, target_slot);
                    self.push_item_use_fx(target_slot, outcome);
                }
                if self.battle_escaped {
                    // Escape item succeeded: leave the encounter (no loot, no
                    // game-over) through the escape teardown's fade + exit
                    // hold instead of cycling the turn.
                    self.battle_end = Some(BattleEndCause::Escaped);
                    self.begin_battle_end_sequence();
                    return;
                }
                // Using an item is the actor's whole turn - and in retail the
                // turn *is* the action SM's Item band: the committed
                // category-1 action seeds through `FUN_801E295C`'s item arm
                // (`item_seed_band`) into the `0x3C..0x40` cast states, which
                // fire the cast-audio cue (`FUN_801F3990` via `spirit_wait`),
                // stamp the item's effect-descriptor `(class, tier)` pair, and
                // expand the item's cue group (`FUN_800402F4`'s eleven
                // `FUN_801E22C8` sites via `place_cue_group`). The simulation
                // fold stays above; the band's own `apply_damage` hook is the
                // presentation seam. The two SummonFlute ids (`0x98`/`0x99`)
                // reroute inside `action_seed` to the summon band, which the
                // live loop's settle glue (`live_battle_tick`) walks to
                // completion.
                let target = if used_slots.len() > 1 {
                    vm::battle_cue_group::TARGET_PARTY_WIDE
                } else {
                    used_slots.first().copied().unwrap_or(actor)
                };
                self.clear_action_stream(actor);
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = vm::battle_action::ActionCategory::Item.as_byte();
                    a.battle.params[0] = item_id;
                }
                self.battle_ctx.queued_action = vm::battle_action::ActionCategory::Item.as_byte();
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
            }
            Pending::Spirit => {
                // The AP charge (+5, idempotent per turn - the retail
                // Square-press kernel). The guard stance has been up since
                // the commit.
                if let Some(gauge) = self.ap_gauges.get_mut(actor as usize) {
                    gauge.charge_spirit();
                }
                if let Some(guard) = self.battle_guarding.get_mut(actor as usize) {
                    *guard = true;
                }
                self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
                self.cycle_battle_turn();
            }
            Pending::Run => {
                // Roll the escape and arm the action SM's run band (category
                // 5 -> RunBegin/RunWait/RunEscape, retail 0x64..0x66). The SM
                // carries the roll outcome on `multi_cast_gate` (success
                // floors downed party HP at 1 and tears the battle down
                // `Escaped`; failure consumes the turn via the Done band).
                // The roll is the retail `FUN_801E791C` formula (the writer of
                // `_DAT_8007726C`): party SPD*1.5 + missing-HP/16 vs enemy SPD
                // + missing-HP/32, two rand draws, Chicken Heart/King accessory
                // bits folded from the living party members' second ability
                // word. Retail rolls it inside the run band (`0x801E57C8`),
                // once per party member that dispatches with category 5.
                let escaped = self.roll_battle_escape();
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.action_category = 5; // Run band
                }
                self.battle_ctx.queued_action = 5;
                self.battle_ctx.multi_cast_gate = u8::from(escaped);
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
            }
            Pending::StandBy => {
                // Category 0 dispatches straight to the Done band.
                self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
                self.cycle_battle_turn();
            }
        }
    }

    /// Arts-voice shout: one cue **per art the turn performs**, on that
    /// art's animation-start frame, when the art carries a real action
    /// constant (a synthetic/demo art has none and stays silent - the
    /// retail degradation for arts with no cue-table entry). Retail
    /// stages each art's animation separately and the materialiser
    /// (`FUN_8004AD80`) calls the cue selector per staging, so an entry
    /// that performs three arts requests three shouts; the mixer queues
    /// a back-to-back request behind the sounding one rather than cutting
    /// it. The host resolves each (character, action) pair against the
    /// arts-voice tables + XA clip banks and plays the CD-XA shout with the
    /// modeled CD-response delay, so the audio trails this frame rather than
    /// leading it. REF: FUN_8004C140.
    fn push_art_shout_cues(&mut self, caster: u8, actions: &[legaia_art::ActionConstant]) {
        let character = self.caster_character(caster);
        let cslot = legaia_art::Character::all()
            .iter()
            .position(|c| *c == character)
            .unwrap_or(usize::MAX);
        if cslot >= 3 {
            return;
        }
        for action in actions {
            self.battle_shout_cues
                .push(crate::battle_events::BattleShoutCue {
                    cslot: cslot as u8,
                    action: action.as_byte(),
                });
        }
    }

    /// Build the battle Magic submenu for `caster` (an actor-table / party-row
    /// index). Reads the caster's learned spells off their roster record and
    /// their live battle MP to grey out unaffordable rows. Returns `None` when
    /// there's no roster record for the slot, OR when the caster is **silenced
    /// / petrified** (a `blocks_magic` status) - in both cases the caller
    /// reopens the command menu so the player picks a non-magic action, which
    /// is the party-side mirror of the monster AI's cast→physical fallback.
    pub(in crate::world) fn build_battle_spell_session(
        &self,
        caster: u8,
    ) -> Option<crate::battle_magic::BattleSpellSession> {
        if self.actor_blocked_from_magic(caster) {
            return None;
        }
        // `caster` is the battle ordinal; the spell list belongs to the
        // CHARACTER occupying it (roster slot per the present-party
        // composition). Live mirrors (MP, ability bits) stay ordinal-keyed.
        let char_slot = self.party_roster_slot(caster as usize) as u8;
        let member = self.roster.members.get(char_slot as usize)?;
        let list = member.spell_list();
        let n = (list.count as usize).min(list.ids.len());
        // Union the roster's saved spell list with anything learned via Seru
        // capture this session, so a freshly-learned spell is immediately
        // castable without waiting for a save/load round-trip.
        let mut learned: Vec<u8> = list.ids[..n].to_vec();
        for &sid in self.seru_log.learned_spells(char_slot) {
            if !learned.contains(&sid) {
                learned.push(sid);
            }
        }
        let caster_mp = self
            .actors
            .get(caster as usize)
            .map(|a| a.battle.mp)
            .unwrap_or(0);
        // Pass the caster's MP-saver ability bits so the menu greys rows by the
        // effective (reduced) cost the cast charges, not the raw spell cost.
        let ability_bits = self
            .character_ability_bits
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        Some(crate::battle_magic::BattleSpellSession::new(
            caster,
            caster,
            &learned,
            &self.spell_catalog,
            caster_mp,
            ability_bits,
        ))
    }

    /// Drive the open battle Magic submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`crate::battle_magic::BattleSpellInput`] per
    /// frame. On a confirmed cast the action SM's Magic band is armed with the
    /// committed spell and its resolved targets ([`Self::arm_player_cast`]);
    /// the band charges the MP, plays the cast, and the outcome folds at
    /// retail's seam - a cast is the caster's whole turn, no strike fires.
    /// Backing out reopens the command menu for the same actor.
    pub(in crate::world) fn tick_battle_spell_menu(&mut self) {
        use crate::battle_magic::{BattleSpellInput, SpellResolution};
        use crate::input::PadButton;

        let Some(mut menu) = self.battle_spell_menu.take() else {
            return;
        };

        // Same validator-backed target rows as the command menu.
        let (party, monsters) = self.battle_target_rows();

        let ev = BattleSpellInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        menu.input(ev, &self.spell_catalog, party, monsters);

        match menu.resolved() {
            Some(SpellResolution::Confirmed {
                spell_id,
                target_row,
                target_slot,
            }) => {
                // The magic window's commit (`0x46` -> its sub-cursor ->
                // the ring walk): the cast itself is the caster's dispatch.
                let caster = menu.actor;
                if let Some(a) = self.actors.get_mut(caster as usize) {
                    a.battle.action_category = 2;
                }
                self.commit_party_command(
                    caster,
                    crate::battle_round::PendingPartyAction::Spell {
                        spell_id,
                        target_row,
                        target_slot,
                    },
                );
            }
            Some(SpellResolution::Aborted) => {
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            None => {
                self.battle_spell_menu = Some(menu);
            }
        }
    }

    /// Build the battle-context inventory submenu from live world state:
    /// every item the player holds (`count > 0`), one party-member target row
    /// per configured party slot, then one enemy row per live monster slot
    /// (tagged `is_enemy`). Healing / cure / revive items validate against the
    /// party rows; offensive items (Bomb / capture / escape) validate against
    /// the enemy rows - the session routes the cursor to the correct side.
    pub(in crate::world) fn build_battle_item_session(
        &self,
    ) -> crate::inventory_use::InventoryUseSession {
        use crate::inventory_use::{InventoryContext, InventoryUseSession, TargetRow};
        let names = crate::field_menu_dispatch::roster_names(self);
        let items: Vec<u8> = self
            .inventory
            .iter()
            .filter_map(|(id, qty)| (*qty > 0).then_some(*id))
            .collect();
        let pc = self.party_count.clamp(1, 3) as usize;
        let mut targets: Vec<TargetRow> = (0..pc)
            .filter_map(|i| {
                let a = self.actors.get(i)?;
                // Skip unconfigured party slots (no battle stats).
                if a.battle.max_hp == 0 {
                    return None;
                }
                let mp_max = self.character_max_mp.get(i).copied().unwrap_or(0);
                // Row label = the occupying character's name (roster_names is
                // roster-slot keyed; `i` is the battle ordinal).
                let name = names
                    .get(self.party_roster_slot(i))
                    .cloned()
                    .unwrap_or_else(|| format!("P{}", i + 1));
                let mut row = TargetRow::new(i as u8, name)
                    .with_stats(a.battle.hp, a.battle.max_hp, a.battle.mp, mp_max)
                    .with_statuses(self.status_effects.statuses(i as u8).iter().map(|s| s.kind));
                row.alive = a.battle.liveness != 0;
                Some(row)
            })
            .collect();
        // Enemy rows: every monster slot that's configured for battle. Tagged
        // `is_enemy` so the session only accepts offensive items here.
        for slot in pc..self.actors.len() {
            let Some(a) = self.actors.get(slot) else {
                break;
            };
            if a.battle.max_hp == 0 || a.battle_monster_id.is_none() {
                continue;
            }
            let name = a
                .battle_monster_id
                .and_then(|id| self.monster_catalog.get(id))
                .map(|d| d.name.clone())
                .unwrap_or_else(|| format!("Enemy {}", slot - pc + 1));
            let mut row = TargetRow::new(slot as u8, name)
                .with_stats(a.battle.hp, a.battle.max_hp, 0, 0)
                .with_enemy(true);
            row.alive = a.battle.liveness != 0;
            targets.push(row);
        }
        InventoryUseSession::new(
            self.item_catalog.clone(),
            items,
            targets,
            InventoryContext::Battle,
        )
    }

    /// Owned projection of the open battle item menu for the windowed item
    /// surface (`legaia_engine_ui::battle_item_ui` on both hosts): the
    /// dedup row list + mapped cursor
    /// ([`InventoryUseSession::menu_view`]), the highlighted item's disc
    /// info-window line, the acting member's name (the middle breadcrumb of
    /// retail's `Begin | <name> | Item` trail) and the target roster while
    /// the session is picking a target.
    ///
    /// `None` while no battle item menu is up or a dialogue box owns the
    /// frame - the same suppression the command chips follow. Living here
    /// rather than in each host keeps the two hosts on one projection
    /// (host-drift tier: paired simulation injection sites).
    pub fn battle_item_menu_model(&self) -> Option<crate::inventory_use::BattleItemMenuModel> {
        if self.mode != crate::world::SceneMode::Battle {
            return None;
        }
        if self.current_dialog.is_some() || self.inline_dialogue.is_some() {
            return None;
        }
        let menu = self.battle_item_menu.as_ref()?;
        let view = menu.menu_view();
        let description = view
            .selected_id
            .and_then(|id| self.menu_text.as_ref().and_then(|t| t.item_desc(id)))
            .map(str::to_string);
        let actor = self.battle_ctx.active_actor;
        let actor_name = menu
            .targets
            .iter()
            .find(|t| t.slot == actor)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| format!("P{}", actor + 1));
        let targets = view.target_select.then(|| {
            // Only the rows on the selected item's own side are LISTED -
            // retail's state-0x64 panel is the party roster and its cursor
            // walk wraps inside the party band `[0, ctx[+0x00])`; the
            // enemy-side item states ring the monster seats instead
            // (`FUN_801D0748`; rule pinned at
            // `crate::inventory_use::target_on_effect_side`). The session
            // cursor indexes the full roster, so it is remapped onto the
            // filtered list.
            let side_ok = |t: &crate::inventory_use::TargetRow| {
                menu.current_item()
                    .map(|e| crate::inventory_use::target_on_effect_side(&e.effect, t))
                    .unwrap_or(!t.is_enemy)
            };
            let cursor = menu
                .targets
                .iter()
                .take(view.target_cursor)
                .filter(|t| side_ok(t))
                .count();
            (
                menu.targets
                    .iter()
                    .filter(|t| side_ok(t))
                    .map(|t| crate::inventory_use::BattleItemTargetRow {
                        name: t.name.clone(),
                        hp: t.hp,
                        hp_max: t.hp_max,
                        mp: t.mp,
                        mp_max: t.mp_max,
                        alive: t.alive,
                    })
                    .collect(),
                cursor,
            )
        });
        Some(crate::inventory_use::BattleItemMenuModel {
            view,
            description,
            actor_name,
            targets,
        })
    }

    /// Drive the open battle inventory submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`crate::inventory_use::InventoryUseInput`] per
    /// frame. On a completed use the chosen item is applied authoritatively via
    /// [`Self::use_item`], one copy is consumed from the inventory, a heal /
    /// cure popup is surfaced for the HUD, and the action SM is parked at
    /// `EndOfAction` so the live loop cycles to the next combatant (no strike
    /// fires - using an item is the actor's whole turn). Backing out reopens
    /// the command menu for the same actor.
    pub(in crate::world) fn tick_battle_item_menu(&mut self) {
        use crate::input::PadButton;
        use crate::inventory_use::{InventoryUseInput, InventoryUseState};

        let Some(mut menu) = self.battle_item_menu.take() else {
            return;
        };

        // Retail's state 0x64 steps the target on the HORIZONTAL pad masks
        // (`0x2000`/`0x8000` at `0x801D2A50..0x801D2A5C` - the same pair the
        // ring uses), so during target select Left/Right drive the strip
        // cursor alongside Up/Down.
        let target_select = matches!(menu.state, InventoryUseState::TargetSelect { .. });
        let ev = if self.input.just_pressed(PadButton::Up)
            || (target_select && self.input.just_pressed(PadButton::Left))
        {
            Some(InventoryUseInput::Up)
        } else if self.input.just_pressed(PadButton::Down)
            || (target_select && self.input.just_pressed(PadButton::Right))
        {
            Some(InventoryUseInput::Down)
        } else if self.input.just_pressed(PadButton::Cross) {
            Some(InventoryUseInput::Confirm)
        } else if self.input.just_pressed(PadButton::Circle) {
            Some(InventoryUseInput::Cancel)
        } else {
            None
        };

        // The item under the cursor before the input - `current_item` reads
        // the `item_cursor` in TargetSelect, so this is the item that a Confirm
        // on a target row resolves to (the Done state no longer exposes it).
        let item_before = menu.current_item().map(|e| e.id);
        if let Some(ev) = ev {
            menu.input(ev);
        }
        // Discard the event log; `used_slots` is the authoritative list of
        // targets the completed use applied to (one for a single-target item,
        // every healed ally for an all-party item).
        let _ = menu.drain_events();
        let used_slots = menu.used_slots.clone();

        if !used_slots.is_empty() {
            // The item window's commit. Retail consumes the copy here - the
            // `0x6E` step-back and the dead-actor sweep both *refund* it
            // through `FUN_800421D4` - and the effect lands when the member
            // dispatches: the committed category-1 action seeds through
            // `FUN_801E295C`'s item arm (`item_seed_band`) into the
            // `0x3C..0x40` cast states (`dispatch_pending_party_action`).
            let actor = self.battle_ctx.active_actor;
            match item_before {
                Some(item_id) => {
                    self.consume_item(item_id);
                    if let Some(a) = self.actors.get_mut(actor as usize) {
                        a.battle.action_category =
                            vm::battle_action::ActionCategory::Item.as_byte();
                    }
                    self.commit_party_command(
                        actor,
                        crate::battle_round::PendingPartyAction::Item {
                            item_id,
                            used_slots,
                        },
                    );
                }
                // No item id resolved (defensive) - the member stands by.
                None => self
                    .commit_party_command(actor, crate::battle_round::PendingPartyAction::StandBy),
            }
            return;
        }

        match menu.state {
            InventoryUseState::Aborted => {
                // Backed out without using an item - reopen the command menu.
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            _ => {
                // Still browsing / target-selecting - keep the menu open.
                self.battle_item_menu = Some(menu);
            }
        }
    }

    /// Use `item_id` on `target_slot` **inside a battle**: the shared
    /// [`World::use_item`] resolution, plus the HP-readout seed the battle
    /// context needs and the field menu does not.
    ///
    /// `use_item` writes live HP directly. Out of battle that is complete -
    /// there is no readout. In battle it is half of retail's applier: the
    /// restore primitive `FUN_800402F4` folds the delta into the stat halfword
    /// (`0x800408A8`) **and** assigns the readout's pending accumulator
    /// `-delta` (`0x800408FC` / `0x80040D28` / `0x800410BC`). Writing only the
    /// stat leaves `hp != hp_display` with a **zero** accumulator, and that
    /// pair is absorbing: the ramp's one guard is `+0x10 != 0`
    /// (`0x800474E8`), so nothing ever moves the bar back. The action SM's
    /// `0x51` exit waits on that bar for any party-targeted action, so the
    /// next monster swing at the healed member parks the fight - with no
    /// in-battle exit, since the turn pump that would notice a KO is the thing
    /// that stopped.
    ///
    /// The seed is an **assignment**, not an accumulation, because that is
    /// what the three seed sites are: `sll/sra` the signed halfword delta,
    /// `subu v0,zero,v0`, `sw v0,0x10(actor)`. The remainder of a drain still
    /// in flight is discarded, which is retail's behaviour and not an
    /// approximation of it.
    ///
    /// REF: FUN_800402F4 (the assigning seed; kernel in
    /// `legaia_engine_vm::battle_hp_bar::assign_pending`)
    pub(in crate::world) fn apply_battle_item(
        &mut self,
        item_id: u8,
        target_slot: u8,
    ) -> crate::items::ItemOutcome {
        let before = self
            .actors
            .get(target_slot as usize)
            .map(|a| a.battle.hp)
            .unwrap_or(0);
        let outcome = self.use_item(item_id, target_slot);
        if let Some(a) = self.actors.get_mut(target_slot as usize) {
            let delta = i32::from(a.battle.hp) - i32::from(before);
            if delta != 0 {
                a.battle
                    .assign_hp_bar(delta.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
            }
        }
        outcome
    }

    /// Remove one copy of `item_id` from the inventory, dropping the entry
    /// when the count reaches zero. No-op when the player holds none.
    pub fn consume_item(&mut self, item_id: u8) {
        if let Some(qty) = self.inventory.get_mut(&item_id) {
            *qty = qty.saturating_sub(1);
            if *qty == 0 {
                self.inventory.remove(&item_id);
            }
        }
    }

    /// Surface a cosmetic HUD popup for a resolved item use. Heals / MP
    /// restores / revives push a heal-coloured number; offensive items push a
    /// damage-coloured number; cures push the status letter. The HP / status
    /// side is already applied by [`Self::use_item`]; this is presentation-only
    /// (drained via [`Self::drain_battle_hit_fx`]).
    fn push_item_use_fx(&mut self, target_slot: u8, outcome: crate::items::ItemOutcome) {
        use crate::items::ItemOutcome;
        let (amount, is_heal) = match outcome {
            ItemOutcome::HealedHp { amount } | ItemOutcome::HealedMp { amount } => (amount, true),
            ItemOutcome::Revived { hp_after } => (hp_after, true),
            ItemOutcome::DamageDealt { amount } => (amount, false),
            // Cures / capture / escape / stat boosts / no-effect: no number.
            _ => return,
        };
        if amount == 0 {
            return;
        }
        self.battle_hit_fx.push(BattleHitFx {
            target_slot,
            amount,
            is_heal,
            is_crit: false,
        });
    }
}

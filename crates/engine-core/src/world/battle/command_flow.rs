//! Player-driven battle command menu and the Arts / Magic / Item submenu
//! drivers. Split out of `battle.rs` as additional `impl World` blocks; no
//! logic change from the original inline definitions.

use super::*;

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
                // v0.1 only enables Attack, so `command` is always Attack here;
                // Arts/Magic/Item aren't wired into the live loop yet.
                command: _,
                target_row,
                target_slot,
            }) => {
                let target = match target_row {
                    CursorRow::Enemy => party_count + target_slot,
                    CursorRow::Ally => target_slot,
                };
                let actor = session.actor;
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
                self.battle_ctx.active_actor = actor;
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
                // Session done; SM resumes next tick.
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
                // Player picked Spirit: charge the AP gauge (+5, idempotent
                // per turn - the retail Square-press kernel) and raise the
                // guard stance (retail pending-action byte +0x1DE = 4, the
                // damage finisher's guard-halve input). The stance holds
                // until this actor's next turn starts. Spirit is the whole
                // turn: park at EndOfAction so the loop cycles.
                let actor = session.actor;
                if let Some(gauge) = self.ap_gauges.get_mut(actor as usize) {
                    gauge.charge_spirit();
                }
                if let Some(guard) = self.battle_guarding.get_mut(actor as usize) {
                    *guard = true;
                }
                self.battle_ctx.active_actor = actor;
                self.battle_ctx.action_state =
                    vm::battle_action::ActionState::EndOfAction.as_byte();
            }
            Some(Resolution::RunAway) => {
                // Player picked Run: roll the escape and arm the action SM's
                // run band (category 5 -> RunBegin/RunWait/RunEscape, retail
                // 0x64..0x66). The SM carries the roll outcome on
                // `multi_cast_gate` (success floors downed party HP at 1 and
                // tears the battle down `Escaped`; failure consumes the turn
                // via the Done band). The roll is the retail `FUN_801E791C`
                // formula (the writer of `_DAT_8007726C`): party SPD*1.5 +
                // missing-HP/16 vs enemy SPD + missing-HP/32, two rand draws,
                // Chicken Heart/King accessory bits folded from the living
                // party members' second ability word.
                let actor = session.actor;
                let escaped = self.roll_battle_escape();
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.action_category = 5; // Run band
                }
                self.battle_ctx.active_actor = actor;
                self.battle_ctx.queued_action = 5;
                self.battle_ctx.multi_cast_gate = u8::from(escaped);
                self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
            }
            Some(Resolution::Aborted) => {
                // No valid target the player could pick - arm a default strike
                // on the first living monster so the loop progresses.
                let actor = session.actor;
                let target = (party_count..self.actors.len() as u8)
                    .find(|&i| self.actors[i as usize].battle.liveness != 0)
                    .unwrap_or(party_count);
                self.clear_action_stream(actor);
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                }
                self.seed_basic_attack_queue(actor, target);
                self.battle_ctx.active_actor = actor;
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
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

    /// The target's swing class - retail's monster record `+0x1E`, read
    /// record-direct through the `0x801C9348` pointer table by
    /// `FUN_801EED1C`'s no-input attack arm.
    ///
    /// Always `0` today: the byte is not parsed by
    /// `legaia_asset::monster_archive` and has no
    /// [`crate::monster_catalog::MonsterDef`] field, so there is nothing to
    /// read it from. `0` is the ordinary-target answer, which is what every
    /// non-class-`2` monster gives in retail too.
    fn attack_swing_class_of(&self, _target: u8) -> u8 {
        0
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
    /// bright/dim colour words, the q12 scale), applied here over the
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
            CURSOR_COLOR_BRIGHT, CURSOR_COLOR_DIM, CURSOR_FLAG_DIMMED, CURSOR_FLAG_SELECTED,
            CURSOR_SCALE_ON,
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
                actor.render_scale = 0;
                actor.render_color = CURSOR_COLOR_BRIGHT;
            } else if selected {
                actor.render_scale = CURSOR_SCALE_ON;
                actor.render_flag = CURSOR_FLAG_SELECTED;
                actor.render_color = CURSOR_COLOR_BRIGHT;
            } else {
                actor.render_scale = CURSOR_SCALE_ON;
                actor.render_flag = CURSOR_FLAG_DIMMED;
                actor.render_color = CURSOR_COLOR_DIM;
            }
        }
    }

    /// Drive the open battle Arts submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`crate::battle_arts::BattleArtsInput`] per
    /// frame. On a confirmed execution the art runs via [`Self::apply_battle_art`]
    /// (driving each strike's power byte through the real `apply_art_strike`
    /// path) and the action SM parks at `EndOfAction` so the live loop cycles to
    /// the next combatant. Backing out reopens the command menu.
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
                let (power, enemy_effect, action) = menu
                    .arts
                    .get(art_index as usize)
                    .map(|a| (a.power.clone(), a.enemy_effect, a.action))
                    .unwrap_or_default();
                // A saved-chain row collapses to a single executed art (the
                // row's own matched constant), so its shout list is that one
                // constant - or empty for a synthetic row.
                let actions: Vec<legaia_art::ActionConstant> = action.into_iter().collect();
                self.run_battle_art(
                    caster,
                    &power,
                    enemy_effect,
                    &actions,
                    target_row,
                    target_slot,
                );
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
    /// On a confirmed Begin the entered sequence resolves through the
    /// matcher family ([`Self::resolve_arts_input_entry`]) and runs via
    /// [`Self::apply_battle_art`]; the SM parks at `EndOfAction` so the
    /// live loop cycles. Backing out (empty buffer + Circle, or no valid
    /// target) reopens the command menu.
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
                let (power, enemy_effect, actions) =
                    self.resolve_arts_input_entry(caster, &session.buffer);
                self.run_battle_art(
                    caster,
                    &power,
                    enemy_effect,
                    &actions,
                    target_row,
                    target_slot,
                );
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

    /// Resolve an entered directional buffer to a per-strike power profile
    /// through the retail matcher order: exact **Miracle** string replaces
    /// the whole queue, a recognized art sequence ending on a **Super**
    /// combination replaces the tail, and otherwise each recognized named
    /// art contributes its record's strikes with unmatched directions
    /// staying plain swings
    /// ([`crate::arts_command_input::resolve_entered_commands`]).
    ///
    /// The third element is the turn's **shout list**: one action constant
    /// per art the entry performs, in performed order. Retail's entry runs
    /// until the AP pool is spent, so a plain entry routinely performs
    /// several named arts, and each one is a separately staged animation
    /// whose materialiser calls the cue selector - hence one constant per
    /// art, not one for the whole turn. A Miracle / Super replacement
    /// answers a single constant (its finisher), which is the pinned key
    /// for those two paths; the per-constant staging inside a replacement
    /// queue is not captured, so it is deliberately not expanded here.
    ///
    /// REF: FUN_801EED1C
    /// REF: FUN_8004C140
    fn resolve_arts_input_entry(
        &self,
        caster: u8,
        buffer: &[u8],
    ) -> (
        Vec<legaia_art::PowerByte>,
        legaia_art::EnemyEffect,
        Vec<legaia_art::ActionConstant>,
    ) {
        use crate::battle_arts::{miracle_for_chain, super_for_chain};
        let char_slot = self.party_roster_slot(caster as usize) as u8;
        let character = self.caster_character(char_slot);
        if let Some(miracle) = miracle_for_chain(character, buffer) {
            let (power, effect) = self.miracle_strike_profile(character, miracle);
            let action = legaia_engine_vm::battle_action::resolve_action_queue(
                character,
                miracle.commands,
                &[],
            )
            .actions()
            .iter()
            .rev()
            .copied()
            .find(|a| a.is_art());
            return (power, effect, action.into_iter().collect());
        }
        let caster_records = || {
            self.art_records
                .iter()
                .filter(|((ch, _), _)| *ch == character)
                .map(|(_, rec)| rec)
        };
        if let Some(sa) = super_for_chain(character, buffer, caster_records()) {
            let (power, effect) = self.super_strike_profile(character, sa);
            let action = sa
                .replace
                .iter()
                .rev()
                .filter_map(|&b| legaia_art::ActionConstant::from_byte(b))
                .find(|a| a.is_art());
            return (power, effect, action.into_iter().collect());
        }
        let records: Vec<(legaia_art::ActionConstant, legaia_art::ArtRecord)> = self
            .art_records
            .iter()
            .filter(|((ch, _), _)| *ch == character)
            .map(|((_, action), rec)| (*action, rec.clone()))
            .collect();
        let entry = crate::arts_command_input::resolve_entered_commands(&records, buffer);
        // One shout per recognized art, in performed order - `matched` is
        // exactly that list, and unmatched directions (plain swings) carry
        // no constant, so they stay silent as retail's no-cue-entry arts do.
        (entry.power, entry.enemy_effect, entry.matched)
    }

    /// Run a resolved Tactical-Arts turn against the picked target.
    ///
    /// **This is the seam the Arts path used to bypass.** The entry resolver
    /// hands over a per-strike power profile; the turn is then *executed* by
    /// the battle-action state machine's attack band - the same band a
    /// physical Attack runs - by staging the art's action constant into the
    /// actor's action-parameter stream and arming category `3`
    /// ([`Self::arm_battle_art_action`]). Everything the band owns therefore
    /// applies to an art as well: the face / approach / strike-pace states,
    /// the staged art-bank animation, and - because the anim commit latches
    /// the staged constant into `+0x1DB` - the per-art attack camera, whose
    /// jump tables are keyed on exactly that byte
    /// (`docs/formats/battle-attack-camera-table.md`).
    ///
    /// The pre-SM behaviour (resolve every strike inline, park the SM at
    /// `EndOfAction`) survives as [`Self::apply_battle_art`] and is taken for
    /// an entry that performs **no named art** - a synthetic / demo row has
    /// no action constant to stage, so there is no animation, no camera arm
    /// and nothing for the band to walk.
    pub(in crate::world) fn run_battle_art(
        &mut self,
        caster: u8,
        power: &[legaia_art::PowerByte],
        enemy_effect: legaia_art::EnemyEffect,
        actions: &[legaia_art::ActionConstant],
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) {
        if self.arm_battle_art_action(
            caster,
            power,
            enemy_effect,
            actions,
            target_row,
            target_slot,
        ) {
            // The SM owns the turn from here; the live loop's next step
            // drives the attack band and cycles the turn at its own
            // `EndOfAction`.
            return;
        }
        self.apply_battle_art(
            caster,
            power,
            enemy_effect,
            actions,
            target_row,
            target_slot,
        );
        self.battle_ctx.action_state = vm::battle_action::ActionState::EndOfAction.as_byte();
    }

    /// Stage a Tactical-Arts turn on the acting actor and arm the action SM's
    /// attack band for it. Returns `false` when the entry performs no named
    /// art, in which case the caller falls back to the inline resolver.
    ///
    /// The stream is **one byte per resolved strike**, each carrying the
    /// turn's action constant. That is the port's reading of retail's stream
    /// alphabet - direction swings `0x0C..0x0F`, art starters `0x19`/`0x1A`,
    /// art action constants `0x1B+`, walked one byte per staged swing
    /// (`docs/subsystems/battle-action.md` § Attack chain - strike loop) -
    /// with the per-strike power carried alongside in
    /// [`vm::battle_action::BattleActor::art_power`] rather than re-derived
    /// from a record, because the entry resolver has already folded the
    /// Miracle / Super and no-record degradations the record alone cannot
    /// answer.
    ///
    /// **Disclosed approximation.** A multi-art entry stages every strike
    /// under `actions[0]`. That is not new: the inline resolver keys
    /// `ArtStrikeInfo::art` the same way, because the flat power list the
    /// entry resolver returns carries no per-hit attribution back to the art
    /// that produced it.
    fn arm_battle_art_action(
        &mut self,
        caster: u8,
        power: &[legaia_art::PowerByte],
        enemy_effect: legaia_art::EnemyEffect,
        actions: &[legaia_art::ActionConstant],
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) -> bool {
        use crate::target_picker::CursorRow;
        let Some(art) = actions.first().copied() else {
            return false;
        };
        let party_count = self.party_count.clamp(1, 3);
        let target = match target_row {
            CursorRow::Enemy => party_count + target_slot,
            CursorRow::Ally => target_slot,
        };
        if usize::from(target) >= self.actors.len() || power.is_empty() {
            return false;
        }
        // The **roster**-slot keying `resolve_arts_input_entry` resolved the
        // entry under, not the battle ordinal: the SM looks the art record up
        // by this key, so the two have to agree or a three-member party reads
        // the wrong character's table.
        let char_slot = self.party_roster_slot(caster as usize) as u8;
        let character = self.caster_character(char_slot);
        self.push_art_shout_cues(caster, actions);
        // A freshly-armed action starts from an empty strike script - see
        // `World::clear_action_stream` for the soft-lock a carried-over byte
        // produces. It also drops the previous turn's staged art profile.
        self.clear_action_stream(caster);
        let Some(a) = self.actors.get_mut(caster as usize) else {
            return false;
        };
        // Leave room for the `0x00` terminator the attack band stops on.
        let hits = power.len().min(a.battle.params.len().saturating_sub(1));
        for slot in a.battle.params.iter_mut().take(hits) {
            *slot = art.as_byte();
        }
        a.battle.params[hits] = 0;
        a.battle.strike_index = 0;
        a.battle.active_target = target;
        a.battle.action_category = 3;
        // The art-record lookup key. Nothing else in the port writes it, so
        // an unset slot would resolve every character's arts against Vahn's
        // table.
        a.battle.character = character;
        a.battle
            .stage_art_profile(Some(art), &power[..hits], enemy_effect);
        self.battle_ctx.active_actor = caster;
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
        true
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

    /// Execute an art against the picked target through the real art-power
    /// path, **without** the action state machine - the fallback
    /// [`Self::run_battle_art`] takes for an entry that performs no named
    /// art (a synthetic / demo row).
    ///
    /// Each [`legaia_art::PowerByte`] in `power` drives one strike through
    /// [`crate::art_strike::apply_art_strike`]: the byte's multiplier tier +
    /// UDF/LDF target are decoded, [`Self::resolve_battle_defense`] picks the
    /// matching defense half (when a UDF/LDF split is configured), and the
    /// per-strike damage is deducted. The art's `enemy_effect` is applied once
    /// after a landing hit (if the target survives). Summed damage surfaces as
    /// one HUD popup; the target is downed if its HP reaches zero.
    ///
    /// `power` comes from the matched art record when one is staged, else a
    /// synthetic per-direction profile (see [`Self::build_battle_arts_rows`]),
    /// so the same kernel handles both real and demo arts.
    ///
    /// `actions` is the list of **named arts this turn performs**, in
    /// performed order - one entry per recognized art in a per-press entry,
    /// a single finisher constant for a Miracle / Super replacement, and
    /// empty for a synthetic art with no matched record. It is not a
    /// display list: it drives one shout cue and one learn-on-use check per
    /// art, both of which retail runs per art rather than per turn.
    fn apply_battle_art(
        &mut self,
        caster: u8,
        power: &[legaia_art::PowerByte],
        enemy_effect: legaia_art::EnemyEffect,
        actions: &[legaia_art::ActionConstant],
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) {
        use crate::target_picker::CursorRow;
        use legaia_engine_vm::battle_action::ArtStrikeInfo;
        let party_count = self.party_count.clamp(1, 3);
        let target = match target_row {
            CursorRow::Enemy => party_count + target_slot,
            CursorRow::Ally => target_slot,
        } as usize;
        if target >= self.actors.len() {
            return;
        }
        let attack = self
            .battle_attack
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        let character = self.caster_character(caster);
        // One shout per art the turn performs (the SM-routed path pushes the
        // same list at arm time).
        self.push_art_shout_cues(caster, actions);
        let roster = self.party_roster_slot(caster as usize) as u8;
        for action in actions {
            // Learn-on-use, likewise per art. This path reaches
            // `art_strike::apply_art_strike` directly rather than through
            // `BattleActionHost::apply_art_strike`, so retail's per-art check
            // (`FUN_801EFBFC`, wired in the host impl and run once per
            // accepted art in the queue-builder walk) has to be run here too.
            // A synthetic art contributes no constant and is skipped - there
            // is no real art id to insert.
            self.notify_art_used(roster, action.as_byte());
        }
        let action = actions.first().copied();
        // Selector-9 accuracy/evasion terms (retail actor `+0x168`): the
        // attacker's accuracy vs the target's evasion. The roll engages only
        // when the ATTACKER has a seeded accuracy stat; an unseeded attacker
        // (`acc == 0`, the synthetic case) auto-hits AND consumes no RNG, so it
        // can't be made to whiff against a positive-evasion target and battles
        // without seeded stats keep their bit-identical streams.
        let attacker_acc = self
            .battle_accuracy
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        let target_eva = self.battle_evasion.get(target).copied().unwrap_or(0);
        let mut total: u32 = 0;
        let mut landed: u8 = 0;
        for (i, pb) in power.iter().enumerate() {
            if self.actors[target].battle.liveness == 0 {
                break;
            }
            // Minimal per-strike info: `apply_art_strike` + `resolve_battle_defense`
            // only read `power` + `enemy_effect`. `art` carries the turn's
            // first performed art when one exists; the placeholder only
            // remains for synthetic entries, and the live loop doesn't drive
            // the per-art animation script either way, so a multi-art entry's
            // later strikes are not re-keyed (nothing downstream reads it).
            let info = ArtStrikeInfo {
                strike_index: i as u8,
                anim_byte: 0,
                actor_slot: caster,
                target_slot: target as u8,
                character,
                art: action.unwrap_or(legaia_art::ActionConstant::Art1B),
                power: Some(*pb),
                dmg_timing: None,
                enemy_effect,
                hit_cue: None,
            };
            let defense = self.resolve_battle_defense(target as u8, &info);
            let outcome = crate::art_strike::apply_art_strike(attack, defense, &info);
            if let Some(dmg) = outcome.damage {
                // Roll the strike against the target's evasion. Only consume
                // RNG when the roll is meaningful (some stat seeded), so the
                // unseeded auto-hit path leaves the RNG stream untouched.
                let hit = if attacker_acc == 0 {
                    true
                } else {
                    let mut seed = self.next_rng();
                    legaia_engine_vm::battle_formulas::accuracy_roll(
                        attacker_acc,
                        target_eva,
                        &mut seed,
                    )
                };
                if hit {
                    // A petrified target (Stone) absorbs the hit - no HP loss
                    // (Stone is invulnerable at every damage entry point). The
                    // strike still counts as landed (it connected, then was
                    // nullified), matching the basic-attack / spell paths.
                    let applied = if self.actor_is_petrified(target as u8) {
                        0
                    } else {
                        dmg
                    };
                    self.apply_battle_hp_delta(target, i32::from(applied));
                    total = total.saturating_add(applied as u32);
                    landed = landed.saturating_add(1);
                }
            }
        }
        if landed > 0
            && enemy_effect != legaia_art::EnemyEffect::None
            && self.actors[target].battle.liveness != 0
        {
            let applied = self
                .status_effects
                .apply_from_enemy_effect(target as u8, enemy_effect);
            // Rot's applier rolls the disabled limb (`rand % 3`, the retail
            // `1 << (rand%3 + 3)` bit pick).
            if applied == Some(legaia_engine_vm::status_effects::StatusKind::Rot) {
                let limb = (self.next_rng() % 3) as u8;
                self.status_effects.set_rot_limb(target as u8, limb);
            }
        }
        if total > 0 {
            self.battle_hit_fx.push(BattleHitFx {
                target_slot: target as u8,
                amount: total.min(u16::MAX as u32) as u16,
                is_heal: false,
                is_crit: landed > 1,
            });
            let survives = self.actors[target].battle.hp > 0;
            self.queue_battle_reaction(target, survives);
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
    /// frame. On a confirmed cast the spell applies via [`Self::apply_battle_spell`]
    /// (MP deducted, HP / heal / cure / revive folded, popups surfaced) and the
    /// action SM parks at `EndOfAction` so the live loop cycles to the next
    /// combatant - a cast is the caster's whole turn, no strike fires. Backing
    /// out reopens the command menu for the same actor.
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
                let caster = menu.actor;
                self.apply_battle_spell(caster, spell_id, target_row, target_slot);
                if self.battle_escaped {
                    // Escape spell succeeded: leave the encounter now (no loot,
                    // no game-over) instead of cycling the turn.
                    self.finish_battle();
                } else {
                    self.battle_ctx.action_state =
                        vm::battle_action::ActionState::EndOfAction.as_byte();
                }
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

    /// Cast `spell_id` from `caster` against the picked target and fold the
    /// outcome into world state. MP is deducted once up-front; the spell's
    /// [`crate::spells::SpellTarget`] shape decides which slots are affected
    /// (single → the picked slot; `AllEnemies` / `AllAllies` → the whole band),
    /// each resolved through [`crate::spells::cast_spell`]. Caster magic comes
    /// from [`Self::battle_magic`]; target magic-defense reuses
    /// [`Self::battle_defense`]. Damage / heal / cure / revive / buff / capture
    /// / escape all fold through [`Self::fold_spell_outcome`].
    fn apply_battle_spell(
        &mut self,
        caster: u8,
        spell_id: u8,
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) {
        use crate::spells::SpellTarget;
        use crate::target_picker::CursorRow;

        let Some(def) = self.spell_catalog.get(spell_id).cloned() else {
            return;
        };
        let party_count = self.party_count.clamp(1, 3);
        let targets: Vec<u8> = match def.target {
            SpellTarget::OneEnemy | SpellTarget::OneAlly | SpellTarget::SelfOnly => {
                let abs = match target_row {
                    CursorRow::Enemy => party_count + target_slot,
                    CursorRow::Ally => target_slot,
                };
                vec![abs]
            }
            SpellTarget::AllEnemies => (party_count..self.actors.len() as u8).collect(),
            SpellTarget::AllAllies => (0..party_count).collect(),
        };
        self.cast_spell_on_slots(caster, &def, &targets);
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
            (
                menu.targets
                    .iter()
                    .map(|t| crate::inventory_use::BattleItemTargetRow {
                        name: t.name.clone(),
                        hp: t.hp,
                        hp_max: t.hp_max,
                        alive: t.alive,
                    })
                    .collect(),
                view.target_cursor,
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

        let ev = if self.input.just_pressed(PadButton::Up) {
            Some(InventoryUseInput::Up)
        } else if self.input.just_pressed(PadButton::Down) {
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
            if let Some(item_id) = item_before {
                // Apply to every affected slot, but consume only one copy.
                for &target_slot in &used_slots {
                    let outcome = self.apply_battle_item(item_id, target_slot);
                    self.push_item_use_fx(target_slot, outcome);
                }
                self.consume_item(item_id);
            }
            if self.battle_escaped {
                // Escape item succeeded: leave the encounter now (no loot, no
                // game-over) instead of cycling the turn.
                self.finish_battle();
            } else {
                // Using an item is the actor's whole turn: park at EndOfAction
                // so the live loop's re-arm block cycles to the next combatant.
                self.battle_ctx.action_state =
                    vm::battle_action::ActionState::EndOfAction.as_byte();
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

//! The command ring: opening the prompt, the per-frame command tick, the basic-
//! attack queue seed, swing classification, the target-cursor tint and the
//! commit clip stamp.

use super::*;

impl World {
    /// Open the player-driven command menu for party member `actor` and park
    /// the action SM. The action context's `active_actor` is set now; the
    /// queued action / target is filled in by [`Self::tick_battle_command`]
    /// once the player confirms. No-op unless [`crate::world::BattleState::player_driven`].
    pub(in crate::world) fn open_battle_command(&mut self, actor: u8) {
        use legaia_engine_battle_menus::battle_flow::BattleFlowState as Flow;
        use legaia_engine_battle_menus::battle_input::BattleCommandSession;
        if !self.battle.player_driven {
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
        let round_open = matches!(self.battle.flow, Flow::Idle | Flow::TurnPrompt);
        // A sub-screen backed out of onto the ring brings a launched log back
        // (retail's mode-1 launch, step `0x08` for the magic window). A commit
        // clears the launch before it reopens the ring, so the next member's
        // ring finds none here.
        if self.battle.commit_log_launch.is_some() {
            self.launch_commit_log(true);
        }
        if round_open {
            self.start_round_prompt_glide();
        }
        self.battle.command = Some(if round_open {
            BattleCommandSession::new_round_open(actor, actor, self.battle.no_escape)
        } else {
            BattleCommandSession::new(actor, actor)
        });
        if self.battle.flow == Flow::Idle {
            self.set_battle_flow(Flow::TurnPrompt);
        }
    }

    /// Drive the open command session one frame from [`World::input`]. When the
    /// session resolves, arm the action SM with the chosen command + target
    /// (v0.1: a physical Attack) and clear the session so the SM resumes.
    /// On an abort (no valid target) it falls back to the first living monster
    /// so the loop never deadlocks.
    pub(in crate::world) fn tick_battle_command(&mut self) {
        use crate::battle_round::PendingPartyAction;
        use legaia_engine_battle::target_picker::CursorRow;
        use legaia_engine_battle_menus::battle_input::{
            BattleCommandInput, CommandPhase, Resolution,
        };

        let Some(mut session) = self.battle.command.take() else {
            return;
        };

        let party_count = self.party.party_count.clamp(1, 3);
        // Target-row selectability: the per-slot validity byte the retail
        // validator (`FUN_8003FB10` arm `0x05`) writes, not an inline liveness
        // test - see `super::super::validator_host`.
        let (party, monsters) = self.battle_target_rows();

        // The pad word of a dispatched pass: a press parked while the
        // prompt's glide was landing stands in for a tick with none.
        let mut ev: BattleCommandInput = self.take_command_pad_buffer();
        // The cross-button entry walks a highlight ahead of the handlers and
        // keys `FUN_8004FCC8(0x22)` when it moves (`0x801D082C` / `0x801D08D0`
        // / `0x801D0988` / `0x801D0A40`).
        if self.toggles.battle_command
            == legaia_engine_system::option_values::BattleCommandOpt::CrossButton
            && session.walk_highlight(&mut ev)
        {
            self.push_battle_ui_cue(super::ui_cues::BattleUiCue::Cursor);
        }
        // The ring's two status refusals (`ring_arm_refused`): a press on
        // a refused arm is dropped with the buzz cue, and the ring stays up.
        if let CommandPhase::Menu { .. } = session.phase {
            use legaia_engine_battle_menus::battle_input::BattleCommand;
            let status = self.raw_status_word(session.actor);
            let cursor_arm = session.menu_command();
            let mut refused = false;
            for (pressed, arm) in [
                (&mut ev.left, Some(BattleCommand::Attack)),
                (&mut ev.right, Some(BattleCommand::Magic)),
                (&mut ev.cross, cursor_arm),
            ] {
                if *pressed && arm.is_some_and(|a| ring_arm_refused(status, a)) {
                    *pressed = false;
                    refused = true;
                }
            }
            if refused {
                // `FUN_8004FCC8(0x23)` through the shared tail `0x801D2968`.
                self.push_battle_ui_cue(super::ui_cues::BattleUiCue::Back);
            }
        }
        let was_round_prompt = matches!(session.phase, CommandPhase::RoundPrompt { .. });
        let phase_before = session.phase.clone();
        session.input(ev, party, monsters);
        // The surface's blip (`super::ui_cues`). A Ra-Seru arm the special
        // word forbids is refused below, and retail answers that refusal
        // with the cancel cue through `0x801D2968` (`0x801D1454`), not the
        // commit.
        let refused_magic =
            matches!(session.phase, CommandPhase::OpenSpellMenu) && self.battle.raseru_forbidden();
        let cue = if refused_magic {
            Some(super::ui_cues::BattleUiCue::Back)
        } else {
            super::ui_cues::command_phase_cue(&phase_before, &session.phase)
        };
        if let Some(cue) = cue {
            self.push_battle_ui_cue(cue);
        }
        // A target cursor this press opened starts on the member's current
        // target, as retail's does ([`World::seat_attack_cursor`]).
        if !matches!(phase_before, CommandPhase::Targeting { .. })
            && let CommandPhase::Targeting { picker, .. } = &mut session.phase
        {
            self.seat_attack_cursor(session.actor, picker);
        }
        // Leaving the ring for a sub-screen launches the commit log off the
        // left edge; cancelling back onto it slides the log home
        // (`FUN_801D388C`'s launch tail, `super::super::commit_log_launch`).
        self.launch_commit_log_for(&phase_before, &session.phase);
        // ...and the attack-mode pick is the per-fighter Auto flag
        // (`ctx[+0x266 + seat]`, `super::super::auto_combo`).
        self.note_auto_attack_pick(&phase_before, &session.phase);
        // `Begin` with nobody able to act: retail's round-prompt confirm arm
        // finds `FUN_801DBA04` equal to the party count and stores the commit
        // confirm `0x6E` directly (`0x801D10A0`, step `0x27`) instead of
        // opening a ring nobody can use.
        if was_round_prompt
            && matches!(session.phase, CommandPhase::Menu { .. })
            && self.next_member_owing_command(None).is_none()
        {
            session.phase = CommandPhase::CommitConfirm { cursor: 0 };
        }
        // `Begin` onto the ring is sub-draw step 1 (`li a0,0x1` /
        // `jal 0x801D388C` at `0x801D109C`): its records spawn at seat A
        // and glide in, and the ring reads no pad until they land.
        // REF: FUN_801D0748 (state 0x1E confirm arm)
        if was_round_prompt && matches!(session.phase, CommandPhase::Menu { .. }) {
            self.start_ring_open_glide();
        }
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
            // The `90` hook reads seat 0's Auto flag `ctx[+0x266]`, which
            // the pick just made writes (`note_auto_attack_pick` above).
            if let Some(tut) = self.battle.tutorial.as_mut() {
                tut.inputs.auto_attack = self.battle.auto_combo.pending;
            }
            // An unresolved surface's hook can rewind too: the target cursor
            // (`90`) refuses an Attack outside the attack lessons and an
            // Auto attack in the hyper-arts drill, and the attack-mode
            // prompt (`120`) refuses Attack in the Items / Spirit lessons.
            if self.sync_battle_flow(Some(&session.phase)) {
                self.open_battle_command(session.actor);
                return;
            }
        }
        if self.battle.tutorial.is_some()
            && let Some(res) = resolution
        {
            use legaia_engine_battle_menus::battle_flow::BattleFlowState as Flow;
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
                Resolution::Aborted
                | Resolution::StepBack
                | Resolution::BeginRound
                | Resolution::Reselect => false,
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
                // list stays reachable behind `WorldToggles::arts_saved_list`,
                // which both hosts can raise.
                self.battle_ctx.active_actor = session.actor;
                if self.toggles.arts_saved_list {
                    let rows = self.build_battle_arts_rows(session.actor);
                    self.battle.arts_menu =
                        Some(legaia_engine_battle::battle_arts::BattleArtsSession::new(
                            session.actor,
                            session.actor,
                            rows,
                        ));
                } else {
                    self.open_arts_command_input(session.actor);
                }
                // The flow byte follows the player into the window: retail's
                // arts entry stores `ctx[+0x06] = 0x50` as it opens (`sb` at
                // `0x801D1738`, the delay slot of `jal FUN_801DA34C`), and the
                // ring's item / magic arms store `0x3C` / `0x46` the same way
                // (the `0x28` row of `battle.md`'s flow table). Without it the byte reads the surface the player
                // left (`0x78` / `0x28`) for as long as the window is up.
                // REF: FUN_801D0748 (states 0x28 / 0x78 into 0x3C / 0x46 / 0x50)
                self.sync_battle_flow(None);
            }
            Some(Resolution::OpenSpellMenu) if self.battle.raseru_forbidden() => {
                // The special word forbids the Ra-Seru chip: retail's arm
                // returns without committing (`0x801D1448..0x801D1454`), so
                // the ring stays up.
                self.open_battle_command(session.actor);
            }
            Some(Resolution::OpenSpellMenu) => {
                // Player picked Magic: hand off to the spell submenu (same
                // pattern as Item). `tick_battle_spell_menu` drives until the
                // player casts (turn cycles via EndOfAction) or backs out.
                self.battle_ctx.active_actor = session.actor;
                self.stamp_commit_clip(session.actor, CAST_COMMIT_CLIP);
                match self.build_battle_spell_session(session.actor) {
                    Some(menu) => {
                        self.battle.spell_menu = Some(menu);
                        self.sync_battle_flow(None);
                    }
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
                self.stamp_commit_clip(session.actor, CAST_COMMIT_CLIP);
                self.battle.item_menu = Some(self.build_battle_item_session());
                self.sync_battle_flow(None);
            }
            Some(Resolution::SpiritGuard) => {
                // Player picked Spirit: the guard stance (retail's pending
                // category `+0x1DE = 4`, the melee kernel's tripled guard
                // roll) is up from the commit - it protects against every
                // monster that dispatches ahead of this member - and lasts
                // until the next round's sweep clears the category. The AP
                // charge is the Spirit band's own, at dispatch.
                let actor = session.actor;
                // Retail's Spirit commit stamps category `4` and the spirit
                // clip `+0x1E7 = 0x10` (`0x801D168C..0x801D16B0`, the store
                // in the delay slot of `jal FUN_801DB81C`) - the clip the
                // action SM's spirit band stages at `0x46`.
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.action_category = 4;
                }
                self.stamp_commit_clip(actor, SPIRIT_COMMIT_CLIP);
                if let Some(guard) = self.battle.guarding.get_mut(actor as usize) {
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
            Some(Resolution::StepBack) => self.step_back_battle_command(session.actor),
            // The commit-confirm screen (`0x6E`): Begin plays the round out,
            // Reselect walks the member cursor back to the last member's ring.
            Some(Resolution::BeginRound) => self.begin_round_execution(),
            Some(Resolution::Reselect) => self.reselect_battle_commands(session.actor),
            None => {
                // Still selecting - keep the session open for the next frame.
                self.battle.command = Some(session);
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
    /// pass to project from ([`crate::world::CastFxState::move_fx_streak`], whose `action` key is
    /// this stream's first byte).
    ///
    /// The bytes come from [`vm::battle_action::basic_attack_queue`], the port
    /// of the one queue arm retail builds **without** running the Arts command
    /// gauge (`FUN_801EED1C`'s no-directional-input arm): two independently
    /// rolled Left/Right arm swings against an ordinary target, one low swing
    /// against a [`vm::battle_action::LOW_SWING_TARGET_CLASS`] target. The
    /// engine's Attack command is exactly that situation - it resolves a
    /// target with no direction input - so it is the retail kernel that
    /// applies. It is also the **second** of two seeds, not the only one:
    /// retail's command SM first preseeds the same window from the
    /// character's own saved auto command string (`FUN_801DA34C` /
    /// [`vm::battle_action::preseed_action_queue`], over record-relative
    /// `+0x1A7` / `+0x1B7` chosen on `u16[+0x156] < u16[+0x154]`), and this
    /// roll is what a character with no such string falls through to.
    /// [`World::preseed_auto_command_string`] runs that read at dispatch and
    /// this arm runs only when it comes back empty.
    ///
    /// Not to be confused with `World::party.saved_chains`, the engine's
    /// **named** chain library (LGSF v2, edited by `tactical_arts_editor`,
    /// read by the battle arts path) - a different list with a different
    /// lifetime.
    ///
    /// **Disclosed stand-in.** Retail picks between the two shapes on the
    /// target monster record's `+0x1E` byte, which
    /// `legaia_actor_formats::monster_archive` does not parse and `MonsterDef` does
    /// not carry, so [`Self::attack_swing_class_of`] answers `0` and the
    /// live path always takes the two-arm-swing form. That is retail's own
    /// behaviour for every non-class-`2` target; only the low-swing collapse
    /// is unreachable until the byte is parsed.
    ///
    /// REF: FUN_801EED1C
    pub(in crate::world) fn seed_basic_attack_queue(&mut self, actor: u8, target: u8) -> usize {
        let swing_class = self.attack_swing_class_of(target);
        // `FUN_801EED1C`'s head clears `0x801F696C` (`0x801EED88`); the
        // no-directional-input arm sets nothing.
        self.battle_ctx.super_trigger = false;
        let mut queue = [0u8; legaia_engine_battle_vm::battle_action::ACTION_QUEUE_CAP];
        let written = legaia_engine_battle_vm::battle_action::basic_attack_queue(
            &mut queue,
            swing_class,
            &mut || self.next_rand(),
        );
        let character = self.caster_character(self.party_roster_slot(actor as usize) as u8);
        if let Some(a) = self.actors.get_mut(actor as usize) {
            // Keep the seat's character key truthful on the plain-Attack path
            // too: the arts arming is otherwise its only writer, so every
            // party seat that only ever attacked read as Vahn.
            a.battle.character = character;
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
        let Some(record) = self.party.roster.members.get(roster_slot as usize) else {
            return false;
        };
        // Retail's table `0x8007BD10` holds a **1-based** char id; the
        // engine's `active_party` mirror holds the 0-based roster slot.
        legaia_engine_battle_vm::battle_action::miracle_marker_armed(
            roster_slot.wrapping_add(1),
            &record.equipment().slots,
        )
    }

    /// The target's swing class - retail's monster record `+0x1E`, read
    /// record-direct through the `0x801C9348` pointer table by
    /// `FUN_801EED1C`'s no-input attack arm.
    ///
    /// Resolved through the slot's seated monster id and the catalog's
    /// [`legaia_engine_battle::monster_catalog::MonsterDef::swing_class`] (record `+0x1E`).
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
        self.tables
            .monster_catalog
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
    /// Seat a just-opened attack-target cursor on `actor`'s current target
    /// (`+0x1DD`, the actor's `battle.active_target`) when that is a
    /// standing monster.
    ///
    /// Retail never resets the byte when the cursor opens: the ring's
    /// Attack confirm keeps any monster-slot target (`+0x1DD - 3 < 5`) and
    /// re-picks anything else through `FUN_801DB8B4`
    /// (`0x801D1574..0x801D15AC`), and the round boundary re-picks only a
    /// dead or past-slot-6 one (`FUN_801D88CC`); the ring builder
    /// `FUN_801D8A88` then takes that byte as the cursor's start. A member
    /// who struck a monster last round therefore opens on it again while it
    /// stands, and a bare confirm strikes it. Anything else leaves the
    /// picker's first-live-slot opening, which is `FUN_801DB8B4`'s pick.
    ///
    /// REF: FUN_801D0748 (0x801D1574..0x801D15AC), FUN_801D8A88
    pub(in crate::world) fn seat_attack_cursor(
        &self,
        actor: u8,
        picker: &mut legaia_engine_battle::target_picker::TargetPickerSession,
    ) {
        use legaia_engine_battle::target_picker::{CursorRow, TargetKind};
        if picker.kind() != TargetKind::SingleEnemy {
            return;
        }
        let n = self.party.party_count.clamp(1, 3);
        let Some(t) = self
            .actors
            .get(usize::from(actor))
            .map(|a| a.battle.active_target)
        else {
            return;
        };
        if t >= n && t < n + 5 {
            picker.seat_cursor(CursorRow::Enemy, t - n);
        }
    }

    fn apply_target_cursor_tint(
        &mut self,
        session: &legaia_engine_battle_menus::battle_input::BattleCommandSession,
    ) {
        use legaia_engine_battle::target_picker::{CursorRow, PickerState};
        use legaia_engine_battle_vm::battle_action::{
            CURSOR_BLEND_ON, CURSOR_COLOR_BRIGHT, CURSOR_COLOR_DIM, CURSOR_FLAG_DIMMED,
            CURSOR_FLAG_SELECTED,
        };
        let party_count = self.party.party_count.clamp(1, 3);
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

    /// Write the clip a ring commit stages behind the action
    /// (`actor[+0x1E7]`): [`CAST_COMMIT_CLIP`] for Item / Magic,
    /// [`SPIRIT_COMMIT_CLIP`] for Spirit.
    ///
    /// PORT: FUN_801D0748 (`sb v0,0x1e7(v1)` at `0x801D13E8` / `0x801D14C0` / `0x801D16B0`)
    pub(in crate::world) fn stamp_commit_clip(&mut self, actor: u8, clip: u8) {
        if let Some(a) = self.actors.get_mut(actor as usize) {
            a.battle.queued_anim_b = clip;
        }
    }
}

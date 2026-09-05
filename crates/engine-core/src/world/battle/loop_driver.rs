//! The per-frame live battle loop, basic-attack strike, target resolution, and
//! status-block / defeat predicates (incl. the Final Heal revive sweep). Split
//! out of `battle.rs` as additional `impl World` blocks; no logic change from
//! the original inline definitions.

use super::*;

impl World {
    /// Apply a signed HP change to a battle slot **through the retail HP-bar
    /// machinery**: live HP moves at once, the displayed bar is left owing the
    /// difference, and the per-frame ramp
    /// ([`vm::battle_action::tick_hp_bars`], retail `FUN_80047430`) walks it
    /// down a quarter at a time.
    ///
    /// `delta` is positive for damage. The clamp against max HP on the heal
    /// side and the `hp == 0 -> liveness = 0` edge are the engine's existing
    /// per-site behaviour, folded here so every damage entry point seeds the
    /// accumulator the same way. Returns the amount live HP actually moved by
    /// (positive = HP lost), which is also what gets seeded - a hit that
    /// saturates at zero owes the bar only the distance it really travelled.
    ///
    /// The bar is *armed* on the first change ([`BattleActor::arm_hp_bar`]).
    /// Retail seeds `+0x172` at battle load instead; arming here is equivalent
    /// because there is no desync to inherit before the first write, and it
    /// keeps a host that never damages anyone in the "bars not animated" state
    /// the port started from.
    ///
    /// REF: FUN_801EC3E4 (the accumulating seed this uses)
    pub(in crate::world) fn apply_battle_hp_delta(&mut self, slot: usize, delta: i32) -> i32 {
        let Some(a) = self.actors.get_mut(slot) else {
            return 0;
        };
        a.battle.arm_hp_bar();
        let before = a.battle.hp;
        a.battle.hp = if delta >= 0 {
            before.saturating_sub(delta.min(i32::from(u16::MAX)) as u16)
        } else {
            before
                .saturating_add((-delta).min(i32::from(u16::MAX)) as u16)
                .min(a.battle.max_hp)
        };
        let moved = i32::from(before) - i32::from(a.battle.hp);
        a.battle.accumulate_hp_bar(moved);
        // `hp == 0 -> liveness = 0` holds for **present** actors only - the
        // same `max_hp > 0` guard the per-tick dead-marking sweep in
        // [`Self::step_battle_frame`] applies. A seated-but-unrolled slot
        // (`max_hp == 0`, the hollow party shape the seated-vs-dead fix
        // documents) taking a zero-damage hit is not a death; retail cannot
        // even represent the state (battle load always stats a seated slot),
        // so the two port sites must at least agree with each other.
        if a.battle.max_hp > 0 && a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
        moved
    }

    /// One frame of HP-bar ramp across every battle slot.
    ///
    /// The retail split is by slot index, not by side: slots `0..=2` drain a
    /// quarter of the outstanding delta per frame, everything else settles in
    /// one frame (`FUN_80047430`'s `sltiu v0,s1,0x3` at `0x800474F4`). The
    /// engine seats the party in the same low slots, so the same test holds.
    ///
    /// REF: FUN_80047430 (kernel + `// PORT:` tags in
    /// `legaia_engine_vm::battle_hp_bar`)
    pub(in crate::world) fn tick_battle_hp_bars(&mut self) {
        for (slot, a) in self.actors.iter_mut().enumerate() {
            a.battle.tick_hp_bar(slot as u8);
        }
    }

    /// Rebuild the four cast-census bytes on the battle context - the head of
    /// retail's per-frame cast tick.
    ///
    /// Before this ran, `ctx[+0x249]` / `ctx[+0x24D]` / `ctx[+0x24A]` /
    /// `ctx[+0x24B]` were modelled on [`vm::battle_action::BattleActionCtx`]
    /// and read by the magic band, but nothing outside tests ever wrote them -
    /// the same inert-gate shape the HP-bar settle check had.
    ///
    /// REF: FUN_801E09F8 (census head; kernel + `// PORT:` tag in
    /// `legaia_engine_vm::battle_cast_census`)
    pub(in crate::world) fn tick_battle_cast_census(&mut self) {
        let ctx_ptr: *mut BattleActionCtx = &mut self.battle_ctx;
        let host = BattleHostImpl { world: self };
        // SAFETY: same argument as `step_battle` - `BattleHostImpl` never
        // reaches `world.battle_ctx` through its borrow, and the census reads
        // only the actor table.
        let ctx = unsafe { &mut *ctx_ptr };
        vm::battle_action::tick_cast_census(&host, ctx);
    }

    /// Per-frame battle-side driver for the live gameplay loop. Gated by
    /// [`Self::live_gameplay_loop`] in [`Self::tick`].
    ///
    /// Wraps [`Self::step_battle`] with the host-side glue retail performs
    /// through the render + animation systems, so the battle resolves from
    /// `tick` alone:
    ///
    /// - **Damage application.** Drains this step's [`BattleEvent`]s and
    ///   folds [`BattleEvent::ApplyArtStrike`] damage into target HP. A
    ///   generic physical attack (no art) is applied on the
    ///   `AttackChain -> AttackRecovery` edge via [`Self::apply_basic_attack`].
    /// - **Liveness.** Any combatant whose HP hit zero is marked dead so the
    ///   SM's wipe scan sees it.
    /// - **Turn cycling.** When the SM idles at `EndOfAction` with monsters
    ///   still alive, the next party member is re-armed (v0.1 keeps monsters
    ///   passive - party turns only).
    /// - **Recovery edge.** Clears `ADVANCE_DONE` at `AttackRecovery`, the
    ///   edge the retail recovery animation drives.
    ///
    /// On [`StepOutcome::BattleComplete`] it runs [`Self::finish_battle`] to
    /// apply loot and return to the field.
    /// The Lost Grail "Final Heal" auto-revive sweep.
    ///
    /// PORT: FUN_801e6968 (battle overlay 0898;
    /// `ghidra/scripts/funcs/overlay_battle_action_801e6968.txt`) - the
    /// action-cleanup helper state `0x50` of `FUN_801E295C` calls before its
    /// liveness count. For each party member in scope that is **down** (live
    /// HP `+0x14C` == 0) and carries ability bit `0x27` - *Final Heal*, the
    /// Lost Grail passive, record `+0xF8 & 0x80` (bit 39 = word 1 bit 7 of
    /// the `+0xF4` bitfield) - retail:
    ///
    /// - revives at **full max HP** via `FUN_800402F4(4, 1, slot)` (the
    ///   item-effect apply handler's revive class with the non-zero tier:
    ///   `uVar13 = max_hp`, statuses cleared - `800402f4.txt` case 4);
    /// - **consumes one equipped Lost Grail** (item id `0xE7`): zeroes the
    ///   first accessory slot (record `+0x19B..+0x19D`, equipment array
    ///   indices 5..8) holding `0xE7` and clears the ability bit;
    /// - re-sets the bit when another Lost Grail is still equipped (the
    ///   second slot scan).
    ///
    /// Retail dispatches on the acting summon's target byte (`+0x1DD` `< 3`
    /// = the single party target, `== 8` = sweep all party slots); the
    /// engine sweeps the whole party after each step - equivalent, since a
    /// member without the bit stays down and a member with it is revived by
    /// the first sweep after death. Item id `0xE7` = "Lost Grail"
    /// (disc-decoded `SCUS_942.54` item table); passive `0x27` mapping per
    /// `docs/formats/accessory-passive-table.md`. The dump's tail (first
    /// monster slot dead + `DAT_8007BD0C == 0xB5` boss-transition arm,
    /// `0x801E6CE4..0x801E6D64`) is the second battle stage-id writer - the
    /// Cort form transition to stage 3 / entry 969 - ported separately as
    /// [`crate::overlay_loader::boss_transition_stage_id`], resolved live by
    /// [`World::battle_stage_id`].
    ///
    /// REF: FUN_800402F4 (the revive arm this calls - case 4, tier 1 = full
    /// max HP + status clear)
    pub(in crate::world) fn apply_final_heal_revives(&mut self) {
        const LOST_GRAIL: u8 = 0xE7;
        const FINAL_HEAL_WORD1_BIT: u32 = 0x80; // ability bit 0x27 (39)
        let pc = (self.party_count.min(3) as usize).min(self.actors.len());
        for slot in 0..pc {
            let (max_hp, down) = {
                let a = &self.actors[slot].battle;
                (a.max_hp, a.max_hp > 0 && a.hp == 0)
            };
            if !down {
                continue;
            }
            // The Lost Grail + ability bit live on the occupying character's
            // record; the revive itself targets the battle ordinal's mirrors.
            let char_slot = self.party_roster_slot(slot);
            let Some(record) = self.roster.members.get_mut(char_slot) else {
                continue;
            };
            let mut bits = record.ability_bits();
            let word1 = u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]]);
            if word1 & FINAL_HEAL_WORD1_BIT == 0 {
                continue;
            }
            // Consume the first equipped Lost Grail (accessory slots 5..8).
            let mut eq = record.equipment();
            if let Some(i) = (5..8).find(|&i| eq.slots[i] == LOST_GRAIL) {
                eq.slots[i] = 0;
                record.set_equipment(eq);
            }
            // Clear the bit; re-set it when another Lost Grail remains.
            let still_equipped = (5..8).any(|i| eq.slots[i] == LOST_GRAIL);
            let word1 = if still_equipped {
                word1
            } else {
                word1 & !FINAL_HEAL_WORD1_BIT
            };
            bits[4..8].copy_from_slice(&word1.to_le_bytes());
            record.set_ability_bits(bits);
            // Full revive (FUN_800402F4 class 4, tier 1): max HP + statuses
            // cleared; liveness restored so the SM's scans see them alive.
            self.status_effects.cure_all(slot as u8);
            let a = &mut self.actors[slot].battle;
            let before = a.hp;
            a.hp = max_hp;
            a.liveness = 1;
            // ...including that routine's readout seed. A revive that writes
            // live HP alone leaves `hp != hp_display` with a zero accumulator,
            // and the ramp's `+0x10 != 0` guard makes that pair absorbing - the
            // `0x51` gate would then park the fight on the member Final Heal
            // just saved.
            let delta = i32::from(a.hp) - i32::from(before);
            if delta != 0 {
                a.assign_hp_bar(delta.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
            }
            self.battle_hit_fx.push(BattleHitFx {
                target_slot: slot as u8,
                amount: max_hp,
                is_heal: true,
                is_crit: false,
            });
        }
    }

    pub(in crate::world) fn live_battle_tick(&mut self) -> Option<StepOutcome> {
        use vm::battle_action::{ActionState, ActorFlags};

        // The modelled CD drive: one clip read span elapses per frame.
        self.battle_xa_busy_frames = self.battle_xa_busy_frames.saturating_sub(1);

        // The battle has ended and its presentation owns the frame: retail's
        // battle tick runs the results sequencer instead of the action SM
        // while `DAT_8007BD71 == 0xFE` (`FUN_80046A20` `0x80047040` /
        // `0x800470D0`), and nothing else - no round prompt, no turn cycling,
        // no menu - until the exit gate fires.
        // REF: FUN_80046A20
        if self.battle_victory.is_some() {
            self.tick_battle_end_sequence();
            return None;
        }

        // Everything already in the battle-event queue belongs to an earlier
        // tick and has been folded once; only this tick's tail may be folded
        // below. See the fold site for what re-folding costs.
        let events_before = self.pending_battle_events.len();

        // Round-open prompt: the flow byte sitting at `TurnPrompt` is the
        // port's `ctx[+0x06] == 0x1E`, and retail reaches it once per round
        // (state `0x14` sets it unconditionally, and the action SM's
        // round-end at `801e67e8` parks the flow back at `0x14`). Swap the
        // freshly opened command session onto its `Begin | Run` phase here so
        // both entry points - the battle's opening turn and every later round
        // boundary - raise it, and a mid-round reopen does not.
        //
        // Ahead of the message-box park below, not after it: retail's own
        // `0x14 -> 0x1E` write happens whether or not a box is up, and the
        // sparring tutorial's very first box (`Select [Begin]`) is queued by
        // the same flow transition - so arming behind the park would leave the
        // player staring at an instruction for a prompt that never appeared.
        // REF: FUN_801D0748 (states 0x14 / 0x1E)
        self.arm_round_open_prompt();

        // A message box on screen parks the entire battle - retail's
        // `FUN_801D0748` returns before it reads the flow state when
        // `FUN_801D9BBC` reports a box up (`ctx[+0x6B2]`). The guard is the
        // box queue, not the tutorial: the battle-open formation banner
        // (`raise_battle_open_banner`) rides the same single surface, and
        // gating on `battle_tutorial` meant an `Ambushed!` outside the
        // sparring fight queued a box nothing ever ticked or dismissed.
        // REF: FUN_801D0748, FUN_801D9BBC
        if self.tick_battle_tutorial_boxes() {
            return None;
        }

        // Player-driven: while the retail-model Arts command input is open
        // the action SM is parked - the per-press entry / review / Begin
        // flow owns the pad until the entered sequence runs (turn cycles)
        // or the player backs out (reopens the command menu).
        if self.battle_arts_input.is_some() {
            self.tick_battle_arts_input();
            return None;
        }

        // Player-driven: while the Arts submenu is open the action SM is
        // parked - drive it from the pad and return until the player runs an
        // art (turn cycles) or backs out (reopens the command menu).
        if self.battle_arts_menu.is_some() {
            self.tick_battle_arts_menu();
            return None;
        }

        // Player-driven: while the spell submenu is open the action SM is
        // parked - drive it from the pad and return until the player casts
        // (turn cycles) or backs out (reopens the command menu).
        if self.battle_spell_menu.is_some() {
            self.tick_battle_spell_menu();
            return None;
        }

        // Player-driven: while the inventory submenu is open the action SM is
        // parked - drive it from the pad and return until the player uses an
        // item (turn cycles) or backs out (reopens the command menu).
        if self.battle_item_menu.is_some() {
            self.tick_battle_item_menu();
            return None;
        }

        // Player-driven: while a command session is open the action SM is
        // parked - drive the command picker from the pad and return without
        // advancing the SM until the player confirms.
        if self.battle_command.is_some() {
            self.tick_battle_command();
            return None;
        }

        // No command session and no submenu open: the action SM owns the
        // frame, which is retail's flow band outside the selection states.
        // Returning to Idle here is what lets the next turn's
        // `open_battle_command` raise the turn-start prompt again.
        if self.battle_tutorial.is_some() {
            self.set_battle_flow(crate::battle_flow::BattleFlowState::Idle);
        }

        // `ctx.menu_open` is retail's cast-menu latch: the summon-invoke arm
        // sets it and the menu system clears it when the battle menu closes.
        // The engine's menus are the session objects the early returns above
        // gate on, so reaching this line IS "no menu open" - release the
        // latch here, or the Done band's `0x51` gate (which stays while it is
        // set) parks every summon-band action forever.
        self.battle_ctx.menu_open = 0;

        // Battle locomotion - the anim tick's root-motion drive
        // (`FUN_80047430`): the attack band's approach walk toward the
        // target and the recovery band's walk back to the seat. Retail runs
        // the actor-list anim tick ahead of the battle-scene per-frame tick
        // (`FUN_80046A20`), so this goes ahead of the SM step below.
        // REF: FUN_80047430 (root-motion term; `World::tick_battle_locomotion`)
        self.tick_battle_locomotion();

        // The hit-event driver - the anim tick's per-frame damage-kernel call
        // and its event-path commit (`FUN_80047430` -> `FUN_801EC3E4`). Runs
        // on the cursor the frame tick advanced ahead of this function and
        // ahead of the SM step, retail's order.
        // REF: FUN_80047430 (`0x8004787C..0x800478A4`, `0x80047900..0x80047A44`)
        self.tick_battle_hit_events();

        // Final Heal sweep (FUN_801e6968): retail runs it in the cleanup
        // state 0x50 *before* the liveness count resolves a wipe. Run it
        // before the SM step so a party member downed late last tick (a
        // monster cast / DoT) is revived before this step's wipe scan, and
        // again after this tick's damage lands (below).
        self.apply_final_heal_revives();

        // One frame of HP-bar ramp before the SM steps, so the `0x51` settle
        // check (`FUN_801E7250`) sees this frame's bar movement. Retail's
        // caller for `FUN_80047430` is not in the dumped corpus, so the
        // cadence is the port's choice; the arithmetic is not
        // (`legaia_engine_vm::battle_hp_bar`).
        // REF: FUN_80047430
        self.tick_battle_hp_bars();

        // Rebuild the cast-census bytes the magic band's exit states read.
        // Retail's cast tick (`FUN_801E09F8`) does this from zero every frame
        // before it drives any effect child, so the gates are measurements
        // rather than latches.
        // REF: FUN_801E09F8
        self.tick_battle_cast_census();

        // Pre-step snapshot of the attack chain's cursor. The chain consumes
        // one queued swing byte per frame it advances (`actor[+0x15]`, bumped
        // at the same site that stages the byte), and resets the cursor to `0`
        // on the frame it reads the terminator - so `strike_cursor_before` is
        // both "did this frame stage a swing" and, at the terminator, "how
        // many swings this action ran". Both readings are consumed by the
        // strike reconciliation below the step.
        let chain_actor = self.battle_ctx.active_actor as usize;
        let chain_state_before = self.battle_ctx.action_state;
        let strike_cursor_before = self
            .actors
            .get(chain_actor)
            .map(|a| a.battle.strike_index)
            .unwrap_or(0);

        let outcome = self.step_battle();

        // Cast band: fold the owed outcome at retail's seam (the frame the
        // band leaves `0x29`; the summon route folds in its stager).
        self.settle_cast_band(&outcome);

        // The all-pairs separation pass, on the line after the action SM -
        // retail's exact slot (`FUN_80046A20` runs `jal 0x801E295C` then
        // `jal 0x80051078`, every live battle frame).
        // REF: FUN_80051078, FUN_80050BB8 (kernels in
        // `legaia_engine_vm::battle_separation`; driver
        // `World::tick_battle_separation`)
        self.tick_battle_separation();

        // Apply this step's damage events (art strikes carry a damage value;
        // the loop owns folding while live, so events are consumed here).
        //
        // Only what **this** tick produced (`events_before` was measured on
        // entry): everything already in the queue has been folded once and is
        // only sitting there so a host can still observe it. Re-folding it
        // applies its HP delta again every frame until the host drains, which
        // for `ApplyArtStrike` is a target losing the same damage on repeat.
        // Both play hosts drain once per simulation tick, so this only bit a
        // driver that drains on redraw - but the queue's contract is "folded
        // once", not "drained promptly".
        let events: Vec<BattleEvent> = self.pending_battle_events.split_off(events_before);
        for e in &events {
            if let BattleEvent::ApplyArtStrike {
                actor_slot,
                target_slot,
                outcome,
                ..
            } = e
            {
                // Surface the resolved strike damage for HUD popups (the
                // fold below applies the HP side; this is cosmetic only).
                if let Some(dmg) = outcome.damage
                    && dmg > 0
                {
                    self.battle_hit_fx.push(BattleHitFx {
                        target_slot: *target_slot,
                        amount: dmg,
                        is_heal: false,
                        is_crit: false,
                    });
                }
                // A connecting art strike arms the impact-tint triple on
                // its target from the acting record's `+0x7A` class - the
                // same `FUN_801EC3E4` arm the basic swing takes (retail
                // runs that routine once per strike; a zero-damage connect
                // still reaches it).
                // REF: FUN_801EC3E4
                if outcome.damage.is_some() {
                    let class = self.attacker_impact_class(usize::from(*actor_slot));
                    if class < crate::move_power::IMPACT_CLASS_LIMIT {
                        self.arm_impact_tint(usize::from(*target_slot), class);
                    }
                }
            }
            self.fold_battle_event(e);
        }
        // Re-publish the folded stream so hosts can still *observe* it. The
        // loop owns the gameplay fold (folding twice would apply an art
        // strike's HP twice), but the same stream also carries
        // presentation-only members - `CameraFrameHeight`, anim / cast
        // triggers - and taking it here used to drop those on the floor for
        // every host running the live loop. Hosts drain, they do not fold.
        // Appended, not prepended: an undrained backlog keeps its order and
        // this tick's tail stays behind it.
        self.pending_battle_events.extend(events);

        // A byte staged this step on an actor that has **no clip** to play
        // it (a clip-less host, or an engine-synthetic clip without an entry
        // head): a zero-length clip. Retail cannot have one - every entry on
        // the disc carries its head - so the port resolves such a byte's
        // hits at stage time, the pre-hit-event pacing, and applies the
        // combo total when the byte is the action's last.
        if chain_state_before == ActionState::AttackChain.as_byte()
            && self.battle_ctx.active_actor as usize == chain_actor
        {
            let (cursor_now, staged) = self
                .actors
                .get(chain_actor)
                .map(|a| (a.battle.strike_index, a.battle.queued_anim))
                .unwrap_or((0, 0));
            if cursor_now > strike_cursor_before && !self.staged_byte_has_clip(chain_actor, staged)
            {
                self.resolve_zero_length_clip_hits(chain_actor as u8, staged);
            }
        }

        // Generic physical attack: deal damage on the strike-landed edge when
        // **the chain itself staged nothing**.
        //
        // `strike_cursor_before` is the number of bytes this action's chain
        // consumed (the terminator step leaves it at the terminator index).
        // A zero count is an actor whose stream was never seeded - a monster
        // whose catalog carries no attack entries (the synthetic catalog), or
        // a synthetic party slot - and that keeps its single edge-triggered
        // application: the AGL-budget swings, resolved as immediate hits and
        // applied as one combo total.
        if let StepOutcome::Transition { from, to } = outcome
            && from == ActionState::AttackChain.as_byte()
            && to == ActionState::AttackRecovery.as_byte()
            && strike_cursor_before == 0
        {
            self.apply_basic_attack();
        }

        // Mark the dead so the SM's liveness scan resolves the wipe.
        for a in self.actors.iter_mut() {
            if a.battle.max_hp > 0 && a.battle.hp == 0 {
                a.battle.liveness = 0;
            }
        }

        // Final Heal sweep (FUN_801e6968) over this step's casualties - the
        // engine point closest to retail's state-0x50 "cleanup before the
        // liveness count" placement.
        self.apply_final_heal_revives();

        // Recovery-edge ADVANCE_DONE clear (retail clears this when the
        // recovery animation finishes; we simulate the same edge inline).
        //
        // The second arm is the **stall guard** for the strike-pacing gate.
        // `attack_chain` sets `ADVANCE_DONE` when it stages a swing byte and
        // then holds until the animation system retires it. The engine's anim
        // commit ([`Self::commit_staged_battle_anim`]) does retire it for a
        // clip-less swing - but only through the branch it reaches *after* the
        // `queued_anim == current_anim` early-out, so a staged byte that
        // happens to equal the actor's current anim id never gets there and
        // the chain parks at `AttackChain` (`0x1E`) for the rest of the
        // session. Retire the flag here whenever the id pair has converged and
        // no *staged* clip is in flight, which is exactly the zero-length-swing
        // case; a real strike clip still paces the chain, because the commit
        // sets `battle_staged_anim` when it installs the player and only
        // `tick_battle_animations` clears it at end of clip. The idle / pose
        // player is deliberately not consulted - a pose is not a strike clip,
        // and requiring it to be absent would leave the same park in place on
        // the host that draws poses.
        let attacker = self.battle_ctx.active_actor as usize;
        if attacker < self.actors.len()
            && self.actors[attacker]
                .battle
                .flag_bits
                .has(ActorFlags::ADVANCE_DONE)
        {
            let a = &self.actors[attacker];
            let converged_idle =
                a.battle_staged_anim.is_none() && a.battle.queued_anim == a.battle.current_anim;
            if self.battle_ctx.action_state == ActionState::AttackRecovery.as_byte()
                || converged_idle
            {
                self.actors[attacker]
                    .battle
                    .flag_bits
                    .clear(ActorFlags::ADVANCE_DONE);
            }
        }

        // Cast-animation completion edge, the sibling of the clear above.
        //
        // `MagicSustain` (`0x2B`) holds while the caster's `spell_iter`
        // (`actor+0x1FA`) is non-zero, and the SM itself only ever *sets* it -
        // retail's cast-animation system is what counts it back down. The
        // port has no such driver, so a cast parked the action SM forever:
        // any battle in which a monster (or a party member) cast a spell
        // stopped dead, which is most real encounters. Retire it on the frame
        // the state is reached, exactly as the recovery edge above retires
        // `ADVANCE_DONE`.
        if self.battle_ctx.action_state == ActionState::MagicSustain.as_byte() {
            let caster = self.battle_ctx.active_actor as usize;
            if let Some(a) = self.actors.get_mut(caster) {
                a.battle.spell_iter = 0;
            }
        }

        // Summon-band settle glue, three siblings of the MagicSustain retire
        // above (reached by the SummonFlute items `0x98`/`0x99`, whose
        // `item_seed_band` stages `sub_route = 9`):
        //
        // * `SummonFadeIn` (`0x2A`) waits on the caster's anim-cue byte, which
        //   retail's cast-animation driver raises when the windup lands. The
        //   port has no such driver - cue it on the frame the state is
        //   reached.
        // * `SummonActorFreeze` (`0x2B`-family `0x35`) waits for the caster's
        //   invoke clip (queued id 9) to converge back to idle. With a real
        //   action-clip bank the one-shot's end converges it
        //   (`tick_battle_animations`); a clip-less actor never converges, so
        //   settle it here exactly as the zero-length-swing arm does.
        // * `summon_invoke` parks `ctx.menu_open = 1` (retail's cast-menu
        //   latch, cleared by the menu system); the release lives at the top
        //   of this function - reaching the SM step means no engine menu
        //   session is open - so the `0x51` gate and `QueuedFromMenu` see it
        //   down. The `anim_cue` latch is dropped at `EndOfAction` below.
        if self.battle_ctx.action_state == ActionState::SummonFadeIn.as_byte() {
            let caster = self.battle_ctx.active_actor as usize;
            if let Some(a) = self.actors.get_mut(caster) {
                a.battle.anim_cue = 1;
            }
        }
        if self.battle_ctx.action_state == ActionState::SummonActorFreeze.as_byte() {
            let caster = self.battle_ctx.active_actor as usize;
            if let Some(a) = self.actors.get_mut(caster)
                && a.battle_staged_anim.is_none()
            {
                a.battle.queued_anim = 0;
                a.battle.current_anim = 0;
            }
        }
        if self.battle_ctx.action_state == ActionState::EndOfAction.as_byte() {
            let caster = self.battle_ctx.active_actor as usize;
            if let Some(a) = self.actors.get_mut(caster) {
                a.battle.anim_cue = 0;
            }
        }

        // An escape spell that folded this tick (Warp and its item twins
        // land here through the band) ends the encounter now - no loot, no
        // game-over - the way the item path does on its own fold.
        if self.battle_escaped && self.mode == SceneMode::Battle {
            // Through the escape teardown's fade + exit hold, like the item
            // path - not the instant finish the results sequencer retired.
            self.battle_end = Some(BattleEndCause::Escaped);
            self.begin_battle_end_sequence();
            return Some(outcome);
        }

        self.cycle_battle_turn();

        if matches!(outcome, StepOutcome::BattleComplete) {
            // Retail does not leave the battle on the frame the wipe scan
            // raises the signal: the results sequencer holds the scene for
            // the load window, the result screen and the exit fade first.
            self.begin_battle_end_sequence();
        }
        Some(outcome)
    }

    /// Turn cycling for the live loop - retail's round machine, keyed on the
    /// action SM idling at `EndOfAction`.
    ///
    /// The round has two bands ([`crate::battle_round::RoundPhase`]) that
    /// never overlap. In the **command band** the flow SM owns the frame and
    /// this does nothing: the command tick walks the party through their
    /// rings and [`Self::begin_round_execution`] hands the round over once the
    /// last member commits. In the **execution band** every idle is a pick:
    /// the highest unspent initiative key acts next, party or monster, a party
    /// member dispatching the command it committed
    /// ([`Self::dispatch_pending_party_action`]) and a monster its AI pick.
    /// When no key is left the round ends (`0xFF`: the mode-counter bump +
    /// the `0x400` waker) and the next one opens (`0x14`: the actor sweep,
    /// the key re-seed, the DoT tick, `Begin | Run`).
    ///
    /// Extracted so every site that PARKS the SM at `EndOfAction` mid-tick
    /// (the spell / Spirit arms and the monster cast fold, which run in a
    /// tick that returns before the step) can claim the turn in the same
    /// tick. The SM's own `end_of_action` handler otherwise steps
    /// `EndOfAction -> PreActionWait -> ActionSeed` on the NEXT tick and
    /// re-seeds the same actor's **stale** action bytes - the shape that
    /// made every Spirit guard and every spell cast grant its actor a free
    /// bonus attack off the battle-entry queue (caught by the
    /// `seru_cast_magic_xp_ladder` test).
    ///
    /// REF: FUN_801D0748 (states `0x14` / `0x6E` / `0xFE`)
    /// REF: FUN_801E295C (the `0x5A` re-pick and the `0xFF` round end at
    /// `0x801E67E8`)
    pub(in crate::world) fn cycle_battle_turn(&mut self) {
        use crate::battle_round::RoundPhase;
        use vm::battle_action::ActionState;
        if self.battle_ctx.action_state != ActionState::EndOfAction.as_byte() {
            return;
        }
        // Only cycle while BOTH sides still have a living member - if either
        // side is wiped we leave the SM at EndOfAction so its liveness scan
        // resolves the wipe into BattleComplete next step. A petrified actor
        // counts as defeated (Stone), so it doesn't keep its side "alive" - a
        // fully-petrified party is a wipe, not a stuck loop.
        if !self.battle_both_sides_alive() {
            return;
        }
        match self.battle_round_flow.phase {
            // The flow SM owns the frame; the command tick advances it.
            RoundPhase::Command => {}
            // Battle entry without the formation path (`World::enter_battle`
            // alone, or a host that staged the SM by hand): the first idle is
            // the first round start.
            RoundPhase::Open => self.begin_battle_round(),
            RoundPhase::Execute => {
                if let Some(next) = self.next_combatant_by_initiative() {
                    self.dispatch_battle_turn(next);
                } else {
                    self.end_battle_round();
                    // A DoT can down the last member of a side; the round
                    // start re-checks before it opens a prompt.
                    self.begin_battle_round();
                }
            }
        }
    }

    /// `true` while each side still has a member who is not defeated.
    pub(in crate::world) fn battle_both_sides_alive(&self) -> bool {
        let party_count = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let party_alive = (0..party_count).any(|i| !self.actor_effectively_defeated(i));
        let monsters_alive = (party_count..n).any(|i| !self.actor_effectively_defeated(i));
        party_alive && monsters_alive
    }

    /// Retail's round end - the action SM's `ctx[+0x07] == 0xFF` arm
    /// (`0x801E67E8`), reached once the per-round action cursor has passed
    /// every living actor: bump the round counter `ctx[+0x28A]` and run the
    /// `0x400` waker `FUN_801F45A4`. The flow byte it parks at `0x14` is the
    /// next [`Self::begin_battle_round`].
    ///
    /// PORT: FUN_801E295C (state `0xFF`, `0x801E67E8..0x801E6810`)
    pub(in crate::world) fn end_battle_round(&mut self) {
        self.advance_battle_mode();
        self.tick_status_0x400_wakes();
    }

    /// Retail's round start - `FUN_801D0748` state `0x14` (`0x801D0EC4`):
    /// the actor sweep `FUN_801D88CC`, the initiative seeder `FUN_801DA780`,
    /// the per-round DoT ticker `FUN_801E752C` (round index `!= 0`), and the
    /// unconditional `ctx[+0x06] = 0x1E` that opens `Begin | Run` for the
    /// round's first party command. Every later member's ring is reached
    /// through that prompt, and nothing executes until the last commit.
    ///
    /// The keys are re-seeded only when none is live: the battle-open path
    /// ([`World::enter_battle_from_formation`]) seeds them itself ahead of the
    /// formation latch, because the seeder is the one reader of the unlatched
    /// `ctx+0x290` and the side lockout would otherwise be lost; every later
    /// round finds them all spent and re-rolls.
    ///
    /// A **back attack** on the opening round takes retail's `0x0B -> 0xFE`
    /// jump instead of the prompt: the party enters no command, and with its
    /// keys zeroed by the lockout only the monsters dispatch.
    ///
    /// PORT: FUN_801D0748 (state `0x14`, `0x801D0EC4..0x801D0F0C`; the `0x0B`
    /// back-attack arm at `0x801D0E68..0x801D0EB0`)
    pub(in crate::world) fn begin_battle_round(&mut self) {
        use crate::battle_flow::BattleFlowState;
        use crate::battle_round::RoundPhase;
        // The sparring fight's opening caption holds the round start back:
        // retail's side-band tick sees `0x14` stored, raises the caption and
        // sets `ctx[+0x6B0]`, and `FUN_801D0748` returns on it before its
        // state switch (`0x801D0BDC`) - so none of the sweep / seed / prompt
        // below runs until the caption has gone. The box tick reopens the
        // round when it does.
        // REF: FUN_80056208 (stage-1 phases 0..1), FUN_801D0748 (`0x801D0BDC`)
        if self.raise_sparring_caption_if_due() {
            return;
        }
        self.battle_round_flow.flat_walk_last = None;
        // The actor sweep (`FUN_801D88CC`): action-gauge restore, the
        // `+0x1DF` action-stream clear, and the party band's stale-target
        // re-pick + category clear. Retail runs it *before* the initiative
        // seed and before the DoT tick (`801d0ec4..801d0ed8`). It draws no
        // RNG, so the seeder's stream is unchanged.
        crate::battle_round::BattleRound::boundary(self);
        // The Spirit stance is the `+0x1DE == 4` category the sweep just
        // cleared - it lasts exactly one round.
        self.battle_guarding = [false; 3];
        if !self.any_living_initiative_key() {
            self.reseed_initiative();
        }
        self.battle_round_flow.clear_pending();
        self.battle_round_flow.cursor = 0;
        // `FUN_801E752C` - the per-round status DoT ticker, skipped on round
        // 0 (`beq v0,zero` on `ctx[+0x28A]` at `0x801D0EFC`). RNG-free.
        if self.battle_mode() != 0 {
            self.tick_status_effects();
            if !self.battle_both_sides_alive() {
                return;
            }
        }
        let first_round = self.battle_mode() == 0;
        let ambushed = first_round
            && self.battle_formation_latched()
                == vm::battle_formulas::FormationAdvantage::BackAttack;
        if ambushed {
            // `0x0B`'s `ctx[+0x290] == 1` arm stores `0xFE` outright.
            self.begin_round_execution();
            return;
        }
        self.battle_round_flow.phase = RoundPhase::Command;
        // `0x14 -> 0x1E`, unconditional.
        self.set_battle_flow(BattleFlowState::TurnPrompt);
        if !self.battle_player_driven {
            // No pad drives the rings: every member strikes at its dispatch
            // (the auto-fight arm `FUN_801EED1C` seeds), so the round is
            // armed at once.
            self.begin_round_execution();
            return;
        }
        match self.next_member_owing_command(None) {
            Some(first) => self.open_battle_command(first),
            None => self.begin_round_execution(),
        }
    }

    /// Hand the round to the action SM - retail's `0x6E` begin arm storing
    /// `0xFE` (`0x801D31AC`) and `0xFE` storing `ctx[+0x07] = 0`
    /// (`0x801D3224`). The first pick happens here: retail's seeder made it
    /// at `0x14` (`FUN_801DA780` ends in `jal 0x801DABA4`) and the SM's
    /// `0x0C` dispatches whatever `ctx[+0x274]` names; the engine picks and
    /// dispatches on one call, and the command band draws no RNG in between,
    /// so the stream is retail's.
    ///
    /// PORT: FUN_801D0748 (state `0xFE`, `0x801D31E8..0x801D3224`)
    pub(in crate::world) fn begin_round_execution(&mut self) {
        use crate::battle_flow::BattleFlowState;
        use crate::battle_round::RoundPhase;
        use vm::battle_action::ActionState;
        self.battle_round_flow.phase = RoundPhase::Execute;
        self.battle_round_flow.flat_walk_last = None;
        self.battle_command = None;
        self.set_battle_flow(BattleFlowState::Idle);
        // A round entered without its start (a host or test that opened a
        // command surface on a hand-built battle) has no keys yet; retail
        // never reaches `0xFE` without `0x14`'s seed, so seed here rather
        // than let the first pick read an empty round and drop every
        // commit. A round that came through `begin_battle_round` finds its
        // keys live and this is a no-op.
        if !self.any_living_initiative_key() {
            self.reseed_initiative();
        }
        self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
        self.cycle_battle_turn();
    }

    /// The next party member who still owes this round a command, scanning
    /// forward from `after` (or from slot 0) - retail `FUN_801DB81C` (from
    /// `ctx[+0x13] + 1`) and its sibling `FUN_801DBA04` (from zero). Both skip
    /// a member already committed (`_DAT_8007BD10[i] == 4`), one with no HP,
    /// and one whose status word carries `+0x16E & 0xF84` - the petrified /
    /// asleep / numbed band and the `0x380` AI-delegated bits, none of which
    /// hands the pad a ring.
    ///
    /// PORT: FUN_801DB81C
    /// REF: FUN_801DBA04
    pub(in crate::world) fn next_member_owing_command(&self, after: Option<u8>) -> Option<u8> {
        let party_count = self.party_count.clamp(1, 3);
        let start = after.map_or(0, |a| a.saturating_add(1));
        (start..party_count).find(|&slot| {
            let alive = self
                .actors
                .get(usize::from(slot))
                .is_some_and(|a| a.battle.liveness != 0 && a.battle.hp != 0);
            alive
                && !self.battle_round_flow.committed(slot)
                && !self.actor_blocked_from_acting(slot)
                && !self.actor_is_confused(slot)
        })
    }

    /// Commit `action` as `actor`'s command for this round and walk the ring
    /// on - retail's ten-site commit idiom (`0x801D16AC` and siblings):
    /// advance to the next member that still owes a command, or begin the
    /// round. The `Run` commit is the exception retail makes at `0x32`
    /// (`0x801D1174..0x801D1184`): it stamps category `5` on every party actor
    /// and begins the round at once.
    ///
    /// PORT: FUN_801D0748 (the commit idiom; `0x32`'s run confirm)
    pub(in crate::world) fn commit_party_command(
        &mut self,
        actor: u8,
        action: crate::battle_round::PendingPartyAction,
    ) {
        use crate::battle_round::PendingPartyAction;
        let party_count = self.party_count.clamp(1, 3);
        let run = matches!(action, PendingPartyAction::Run);
        if let Some(slot) = self.battle_round_flow.pending.get_mut(usize::from(actor)) {
            *slot = Some(action);
        }
        self.battle_round_flow.cursor = actor;
        if run {
            for slot in 0..party_count {
                let alive = self
                    .actors
                    .get(usize::from(slot))
                    .is_some_and(|a| a.battle.liveness != 0);
                if alive {
                    self.battle_round_flow.pending[usize::from(slot)] =
                        Some(PendingPartyAction::Run);
                }
            }
            self.begin_round_execution();
            return;
        }
        match self.next_member_owing_command(Some(actor)) {
            Some(next) => self.open_battle_command(next),
            None => self.begin_round_execution(),
        }
    }

    /// Give `next` its turn in the execution band: age its buffs, then a
    /// blocked actor loses the turn, a monster runs its AI pick, and a party
    /// member dispatches the command it committed (a member with none - the
    /// auto-fight party, or one the member walk skipped - strikes, and a
    /// confused one strikes and re-targets).
    ///
    /// REF: FUN_801E295C (state `0x0C`: `FUN_801EED1C` for a party slot, the
    /// `0x380` re-target for a delegated one)
    fn dispatch_battle_turn(&mut self, next: u8) {
        let party_count = self.party_count.max(1);
        // Start-of-turn: age this actor's buffs / debuffs, reverting any
        // that expire this turn.
        self.tick_battle_buffs_on_turn(next);
        if self.actor_blocked_from_acting(next) {
            // Sleep / Stone / Faint: the actor loses its turn. Its
            // initiative key was already consumed by the picker, so the
            // next advance moves on; advancing `active_actor` also moves
            // the no-speed walk past it. The status duration ticks once per
            // round at the round start (`tick_status_effects`), so the
            // affliction still wears off. The SM stays at EndOfAction (no
            // action armed) - exactly the "skipped turn" outcome.
            self.battle_ctx.active_actor = next;
            return;
        }
        if next >= party_count {
            self.take_monster_turn(next);
            return;
        }
        if self.actor_is_confused(next) {
            // Confused party member: it "acts uncontrollably", so the player
            // never got the ring - auto-arm a physical strike, then flip the
            // target to a random living ally (the retarget runs inside
            // `arm_party_physical`).
            self.arm_party_physical(next);
            return;
        }
        match self.battle_round_flow.pending[usize::from(next)].take() {
            Some(action) => self.dispatch_pending_party_action(next, action),
            None => self.arm_party_physical(next),
        }
    }

    /// Backstop for a session that was already open when the flow byte moved
    /// onto the round-open `Begin | Run` prompt.
    ///
    /// Retail's `0x14` arm sets `ctx[+0x06] = 0x1E` before any member picks,
    /// and the ring (`0x28`) is only reached through it - so the prompt is a
    /// property of the **round**, not of the turn. The port reads that off
    /// [`crate::battle_flow::BattleFlowState::TurnPrompt`]: the round boundary
    /// parks the flow there and battle entry leaves it at `Idle`, and
    /// [`World::open_battle_command`] now builds the session **already on the
    /// prompt** in both cases. What is left for this pass is the one ordering
    /// it cannot cover - a session opened while the flow was elsewhere and
    /// still open when the boundary parks it here. A session reopened
    /// mid-round (a submenu backed out of) finds the flow on a window state
    /// and is left alone, which is where retail's own cancel arms land.
    ///
    /// An **ambushed** party never reaches here on its lost round: the
    /// `ctx[+0x290]` side lockout ([`World::reseed_initiative`]) zeroes every
    /// party key, so no party turn opens - retail's `0x0B -> 0xFE` jump in the
    /// port's own seating.
    ///
    /// REF: FUN_801D0748 (states 0x14 / 0x1E)
    fn arm_round_open_prompt(&mut self) {
        use crate::battle_flow::BattleFlowState;
        use crate::battle_input::CommandPhase;
        if self.battle_flow != BattleFlowState::TurnPrompt {
            return;
        }
        let no_escape = self.battle_no_escape;
        if let Some(session) = self.battle_command.as_mut()
            && matches!(session.phase, CommandPhase::Menu { .. })
        {
            session.no_escape = no_escape;
            session.phase = CommandPhase::RoundPrompt { cursor: 0 };
        }
    }

    /// Apply one generic physical attack from the active attacker to its
    /// resolved target as an **immediate** combo: every swing of the AGL
    /// budget rolls through the retail melee kernel
    /// ([`legaia_engine_vm::battle_formulas::physical_predamage`], the body
    /// of `FUN_801EC3E4`) and accumulates, then the total lands on live HP
    /// once - the retail accumulate / apply shape without a clip to pace it.
    ///
    /// This is the path for an attacker whose stream carries no bytes: a
    /// monster with no attack entries in its catalog (the synthetic
    /// catalog), whose swing count is the AGL budget
    /// ([`Self::arm_monster_strike_budget`]). Every actor with a seeded
    /// stream is paced by the hit-event driver instead.
    ///
    /// REF: FUN_801EC3E4
    pub(in crate::world) fn apply_basic_attack(&mut self) {
        let attacker = self.battle_ctx.active_actor;
        let party_count = self.party_count.max(1);
        let strikes = if attacker >= party_count {
            self.monster_strike_budget.max(1)
        } else {
            1
        };
        let committed = self
            .actors
            .get(attacker as usize)
            .map(|a| a.battle.current_anim)
            .unwrap_or(0);
        let mut target = None;
        for _ in 0..strikes {
            let Some(t) = self.resolve_attack_target(attacker) else {
                break;
            };
            target = Some(t);
            self.land_melee_hit(attacker, t, BASIC_ATTACK_COMMAND, committed, false);
        }
        if let Some(t) = target {
            self.apply_combo_total(t);
        }
    }

    /// The engine seat of retail's per-frame damage-kernel call. For every
    /// actor whose committed one-shot clip is in flight, ask the kernel's
    /// head ([`vm::battle_action::hit_event_admits`]) whether the frame the
    /// clip is on is one of its `+0x10..+0x13` beats; resolve the hit; then
    /// run the tick's **event-path commit** - with a byte staged behind the
    /// clip (`ADVANCE_DONE`, retail `+0x1DC` bit 1) and the entry's `+0x76`
    /// lock clear, the queued clip commits once the cursor is past the beat
    /// by more than two frames ([`vm::battle_action::event_commit_due`]),
    /// cutting the swing short. That cut is the swing chain's pacing: a
    /// retail two-arrow attack starts its second swing three frames after
    /// the first one's hit, not at the first clip's end. Locked entries
    /// (every art record on the disc carries `+0x76 = 1`) play to their
    /// natural end and the next byte commits there
    /// ([`Self::tick_battle_animations`]).
    ///
    /// PORT: FUN_80047430 (`0x8004787C..0x800478A4`: the per-frame
    /// `FUN_801EC3E4(actor, entry, cursor >> 4)` call; `0x80047900..
    /// 0x80047A44`: the bit-1 event-path commit)
    /// REF: FUN_801EC3E4 (head guard chain ported as
    /// `legaia_engine_vm::battle_action::hit_event_admits`)
    pub(in crate::world) fn tick_battle_hit_events(&mut self) {
        use vm::battle_action::{ActionState, ActorFlags, event_commit_due, hit_event_admits};
        let state = self.battle_ctx.action_state;
        // A fresh action starts a fresh combo total on every target.
        if state == ActionState::Begin.as_byte() {
            for a in self.actors.iter_mut() {
                a.battle.damage_accum = 0;
            }
        }
        for i in 0..self.actors.len() {
            let source = {
                let a = &self.actors[i];
                match (a.battle_staged_anim, &a.battle_animation) {
                    (Some(_), Some(p)) => p.hit_source().map(|src| (src, p.current_frame())),
                    _ => None,
                }
            };
            let Some((src, frame)) = source else {
                continue;
            };
            let frame_u8 = frame.clamp(0, 255) as u8;
            let hit_index = self.actors[i].battle.input_cursor;
            if let Some(hit) = hit_event_admits(
                state,
                &src.power_run,
                &src.event_frames,
                hit_index,
                frame_u8,
            ) {
                // The epilogue bump (`0x801EECDC..0x801EECE8`), on every
                // resolved call.
                self.actors[i].battle.input_cursor = hit_index.wrapping_add(1);
                self.resolve_hit_event(i as u8, hit, src.event_frames);
            }
            // The event-path commit: only with a byte staged behind this clip.
            let staged_behind = self.actors[i]
                .battle
                .flag_bits
                .has(ActorFlags::ADVANCE_DONE);
            if staged_behind && event_commit_due(&src.event_frames, src.event_lock, frame) {
                self.commit_staged_battle_anim_at_boundary(i);
            }
        }
    }

    /// Resolve one admitted hit event of `attacker`'s committed clip: the
    /// weapon fold, the melee roll with the clip's power byte, the
    /// accumulate, the art record's side data (status, hit cue) when the
    /// latched staged id names a Tactical Art, and the retail apply law -
    /// the hit landing once the band has left the strike loop (cursor parked
    /// at `0xFF`) that is its clip's last listed hit subtracts the whole
    /// accumulated total from live HP.
    ///
    /// PORT: FUN_801EC3E4 (`0x801EE984..0x801EEA40`: the apply gate -
    /// `ctx[+0x15] == 0xFF` and `entry[0x11 + idx] == 0 || idx == 3`)
    fn resolve_hit_event(
        &mut self,
        attacker: u8,
        hit: vm::battle_action::HitEvent,
        event_frames: [u8; 4],
    ) {
        use vm::battle_action::{
            STRIKE_CURSOR_PARKED, fold_weapon_atk_on_hit, staged_art_constant,
        };
        let state = self.battle_ctx.action_state;
        let (party, latched, chosen, committed) = {
            let a = &self.actors[attacker as usize];
            (
                a.battle_monster_id.is_none(),
                a.battle.latched_anim,
                a.battle.chosen_art,
                a.battle.current_anim,
            )
        };
        let art = staged_art_constant(latched, chosen, party);
        {
            let mut host = BattleHostImpl { world: self };
            fold_weapon_atk_on_hit(&mut host, attacker, state, &hit);
        }
        let Some(target) = self.resolve_attack_target(attacker) else {
            return;
        };
        let dmg = self.land_melee_hit(attacker, target, hit.power_byte, committed, art.is_some());
        if let Some(art) = art {
            self.apply_art_hit_side_data(attacker, target, art, hit.hit_index, dmg);
        }
        let cursor_parked =
            self.actors[attacker as usize].battle.strike_index == STRIKE_CURSOR_PARKED;
        let last_of_clip = hit.hit_index >= 3
            || event_frames
                .get(usize::from(hit.hit_index) + 1)
                .is_none_or(|&f| f == 0);
        let applied = cursor_parked && last_of_clip;
        if applied {
            self.apply_combo_total(target);
        }
        let running_total = self.actors[target as usize].battle.damage_accum;
        self.battle_hit_events
            .push(crate::battle_events::BattleHitEvent {
                attacker_slot: attacker,
                target_slot: target,
                hit_index: hit.hit_index,
                power_byte: hit.power_byte,
                damage: dmg,
                running_total: running_total.min(u32::from(u16::MAX)) as u16,
                applied,
                is_art: art.is_some(),
            });
    }

    /// Whether the byte the chain just staged on `slot` has a clip with an
    /// entry head to play - the test that separates a real, paced clip
    /// from the zero-length fallback. Mirrors the anim commit's lookup
    /// ([`Self::commit_staged_battle_anim_at_boundary`]).
    fn staged_byte_has_clip(&self, slot: usize, staged: u8) -> bool {
        use vm::anim_vm::{StagedAnimTarget, resolve_staged_anim};
        let Some(actor) = self.actors.get(slot) else {
            return false;
        };
        let clip = match resolve_staged_anim(staged) {
            StagedAnimTarget::ArtBank { record, .. } if actor.battle_art_bank.is_some() => actor
                .battle_art_bank
                .as_ref()
                .and_then(|b| b.get(record as usize))
                .and_then(|c| c.as_ref()),
            _ => actor
                .battle_action_clips
                .as_ref()
                .and_then(|cl| cl.get(staged as usize))
                .and_then(|c| c.as_ref()),
        };
        clip.is_some_and(|c| c.has_entry_head() && c.frame_count > 0 && c.part_count > 0)
    }

    /// The zero-length-clip fallback: resolve every hit the staged byte
    /// would have carried, now. A party art constant walks its record's
    /// power list (the embedded entry's power run is that list); any other
    /// byte is one hit with the byte itself as the power byte - the swing
    /// command's own tier, the pre-clip reading. When the byte is the
    /// action's last (the terminator is next) the combo total lands.
    fn resolve_zero_length_clip_hits(&mut self, attacker: u8, staged: u8) {
        use vm::battle_action::staged_art_constant;
        let (party, chosen, next_is_end) = {
            let a = &self.actors[attacker as usize];
            (
                a.battle_monster_id.is_none(),
                a.battle.chosen_art,
                a.battle.read_param(0) == 0,
            )
        };
        let Some(target) = self.resolve_attack_target(attacker) else {
            return;
        };
        let art = staged_art_constant(staged, chosen, party);
        let mut hits: Vec<u8> = Vec::new();
        if let Some(art) = art {
            let character = self.actors[attacker as usize].battle.character;
            if let Some(rec) = self.art_records.get(&(character, art)) {
                hits.extend(rec.power.iter().filter_map(|p| match p {
                    // Re-encode the decoded power byte for the kernel.
                    legaia_art::PowerByte::Damage(ap) => Some(power_byte_of(*ap)),
                    legaia_art::PowerByte::NoDamage => None,
                }));
            }
            hits.truncate(usize::from(vm::battle_action::HIT_EVENT_SLOTS));
        }
        // Any other byte is one hit with the byte itself as the power byte -
        // except the `0x19` / `0x1A` art starters, whose records carry no
        // event frame (a retail starter clip plays with `+0x10 = 0`, capture
        // and disc alike), and bytes outside the kernel's admitted band.
        if hits.is_empty()
            && (0x0C..=0x1F).contains(&staged)
            && !legaia_art::ActionConstant::from_byte(staged).is_some_and(|a| a.is_starter())
        {
            hits.push(staged);
        }
        if hits.is_empty() {
            return;
        }
        let n = hits.len();
        for (i, power) in hits.into_iter().enumerate() {
            let dmg = self.land_melee_hit(attacker, target, power, staged, art.is_some());
            if let Some(art) = art {
                self.apply_art_hit_side_data(attacker, target, art, i as u8, dmg);
            }
            let applied = next_is_end && i + 1 == n;
            if applied {
                self.apply_combo_total(target);
            }
            let running_total = self.actors[target as usize].battle.damage_accum;
            self.battle_hit_events
                .push(crate::battle_events::BattleHitEvent {
                    attacker_slot: attacker,
                    target_slot: target,
                    hit_index: i as u8,
                    power_byte: power,
                    damage: dmg,
                    running_total: running_total.min(u32::from(u16::MAX)) as u16,
                    applied,
                    is_art: art.is_some(),
                });
        }
    }

    /// The art record's per-hit side data retail's kernel does not carry
    /// (it reads the clip entry only): the status effect on a landing hit
    /// and the hit cue at this index, scheduled now (the clip is on the
    /// beat).
    fn apply_art_hit_side_data(
        &mut self,
        attacker: u8,
        target: u8,
        art: legaia_art::ActionConstant,
        hit_index: u8,
        dmg: u16,
    ) {
        let character = self.actors[attacker as usize].battle.character;
        let Some(rec) = self.art_records.get(&(character, art)) else {
            return;
        };
        let effect = rec.enemy_effect;
        let cue = rec.hit_cues.get(usize::from(hit_index)).copied();
        if dmg > 0
            && effect != legaia_art::EnemyEffect::None
            && self.actors[target as usize].battle.liveness != 0
        {
            let applied = self.status_effects.apply_from_enemy_effect(target, effect);
            // Rot's applier rolls the disabled limb (`rand % 3`, the retail
            // `1 << (rand%3 + 3)` bit pick).
            if applied == Some(legaia_engine_vm::status_effects::StatusKind::Rot) {
                let limb = (self.next_rng() % 3) as u8;
                self.status_effects.set_rot_limb(target, limb);
            }
        }
        if let Some(cue) = cue
            && cue.is_sound()
        {
            self.battle_sfx_cues
                .push(crate::battle_events::BattleSfxCue {
                    kind: cue.kind,
                    timing_frames: 0,
                    actor_slot: attacker,
                    target_slot: target,
                });
        }
    }

    /// Roll one melee hit from `attacker` on `target` and **accumulate** it
    /// - the retail melee kernel `FUN_801EC3E4`'s body: attacker ATK (plus the
    /// execution-time equipment fold of the committed command) rolled against
    /// the defender's UDF / LDF, the underdog rewrite, the finisher's post
    /// stages; then the hit's damage into the target's combo accumulator
    /// (`target[+0x0]`, `0x801EDB40`) and HP-bar accumulator (`+0x10`,
    /// `0x801EDB58`), the Spirit accrual, the popup, the impact cue and the
    /// flinch. Live HP is **not** touched - [`Self::apply_combo_total`] does
    /// that once per combo. Returns the damage the hit rolled.
    ///
    /// `power_byte` is the byte the kernel resolves the hit from - the clip
    /// entry's `entry[hit_index]` (`0x801EC494`): it picks the defence half
    /// (`(byte - 0x0C) % 10 < 5` -> UDF, [`vm::battle_formulas::physical_defense_is_udf`])
    /// and the power scalar (`0x801F64EC[(byte - 0x0C) % 5]`,
    /// [`vm::battle_formulas::command_power_scalar`]). A swing entry's byte 0
    /// is a real power byte (Vahn's high swing reads `0x18`, his low swing
    /// `0x1D`), not the command. `committed` is the attacker's `+0x1D9`
    /// (the committed anim id): the equipment fold dispatches on it and the
    /// art scale (`> 0x10`) reads it.
    ///
    /// **A melee swing always connects.** The routine contains no read of the
    /// accuracy / evasion halfword `+0x168` at all. `FUN_800402F4`'s
    /// selector-9 roll, which the port used to gate this strike on, is the
    /// **queued-action interrupt** check - a stun, not a miss. Retail's
    /// "Miss" on a normal attack is the limb-vs-height mismatch (`+0x1E`
    /// class 2 / 3 against the power byte's class, `0x801EC494..0x801EC554`),
    /// which the port does not model yet.
    ///
    /// PORT: FUN_801EC3E4 (the accumulating body; the head is
    /// `legaia_engine_vm::battle_action::hit_event_admits`)
    /// REF: FUN_800402F4 (selector 9 = the action-interrupt roll, ported as
    /// `battle_formulas::accuracy_roll`)
    fn land_melee_hit(
        &mut self,
        attacker: u8,
        target: u8,
        power_byte: u8,
        committed: u8,
        _is_art: bool,
    ) -> u16 {
        let attacker_i = attacker as usize;
        let target_i = target as usize;
        // Base ATK (`+0x158`, seeded without equipment) plus the execution-time
        // equipment fold: half of the one equipment slot the committed command
        // reads (`FUN_801EC3E4`'s `PTR_801CF4B4` arms - footwear for High /
        // Low, slot 2 / 3 for the two arm commands, all five for an art).
        // Party attackers only; the monster branch performs no fold.
        let mut attack = self.battle_attack.get(attacker_i).copied().unwrap_or(0);
        if attacker < self.party_count
            && let Some(bonuses) = self.battle_equip_atk.get(attacker_i)
        {
            let fold_command = if committed > vm::battle_formulas::ART_ANIM_THRESHOLD {
                vm::battle_formulas::ARMS_ART_COMMAND
            } else {
                committed
            };
            if let Some(fold) = vm::battle_formulas::arms_weapon_atk_fold(fold_command, bonuses) {
                attack = attack.saturating_add(fold);
            }
        }
        let defense = self.physical_defense_of(target, power_byte);
        // Spirit guard stance on the defender (a party slot that picked
        // Spirit and hasn't started its next turn).
        let target_guarding = self.battle_guarding.get(target_i).copied().unwrap_or(false);
        let hp = self
            .actors
            .get(attacker_i)
            .map(|a| a.battle.hp)
            .unwrap_or_default();
        let hit_inputs = vm::battle_formulas::PhysicalHit {
            attacker_atk: attack,
            attacker_hp: hp,
            defender_def: defense,
            command_scalar: vm::battle_formulas::command_power_scalar(power_byte),
            staged_anim: committed,
            defender_guarding: target_guarding,
            ..Default::default()
        };
        let mut raw = vm::battle_formulas::physical_predamage(&hit_inputs, &mut || {
            (self.next_rng() & 0x7FFF) as u16
        });
        if self.use_damage_finish {
            // The finisher's *post* stages only: the defender's equipment
            // elemental-guard / All-Guard ladder, the 9999 cap and the
            // rand-based no-damage floor. `defender_guarding` is passed
            // `false` because the melee kernel above already accounted for
            // the Spirit stance - taking the finisher's halve as well would
            // charge the stance twice. The floor draws a rand only when the
            // hit zeroes out, which the melee kernel's chip floor makes rare.
            let floor_rand = if raw == 0 {
                (self.next_rng() & 0x7FFF) as u16
            } else {
                0
            };
            let attacker_is_party = attacker < self.party_count;
            let target_is_party = target < self.party_count;
            let defender_resist = self.defender_resist(target);
            raw = vm::battle_formulas::damage_finish(&vm::battle_formulas::DamageFinish {
                predamage: u32::from(raw),
                attacker_slot: if attacker_is_party { 0 } else { 3 },
                defender_slot: if target_is_party { 0 } else { 3 },
                attacker_element: 7, // basic attack is non-elemental
                defender_resist,
                defender_guarding: false,
                enemy_defender_halve: false,
                bypass_party_resist: false,
                summon_power_pct: 100,
                floor_rand,
            }) as u16;
        }
        let dmg = raw;
        // Spirit accrues from the pre-nullify hit: retail's finisher fills the
        // gauge before the nullify/absorb stage zeroes the HP loss, so a Stone
        // target's absorbed hit still charges its gauge.
        self.accrue_spirit_gauge(target, dmg);
        // A petrified target (Stone) absorbs the hit - no HP loss.
        let dmg = if self.actor_is_petrified(target) {
            0
        } else {
            dmg
        };
        // Accumulate: the combo total and the bar's owed delta, never live HP
        // (`0x801EDB40` / `0x801EDB58`; the bar ramp is what the player sees
        // falling hit by hit).
        let survives = {
            let t = &mut self.actors[target_i].battle;
            t.damage_accum = t.damage_accum.saturating_add(u32::from(dmg));
            t.arm_hp_bar();
            t.accumulate_hp_bar(i32::from(dmg));
            t.damage_accum < u32::from(t.hp)
        };
        // Surface the strike for HUD damage popups.
        self.battle_hit_fx.push(BattleHitFx {
            target_slot: target,
            amount: dmg,
            is_heal: false,
            is_crit: false,
        });
        // The impact-tint triple on the struck actor (`FUN_801EC3E4`
        // `0x801EE3D4..0x801EE43C`): the acting record's `+0x7A` class,
        // gated `0 < class < 6`. The routine has no exit ahead of this arm
        // - every connecting swing reaches it, so a Stone-absorbed hit
        // tints too. Party and monster attackers alike: the monster's basic
        // swing goes through the same routine with its archive entry as
        // the record.
        // REF: FUN_801EC3E4 (`sltiu v0,v0,0x6` at 0x801EE3E0)
        let class = self.attacker_impact_class(attacker_i);
        if class < crate::move_power::IMPACT_CLASS_LIMIT {
            self.arm_impact_tint(target_i, class);
        }
        // ... and its sound.
        self.fire_melee_impact_cue(attacker, target);
        // The flinch is staged for a target still standing on the accumulated
        // total (`target[+0x0] < hp`, `0x801EEC18..0x801EEC30`).
        if dmg > 0 {
            self.queue_battle_reaction(target_i, survives);
        }
        dmg
    }

    /// Land the accumulated combo total on `target`'s live HP - retail's one
    /// `+0x14C` write per combo (`0x801EEA10..0x801EEA3C`: `hp - total`,
    /// floored at zero) followed by the accumulator clear (`0x801EEA74`).
    /// The bar was seeded hit by hit, so this does not touch it.
    ///
    /// PORT: FUN_801EC3E4 (`0x801EE9F8..0x801EEA78`, the apply arm)
    pub(in crate::world) fn apply_combo_total(&mut self, target: u8) {
        let Some(a) = self.actors.get_mut(target as usize) else {
            return;
        };
        let total = a.battle.damage_accum;
        a.battle.damage_accum = 0;
        if total == 0 {
            return;
        }
        let before = a.battle.hp;
        a.battle.hp = if total < u32::from(before) {
            before - total as u16
        } else {
            0
        };
        if a.battle.max_hp > 0 && a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
    }

    /// The melee kernel's own sound - retail's `FUN_801EC3E4` tail
    /// (`0x801EEA80..0x801EEBEC`), **one** of two emissions, selected by the
    /// `_DAT_8007BD84` word:
    ///
    /// * **zero** - every ordinary swing: the battle-start sweep
    ///   `FUN_80055B6C` and the round reset `FUN_8004CE2C` both store zero
    ///   there and no dumped routine stores anything else - takes the
    ///   per-character **grunt**, `FUN_8003D53C(0x1D, chan, dur)` at
    ///   `0x801EEB44`, the seat's 1-based character id (`DAT_8007BD10[seat]`)
    ///   selecting `(0, 0x26)` Vahn / `(4, 0x2E)` Noa / `(6, 0x1A)` Gala
    ///   (`0x801EEAD0..0x801EEB40`); clip slot `0x1D` = `XA30`, the
    ///   ten-channel mono grunt bank. The fall-through re-reads the word at
    ///   `0x801EEB60` and, still zero, skips the cue at `0x801EEB68`.
    /// * **non-zero**: `bne v0,zero,0x801EEB70` at `0x801EEAC8` jumps over
    ///   the grunt into the cue path - `0x10C` through the battle overlay's
    ///   one sound funnel `FUN_8004FE5C` ([`crate::sfx_cue::route_sfx_cue`])
    ///   with the **attacker's actor-table index as the category**
    ///   (`0x801EEBD8`), so the two sides sound different by construction: a
    ///   party attacker takes the CD-XA voice leg (`XA27` channel 4, an
    ///   attack sting), a monster the element-tinted ring leg (`0x2A8`).
    ///
    /// The engine mirrors the word as `MonsterAiState::flag_bd84` - the same
    /// cell the damage finisher reads as the enemy-defender halve and the
    /// `0xB4` boss-intro cast gates on. Two grunt gates are not modelled -
    /// the per-strike latch `s7` (`0x801EEA84`, matched against the target's
    /// `+0x1F3`) and the voice pass's in-flight counter `_DAT_8007BC20 < 2`
    /// (`0x801EEAB4`) - so the port grunts on every party strike the word
    /// leaves to the grunt arm. The cue arm keeps its retail gates: the
    /// target playing a plain action-table clip (`+0x1D9 < 0x10`,
    /// `0x801EEB88`) and, inside the funnel, the drive being idle
    /// (`FUN_8003DE7C(1) == 0` at `0x8004FE9C`, modelled as
    /// [`World::battle_xa_busy_frames`]).
    ///
    /// The ring is transient by design. Its slots are drained in retail by
    /// `FUN_80016B6C`, which the port does not model (the hosts' own SFX
    /// scheduler is the drain), and the only state a persistent ring would
    /// carry across calls is the `last_played` dedupe word that same drainer
    /// maintains - so a stored ring would sit at zero and dedupe nothing.
    ///
    /// PORT: FUN_801EC3E4 (`0x801EEA80..0x801EEBEC`, the two sound sites)
    fn fire_melee_impact_cue(&mut self, attacker: u8, target: u8) {
        let category = self.retail_actor_category(attacker);
        if self.monster_ai_state.flag_bd84 == 0 {
            // The grunt arm. `XA30` channel + read span per character id
            // (`DAT_8007BD10[seat]`, 1-based); a monster seat names no
            // character and falls out of the switch silent (`0x801EEAFC`).
            if category < 3 {
                let char_id = self.party_roster_slot(attacker as usize) as u8 + 1;
                let grunt = match char_id {
                    1 => Some((0u32, 0x26u32)),
                    2 => Some((4, 0x2E)),
                    3 => Some((6, 0x1A)),
                    _ => None,
                };
                if let Some((channel, duration_sectors)) = grunt {
                    self.push_battle_xa_cue(crate::sfx_cue::XaVoiceClip {
                        clip: GRUNT_CLIP_SLOT,
                        channel,
                        duration_sectors,
                    });
                }
            }
            return;
        }
        // The cue arm: `0x10C` through the funnel.
        let target_anim = self
            .actors
            .get(target as usize)
            .map(|a| a.battle.current_anim)
            .unwrap_or(0);
        if target_anim >= 0x10 {
            return;
        }
        // The funnel switches legs on `category < 3`, which is retail's
        // **actor-table** index space (party `0..=2`, monsters `3..=7`). The
        // engine compacts seating to `party_count..`, so the slot has to be
        // re-based first or a monster seated at index 1 takes the party leg
        // and the fight goes silent from the wrong side.
        let element_of = |cat: u8| {
            let slot = self.engine_slot_of_retail_category(cat);
            self.battle_slot_element(slot).unwrap_or(NEUTRAL_ELEMENT)
        };
        let durations = self.xa_cue_durations.as_deref();
        let xa_duration_raw = |n: u32| {
            durations
                .and_then(|t| t.get(n as usize).copied())
                .unwrap_or(0)
        };
        let src = crate::sfx_cue::SfxCueSources {
            element_of: &element_of,
            xa_duration_raw: &xa_duration_raw,
            tutorial_active: self.battle_tutorial.is_some(),
            cd_read_busy: self.battle_xa_busy_frames > 0,
        };
        let mut ring = crate::sfx_cue::SfxCueRing::default();
        let out = crate::sfx_cue::route_sfx_cue(&mut ring, MELEE_IMPACT_CUE, category, &src);
        if let Some(id) = out.enqueued {
            self.battle_sfx_cues
                .push(crate::battle_events::BattleSfxCue {
                    kind: id,
                    timing_frames: 0,
                    actor_slot: attacker,
                    target_slot: target,
                });
        }
        if let Some(xa) = out.xa
            && xa.duration_sectors > 0
        {
            self.push_battle_xa_cue(xa);
        }
    }

    /// Queue one CD-XA clip start and hold the modelled drive busy for its
    /// read span (`dur` vsyncs - see [`World::battle_xa_busy_frames`]).
    /// REF: FUN_8003D53C
    fn push_battle_xa_cue(&mut self, cue: crate::sfx_cue::XaVoiceClip) {
        self.battle_xa_busy_frames = cue.duration_sectors.min(u16::MAX as u32) as u16;
        self.battle_xa_cues.push(cue);
    }

    /// An engine seat index in **retail's** actor-table index space: party
    /// `0..=2`, monsters `3..=7`. The engine compacts monster seating to
    /// `party_count..`, so any retail kernel that switches on "is this index a
    /// party slot" needs the re-based value, not the seat.
    fn retail_actor_category(&self, slot: u8) -> u8 {
        let pc = self.party_count.min(3);
        if slot < pc {
            slot
        } else {
            3u8.saturating_add(slot.saturating_sub(pc)).min(7)
        }
    }

    /// Inverse of [`Self::retail_actor_category`].
    fn engine_slot_of_retail_category(&self, category: u8) -> u8 {
        let pc = self.party_count.min(3);
        if category < 3 {
            category
        } else {
            pc.saturating_add(category - 3)
        }
    }

    /// Resolve the slot a strike from `attacker` should land on. The armed
    /// [`battle::BattleActor::active_target`] is **authoritative** whenever it
    /// names a living actor - on either band, the attacker's own included.
    ///
    /// Retail's melee resolver `FUN_801EC3E4` fetches the target actor as
    /// `actor_table[+0x1DD]` with no side test at all (`overlay_0898` dump,
    /// `0x801EC5A8..0x801EC5B4`: `andi v0,s4,0xff; sll v0,v0,2; addu s3,v0,a2;
    /// lw a3,0(s3)` where `s4` is the `+0x1DD` byte loaded at `0x801EC450`).
    /// The confuse retarget (`FUN_801E7320`, [`Self::resolve_monster_target`])
    /// depends on that: it rewrites `+0x1DD` onto the *caster's own* band, and
    /// an opposing-side clamp here silently discarded the rewrite, making the
    /// whole confuse mechanic inert at the point it is felt.
    ///
    /// The [`Self::first_living_opponent_of`] fallback survives only as the
    /// port-side safety net for a target that is unset-dead or out of the
    /// table - every retail arming path writes `+0x1DD` before the SM strikes.
    ///
    /// REF: FUN_801EC3E4 (target = `actor_table[+0x1DD]`, no side clamp)
    /// REF: FUN_801E7320 (the confuse retarget this must not discard)
    fn resolve_attack_target(&self, attacker: u8) -> Option<u8> {
        if let Some(a) = self.actors.get(attacker as usize) {
            let t = a.battle.active_target;
            if self
                .actors
                .get(t as usize)
                .is_some_and(|x| x.battle.liveness != 0)
            {
                return Some(t);
            }
        }
        self.first_living_opponent_of(attacker)
    }

    /// Drive one monster's turn. Runs the action picker
    /// ([`Self::pick_monster_action`], the port of `FUN_801E9FD4`'s generic
    /// decision core) and either folds the chosen cast and parks the SM at
    /// `EndOfAction` (a spell is the whole turn, like the player magic path) or
    /// arms a physical strike for the action SM to run.
    /// True if `slot` carries any status that blocks all actions (Sleep /
    /// Stone / Faint), so it loses its turn. The blocking set is defined
    /// by [`legaia_engine_vm::status_effects::StatusKind::blocks_actions`]; the
    /// battle turn loop ([`Self::advance_battle_mode`]) enforces it here.
    pub(in crate::world) fn actor_blocked_from_acting(&self, slot: u8) -> bool {
        self.status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind.blocks_actions())
    }

    /// True if `slot` carries any status that blocks magic (Curse /
    /// Faint). A blocked caster falls back to a physical strike rather
    /// than casting.
    pub(in crate::world) fn actor_blocked_from_magic(&self, slot: u8) -> bool {
        self.status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind.blocks_magic())
    }

    /// True if `slot` is petrified (Stone). A petrified actor can't be damaged
    /// (the wiki: it is "no longer able to be damaged") and counts as defeated.
    pub(crate) fn actor_is_petrified(&self, slot: u8) -> bool {
        self.status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind == vm::status_effects::StatusKind::Stone)
    }

    /// True if `slot` is out of the fight for wipe-detection purposes: either
    /// downed (`liveness == 0`, i.e. KO / Faint) or petrified (Stone counts as
    /// defeated even though the actor's `liveness` stays non-zero). A petrified
    /// member is still a valid target ("distraction") - this only governs the
    /// party-/monster-wipe checks, not target selection.
    pub(crate) fn actor_effectively_defeated(&self, slot: u8) -> bool {
        self.actors
            .get(slot as usize)
            .is_none_or(|a| a.battle.liveness == 0)
            || self.actor_is_petrified(slot)
    }

    /// The per-round status-`0x400` waker retail's action-SM state `0xFF`
    /// tail-calls (`jal 0x801f45a4` at `801e680c`).
    ///
    /// Retail sweeps the seven battle-actor slots and, for each **live** actor
    /// whose `+0x16E` carries bit `0x400`, draws one RNG sample and clears the
    /// bit on a 1-in-8 hit. The port keeps `+0x16E` as
    /// `BattleActor::field_flags`, so the sweep runs over the same word. The
    /// RNG is drawn only for a live afflicted actor - stepping the stream for
    /// an empty or unafflicted slot would desync it - and no retail applier
    /// sets `0x400`, so on a normal battle this loop consumes nothing.
    ///
    /// PORT: FUN_801F45A4 (the caller-side slot sweep)
    pub(in crate::world) fn tick_status_0x400_wakes(&mut self) {
        use vm::battle_formulas::{STATUS_BIT_0X400, status_0x400_wakes};
        // Retail's `&DAT_801C9370` sweep runs seven slots.
        const RETAIL_ACTOR_SLOTS: usize = 7;
        let n = self.actors.len().min(RETAIL_ACTOR_SLOTS);
        for slot in 0..n {
            let (status, alive) = {
                let a = &self.actors[slot].battle;
                (a.field_flags, a.liveness != 0)
            };
            if !alive || status & STATUS_BIT_0X400 == 0 {
                continue;
            }
            let roll = self.next_rng() as u16;
            if let Some(next) = status_0x400_wakes(status, alive, || roll) {
                self.actors[slot].battle.field_flags = next;
            }
        }
    }
}

/// Re-encode a decoded [`legaia_art::ArtPower`] into the power byte the
/// damage kernel reads (`0x801EC494`): the inverse of
/// [`legaia_art::PowerByte::from_byte`] over the admitted band `0x0C..=0x1F`
/// (five multiplier tiers x UDF / LDF x the plain / alt range). Used only by
/// the zero-length-clip fallback, which walks an art record's decoded power
/// list where a real clip would carry the raw run in its entry head.
fn power_byte_of(ap: legaia_art::ArtPower) -> u8 {
    use legaia_art::PowerTarget;
    let tier = match ap.multiplier {
        12 => 0,
        18 => 1,
        20 => 2,
        22 => 3,
        _ => 4,
    };
    let base = match (ap.alt_range, ap.target) {
        (false, PowerTarget::Udf) => 0x16,
        (false, PowerTarget::Ldf) => 0x1B,
        (true, PowerTarget::Udf) => 0x0C,
        (true, PowerTarget::Ldf) => 0x11,
    };
    base + tier
}

#[cfg(test)]
mod power_byte_tests {
    use super::*;

    #[test]
    fn power_byte_of_inverts_the_decoder_over_the_admitted_band() {
        for b in 0x0Cu8..=0x1F {
            let legaia_art::PowerByte::Damage(ap) = legaia_art::PowerByte::from_byte(b) else {
                panic!("{b:#x} is in the damage band");
            };
            assert_eq!(power_byte_of(ap), b, "round trip of {b:#x}");
        }
    }
}

#[cfg(test)]
mod melee_cue_tests {
    use super::*;

    /// A battle with one party member and one monster, both alive.
    fn duel() -> World {
        let mut w = World::new();
        w.enter_battle(1, 1);
        for i in 0..2 {
            w.actors[i].battle.liveness = 1;
            w.actors[i].battle.hp = 500;
            w.actors[i].battle.max_hp = 500;
        }
        w.set_battle_attack(0, 80);
        w.set_battle_attack(1, 80);
        w.actors[0].battle.active_target = 1;
        w.actors[1].battle.active_target = 0;
        w
    }

    /// The `0x800788B8` duration table with the melee entry (`0x0C`) at its
    /// retail value, `373` -> `(373 * 60 + 99) / 100 = 224` sectors.
    fn durations_with_melee_entry() -> Vec<u16> {
        let mut t = vec![0u16; 0x40];
        t[0x0C] = 373;
        t
    }

    #[test]
    fn an_ordinary_party_swing_grunts_and_requests_no_sting() {
        let mut w = duel();
        w.xa_cue_durations = Some(durations_with_melee_entry());
        w.battle_ctx.active_actor = 0; // the party member attacks
        assert_eq!(
            w.monster_ai_state.flag_bd84, 0,
            "the `_DAT_8007BD84` word is zero at battle start"
        );
        assert!(w.apply_one_basic_strike(BASIC_ATTACK_COMMAND));
        assert!(
            w.drain_battle_sfx_cues().is_empty(),
            "the grunt arm submits nothing to the SPU ring"
        );
        let xa = w.drain_battle_xa_cues();
        assert_eq!(xa.len(), 1, "one swing, one grunt, no sting: {xa:?}");
        // `FUN_8003D53C(0x1D, 0, 0x26)` - Vahn's `XA30` channel + read span.
        assert_eq!(
            (xa[0].clip, xa[0].channel, xa[0].duration_sectors),
            (0x1D, 0, 0x26)
        );
        assert_eq!(
            w.battle_xa_busy_frames, 0x26,
            "the modelled drive stays busy for the read span"
        );
    }

    #[test]
    fn an_ordinary_monster_swing_is_silent_at_this_site() {
        let mut w = duel();
        w.battle_ctx.active_actor = 1; // the monster attacks
        assert!(w.apply_one_basic_strike(BASIC_ATTACK_COMMAND));
        // `sltiu v0,a0,0x3` at `0x801EEA7C` skips the grunt for a monster
        // seat, and the re-read of the zero word at `0x801EEB60` skips the
        // cue: a monster's ordinary swing makes no sound from this routine.
        assert!(w.drain_battle_sfx_cues().is_empty());
        assert!(w.drain_battle_xa_cues().is_empty());
    }

    #[test]
    fn a_flagged_monster_swing_enqueues_the_melee_impact_cue() {
        let mut w = duel();
        // Non-zero word: `bne v0,zero,0x801EEB70` at `0x801EEAC8` takes the
        // cue arm.
        w.monster_ai_state.flag_bd84 = 1;
        w.battle_ctx.active_actor = 1; // the monster attacks
        w.land_melee_hit(1, 0, BASIC_ATTACK_COMMAND, 0, false);
        let cues = w.drain_battle_sfx_cues();
        assert_eq!(cues.len(), 1, "one swing, one cue: {cues:?}");
        // The funnel's element-tinted high leg: `0x10C + 0x19C`.
        assert_eq!(cues[0].kind, 0x2A8);
        assert_eq!(cues[0].actor_slot, 1);
        assert_eq!(cues[0].target_slot, 0);
        assert!(
            w.drain_battle_xa_cues().is_empty(),
            "no grunt on the cue arm"
        );
    }

    #[test]
    fn a_flagged_party_swing_takes_the_xa_leg_and_enqueues_nothing() {
        let mut w = duel();
        w.monster_ai_state.flag_bd84 = 1;
        w.xa_cue_durations = Some(durations_with_melee_entry());
        w.battle_ctx.active_actor = 0; // the party member attacks
        w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false);
        assert!(
            w.drain_battle_sfx_cues().is_empty(),
            "a party attacker's `0x10C` is a CD-XA voice request, not a ring id"
        );
        let xa = w.drain_battle_xa_cues();
        assert_eq!(xa.len(), 1, "the sting, and no grunt: {xa:?}");
        // Clip `(0x0C >> 3) = 1` remapped to `26` (`XA27`), channel `0x0C & 7`.
        assert_eq!(
            (xa[0].clip, xa[0].channel, xa[0].duration_sectors),
            (26, 4, 224)
        );
    }

    #[test]
    fn a_flagged_party_swing_is_dropped_while_the_drive_is_busy() {
        let mut w = duel();
        w.monster_ai_state.flag_bd84 = 1;
        w.xa_cue_durations = Some(durations_with_melee_entry());
        w.battle_ctx.active_actor = 0;
        // `FUN_8003DE7C(1) != 0` at `0x8004FE9C`: a read in flight drops the
        // voice leg's request.
        w.battle_xa_busy_frames = 5;
        assert!(w.apply_one_basic_strike(BASIC_ATTACK_COMMAND));
        assert!(w.drain_battle_sfx_cues().is_empty());
        assert!(w.drain_battle_xa_cues().is_empty());
    }

    #[test]
    fn a_target_playing_an_art_bank_clip_is_silent() {
        let mut w = duel();
        w.monster_ai_state.flag_bd84 = 1;
        w.battle_ctx.active_actor = 1;
        // Retail gate `0x801EEB88`: the cue is submitted only while the target
        // is playing a plain action-table clip.
        w.actors[0].battle.current_anim = 0x11;
        w.land_melee_hit(1, 0, BASIC_ATTACK_COMMAND, 0, false);
        assert!(w.drain_battle_sfx_cues().is_empty());
    }
}

#[cfg(test)]
mod impact_tint_arm_tests {
    use super::*;
    use crate::battle_anim::MonsterAnimPlayer;
    use legaia_asset::monster_archive::{MonsterAnimation, PartPose};

    /// A one-part, two-frame clip whose entry head carries `impact_class`.
    fn clip(impact_class: u8) -> MonsterAnimation {
        MonsterAnimation {
            action_id: 0xC,
            rate: 2,
            attach_key: 0,
            solo_flag: 0,
            impact_class,
            effect_script: Vec::new(),
            part_count: 1,
            frame_count: 2,
            frames: vec![vec![PartPose::default()], vec![PartPose::default()]],
        }
    }

    /// A battle with one party member and one monster, both alive, the
    /// attacker playing `clip(class)`.
    fn duel_with_attacker_clip(attacker: usize, class: u8) -> World {
        let mut w = World::new();
        w.enter_battle(1, 1);
        for i in 0..2 {
            w.actors[i].battle.liveness = 1;
            w.actors[i].battle.hp = 500;
            w.actors[i].battle.max_hp = 500;
        }
        w.set_battle_attack(0, 80);
        w.set_battle_attack(1, 80);
        w.actors[0].battle.active_target = 1;
        w.actors[1].battle.active_target = 0;
        w.actors[attacker].battle_animation = MonsterAnimPlayer::new(&clip(class));
        w.battle_ctx.active_actor = attacker as u8;
        w
    }

    /// A connecting swing stamps the retail triple on the STRUCK actor -
    /// `+0x21F = class`, `+0x0C = 0x1000` - from the attacker's committed
    /// record `+0x7A` (`FUN_801EC3E4` `0x801EE3D4..0x801EE43C`). No disc
    /// impact table is installed here, so the colour word is the one
    /// write with nothing to carry.
    #[test]
    fn a_connecting_swing_arms_the_impact_triple_from_the_attackers_clip() {
        let mut w = duel_with_attacker_clip(0, 1);
        assert!(w.apply_one_basic_strike(BASIC_ATTACK_COMMAND));
        assert_eq!(w.actors[1].battle.impact_state, 1);
        assert_eq!(
            w.actors[1].battle.render_blend,
            legaia_engine_vm::battle_formulas::TINT_BLEND_FULL
        );
        assert_eq!(
            w.actors[0].battle.impact_state, 0,
            "the attacker is untouched"
        );
    }

    /// The monster's basic swing goes through the same routine with its
    /// archive entry as the record.
    #[test]
    fn a_monster_swing_arms_the_party_target_the_same_way() {
        let mut w = duel_with_attacker_clip(1, 2);
        assert!(w.apply_one_basic_strike(BASIC_ATTACK_COMMAND));
        assert_eq!(w.actors[0].battle.impact_state, 2);
        assert_eq!(w.actors[0].battle.render_blend, 0x1000);
    }

    /// Class `0` and a class past the table (`sltiu v0,v0,0x6` at
    /// `0x801EE3E0`) arm nothing - the swing still lands.
    #[test]
    fn class_zero_and_out_of_table_classes_arm_nothing() {
        for class in [0u8, crate::move_power::IMPACT_CLASS_LIMIT, 0xFF] {
            let mut w = duel_with_attacker_clip(0, class);
            let hp_before = w.actors[1].battle.hp;
            assert!(w.apply_one_basic_strike(BASIC_ATTACK_COMMAND));
            assert!(
                w.actors[1].battle.hp < hp_before,
                "class {class}: the swing landed"
            );
            assert_eq!(w.actors[1].battle.impact_state, 0, "class {class}");
            assert_eq!(w.actors[1].battle.render_blend, 0, "class {class}");
        }
    }

    /// With no clip playing (a synthetic battle) the class reads `0`.
    #[test]
    fn no_playing_clip_reads_class_zero() {
        let mut w = duel_with_attacker_clip(0, 3);
        w.actors[0].battle_animation = None;
        assert!(w.apply_one_basic_strike(BASIC_ATTACK_COMMAND));
        assert_eq!(w.actors[1].battle.impact_state, 0);
    }
}

#[cfg(test)]
mod hp_delta_liveness_tests {
    use super::*;

    /// The two `hp == 0 -> liveness = 0` sites - the per-hit fold in
    /// [`World::apply_battle_hp_delta`] and the per-tick dead-marking sweep in
    /// [`World::step_battle_frame`] - must apply the SAME predicate: dead
    /// means `max_hp > 0 && hp == 0`. A seated-but-unrolled slot
    /// (`max_hp == 0`) taking a zero-damage hit is not a death on either
    /// path; a statted slot drained to zero is a death on both.
    #[test]
    fn zero_damage_on_an_unrolled_slot_is_not_a_death_on_either_path() {
        let mut w = World::new();
        w.enter_battle(1, 1);
        // Slot 0: seated but never statted (the hollow-party shape).
        w.actors[0].battle.hp = 0;
        w.actors[0].battle.max_hp = 0;
        w.actors[0].battle.liveness = 1;
        // Slot 1: a real combatant.
        w.actors[1].battle.hp = 40;
        w.actors[1].battle.max_hp = 500;
        w.actors[1].battle.liveness = 1;

        // Path 1: the per-hit fold. A 0-damage hit on the hollow slot lands
        // on `hp == 0` but must not mark it dead.
        assert_eq!(w.apply_battle_hp_delta(0, 0), 0);
        assert_eq!(
            w.actors[0].battle.liveness, 1,
            "apply_battle_hp_delta marked an unrolled slot dead"
        );

        // Path 2: the sweep predicate, same actor state, same verdict.
        let swept_dead = w.actors[0].battle.max_hp > 0 && w.actors[0].battle.hp == 0;
        assert!(!swept_dead, "the sweep and the fold must agree");

        // And the real death still resolves on both: drain the statted slot.
        assert_eq!(w.apply_battle_hp_delta(1, 40), 40);
        assert_eq!(w.actors[1].battle.hp, 0);
        assert_eq!(
            w.actors[1].battle.liveness, 0,
            "a statted slot at zero HP is dead on the fold path"
        );
        assert!(w.actors[1].battle.max_hp > 0 && w.actors[1].battle.hp == 0);
    }
}

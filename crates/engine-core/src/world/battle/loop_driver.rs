//! The per-frame live battle loop, basic-attack strike, target resolution, and
//! status-block / defeat predicates (incl. the Final Heal revive sweep). Split
//! out of `battle.rs` as additional `impl World` blocks; no logic change from
//! the original inline definitions.

use super::*;
mod hits;
mod round;

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
    /// [`crate::world::WorldToggles::live_gameplay_loop`] in [`Self::tick`].
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
    /// [`crate::battle_stage_module::boss_transition_stage_id`], run at the
    /// head of cleanup state `0x50` by [`World::run_boss_transition_arm`].
    ///
    /// REF: FUN_800402F4 (the revive arm this calls - case 4, tier 1 = full
    /// max HP + status clear)
    pub(in crate::world) fn apply_final_heal_revives(&mut self) {
        const LOST_GRAIL: u8 = 0xE7;
        const FINAL_HEAL_WORD1_BIT: u32 = 0x80; // ability bit 0x27 (39)
        let pc = (self.party.party_count.min(3) as usize).min(self.actors.len());
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
            let Some(record) = self.party.roster.members.get_mut(char_slot) else {
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
            self.battle.status_effects.cure_all(slot as u8);
            let a = &mut self.actors[slot].battle;
            // Retail sweeps in cleanup state `0x50`, after the killing
            // action has played out and its hit's ramp has carried the
            // readout down to the live zero. The port's post-damage sweep
            // runs on the hit's own tick, with that ramp still in flight, and
            // the revive's seed is an assignment that discards it - which
            // left the readout above max HP by the undrained remainder, a
            // `hp != hp_display` pair with a zero accumulator that parks the
            // `0x51` gate for good (soak: `jouina`, a member downed and
            // Final-Healed by one enemy hit). Settle the readout first, as
            // the elapsed ramp would have.
            // REF: FUN_801E6968, FUN_80047430
            a.resync_hp_bar();
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
            self.stand_revived_party_member(slot);
            self.battle.hit_fx.push(BattleHitFx {
                target_slot: slot as u8,
                amount: max_hp,
                is_heal: true,
                is_crit: false,
            });
        }
    }

    pub(in crate::world) fn live_battle_tick(&mut self) -> Option<StepOutcome> {
        use vm::battle_action::{ActionState, ActorFlags};

        // A party wipe with the scripted-loss latch clear has left battle:
        // MAIN INIT's back-from-battle arm stores `game_mode = 0x16` (CARD
        // INIT, `FUN_8003AEB0` `0x8003B5D4..0x8003B5EC`), so no battle frame
        // runs again. The port keeps the battle scene up only as the frozen
        // frame the game-over hand-off draws over; ticking its state
        // machines would start a fresh round on the members the wipe floor
        // stood back up (`0x8004FB94..0x8004FBA4`).
        // REF: FUN_8003AEB0
        if self.game_over_hold {
            return None;
        }

        // The modelled CD drive: one clip read span elapses per frame.
        self.audio.battle_xa_busy_frames = self.audio.battle_xa_busy_frames.saturating_sub(1);

        // The side-band pass: the frame driver runs it first on every battle
        // frame (`FUN_80046A20`, `jal 0x80056208` at `0x80046D60`), ahead of
        // the results sequencer and both state machines. While a boss-stage
        // module owns the fight nothing else runs.
        // REF: FUN_80046A20
        if self.tick_battle_sideband() {
            return None;
        }

        // The battle has ended and its presentation owns the frame: retail's
        // battle tick runs the results sequencer instead of the action SM
        // while `DAT_8007BD71 == 0xFE` (`FUN_80046A20` `0x80047040` /
        // `0x800470D0`), and nothing else - no round prompt, no turn cycling,
        // no menu - until the exit gate fires.
        // REF: FUN_80046A20
        if self.battle.victory.is_some() {
            // The body-pair store is not the SM's: the battle draw callback
            // `FUN_80048A08` -> `FUN_8004998C` re-derives every drawn actor's
            // `+0x3C` / `+0x40` whatever `DAT_8007BD71` says, so the posing
            // leader's pair follows the win pose through the whole sequence
            // - and that pair is the focus case 6's battle-over arm frames
            // (`noa_levelup_banner`: Vahn's pair reads `(78, -15)` off a
            // live `(2, -3)`, 38 frames into pose `0x14`). Skipping it left
            // the pair on the killing blow's pose. The root-motion half of
            // the locomotion pass stays with the SM.
            // REF: FUN_8004998C
            self.refresh_battle_body_pairs();
            // The battle main dispatcher still runs ahead of the sequencer
            // and steps every tracked widget glide (`FUN_801D9BBC` at
            // `0x801D0B2C`) - the result windows' raise among them.
            // REF: FUN_801D9BBC
            if let Some(g) = self.battle.result_windows_glide.as_mut() {
                g.step(super::commit_log_launch::BATTLE_PASS_STEP_PER_TICK);
            }
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
        // The commit log's launch glide runs every battle frame, whichever
        // surface owns the pad (`FUN_801D9BBC` is not gated on the flow).
        self.step_commit_log_launch();
        self.step_action_plate_glides();
        self.step_battle_intro_names();

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
        // The sparring fight's open: retail's flow sits in `0x0A` / `0x0B`
        // under the enemy names and reaches no round until they have gone
        // (`World::sparring_open_held`), so nothing below runs either.
        // REF: FUN_801D0748 (flow states `0x0A` / `0x0B`)
        if self.battle.sparring_round_pending {
            return None;
        }

        // Retail-compare debug seed: a capture taken mid-cast starts its cast
        // from the first command prompt, bypassing the pad - once the camera's
        // battle-entry sweep is over, since retail's battle tick opens no
        // prompt under it.
        if self.battle.inflight_seed.is_some()
            && self.battle.command.is_some()
            && self.battle.flow == crate::battle_flow::BattleFlowState::TurnPrompt
            && self
                .battle
                .camera
                .as_ref()
                .is_none_or(|c| c.entry_sweep_counter().is_none())
            && let Some(seed) = self.battle.inflight_seed.take()
        {
            self.dispatch_inflight_seed(seed);
        }

        // Player-driven: while the retail-model Arts command input is open
        // the action SM is parked - the per-press entry / review / Begin
        // flow owns the pad until the entered sequence runs (turn cycles)
        // or the player backs out (reopens the command menu).
        if self.battle.arts_input.is_some() {
            self.tick_battle_arts_input();
            return None;
        }

        // Player-driven: while the Arts submenu is open the action SM is
        // parked - drive it from the pad and return until the player runs an
        // art (turn cycles) or backs out (reopens the command menu).
        if self.battle.arts_menu.is_some() {
            self.tick_battle_arts_menu();
            return None;
        }

        // Player-driven: while the spell submenu is open the action SM is
        // parked - drive it from the pad and return until the player casts
        // (turn cycles) or backs out (reopens the command menu).
        if self.battle.spell_menu.is_some() {
            self.tick_battle_spell_menu();
            return None;
        }

        // Player-driven: while the inventory submenu is open the action SM is
        // parked - drive it from the pad and return until the player uses an
        // item (turn cycles) or backs out (reopens the command menu).
        if self.battle.item_menu.is_some() {
            self.tick_battle_item_menu();
            return None;
        }

        // Player-driven: while a command session is open the action SM is
        // parked - drive the command picker from the pad and return without
        // advancing the SM until the player confirms.
        if self.battle.command.is_some() {
            self.tick_battle_command();
            return None;
        }

        // No command session and no submenu open: the action SM owns the
        // frame, which is retail's flow band outside the selection states.
        // Returning to Idle here is what lets the next turn's
        // `open_battle_command` raise the turn-start prompt again.
        if self.battle.tutorial.is_some() {
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

        // The death-spoils caption (HUD element `0x5B`) holds for the rest of
        // the action that raised it (`crate::battle_steal`).
        self.tick_steal_caption();

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

        // Cleanup state `0x50` opens with the Final Heal sweep (`jal
        // 0x801E6968` at `0x801E5C6C`), whose tail hands the Cort fight to its
        // form-transition module once the first form has fallen. The tail
        // parks the SM at `0xFD`, and the state's own advance is guarded on
        // `ctx[+0x07]` still reading `0x50` (`0x801E5F4C..0x801E5F5C`) - so
        // the rest of the `0x50` body runs, the advance does not, and the
        // end-of-action gate `0x5A` whose survivor count would end the fight
        // is never reached.
        let boss_transition = self.battle_ctx.action_state == ActionState::DoneCleanup.as_byte()
            && self.run_boss_transition_arm();

        let outcome = self.step_battle();
        if boss_transition {
            self.battle_ctx.action_state = ActionState::IdleHold.as_byte();
        }

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
                    self.battle.hit_fx.push(BattleHitFx {
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

        // No combo total may outlive the attack band. Retail's ordering makes
        // a stranded total impossible when the stream holds swing entries:
        // a staged byte commits on the playing clip's event frame, after
        // that clip's hit, so every hit but the last swing's lands before the
        // `0x1F` park and the last swing's lands after it - the parked,
        // last-beat hit that subtracts the whole total
        // (`0x801EE984..0x801EEA40`). A last staged entry with no hit event
        // (a clip set a stale or synthetic install put behind the stream)
        // would leave the total on the target with live HP never written,
        // the bar's display short of it, and the `0x51` settle gate
        // (`FUN_801E7250`) holding the band forever. Engine choice: land any
        // such total as the action enters `0x50`.
        //
        // Every door into `0x50`, not just the attack band's: a cast's clip
        // runs the same kernel off its own hit events (`FUN_80047430` calls
        // `FUN_801EC3E4` for every drawn actor while the battle phase is
        // `0xFF`), and the cursor that would land its total is not parked in
        // a cast band. PROT 0955's Terror Scream is the shape: a status-only
        // capture body whose caster clip (`+0x1DA = 8`) carries power bytes,
        // so a monster's cast accumulated 97 on Gala, left `0x71` for `0x50`
        // with live HP never written, and parked `0x51` for good.
        if let StepOutcome::Transition { to, .. } = outcome
            && to == ActionState::DoneCleanup.as_byte()
        {
            let stranded: Vec<u8> = (0..self.actors.len().min(8))
                .filter(|&i| self.actors[i].battle.damage_accum > 0)
                .map(|i| i as u8)
                .collect();
            for t in stranded {
                self.apply_combo_total(t);
            }
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
            // Only the converged, clip-less case. `AttackRecovery` itself
            // must NOT release the latch: retail's `0x1F` waits for the last
            // staged byte to commit at the playing clip's boundary
            // (`0x801E3AEC..0x801E3AF8`), and releasing it here dropped that
            // byte - a two-swing queue played one swing.
            if converged_idle {
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
        //
        // Retail's counter-down is the anim commit `FUN_8004AD80`
        // (`0x8004B06C..0x8004B07C`, `+0x1FA -= 1` when non-zero), which runs
        // at a clip boundary: the latch `0x2A` raises drops when the cast
        // clip it staged re-commits at its end, so `0x2B` lasts the clip and
        // the cast-effect driver films it all that while
        // (`battle_gimard_tail_fire_a` is parked there). The retire waits for
        // that boundary: no one-shot clip still in flight on the caster.
        // `MagicAnimChain` (`0x2A`) takes its next `(clip, shot)` pair only
        // once the same latch is down, so a chain longer than one pair
        // parks there the same way.
        if self.battle_ctx.action_state == ActionState::MagicSustain.as_byte()
            || self.battle_ctx.action_state == ActionState::MagicAnimChain.as_byte()
        {
            let caster = self.battle_ctx.active_actor as usize;
            if let Some(a) = self.actors.get_mut(caster) {
                let clip_in_flight = a
                    .battle_animation
                    .as_ref()
                    .is_some_and(|p| !p.finished() && !p.is_looping());
                if !clip_in_flight {
                    a.battle.spell_iter = 0;
                }
            }
        }

        // Summon-band settle glue, three siblings of the MagicSustain retire
        // above (reached by the SummonFlute items `0x98`/`0x99`, whose
        // `item_seed_band` stages `sub_route = 9`):
        //
        // * `SummonFadeIn` (`0x33`) waits on the caster's `+0x1F5`, which is
        //   the **effect-script cursor** of the invoke clip: the battle
        //   effect-script walker `FUN_801DEA50` bumps it as each record fires
        //   on its frame, so the flash-in lands when the clip's first record
        //   does ([`summon_windup_cue`]).
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
                a.battle.anim_cue = summon_windup_cue(a);
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
        if self.battle.escaped && self.mode == SceneMode::Battle {
            // Through the escape teardown's fade + exit hold, like the item
            // path - not the instant finish the results sequencer retired.
            self.battle.end = Some(BattleEndCause::Escaped);
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

    /// Queue one CD-XA clip start and hold the modelled drive busy for its
    /// read span (`dur` vsyncs - see [`crate::world::AudioState::battle_xa_busy_frames`]).
    /// REF: FUN_8003D53C
    pub(in crate::world) fn push_battle_xa_cue(&mut self, cue: crate::sfx_cue::XaVoiceClip) {
        self.audio.battle_xa_busy_frames = cue.duration_sectors.min(u16::MAX as u32) as u16;
        self.audio.battle_xa_cues.push(cue);
    }

    /// An engine seat index in **retail's** actor-table index space: party
    /// `0..=2`, monsters `3..=7`. The engine compacts monster seating to
    /// `party_count..`, so any retail kernel that switches on "is this index a
    /// party slot" needs the re-based value, not the seat.
    pub(in crate::world) fn retail_actor_category(&self, slot: u8) -> u8 {
        let pc = self.party.party_count.min(3);
        if slot < pc {
            slot
        } else {
            3u8.saturating_add(slot.saturating_sub(pc)).min(7)
        }
    }

    /// Inverse of [`Self::retail_actor_category`].
    pub(in crate::world) fn engine_slot_of_retail_category(&self, category: u8) -> u8 {
        let pc = self.party.party_count.min(3);
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
        self.battle
            .status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind.blocks_actions())
    }

    /// True if `slot` carries any status that blocks magic (Curse /
    /// Faint). A blocked caster falls back to a physical strike rather
    /// than casting.
    pub(in crate::world) fn actor_blocked_from_magic(&self, slot: u8) -> bool {
        self.battle
            .status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind.blocks_magic())
    }

    /// True if `slot` is petrified (Stone). A petrified actor can't be damaged
    /// (the wiki: it is "no longer able to be damaged") and counts as defeated.
    pub(crate) fn actor_is_petrified(&self, slot: u8) -> bool {
        self.battle
            .status_effects
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
            let roll = self.next_rand() as u16;
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
mod melee_cue_tests;

#[cfg(test)]
mod impact_tint_arm_tests;

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

/// The caster's `+0x1F5` as the summon band's `0x33` reads it: the effect-script
/// cursor of the committed invoke clip (`Actor::battle_effect_cursor`, which the
/// walker `FUN_801DEA50` bumps per fired record and the anim commit
/// `FUN_8004AD80` zeroes). Before the invoke clip commits the byte is still
/// the previous clip's zeroed cursor, so the band waits. A caster with no
/// effect script, or whose invoke clip carries no record to fire, cannot
/// raise it and is cued at once (the port's clip-less stand-in).
// REF: FUN_801DEA50 (`0x801DEC08..0x801DEC48`), FUN_8004AD80
fn summon_windup_cue(a: &Actor) -> u8 {
    use crate::action_effect_script::EffectRecord;
    let Some(script) = a.battle_effect_script.as_ref() else {
        return 1;
    };
    if a.battle.queued_anim != a.battle.current_anim {
        return 0;
    }
    let first_fires = EffectRecord::at(script, 0).is_some_and(|r| r.frame != 0);
    if first_fires {
        a.battle_effect_cursor
    } else {
        1
    }
}

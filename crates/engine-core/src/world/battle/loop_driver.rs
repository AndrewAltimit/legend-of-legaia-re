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
        if a.battle.hp == 0 {
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
    /// monster slot dead + `DAT_8007BD0C == 0xB5` boss-transition arm) is
    /// scripted-fight glue and is not modelled here.
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
        let mut art_strike_applied = false;
        for e in &events {
            if let BattleEvent::ApplyArtStrike {
                target_slot,
                outcome,
                ..
            } = e
            {
                art_strike_applied = true;
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

        // Chain-driven strike: this frame's `AttackChain` pass staged one
        // queued swing byte, so one hit resolves for it. Retail's seam is the
        // same one - `FUN_801EC3E4` runs once per committed arms command and
        // applies the HP loss itself.
        if chain_state_before == ActionState::AttackChain.as_byte()
            && !art_strike_applied
            && self.battle_ctx.active_actor as usize == chain_actor
        {
            let (cursor_now, staged) = self
                .actors
                .get(chain_actor)
                .map(|a| (a.battle.strike_index, a.battle.queued_anim))
                .unwrap_or((0, 0));
            if cursor_now > strike_cursor_before && vm::battle_action::is_swing_command(staged) {
                self.apply_one_basic_strike(staged);
            }
        }

        // Generic physical attack: deal damage on the strike-landed edge when
        // no art strike already did **and the chain itself struck nothing**.
        //
        // `strike_cursor_before` is the number of swing bytes this action's
        // chain consumed (the terminator frame is the one that zeroes the
        // cursor, and this is its pre-step value). A non-zero count means the
        // per-swing arm above already applied every hit; firing here as well
        // would charge the action one extra swing. A zero count is an actor
        // whose stream was never seeded - today the monster band, whose swing
        // count is the AGL budget rather than a queue - and that keeps its
        // single edge-triggered application.
        if let StepOutcome::Transition { from, to } = outcome
            && from == ActionState::AttackChain.as_byte()
            && to == ActionState::AttackRecovery.as_byte()
            && !art_strike_applied
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

        self.cycle_battle_turn();

        if matches!(outcome, StepOutcome::BattleComplete) {
            self.finish_battle();
        }
        Some(outcome)
    }

    /// Turn cycling for the live loop: the round boundary + the next-combatant
    /// re-arm, both keyed on the SM idling at `EndOfAction`.
    ///
    /// Extracted so every site that PARKS the SM at `EndOfAction` mid-tick
    /// (the spell / Spirit / tutorial arms and the monster cast fold, which
    /// all run in a menu tick that returns before the step) can claim the
    /// turn in the same tick. The SM's own `end_of_action` handler otherwise
    /// steps `EndOfAction -> PreActionWait -> ActionSeed` on the NEXT tick
    /// and re-seeds the same actor's **stale** action bytes - the shape that
    /// made every Spirit guard and every spell cast grant its actor a free
    /// bonus attack off the battle-entry queue (caught by the
    /// `seru_cast_magic_xp_ladder` test).
    ///
    /// REF: FUN_801D0748 (retail's flow SM owns this arming; the SM's 0x5A
    /// self-advance assumes the flow SM has already staged the next action)
    pub(in crate::world) fn cycle_battle_turn(&mut self) {
        use vm::battle_action::ActionState;
        // Re-arm the next combatant when the SM idles at EndOfAction, cycling
        // across the whole actor table (party AND monsters) in slot order so
        // monsters take their turns. Only re-arm while BOTH sides still have a
        // living member - if either side is wiped we leave the SM at
        // EndOfAction so its liveness scan resolves the wipe into
        // BattleComplete next step.
        let party_count = self.party_count.max(1);
        let n = self.actors.len() as u8;
        // A petrified actor counts as defeated (Stone), so it doesn't keep its
        // side "alive" - a fully-petrified party is a wipe, not a stuck loop.
        let mut party_alive = (0..party_count).any(|i| !self.actor_effectively_defeated(i));
        let mut monsters_alive = (party_count..n).any(|i| !self.actor_effectively_defeated(i));

        // Round boundary: the SM idles at EndOfAction and no living actor still
        // holds an initiative key, so a full round just completed. Tick every
        // actor's status effects once here - DoT damage (Venom / Toxic) plus
        // duration decay - mirroring the `BattleRound::end` tick the runner path
        // uses, so poison actually drains HP and afflictions wear off in the
        // live loop (this is the tick the skip-turn comment below relies on).
        // RNG-free (DoT is deterministic), so the upcoming reseed's RNG stream
        // is unchanged; gated on the SPD initiative path (a no-SPD synthetic
        // battle has no round concept). A DoT can down the last member of a
        // side, so re-evaluate the wipe flags afterward before arming a turn.
        if self.battle_ctx.action_state == ActionState::EndOfAction.as_byte()
            && party_alive
            && monsters_alive
            && self.any_battle_speed()
            && !self.any_living_initiative_key()
        {
            // End-of-round handler. Retail reaches it as action-SM state
            // `0xFF` (`801e67e8`), which the `0x5A` gate arms once every
            // living actor has acted: it parks the flow byte at `0x14` - the
            // round-driver state the sweep + DoT tick below stand in for -
            // then bumps the round counter `ctx[+0x28A]` and calls
            // `FUN_801F45A4`. Both run here, ahead of the sweep, in that
            // order. The bump is what walks a multi-phase boss through its
            // scripted casts (`crate::monster_ai::decide` reads the same
            // counter); the waker is RNG-free unless an actor carries the
            // latent `0x400` status, which no retail applier sets.
            self.advance_battle_mode();
            // Retail's round-end writes `ctx[+0x06] = 0x14`, and `0x14` opens
            // the `Begin | Run` prompt (`0x1E`) unconditionally. Park the
            // port's flow byte on the same state so the next party command of
            // the new round gets the prompt and the ones after it do not.
            self.set_battle_flow(crate::battle_flow::BattleFlowState::TurnPrompt);
            self.tick_status_0x400_wakes();
            // The actor sweep (`FUN_801D88CC`): action-gauge restore, the
            // `+0x1DF` action-stream clear, and the party band's stale-target
            // re-pick. Retail's flow SM runs it here, *before* the initiative
            // reseed and before the DoT tick (`FUN_801D0748` at `801d0ec4`),
            // so it goes ahead of `tick_status_effects` below and ahead of the
            // reseed the picker performs. It draws no RNG, so the reseed's
            // stream is unchanged.
            crate::battle_round::BattleRound::boundary(self);
            self.tick_status_effects();
            party_alive = (0..party_count).any(|i| !self.actor_effectively_defeated(i));
            monsters_alive = (party_count..n).any(|i| !self.actor_effectively_defeated(i));
        }

        if self.battle_ctx.action_state == ActionState::EndOfAction.as_byte()
            && party_alive
            && monsters_alive
            && let Some(next) = self.next_combatant_by_initiative()
        {
            // Start-of-turn: age this actor's buffs / debuffs, reverting any
            // that expire this turn.
            self.tick_battle_buffs_on_turn(next);
            // A Spirit guard stance lasts until the guarding actor's next
            // turn starts (the retail pending-action byte is overwritten by
            // the new command).
            if let Some(guard) = self.battle_guarding.get_mut(next as usize) {
                *guard = false;
            }
            let next_is_party = next < party_count;
            if self.actor_blocked_from_acting(next) {
                // Sleep / Stone / Faint: the actor loses its turn. Its
                // initiative key was already consumed by the picker, so the
                // next advance moves on; advancing `active_actor` also moves
                // the no-speed round-robin past it. The status duration ticks
                // once per round at the boundary above (`tick_status_effects`),
                // so the affliction still wears off. The SM stays at EndOfAction
                // (no action armed) - exactly the "skipped turn" outcome.
                self.battle_ctx.active_actor = next;
            } else if next_is_party && self.actor_is_confused(next) {
                // Confused party member: it "acts uncontrollably", so the player
                // does NOT get the command menu - auto-arm a physical strike,
                // then flip the target to a random living ally (the retarget
                // runs inside `arm_party_physical`).
                self.arm_party_physical(next);
            } else if next_is_party && self.battle_player_driven {
                // Party turn under player control: pause the SM and let the
                // player pick the command. `tick_battle_command` arms the SM
                // on confirm.
                self.open_battle_command(next);
            } else if !next_is_party {
                // Monster turn: the AI picks a spell or a physical strike.
                self.take_monster_turn(next);
            } else {
                // Party turn when not player-driven: arm a generic physical
                // attack against the first living opponent.
                self.arm_party_physical(next);
            }
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

    /// Apply one generic physical strike from the active attacker to the
    /// first living combatant on the opposing side, resolved through the
    /// retail melee roll pair
    /// ([`legaia_engine_vm::battle_formulas::physical_predamage`], the port of
    /// `FUN_801EC3E4`) - attacker ATK rolled against the defender's UDF/LDF,
    /// with the underdog rewrite that keeps a weak attacker's hit scaling
    /// instead of flooring it.
    ///
    /// The opposing side is chosen by the attacker's slot: party slots
    /// (`< party_count`) strike monsters; monster slots strike the party.
    ///
    /// REF: FUN_801EC3E4
    pub(in crate::world) fn apply_basic_attack(&mut self) {
        let attacker = self.battle_ctx.active_actor as usize;
        let party_count = self.party_count.max(1) as usize;
        // Enemy multi-action budget (AGL-driven): a monster attacker lands the
        // number of swings its per-round AGL gauge affords this turn (computed at
        // turn arm by `arm_monster_strike_budget` / `enemy_action_budget`, the
        // port of `FUN_801E9FD4`'s budget loop). A party attacker always swings
        // once here - its multi-hit is the AP / arts system. An emptied
        // opposing side ends the loop.
        let strikes = if attacker >= party_count {
            self.monster_strike_budget.max(1)
        } else {
            1
        };
        for _ in 0..strikes {
            if !self.apply_one_basic_strike(BASIC_ATTACK_COMMAND) {
                break;
            }
        }
    }

    /// Apply a single generic physical strike from the active attacker. Returns
    /// `false` when there is no living opposing target (the caller stops the
    /// multi-swing loop). See [`Self::apply_basic_attack`].
    ///
    /// `command` is the arms command this swing executes - the byte the attack
    /// chain staged out of the action queue (`0x0C`..`0x0F`). It picks the
    /// defence half (`physical_defense_is_udf`) and the command power scalar,
    /// so a low swing and a high swing resolve against different numbers.
    /// Callers with no queued byte pass [`BASIC_ATTACK_COMMAND`], the arm
    /// command, which is what the routine assumed unconditionally before the
    /// queue existed.
    ///
    /// **A melee swing always connects.** The routine that resolves a physical
    /// hit, `FUN_801EC3E4`, contains no read of the accuracy / evasion
    /// halfword `+0x168` at all - it rolls ATK against UDF/LDF and applies the
    /// HP loss. `FUN_800402F4`'s selector-9 roll, which the port used to gate
    /// this strike on, is the **queued-action interrupt** check: its success
    /// arm sets the target's `+0x16E` bit `0x4` and clears the target's
    /// pending action category, which is a stun, not a miss.
    ///
    /// Gating melee on that roll was not merely unfaithful, it inverted the
    /// fight: the engine seeds a party slot's `+0x168` from AGL (~100 at level
    /// one) and a monster's from its record INT (~12 for the opening
    /// bestiary), so `acc / (acc + eva)` gave the party an ~89% hit rate and
    /// the monsters ~11%. Enemies whiffed nine swings in ten.
    ///
    /// Retail's "Miss" on a normal attack is the limb-vs-height mismatch (an
    /// LDF-target swing at a floating enemy, a UDF-target swing at a short
    /// one - see `legaia_art::power`), which is a size-class gate the port
    /// does not model yet, not a stat roll.
    ///
    /// REF: FUN_801EC3E4 (no `+0x168` read), FUN_800402F4 (selector 9 = the
    /// action-interrupt roll, ported as `battle_formulas::accuracy_roll`)
    fn apply_one_basic_strike(&mut self, command: u8) -> bool {
        let attacker = self.battle_ctx.active_actor as usize;
        let Some(target) = self.resolve_attack_target(attacker as u8) else {
            return false;
        };
        let target = target as usize;
        let attack = self.battle_attack.get(attacker).copied().unwrap_or(0);
        let defense = self.physical_defense_of(target as u8, command);
        // Spirit guard stance on the defender (a party slot that picked
        // Spirit and hasn't started its next turn).
        let target_guarding = self.battle_guarding.get(target).copied().unwrap_or(false);
        // The melee roll pair (`FUN_801EC3E4`). This is the whole damage
        // model for a physical swing: retail's melee routine rolls attacker
        // ATK against the defender's UDF/LDF, rewrites a roll that fails to
        // clear the guard instead of flooring it, and applies the HP loss
        // itself - it does **not** run through the summon/arts finisher
        // `FUN_801DDB30`, which is why the Spirit stance arrives here as the
        // guard-roll triple rather than as the finisher's halve.
        let hp = self
            .actors
            .get(attacker)
            .map(|a| a.battle.hp)
            .unwrap_or_default();
        let hit_inputs = vm::battle_formulas::PhysicalHit {
            attacker_atk: attack,
            attacker_hp: hp,
            defender_def: defense,
            command_scalar: vm::battle_formulas::command_power_scalar(command),
            staged_anim: command,
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
            let attacker_is_party = (attacker as u8) < self.party_count;
            let target_is_party = (target as u8) < self.party_count;
            let defender_resist = self.defender_resist(target as u8);
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
        self.accrue_spirit_gauge(target as u8, dmg);
        // A petrified target (Stone) absorbs the hit - no HP loss.
        let dmg = if self.actor_is_petrified(target as u8) {
            0
        } else {
            dmg
        };
        self.apply_battle_hp_delta(target, i32::from(dmg));
        // Surface the strike for HUD damage popups.
        self.battle_hit_fx.push(BattleHitFx {
            target_slot: target as u8,
            amount: dmg,
            is_heal: false,
            is_crit: false,
        });
        // ... and its sound. The live loop resolves melee damage inline rather
        // than through the art-strike event, which is why nothing downstream of
        // `fold_battle_event` used to see a swing at all.
        self.fire_melee_impact_cue(attacker as u8, target as u8);
        if dmg > 0 {
            let survives = self.actors[target].battle.hp > 0;
            self.queue_battle_reaction(target, survives);
        }
        true
    }

    /// The melee kernel's own sound cue - retail's `FUN_801EC3E4` tail
    /// (`0x801EEB5C..0x801EEBEC`), which submits cue id [`MELEE_IMPACT_CUE`]
    /// with the **attacker slot as the category** through the battle overlay's
    /// one sound funnel `FUN_8004FE5C`
    /// ([`crate::sfx_cue::route_sfx_cue`]).
    ///
    /// This is the only `jal 0x8004fe5c` in the melee kernel, so it is the
    /// whole of a physical swing's sound, and the funnel's two legs make the
    /// two sides of a fight sound different by construction:
    ///
    /// * a **party** attacker (`category < 3`) takes the CD-XA voice leg -
    ///   `0x10C` resolves to clip `26` / channel `4`, i.e. `XA27`. Boot stages
    ///   only `XA2`/`XA4`/`XA6`, so that clip is not resident and the port has
    ///   no carrier for a raw (clip, channel, duration) request either. The
    ///   route is run and its outcome discarded rather than faked: a party
    ///   swing stays silent, and that is a *staging* gap, not a missing
    ///   producer.
    /// * a **monster** attacker takes the element-tinted high leg, which
    ///   enqueues ring id `0x10C + 0x19C = 0x2A8`. That one has a carrier -
    ///   [`World::battle_sfx_cues`], the queue both hosts drain into their SFX
    ///   scheduler - so it is pushed.
    ///
    /// Two retail gates, one modelled and one not. Modelled: the target must be
    /// playing a plain action-table clip (`+0x1D9 < 0x10`, `0x801EEB88`), so a
    /// hit landing during an art-bank animation is silent. Not modelled:
    /// `_DAT_8007BD84`, the word that selects between this cue and the
    /// per-character XA30 grunt above it (`0x801EEAC0` / `0x801EEB60`) - it has
    /// no engine model and no other reader, so the port always takes the cue
    /// arm.
    ///
    /// The ring is transient by design. Its slots are drained in retail by
    /// `FUN_80016B6C`, which the port does not model (the hosts' own SFX
    /// scheduler is the drain), and the only state a persistent ring would
    /// carry across calls is the `last_played` dedupe word that same drainer
    /// maintains - so a stored ring would sit at zero and dedupe nothing.
    ///
    /// PORT: FUN_801EC3E4 (`0x801EEBD8..0x801EEBEC`, the cue-submit site)
    fn fire_melee_impact_cue(&mut self, attacker: u8, target: u8) {
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
        let category = self.retail_actor_category(attacker);
        let element_of = |cat: u8| {
            let slot = self.engine_slot_of_retail_category(cat);
            self.battle_slot_element(slot).unwrap_or(NEUTRAL_ELEMENT)
        };
        // The `0x800788B8` per-clip duration table is not parsed; it is read
        // only on the XA leg, whose request is discarded below.
        let xa_duration_raw = |_: u32| 0u16;
        let src = crate::sfx_cue::SfxCueSources {
            element_of: &element_of,
            xa_duration_raw: &xa_duration_raw,
            tutorial_active: self.battle_tutorial.is_some(),
            cd_read_busy: false,
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

    #[test]
    fn a_monster_swing_enqueues_the_melee_impact_cue() {
        let mut w = duel();
        w.battle_ctx.active_actor = 1; // the monster attacks
        assert!(w.apply_one_basic_strike(BASIC_ATTACK_COMMAND));
        let cues = w.drain_battle_sfx_cues();
        assert_eq!(cues.len(), 1, "one swing, one cue: {cues:?}");
        // The funnel's element-tinted high leg: `0x10C + 0x19C`.
        assert_eq!(cues[0].kind, 0x2A8);
        assert_eq!(cues[0].actor_slot, 1);
        assert_eq!(cues[0].target_slot, 0);
    }

    #[test]
    fn a_party_swing_takes_the_xa_leg_and_enqueues_nothing() {
        let mut w = duel();
        w.battle_ctx.active_actor = 0; // the party member attacks
        assert!(w.apply_one_basic_strike(BASIC_ATTACK_COMMAND));
        assert!(
            w.drain_battle_sfx_cues().is_empty(),
            "a party attacker's `0x10C` is a CD-XA voice request, not a ring id"
        );
    }

    #[test]
    fn a_target_playing_an_art_bank_clip_is_silent() {
        let mut w = duel();
        w.battle_ctx.active_actor = 1;
        // Retail gate `0x801EEB88`: the cue is submitted only while the target
        // is playing a plain action-table clip.
        w.actors[0].battle.current_anim = 0x11;
        assert!(w.apply_one_basic_strike(BASIC_ATTACK_COMMAND));
        assert!(w.drain_battle_sfx_cues().is_empty());
    }
}

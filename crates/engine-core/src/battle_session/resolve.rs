//! Resolve-phase action-SM driver: builds the per-slot resolve queue, arms
//! and steps the battle action SM one attacker at a time, applies clean-room
//! swing damage, and runs end-of-round bookkeeping + wipe detection.

use super::*;

impl BattleSession {
    /// Build the [`ResolveDriver`] queue from the freshly-committed
    /// resolved-action queues. One entry per live, non-blocked party slot
    /// whose resolved queue is non-empty; entries are emitted in slot
    /// order so the action SM drives slot 0 → 1 → 2.
    ///
    /// MVP routing: a slot with at least one chained art is routed through
    /// `TacticalArts`; otherwise the slot drives the standard `Attack`
    /// category. Slots with no resolved actions don't enter the queue
    /// (they tick by without swinging). Engines that need finer-grained
    /// category routing once the magic / item paths land in the SM can
    /// extend [`ResolveSlot::category`] off the runner's resolved queues.
    pub(super) fn install_resolve_queue(&mut self, world: &World) {
        let mut queue: Vec<ResolveSlot> = Vec::new();
        for slot in 0u8..3 {
            // Skip empty / dead / blocked slots.
            if self.slots[slot as usize].record.is_none() {
                continue;
            }
            let alive = world
                .actors
                .get(slot as usize)
                .is_some_and(|a| a.battle.hp > 0);
            if !alive {
                continue;
            }
            if let Some(round) = self.round.as_ref()
                && round.action_blocked[slot as usize]
            {
                continue;
            }
            let Some(resolved) = self.runner.resolved_queue(slot) else {
                continue;
            };
            if resolved.is_empty() {
                continue;
            }
            let category = if resolved
                .actions()
                .iter()
                .any(|a| matches!(a, ActionConstant::RegularStarter))
            {
                ActionCategory::TacticalArts.as_byte()
            } else {
                ActionCategory::Attack.as_byte()
            };
            queue.push(ResolveSlot { slot, category });
        }
        self.resolve_driver = Some(ResolveDriver {
            queue,
            pos: 0,
            armed: false,
        });
    }

    /// Drive the action SM for the head-of-queue attacker by exactly one
    /// `world.tick()`. Routes the SM's per-frame `StepOutcome` into HUD
    /// damage popups, advances to the next attacker on `EndOfAction`, and
    /// transitions to `RoundOutro` when the queue empties.
    pub(super) fn step_resolve(&mut self, world: &mut World, out: &mut Vec<SessionEvent>) {
        // Re-arm gate. We always drain world events first so any side-effects
        // from the previous frame's SM step land in the HUD before this
        // frame's tick.
        self.drain_world_events(world, out);

        // Borrow-check dance: pull the head entry out of the driver before
        // we touch `world.actors` mutably below.
        let head = match self.resolve_driver.as_ref() {
            Some(d) if d.pos < d.queue.len() => d.queue[d.pos],
            Some(_) => {
                // Queue drained - finish Resolve.
                self.resolve_driver = None;
                self.transition_emit(BattlePhase::RoundOutro, out);
                return;
            }
            None => return,
        };

        // Arm the action SM for `head` on first entry. Picks the first
        // alive monster as `active_target`; if no monsters remain, the
        // driver short-circuits to the end.
        let driver_armed = self
            .resolve_driver
            .as_ref()
            .map(|d| d.armed)
            .unwrap_or(false);
        if !driver_armed {
            let monster_target = self.first_alive_monster_slot(world);
            let Some(target) = monster_target else {
                // No monsters left - drop the rest of the queue and let
                // the action SM observe MonsterWipe on its next tick.
                if let Some(d) = self.resolve_driver.as_mut() {
                    d.pos = d.queue.len();
                }
                return;
            };
            self.arm_action_sm(world, head, target);
            if let Some(d) = self.resolve_driver.as_mut() {
                d.armed = true;
            }
        }

        // One frame of the action SM.
        let outcome = world.tick();

        // Render-side ADVANCE_DONE clear: in retail the renderer clears
        // this bit when the recovery animation finishes; we mirror the
        // same edge inline since the session doesn't render.
        let active_idx = world.battle_ctx.active_actor as usize;
        if active_idx < world.actors.len()
            && world.actors[active_idx]
                .battle
                .flag_bits
                .has(ActorFlags::ADVANCE_DONE)
            && world.battle_ctx.action_state == ActionState::AttackRecovery.as_byte()
        {
            world.actors[active_idx]
                .battle
                .flag_bits
                .clear(ActorFlags::ADVANCE_DONE);
        }

        // Damage hook: AttackChain → AttackRecovery is the canonical
        // strike-landed transition. Apply clean-room formula damage to
        // the attacker's `active_target`.
        if let Some(StepOutcome::Transition { from, to }) = outcome
            && from == ActionState::AttackChain.as_byte()
            && to == ActionState::AttackRecovery.as_byte()
        {
            self.apply_session_swing(world, head.slot, out);
        }

        // The action SM raised `BattleComplete` (which fires once the
        // BattleEnd event has been pushed for the wipe-detected side).
        // Drain the resulting events so the session emits a
        // `BattleEnded` event + transitions to the terminal phase.
        if matches!(outcome, Some(StepOutcome::BattleComplete)) {
            self.drain_world_events(world, out);
            self.resolve_driver = None;
            return;
        }

        // EndOfAction means this attacker is done. Pop and re-arm next
        // frame.
        if world.battle_ctx.action_state == ActionState::EndOfAction.as_byte()
            && let Some(d) = self.resolve_driver.as_mut()
        {
            d.pos += 1;
            d.armed = false;
        }
    }

    /// First alive monster slot index (3..=7), `None` if every monster
    /// slot is dead / empty.
    fn first_alive_monster_slot(&self, world: &World) -> Option<u8> {
        let end = (3 + self.monster_count).min(8);
        (3u8..end).find(|&i| {
            world
                .actors
                .get(i as usize)
                .is_some_and(|a| a.battle.hp > 0)
        })
    }

    /// Arm the action SM for `head.slot` against `target_slot`.
    ///
    /// Mirrors what `enter_battle` does in the end-to-end test, plus the
    /// per-slot `actor.action_category` byte that drives `ActionSeed`.
    fn arm_action_sm(&self, world: &mut World, head: ResolveSlot, target_slot: u8) {
        world.battle_ctx.active_actor = head.slot;
        world.battle_ctx.queued_action = head.category;
        world.battle_ctx.action_state = ActionState::Begin.as_byte();
        if let Some(actor) = world.actors.get_mut(head.slot as usize) {
            actor.battle.action_category = head.category;
            actor.battle.active_target = target_slot;
        }
    }

    /// Clean-room damage roll on AttackChain → AttackRecovery.
    ///
    /// Reads attacker `atk` + target `udf` off [`Self::round.stats`]
    /// (computed by [`BattleRound::begin`]), runs an accuracy roll, then
    /// folds the clean-room PSY-Q variance into the raw damage. Writes
    /// the result back into the target's `BattleActor::hp`, pushes a HUD
    /// popup, and emits [`SessionEvent::HpChanged`].
    fn apply_session_swing(
        &mut self,
        world: &mut World,
        attacker: u8,
        out: &mut Vec<SessionEvent>,
    ) {
        let target_slot = match world.actors.get(attacker as usize) {
            Some(a) => a.battle.active_target,
            None => return,
        };
        if (target_slot as usize) >= world.actors.len() {
            return;
        }
        if world.actors[target_slot as usize].battle.hp == 0 {
            return;
        }
        let (atk, target_def, acc, eva) = match self.round.as_ref() {
            Some(round) => (
                round.stats[attacker as usize].atk as i32,
                round.stats[target_slot as usize].udf as i32,
                round.stats[attacker as usize].acc,
                round.stats[target_slot as usize].eva,
            ),
            None => (30, 10, 80, 20),
        };
        if !accuracy_roll(acc, eva, &mut self.rng_seed) {
            self.hud
                .push_log(format!("Miss slot {target_slot}"), LogAccent::Neutral);
            return;
        }
        let raw = (atk * 2 - target_def).max(1);
        let var = (psyq_rand_step(&mut self.rng_seed) as i32 % 25) - 12;
        // Roll the damage as usual (RNG unaffected), then absorb it if the
        // target is petrified (Stone can't be damaged).
        let dmg = if world.actor_is_petrified(target_slot) {
            0
        } else {
            (raw + raw * var / 100).clamp(1, 0xFFFF) as u16
        };
        let target = &mut world.actors[target_slot as usize].battle;
        target.hp = target.hp.saturating_sub(dmg);
        if target.hp == 0 {
            target.liveness = 0;
        }
        self.hud.push_damage(target_slot, dmg);
        self.hud
            .push_log(format!("-{dmg} HP slot {target_slot}"), LogAccent::Party);
        out.push(SessionEvent::HpChanged {
            slot: target_slot,
            amount: dmg,
            is_heal: false,
        });
    }

    /// Drive end-of-round logic. Calls [`BattleRound::end`] for tick damage,
    /// counts wipes, transitions to a terminal phase or back to RoundIntro.
    pub(super) fn end_round_and_check_wipe(
        &mut self,
        world: &mut World,
        out: &mut Vec<SessionEvent>,
    ) {
        // Drain tick damage from status effects.
        let tick_deaths = self.runner.end_round(world);
        if tick_deaths > 0 {
            self.hud.push_log(
                format!("{tick_deaths} died from status"),
                LogAccent::Highlight,
            );
        }
        // Re-sync HUD HP / status icons after tick damage.
        for i in 0..self.slots.len() {
            if let Some(actor) = world.actors.get(i) {
                let info = &self.slots[i];
                if info.name.is_empty() && info.record.is_none() {
                    continue;
                }
                let ap = if info.is_party && i < 3 {
                    Some(&world.ap_gauges[i])
                } else {
                    None
                };
                self.hud.sync_slot(
                    i as u8,
                    SlotSyncInfo {
                        name: &info.name,
                        is_party: info.is_party,
                        alive: actor.battle.hp > 0,
                        hp: actor.battle.hp,
                        hp_max: actor.battle.max_hp,
                        mp: actor.battle.mp,
                        mp_max: info.mp_max,
                        ap,
                    },
                );
                self.hud.sync_status(i as u8, &world.status_effects);
            }
        }
        self.turn = self.turn.saturating_add(1);
        // Wipe detection: party = slots 0..=2; monsters = slots 3..3+count.
        // A petrified actor (Stone) counts as defeated even at full HP.
        let party_alive = (0..3).filter(|i| self.slots[*i].record.is_some()).any(|i| {
            world.actors.get(i).is_some_and(|a| a.battle.hp > 0)
                && !world.actor_is_petrified(i as u8)
        });
        let monsters_alive = (0..self.monster_count as usize).any(|i| {
            world.actors.get(3 + i).is_some_and(|a| a.battle.hp > 0)
                && !world.actor_is_petrified((3 + i) as u8)
        });
        if !party_alive {
            self.handle_battle_end(BattleEndCause::PartyWipe, out);
        } else if !monsters_alive {
            self.handle_battle_end(BattleEndCause::MonsterWipe, out);
        } else {
            // Round complete; loop back through intro splash for the next.
            self.runner.begin_round(
                world,
                &self.per_slot_records,
                &self.equipment,
                &self.modifiers,
            );
            // (BattleRound::begin already ran inside the runner; we keep
            // `self.round` updated for future renders.)
            // Advance phase.
            self.transition_emit(BattlePhase::RoundIntro, out);
        }
    }

    pub(super) fn handle_battle_end(&mut self, cause: BattleEndCause, out: &mut Vec<SessionEvent>) {
        let next = match cause {
            BattleEndCause::PartyWipe => BattlePhase::Defeat,
            BattleEndCause::MonsterWipe => BattlePhase::Victory,
            BattleEndCause::Escaped => BattlePhase::Escaped,
        };
        out.push(SessionEvent::BattleEnded { cause });
        self.transition_emit(next, out);
    }
}

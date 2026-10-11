//! Arming a turn's action: the monster turn entry, the AI companion, the party
//! and monster physical arms, the action-stream clear and the strike budget.

use super::*;

impl World {
    pub(in crate::world) fn take_monster_turn(&mut self, slot: u8) {
        use legaia_engine_battle_vm::battle_action::ActionState;

        self.battle_ctx.active_actor = slot;
        match self.pick_monster_action(slot) {
            // A silenced/petrified caster can't cast - fall back to a physical
            // strike (mirrors the affordability fallback below).
            MonsterAction::Cast { .. } if self.actor_blocked_from_magic(slot) => {
                self.arm_monster_physical(slot);
            }
            MonsterAction::Cast {
                spell_id,
                mut targets,
            } => {
                // The turn picker's monster arm runs the dead-target redirect
                // after the AI pick for every category, not just strikes
                // (`jal 0x801DB124` at `0x801DAF50`, no gate in front of it).
                // A cast is category `2` with the spell id in `+0x1DF`; the
                // kernel re-rolls a fallen single target on its own side
                // whenever the spell's class byte is `>= 0x0A`, which every
                // monster-reachable record is. An all-side class (`8` / `9`)
                // fails the kernel's `target < 8` test and is left alone -
                // the engine's resolved list already holds only the living.
                if let [single] = targets.as_mut_slice()
                    && *single != slot
                {
                    *single = self.redirect_dead_battle_target(*single, 2, spell_id);
                }
                // A confused caster's spell lands on the opposite side.
                self.confuse_retarget_cast(slot, &mut targets);
                // The seed is spent by the cast it names.
                if self.battle.forced_monster_cast == Some((slot, spell_id)) {
                    self.battle.forced_monster_cast = None;
                }
                let def = self.monster_cast_def(spell_id);
                let mp = self
                    .actors
                    .get(slot as usize)
                    .map(|a| a.battle.mp)
                    .unwrap_or(0);
                if let Some(def) = def
                    && mp >= u16::from(def.mp_cost)
                {
                    // The cast is the action SM's Magic band, as in retail:
                    // `0x28` faces the target, raises the monster-only
                    // spell-name label and debits the MP, `0x29` waits and
                    // stages the cast clip, and the outcome folds the frame
                    // the band leaves `0x29` (`World::settle_cast_band`) -
                    // the impact-status and AGL procs ride the same fold.
                    self.arm_monster_cast(slot, &def, targets);
                    return;
                }
                // No catalog entry / unaffordable after the pick - fall
                // through to a physical strike.
                self.arm_monster_physical(slot);
            }
            MonsterAction::Physical { target } => {
                // AGL-driven multi-action budget: how many swings this monster
                // lands this turn (single swing when it has no AGL / swing data).
                self.arm_monster_strike_budget(slot);
                self.clear_action_stream(slot);
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
                // The turn picker's monster arm runs the dead-target redirect
                // straight after the AI picker, unconditionally
                // (`jal 0x801E9FD4` then `jal 0x801DB124` at `0x801DAF48..
                // 0x801DAF50`): a strike aimed at a party member who has
                // fallen since the pick re-rolls a living one, so the
                // approach never walks at a corpse (the short step `0x19`
                // has no timeout).
                let target = self.redirect_dead_battle_target(target, 3, 0);
                let picks = std::mem::take(&mut self.battle.monster_strike_entries);
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                    // The picker's physical branch writes its budget picks -
                    // archive entry indices - into the stream `+0x1DF..`
                    // (`FUN_801E9FD4`), and the strike loop stages each as its
                    // own swing clip whose hit events carry the swing's power
                    // byte (`super_queue_replace_*` holds a Gobu Gobu's
                    // `09 09 08`). An empty stream sent every AI strike down
                    // the clip-less immediate path: no swing clip, every hit
                    // in one tick, and the block roll keyed on the generic
                    // command byte `0x0C`, whose scalar is the largest.
                    let n = picks.len().min(a.battle.params.len().saturating_sub(1));
                    a.battle.params[..n].copy_from_slice(&picks[..n]);
                    a.battle.params[n] = 0;
                }
                // The picker's counter roll follows the redirect
                // (`0x801DAF74..0x801DB050`, `World::roll_counterattack`).
                self.roll_counterattack(slot, target);
                self.maybe_confuse_retarget(slot);
            }
            MonsterAction::Flee => {
                // The picker's flee checkpoint fired: arm the Run band
                // (category 5). The action SM's seed routes a monster-slot
                // category-5 actor to the state-0x68 leave-battle arm
                // (`ActionState::CaptureStart` and siblings), which plays the
                // break-off and removes the monster from the pool.
                self.battle_ctx.queued_action = 5;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
            }
        }
    }

    /// Arm a generic physical strike for party member `slot` against the first
    /// living opponent, then apply any [`Self::maybe_confuse_retarget`]. Shared
    /// by the non-player-driven party turn and the confused-party turn (a
    /// confused member can't be controlled, so it auto-acts and the retarget
    /// flips its strike to a random living ally). No-op retarget when the member
    /// isn't confused, so the auto-resolve path is RNG-unchanged.
    /// Retail's delegated pick for the AI-controlled party member -
    /// `FUN_801EED1C`'s `DAT_8007BD10[slot] == 4` block, ported as
    /// [`vm::battle_action::ai_companion_pick`].
    ///
    /// Returns `false` when this slot is not the AI companion, so
    /// [`Self::arm_party_physical`] falls through to the plain auto-swing it
    /// has always used for the rest of the party.
    ///
    /// Two deviations from the bytes, both because retail's block was written
    /// for one seating and the engine's roster is not fixed to it:
    ///
    /// * retail writes battle seat `1` unconditionally
    ///   ([`vm::battle_action::AI_COMPANION_SEAT`]); the port writes the slot
    ///   whose roster character id is `4`, which is the same seat whenever
    ///   the companion is seated where retail seats it;
    /// * retail's target redraw loop is unbounded, so a row with no living
    ///   monster spins; the port bounds it and stands by.
    ///
    /// The watched seat is retail's own: battle seat `0`, the party leader,
    /// **not** the companion itself.
    fn arm_ai_companion(&mut self, slot: u8) -> bool {
        use legaia_engine_battle_vm::battle_action::ActionState;
        use legaia_engine_battle_vm::battle_action::{
            AI_COMPANION_CHAR_ID, AI_COMPANION_MAGIC_SUB_ROUTE, AI_COMPANION_WATCHED_SEAT,
            AiCompanionPick, AiCompanionWatch, ai_companion_pick,
        };
        if self.party_roster_slot(slot as usize) as u8 + 1 != AI_COMPANION_CHAR_ID {
            return false;
        }
        let watch = self
            .actors
            .get(AI_COMPANION_WATCHED_SEAT as usize)
            .map(|a| AiCompanionWatch {
                hp: a.battle.hp,
                hp_max: a.battle.max_hp,
                status: a.battle.field_flags,
            })
            .unwrap_or_default();
        let table = self
            .actors
            .len()
            .min(crate::world::battle::cast_band::BATTLE_TABLE_SLOTS);
        let monster_count = (self.party.party_count as usize..table)
            .filter(|&s| self.actors[s].active)
            .count() as u8;
        let alive: Vec<bool> = (0..table).map(|s| self.actors[s].battle.hp != 0).collect();
        let guard: Vec<u8> = (0..table)
            .map(|s| {
                self.actors[s]
                    .battle_monster_id
                    .and_then(|id| self.tables.monster_catalog.get(id))
                    .map(|d| d.swing_class)
                    .unwrap_or(0)
            })
            .collect();
        let mut rolls: Vec<u32> = Vec::new();
        for _ in 0..(2 + legaia_engine_battle_vm::battle_action::AI_COMPANION_MAX_TARGET_DRAWS + 2)
        {
            rolls.push(self.next_rand());
        }
        let mut cursor = rolls.into_iter();
        let pick = ai_companion_pick(
            watch,
            monster_count,
            |s| alive.get(s as usize).copied().unwrap_or(false),
            |s| guard.get(s as usize).copied().unwrap_or(0),
            || cursor.next().unwrap_or(0),
        );

        self.battle_ctx.active_actor = slot;
        self.clear_action_stream(slot);
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
        match pick {
            AiCompanionPick::Magic { spell } => {
                self.battle_ctx.queued_action = 2;
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 2;
                    a.battle.active_target = 0;
                    if let Some(p) = a.battle.params.first_mut() {
                        *p = spell;
                    }
                    a.battle.sub_route = AI_COMPANION_MAGIC_SUB_ROUTE;
                }
            }
            AiCompanionPick::Attack { target, commands } => {
                self.battle_ctx.queued_action = 3;
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 3;
                    a.battle.active_target = target;
                    for (i, cmd) in commands.iter().enumerate() {
                        if let (Some(c), Some(p)) = (cmd, a.battle.params.get_mut(i)) {
                            *p = *c;
                        }
                    }
                }
                self.maybe_confuse_retarget(slot);
            }
            AiCompanionPick::StandBy => {
                self.battle_ctx.queued_action = 0;
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 0;
                }
            }
        }
        true
    }

    pub(in crate::world) fn arm_party_physical(&mut self, slot: u8) {
        use legaia_engine_battle_vm::battle_action::ActionState;
        if self.arm_ai_companion(slot) {
            return;
        }
        let target = self.first_living_opponent_of(slot).unwrap_or(slot);
        self.battle_ctx.active_actor = slot;
        self.clear_action_stream(slot);
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = target;
            a.battle.action_category = 3;
        }
        // Same swing stream the player-driven Attack confirm seeds - this is
        // the path retail's no-input queue arm was written for in the first
        // place (`FUN_801EED1C` selects it on the slot's AI control byte
        // `(&DAT_8007BD10)[slot] == 4`). Without it the attack band reads its
        // terminator on byte 0 and the turn passes without a swing.
        self.seed_basic_attack_queue(slot, target);
        self.maybe_confuse_retarget(slot);
    }

    /// Zero an actor's action-parameter stream (`+0x1DF..+0x1EE`) and rewind
    /// its strike cursor, the state a freshly-armed action starts from.
    ///
    /// Retail re-seeds the stream per action (`FUN_801EED1C` for a party
    /// action, the AI picker for a monster one) and clears it wholesale at the
    /// round boundary (`FUN_801D88CC` loop A, ported as
    /// [`crate::battle_round::BattleRound::boundary`]), so no action ever
    /// reads a byte another action left behind. The engine had no per-action
    /// clear, and the gap was a **soft-lock**: the monster-AI picker writes
    /// the chosen spell id into `params[0]` before the cast is folded, and a
    /// cast that cannot fold (no catalog entry for the id - the default on a
    /// host with no spell catalog) falls back to a physical strike that then
    /// walked the spell id as if it were a swing-anim byte. The attack chain
    /// staged it, the staged id happened to equal the actor's current anim id,
    /// the anim commit's `q == current_anim` early-out skipped the
    /// `ADVANCE_DONE` clear, and the chain's pacing gate held at `AttackChain`
    /// (`0x1E`) for the rest of the session.
    ///
    /// REF: FUN_801D88CC (the round-boundary clear this is the per-action
    /// sibling of)
    /// REF: FUN_801EED1C (the party-side per-action seeder whose absence is
    /// what leaves a stale stream behind)
    pub(in crate::world) fn clear_action_stream(&mut self, slot: u8) {
        let len = legaia_engine_battle_vm::battle_formulas::ACTION_STREAM_RANGE.len();
        if let Some(a) = self.actors.get_mut(slot as usize) {
            for b in a.battle.params.iter_mut().take(len) {
                *b = 0;
            }
            a.battle.strike_index = 0;
            // The staged Tactical-Arts profile belongs to the stream that is
            // being cleared - keeping it would re-key the next action's
            // strikes to the previous turn's art.
            a.battle.clear_art_profile();
            // Retail clears the side array `0x801F6990` at the queue
            // builder's own head (the 16-word `sw zero` loop at
            // `0x801EED5C..0x801EED74`), i.e. exactly when the stream it
            // describes is replaced. This is that seam: every armer runs it
            // and the arts armer re-installs the marks straight after.
            a.battle.starter_marks = None;
        }
    }

    /// Arm a generic physical strike for monster `slot` against the first
    /// living party member (fallback when a picked cast can't fold).
    fn arm_monster_physical(&mut self, slot: u8) {
        use legaia_engine_battle_vm::battle_action::ActionState;
        self.arm_monster_strike_budget(slot);
        self.clear_action_stream(slot);
        let target = self.first_living_opponent_of(slot).unwrap_or(slot);
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
        let picks = std::mem::take(&mut self.battle.monster_strike_entries);
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = target;
            a.battle.action_category = 3;
            // The picked attack entries into the stream (retail: the AI
            // picker's physical branch writes its picks to `+0x1DF..`), the
            // `0x00` terminator kept inside the window.
            let n = picks.len().min(a.battle.params.len().saturating_sub(1));
            a.battle.params[..n].copy_from_slice(&picks[..n]);
            a.battle.params[n] = 0;
        }
        self.maybe_confuse_retarget(slot);
    }

    /// Compute + store the AGL-driven multi-action budget for the physical swing
    /// monster `slot` is about to make - the enemy analogue of the party Arts AP
    /// gauge. Port of the AGL-gauge spending loop in the picker
    /// `FUN_801E9FD4`: the monster gets one swing per action its per-round AGL
    /// gauge ([`legaia_engine_battle::monster_catalog::MonsterDef::agl`]) can afford from its
    /// physical swing costs (`action_costs`), capped at 15, via
    /// [`vm::battle_action::enemy_action_budget`]. Draws battle RNG (one roll per
    /// candidate pick) exactly as retail's picker does.
    ///
    /// Falls back to a single swing - drawing **no** RNG - when the monster has
    /// no AGL gauge or no costed swing actions (the disc-free / synthetic
    /// catalog), so unbudgeted battles keep their RNG stream and behaviour
    /// bit-identical. The result is consumed by [`Self::apply_basic_attack`].
    ///
    /// PORT: FUN_801E9FD4
    pub(in crate::world) fn arm_monster_strike_budget(&mut self, slot: u8) {
        let (catalog_agl, costs, entries) = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.tables.monster_catalog.get(id))
            .map(|d| (d.agl, d.action_costs.clone(), d.action_entries.clone()))
            .unwrap_or((0, Vec::new(), Vec::new()));
        self.battle.monster_strike_entries.clear();
        let cut = std::mem::take(&mut self.battle.monster_ai_state.cut_strike_stream);
        // The gauge retail spends is the actor's **live** `+0x154`, which the
        // round boundary (`BattleRound::boundary`, the port of `FUN_801D88CC`)
        // restores from `+0x156` once per round. A slot whose base `+0x156` was
        // never seeded is not on that maintenance path at all - a synthetic
        // battle assembled without the formation seeder - so it keeps reading
        // the catalog stat directly and behaves exactly as it did before the
        // gauge existed.
        let base_seeded = self
            .actors
            .get(slot as usize)
            .is_some_and(|a| a.battle.agl_base != 0);
        let agl = if base_seeded {
            self.actors[slot as usize].battle.agl
        } else {
            catalog_agl
        };
        self.battle.monster_strike_budget = if agl > 0 && !costs.is_empty() {
            let stream = legaia_engine_battle_vm::battle_action::enemy_action_budget(
                agl,
                &costs,
                &mut || self.next_rand(),
            );
            // Retail's budget loop spends the gauge it walked. Only do so on
            // the SPD-seeded path: the round-robin fallback has no round
            // boundary to restore it, so an unrestorable gauge would drain to
            // a single swing and stay there.
            if base_seeded && self.any_battle_speed() {
                let spent: u16 = stream
                    .iter()
                    .map(|&pick| u16::from(costs.get(pick as usize).copied().unwrap_or(0)))
                    .sum();
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.agl = a.battle.agl.saturating_sub(spent);
                }
            }
            // The picks themselves, as archive entry indices: retail's
            // picker writes them into the monster's action stream, and the
            // attack band stages each one as its own clip (the hit events
            // of that entry are the swing's damage). Without the aligned
            // entry list (the synthetic catalog) the stream stays empty and
            // the budget count drives immediate swings instead.
            self.battle.monster_strike_entries = stream
                .iter()
                .filter_map(|&pick| entries.get(pick as usize).copied())
                .collect();
            // The `0xB3` arm's cut: the stream keeps its first entry only
            // (retail zeroes `+0x1E0..+0x1ED` after the picks are written).
            if cut {
                self.battle.monster_strike_entries.truncate(1);
                1
            } else {
                (stream.len() as u8).max(1)
            }
        } else {
            1
        };
    }
}

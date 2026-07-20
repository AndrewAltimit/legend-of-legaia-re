//! The per-frame live battle loop, basic-attack strike, target resolution, and
//! status-block / defeat predicates (incl. the Final Heal revive sweep). Split
//! out of `battle.rs` as additional `impl World` blocks; no logic change from
//! the original inline definitions.

use super::*;

impl World {
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
            a.hp = max_hp;
            a.liveness = 1;
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

        // Sparring tutorial: a prompt box on screen parks the entire battle -
        // retail's `FUN_801D0748` returns before it reads the flow state when
        // `FUN_801D9BBC` reports a box up (`ctx[+0x6B2]`).
        // REF: FUN_801D0748, FUN_801D9BBC
        if self.battle_tutorial.is_some() && self.tick_battle_tutorial_boxes() {
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

        // Final Heal sweep (FUN_801e6968): retail runs it in the cleanup
        // state 0x50 *before* the liveness count resolves a wipe. Run it
        // before the SM step so a party member downed late last tick (a
        // monster cast / DoT) is revived before this step's wipe scan, and
        // again after this tick's damage lands (below).
        self.apply_final_heal_revives();

        let outcome = self.step_battle();

        // Apply this step's damage events (art strikes carry a damage value;
        // the loop owns folding while live, so events are consumed here).
        let events = std::mem::take(&mut self.pending_battle_events);
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

        // Generic physical attack: deal damage on the strike-landed edge when
        // no art strike already did.
        if let StepOutcome::Transition { from, to } = outcome
            && from == ActionState::AttackChain.as_byte()
            && to == ActionState::AttackRecovery.as_byte()
            && !art_strike_applied
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
        let attacker = self.battle_ctx.active_actor as usize;
        if attacker < self.actors.len()
            && self.actors[attacker]
                .battle
                .flag_bits
                .has(ActorFlags::ADVANCE_DONE)
            && self.battle_ctx.action_state == ActionState::AttackRecovery.as_byte()
        {
            self.actors[attacker]
                .battle
                .flag_bits
                .clear(ActorFlags::ADVANCE_DONE);
        }

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

        if matches!(outcome, StepOutcome::BattleComplete) {
            self.finish_battle();
        }
        Some(outcome)
    }

    /// Apply one generic physical strike from the active attacker to the
    /// first living combatant on the opposing side. v0.1 stand-in for the
    /// art-driven strike path: `damage = art_strike_damage_default(attack,
    /// defense, 16)` (≈ `attack - defense`, floored at 1) so a party with no
    /// configured weapon attack still chips the monster down - and, for a
    /// monster attacker, a monster with no configured attack still chips the
    /// party. The strike rolls against the target's evasion
    /// ([`legaia_engine_vm::battle_formulas::accuracy_roll`]); when neither the
    /// attacker's accuracy nor the target's evasion is seeded the roll auto-hits
    /// and consumes no RNG, so the unseeded synthetic loop still resolves
    /// exactly as before.
    ///
    /// The opposing side is chosen by the attacker's slot: party slots
    /// (`< party_count`) strike monsters; monster slots strike the party.
    pub(in crate::world) fn apply_basic_attack(&mut self) {
        let attacker = self.battle_ctx.active_actor as usize;
        let party_count = self.party_count.max(1) as usize;
        // Enemy multi-action budget (AGL-driven): a monster attacker lands the
        // number of swings its per-round AGL gauge affords this turn (computed at
        // turn arm by `arm_monster_strike_budget` / `enemy_action_budget`, the
        // port of `FUN_801E9FD4`'s budget loop). A party attacker always swings
        // once here - its multi-hit is the AP / arts system. A miss doesn't end
        // the turn (the loop continues); an emptied opposing side does.
        let strikes = if attacker >= party_count {
            self.monster_strike_budget.max(1)
        } else {
            1
        };
        for _ in 0..strikes {
            if !self.apply_one_basic_strike() {
                break;
            }
        }
    }

    /// Apply a single generic physical strike from the active attacker. Returns
    /// `false` when there is no living opposing target (the caller stops the
    /// multi-swing loop); a plain accuracy miss returns `true` (the turn's
    /// remaining swings still happen). See [`Self::apply_basic_attack`].
    fn apply_one_basic_strike(&mut self) -> bool {
        let attacker = self.battle_ctx.active_actor as usize;
        let Some(target) = self.resolve_attack_target(attacker as u8) else {
            return false;
        };
        let target = target as usize;
        let attack = self.battle_attack.get(attacker).copied().unwrap_or(0);
        let defense = self.battle_defense.get(target).copied().unwrap_or(0);
        let acc = self.battle_accuracy.get(attacker).copied().unwrap_or(0);
        let eva = self.battle_evasion.get(target).copied().unwrap_or(0);
        // Roll only when the attacker's accuracy is seeded; an unseeded
        // attacker (`acc == 0`) auto-hits and consumes no RNG.
        let hit = if acc == 0 {
            true
        } else {
            let mut seed = self.next_rng();
            vm::battle_formulas::accuracy_roll(acc, eva, &mut seed)
        };
        if !hit {
            return true;
        }
        // Spirit guard stance on the defender (a party slot that picked
        // Spirit and hasn't started its next turn).
        let target_guarding = self.battle_guarding.get(target).copied().unwrap_or(false);
        let dmg = if self.use_damage_finish {
            // Raw roll BEFORE any floor (`min_floor = 0`), then run it through
            // the retail damage finisher so the universal post-stages apply.
            // The defender's equipment resist words come from the real ability
            // bitfield ([`Self::defender_resist`]) - a no-op for this path's
            // non-elemental strike, but the All-Guard gate reads them the way
            // retail does; the finisher still contributes the 9999 cap and the
            // rand-based no-damage floor. The finisher draws a rand ONLY when
            // the hit zeroes out, so draw one only then to keep the RNG
            // call-count identical to retail (and to the flat path when the
            // gate is off). Slots are classified party (`< 3`) vs enemy
            // (`>= 3`) the way the finisher expects, independent of the
            // engine's variable monster-slot base.
            let raw = vm::battle_formulas::art_strike_damage(attack, defense, 16, 16, 0);
            let floor_rand = if raw == 0 {
                (self.next_rng() & 0x7FFF) as u16
            } else {
                0
            };
            let attacker_is_party = (attacker as u8) < self.party_count;
            let target_is_party = (target as u8) < self.party_count;
            let defender_resist = self.defender_resist(target as u8);
            vm::battle_formulas::damage_finish(&vm::battle_formulas::DamageFinish {
                predamage: raw as u32,
                attacker_slot: if attacker_is_party { 0 } else { 3 },
                defender_slot: if target_is_party { 0 } else { 3 },
                attacker_element: 7, // basic attack is non-elemental
                defender_resist,
                defender_guarding: target_guarding,
                enemy_defender_halve: false,
                bypass_party_resist: false,
                summon_power_pct: 100,
                floor_rand,
            }) as u16
        } else {
            // The flat path skips the finisher, so apply its guard-halve
            // stage (`over >>= 1` when the defender guards) here so Spirit
            // still defends without `--damage-finish`.
            let flat = vm::battle_formulas::art_strike_damage_default(attack, defense, 16);
            if target_guarding { flat >> 1 } else { flat }
        };
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
        let a = &mut self.actors[target];
        a.battle.hp = a.battle.hp.saturating_sub(dmg);
        if a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
        // Surface the strike for HUD damage popups.
        self.battle_hit_fx.push(BattleHitFx {
            target_slot: target as u8,
            amount: dmg,
            is_heal: false,
            is_crit: false,
        });
        if dmg > 0 {
            let survives = self.actors[target].battle.hp > 0;
            self.queue_battle_reaction(target, survives);
        }
        true
    }

    /// Resolve the slot a strike from `attacker` should land on. Honors a
    /// pre-selected [`battle::BattleActor::active_target`] when it points at a
    /// living actor on the opposing side (so the player's target-picker choice
    /// and the monster-AI target choice both take effect), otherwise falls back
    /// to [`Self::first_living_opponent_of`].
    fn resolve_attack_target(&self, attacker: u8) -> Option<u8> {
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let (lo, hi) = if attacker < pc { (pc, n) } else { (0, pc) };
        if let Some(a) = self.actors.get(attacker as usize) {
            let t = a.battle.active_target;
            if (lo..hi).contains(&t)
                && self
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
}

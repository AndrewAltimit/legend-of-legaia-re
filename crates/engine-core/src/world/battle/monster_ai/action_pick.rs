//! The monster's action pick: its cast definition lookup, the weighted pick and
//! the battle-mode word.

use super::*;

impl World {
    /// The cast record a monster's turn arms for `spell_id`: the catalog's,
    /// or - for a capture-class special, which the catalog does not carry -
    /// one built off the disc spell table
    /// ([`legaia_engine_battle::retail_magic::capture_special_def`]). The action SM routes
    /// the latter to its capture band on the table's class byte, as retail's
    /// does; without it the pick degraded to a strike.
    pub(in crate::world) fn monster_cast_def(
        &self,
        spell_id: u8,
    ) -> Option<legaia_engine_battle::spells::SpellDef> {
        self.tables
            .spell_catalog
            .get(spell_id)
            .cloned()
            .or_else(|| {
                let table = self.menu.text.as_ref()?.spell_names.as_ref()?;
                legaia_engine_battle::retail_magic::capture_special_def(table, spell_id)
            })
    }

    /// Monster-AI action picker - the driver of `FUN_801E9FD4`
    /// (`overlay_0898_801e9fd4.txt`), the routine retail runs from the turn
    /// pick `FUN_801DABA4` for each monster turn.
    ///
    /// In retail order:
    ///
    /// 1. The generic decision core
    ///    ([`legaia_engine_battle::monster_ai::generic_core`], through
    ///    [`Self::generic_core_pick`]): the physical-or-magic gate, the
    ///    tag-`0x23` cast walk with its re-matches, the target-class arms and
    ///    the redrawn pass. A monster with no record or no spell table behind
    ///    it (the disc-free synthetic catalog) rolls one draw over
    ///    [`legaia_engine_battle::monster_catalog::MonsterDef::magic_attacks`]
    ///    instead.
    /// 2. The physical arm's swing budget
    ///    ([`Self::arm_monster_strike_budget`]), rolled in place.
    /// 3. The flee checkpoint `FUN_801EC0DC`, once per pick.
    /// 4. The per-monster `switch` and the recent-target ring.
    ///
    /// The large per-monster-id scripted-cast `switch` that follows the core in
    /// retail keys on `DAT_8007BD0C[slot]`, which `FUN_801DA51C` fills from the
    /// encounter record's `[+4 + slot]` monster ids - i.e. the **monster id**,
    /// not an abstract AI-type, so each case is bespoke AI for a specific
    /// monster the engine already identifies via `battle_monster_id`. That
    /// switch is ported in [`legaia_engine_battle::monster_ai`] ([`legaia_engine_battle::monster_ai::decide`])
    /// and consulted here as an override, followed by the post-switch
    /// recent-target ring ([`legaia_engine_battle::monster_ai::apply_recent_target_ring`]). The
    /// companion target resolver `FUN_801E7320` is ported as
    /// [`Self::resolve_monster_target`] (the `monster_setup` hook).
    ///
    /// PORT: FUN_801E9FD4
    /// REF: FUN_801DABA4, FUN_801DA51C
    pub(in crate::world) fn pick_monster_action(&mut self, slot: u8) -> MonsterAction {
        let pc = self.party.party_count.max(1);
        self.battle.monster_core_clip = None;

        // Retail-compare debug seed: a capture taken mid monster cast replays
        // that cast on the seat's next turn instead of the AI's pick.
        if let Some((seat, spell_id)) = self.battle.forced_monster_cast
            && seat == slot
            && let Some(def) = self.monster_cast_def(spell_id)
        {
            let class = self.monster_cast_target_class(slot, &def);
            let mut targets = self.resolve_class_to_slots(slot, class);
            if targets.len() == 1
                && let Some(t) = self.battle.forced_monster_target.take()
                && self.actors.get(t as usize).is_some_and(|a| a.battle.hp > 0)
            {
                targets = vec![t];
            }
            if !targets.is_empty() {
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 2;
                    a.battle.params[0] = spell_id;
                    a.battle.mp = a.battle.mp.saturating_add(u16::from(def.mp_cost));
                }
                return MonsterAction::Cast { spell_id, targets };
            }
        }

        // An expansion boss script picks for a scripted pack monster in
        // place of the retail AI (never without a pack).
        if let Some(action) = self.expansion_boss_pick(slot) {
            return action;
        }

        // --- generic decision core ---
        // The monster's own castable global magic ids (parser already drops the
        // empty `<= 1` slots, so every entry is "live").
        let magic: Vec<u8> = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.tables.monster_catalog.get(id))
            .map(|d| d.magic_attacks.clone())
            .unwrap_or_default();
        let mp = self
            .actors
            .get(slot as usize)
            .map(|a| a.battle.mp)
            .unwrap_or(0);

        let core = self.generic_core_pick(slot);
        let core_ran = core.is_some();
        let (mut category, mut spell_id, mut target_class) = match core {
            Some(picked) => picked,
            None => {
                // No record or spell table behind the monster (the disc-free
                // synthetic catalog): one roll over `1 + live_magic_count`,
                // `0` the physical strike.
                let denom = 1 + magic.len() as u32;
                let roll = self.next_rand() % denom;
                match magic.get(roll.wrapping_sub(1) as usize).copied() {
                    Some(id) if roll != 0 => match self.monster_cast_def(id) {
                        Some(def) if mp >= u16::from(def.mp_cost) => {
                            (2, id, self.monster_cast_target_class(slot, &def))
                        }
                        _ => (3, 0, self.random_living_party_member(pc).unwrap_or(slot)),
                    },
                    _ => (3, 0, self.random_living_party_member(pc).unwrap_or(slot)),
                }
            }
        };

        // The core's physical arm runs its swing-budget loop in place, ahead
        // of the flee checkpoint and the switch (`0x801EA1FC..0x801EA3CC`);
        // the strike arm takes the picks it left
        // (`BattleState::monster_budget_armed`).
        self.battle.monster_budget_armed = None;
        if core_ran && category == 3 {
            self.arm_monster_strike_budget(slot);
            self.battle.monster_budget_armed = Some(slot);
        }

        // --- the flee checkpoint (`jal 0x801EC0DC` at `0x801EA980`) ---
        // The loop exit runs it on every call of the picker - one call per
        // monster turn, from the turn pick `FUN_801DABA4` (`0x801DAF48`) -
        // on the pass that did not fail (`s8 == 0`, `0x801EA974`), and
        // before the per-monster `switch` (`0x801EA9C0`). The roll draws its
        // RNG either way; the special-battle word is tested after it
        // (`0x801EA994`), so a granted flee is dropped in an arena leg or a
        // Ra-Seru-forbidden fight. A success stores category `5`
        // (`sb v0,0x1de(s4)` at `0x801EA9A8`), which a scripted override the
        // switch then stores replaces.
        let fled = self.monster_flee_roll(slot) && self.special_battle_word() == 0;
        if fled {
            category = 5;
        }
        let mut overridden = false;

        // --- per-monster-id scripted override (the FUN_801E9FD4 switch) + the
        // post-switch recent-target anti-repeat ring. Run in a borrow window
        // with the AI state owned locally so the RNG closure can take `self`.
        if let Some(monster_id) = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
        {
            let (hp, max_hp) = self
                .actors
                .get(slot as usize)
                .map(|a| (a.battle.hp, a.battle.max_hp))
                .unwrap_or((0, 0));
            let allies_with_mp = (0..pc)
                .filter(|&i| {
                    self.actors
                        .get(i as usize)
                        .is_some_and(|a| a.battle.liveness != 0 && a.battle.mp != 0)
                })
                .count() as u8;
            // `ctx[+1]`, the seated monster count - not the actor table's
            // length, which would read every `monster_count != 4` / `!= 3`
            // scripted-cast gate as open.
            let seated = self.seated_monster_slots().count() as u8;
            let ctx = legaia_engine_battle::monster_ai::MonsterAiCtx {
                monster_id: (monster_id & 0xFF) as u8,
                monster_index: slot.saturating_sub(pc),
                caster_slot: slot,
                hp,
                max_hp,
                mp,
                party_count: pc,
                monster_count: seated.max(1),
                field_flags: self
                    .actors
                    .get(slot as usize)
                    .map(|a| a.battle.field_flags)
                    .unwrap_or(0),
                allies_with_mp,
                spirit_gauge: self
                    .actors
                    .get(slot as usize)
                    .map(|a| a.battle.spirit_gauge)
                    .unwrap_or(0),
                core_category: category,
            };
            let mut ai = std::mem::take(&mut self.battle.monster_ai_state);
            let mut spirit_writeback = None;
            if let Some(cast) =
                legaia_engine_battle::monster_ai::decide(&ctx, &mut ai, &mut || self.next_rand())
            {
                overridden = true;
                category = cast.category;
                spell_id = cast.spell_id;
                if !cast.keep_target {
                    target_class = cast.target_class;
                }
                spirit_writeback = cast.spirit_gauge_writeback;
            }
            // The 0x8A charge gate clamps the caster's own gauge as it fires
            // (`actor+0x170 = 0x32`). Applied after the RNG borrow window; it
            // draws no RNG, so the determinism stream is untouched.
            if let Some(g) = spirit_writeback
                && let Some(a) = self.actors.get_mut(slot as usize)
            {
                a.battle.spirit_gauge = g;
            }
            // Anti-repeat ring (applies to whichever single party target stands).
            target_class = legaia_engine_battle::monster_ai::apply_recent_target_ring(
                target_class,
                spell_id,
                pc,
                &mut ai,
                &mut || self.next_rand(),
            );
            self.battle.monster_ai_state = ai;
        }

        if fled && !overridden {
            if let Some(a) = self.actors.get_mut(slot as usize) {
                a.battle.action_category = 5;
            }
            self.battle.monster_budget_armed = None;
            return MonsterAction::Flee;
        }
        // Optional, gated, NON-FAITHFUL: smarter single-target selection. By
        // now every RNG draw of the faithful random pick (magic roll, target
        // roll + re-roll loop, scripted override, anti-repeat ring) is already
        // consumed, so overriding the chosen slot here does not move the RNG
        // stream. We only redirect a single living-party target (`class < pc`)
        // to the lowest-HP living member; all-party (8) / monster-band (9) /
        // self targets are left exactly as the faithful path resolved them.
        if self.run_smart_monsters()
            && target_class < pc
            && let Some(low) = self.lowest_hp_living_party_member(pc)
        {
            target_class = low;
        }

        // --- build the action ---
        if category == 2 {
            let targets = self.resolve_class_to_slots(slot, target_class);
            if !targets.is_empty() {
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 2;
                    a.battle.params[0] = spell_id;
                }
                return MonsterAction::Cast { spell_id, targets };
            }
        }
        // Physical strike (or a cast that resolved no targets).
        let target = if target_class < pc {
            target_class
        } else {
            self.random_living_party_member(pc)
                .or_else(|| self.first_living_opponent_of(slot))
                .unwrap_or(slot)
        };
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.action_category = 3;
            a.battle.active_target = target;
        }
        MonsterAction::Physical { target }
    }

    /// The picker's generic decision core
    /// ([`legaia_engine_battle::monster_ai::generic_core`]) over the monster
    /// in `slot`: its record's magic-slot window and archive entry tags, the
    /// disc spell table, the seated monsters. Returns the provisional
    /// `(category, spell, target class)` in engine numbering, and parks a
    /// cast's clips for the cast arm (`BattleState::monster_core_clip`).
    /// `None` when the monster has no record or the spell table was not read
    /// - the caller's synthetic fallback.
    ///
    /// A walk that matched nothing leaves retail's command bytes as the
    /// monster's last action left them; the port, which does not carry them,
    /// strikes a living party member instead.
    ///
    /// PORT: FUN_801E9FD4 (`0x801EA058..0x801EA974`)
    fn generic_core_pick(&mut self, slot: u8) -> Option<(u8, u8, u8)> {
        use legaia_engine_battle::monster_ai::generic_core::{
            self as core, CoreInput, CorePick, MonsterSeat, SpellBytes,
        };
        let pc = self.party.party_count.max(1);
        let def = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.tables.monster_catalog.get(id))?;
        if def.entry_tags.is_empty() {
            return None;
        }
        let table = self.menu.text.as_ref()?.spell_names.as_ref()?;
        let bytes: Vec<Option<SpellBytes>> = (0..=u8::MAX)
            .map(|id| {
                table.entry(id).map(|e| SpellBytes {
                    class: e.class,
                    target: e.target,
                    mp: e.mp,
                })
            })
            .collect();
        let monster_id = (def.id & 0xFF) as u8;
        let ai_bytes = def.ai_bytes;
        let entry_tags = def.entry_tags.clone();
        let end = self
            .seated_monster_slots()
            .last()
            .map_or(usize::from(pc), |s| s + 1);
        let monsters: Vec<MonsterSeat> = (usize::from(pc)..end)
            .map(|s| {
                let a = &self.actors[s];
                MonsterSeat {
                    hp: a.battle.hp,
                    max_hp: a.battle.max_hp,
                    monster_id: a.battle_monster_id.map_or(0, |id| (id & 0xFF) as u8),
                }
            })
            .collect();
        let companion_in_seat_1 = pc > 1
            && self.party_roster_slot(1) as u8 + 1
                == legaia_engine_battle_vm::battle_action::AI_COMPANION_CHAR_ID;
        let input = CoreInput {
            caster_index: slot.saturating_sub(pc),
            monster_id,
            round: self.battle_mode(),
            ai_bytes,
            entry_tags: &entry_tags,
            mp: self.actors.get(slot as usize).map_or(0, |a| a.battle.mp),
            party_count: pc,
            monsters: &monsters,
            companion_in_seat_1,
        };
        let spell = |id: u8| bytes[usize::from(id)];
        let (pick, _) = core::pick(&input, &spell, &mut || self.next_rand());
        Some(match pick {
            CorePick::Physical { target } => (3, 0, target),
            CorePick::Magic {
                spell,
                clip,
                spirit_clip,
                target,
            } => {
                self.battle.monster_core_clip = Some((slot, spell, clip, spirit_clip));
                let target = match target {
                    t @ 3..8 => pc + (t - 3),
                    t => t,
                };
                (2, spell, target)
            }
            CorePick::Unmatched => (3, 0, self.random_living_party_member(pc).unwrap_or(slot)),
        })
    }

    /// The live battle-mode counter (`ctx+0x28A`, `_DAT_8007BD24[0x28A]`).
    ///
    /// This is the boss/scripted-mode gate the per-monster AI `switch` reads:
    /// multi-phase bosses (`0xA8`, `0xB4`, `0xB5`, `0xB6`, `0xA2..=0xA4`, …)
    /// change which spell they cast as it advances. `0` in a normal battle.
    pub fn battle_mode(&self) -> u8 {
        self.battle.monster_ai_state.mode_flags
    }

    /// Advance the battle-mode counter by one - the faithful port of the
    /// battle-action SM's `case 0xFF` (`_DAT_8007BD24[0x28A] += 1`), the
    /// boss-phase-transition pseudo-action. A boss script issues action `0xFF`
    /// when the fight crosses a scripted phase boundary; the next monster turn's
    /// `Self::pick_monster_action` then reads the bumped mode through
    /// [`legaia_engine_battle::monster_ai::decide`], activating that phase's scripted casts.
    /// The retail counter is a byte, so it wraps at `0xFF`.
    ///
    /// PORT: FUN_801E295C
    pub fn advance_battle_mode(&mut self) {
        self.battle.monster_ai_state.mode_flags =
            self.battle.monster_ai_state.mode_flags.wrapping_add(1);
    }
}

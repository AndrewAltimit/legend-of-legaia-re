//! Arming a cast and folding its outcome: target resolution, the player and
//! monster arms, the pending-cast fold and the cast-band settle.

use super::*;
use legaia_engine_battle_vm::battle_action::{ActionCategory, ActionState, StepOutcome};

impl World {
    /// Absolute actor slots a player cast lands on, from the submenu's
    /// resolution: a single-target shape takes the picked slot (enemy rows
    /// sit behind the party), a group shape takes the whole band.
    pub(in crate::world) fn spell_targets_for(
        &self,
        def: &legaia_engine_battle::spells::SpellDef,
        target_row: legaia_engine_battle::target_picker::CursorRow,
        target_slot: u8,
    ) -> Vec<u8> {
        use legaia_engine_battle::spells::SpellTarget;
        use legaia_engine_battle::target_picker::CursorRow;
        let party_count = self.party.party_count.clamp(1, 3);
        match def.target {
            SpellTarget::OneEnemy | SpellTarget::OneAlly | SpellTarget::SelfOnly => {
                let abs = match target_row {
                    CursorRow::Enemy => party_count + target_slot,
                    CursorRow::Ally => target_slot,
                };
                vec![abs]
            }
            SpellTarget::AllEnemies => (party_count..self.actors.len() as u8).collect(),
            SpellTarget::AllAllies => (0..party_count).collect(),
        }
    }

    /// The `+0x1DD` target byte a cast of `def` onto `targets` carries: a
    /// group shape is the group code the cast-begin facing store and the
    /// cue expander decode (`8` = the party, `9` = the enemy row), a single
    /// shape is the slot itself.
    fn cast_target_code(
        &self,
        def: &legaia_engine_battle::spells::SpellDef,
        targets: &[u8],
        caster: u8,
    ) -> u8 {
        use legaia_engine_battle::spells::SpellTarget;
        use legaia_engine_battle_vm::battle_target_group::{
            TARGET_GROUP_ENEMIES, TARGET_GROUP_PARTY,
        };
        let caster_is_party = caster < self.party.party_count;
        match def.target {
            // Codes are in retail's absolute numbering: 8 = party, 9 = the
            // enemy row - whichever side the caster stands on.
            SpellTarget::AllEnemies => {
                if caster_is_party {
                    TARGET_GROUP_ENEMIES
                } else {
                    TARGET_GROUP_PARTY
                }
            }
            SpellTarget::AllAllies => {
                if caster_is_party {
                    TARGET_GROUP_PARTY
                } else {
                    TARGET_GROUP_ENEMIES
                }
            }
            _ => targets.first().copied().unwrap_or(caster),
        }
    }

    /// Arm the action SM's Magic band for `caster`'s `def` cast onto
    /// `targets`: the committed category-2 action retail's menu confirm
    /// leaves for `FUN_801E295C` to seed (`actor[+0x1DE] = 2`, `+0x1DF` =
    /// the spell id, `+0x1DD` = the target byte), plus the [`PendingCast`]
    /// that owes the outcome.
    ///
    /// The band then does what retail's does: faces the target and debits
    /// the ability-bit-scaled MP at `0x28`, waits `0x14` frames at `0x29`,
    /// runs the trigger (the summon route for a Seru id), and carries the
    /// clips; the outcome folds at the seam the module docs name.
    pub(in crate::world) fn arm_player_cast(
        &mut self,
        caster: u8,
        def: &legaia_engine_battle::spells::SpellDef,
        targets: Vec<u8>,
    ) {
        let code = self.cast_target_code(def, &targets, caster);
        self.clear_action_stream(caster);
        if let Some(a) = self.actors.get_mut(caster as usize) {
            a.battle.active_target = code;
            a.battle.action_category = ActionCategory::Magic.as_byte();
            a.battle.params[0] = def.id;
            a.battle.sub_route = 0;
        }
        self.casting.pending_cast = Some(PendingCast {
            caster,
            spell_id: def.id,
            targets,
            on_flight: false,
        });
        self.battle_ctx.active_actor = caster;
        self.battle_ctx.queued_action = ActionCategory::Magic.as_byte();
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
    }

    /// The monster twin of [`Self::arm_player_cast`]. A monster's stream
    /// carries its cast clip behind the spell id (`params[1]`, the `+0x1E0`
    /// byte the `0x29` arm stages into `+0x1DA` - see
    /// [`Self::monster_cast_clip`]), then its opening camera shot
    /// (`params[2]`, `+0x1E1`, a case of the cast-effect driver - see
    /// `SpellAnimPairs::opening_shot`), terminated at `params[3]`
    /// (`FUN_801E9FD4`, `0x801EA53C..0x801EA588`); a monster with no such
    /// clip installed stages the terminator at once and the band ends the
    /// action after the wait.
    pub(in crate::world) fn arm_monster_cast(
        &mut self,
        slot: u8,
        def: &legaia_engine_battle::spells::SpellDef,
        targets: Vec<u8>,
    ) {
        let code = self.cast_target_code(def, &targets, slot);
        let clip = self.monster_cast_clip(slot, def.id).unwrap_or(0xFF);
        // A table that was not read stages the terminator in the shot's
        // place, the stream the port carried before the shot existed.
        let shot = self
            .battle
            .spell_anim_pairs
            .opening_shot(def.id)
            .unwrap_or(0xFF);
        self.clear_action_stream(slot);
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = code;
            a.battle.action_category = ActionCategory::Magic.as_byte();
            a.battle.params[0] = def.id;
            a.battle.params[1] = clip;
            a.battle.params[2] = shot;
            a.battle.params[3] = 0xFF;
            a.battle.sub_route = 0;
            // The picker stores the same entry into `+0x1E7` one instruction
            // ahead of `+0x1E0` (`sb s2,0x1e7(s4)` at `0x801EA53C`, and the
            // `OneAlly` re-pick's `sb s5,0x1e7(s4)` at `0x801EA6C4`). That is
            // the byte the action seed's **Spirit band** stages instead of
            // `+0x1E0` (`lbu v0,0x1e7(s3); sb v0,0x1da(s3)` at
            // `0x801E3B4C..0x801E3B54`), which every monster cast of a
            // class-`< 0x14` record below id `0x65` - the heals and buffs -
            // runs through. A walk that found no entry writes neither byte.
            if clip != 0xFF {
                a.battle.queued_anim_b = clip;
            }
        }
        self.casting.pending_cast = Some(PendingCast {
            caster: slot,
            spell_id: def.id,
            targets,
            on_flight: false,
        });
        self.battle_ctx.active_actor = slot;
        self.battle_ctx.queued_action = ActionCategory::Magic.as_byte();
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
    }

    /// The archive entry a monster's cast plays - the `+0x1E0` byte the
    /// picker `FUN_801E9FD4` stores beside the spell id.
    ///
    /// Retail's generic magic pick does not look the clip up by the spell id.
    /// It counts the record's live magic slots (`+0x21..+0x23` at `>= 2`,
    /// `0x801EA3E0..0x801EA408`), rolls `k = rand() % count`, then walks the
    /// entry table from index 2 counting the entries tagged `0x23`
    /// (`0x801EA4C8..0x801EA4D0`): the `k`-th one is the clip, and the spell
    /// is magic slot `count - 1 - k` (`+0x21 + (s1 - 1)` with `s1` counted
    /// down beside `k`, `0x801EA4E0..0x801EA544`). So the clip is keyed on the
    /// spell's **slot**, reversed, and on no tag the spell id names - Gimard's
    /// Tail Fire `0x27` plays his lone tag-`0x23` entry 8, which a search for
    /// tag `0x27` never finds (`battle_gimard_tail_fire_a` reads `+0x1E0 = 8`).
    ///
    /// A spell outside the record's magic slots (a per-monster scripted
    /// cast) falls back to the entry whose tag is the spell id.
    ///
    /// PORT: FUN_801E9FD4 (the tag-`0x23` clip walk, `0x801EA4A4..0x801EA548`)
    pub(in crate::world) fn monster_cast_clip(&self, slot: u8, spell_id: u8) -> Option<u8> {
        /// The tag the picker walks for a castable's clip.
        const CAST_CLIP_TAG: u8 = 0x23;
        /// The walk starts past the idle and walk entries (`li s2,0x2`).
        const FIRST_CAST_ENTRY: usize = 2;
        let actor = self.actors.get(slot as usize)?;
        let clips = actor.battle_action_clips.as_ref()?;
        let magic = actor
            .battle_monster_id
            .and_then(|id| self.tables.monster_catalog.get(id))
            .map(|d| d.magic_attacks.as_slice())
            .unwrap_or(&[]);
        if let Some(m) = magic.iter().position(|&id| id == spell_id) {
            let k = magic.len() - 1 - m;
            return clips
                .iter()
                .enumerate()
                .skip(FIRST_CAST_ENTRY)
                .filter(|(_, c)| c.as_ref().is_some_and(|c| c.action_id == CAST_CLIP_TAG))
                .nth(k)
                .and_then(|(i, _)| u8::try_from(i).ok());
        }
        clips
            .iter()
            .position(|c| c.as_ref().is_some_and(|c| c.action_id == spell_id))
            .and_then(|i| u8::try_from(i).ok())
    }

    /// Fold the owed cast's outcome - exactly once. The MP was the band's
    /// (`0x28`), so the fold is the prepaid one; a monster cast also rolls
    /// its move's impact-status and AGL-status procs onto what it reached.
    pub(in crate::world) fn fold_pending_cast(&mut self) {
        let Some(pc) = self.casting.pending_cast.take() else {
            return;
        };
        self.fold_cast(pc);
    }

    /// Fold the owed cast onto `target` alone and keep the rest owed - the
    /// hit one homing effect child lands
    /// ([`legaia_engine_effects::action_effect_script::HomingSlots::hits`]). A target the
    /// cast does not owe is left alone.
    ///
    /// REF: FUN_801E09F8 (the effect-child hit arm `0x801E1844..0x801E1A68`,
    /// one victim per landed slot)
    pub(in crate::world) fn fold_pending_cast_target(&mut self, target: u8) {
        let Some(mut pc) = self.casting.pending_cast.take() else {
            return;
        };
        let Some(i) = pc.targets.iter().position(|&t| t == target) else {
            self.casting.pending_cast = Some(pc);
            return;
        };
        pc.targets.remove(i);
        let caster = pc.caster;
        let one = PendingCast {
            targets: vec![target],
            ..pc.clone()
        };
        // The flight has already drawn the move's lists on its way; every
        // per-target fold finds them held, not just the first.
        self.casting.homing_holds_lists = Some(caster);
        let stoned = self.actor_is_petrified(target);
        self.fold_cast(one);
        // The hit arm turns its victim to face the caster unless it stands
        // Stoned (`+0x16E & 4`, `0x801E19FC..0x801E1A08`):
        // `FUN_80019B28(victim[+0x38], victim[+0x34], caster[+0x38],
        // caster[+0x34])` stored to the victim's `+0x46`
        // (`0x801E1A20..0x801E1A54`), the caster being `ctx[+0x13]`
        // (`0x801E1850`). Live pairs at both ends, unlike the melee strike's
        // body-pair turn. `battle_gimard_tail_fire_*`'s Vahn reads `4068`
        // turned onto Gimard.
        if !stoned {
            self.turn_victim_to_caster(target, caster);
        }
        if pc.targets.is_empty() {
            self.casting.homing_holds_lists = None;
        } else {
            self.casting.pending_cast = Some(pc);
        }
    }

    /// Turn battle slot `victim` to face `caster` along the two live pairs
    /// `+0x34` / `+0x38` - the homing hit arm's turn
    /// (`FUN_801E09F8`, `0x801E1A20..0x801E1A54`).
    fn turn_victim_to_caster(&mut self, victim: u8, caster: u8) {
        let (Some(v), Some(c)) = (
            self.actors.get(usize::from(victim)),
            self.actors.get(usize::from(caster)),
        ) else {
            return;
        };
        let facing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
            v.move_state.world_z,
            v.move_state.world_x,
            c.move_state.world_z,
            c.move_state.world_x,
        ) & 0xFFF;
        self.actors[usize::from(victim)].battle.facing_angle = facing;
    }

    /// Whether the owed cast's outcome is its homing flight's to land: a
    /// monster's cast whose staged clip (`params[1]`, `+0x1E0`) carries an
    /// effect-script terminator - the record whose step seeds the homing
    /// slots - on a move the move-power table resolves, so the flight has a
    /// record to fly on.
    fn cast_fold_rides_the_flight(&self, pc: &PendingCast) -> bool {
        use legaia_engine_effects::action_effect_script as fx;
        if pc.caster < self.party.party_count
            || legaia_engine_effects::summon::PLAYER_SUMMON_IDS.contains(&pc.spell_id)
        {
            return false;
        }
        let Some(a) = self.actors.get(usize::from(pc.caster)) else {
            return false;
        };
        let clip = a.battle.params.get(1).copied().unwrap_or(0xFF);
        let terminates = a
            .battle_action_clips
            .as_ref()
            .and_then(|clips| clips.get(usize::from(clip))?.as_ref())
            .is_some_and(|c| {
                (0..fx::MAX_CURSOR)
                    .map_while(|k| fx::EffectRecord::at(&c.effect_script, k))
                    .any(|r| r.is_terminator())
            });
        terminates
            && self
                .tables
                .move_power
                .as_ref()
                .is_some_and(|cat| cat.fx_for_move_id(pc.spell_id).is_some())
    }

    fn fold_cast(&mut self, pc: PendingCast) {
        // A capture body that took a branch with no damage site (PROT 0953's
        // charge, `0x801F6CEC`) owes nothing: retail never reaches its
        // `FUN_801DD6B4` on that path.
        if std::mem::take(&mut self.casting.module_skips_fold) {
            return;
        }
        // Two casts in the band do not fold through the catalog at all:
        // PROT 0927 and PROT 0966 apply their damage inside the module's own
        // `0x801F6734` stager, over a seat range the spell record cannot
        // express and with a clamp that cannot kill. Where the module owns
        // the outcome, run the module - and do not also run the generic fold,
        // which would apply the hit twice.
        if self
            .run_cast_module_aoe_for(pc.caster, pc.spell_id)
            .is_some()
        {
            return;
        }
        // ...and neither do the three whole-row **tick** sweeps, once they
        // have run. PROT 0938's two bodies and PROT 0965's each roll their
        // own baked power per hittable seat and store the clamped net into
        // `+0x14C` themselves, over `actor_table[0 .. ctx[+0]]` - a seat
        // range the spell record cannot express, exactly like the two
        // stagers above. The one difference is the clamp: these kill, the
        // stagers cannot.
        //
        // The `phase > sweep_arm` test is what keeps this from *losing* an
        // outcome: a host that never drove the band past the sweep arm has
        // had no module damage applied, and skipping the generic fold there
        // would leave the cast owed forever instead of double-applied.
        if let Some(entry) = self.cast_module_for(pc.spell_id)
            && let Some(body) =
                legaia_engine_cast::cast_module_ticks::capture_tick_body(entry, pc.spell_id)
            && legaia_engine_cast::cast_module_ticks::tick_body_owns_the_fold(entry, body)
            && let Some(arm) = legaia_engine_cast::cast_module_ticks::sweep_arm_for(entry, body)
            && self.casting.module_phase > arm
        {
            return;
        }
        // --- the fourteen trampoline arms ---
        // The same property for the three whole-row sweeps among them (PROT
        // 0941's `0xB9`, 0950's `0xAB`, 0956's `0x71`): each rolls its own
        // baked power per hittable seat and stores the clamped net into
        // `+0x14C` itself, over a seat range the spell record cannot express.
        // The `phase > arm` test is the same guard - a host that never drove
        // the band past the sweep arm is still owed the generic fold.
        if let Some(entry) = self.cast_module_for(pc.spell_id)
            && let Some(body) =
                legaia_engine_cast::cast_module_ticks::capture_tick_body(entry, pc.spell_id)
            && let Some(arm) = legaia_engine_cast::cast_arm_ticks::arm_sweep_arm(entry, body)
            && self.casting.module_phase > arm
        {
            return;
        }
        // A monster's capture-class special is not in the catalog (its record
        // is the module's), so the fold resolves it the way the arm did -
        // and only when the module has a damage site to seed it with; a
        // status-only body (Glare) folds nothing here.
        let def = self
            .tables
            .spell_catalog
            .get(pc.spell_id)
            .cloned()
            .or_else(|| {
                self.monster_capture_power(pc.caster, pc.spell_id)
                    .and_then(|_| self.monster_cast_def(pc.spell_id))
            });
        let Some(def) = def else {
            // A status-only body still resolves its status: Glare's PROT
            // 0940 body rolls Stone on its victim and raises its caption
            // (`0x801F6F9C..0x801F719C`) with no damage site to seed a fold.
            if pc.caster >= self.party.party_count {
                self.apply_enemy_agl_status(pc.caster, pc.spell_id, &pc.targets);
            }
            return;
        };
        let hit_fx_start = self.battle.hit_fx.len();
        self.cast_spell_on_slots_prepaid(pc.caster, &def, &pc.targets);
        if pc.caster >= self.party.party_count {
            self.apply_enemy_move_status(pc.caster, def.id, hit_fx_start);
            self.apply_enemy_agl_status(pc.caster, def.id, &pc.targets);
        }
    }

    /// The live loop's post-step glue for the cast band: fold the owed cast
    /// at retail's seam.
    ///
    /// * A monster's cast whose clip seeds a homing flight folds per target
    ///   on the pass that target's effect child lands its hit - retail's
    ///   only damage site for it (`FUN_801E09F8`, `jal 0x801DD0AC` at
    ///   `0x801E188C`, reached when a landed slot's counter has run out).
    ///   `battle_gimard_tail_fire_b` holds that instant: `0x2B`, 46 vsyncs
    ///   into the cast clip, the victim's reaction clip and hit tint just
    ///   staged. [`World::tick_homing_slots`] makes the call.
    /// * Any other non-summon cast folds the frame the SM leaves `0x29` for
    ///   the anim chain (the module has been paged in and the first clip
    ///   staged); the party trigger already folded the `< 0x25` arm's cast.
    /// * The summon route (`0x29 -> 0x32`) leaves the fold to the stager's
    ///   strike.
    /// * Any door out of the bands (`0x50` and above: done, the capture
    ///   branch) folds whatever is still owed, so a cast never spends its MP
    ///   for nothing.
    pub(in crate::world) fn settle_cast_band(&mut self, outcome: &StepOutcome) {
        let Some(pc) = &self.casting.pending_cast else {
            return;
        };
        if pc.caster != self.battle_ctx.active_actor {
            return;
        }
        let StepOutcome::Transition { from, to } = *outcome else {
            return;
        };
        let left_precast = from == ActionState::MagicPreCastWait.as_byte()
            && to != ActionState::MagicPreCastWait.as_byte()
            && to != ActionState::SummonInvoke.as_byte();
        // The capture band (`0x6E..=0x71`) sits above `0x50` in the state
        // space but is not a door out: its module has not run yet, so its
        // fold waits for the band's own exit into `0x50`, after the caster's
        // stages and the module's arms - where retail's module lands its hit.
        let capture_band = (ActionState::MagicCaptureBranch.as_byte()
            ..=ActionState::MagicCaptureFinalize.as_byte())
            .contains(&to);
        let band_over = to >= ActionState::DoneCleanup.as_byte() && !capture_band;
        if left_precast && !band_over && self.cast_fold_rides_the_flight(pc) {
            if let Some(pc) = self.casting.pending_cast.as_mut() {
                pc.on_flight = true;
            }
            return;
        }
        if left_precast || band_over {
            self.fold_pending_cast();
        }
    }
}

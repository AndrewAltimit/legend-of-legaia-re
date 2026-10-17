//! Per-spell outcome kernels a cast module resolves through: the cure selector,
//! Vera's restore and Nighto's verdict.

use super::*;

impl World {
    /// PROT 0907 (Nighto)'s kill / confuse / resist verdict for the resident
    /// cast - drawn once and held on [`crate::world::CastFxState::module_nighto_outcome`].
    ///
    /// Retail's arm 0 draws both rolls off the SCUS RNG `FUN_80056798` and
    /// parks them in the module's own words, so the outcome is settled the
    /// frame the cast starts (`0x801F6B50` kill roll, `0x801F6C28` resist
    /// throw, `0x801F6CF0` the third-character extra throw). This is the same
    /// draw against this world's RNG cursor, and the arithmetic is the ported
    /// kernel's ([`legaia_engine_cast::seru_magic_ticks::nighto_outcome`]).
    ///
    /// The three inputs the roll needs beyond the dice:
    ///
    /// * the caster's **magic level** for this spell, the record byte both
    ///   heal modules scan for ([`Self::caster_magic_power_byte`]);
    /// * the victim's immunity - retail's `ctx[+0x287] != 0 && record[+0x20]
    ///   != 0`, i.e. the scripted-fight flag AND the monster record's
    ///   double-width texture-page byte read as a "big model" proxy
    ///   ([`legaia_engine_battle::monster_catalog::MonsterDef::wide_texture_page`]);
    /// * whether the caster is character index `3`, the only one that takes
    ///   the extra forced-resist throw. Retail reads `0x8007BD10[ctx+0x13]`,
    ///   the **1-based** present-party character id; the engine's mirror of
    ///   that list is [`crate::world::World::party_roster_slot`], so the id is
    ///   its roster slot plus one.
    ///
    /// The **cure selector** three cast modules read out of `0x801F6960`.
    ///
    /// That word is not a module constant and not a per-spell field: it is the
    /// Seru side-effect stager's own output latch. `FUN_801F3D3C` picks an
    /// 8-byte record out of the `[element][level band]` table at `0x801F6870`
    /// (`0x801F4420..0x801F4480`: `0x801F6870 + ((level - 3) >> 1) * 8 +
    /// element * 0x20`) and stores the record's **first byte** to `0x801F6960`.
    /// On the six damaging rows that byte is a percent (`5 / 10 / 15 / 20`);
    /// on the **light** row it is a cure class `1..=4`, and `1..=4` is exactly
    /// the switch PROT 0905 (`0x801F7D68`), 0911 (`0x801F7BE4`) and 0919
    /// (`0x801F8168`) compare against. So a non-light summon's cast leaves a
    /// percent in the latch, matches none of the four arms and cures nothing -
    /// the element gate is the latch's own value, not a second test.
    ///
    /// The port's copy of that latch is
    /// [`legaia_engine_battle_vm::battle_action::BattleActionCtx::follow_up_pending`],
    /// written on every player Seru cast by
    /// [`Self::stage_seru_side_effect`]. `min_level` is the module's own
    /// `sltiu v0,v0,0x3` gate, below which its ladder is skipped entirely.
    ///
    /// REF: FUN_801F3D3C (the stager), FUN_801F69D8 (the three readers)
    pub(in crate::world::battle::cast_band) fn cure_selector(
        &self,
        caster_slot: u8,
        spell_id: u8,
        min_level: u8,
    ) -> Option<u8> {
        if self.caster_magic_power_byte(caster_slot, spell_id) < min_level {
            return None;
        }
        Some(self.battle_ctx.follow_up_pending)
    }

    /// What PROT 0905's restore arm needs: the caster's per-spell magic level,
    /// the ally target's max HP and the cure selector above.
    ///
    /// `apply_hp` is **clear**. The arm's cure sweep and its phase machine are
    /// this body's, but its HP store is not: the engine folds a cast's HP
    /// outcome exactly once at [`Self::cast_spell_on_slots_prepaid`], and that
    /// fold already routes this module's own magnitude in through
    /// `seru_tick_heal_amount`. Since [`Self::summon_stager_tick`] re-enters
    /// the module every frame, leaving the store here would restore twice -
    /// once in the arm, once in the fold. This is the same neutral-magnitude
    /// posture the rest of the band takes.
    ///
    /// REF: FUN_801F69D8 (PROT 0905 arm 9, `0x801F7C28..0x801F7F4C`)
    pub(in crate::world::battle::cast_band) fn vera_restore(
        &self,
        caster_slot: u8,
        victim_slot: u8,
        spell_id: u8,
    ) -> Option<legaia_engine_cast::seru_magic_ticks::VeraRestore> {
        let magic_level = self.caster_magic_power_byte(caster_slot, spell_id);
        let max_hp = self.actors.get(victim_slot as usize)?.battle.max_hp;
        Some(legaia_engine_cast::seru_magic_ticks::VeraRestore {
            magic_level,
            max_hp,
            cure_tier: self
                .cure_selector(
                    caster_slot,
                    spell_id,
                    legaia_engine_cast::seru_magic_ticks::VERA_CURE_MIN_LEVEL,
                )
                .unwrap_or(0),
            apply_hp: false,
        })
    }

    /// REF: FUN_801F69E8 (`0x801F6B50..0x801F6D28`, PROT 0907 arm 0)
    pub(in crate::world::battle::cast_band) fn nighto_verdict(
        &mut self,
        caster_slot: u8,
        victim_slot: u8,
        spell_id: u8,
    ) -> legaia_engine_cast::seru_magic_ticks::NightoOutcome {
        if let Some(held) = self.casting.module_nighto_outcome {
            return held;
        }
        use legaia_engine_cast::seru_magic_ticks as ticks_a;
        let magic_level = self.caster_magic_power_byte(caster_slot, spell_id);
        let target_immune = self.battle_ctx.scripted_fight != 0
            && self
                .actors
                .get(victim_slot as usize)
                .and_then(|a| a.battle_monster_id)
                .and_then(|id| self.tables.monster_catalog.get(id))
                .is_some_and(|def| def.wide_texture_page != 0);
        // Retail's character id is 1-based over the present-party list.
        let caster_character = self.party_roster_slot(caster_slot as usize) as u8 + 1;
        let kill_roll = self.next_rand();
        let resist_roll = self.next_rand();
        let extra_roll =
            (caster_character == ticks_a::NIGHTO_EXTRA_ROLL_CHARACTER).then(|| self.next_rand());
        let outcome = ticks_a::nighto_outcome(&ticks_a::NightoRoll {
            kill_roll,
            resist_roll,
            magic_level,
            target_immune,
            extra_roll,
        });
        self.casting.module_nighto_outcome = Some(outcome);
        outcome
    }

    /// One pass of PROT 0908's shatter loop
    /// ([`legaia_engine_cast::seru_magic_ticks::zenoir_shatter_pass`])
    /// over the seated row: every dead monster with a colour word left takes
    /// the frame product onto its `+0x1BA`. `victim_slot` is the cast's
    /// latched victim, whose record `+0x20` byte stands in for every seat.
    ///
    /// The step is one engine tick's share of retail's frame product
    /// `[0x1F800393] * [0x1F80037D]` - the scalar alone, as the module
    /// countdowns drain ([`legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK`]).
    pub(in crate::world::battle::cast_band) fn zenoir_shatter(&mut self, victim_slot: u8) {
        use legaia_engine_cast::seru_magic_ticks as ticks_a;
        let record_20 = |w: &World, slot: u8| {
            w.actors
                .get(slot as usize)
                .and_then(|a| a.battle_monster_id)
                .and_then(|id| w.tables.monster_catalog.get(id))
                .map_or(0, |def| def.wide_texture_page)
        };
        let formation_first = self
            .battle_monster_slots()
            .into_iter()
            .find(|&(_, _, slot)| slot == 0)
            .map_or(0, |(_, id, _)| id as u8);
        let gate = ticks_a::ZenoirShatterGate {
            scripted: self.battle_ctx.scripted_fight != 0,
            victim_record_20: record_20(self, victim_slot),
            formation_first,
        };
        let mut seats = self.cast_seat_row();
        let before: Vec<u16> = seats.iter().map(|s| s.shatter).collect();
        // The engine's resting colour word `0` is retail's neutral
        // `0x20080200` on a seated monster until a presentation arm owns the
        // word; an empty seat's is a real zero.
        let map = self.cast_seat_map();
        let seated: Vec<bool> = (0..seats.len())
            .map(|r| {
                map.engine(r as u8)
                    .and_then(|e| self.actors.get(e as usize))
                    .is_some_and(|a| a.active && a.battle_monster_id.is_some())
            })
            .collect();
        ticks_a::zenoir_shatter_pass(
            &mut seats,
            gate,
            legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK as u16,
            |seat, s| s.render_flag == 0 && seated.get(usize::from(seat)).copied().unwrap_or(false),
        );
        for (r, (st, was)) in seats.iter().zip(before).enumerate() {
            if st.shatter != was
                && let Some(e) = map.engine(r as u8)
                && let Some(a) = self.actors.get_mut(e as usize)
            {
                a.battle.shatter = st.shatter;
            }
        }
    }
}

//! The cast module's view of a battle seat: retail-pool slot mapping, the
//! actor-state snapshot and write-back, defence split, agility, element change
//! and seat position.

use super::*;

impl World {
    /// The engine slot of retail battle-pool slot `retail` - the inverse of
    /// [`World::retail_battle_pool_slot`].
    ///
    /// Retail's monster rows do not move with the party: the battle loader
    /// seats party member `i` at pool slot `i` and monster `k` at `3 + k`
    /// whatever the party size (`FUN_800513F0`, the monster loop's
    /// `addiu s0,s2,0x3` at `0x8005185C`), so the modules' fixed seat base
    /// `3` (`cast_module_ticks::FIRST_MONSTER_SEAT`) is right for every
    /// party. The engine compacts the monster row to `party_count + k`, so
    /// an engine-side seat expression built on that base goes through this
    /// map. `None` for a party seat a small party leaves empty.
    pub(in crate::world) fn engine_slot_for_retail_pool(&self, retail: u8) -> Option<u8> {
        self.battle_seat_map().engine(retail)
    }

    /// Lift one actor slot into the state view the slot-B kernels take.
    ///
    /// The engine carries every field these routines touch except the
    /// `+0x1DC` restage counter, which lives in the run's own scratch because
    /// the engine's `flag_bits` byte at that offset is a flag set, not a
    /// counter (`docs/subsystems/battle-action.md`
    /// and `docs/subsystems/cast-module.md` read `+0x1DC` differently, and the
    /// bytes here only ever increment it).
    pub(in crate::world::battle::cast_band) fn cast_actor_state(
        &self,
        slot: u8,
    ) -> legaia_engine_cast::cast_module_ticks::CastActorState {
        use legaia_engine_cast::cast_module_ticks::{ANIM_RATE_NORMAL, CastActorState};
        let Some(a) = self.actors.get(slot as usize) else {
            return CastActorState {
                anim_rate: ANIM_RATE_NORMAL,
                ..Default::default()
            };
        };
        // The reaction slots `+0x1EF..+0x1F2`, as the monster init builds them
        // off the entry tags (`World::battle_reaction_map`, the party's fixed
        // map on a party seat); the action bytes stand in for a seat with no
        // clips.
        let param = |off: usize| a.battle.params.get(off - 0x1DF).copied().unwrap_or(0);
        let map = self.battle_reaction_map(slot as usize).unwrap_or([
            param(0x1EF),
            param(0x1F0),
            param(0x1F1),
            param(0x1F2),
            0,
        ]);
        CastActorState {
            // `+0x0C` is the tint blend the tint pass `FUN_8004A908` weights
            // the colour word by (`render_blend`): a module's `0x1000` there
            // (Freed's arms 7 and `0x10`, every Seru hit) pushes the body
            // fully onto its `+0x04` lanes.
            root_speed: a.battle.render_blend as i32,
            hp_bar_delta: a.battle.hp_bar_pending,
            hp: a.battle.hp,
            flags: a.battle.field_flags,
            playing_anim: a.battle.current_anim,
            staged_anim: a.battle.queued_anim,
            restage: 0,
            target_code: a.battle.active_target,
            // `+0x1F1` sits inside the per-action parameter stream that starts
            // at `+0x1DF`.
            knockdown_anim: map[2],
            render_flag: a.battle.render_flag,
            // The three mirrors the band's hit arms stamp beside the HP write
            // all have a home on the battle actor already, so they are seeded
            // and written back like every other view field rather than being
            // dropped at the seam.
            present_04: a.battle.render_color,
            render_21f: a.battle.impact_state,
            render_225: a.battle.capture_state,
            anim_rate: a.battle.anim_rate.get(),
            // The whole `+0x158..+0x16A` stat block, five `(working, base)`
            // pairs, now has a home on the battle actor - so every one of
            // Melt Spray's ten halfword stores and both of Power Charge's
            // `+0x15A` stores land instead of stopping at the view. The
            // defence pair is still kept in the world's per-slot split,
            // which is where the physical-defence facet reads it.
            agl: a.battle.agl,
            agl_base: a.battle.agl_base,
            atk: a.battle.atk_working,
            atk_base: a.battle.atk_base,
            udf: self.cast_defence_split(slot).0,
            udf_base: self.cast_defence_split(slot).0,
            ldf: self.cast_defence_split(slot).1,
            ldf_base: self.cast_defence_split(slot).1,
            // SPD and INT live in two places: the actor's own halfwords (new,
            // so the band's writers have somewhere to land) and the world's
            // per-slot mirrors, which are what turn order / the escape roll
            // (`battle_speed`) and the accuracy seed (`battle_accuracy`)
            // actually read. Seed from the mirror whenever the actor's own
            // halfword is still zero - `seed_party_battle_stats` fills the
            // mirrors, not the actor - so a debuff computes on a real number
            // instead of underflowing zero.
            spd: nonzero_or(a.battle.spd, self.battle.speed.get(slot as usize).copied()),
            spd_base: nonzero_or(
                a.battle.spd_base,
                self.battle.speed.get(slot as usize).copied(),
            ),
            intel: nonzero_or(
                a.battle.intel,
                self.battle.accuracy.get(slot as usize).copied(),
            ),
            intel_base: nonzero_or(
                a.battle.intel_base,
                self.battle.accuracy.get(slot as usize).copied(),
            ),
            spirit_gauge: a.battle.spirit_gauge,
            init_key: a.battle.init_key,
            action_category: a.battle.action_category,
            queued_action: a.battle.params.first().copied().unwrap_or(0),
            reaction_alt: map[0],
            reaction_alt2: map[1],
            reaction_gate: map[3],
            combo_total: a.battle.damage_accum,
            shatter: a.battle.shatter,
            loop_count: a.battle.hit_count_bound,
            loop_timer: a.battle_animation.as_ref().map_or(0, |p| p.loop_timer()),
            hits_pending: a.battle_staged_anim.is_some()
                && a.battle_animation
                    .as_ref()
                    .and_then(|p| p.hit_source())
                    .is_some_and(|src| {
                        let in_band = src.power_run[0]
                            .wrapping_sub(legaia_engine_battle_vm::battle_action::HIT_POWER_BASE)
                            < legaia_engine_battle_vm::battle_action::HIT_POWER_SPAN;
                        let next = src.event_frames.get(usize::from(a.battle.input_cursor));
                        in_band && next.is_some_and(|&f| f != 0)
                    }),
        }
    }

    /// The live UDF / LDF pair for one battle slot, out of the world's own
    /// per-slot defence split (the same store the physical-defence facet
    /// reads).
    pub(in crate::world::battle::cast_band) fn cast_defence_split(&self, slot: u8) -> (u16, u16) {
        self.battle
            .defense_split
            .get(slot as usize)
            .copied()
            .flatten()
            .unwrap_or((0, 0))
    }

    /// The monster **record**'s AGL (`+0x0E`) for one battle seat - what
    /// PROT 0942's Power Up arm reads through `0x801C9348[seat - 3]` before
    /// it writes `caster[+0x156]`.
    ///
    /// Retail's table only covers the monster seats; a party caster indexes
    /// out of it, so the engine falls back to the actor's own AGL base rather
    /// than reading whatever sits below the table.
    pub(in crate::world::battle::cast_band) fn cast_record_agl(&self, slot: u8) -> u16 {
        self.actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.tables.monster_catalog.get(id))
            .map(|d| d.agl)
            .unwrap_or_else(|| {
                self.actors
                    .get(slot as usize)
                    .map(|a| a.battle.agl_base)
                    .unwrap_or(0)
            })
    }

    /// Retail's `0x801C8FE4`, the word PROT 0964's re-roll compares its draw
    /// against (`lw v1,4(a1)` with `a1 = 0x801C8FE0` at `0x801F8A20`, again
    /// at `0x801F8A4C`) and then overwrites with the accepted one (`sw a3,
    /// 4(v1)` at `0x801F8A90`). It is the AI's phase counter
    /// ([`legaia_engine_battle::monster_ai::MonsterAiState::counter`]): Rogue's picker reads
    /// the same byte back as its next attack, `counter - 0x50` (`0xB0` Wind /
    /// `0xB1` Thunder / `0xB2` Flame), so the roll that recolours the record
    /// is also what makes the attacks cycle. Battle init zeroes it, so the
    /// first Element Change cannot roll `0`.
    pub(in crate::world::battle::cast_band) fn cast_element_change_last_roll(&self) -> u8 {
        self.battle.monster_ai_state.counter() as u8
    }

    /// Commit PROT 0964's new element onto the first monster seat.
    ///
    /// Retail writes the record copy the battle loader made for that seat
    /// (`0x801C9348[0]`, record `+0x1D`) - per seat and per fight. The engine
    /// keeps that copy as the seat's [`crate::world::Actor::battle_element`],
    /// which `World::battle_slot_element` and the plaque badge read ahead of
    /// the catalog; the shared catalog entry is never touched, so the next
    /// battle with the same monster starts on its disc element.
    pub(in crate::world) fn apply_cast_element_change(&mut self, element: u8) {
        let first = self
            .engine_slot_for_retail_pool(legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT)
            .unwrap_or(legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT);
        if let Some(a) = self
            .actors
            .get_mut(first as usize)
            .filter(|a| a.battle_monster_id.is_some())
        {
            a.battle_element = Some(element);
        }
    }

    /// Write a kernel's state view back onto an actor slot.
    pub(in crate::world::battle::cast_band) fn write_cast_actor_state(
        &mut self,
        slot: u8,
        st: &legaia_engine_cast::cast_module_ticks::CastActorState,
    ) {
        use legaia_engine_battle_vm::battle_anim_rate::AnimRate;
        let creature = self.casting.summon_actor_slot == Some(slot);
        let reaction_map = self.battle_reaction_map(slot as usize);
        let Some(a) = self.actors.get_mut(slot as usize) else {
            return;
        };
        // The summon creature's `+0x1DC` bump (read in as `0`): bit 0 is the
        // stage bit that commits its `+0x1DA` at once rather than at the
        // looping clip's natural end (`World::summon_seat_waits_for_cycle`).
        if creature && st.restage & 1 != 0 {
            a.battle
                .flag_bits
                .set(legaia_engine_battle_vm::battle_action::ActorFlags::WINDUP_DONE);
        }
        a.battle.hp_bar_pending = st.hp_bar_delta;
        a.battle.hp = st.hp;
        a.battle.field_flags = st.flags;
        // A reaction a module stages on a combatant with the commit bit
        // (`+0x1DC |= 1`) - one of its `+0x1EF..+0x1F2` entries - plays at
        // once on the reaction channel, the engine's seat of retail's
        // `FUN_8004AD80` for an entry a hit staged
        // (`World::commit_battle_reaction_entry`): its tag ladder, its effect
        // script (the hit spark) and the death arm at its natural end. A
        // caster's own clip stays on the action channel.
        let reaction = (!creature
            && st.restage & 1 != 0
            && st.staged_anim != 0
            && reaction_map.is_some_and(|m| m[..4].contains(&st.staged_anim)))
        .then_some(st.staged_anim);
        if reaction.is_none() {
            a.battle.queued_anim = st.staged_anim;
        }
        a.battle.active_target = st.target_code;
        a.battle.render_flag = st.render_flag;
        a.battle.render_color = st.present_04;
        if st.root_speed != a.battle.render_blend as i32 {
            a.battle.render_blend = st.root_speed as u32;
        }
        a.battle.impact_state = st.render_21f;
        a.battle.capture_state = st.render_225;
        a.battle.anim_rate = AnimRate(st.anim_rate);
        a.battle.agl = st.agl;
        a.battle.agl_base = st.agl_base;
        a.battle.atk_working = st.atk;
        a.battle.atk_base = st.atk_base;
        a.battle.spd = st.spd;
        a.battle.spd_base = st.spd_base;
        a.battle.intel = st.intel;
        a.battle.intel_base = st.intel_base;
        a.battle.init_key = st.init_key;
        a.battle.spirit_gauge = st.spirit_gauge;
        a.battle.action_category = st.action_category;
        a.battle.damage_accum = st.combo_total;
        a.battle.shatter = st.shatter;
        // `+0x21B` / `+0x176`: a module's stores land on the actor's counter
        // and on the playing clip's hold budget. The budget is written only
        // when the module moved it, so the view's whole-unit rounding never
        // eats a park's fractional overshoot.
        a.battle.hit_count_bound = st.loop_count;
        if let Some(p) = a.battle_animation.as_mut()
            && p.loop_timer() != st.loop_timer
        {
            p.set_loop_timer(st.loop_timer);
        }
        // ...and back into the mirrors the rest of the engine reads, so a
        // five-stat debuff is visible to turn order and the accuracy seed
        // rather than only to the next module tick.
        if let Some(s) = self.battle.speed.get_mut(slot as usize) {
            *s = st.spd;
        }
        if let Some(s) = self.battle.accuracy.get_mut(slot as usize) {
            *s = st.intel;
        }
        if let Some(s) = self.battle.defense_split.get_mut(slot as usize)
            && s.is_some()
        {
            *s = Some((st.udf, st.ldf));
        }
        if let Some(entry) = reaction {
            self.commit_battle_reaction_entry(slot as usize, entry);
        }
    }

    /// The `(x, z)` pair a battle seat occupies - its anchor when the battle
    /// loader seeded one, else its live position.
    pub(in crate::world::battle::cast_band) fn cast_seat_xz(&self, slot: u8) -> (i16, i16) {
        self.actors
            .get(slot as usize)
            .map(|a| {
                a.battle
                    .seat
                    .unwrap_or((a.move_state.world_x, a.move_state.world_z))
            })
            .unwrap_or((0, 0))
    }
}

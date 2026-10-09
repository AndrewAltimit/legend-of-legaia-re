use super::*;

/// The Spikefish flute's band entry (spell `0x97`).
const SPIKEFISH_ENTRY: u32 = 925;

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
        let pc = self.party.party_count.clamp(1, 3);
        let fm = vm::cast_module_ticks::FIRST_MONSTER_SEAT;
        if retail < fm {
            (retail < pc).then_some(retail)
        } else {
            Some(pc + (retail - fm))
        }
    }

    /// Lift one actor slot into the state view the slot-B kernels take.
    ///
    /// The engine carries every field these routines touch except two: the
    /// `+0x0C` root-speed word and the `+0x1DC` restage counter, which live
    /// in the run's own scratch because the engine's `flag_bits` byte at that
    /// offset is a flag set, not a counter (`docs/subsystems/battle-action.md`
    /// and `docs/subsystems/cast-module.md` read `+0x1DC` differently, and the
    /// bytes here only ever increment it).
    pub(super) fn cast_actor_state(&self, slot: u8) -> vm::cast_module_ticks::CastActorState {
        use vm::cast_module_ticks::{ANIM_RATE_NORMAL, CastActorState};
        let Some(a) = self.actors.get(slot as usize) else {
            return CastActorState {
                anim_rate: ANIM_RATE_NORMAL,
                ..Default::default()
            };
        };
        CastActorState {
            root_speed: 0,
            hp_bar_delta: a.battle.hp_bar_pending,
            hp: a.battle.hp,
            flags: a.battle.field_flags,
            playing_anim: a.battle.current_anim,
            staged_anim: a.battle.queued_anim,
            restage: 0,
            target_code: a.battle.active_target,
            // `+0x1F1` sits inside the per-action parameter stream that starts
            // at `+0x1DF`.
            knockdown_anim: a.battle.params.get(0x1F1 - 0x1DF).copied().unwrap_or(0),
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
            reaction_alt: a.battle.params.get(0x1EF - 0x1DF).copied().unwrap_or(0),
            reaction_alt2: a.battle.params.get(0x1F0 - 0x1DF).copied().unwrap_or(0),
            reaction_gate: a.battle.params.get(0x1F2 - 0x1DF).copied().unwrap_or(0),
            combo_total: a.battle.damage_accum,
            hits_pending: a.battle_staged_anim.is_some()
                && a.battle_animation
                    .as_ref()
                    .and_then(|p| p.hit_source())
                    .is_some_and(|src| {
                        let in_band = src.power_run[0]
                            .wrapping_sub(vm::battle_action::HIT_POWER_BASE)
                            < vm::battle_action::HIT_POWER_SPAN;
                        let next = src.event_frames.get(usize::from(a.battle.input_cursor));
                        in_band && next.is_some_and(|&f| f != 0)
                    }),
        }
    }

    /// The live UDF / LDF pair for one battle slot, out of the world's own
    /// per-slot defence split (the same store the physical-defence facet
    /// reads).
    pub(super) fn cast_defence_split(&self, slot: u8) -> (u16, u16) {
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
    pub(super) fn cast_record_agl(&self, slot: u8) -> u16 {
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
    /// ([`crate::monster_ai::MonsterAiState::counter`]): Rogue's picker reads
    /// the same byte back as its next attack, `counter - 0x50` (`0xB0` Wind /
    /// `0xB1` Thunder / `0xB2` Flame), so the roll that recolours the record
    /// is also what makes the attacks cycle. Battle init zeroes it, so the
    /// first Element Change cannot roll `0`.
    pub(super) fn cast_element_change_last_roll(&self) -> u8 {
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
            .engine_slot_for_retail_pool(vm::cast_module_ticks::FIRST_MONSTER_SEAT)
            .unwrap_or(vm::cast_module_ticks::FIRST_MONSTER_SEAT);
        if let Some(a) = self
            .actors
            .get_mut(first as usize)
            .filter(|a| a.battle_monster_id.is_some())
        {
            a.battle_element = Some(element);
        }
    }

    /// Write a kernel's state view back onto an actor slot.
    pub(super) fn write_cast_actor_state(
        &mut self,
        slot: u8,
        st: &vm::cast_module_ticks::CastActorState,
    ) {
        use vm::battle_anim_rate::AnimRate;
        let Some(a) = self.actors.get_mut(slot as usize) else {
            return;
        };
        a.battle.hp_bar_pending = st.hp_bar_delta;
        a.battle.hp = st.hp;
        a.battle.field_flags = st.flags;
        a.battle.queued_anim = st.staged_anim;
        a.battle.active_target = st.target_code;
        a.battle.render_flag = st.render_flag;
        a.battle.render_color = st.present_04;
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
    }

    /// The `(x, z)` pair a battle seat occupies - its anchor when the battle
    /// loader seeded one, else its live position.
    pub(super) fn cast_seat_xz(&self, slot: u8) -> (i16, i16) {
        self.actors
            .get(slot as usize)
            .map(|a| {
                a.battle
                    .seat
                    .unwrap_or((a.move_state.world_x, a.move_state.world_z))
            })
            .unwrap_or((0, 0))
    }

    /// PROT 0904's beam root and arm-12 ray tip for sweep word `ctx_6d8`
    /// (before the arm's ramp - the tip is built from the ramped word, as the
    /// tick builds it), from the summon seat's live position and facing
    /// (retail reads slot 7's `+0x34` / `+0x38` / `+0x46`).
    ///
    /// REF: FUN_801F69D8 (PROT 0904 arm 12, `0x801F7AF4..0x801F7C80`)
    pub(in crate::world) fn theeder_ray(
        &self,
        summon_slot: u8,
        ctx_6d8: u16,
    ) -> ([i16; 3], [i16; 3]) {
        use vm::cast_seru_ticks_a as ta;
        let g = self.theeder_geom(summon_slot);
        let mouth = ta::theeder_mouth(g.x, g.z, g.facing);
        let tip = ta::theeder_ray_tip(mouth, g.facing, ta::theeder_sweep_phase(ctx_6d8));
        (mouth, tip)
    }

    /// PROT 0904's arm-4 seat placement
    /// ([`vm::cast_seru_ticks_a::theeder_seat_placement`]) from the victim's
    /// and the caster's live positions.
    ///
    /// REF: FUN_801F69D8 (PROT 0904 arm 4, `0x801F6EEC..0x801F6F8C`)
    pub(super) fn theeder_seat_for(
        &self,
        caster_slot: u8,
        victim_slot: u8,
    ) -> Option<(i16, i16, u16)> {
        let pos = |s: u8| {
            self.actors
                .get(s as usize)
                .map(|a| (a.move_state.world_x, a.move_state.world_z))
        };
        Some(vm::cast_seru_ticks_a::theeder_seat_placement(
            pos(victim_slot)?,
            pos(caster_slot)?,
        ))
    }

    pub(super) fn pin_theeder_seat(&mut self, summon_slot: u8, (x, z, facing): (i16, i16, u16)) {
        if let Some(a) = self.actors.get_mut(summon_slot as usize) {
            a.move_state.world_x = x;
            a.move_state.world_z = z;
            a.battle.facing_angle = facing;
        }
    }

    /// The summon seat's live `(x, z)` and facing, as PROT 0904 reads slot 7.
    pub(super) fn theeder_geom(&self, summon_slot: u8) -> vm::cast_seru_ticks_a::TheederGeom {
        self.actors
            .get(summon_slot as usize)
            .map(|a| vm::cast_seru_ticks_a::TheederGeom {
                x: a.move_state.world_x,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle & 0x0FFF,
            })
            .unwrap_or_default()
    }

    /// PROT 0904's packets for this frame - what the Theeder module's last
    /// tick drew (the arm-9 prongs, the arm-11 charge beam, the arm-11/12
    /// sweeping beam and its trail, the arm-13 retract) - while its cast is
    /// in the band. Both hosts project the points with their battle camera
    /// and build the primitives with `legaia_engine_ui::cast_theeder`.
    ///
    /// REF: FUN_801F815C, FUN_801F83A4, FUN_801F8634, FUN_801F8B84
    pub fn theeder_draw(&self) -> Option<vm::cast_seru_ticks_a::TheederPacket> {
        if self.mode != SceneMode::Battle || self.casting.summon_stager.is_none() {
            return None;
        }
        self.casting.module_theeder.packet
    }

    /// The trail ring [`Self::theeder_draw`]'s fan packets index
    /// (`hist[0]` newest).
    pub fn theeder_trail(&self) -> &[[i16; 3]] {
        &self.casting.module_theeder.trail.hist
    }

    /// Which of `seats` lie inside a `+-half_width` cone about `bearing`, as
    /// seen from `centre` - the geometry PROT 0904's swinging-ray sweep
    /// gates each hit on, and the one thing the module's arm 12 needs from its
    /// host.
    ///
    /// Retail's shape, read off `0x801F7CA0..0x801F7D14`: two calls to the
    /// 12-bit atan2 `FUN_80019B28` against the **same** second point (the
    /// beam root) - one from the ray's tip, one from
    /// the seat - each `+0x800` and masked to `0xFFF`, then
    /// `|ref - seat| - 0x30` compared **unsigned** against `0xFB1`. That
    /// comparison is the wrap: a difference below `0x30` underflows past
    /// `0xFB1` and a difference at or above `0xFE1` exceeds it, so both ends
    /// of the cone are in and everything between is out. The `+0x800` cancels
    /// in the difference and the bearings are measured toward the centre in
    /// both calls, so measuring outward from the centre gives the same
    /// difference.
    ///
    /// The atan2 is the ported `FUN_80019B28`
    /// ([`vm::battle_action::bearing_12bit_approx`]) over the
    /// approximated arctan LUT, the same one the enemy target cursor uses.
    ///
    /// REF: FUN_801F69D8 (PROT 0904 arm 12 cone gate), FUN_80019B28
    pub fn seats_in_cone(
        &self,
        centre: (i16, i16),
        bearing: u16,
        half_width: u16,
        seats: std::ops::Range<u8>,
    ) -> Vec<u8> {
        use vm::battle_action::bearing_12bit_approx;
        const FULL_TURN: u16 = 0x1000;
        seats
            .filter(|&seat| {
                let (sx, sz) = self.cast_seat_xz(seat);
                let to_seat = bearing_12bit_approx(centre.1, centre.0, sz, sx);
                let d = (to_seat.wrapping_sub(bearing)) & (FULL_TURN - 1);
                d <= half_width || d >= FULL_TURN - half_width
            })
            .collect()
    }

    /// PROT 0907 (Nighto)'s kill / confuse / resist verdict for the resident
    /// cast - drawn once and held on [`crate::world::CastFxState::module_nighto_outcome`].
    ///
    /// Retail's arm 0 draws both rolls off the SCUS RNG `FUN_80056798` and
    /// parks them in the module's own words, so the outcome is settled the
    /// frame the cast starts (`0x801F6B50` kill roll, `0x801F6C28` resist
    /// throw, `0x801F6CF0` the third-character extra throw). This is the same
    /// draw against this world's RNG cursor, and the arithmetic is the ported
    /// kernel's ([`vm::cast_seru_ticks_a::nighto_outcome`]).
    ///
    /// The three inputs the roll needs beyond the dice:
    ///
    /// * the caster's **magic level** for this spell, the record byte both
    ///   heal modules scan for ([`Self::caster_magic_power_byte`]);
    /// * the victim's immunity - retail's `ctx[+0x287] != 0 && record[+0x20]
    ///   != 0`, i.e. the scripted-fight flag AND the monster record's
    ///   double-width texture-page byte read as a "big model" proxy
    ///   ([`crate::monster_catalog::MonsterDef::wide_texture_page`]);
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
    /// [`legaia_engine_vm::battle_action::BattleActionCtx::follow_up_pending`],
    /// written on every player Seru cast by
    /// [`Self::stage_seru_side_effect`]. `min_level` is the module's own
    /// `sltiu v0,v0,0x3` gate, below which its ladder is skipped entirely.
    ///
    /// REF: FUN_801F3D3C (the stager), FUN_801F69D8 (the three readers)
    pub(super) fn cure_selector(&self, caster_slot: u8, spell_id: u8, min_level: u8) -> Option<u8> {
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
    pub(super) fn vera_restore(
        &self,
        caster_slot: u8,
        victim_slot: u8,
        spell_id: u8,
    ) -> Option<vm::cast_seru_ticks_a::VeraRestore> {
        let magic_level = self.caster_magic_power_byte(caster_slot, spell_id);
        let max_hp = self.actors.get(victim_slot as usize)?.battle.max_hp;
        Some(vm::cast_seru_ticks_a::VeraRestore {
            magic_level,
            max_hp,
            cure_tier: self
                .cure_selector(
                    caster_slot,
                    spell_id,
                    vm::cast_seru_ticks_a::VERA_CURE_MIN_LEVEL,
                )
                .unwrap_or(0),
            apply_hp: false,
        })
    }

    /// REF: FUN_801F69E8 (`0x801F6B50..0x801F6D28`, PROT 0907 arm 0)
    pub(super) fn nighto_verdict(
        &mut self,
        caster_slot: u8,
        victim_slot: u8,
        spell_id: u8,
    ) -> vm::cast_seru_ticks_a::NightoOutcome {
        if let Some(held) = self.casting.module_nighto_outcome {
            return held;
        }
        use vm::cast_seru_ticks_a as ticks_a;
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

    /// The module's camera state with its `yaw_base` loaded from the battle
    /// camera's live `ctx[+0x6DA]`. Retail has one word: the module's
    /// `sh 0x200, 0x6DA(ctx)` and per-pass swing land in the counter the
    /// action SM's prologue keeps drifting, and the Done band's case 6 reads
    /// what they left (`shiny_refactor_gimard_levelup`).
    pub(super) fn module_cam_with_yaw_base(&self) -> vm::cast_module_camera::ModuleCamState {
        let mut st = self.casting.module_cam;
        if let Some(cam) = self.battle.camera.as_ref() {
            st.yaw_base = cam.action_yaw_base();
        }
        st
    }

    /// Store the module's camera state back and its `yaw_base` into the
    /// battle camera's `ctx[+0x6DA]` ([`Self::module_cam_with_yaw_base`]).
    pub(super) fn store_module_cam(&mut self, st: vm::cast_module_camera::ModuleCamState) {
        if let Some(cam) = self.battle.camera.as_mut() {
            cam.set_action_yaw_base(st.yaw_base);
        }
        self.casting.module_cam = st;
    }

    /// The caster and victim as the module camera arms read them: world
    /// position (`+0x34..+0x38`) and battle heading (`+0x46`).
    pub(in crate::world) fn module_cam_seats(
        &self,
        caster_slot: u8,
        victim_slot: u8,
    ) -> vm::cast_module_camera::ModuleCamSeats {
        let seat = |slot: u8| {
            self.actors
                .get(slot as usize)
                .map(|a| vm::cast_module_camera::ModuleSeat {
                    x: a.move_state.world_x,
                    y: a.move_state.world_y,
                    z: a.move_state.world_z,
                    facing: a.battle.facing_angle & 0xFFF,
                })
                .unwrap_or_default()
        };
        vm::cast_module_camera::ModuleCamSeats {
            caster: seat(caster_slot),
            victim: seat(victim_slot),
            band_timer: i32::from(self.battle_ctx.frame_timer),
            first_monster: self.battle_first_monster_byte(),
            caster_monster: self
                .actors
                .get(caster_slot as usize)
                .and_then(|a| a.battle_monster_id)
                .map_or(0, |id| id as u8),
            action: self
                .actors
                .get(caster_slot as usize)
                .map_or(0, |a| a.battle.params[0]),
            depth_raw: self.battle.camera_frame_height as i32,
            caster_latch: self.caster_latch(caster_slot).unwrap_or(0),
        }
    }

    /// The monster caster's battle-scoped latch word `0x801C8FE0 +
    /// (ctx[+0x13] + 1) * 4` - the monster AI's ability cooldown
    /// `dat[m + 4]`. `None` for a party caster.
    pub(super) fn caster_latch_index(&self, caster_slot: u8) -> Option<usize> {
        let m = usize::from(caster_slot).checked_sub(self.party.party_count as usize)?;
        let i = m + 4;
        (i < self.battle.monster_ai_state.dat.len()).then_some(i)
    }

    pub(super) fn caster_latch(&self, caster_slot: u8) -> Option<i32> {
        self.caster_latch_index(caster_slot)
            .map(|i| self.battle.monster_ai_state.dat[i])
    }

    /// The context bytes the kernels read (`ctx+0`, `+1`, `+0x13`, `+0x278`,
    /// `+0x279`).
    ///
    /// Both counts are bounded by [`BATTLE_TABLE_SLOTS`]: retail's `ctx[+0]`
    /// and `ctx[+1]` index `DAT_801C9370`, whose battle span is the eight
    /// combat seats - the summon seat and any host-side extras above them are
    /// not part of either sweep's range. The monster count starts at
    /// `cast_module_ticks::FIRST_MONSTER_SEAT`, the fixed base the Juggernaut
    /// sweep's `addiu s4, zero, 0xc` encodes.
    pub(in crate::world) fn cast_module_ctx(&self) -> vm::cast_module_ticks::CastModuleCtx {
        use vm::cast_module_ticks::FIRST_MONSTER_SEAT;
        let table = self.actors.len().min(BATTLE_TABLE_SLOTS);
        let map = self.cast_seat_map();
        vm::cast_module_ticks::CastModuleCtx {
            // Retail's `ctx[+0]` is the **party** count, not the actor count:
            // `0x8004B3F0` loads it as the bound of a loop that turns
            // `DAT_8007BD10[i]` - the present-party char-id list - into a
            // `0x414`-byte record (`0x8004B420..0x8004B484`, id-1 scaled by
            // `0x414` onto `0x80084140`). So the seat range it names is the
            // party row `0..FIRST_MONSTER_SEAT`, and nothing above it.
            //
            // The engine's mirror of that list is `PartyState::party_count`
            // (the same ordinal space `World::party_roster_slot` resolves).
            // Seeding this from the whole actor table instead made every
            // `ctx[+0]` sweep - Evil Seru Magic's whole-row hit, the Orb heal,
            // the Element Change hide - run over the monster rows as well,
            // which retail's separate `ctx[+1]` sweep is what covers.
            party_count: self
                .party
                .party_count
                .min(FIRST_MONSTER_SEAT)
                .min(table as u8),
            // Counted over retail's seats `3..8` (the kernels' row,
            // `seat_map`), each read through the engine slot behind it: a
            // small party's monsters sit below engine slot `3`.
            monster_count: (FIRST_MONSTER_SEAT..BATTLE_TABLE_SLOTS as u8)
                .filter_map(|r| map.engine(r))
                .filter(|&e| self.actors.get(usize::from(e)).is_some_and(|a| a.active))
                .count() as u8,
            caster_seat: map.retail(self.battle_ctx.active_actor),
            ctx_278: self.casting.module_ctx_278,
            phase: self.casting.module_phase,
            ctx_0d: 0,
            turn_cursor: self.battle_ctx.turn_cursor,
            ctx_27a: 0,
            ctx_6d8: self.casting.module_ring_angle,
        }
    }

    /// Run the resident module's **code** half for one frame - the band's
    /// **PORT** worklist rows, ported in
    /// [`legaia_engine_vm::cast_module_ticks`].
    ///
    /// Retail re-enters the paged module every frame through
    /// `FUN_801F1ED4` / `FUN_801F2160` and holds battle phase `0x70` while
    /// the tick reports busy; the engine calls this from
    /// [`Self::summon_stager_tick`], which the action SM already drives at
    /// states `0x34` / `0x35` / `0x36` (`crate::world::vm_hosts` ->
    /// `legaia_engine_vm::battle_action::summon`). So the chain from a host
    /// root is: `play-window` / the browser play page -> `World::tick` ->
    /// the action SM's summon band -> here.
    ///
    /// What runs is the module's **presentation and phase** half: the
    /// `ctx+0x279` walk, `ctx+0x278`, the staged-clip and animation-rate
    /// writes, the summon-seat pose and the `+0x1DD` retarget. The damage
    /// half is deliberately *not* re-applied here - the engine folds a cast's
    /// HP outcome once, at [`Self::cast_spell_on_slots_prepaid`], and the
    /// module's own numbers reach that fold as the baked per-hit power
    /// ([`vm::cast_module_ticks::baked_power_for`], read by
    /// `capture_bypass_predamage` / `capture_respect_predamage`) rather than
    /// as a second application.
    ///
    /// Returns `None` when no band entry is resident (a disc-free host, or a
    /// spell that names no module).
    /// Whether one seat has settled the way every settle loop in the band
    /// tests it ([`vm::cast_module_ticks::ChainSettle`]): a live seat once its playing clip
    /// is back to idle, a dead one once it plays the down clip - or, where
    /// `faded` counts, once its defeat fade has run its colour word out
    /// (retail's prim word `+0x04` at `0`).
    ///
    /// The engine's playing clip is the reaction channel's entry while a
    /// reaction plays (retail commits a reaction into `+0x1D9` like any
    /// other clip), else the committed `current_anim`. A downed party seat
    /// holds the engine's defeat pose rather than clip `8`, so a finished
    /// defeat pose reads as settled too.
    pub(super) fn chain_seat_settled(&self, slot: usize, faded: bool) -> bool {
        let Some(a) = self.actors.get(slot) else {
            return true;
        };
        let playing = match a.battle_reaction {
            Some(tag) => a.battle_reaction_entry.unwrap_or(tag.max(1)),
            None => a.battle.current_anim,
        };
        if a.battle.hp != 0 {
            return playing == 0;
        }
        let downed = playing == vm::cast_module_ticks::SETTLE_DOWN_CLIP
            || (a.battle_pose == Some(vm::battle_action::Pose::Defeat as u8)
                && a.battle_animation.as_ref().is_none_or(|p| p.finished()));
        downed || (faded && (a.battle.render_color == 0 || !a.active))
    }

    /// [`Self::chain_seat_settled`] over the seats a [`vm::cast_module_ticks::ChainSettle`]
    /// walks.
    pub(super) fn chain_settled(
        &self,
        settle: vm::cast_module_ticks::ChainSettle,
        caster: u8,
        victim: u8,
        ctx: &vm::cast_module_ticks::CastModuleCtx,
    ) -> bool {
        use vm::cast_module_ticks::ChainSettle as S;
        let party = 0..usize::from(ctx.party_count);
        let pc = usize::from(self.party.party_count);
        let monsters = pc..pc + usize::from(ctx.monster_count);
        match settle {
            S::Victim => self.chain_seat_settled(usize::from(victim), false),
            S::VictimOrFaded => self.chain_seat_settled(usize::from(victim), true),
            S::PartyRow => party.into_iter().all(|s| self.chain_seat_settled(s, false)),
            S::TargetRow => {
                let t = self
                    .actors
                    .get(usize::from(caster))
                    .map_or(0, |a| a.battle.active_target);
                if t == legaia_engine_vm::battle_cue_group::TARGET_PARTY_WIDE || usize::from(t) < pc
                {
                    party.into_iter().all(|s| self.chain_seat_settled(s, false))
                } else {
                    monsters.into_iter().all(|s| {
                        let a = self.actors.get(s);
                        match a {
                            Some(a) if a.battle.hp == 0 => a.battle.render_color == 0 || !a.active,
                            _ => self.chain_seat_settled(s, false),
                        }
                    })
                }
            }
        }
    }

    pub fn run_cast_module_code(&mut self, spell_id: u8, arm: u8) -> Option<CastModuleCodeRun> {
        // --- W1-D ---
        use vm::cast_arm_ticks as arms;
        // --- end W1-D ---
        use vm::cast_module_ticks as ticks;
        // --- W1-B ---
        use vm::cast_seru_ticks_a as ticks_a;
        // --- end W1-B ---

        let entry = self.cast_module_for(spell_id)?;
        // PROT 0954's body is ported whole - camera, records, wheel and
        // outcomes - and drives the band on its own
        // (`world::battle::fatal_decision`).
        if entry == vm::cast_fatal_decision::FATAL_DECISION_ENTRY {
            return Some(self.run_fatal_decision(entry));
        }
        let mut ctx = self.cast_module_ctx();
        let caster_slot = self.battle_ctx.active_actor;
        let victim_slot = self
            .actors
            .get(caster_slot as usize)
            .map(|a| a.battle.active_target)
            .unwrap_or(0);
        let seat_slot = self.casting.summon_actor_slot.unwrap_or(ticks::SUMMON_SEAT);

        // The kernels index a retail-shaped seat row (`seat_map`): these
        // three are the same actors' seats in it.
        let map = self.cast_seat_map();
        let (r_caster, r_victim, r_seat) = (
            usize::from(map.retail(caster_slot)),
            usize::from(map.retail(victim_slot)),
            usize::from(map.retail(seat_slot)),
        );
        let mut caster = self.cast_view(caster_slot);
        let mut victim = self.cast_view(victim_slot);
        let mut seat = self.cast_view(seat_slot);
        let (mut caster_orig, mut victim_orig, mut seat_orig) = (caster, victim, seat);
        let mut run = CastModuleCodeRun {
            prot_entry: entry,
            busy: true,
            ..Default::default()
        };

        // The seven state-touching spawn stagers, by owning entry.
        match entry {
            906 => ticks::gizam_stager(&mut ctx, &mut seat, arm),
            909 => {
                ticks::viguro_stager(&mut ctx, &mut seat, arm);
            }
            922 => ticks::puera_stager(&mut ctx, arm),
            923 => ticks::gilium_stager(&mut ctx, arm),
            949 => ticks::water_crystals_stager(&mut victim, arm),
            _ => {}
        }

        // The tick bodies, by owning entry. `hit` is `None` because the fold
        // is the band seam's, not the tick's (see the note above).
        //
        // A capture-class module reaches its body through a **trampoline**
        // (`ticks::capture_tick_body`), which switches on the caster's queued
        // action id, so a multi-spell cell picks a different choreography for
        // each of its ids. Where the module has one, the trampoline decides
        // whether anything ticks at all: an id it does not name returns zero
        // and the drive loop proceeds.
        //
        // The arm has to be keyed on `(entry, body)`, never on the body VA
        // alone: six modules in the band put a tick body at `0x801F69D8` -
        // the load base itself - so PROT 0960's Neo Star Slash and PROT
        // 0965's Doomsday wear the same address in different images.
        let body = ticks::capture_tick_body(entry, spell_id);
        let has_trampoline = ticks::capture_trampoline_for(entry).is_some();
        // A player-Seru module whose camera arms are ported paces its own
        // arms: the director runs first and, while an arm's countdown holds,
        // the phase-chain body does not run at all (retail's arm returns
        // before any of its writes). Its shot is the camera the summon band's
        // `0x35` / `0x36` hand the battle camera.
        let profile = if has_trampoline {
            None
        } else {
            vm::cast_module_camera::module_profile(entry)
        };
        let direction = if has_trampoline {
            None
        } else {
            vm::cast_module_camera::module_director(entry).map(|direct| {
                let latched = *self
                    .casting
                    .module_cam
                    .victim_slot
                    .get_or_insert(victim_slot);
                let seats = self.module_cam_seats(caster_slot, latched);
                let mut st = self.module_cam_with_yaw_base();
                let d = direct(&mut st, ctx.phase, seats);
                self.store_module_cam(st);
                d
            })
        };
        // PROT 0903's settle arm halves the victim's animation rate on every
        // pass, held or not: the store (`lbu v0,0x69(a0)` / `srl v0,v0,0x1` /
        // `sb v0,0x21d(s2)` at `0x801F76C8..0x801F76D8`) sits past the
        // countdown's `bgtz` at `0x801F7654`, which jumps to it. The body's
        // own arm 12 runs only once the director lets the phase through, so
        // the half the hold covers is stored here - it is what slows the dead
        // victim's knock-back clip, whose fade the arm then waits out.
        if entry == 903
            && ctx.phase == ticks_a::GIMARD_SETTLE_ARM
            && let Some(a) = self.actors.get_mut(usize::from(victim_slot))
        {
            a.battle.anim_rate = vm::battle_anim_rate::AnimRate(ticks_a::GIMARD_SETTLE_ANIM_RATE);
        }
        run.camera_shot = direction.and_then(|d| d.shot);
        let mut capture_held = false;
        let mut capture_arm = None;
        let phase_in = ctx.phase;
        // A capture-class body's camera arms, on the phase it is about to
        // run (they make no gate of their own: the body's port does).
        if let Some(direct) = vm::cast_module_camera::capture_camera_director(
            entry,
            body.unwrap_or(vm::cast_module_camera::SINGLE_BODY),
        ) {
            let seats = self.module_cam_seats(caster_slot, victim_slot);
            let mut st = self.module_cam_with_yaw_base();
            let arm = direct(&mut st, ctx.phase, seats);
            self.store_module_cam(st);
            if let Some(v) = arm.latch
                && let Some(i) = self.caster_latch_index(caster_slot)
            {
                self.battle.monster_ai_state.dat[i] = v;
            }
            if arm.skips_fold {
                self.casting.module_skips_fold = true;
            }
            run.camera_shot = arm.shot;
            run.capture_drift = arm.drift;
            capture_held = arm.hold;
            capture_arm = Some(arm);
        }
        // PROT 0966 (Evil Seru Magic) has no other port: its seat half rides
        // the director's gate, on the phase the director is answering for.
        if entry == 966
            && let Some(arm) = capture_arm
        {
            let (phase, passed) = (ctx.phase, !arm.hold);
            if passed && let Some(t) = evil_seru_magic_fade(phase) {
                crate::fade::spawn_fade(&mut self.presentation.fade, &t, 1);
            }
            // The module's own move-VM stager hit lands mid-cast, not at the
            // fold: arm 10 spawns record `0x801F937C`, whose script waits
            // `0x7F` (`<< 3`, drained `scalar * delta` - 127 vsyncs) and then
            // runs op `0x20` with arm 4, the `0x100` never-kill sweep. That
            // is arm 11's gate (`0x80` vsyncs), so it lands before arm 26's
            // kill-capable hit, and the band's fold owes nothing more.
            let mut stager_hits = Vec::new();
            if passed && phase == vm::cast_module_camera::EVIL_SERU_MAGIC_STAGER_HIT_ARM {
                if let Some(r) =
                    self.run_cast_module_aoe_as(caster_slot, spell_id, AOE_STAGER_WORKING_ARM)
                {
                    stager_hits = r.aoe_hits;
                }
                self.casting.module_skips_fold = true;
            }
            let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
            let mut rolls: Vec<(u8, i32)> = Vec::new();
            if passed && phase == vm::cast_module_camera::EVIL_SERU_MAGIC_SWEEP_ARM {
                // Rolled only for the seats the hit visits, in seat order,
                // so the shared RNG cursor moves as retail's does.
                for s in 0..ctx.party_count {
                    if seats
                        .get(s as usize)
                        .is_some_and(ticks::aoe_seat_is_hittable)
                    {
                        let r = self
                            .capture_module_roll(
                                &ticks::EVIL_SERU_MAGIC_SWEEP_SHAPE,
                                caster_slot,
                                s,
                            )
                            .unwrap_or(0);
                        rolls.push((s, r));
                    }
                }
                self.refresh_seat_spirit(&mut seats);
            }
            let take = |s: u8| {
                rolls
                    .iter()
                    .find(|(seat, _)| *seat == s)
                    .map_or(0, |(_, r)| *r)
            };
            let hits = ticks::evil_seru_magic_seat_writes(
                phase,
                passed,
                ctx.party_count,
                r_caster as u8,
                &mut seats,
                take,
            );
            self.write_cast_seat_row(&seats);
            run.aoe_hits = stager_hits;
            let hits = self.cast_hits_to_engine(hits.iter().map(|h| ticks::AoeHit {
                seat: h.seat,
                applied: h.applied as i32,
            }));
            run.aoe_hits.extend(hits);
        }
        // A capture body with no camera director still gates its arms on
        // the module countdown: the arm's phase chain runs only on the tick
        // the gate lets through (`cast_module_camera::capture_countdown`).
        let arm_countdown = if capture_arm.is_none() {
            vm::cast_module_camera::capture_arm_countdowns(
                entry,
                body.unwrap_or(vm::cast_module_camera::SINGLE_BODY),
            )
            .and_then(|t| vm::cast_module_camera::arm_countdown(t, phase_in))
        } else {
            None
        };
        if let Some(a) = arm_countdown
            && a.holds(&mut self.casting.module_cam.countdown)
        {
            capture_held = true;
        }
        // A phase-chain body's hit lands on the tick its arm first runs -
        // retail calls the damage wrapper inside the arm - and an arm that
        // then waits on the hit seats' clips holds until they settle. The
        // runner itself holds no clip state, so both happen here.
        let chain = if has_trampoline {
            body.and_then(|b| ticks::chain_body_for(entry, b))
        } else {
            ticks::direct_chain_body(entry)
        };
        if let Some(arm) = chain.and_then(|c| c.arm(phase_in))
            && !capture_held
        {
            if arm.wrapper_site.is_some() && self.casting.module_hit_arm != Some(phase_in) {
                self.casting.module_hit_arm = Some(phase_in);
                self.fold_pending_cast();
                // The arm runs on the seats the hit left (a dead victim
                // takes PROT 0956's reaction branch, not its turn-steal).
                caster = self.cast_view(caster_slot);
                victim = self.cast_view(victim_slot);
                seat = self.cast_view(seat_slot);
                (caster_orig, victim_orig, seat_orig) = (caster, victim, seat);
            }
            if let Some(settle) = arm.settle {
                let settled = self.chain_settled(settle, caster_slot, victim_slot, &ctx);
                self.casting.module_settle_ticks =
                    self.casting.module_settle_ticks.saturating_add(1);
                if settled
                    || self.casting.module_settle_ticks > vm::cast_fatal_decision::SETTLE_TICK_LIMIT
                {
                    self.casting.module_settle_ticks = 0;
                } else {
                    capture_held = true;
                }
            }
        }
        run.camera_follow = direction.and_then(|d| d.follow);
        run.camera_end_frame = direction.is_some_and(|d| d.end_frame);
        run.camera_nudge = direction.and_then(|d| d.nudge);
        run.spawns = direction.map_or(&[], |d| d.spawns);
        run.vram_move = direction.and_then(|d| d.vram_move);
        run.caption = direction.and_then(|d| d.caption);
        run.fades = direction.map_or(&[], |d| d.fades);
        run.kills_fades = direction.is_some_and(|d| d.kills_fades);
        let held = direction.is_some_and(|d| d.hold) || capture_held;
        // A camera-only director owns the phase of a module whose tick body
        // is unported: its pass advances it, and it claims no tick.
        let camera_only = profile.is_some_and(|p| !p.paces_band() && p.owns_phase);
        if camera_only && !held {
            ctx.phase = ctx.phase.wrapping_add(1);
        }
        let step = if held && !camera_only {
            Some(ticks::CastTickStep::Busy)
        } else if held || camera_only {
            None
        } else if has_trampoline {
            match (entry, body) {
                (958, Some(ticks::BLAZING_SLASH_TICK)) => {
                    Some(ticks::blazing_slash_tick(&mut ctx, &mut victim, None))
                }
                (952, Some(ticks::ASTRAL_SLASH_TICK)) => {
                    Some(ticks::astral_slash_tick(&mut ctx, &mut caster, &mut victim))
                }
                (945, Some(ticks::WATER_COLUMN_TICK)) => {
                    Some(ticks::water_column_tick(&mut ctx, &mut victim, None, 0))
                }
                // PROT 0945's other choreography: the band's widest stat
                // write, on the caster.
                (945, Some(ticks::ALL_STATS_SURGE_TICK)) => {
                    let agl_record = self.cast_record_agl(caster_slot);
                    Some(ticks::all_stats_surge_tick(
                        &mut ctx,
                        &mut caster,
                        agl_record,
                    ))
                }
                (960, Some(ticks::PLASMA_STRIKE_TICK)) => {
                    // The burst arm's `0x1C0` roll. Retail aims it at
                    // `0x801C9370[0]` - seat 0, not the derived victim - so
                    // it rides the victim view only when the two coincide;
                    // the arm's other writes (the close, the knockdown) are
                    // the victim's either way.
                    let hit = if ctx.phase == ticks::PLASMA_STRIKE_BURST_ARM && victim_slot == 0 {
                        ticks::damage_shape_for(960)
                            .and_then(|sh| self.capture_module_roll(sh, caster_slot, 0))
                    } else {
                        None
                    };
                    // The burst is the module's one wrapper call (`jal
                    // 0x801DD6B4` at `0x801F8168`), and its HP writes are the
                    // cast's whole outcome beside arm `0x0C`'s flurry
                    // landing: once it has landed here, the band's generic
                    // fold owes nothing. Folding as well rolled the
                    // `0x1C0` a second time through the catalog def.
                    if hit.is_some() {
                        self.casting.module_skips_fold = true;
                    }
                    Some(ticks::plasma_strike_tick(
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        hit,
                    ))
                }
                (957, Some(ticks::SUMMON_EFFECT_TICK_B)) => {
                    Some(ticks::summon_effect_tick_b(&mut ctx, &mut victim))
                }
                (957, Some(ticks::SUMMON_EFFECT_TICK_A)) => {
                    Some(ticks::summon_effect_tick_a(&mut ctx, &mut victim, None))
                }
                // PROT 0942's Power Up reads the caster's own monster record
                // `+0x0E` and writes the AGL **base** half only.
                (942, Some(ticks::POWER_UP_TICK)) => {
                    let agl_record = self.cast_record_agl(caster_slot);
                    Some(ticks::power_up_tick(&mut ctx, &mut caster, agl_record))
                }
                // The three whole-row sweeps. Each rolls the module's own
                // baked power per hittable seat off this world's RNG cursor,
                // in seat order, so the draw order stays retail's; the two
                // 1-in-8 status draws Chaos Breath makes per seat come off
                // the same cursor.
                (938, Some(ticks::CHAOS_BREATH_TICK))
                | (938, Some(ticks::MYSTIC_CIRCLE_TICK))
                | (965, Some(ticks::DOOMSDAY_TICK)) => {
                    let body = body.unwrap_or_default();
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let sweep_arm = ctx.phase;
                    let rolls = self.sweep_status_rolls(&ctx, &seats, body, caster_slot);
                    self.refresh_seat_spirit(&mut seats);
                    // The module's own baked power, rolled per seat: these
                    // three write `+0x14C` themselves, so they own the
                    // outcome and `fold_pending_cast` skips the generic fold
                    // for them (see `sweep_status_rolls`).
                    let take = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _, _)| *s == seat)
                            .map(|(_, d, _)| *d)
                            .unwrap_or(0)
                    };
                    let status = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _, _)| *s == seat)
                            .map(|(_, _, st)| *st)
                            .unwrap_or((1, 1))
                    };
                    let (step, hits) = match body {
                        ticks::CHAOS_BREATH_TICK => ticks::chaos_breath_tick(
                            &mut ctx,
                            &mut caster,
                            &mut seats,
                            take,
                            status,
                        ),
                        ticks::MYSTIC_CIRCLE_TICK => ticks::mystic_circle_tick(
                            &mut ctx,
                            &mut seats,
                            sweep_arm == ticks::MYSTIC_CIRCLE_SWEEP_ARM,
                            take,
                        ),
                        _ => ticks::doomsday_tick(
                            &mut ctx,
                            &mut seats,
                            sweep_arm == ticks::DOOMSDAY_SWEEP_ARM,
                            take,
                        ),
                    };
                    self.write_cast_seat_row(&seats);
                    run.aoe_hits = self.cast_hits_to_engine(hits.iter().map(|h| ticks::AoeHit {
                        seat: h.seat,
                        applied: h.applied as i32,
                    }));
                    Some(step)
                }
                (951, Some(ticks::CHAOS_FLARE_TICK)) => {
                    Some(ticks::chaos_flare_tick(&mut ctx, &mut victim, None))
                }
                (951, Some(ticks::SCYTHE_WIND_TICK)) => {
                    Some(ticks::scythe_wind_tick(&mut ctx, &mut victim, None))
                }
                (952, Some(ticks::BLOODY_HORNS_TICK)) => Some(ticks::bloody_horns_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                // PROT 0955's six-spell cell. Its four status / buff bodies
                // write simulation state no damage fold can express, so they
                // run here and their outcome is reported back on the run.
                (955, Some(ticks::WHITE_SHIELD_TICK)) => {
                    // Retail reads the caster's own monster **record** at
                    // `0x801C9348[seat - 3]` rather than the live actor, so
                    // the buff is idempotent; the engine's nearest equivalent
                    // is the un-buffed defence split it seeded at battle load.
                    let record = (caster.udf_base, caster.ldf_base);
                    Some(ticks::white_shield_tick(&mut ctx, &mut caster, record))
                }
                (955, Some(ticks::KISS_OF_DEATH_TICK)) => {
                    let roll = Some(self.next_rand());
                    let (step, refund) = ticks::kiss_of_death_tick(&mut ctx, &mut victim, roll);
                    run.item_refund = refund;
                    Some(step)
                }
                (955, Some(ticks::MELT_SPRAY_TICK)) => {
                    let debuff = ctx.phase == ticks::MELT_SPRAY_DEBUFF_ARM;
                    Some(ticks::melt_spray_tick(&mut ctx, &mut victim, debuff))
                }
                (955, Some(ticks::TERROR_SCREAM_TICK)) => {
                    let (step, refund) = ticks::terror_scream_tick(&mut ctx, &mut victim);
                    run.item_refund = refund;
                    Some(step)
                }
                (955, Some(ticks::POWER_CHARGE_TICK)) => {
                    Some(ticks::power_charge_tick(&mut ctx, &mut caster))
                }
                (955, Some(ticks::VOID_ACCESSORIES_TICK)) => {
                    let rolls = Some((self.next_rand(), self.next_rand()));
                    let accessories = self.cast_victim_accessories(victim_slot);
                    let (step, outcome) =
                        ticks::void_accessories_tick(&mut ctx, &mut victim, accessories, rolls);
                    run.voided_accessory = outcome;
                    Some(step)
                }
                // PROT 0964's Element Change re-rolls the first monster
                // seat's element. `last_roll` is derived from what the record
                // holds now, which is what the previous commit wrote.
                (964, Some(ticks::ELEMENT_CHANGE_TICK)) => {
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let last_roll = self.cast_element_change_last_roll();
                    let mut rolls: Vec<u32> = Vec::new();
                    if ctx.phase == 0 {
                        for _ in 0..=ticks::ELEMENT_CHANGE_MAX_REROLLS {
                            rolls.push(self.next_rand());
                        }
                    }
                    let mut cursor = rolls.into_iter();
                    let (step, outcome) =
                        ticks::element_change_tick(&mut ctx, &mut seats, last_roll, || {
                            cursor.next().unwrap_or(0)
                        });
                    self.write_cast_seat_row(&seats);
                    if let Some(out) = outcome {
                        self.battle
                            .monster_ai_state
                            .set_counter(i32::from(out.roll));
                        self.apply_cast_element_change(out.element);
                        run.element_change = Some((out.element, out.group));
                    }
                    Some(step)
                }
                // --- W1-D: fourteen arms ---
                // PROT 0940 / 0941 / 0943 / 0944 / 0950 / 0956 / 0962, ported
                // in `legaia_engine_vm::cast_arm_ticks`. Three of these bodies
                // wear the VA `0x801F6A04` in three different images, which is
                // why every arm below names its entry.
                (940, Some(arms::GLARE_DIVIDE_BLIND_TICK)) => {
                    // The body works on retail's first monster seat.
                    let first = self
                        .engine_slot_for_retail_pool(vm::cast_module_ticks::FIRST_MONSTER_SEAT)
                        .unwrap_or(vm::cast_module_ticks::FIRST_MONSTER_SEAT);
                    let mut seat = self.cast_view(first);
                    let mut ext = self.cast_arm_ext_state(first);
                    let shield_arm = ctx.phase == arms::MYSTIC_SHIELD_ARM;
                    let step = arms::glare_divide_blind_tick(&mut ctx, &mut seat, &mut ext);
                    self.write_cast_view(first, &seat);
                    self.write_cast_arm_ext_state(first, &ext);
                    if shield_arm {
                        // `_DAT_8007BD84 = FUN_80021B04(..)` - the shield's
                        // effect handle; the engine carries it as a flag.
                        self.battle.monster_ai_state.flag_bd84 = 1;
                    }
                    Some(step)
                }
                (940, Some(arms::GLARE_DIVIDE_SPLIT_TICK)) => {
                    let ext = self.cast_arm_ext_state(caster_slot);
                    let saved = self.casting.module_split_saved_target.unwrap_or(0);
                    let roll = (ctx.phase == 2).then(|| self.next_rand());
                    let (step, split) = arms::glare_divide_split_tick(
                        &mut ctx,
                        &mut caster,
                        &ext,
                        spell_id,
                        saved,
                        roll,
                    );
                    if let Some(sp) = split {
                        self.casting.module_split_saved_target = Some(sp.saved_caster_target);
                        self.apply_glare_divide_split(caster_slot, &sp);
                    }
                    Some(step)
                }
                (941, Some(arms::STEAL_TICK)) => {
                    let outcome = (ctx.phase == 1)
                        .then(|| self.roll_cast_steal(victim_slot))
                        .flatten();
                    let (step, taken) =
                        arms::steal_tick(&mut ctx, &mut caster, CAST_STEAL_RUN_CLIP, outcome);
                    if let Some(arms::StealOutcome::FromBag { item }) = taken {
                        let _removed = self.take_one_from_bag(item);
                    }
                    if let Some(outcome) = taken {
                        self.stash_cast_steal(caster_slot, outcome);
                    }
                    Some(step)
                }
                (941, Some(arms::STEAL_SWEEP_TICK)) => {
                    let (step, hits) = self.run_cast_arm_sweep(
                        &mut ctx,
                        941,
                        arms::STEAL_SWEEP_TICK,
                        caster_slot,
                        &mut caster,
                    );
                    run.aoe_hits = hits;
                    Some(step)
                }
                (943, Some(arms::CURSE_SINGLE_TICK)) => {
                    Some(arms::curse_single_tick(&mut ctx, &mut caster, &mut victim))
                }
                (943, Some(arms::CURSE_MP_DRAIN_TICK)) => {
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let mut exts: Vec<arms::CastArmExtState> = self.cast_arm_ext_row();
                    let (step, _drained) =
                        arms::curse_mp_drain_tick(&mut ctx, &mut seats, &mut exts);
                    self.write_cast_seat_row(&seats);
                    self.write_cast_arm_ext_row(&exts);
                    Some(step)
                }
                (944, Some(arms::GUILTY_CROSS_TICK)) => Some(arms::guilty_cross_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                (944, Some(arms::GUILTY_CROSS_CURSE_TICK)) => {
                    let code = caster.target_code;
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let (step, _marked) =
                        arms::guilty_cross_curse_tick(&mut ctx, code, &mut caster, &mut seats);
                    self.write_cast_seat_row(&seats);
                    Some(step)
                }
                (950, Some(arms::ROLLING_FLARE_TICK)) => Some(arms::rolling_flare_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                (950, Some(arms::ROLLING_FLARE_SWEEP_TICK)) => {
                    let sweep = ctx.phase
                        == arms::arm_sweep_arm(950, arms::ROLLING_FLARE_SWEEP_TICK).unwrap_or(0xFF);
                    let rolls = self.cast_arm_sweep_rolls(
                        &ctx,
                        950,
                        arms::ROLLING_FLARE_SWEEP_TICK,
                        caster_slot,
                    );
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let take = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _)| *s == seat)
                            .map(|(_, d)| *d)
                            .unwrap_or(0)
                    };
                    let (step, hits) = arms::rolling_flare_sweep_tick(
                        &mut ctx,
                        &mut caster,
                        &mut seats,
                        sweep,
                        take,
                    );
                    self.write_cast_seat_row(&seats);
                    run.aoe_hits = self.cast_hits_to_engine(hits.iter().map(|h| ticks::AoeHit {
                        seat: h.seat,
                        applied: h.applied as i32,
                    }));
                    Some(step)
                }
                (956, Some(arms::WATER_HAZARD_TICK)) => {
                    let code = caster.target_code;
                    let rolls =
                        self.cast_arm_sweep_rolls(&ctx, 956, arms::WATER_HAZARD_TICK, caster_slot);
                    let status: Vec<(u8, u32)> = if ctx.phase == 2 {
                        rolls.iter().map(|(s, _)| (*s, self.next_rand())).collect()
                    } else {
                        Vec::new()
                    };
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let take = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _)| *s == seat)
                            .map(|(_, d)| *d)
                            .unwrap_or(0)
                    };
                    let st = |seat: u8| {
                        status
                            .iter()
                            .find(|(s, _)| *s == seat)
                            .map(|(_, r)| *r)
                            .unwrap_or(1)
                    };
                    let (step, hits) = arms::water_hazard_tick(
                        &mut ctx,
                        r_caster as u8,
                        code,
                        &mut caster,
                        &mut seats,
                        take,
                        st,
                    );
                    self.write_cast_seat_row(&seats);
                    run.aoe_hits = self.cast_hits_to_engine(hits.iter().map(|h| ticks::AoeHit {
                        seat: h.seat,
                        applied: h.applied as i32,
                    }));
                    Some(step)
                }
                // `arrived = true` on all three: the band's approach leg
                // (`CasterStagePhase::Approach`, gated on
                // `capture_body_approaches`) walks the caster in and holds
                // the module until the metric reads zero, so by the time
                // this body runs its poll would read arrival.
                //
                // Retail's predicate is `FUN_8004E2F0(ctx[+0x13],
                // caster[+0x1DD]) == 0` - the **zero** side, not the non-zero
                // one: `sll v0,v0,0x10; bne v0,zero,<hold>` at
                // `0x801F7C0C` / `0x801F7D08` sends a non-zero metric to the
                // "stage the run clip and hold" arm, so the metric reads
                // "still out of reach" and zero is arrival. The engine has
                // that metric (`Self::battle_range_metric`, the port of the
                // same routine) and the 0x20 `FUN_80050BB8` calls the arrived
                // path makes are ported too
                // (`legaia_engine_vm::battle_separation`, a pairwise
                // separation nudge - NOT the approach itself).
                //
                // The walk itself is the band's approach leg: the body's arm
                // `0` stages the walk entry and turns the caster onto the
                // victim, and the walk clip's own root motion carries it.
                (962, Some(arms::BLADE_BREATH_A_TICK)) => Some(arms::blade_breath_a_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    true,
                    None,
                )),
                (962, Some(arms::BLADE_BREATH_B_TICK)) => Some(arms::blade_breath_b_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    true,
                    None,
                )),
                (962, Some(arms::BLADE_BREATH_C_TICK)) => Some(arms::blade_breath_c_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    true,
                    None,
                )),
                // --- end W1-D ---
                // The phase-chain bodies (`cast_module_ticks::chain_bodies`):
                // PROT 0942 `0xAA`, 0956 `0x75`, 0959, 0960 `0xA6`, 0961,
                // 0963 and 0964 `0xB0..=0xB2`. Their hits are the fold's.
                (e, Some(b)) if ticks::chain_body_for(e, b).is_some() => {
                    let chain = ticks::chain_body_for(e, b).expect("guarded");
                    let selector = if e == 964 {
                        self.cast_element_change_last_roll()
                    } else {
                        0
                    };
                    let t = ticks::run_chain_body(
                        chain,
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        &mut seat,
                        selector,
                    );
                    run.item_refund = t.refund;
                    Some(t.step)
                }
                // Every ported trampoline arm the band names. An arm whose
                // body has no port ticks nothing, which is exactly what
                // retail's fall-through does for an id the trampoline does
                // not name.
                _ => None,
            }
        } else {
            match entry {
                // --- W1-B: player Seru 0903..0908 ---
                // The first six `0x801CF4EC` arms - the player Seru-magic tick
                // bodies (`legaia_engine_vm::cast_seru_ticks_a`). Unlike the
                // summon-creature ticks below, five of the six sweep or
                // retarget seats other than the caster's own three, so they
                // take the whole seat row and the run writes it back before
                // the caster / victim / summon views are refreshed from it.
                //
                // No damage roll is fed in: the engine folds a cast's HP
                // outcome once at `cast_spell_on_slots_prepaid`, so the bodies
                // below take `None`.
                //
                // PROT 0907's kill / confuse fork is the exception, because it
                // is not a damage roll at all - it writes the victim's HP to
                // zero or `+0x16E |= 0x380` and there is no magnitude for the
                // fold to carry. Its verdict is drawn here, once per cast
                // (`Self::nighto_verdict`), and held on the cast state for
                // every later frame, mirroring retail's arm-0 draw into the
                // module words `0x801F8534` / `0x801F853C`.
                903..=908 => {
                    // PROT 0904's ring sweep: advance the ray, then resolve
                    // the cone once for this tick (both borrow `self`, which
                    // the seat row below does not allow).
                    let cone_seats: Vec<u8> =
                        if entry == 904 && ctx.phase == ticks_a::THEEDER_SWEEP_ARM {
                            use vm::cast_seru_ticks_a::{MONSTER_ROW_END, THEEDER_CONE_HALF_WIDTH};
                            // The ray the sweep arm tests this tick: from the
                            // beam root ahead of the summon seat to the tip the
                            // word `ctx+0x6D8` (after its own ramp) swings about
                            // the summon's facing.
                            let (mouth, tip) = self.theeder_ray(seat_slot, ctx.ctx_6d8);
                            let bearing = vm::battle_action::bearing_12bit_approx(
                                mouth[2], mouth[0], tip[2], tip[0],
                            );
                            self.seats_in_cone(
                                (mouth[0], mouth[2]),
                                bearing,
                                THEEDER_CONE_HALF_WIDTH,
                                // Retail's row `3..7`, in engine slots.
                                self.engine_slot_for_retail_pool(ticks::FIRST_MONSTER_SEAT)
                                    .unwrap_or(ticks::FIRST_MONSTER_SEAT)
                                    ..self
                                        .engine_slot_for_retail_pool(MONSTER_ROW_END)
                                        .unwrap_or(MONSTER_ROW_END),
                            )
                            .into_iter()
                            // ...and back into the kernel's row.
                            .map(|e| map.retail(e))
                            .collect()
                        } else {
                            Vec::new()
                        };
                    let nighto_outcome = if entry == 907 {
                        self.nighto_verdict(caster_slot, victim_slot, spell_id)
                    } else {
                        ticks_a::NightoOutcome::ConfuseResisted
                    };
                    let who = ticks_a::SeruSeats {
                        caster: r_caster as u8,
                        victim: r_victim as u8,
                        summon: r_seat as u8,
                    };
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    // Carry the three views into the row so a stager that ran
                    // above this match is not thrown away.
                    for (slot, view) in [(r_caster, caster), (r_victim, victim), (r_seat, seat)] {
                        if let Some(s) = seats.get_mut(slot) {
                            *s = view;
                        }
                    }
                    let (step, hits) = match entry {
                        903 => ticks_a::gimard_tick(&mut ctx, &mut seats, who, None),
                        904 => {
                            // Arm 12's cone gate is the host's: retail reads
                            // the ring's rim bearing and every seat's, and the
                            // module only ever sees "in" or "out". The damage
                            // magnitude stays the fold's, so an in-cone seat
                            // gets a zero roll and the arm's presentation half
                            // (render flag, reaction bits) runs.
                            let in_cone = cone_seats.clone();
                            // Arm 4 seats the creature between the caster and
                            // the victim, facing the victim (`0x801F6EEC`).
                            // No later arm writes the seat's position, so it
                            // stays pinned there for the rest of the cast.
                            if ctx.phase == ticks_a::THEEDER_RISE_ARM {
                                self.casting.module_theeder.seat =
                                    self.theeder_seat_for(caster_slot, victim_slot);
                            }
                            if let Some(seat) = self.casting.module_theeder.seat {
                                self.pin_theeder_seat(seat_slot, seat);
                            }
                            let geom = self.theeder_geom(seat_slot);
                            let mut fx = self.casting.module_theeder;
                            let run = ticks_a::theeder_tick(
                                &mut ctx,
                                &mut seats,
                                who,
                                geom,
                                &mut fx,
                                |seat| in_cone.contains(&seat).then_some(0),
                            );
                            self.casting.module_theeder = fx;
                            run
                        }
                        905 => {
                            let restore = self.vera_restore(caster_slot, victim_slot, spell_id);
                            let (step, _) = ticks_a::vera_tick(&mut ctx, &mut seats, who, restore);
                            (step, Vec::new())
                        }
                        906 => ticks_a::gizam_tick(&mut ctx, &mut seats, who, |_| None),
                        907 => (
                            ticks_a::nighto_tick(&mut ctx, &mut seats, who, nighto_outcome),
                            Vec::new(),
                        ),
                        _ => ticks_a::zenoir_tick(&mut ctx, &mut seats, who, |_| None),
                    };
                    self.write_cast_seat_row(&seats);
                    caster = seats.get(r_caster).copied().unwrap_or(caster);
                    victim = seats.get(r_victim).copied().unwrap_or(victim);
                    seat = seats.get(r_seat).copied().unwrap_or(seat);
                    run.aoe_hits = self.cast_hits_to_engine(hits.iter().map(|h| ticks::AoeHit {
                        seat: h.seat,
                        applied: h.applied as i32,
                    }));
                    Some(step)
                }
                // --- end W1-B ---
                // The summon band's own tick bodies - no trampoline, the
                // `0x801CF4EC` arm calls them directly.
                918 => ticks::kemaro_tick(&mut ctx, &mut victim, None),
                922 => Some(ticks::puera_tick(&mut ctx, &mut victim, None)),
                924 => Some(ticks::ultimate_rave_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                )),
                925 => Some(ticks::spikefish_tick(&mut ctx, &mut caster)),
                927 => Some(ticks::juggernaut_tick(&mut ctx, &mut victim, None)),
                949 => Some(ticks::water_crystals_tick(&mut ctx, &mut victim, None)),
                // --- W1-C: player Seru 0909..0913 ---
                // These five bodies read and write the whole actor table -
                // the summon seat, the caster and the enemy row - so they
                // take the table rather than the three locals above, and the
                // locals are refreshed from it afterwards.
                //
                // PROT 0909 is the one entry in this band whose move-VM
                // stager also advances `ctx[+0x279]` (its arms `0` and `1`).
                // Retail reaches the stager from the effect script and the
                // tick from `FUN_801F1ED4` - two call sites - while this seam
                // runs both in one call, so the tick is skipped on a frame
                // the stager already stepped the phase. `casting.module_phase`
                // is not written back until the end of this function, so it
                // still holds the phase this call started on.
                909..=913 => {
                    let stager_stepped = ctx.phase != self.casting.module_phase;
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let summon = seat_slot;
                    let step = if entry == 909 && stager_stepped {
                        None
                    } else {
                        let (step, hits) = self.run_seru_b_tick(
                            entry,
                            &mut ctx,
                            &mut seats,
                            caster_slot,
                            summon,
                            victim_slot,
                        );
                        let hits = self.cast_hits_to_engine(hits);
                        run.aoe_hits.extend(hits);
                        Some(step)
                    };
                    if step.is_some() {
                        self.write_cast_seat_row(&seats);
                        // The three locals are written back below; take them
                        // from the table so this arm's writes survive.
                        caster = seats.get(r_caster).copied().unwrap_or(caster);
                        victim = seats.get(r_victim).copied().unwrap_or(victim);
                        seat = seats.get(r_seat).copied().unwrap_or(seat);
                    }
                    step
                }
                // --- end W1-C ---
                // The phase-chain bodies whose tick arm calls them directly
                // (`cast_module_ticks::chain_bodies`): PROT 0919, 0935, 0936,
                // 0937, 0939, 0947 and 0948. Their hits and heals are the
                // fold's.
                e if ticks::direct_chain_body(e).is_some() => {
                    let chain = ticks::direct_chain_body(e).expect("guarded");
                    let phase_before = ctx.phase;
                    let t = ticks::run_chain_body(
                        chain,
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        &mut seat,
                        0,
                    );
                    run.item_refund = t.refund;
                    // PROT 0919 (Spoon) arm 7 also runs the party cure ladder
                    // and its tier-4 AP doubling - the half of the arm the
                    // fold does not own (its heal is the fold's). Once, on
                    // the frame the arm lets the phase through.
                    if e == 919
                        && phase_before == vm::cast_seru_ticks_b::SPOON_HEAL_ARM
                        && ctx.phase != phase_before
                    {
                        // The views carry this frame's chain writes; seat them
                        // in the row before the sweep reads it.
                        let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                        for (slot, view) in [(r_caster, caster), (r_victim, victim), (r_seat, seat)]
                        {
                            if let Some(s) = seats.get_mut(slot) {
                                *s = view;
                            }
                        }
                        let cleanse = self.cure_selector(
                            caster_slot,
                            spell_id,
                            vm::cast_seru_ticks_b::ORB_CLEANSE_MIN_LEVEL,
                        );
                        vm::cast_seru_ticks_b::spoon_cure_sweep(&mut seats, cleanse);
                        self.write_cast_seat_row(&seats);
                        caster = seats.get(r_caster).copied().unwrap_or(caster);
                        victim = seats.get(r_victim).copied().unwrap_or(victim);
                        seat = seats.get(r_seat).copied().unwrap_or(seat);
                    }
                    Some(t.step)
                }
                _ => None,
            }
        };
        if let Some(step) = step {
            run.busy = step == ticks::CastTickStep::Busy;
            run.tick_ported = true;
        } else if let Some(arm) = capture_arm {
            // A capture director over a body with no port (a ported body, or
            // a holding arm, has already produced a step) owns its phase.
            run.tick_ported = true;
            run.busy = arm.next.is_some();
            if let Some(next) = arm.next {
                ctx.phase = next;
            }
        }
        // The finishing arm's `ctx[+0x6DA] = 0x780`, which the Done band's
        // case 6 frames from.
        if run.tick_ported
            && !run.busy
            && let Some(yaw) = vm::cast_module_camera::capture_exit_yaw_base(
                entry,
                if has_trampoline {
                    body.unwrap_or_default()
                } else {
                    vm::cast_module_camera::SINGLE_BODY
                },
            )
            && let Some(cam) = self.battle.camera.as_mut()
        {
            cam.set_action_yaw_base(yaw);
        }

        // Each view writes back only what the tick changed in it, folded onto
        // the slot's live state: the three views can name one actor (a
        // self-targeted cast's victim is its caster), and a whole-view write
        // of the second would undo the first's stores - PROT 0960's phase-5
        // stage of clip `0x0D` on Lu Delilas, read back as never staged,
        // held the band in `0x70` for good.
        for (slot, view, orig) in [
            (caster_slot, &caster, &caster_orig),
            (victim_slot, &victim, &victim_orig),
            (seat_slot, &seat, &seat_orig),
        ] {
            let mut live = self.cast_view(slot);
            live.fold_writes(orig, view);
            self.write_cast_view(slot, &live);
        }
        // PROT 0948's beam: arm 2 zeroes the builder's counter as it passes,
        // and arm 3 calls the builder (`jal 0x801F726C` at `0x801F6EF4`)
        // ahead of its own gate, so it draws on every tick the arm runs.
        self.casting.module_beam_live = false;
        if entry == vm::cast_module_ticks::CROSS_BEAM_ENTRY {
            if phase_in == 2 && ctx.phase != phase_in {
                self.casting.module_beam_counter = 0;
            }
            if phase_in == 3 {
                self.casting.module_beam_counter +=
                    vm::cast_module_ticks::CROSS_BEAM_COUNTER_PER_TICK;
                self.casting.module_beam_live = true;
            }
        }
        // The arm passed: its re-arm of the countdown word.
        if let Some(a) = arm_countdown
            && !capture_held
            && ctx.phase != phase_in
        {
            a.pass(&mut self.casting.module_cam.countdown);
        }
        // PROT 0925's (the Spikefish flute's) outcome arm: on an ordinary
        // fight it stages the party's flee, and on every fight its round
        // tail moves the turn cursor by the living party
        // ([`vm::cast_module_ticks::spikefish_round_tail`]).
        if entry == SPIKEFISH_ENTRY
            && phase_in == vm::cast_module_ticks::SPIKEFISH_OUTCOME_ARM
            && ctx.phase != phase_in
        {
            self.spikefish_outcome(&mut ctx);
        }
        self.casting.module_ctx_278 = ctx.ctx_278;
        self.casting.module_ring_angle = ctx.ctx_6d8;
        self.casting.module_phase = ctx.phase;
        // The turn-steal arms bump `ctx[+0x1A]`; it is a context byte, so it
        // has to travel back out of the view.
        self.battle_ctx.turn_cursor = ctx.turn_cursor;
        run.phase = ctx.phase;
        run.ctx_278 = ctx.ctx_278;
        // PROT 0955's two arms that reach outside the battle actor: the
        // refunded item goes back in the bag (retail's `FUN_800421D4`), and
        // the voided accessory is cleared out of the character record and
        // handed back (retail's record write plus `FUN_80042558`).
        if let Some(item) = run.item_refund {
            let _ = self.party.inventory.add(item, 1);
        }
        if let Some(out) = run.voided_accessory
            && let Some(id) = out.voided
        {
            let rslot = self.party_roster_slot(victim_slot as usize);
            if let Some(rec) = self.party.roster.members.get_mut(rslot) {
                let mut eq = rec.equipment();
                if let Some(slot) = eq.slots.get_mut(ACCESSORY_EQUIP_SLOT_0 + out.slot as usize) {
                    *slot = 0;
                }
                rec.set_equipment(eq);
            }
            let _ = self.party.inventory.add(id, 1);
            self.refresh_party_ability_bits();
        }
        Some(run)
    }

    // --- W1-C: player Seru 0909..0913 ---
    /// Dispatch one of the five player-Seru tick bodies for this frame
    /// ([`legaia_engine_vm::cast_seru_ticks_b`], PROT 0909..0913, action ids
    /// `0x87..=0x8B`).
    ///
    /// The damage and heal inputs are deliberately neutral. The engine folds
    /// a cast's HP outcome exactly once, at
    /// [`Self::cast_spell_on_slots_prepaid`], so what runs here is each
    /// module's phase machine, its `ctx+0x278` discipline and its staging /
    /// render writes; the damage step itself stays a tested kernel rather
    /// than becoming a second application. That is the same posture every
    /// other non-sweep tick body in the band takes, and it is why PROT 0911's
    /// heal amount is passed as `0`.
    ///
    /// The magnitude is not lost by that: `seru_tick_heal_amount` computes
    /// `cast_seru_ticks_b::orb_heal_amount` of the caster's per-spell magic
    /// level (the character record's `+0x161` byte, found by scanning the
    /// learned-id list at `+0x13D`) and overrides the spell catalog's
    /// placeholder inside the fold, so the amount a live Orb restores is
    /// retail's `(level << 6) + 0x1C0` clamped to the seat's missing HP.
    /// Passing it here as well would restore twice, once per owner - which is
    /// exactly what PROT 0905 used to do.
    pub(super) fn run_seru_b_tick(
        &mut self,
        entry: u32,
        ctx: &mut vm::cast_module_ticks::CastModuleCtx,
        seats: &mut [vm::cast_module_ticks::CastActorState],
        caster_slot: u8,
        summon_slot: u8,
        victim_slot: u8,
    ) -> (
        vm::cast_module_ticks::CastTickStep,
        Vec<vm::cast_module_ticks::AoeHit>,
    ) {
        use legaia_engine_vm::cast_seru_ticks_b as seru;
        use vm::cast_module_ticks::{AoeHit, SweepHit};

        // `seats` is the retail-shaped row (`seat_map`); the kernels take
        // their seats in it, the engine's own reads stay engine-indexed.
        let map = self.cast_seat_map();
        let (r_caster, r_summon, r_victim) = (
            map.retail(caster_slot),
            map.retail(summon_slot),
            map.retail(victim_slot),
        );
        fn lift(hits: &[SweepHit]) -> Vec<AoeHit> {
            hits.iter()
                .map(|h| AoeHit {
                    seat: h.seat,
                    applied: h.applied as i32,
                })
                .collect()
        }
        match entry {
            909 => {
                let mut settle = self.casting.module_settle_countdown;
                let (step, sweep) =
                    seru::viguro_tick(ctx, seats, r_caster, r_summon, &mut settle, |_| 0);
                self.casting.module_settle_countdown = settle;
                (step, lift(&sweep.hits))
            }
            // PROT 0910 paces its strike on two timers - arm 7's wind-up and
            // arm 9's staggered slashes - built from the frame step
            // (`0x1F800393`) and the speed scalar (`0x1F80037D`), so the slash
            // reactions land on retail's cadence. Each of the four landings
            // runs `swordie_slash_step` with a neutral wrapper return: the
            // clamp, the hit counter and the per-slash reaction clip are
            // live, the HP outcome stays the fold's.
            //
            // Retail runs the body once a battle pass, which spans the frame
            // step's vsyncs, and drains by `rate * speed` there; the engine
            // runs it once a vsync, so its `rate` is `1` (passing the frame
            // step ran both timers at twice retail's speed). `speed` is the
            // scalar itself - the battle seating's normal rate `8` - not `1`:
            // the thresholds scale with it, and arm 7 stores it as the
            // summon's animation rate `+0x21D`, which at `1` played the
            // strike at an eighth of normal speed.
            910 => {
                let clock = seru::SwordieClock {
                    rate: 1,
                    speed: vm::battle_anim_rate::RATE_NORMAL,
                };
                let mut slashes = self.casting.module_swordie;
                let (step, hits) =
                    seru::swordie_tick(ctx, seats, r_summon, r_victim, &mut slashes, clock, |_| 0);
                self.casting.module_swordie = slashes;
                (step, lift(&hits))
            }
            911 => {
                let maxes: Vec<u16> = (0..map.retail_len())
                    .map(|r| {
                        map.engine(r as u8)
                            .and_then(|e| self.actors.get(usize::from(e)))
                            .map_or(0, |a| a.battle.max_hp)
                    })
                    .collect();
                // Spell id for a `cast_seru_ticks_b` entry: the player
                // Seru-magic block is linear, `entry = 903 + (id - 0x81)`.
                let spell_id = (entry - 903 + 0x81) as u8;
                let cleanse =
                    self.cure_selector(caster_slot, spell_id, seru::ORB_CLEANSE_MIN_LEVEL);
                let mut settle = self.casting.module_settle_countdown;
                let (step, _healed) =
                    seru::orb_tick(ctx, seats, r_summon, 0, cleanse, &mut settle, |s| {
                        maxes.get(s as usize).copied().unwrap_or(0)
                    });
                self.casting.module_settle_countdown = settle;
                (step, Vec::new())
            }
            912 => {
                let mut settle = self.casting.module_settle_countdown;
                let (step, hits) = seru::freed_tick(ctx, seats, r_summon, &mut settle, |_| 0);
                self.casting.module_settle_countdown = settle;
                (step, lift(&hits))
            }
            _ => {
                let mut settle = self.casting.module_settle_countdown;
                let (step, hit) = seru::nova_tick(ctx, seats, r_summon, r_victim, 0, &mut settle);
                self.casting.module_settle_countdown = settle;
                let hits: Vec<SweepHit> = hit.into_iter().collect();
                (step, lift(&hits))
            }
        }
    }
    // --- end W1-C ---

    /// Run the band's two whole-row AoE stagers - PROT 0927 (Juggernaut,
    /// `FUN_801F85A8`) and PROT 0966 (Evil Seru Magic, `FUN_801F8D64`) -
    /// which is where those two casts' damage actually lands in retail.
    ///
    /// Both sweep a seat range with the module's own guards (skip dead, skip
    /// `+0x16E & 4`) and both clamp to `HP - 1`, so neither **sweep** can
    /// kill. That is a property of these two stagers only - PROT 0927's own
    /// tick (`0x801F6A84`) uses the unsigned `sltu` clamp instead and is
    /// kill-capable (`legaia_engine_vm::cast_module_ticks`,
    /// `docs/subsystems/cast-module.md`). ESM also
    /// stages each victim's own `+0x1F1` reaction and drops its animation rate
    /// to `2`. The roll per seat is the module's **baked** power through the
    /// module's own wrapper, drawn off this world's RNG cursor so the draw
    /// order stays retail's.
    ///
    /// Returns `None` when the spell names neither module, so the ordinary
    /// [`Self::fold_pending_cast`] path is unaffected.
    pub fn run_cast_module_aoe(&mut self, spell_id: u8, arm: u8) -> Option<CastModuleCodeRun> {
        self.run_cast_module_aoe_as(self.battle_ctx.active_actor, spell_id, arm)
    }

    /// [`Self::run_cast_module_aoe`] at the cast band's fold seam: the caster
    /// is the [`PendingCast`]'s, not whoever the context happens to point at,
    /// and the arm is the working one of the module's nine.
    pub(super) fn run_cast_module_aoe_for(
        &mut self,
        caster: u8,
        spell_id: u8,
    ) -> Option<CastModuleCodeRun> {
        self.run_cast_module_aoe_as(caster, spell_id, AOE_STAGER_WORKING_ARM)
    }

    pub(super) fn run_cast_module_aoe_as(
        &mut self,
        caster: u8,
        spell_id: u8,
        arm: u8,
    ) -> Option<CastModuleCodeRun> {
        use vm::cast_module_ticks as ticks;

        let entry = self.cast_module_for(spell_id)?;
        let shape = ticks::damage_shape_for(entry).filter(|s| s.never_kills)?;
        let ctx = self.cast_module_ctx();

        let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();

        // Pre-roll so the sweep borrows nothing from `self`, but roll only
        // for the seats the sweep will actually hit and in the order it visits
        // them: retail's loop calls the wrapper *after* its two skip guards,
        // so a dead or non-targetable seat draws nothing and the shared RNG
        // cursor must not advance for it either.
        let order: Vec<u8> = if entry == 966 {
            (0..ctx.party_count).collect()
        } else {
            (0..ctx.monster_count)
                .map(|i| ticks::FIRST_MONSTER_SEAT.saturating_add(i))
                .collect()
        };
        let mut rolls: Vec<(u8, i32)> = Vec::with_capacity(order.len());
        // `order` is in the kernels' retail-shaped row; the roll reads the
        // engine slot behind each seat.
        let map = self.cast_seat_map();
        for seat in order {
            let hittable = seats
                .get(seat as usize)
                .is_some_and(ticks::aoe_seat_is_hittable);
            let Some(target) = map.engine(seat).filter(|_| hittable) else {
                continue;
            };
            let roll = self
                .capture_module_roll(shape, caster, target)
                .unwrap_or_default();
            rolls.push((seat, roll));
        }
        self.refresh_seat_spirit(&mut seats);
        let take = |seat: u8| {
            rolls
                .iter()
                .find(|(s, _)| *s == seat)
                .map(|(_, r)| *r)
                .unwrap_or_default()
        };

        let hits = if entry == 966 {
            ticks::evil_seru_magic_stager(&ctx, &mut seats, arm, take)
        } else {
            ticks::juggernaut_stager(&ctx, &mut seats, arm, take)
        };
        self.write_cast_seat_row(&seats);
        let hits = self.cast_hits_to_engine(hits);
        Some(CastModuleCodeRun {
            prot_entry: entry,
            phase: ctx.phase,
            ctx_278: ctx.ctx_278,
            busy: false,
            tick_ported: true,
            aoe_hits: hits,
            item_refund: None,
            voided_accessory: None,
            element_change: None,
            camera_shot: None,
            camera_follow: None,
            camera_end_frame: false,
            camera_nudge: None,
            capture_drift: None,
            spawns: &[],
            vram_move: None,
            caption: None,
            fades: &[],
            kills_fades: false,
        })
    }

    /// One hit off a module's own damage shape: its baked power, its wrapper,
    /// this world's RNG cursor. The raw signed wrapper return - the caller
    /// applies the module's clamp.
    pub(super) fn capture_module_roll(
        &mut self,
        shape: &vm::cast_module_ticks::CastDamageShape,
        attacker: u8,
        target: u8,
    ) -> Option<i32> {
        use legaia_engine_vm::battle_damage_wrappers::{WrapperAttacker, WrapperDefender};
        use vm::cast_module_ticks::roll_module_hit;

        let element_affinity_pct = self.enemy_affinity_pct(attacker, target);
        let attacker_hp = self.actors.get(attacker as usize)?.battle.hp;
        let defender = self.summon_roll_defender(target)?;
        let a = WrapperAttacker {
            hp: attacker_hp,
            agl: self
                .battle
                .accuracy
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            spell_power: self
                .battle
                .attack
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            status: 0,
        };
        let d = WrapperDefender {
            hp: defender.hp,
            agl: defender.agl,
            stat_a: defender.stat_a,
            stat_b: defender.stat_b,
            status: 0,
            guard: 0,
        };
        let rng = [
            self.next_rand() as u16,
            self.next_rand() as u16,
            self.next_rand() as u16,
        ];
        let net = roll_module_hit(shape, 0, &a, &d, element_affinity_pct, rng, || {
            self.next_rand() as u16
        });
        Some(self.finish_module_hit(shape, attacker, target, net))
    }

    /// The finisher half of a module wrapper hit. Both per-move wrappers end
    /// in the shared finisher: `FUN_801DD4B0` calls `jal 0x801DDB30` at
    /// `0x801DD678` on its own attacker / defender roll words and returns
    /// their difference afterwards (`subu v0,v1,v0` at `0x801DD6A8`), so the
    /// net a module stores into `+0x14C` is **post**-finisher: the party
    /// resist ladder (skipped by `FUN_801DD6B4`'s `param_5 = 1`), Mystic
    /// Shield's enemy-defender halve, the guard halve (`+0x1DE == 4`), the
    /// no-damage floor and the `9999` cap - and the finisher's spirit stage
    /// fills the defender's gauge from the same hit. The earlier port
    /// returned the raw wrapper net, so a guarding member took Cort's Mystic
    /// Circle whole and gained no Spirit from it.
    ///
    /// `SharedSummon` (PROT 0927, `FUN_801DD0AC`'s summon branch) keeps the
    /// raw net: its finisher arguments (attacker slot `7`, the power-percent
    /// scale) are the shared kernel's own and are not modelled here.
    ///
    /// PORT: FUN_801DD4B0 (`0x801DD66C..0x801DD6AC`, the finisher call and return)
    pub(super) fn finish_module_hit(
        &mut self,
        shape: &vm::cast_module_ticks::CastDamageShape,
        attacker: u8,
        target: u8,
        net: i32,
    ) -> i32 {
        use legaia_engine_vm::battle_damage_wrappers::{
            ATK_WRAPPER_BYPASSES_PARTY_RESIST, INT_WRAPPER_BYPASSES_PARTY_RESIST,
        };
        use vm::battle_formulas::{DamageFinish, damage_finish_lazy};
        use vm::cast_module_ticks::CastWrapper;

        let bypass_party_resist = match shape.wrapper {
            CastWrapper::Respect => INT_WRAPPER_BYPASSES_PARTY_RESIST,
            CastWrapper::Bypass => ATK_WRAPPER_BYPASSES_PARTY_RESIST,
            CastWrapper::SharedSummon => return net,
        };
        let party_count = self.party.party_count;
        let attacker_element = self.monster_seat_element(attacker as usize).unwrap_or(7);
        let finish = DamageFinish {
            predamage: net.max(0) as u32,
            attacker_slot: if attacker < party_count { 0 } else { 3 },
            defender_slot: if target < party_count { 0 } else { 3 },
            attacker_element,
            defender_resist: self.defender_resist(target),
            defender_guarding: self
                .battle
                .guarding
                .get(target as usize)
                .copied()
                .unwrap_or(false),
            enemy_defender_halve: self.mystic_shield_up(),
            bypass_party_resist,
            summon_power_pct: 100,
            floor_rand: 0,
        };
        let over = damage_finish_lazy(&finish, || self.next_rand() as u16).min(9999);
        self.accrue_spirit_gauge(target, over as u16);
        over as i32
    }

    /// Carry the spirit gauges [`Self::finish_module_hit`] filled into seat
    /// snapshots taken before the rolls, so the tick's write-back does not
    /// restore the pre-hit gauge. `seats` is the retail-shaped row
    /// ([`Self::cast_seat_row`]).
    pub(super) fn refresh_seat_spirit(&self, seats: &mut [vm::cast_module_ticks::CastActorState]) {
        let map = self.cast_seat_map();
        for (seat, st) in seats.iter_mut().enumerate() {
            if let Some(a) = map
                .engine(seat as u8)
                .and_then(|e| self.actors.get(usize::from(e)))
            {
                st.spirit_gauge = a.battle.spirit_gauge;
            }
        }
    }

    /// Pre-roll the status draws one whole-row sweep makes, in the order
    /// retail visits its seats: the two `FUN_80056798` calls PROT 0938's
    /// `0x4E` body makes per hittable seat (`0x801F7888` / `0x801F78B0`),
    /// which decide Venom then Toxic.
    ///
    /// The **damage** roll comes first, per seat, because retail's loop calls
    /// the wrapper before the status draws: each of the three sweeps opens
    /// its seat body with `jal 0x801DD4B0` on its own baked power
    /// ([`vm::cast_module_ticks::sweep_damage_shape_for`]) and stores the
    /// clamped net into `+0x14C` itself. So these bodies **own** the cast's
    /// HP outcome the way the PROT 0927 / 0966 stagers do, and
    /// [`Self::fold_pending_cast`] must not also run the generic fold - which
    /// is the double-application trap. The clamp is the kill-capable one
    /// (`sltu` at `0x801F77EC` / `0x801F7118` / `0x801F77E0`), unlike the two
    /// stagers' `HP - 1`.
    ///
    /// Returns `(seat, damage, (status_a, status_b))` in visit order.
    pub(super) fn sweep_status_rolls(
        &mut self,
        ctx: &vm::cast_module_ticks::CastModuleCtx,
        seats: &[vm::cast_module_ticks::CastActorState],
        body: u32,
        caster: u8,
    ) -> Vec<(u8, i32, (u32, u32))> {
        use vm::cast_module_ticks as ticks;
        // Only the body's own sweep arm draws anything: retail reaches the
        // wrapper and the two status calls inside that one arm, so rolling on
        // every re-entry would run the shared `rand()` cursor forward on
        // frames retail never draws.
        let sweep_arm = match body {
            ticks::CHAOS_BREATH_TICK => ticks::CHAOS_BREATH_SWEEP_ARM,
            ticks::MYSTIC_CIRCLE_TICK => ticks::MYSTIC_CIRCLE_SWEEP_ARM,
            _ => ticks::DOOMSDAY_SWEEP_ARM,
        };
        if ctx.phase != sweep_arm {
            return Vec::new();
        }
        let shape = ticks::sweep_damage_shape_for(body);
        let mut out = Vec::new();
        for seat in 0..ctx.party_count {
            let Some(s) = seats.get(seat as usize) else {
                continue;
            };
            // PROT 0938's `0xB7` body and PROT 0965's skip only a dead seat;
            // the `0x4E` body also skips `+0x16E & 4`.
            let hittable = if body == ticks::CHAOS_BREATH_TICK {
                ticks::aoe_seat_is_hittable(s)
            } else {
                s.hp != 0
            };
            if !hittable {
                continue;
            }
            let damage = match shape {
                Some(sh) => self.capture_module_roll(sh, caster, seat).unwrap_or(0),
                None => 0,
            };
            out.push((seat, damage, (self.next_rand(), self.next_rand())));
        }
        out
    }

    // --- W1-D: the fourteen trampoline arms ---

    /// The five extra record fields
    /// [`legaia_engine_vm::cast_arm_ticks::CastArmExtState`] carries, lifted
    /// off one actor slot.
    ///
    /// Two of them have an engine home: `+0x150` is the actor's live MP and
    /// `+0x172` its max HP. The other three do not, and the reason is that no
    /// routine in PROT 0903..0966 reads them back - `+0x152` (the MP base) and
    /// `+0x178` (where PROT 0943's drain stashes the old working MP) are
    /// write-only in the band, and `+0x1F3` sits one byte past the end of the
    /// engine's `+0x1DF..+0x1F2` action-parameter window. They round-trip
    /// through the view for the tick's own arithmetic and are dropped here.
    pub(super) fn cast_arm_ext_state(&self, slot: u8) -> vm::cast_arm_ticks::CastArmExtState {
        use vm::cast_arm_ticks::CastArmExtState;
        let Some(a) = self.actors.get(slot as usize) else {
            return CastArmExtState::default();
        };
        CastArmExtState {
            mp: a.battle.mp,
            mp_base: a.battle.mp,
            mp_stash: 0,
            max_hp: a.battle.max_hp,
            reaction_extra: 0,
        }
    }

    /// Write back the two halves of [`Self::cast_arm_ext_state`] the engine
    /// actually carries.
    pub(super) fn write_cast_arm_ext_state(
        &mut self,
        slot: u8,
        st: &vm::cast_arm_ticks::CastArmExtState,
    ) {
        let Some(a) = self.actors.get_mut(slot as usize) else {
            return;
        };
        a.battle.mp = st.mp;
        a.battle.max_hp = st.max_hp;
    }

    /// Pre-roll one whole-row sweep arm's per-seat damage, in the order retail
    /// visits its seats.
    ///
    /// Only the body's own sweep arm draws anything
    /// ([`legaia_engine_vm::cast_arm_ticks::arm_sweep_arm`]); rolling on every
    /// re-entry would run the shared RNG cursor forward on frames retail never
    /// draws. The sibling of [`Self::sweep_status_rolls`] for the arms
    /// `cast_arm_ticks` carries.
    pub(super) fn cast_arm_sweep_rolls(
        &mut self,
        ctx: &vm::cast_module_ticks::CastModuleCtx,
        entry: u32,
        body: u32,
        caster: u8,
    ) -> Vec<(u8, i32)> {
        let Some(arm) = vm::cast_arm_ticks::arm_sweep_arm(entry, body) else {
            return Vec::new();
        };
        if ctx.phase != arm {
            return Vec::new();
        }
        let Some(shape) = vm::cast_arm_ticks::arm_damage_shape_for(entry, body) else {
            return Vec::new();
        };
        let seats: Vec<vm::cast_module_ticks::CastActorState> = (0..ctx.party_count)
            .map(|s| self.cast_actor_state(s))
            .collect();
        let mut out = Vec::new();
        for (seat, s) in seats.iter().enumerate() {
            if !vm::cast_module_ticks::aoe_seat_is_hittable(s) {
                continue;
            }
            let seat = seat as u8;
            out.push((
                seat,
                self.capture_module_roll(shape, caster, seat).unwrap_or(0),
            ));
        }
        out
    }

    /// Drive one of the two table-dispatched sweep arms end to end: roll,
    /// tick, write the seats back.
    pub(super) fn run_cast_arm_sweep(
        &mut self,
        ctx: &mut vm::cast_module_ticks::CastModuleCtx,
        entry: u32,
        body: u32,
        caster_slot: u8,
        _caster: &mut vm::cast_module_ticks::CastActorState,
    ) -> (
        vm::cast_module_ticks::CastTickStep,
        Vec<vm::cast_module_ticks::AoeHit>,
    ) {
        let sweep = Some(ctx.phase) == vm::cast_arm_ticks::arm_sweep_arm(entry, body);
        let rolls = self.cast_arm_sweep_rolls(ctx, entry, body, caster_slot);
        let mut seats: Vec<vm::cast_module_ticks::CastActorState> = self.cast_seat_row();
        let take = |seat: u8| {
            rolls
                .iter()
                .find(|(s, _)| *s == seat)
                .map(|(_, d)| *d)
                .unwrap_or(0)
        };
        let (step, hits) = vm::cast_arm_ticks::steal_sweep_tick(ctx, &mut seats, sweep, take);
        self.write_cast_seat_row(&seats);
        (
            step,
            self.cast_hits_to_engine(hits.iter().map(|h| vm::cast_module_ticks::AoeHit {
                seat: h.seat,
                applied: h.applied as i32,
            })),
        )
    }

    /// PROT 0941's Steal resolution, both legs.
    ///
    /// **Monster seat** (`victim_slot >= party_count`, the engine's row): the static
    /// `SCUS_942.54` steal table `0x80077828 + monster_id * 2`, fields
    /// `[chance, item]` - the same table the player-side steal reads, and NOT
    /// a field of the PROT 867 monster record (`docs/formats/steal-table.md`).
    /// `rand() % 100 < chance` decides. `None` when no table is installed (a
    /// disc-free host) or the victim carries no monster id, which keeps a
    /// synthetic battle from inventing a steal and from drawing the roll.
    ///
    /// **Party seat**: the bag draw plus the consume. The rejection rule
    /// (`id != 0 && count != 0 && the item table knows the id`, up to `0x400`
    /// draws, one RNG draw per rejection so the shared cursor advances the way
    /// retail's does) is the module's, byte for byte.
    ///
    /// The **array** is retail's own: [`crate::world::ItemBag`] holds the
    /// physical 256 slots, so the draw rejects its way past a played-through
    /// bag's holes exactly as retail's does, and the third acceptance leg is
    /// the item record's **shop price** halfword (`0x80074368 + id*0xC + 2`,
    /// `0x801F789C`) rather than a tautology over the ids the bag already
    /// holds - the quest and found-only items carry a zero price and are
    /// unstealable.
    ///
    /// The re-roll floor is applied too. `0x801F77E8` arms it on
    /// `DAT_8007BD10[1] == 4` (the second present-party member's character id)
    /// and re-draws while `slot < *(i16*)0x8007B5EA`, which is `gp[+0x2D2]` -
    /// the active window's **start** (`gp = 0x8007B318`), so the arm confines
    /// the draw to the window's own half.
    ///
    /// The removal is asymmetric with the draw, and deliberately so: the draw
    /// is over the whole array while `FUN_80042310` scans only
    /// `[gp[+0x2D2], gp[+0x2D4])` and returns `0x100` for an id outside it,
    /// touching nothing. A steal that lands on the other half's slot therefore
    /// announces an item the party keeps.
    pub(in crate::world) fn roll_cast_steal(
        &mut self,
        victim_slot: u8,
    ) -> Option<vm::cast_arm_ticks::StealOutcome> {
        use vm::cast_arm_ticks::StealOutcome;
        // A monster seat is the engine's compacted row, not retail's fixed
        // `>= 3` (see [`Self::engine_slot_for_retail_pool`]).
        if victim_slot >= self.party.party_count.clamp(1, 3) {
            let entry = self
                .actors
                .get(victim_slot as usize)
                .and_then(|a| a.battle_monster_id)
                .and_then(|id| self.tables.steal_table.as_ref()?.entry(id))?;
            let roll = (self.next_rand() % 100) as u8;
            return Some(StealOutcome::FromMonster {
                chance: entry.chance_pct,
                item: entry.item_id,
                roll,
                hit: roll < entry.chance_pct,
            });
        }
        let bag: Vec<(u8, u8)> = self.party.inventory.slots().to_vec();
        // The price leg, precomputed so the draw closure can borrow `self`
        // for the RNG. Without a disc image there is no item table to read a
        // price from, so the leg cannot be evaluated and every id passes -
        // a disc-free host still spends retail's draws and rejects on the
        // two legs it can see.
        let mut priced = [true; 256];
        if let Some(data) = self.shops.item_shop_data.as_ref() {
            for (id, cell) in priced.iter_mut().enumerate() {
                *cell = data.price(id as u8) != 0;
            }
        }
        // `DAT_8007BD10[1] == 4`: the second present-party member's character
        // id, which the engine mirrors as `party_roster_slot(1) + 1`. A party
        // of one has no second member, and the port's identity mapping would
        // fabricate one, so the arm needs both.
        let floor = (self.party.party_count > 1 && self.party_roster_slot(1) as u8 + 1 == 4)
            .then(|| self.party.inventory.window_bounds().0.min(0xFF) as u8);
        // One `next_rand` per rejected slot, not a pre-drawn batch: retail
        // advances the shared cursor once per draw, so over-drawing would
        // desynchronise every later roll in the battle.
        let slot = vm::cast_arm_ticks::steal_pick_bag_slot(
            &bag,
            floor,
            || self.next_rand(),
            |id| priced[id as usize],
        );
        match slot.and_then(|s| bag.get(s as usize).copied()) {
            Some((item, _)) => Some(StealOutcome::FromBag { item }),
            None => Some(StealOutcome::BagEmpty),
        }
    }

    /// The inventory consume PROT 0941's Steal performs (`FUN_80042310`),
    /// through the window-bounded helper.
    ///
    /// Returns whether a slot was actually emptied: the helper's `0x100`
    /// sentinel says the id is outside the active window, and retail's own
    /// steal ignores the return, so the message has already been staged by the
    /// time the removal declines. The engine keeps the same order and reports
    /// the difference instead of hiding it.
    pub(in crate::world) fn take_one_from_bag(&mut self, item: u8) -> bool {
        self.party.inventory.consume_returning_slot(item, 1)
            != legaia_save::retail_inventory::NOT_IN_WINDOW
    }

    /// Materialise the seat PROT 0940's split allocated.
    ///
    /// Retail builds a whole actor: it copies the caster's monster-record
    /// pointer into `0x801C9348[seat]`, allocates a display object through
    /// `FUN_80054CB0` / `FUN_80024C88`, and unaligned-copies the caster's
    /// pose. The engine's equivalent is the caster's own actor record cloned
    /// into the seat, and the five simulation writes the arm makes on top of
    /// that (`+0x16C`, `+0x1DE`, `+0x1DF`, `+0x1DD`, `+0x14C` / `+0x172`) are
    /// what this applies.
    ///
    /// Returns `false` when the table has no seat there, which is retail's own
    /// bound: `ctx[+1]` indexes `actor_table` and the engine caps both counts
    /// at [`BATTLE_TABLE_SLOTS`].
    pub(super) fn apply_glare_divide_split(
        &mut self,
        caster_slot: u8,
        split: &vm::cast_arm_ticks::GlareDivideSplit,
    ) -> bool {
        use vm::cast_arm_ticks::{
            SPLIT_CLONE_ACTION, SPLIT_CLONE_CATEGORY, SPLIT_WEAK_AGL, SplitWeakened,
        };
        // `clone_seat` is a retail pool slot (`3 + ctx[+1]`): retail seats
        // monster `k` at `3 + k` whatever the party size, and the engine
        // compacts the monster row down to `party_count + k`
        // (`formation_span`'s module docs). So the clone takes the seat right
        // after the engine's seated monsters. Placing it at the retail index
        // left a small party's clones past the five-seat enemy row the
        // target picker walks (`World::battle_target_rows`): a lone Vahn on
        // `map01` faced a Divide clone in slot 6 that no command could
        // target, and the fight never ended. The bound is retail's own -
        // `ctx[+1] < 5` on both sides.
        let _ = split.clone_seat;
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        let table = self.actors.len().min(BATTLE_TABLE_SLOTS);
        let seated = (pc..table)
            .filter(|&s| self.actors[s].battle_monster_id.is_some())
            .count();
        let seat = pc + seated;
        if seat >= table || seated >= 5 {
            return false;
        }
        let Some(src) = self.actors.get(caster_slot as usize).cloned() else {
            return false;
        };
        let mut clone = src;
        clone.active = true;
        clone.battle.init_key = 0;
        clone.battle.action_category = SPLIT_CLONE_CATEGORY;
        if let Some(p) = clone.battle.params.first_mut() {
            *p = SPLIT_CLONE_ACTION;
        }
        clone.battle.active_target = vm::cast_module_ticks::TARGET_CODE_ENEMY_ROW;
        clone.battle.hp = split.clone_hp;
        clone.battle.max_hp = split.clone_hp;
        if split.weakened == Some(SplitWeakened::Clone) {
            clone.battle.hp = 1;
            clone.battle.mp = 0;
            clone.battle.atk_working = 1;
            clone.battle.agl = SPLIT_WEAK_AGL;
            clone.battle.agl_base = SPLIT_WEAK_AGL;
        }
        self.actors[seat] = clone;
        true
    }

    /// The three accessory ids PROT 0955's Void Accessories rolls between -
    /// `record[+0x19B + slot]` for the character seated at `slot`.
    pub(super) fn cast_victim_accessories(&self, slot: u8) -> [u8; 3] {
        let mut out = [0u8; 3];
        let Some(rec) = self
            .party
            .roster
            .members
            .get(self.party_roster_slot(slot as usize))
        else {
            return out;
        };
        // `+0x196` is equipment slot 0, so `+0x19B` - the module's
        // `+ 0x75E + 5` - is index 5, and the three accessory slots are
        // 5, 6, 7 (`legaia_save::character::EquipmentSlots`).
        let eq = rec.equipment();
        for (i, o) in out.iter_mut().enumerate() {
            *o = eq
                .slots
                .get(ACCESSORY_EQUIP_SLOT_0 + i)
                .copied()
                .unwrap_or(0);
        }
        out
    }

    /// Apply one enemy-cast hit through retail's **safe** applier - the hit
    /// arm of `FUN_801E09F8`'s per-slot effect-child driver
    /// (`legaia_engine_vm::battle_cast_census::effect_child_hit`).
    ///
    /// This is the path a monster's cast takes in retail, and it differs from
    /// the action band's accumulating seed in the one way that matters: the
    /// roll is clamped against live HP **once** and that single value reaches
    /// both the readout accumulator `+0x10` and live HP `+0x14C`, so the bar
    /// can never be asked to travel further than HP moved. The action band's
    /// seed can, which is the `0x51` settle park
    /// `legaia_engine_vm::battle_hp_bar` documents.
    ///
    /// Also carried: the reaction-clip pick (`+0x1F2` gates `+0x1F1` against
    /// `+0x1EF` / `+0x1F0`, and a dead victim always takes `+0x1F1`), the
    /// `+0x1DC` **bit** ORs - retail ORs here, it does not bump - and the
    /// readout cursor `ctx[+0x262]`.
    ///
    /// Returns the damage actually applied.
    pub(in crate::world) fn apply_effect_child_hit(&mut self, slot: usize, damage: i32) -> i32 {
        use vm::battle_cast_census::{EffectChildVictim, effect_child_hit};
        let mut cursor = self.battle_ctx.cast_readout_cursor;
        let Some(a) = self.actors.get_mut(slot) else {
            return 0;
        };
        a.battle.arm_hp_bar();
        let p = |i: usize| a.battle.params.get(i - 0x1DF).copied().unwrap_or(0);
        let mut victim = EffectChildVictim {
            hp: a.battle.hp,
            hp_bar_delta: a.battle.hp_bar_pending,
            flags: a.battle.field_flags,
            staged_anim: a.battle.queued_anim,
            restage: 0,
            reaction_alt: p(0x1EF),
            reaction_alt2: p(0x1F0),
            knockdown_anim: p(0x1F1),
            reaction_gate: p(0x1F2),
        };
        let hit = effect_child_hit(&mut victim, damage, &mut cursor);
        a.battle.hp = victim.hp;
        a.battle.hp_bar_pending = victim.hp_bar_delta;
        a.battle.queued_anim = victim.staged_anim;
        // `hp == 0 -> liveness = 0` holds for **present** actors only, the
        // same `max_hp > 0` guard `apply_battle_hp_delta` applies.
        if a.battle.max_hp > 0 && a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
        self.battle_ctx.cast_readout_cursor = cursor;
        hit.applied
    }
}

impl World {
    /// PROT 0925's outcome arm (arm 8, `FUN_801F6A00`). With the no-escape
    /// byte `ctx[+0x287]` clear (`0x801F7764`) the arm stages the party's
    /// flee - the same staging, camera cut and HP floor the run band's
    /// granted escape uses (`World::stage_party_flee`; the HP floor is
    /// `0x801F7964..0x801F7984`) - and hands the action SM state `0x65` with
    /// `ctx[+0x6D8] = 0x3C` and the escape outcome, which the step applies
    /// after its write-back. On every fight it then runs the round tail.
    ///
    /// PORT: FUN_801F6A00 (arm 8: flee staging + round tail; the record-side
    /// HP / MP write-back is the battle teardown's)
    fn spikefish_outcome(&mut self, ctx: &mut vm::cast_module_ticks::CastModuleCtx) {
        let party_n = usize::from(
            self.party
                .party_count
                .min(vm::cast_module_ticks::FIRST_MONSTER_SEAT),
        )
        .min(self.actors.len());
        if !self.battle.no_escape {
            self.stage_party_flee();
            for a in self.actors.iter_mut().take(party_n) {
                if a.battle.hp == 0 {
                    a.battle.hp = 1;
                    a.battle.liveness = 1;
                }
            }
            self.casting.module_flee = true;
        }
        let mut party: Vec<_> = (0..party_n as u8)
            .map(|s| self.cast_actor_state(s))
            .collect();
        let refunds = vm::cast_module_ticks::spikefish_round_tail(ctx, &mut party);
        for (a, view) in self.actors.iter_mut().zip(&party) {
            a.battle.init_key = view.init_key;
        }
        for item in refunds {
            let _ = self.party.inventory.add(item, 1);
        }
    }
}

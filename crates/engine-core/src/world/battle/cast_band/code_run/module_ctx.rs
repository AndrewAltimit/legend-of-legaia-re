//! The cast-module context a tick runs against: module camera store and seats,
//! the caster latch, context assembly and the chain-settled probes.

use super::*;

impl World {
    /// The module's camera state with its `yaw_base` loaded from the battle
    /// camera's live `ctx[+0x6DA]`. Retail has one word: the module's
    /// `sh 0x200, 0x6DA(ctx)` and per-pass swing land in the counter the
    /// action SM's prologue keeps drifting, and the Done band's case 6 reads
    /// what they left (`shiny_refactor_gimard_levelup`).
    pub(in crate::world::battle::cast_band) fn module_cam_with_yaw_base(
        &self,
    ) -> legaia_engine_cast::cast_module_camera::ModuleCamState {
        let mut st = self.casting.module_cam;
        if let Some(cam) = self.battle.camera.as_ref() {
            st.yaw_base = cam.action_yaw_base();
        }
        st
    }

    /// Store the module's camera state back and its `yaw_base` into the
    /// battle camera's `ctx[+0x6DA]` ([`Self::module_cam_with_yaw_base`]).
    pub(in crate::world::battle::cast_band) fn store_module_cam(
        &mut self,
        st: legaia_engine_cast::cast_module_camera::ModuleCamState,
    ) {
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
    ) -> legaia_engine_cast::cast_module_camera::ModuleCamSeats {
        let seat = |slot: u8| {
            self.actors
                .get(slot as usize)
                .map(|a| legaia_engine_cast::cast_module_camera::ModuleSeat {
                    x: a.move_state.world_x,
                    y: a.move_state.world_y,
                    z: a.move_state.world_z,
                    facing: a.battle.facing_angle & 0xFFF,
                })
                .unwrap_or_default()
        };
        legaia_engine_cast::cast_module_camera::ModuleCamSeats {
            caster: seat(caster_slot),
            victim: seat(victim_slot),
            // The SM's post-drain word while it is calling the stager from
            // `0x34` / `0x35` (`World::summon_band_word_in`); the
            // context's copy is the pre-step value there.
            band_timer: i32::from(
                self.casting
                    .band_word_live
                    .unwrap_or(self.battle_ctx.frame_timer),
            ),
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
            victim_radius: self.battle_body_radius(victim_slot),
        }
    }

    /// The monster caster's battle-scoped latch word `0x801C8FE0 +
    /// (ctx[+0x13] + 1) * 4` - the monster AI's ability cooldown
    /// `dat[m + 4]`. `None` for a party caster.
    pub(in crate::world::battle::cast_band) fn caster_latch_index(
        &self,
        caster_slot: u8,
    ) -> Option<usize> {
        let m = usize::from(caster_slot).checked_sub(self.party.party_count as usize)?;
        let i = m + 4;
        (i < self.battle.monster_ai_state.dat.len()).then_some(i)
    }

    pub(in crate::world::battle::cast_band) fn caster_latch(&self, caster_slot: u8) -> Option<i32> {
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
    pub(in crate::world) fn cast_module_ctx(
        &self,
    ) -> legaia_engine_cast::cast_module_ticks::CastModuleCtx {
        use legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT;
        let table = self.actors.len().min(BATTLE_TABLE_SLOTS);
        let map = self.cast_seat_map();
        legaia_engine_cast::cast_module_ticks::CastModuleCtx {
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
            // Retail's `ctx[+1]` is the fight's **seated monster** count: the
            // battle loader seats monster `k` at pool slot `3 + k`
            // (`addiu s0,s2,0x3` at `0x8005185C`) and every reader bounds its
            // sweep with it from seat `3` (`lbu a1,1(ctx)` at `0x8004B10C`),
            // a downed monster included (the escape divisor keeps it,
            // `0x801EC118`). Counted over retail's seats `3..8` (the
            // kernels' row, `seat_map`), each read through the engine slot
            // behind it - a small party's monsters sit below engine slot `3`
            // - and counted by the monster seated there, not by the slot's
            // `active` flag: an empty slot left active (a stale row, a
            // host-side extra) is no monster, and a row poll over it never
            // ends (Theeder's and Gizam's arm 14).
            monster_count: (FIRST_MONSTER_SEAT..BATTLE_TABLE_SLOTS as u8)
                .filter_map(|r| map.engine(r))
                .filter(|&e| {
                    self.actors
                        .get(usize::from(e))
                        .is_some_and(|a| a.battle_monster_id.is_some())
                })
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
    /// [`legaia_engine_cast::cast_module_ticks`].
    ///
    /// Retail re-enters the paged module every frame through
    /// `FUN_801F1ED4` / `FUN_801F2160` and holds battle phase `0x70` while
    /// the tick reports busy; the engine calls this from
    /// [`Self::summon_stager_tick`], which the action SM already drives at
    /// states `0x34` / `0x35` / `0x36` (`crate::world::vm_hosts` ->
    /// `legaia_engine_battle_vm::battle_action::summon`). So the chain from a host
    /// root is: `play-window` / the browser play page -> `World::tick` ->
    /// the action SM's summon band -> here.
    ///
    /// What runs is the module's **presentation and phase** half: the
    /// `ctx+0x279` walk, `ctx+0x278`, the staged-clip and animation-rate
    /// writes, the summon-seat pose and the `+0x1DD` retarget. The damage
    /// half is deliberately *not* re-applied here - the engine folds a cast's
    /// HP outcome once, at [`Self::cast_spell_on_slots_prepaid`], and the
    /// module's own numbers reach that fold as the baked per-hit power
    /// ([`legaia_engine_cast::cast_module_ticks::baked_power_for`], read by
    /// `capture_bypass_predamage` / `capture_respect_predamage`) rather than
    /// as a second application.
    ///
    /// Returns `None` when no band entry is resident (a disc-free host, or a
    /// spell that names no module).
    /// Whether one seat has settled the way every settle loop in the band
    /// tests it ([`legaia_engine_cast::cast_module_ticks::ChainSettle`]): a live seat once its playing clip
    /// is back to idle, a dead one once it plays the down clip - or, where
    /// `faded` counts, once its defeat fade has run its colour word out
    /// (retail's prim word `+0x04` at `0`).
    ///
    /// The engine's playing clip is the reaction channel's entry while a
    /// reaction plays (retail commits a reaction into `+0x1D9` like any
    /// other clip), else the committed `current_anim`. A downed party seat
    /// holds the engine's defeat pose rather than clip `8`, so a finished
    /// defeat pose reads as settled too.
    pub(in crate::world::battle::cast_band) fn chain_seat_settled(
        &self,
        slot: usize,
        faded: bool,
    ) -> bool {
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
        let downed = playing == legaia_engine_cast::cast_module_ticks::SETTLE_DOWN_CLIP
            || (a.battle_pose == Some(legaia_engine_battle_vm::battle_action::Pose::Defeat as u8)
                && a.battle_animation.as_ref().is_none_or(|p| p.finished()));
        downed || (faded && (a.battle.render_color == 0 || !a.active))
    }

    /// [`Self::chain_seat_settled`] over the seats a [`legaia_engine_cast::cast_module_ticks::ChainSettle`]
    /// walks.
    pub(in crate::world::battle::cast_band) fn chain_settled(
        &self,
        settle: legaia_engine_cast::cast_module_ticks::ChainSettle,
        caster: u8,
        victim: u8,
        ctx: &legaia_engine_cast::cast_module_ticks::CastModuleCtx,
    ) -> bool {
        use legaia_engine_cast::cast_module_ticks::ChainSettle as S;
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
                if t == legaia_engine_battle_vm::battle_cue_group::TARGET_PARTY_WIDE
                    || usize::from(t) < pc
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
}

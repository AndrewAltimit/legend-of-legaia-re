//! The summon stager: arming, the per-frame tick, the summon walk and the
//! despawn.

use super::*;

impl World {
    /// Arm the stager for `caster`'s `spell_id` cast. Called from the cast
    /// trigger (`FUN_801DBF9C`'s `>= 0x25` arm); the band's first stager
    /// tick does the spawn.
    ///
    /// Public because it is the band's arming *seam*, not an internal step:
    /// the trigger runs it for a host's cast, and a parity oracle that drives
    /// the band a frame at a time has to reach the same door rather than a
    /// second one of its own.
    pub fn arm_summon_stager(&mut self, caster: u8, spell_id: u8) {
        let (cx, cy, cz) = self
            .actors
            .get(caster as usize)
            .map(|a| {
                (
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                )
            })
            .unwrap_or((0, 0, -542));
        self.casting.summon_seat_owed = None;
        // The cast's side-band stream: its `summon.dat` texture slots land in
        // the battle VRAM before the module's parts sample them.
        if let Some(summon_dat) = self.tables.summon_dat.clone() {
            crate::battle_sideband_textures::record_cast_sideband_textures(
                &summon_dat,
                spell_id,
                &mut self.battle.vram_loads,
            );
        }
        // Retail's cast-start site `0x801E4B1C` zeroes `ctx+0x278` and the
        // module phase `ctx+0x279` before the first tick.
        self.casting.module_phase = 0;
        self.casting.module_ctx_278 = 0;
        self.casting.module_handover_waits = false;
        // A new cast re-arms the Nighto verdict; the roll happens on the first
        // tick of the module, the way retail's arm 0 draws it. The ring sweep
        // starts from zero for the same reason.
        self.casting.module_nighto_outcome = None;
        self.casting.module_stager_calls.clear();
        self.casting.module_barra_counter = 0;
        self.casting.module_running_entry = None;
        self.casting.module_spikefish_words = Default::default();
        self.casting.module_ozma_word = 0;
        self.casting.module_stager_seats.clear();
        self.casting.module_record_clocks.clear();
        self.casting.module_ring_angle = 0;
        self.casting.module_theeder = Default::default();
        self.casting.module_swordie = Default::default();
        self.casting.module_rain = Default::default();
        self.casting.module_orb = Default::default();
        self.casting.module_settle_countdown = 0;
        self.casting.module_cam = Default::default();
        self.casting.module_pres = Default::default();
        self.casting.module_saved_poses.clear();
        self.casting.module_creature_clip_kind = 0;
        self.casting.module_shake = 0;
        self.casting.summon_stager = Some(SummonStager {
            caster,
            spell_id,
            phase: SummonPhase::Armed,
            frames: 0,
            spawn: [cx, cy, cz.saturating_sub(SUMMON_SPAWN_BEHIND)],
            goal: [cx, cy, cz.saturating_sub(SUMMON_STRIKE_BEHIND)],
            walked: false,
        });
        self.emit_cast_module_voice(spell_id);
    }

    /// One stager tick - the engine body behind
    /// `BattleActionHost::summon_stager_tick`. Returns `true` while the
    /// choreography is still running (retail: the stager's non-zero return
    /// the `0x36` arm holds on).
    ///
    /// PORT: FUN_801F1ED4 (the dispatch seam; the choreography is the
    /// engine's - see the module docs)
    pub fn summon_stager_tick(&mut self) -> bool {
        let Some(mut st) = self.casting.summon_stager.take() else {
            return false;
        };
        // Retail re-enters the paged module every frame from this seam; the
        // band's PORT rows are the code that runs there.
        let module_arm = self.casting.module_phase;
        self.orb_land_deferred_seat();
        // The creature's live seat and walk state, as a directed module's
        // walk arm reads them.
        let seat_live = self
            .casting
            .summon_actor_slot
            .filter(|&s| self.actors.get(s as usize).is_some_and(|a| a.active))
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| legaia_engine_cast::cast_module_camera::ModuleSeat {
                x: a.move_state.world_x,
                y: a.move_state.world_y,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle & 0xFFF,
            });
        self.casting.module_cam.creature_live = seat_live;
        self.casting.module_cam.victim_untargetable =
            self.casting.module_cam.victim_slot.is_some_and(|v| {
                self.cast_view(v).flags & legaia_engine_cast::cast_module_ticks::FLAG_NON_TARGETABLE
                    != 0
            });
        self.casting.module_cam.creature_arrived = st.walked;
        self.feed_module_creature(st.caster);
        let placed_before = self.casting.module_cam.creature.is_some();
        let run = self.run_cast_module_code(st.spell_id, module_arm);
        // PROT 0920's rain routine, which the draw tick calls after the
        // module's pass while the module's budget word is set.
        self.step_module_rain();
        if let Some(r) = run.as_ref() {
            self.apply_module_creature_writes(r.prot_entry, r.phase, r.capture_drift);
            self.apply_module_seat_motion(r.prot_entry, r.phase);
        }
        // The module seats its creature itself (Gimard's arm 3); put the
        // engine's creature where it did.
        if !placed_before
            && let Some(c) = self.casting.module_cam.creature
            && let Some(slot) = self.casting.summon_actor_slot
            && let Some(a) = self.actors.get_mut(slot as usize)
        {
            a.move_state.world_x = c.x;
            a.move_state.world_z = c.z;
            a.battle.facing_angle = c.facing;
        }
        // A module whose arms are paced by their own countdown
        // (`legaia_engine_cast::cast_module_camera`) owns the band's length the way retail's
        // does: `0x36` holds on the module's return, and the outcome lands on
        // the module's hit arm rather than on the engine's walk-in.
        let profile = run
            .as_ref()
            .and_then(|r| legaia_engine_cast::cast_module_camera::module_profile(r.prot_entry))
            .filter(|p| p.paces_band());
        // A module whose director reports its spawn calls seats each record
        // on the pass its arm makes the call, after the arm's own creature
        // placement above (Gimard's arm 3 spawns on the seat it just set).
        let stages_spawns = run
            .as_ref()
            .and_then(|r| legaia_engine_cast::cast_module_camera::module_profile(r.prot_entry))
            .is_some_and(|p| p.stages_spawns);
        if let Some(r) = run.as_ref().filter(|_| stages_spawns) {
            if let Some(m) = r.vram_move {
                let i = |v: u16| v as i16;
                self.battle.vram_moves.push(crate::world::ScriptVramMove {
                    src: (i(m.src.0), i(m.src.1)),
                    size: (i(m.size.0), i(m.size.1)),
                    dst: (i(m.dst.0), i(m.dst.1)),
                });
            }
            if !r.spawns.is_empty() {
                self.stage_module_arm_spawns(r.prot_entry, r.spawns, r.camera_shot, r.arm_point);
            }
            if !r.every_spawns.is_empty() {
                self.stage_module_arm_spawns(
                    r.prot_entry,
                    r.every_spawns,
                    r.camera_shot,
                    r.arm_point,
                );
            }
            // The arm's own fades run beside the band's flash: retail's
            // spawner takes a fresh pool actor per call.
            if r.kills_fades {
                self.presentation.module_fades.clear();
            }
            for (t, id) in r.fades {
                let mut seat = None;
                legaia_engine_system::fade::spawn_fade(
                    &mut seat,
                    &legaia_engine_battle_vm::battle_action::summon_fade_template(t),
                    *id,
                );
                self.presentation.module_fades.extend(seat);
            }
        }
        // A caption is the arm's own text actor, whichever way the module's
        // spawns are seated.
        if let Some(c) = run.as_ref().and_then(|r| r.caption) {
            self.casting.module_caption = Some(c);
        }
        let directed_hit = profile.and_then(|p| p.hit_arm);
        let module_busy =
            profile.is_some() && run.as_ref().is_some_and(|r| r.tick_ported && r.busy);
        let module_phase = run.as_ref().map_or(0, |r| r.phase);
        if let Some(shot) = run.as_ref().and_then(|r| r.camera_shot)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            let (pose, raw_z) = shot.pose();
            cam.arm_module_shot(pose, raw_z, u32::from(shot.frames));
        }
        if let Some(n) = run.as_ref().and_then(|r| r.camera_nudge)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            cam.nudge_module(n.pitch, n.tr_y, n.tr_z);
            if n.tr_x != 0 {
                cam.nudge_module_tr_x(n.tr_x);
            }
        }
        if let Some(f) = run.as_ref().and_then(|r| r.camera_follow)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            let actor = legaia_engine_battle_vm::battle_cam_script::BattleCamActor {
                facing: i32::from(f.seat.facing),
                world: [
                    f32::from(f.seat.x),
                    f32::from(f.seat.y),
                    f32::from(f.seat.z),
                ],
                height: None,
            };
            cam.arm_module_follow(actor, f.yaw_base, f.depth_raw);
        }
        if run.as_ref().is_some_and(|r| r.camera_end_frame)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            cam.arm_module_end_frame();
        }
        // The module's seat arm (`FUN_801F19EC`): a seat owed since the
        // stager armed is requested as the module reaches it.
        let seat_arm = run
            .as_ref()
            .and_then(|r| legaia_engine_cast::cast_module_camera::module_profile(r.prot_entry))
            .and_then(|p| p.seat_arm);
        let seat_entry = run.as_ref().map(|r| r.prot_entry);
        if let Some((arm, spell, at)) = self.casting.summon_seat_owed
            && (super::module_seat_arm::seat_due(seat_entry, arm, module_phase) || run.is_none())
        {
            self.casting.summon_seat_owed = None;
            self.casting.pending_summon_spawn = Some((spell, at));
        }
        let busy = match st.phase {
            SummonPhase::Armed => {
                // Phase 0: seat the creature (retail: the stager's
                // `FUN_801F19EC` installs the streamed record as slot 7) -
                // here, unless the module seats it in a later arm.
                match seat_arm
                    .filter(|&a| !super::module_seat_arm::seat_due(seat_entry, a, module_phase))
                {
                    Some(arm) => {
                        self.casting.summon_seat_owed = Some((arm, st.spell_id, st.spawn));
                    }
                    None => {
                        self.casting.pending_summon_spawn = Some((st.spell_id, st.spawn));
                    }
                }
                // ...and stage the module's own effect parts. This is the
                // `0x801E4B1C` site's other half: `FUN_801F1ED4` dispatches
                // into the paged module, whose spawn records are the cast's
                // particle layer. Seated where the creature is, since the
                // records carry summon-local offsets.
                if !stages_spawns {
                    self.spawn_cast_module_fx(st.spell_id, st.spawn);
                } else if self.cast_module_for(st.spell_id).is_some_and(|e| {
                    legaia_engine_cast::cast_module_camera::CREATURE_SPAWN_ENTRIES.contains(&e)
                }) {
                    self.spawn_cast_module_stager_records(st.spell_id, st.spawn);
                }
                st.phase = SummonPhase::Approach;
                st.frames = 0;
                true
            }
            SummonPhase::Approach => {
                st.frames = st.frames.saturating_add(1);
                let seat = self
                    .casting
                    .summon_actor_slot
                    .filter(|&s| self.actors.get(s as usize).is_some_and(|a| a.active));
                // A directed module walks its creature in its own walk arm,
                // not after the engine's idle count.
                let may_walk = match profile {
                    Some(p) => p.walk_arm.is_some_and(|w| module_phase >= w),
                    None => st.frames > SUMMON_IDLE_FRAMES,
                };
                // A directed walk arm walks the creature onto the **victim**
                // and holds on the range poll `FUN_8004E2F0(7, victim)`
                // (PROT 0903's arm 11, `bne v0,zero` at `0x801F7418`); the
                // engine's fixed goal stands in only where no module walks.
                let victim = profile
                    .filter(|p| p.walk_arm.is_some())
                    .map(|_| self.actors.get(st.caster as usize))
                    .and_then(|c| c.map(|c| c.battle.active_target))
                    .filter(|&v| self.actors.get(usize::from(v)).is_some_and(|a| a.active));
                let walked = match (seat, victim) {
                    (Some(slot), Some(v)) => may_walk && self.summon_walk_to_victim(slot, v),
                    (Some(slot), None) => may_walk && self.summon_walk_step(slot as usize, st.goal),
                    (None, _) => {
                        self.casting.summon_seat_owed.is_none()
                            && st.frames >= SUMMON_UNSEATED_GRACE
                    }
                };
                st.walked = walked;
                let strike = match directed_hit {
                    Some(hit) => module_phase > hit || !module_busy,
                    None => walked,
                };
                if strike {
                    self.fold_pending_cast();
                    st.phase = SummonPhase::Linger;
                    st.frames = 0;
                }
                true
            }
            SummonPhase::Linger => {
                st.frames = st.frames.saturating_add(1);
                if st.frames == 1
                    && let Some(slot) = self.casting.summon_actor_slot
                    && let Some(a) = self.actors.get_mut(slot as usize)
                {
                    // Back to the idle loop for the hold.
                    a.battle.queued_anim = 0;
                }
                // A module still busy long past its outcome is held on a poll
                // the engine cannot answer (`SUMMON_LINGER_STALL_FRAMES`).
                let stalled = module_busy && st.frames >= SUMMON_LINGER_STALL_FRAMES;
                if (st.frames >= SUMMON_LINGER_FRAMES && !module_busy) || stalled {
                    self.despawn_summon_actor();
                    st.phase = SummonPhase::Done;
                }
                true
            }
            SummonPhase::Done => false,
        };
        if busy {
            self.casting.summon_stager = Some(st);
        }
        busy
    }

    /// Stage the walk clip (id `1`, the looping approach - the capture's
    /// `+0x1D9 == 1`) and glide the creature one step toward `goal`.
    /// Returns `true` on arrival.
    fn summon_walk_step(&mut self, slot: usize, goal: [i16; 3]) -> bool {
        let Some(a) = self.actors.get_mut(slot) else {
            return true;
        };
        let ms = &a.move_state;
        if (ms.world_x, ms.world_z) == (goal[0], goal[2]) {
            // Already there (a module seated it on its goal): no walk clip.
            return true;
        }
        if a.battle.queued_anim != 1 {
            a.battle.queued_anim = 1;
        }
        let ms = &mut a.move_state;
        let dx = i32::from(goal[0]) - i32::from(ms.world_x);
        let dz = i32::from(goal[2]) - i32::from(ms.world_z);
        let step = i32::from(SUMMON_WALK_STEP);
        if dx.abs() <= step && dz.abs() <= step {
            ms.world_x = goal[0];
            ms.world_z = goal[2];
            return true;
        }
        ms.world_x = (i32::from(ms.world_x) + dx.clamp(-step, step)) as i16;
        ms.world_z = (i32::from(ms.world_z) + dz.clamp(-step, step)) as i16;
        false
    }

    /// A directed walk arm's step: stage the walk clip, glide the creature
    /// one step toward `victim`'s seat, and report arrival on the range poll
    /// retail's arm holds on - `FUN_8004E2F0(7, victim)` reading `0`. Retail
    /// turns the creature onto the victim every pass (`sh v0,0x46(s3)` at
    /// `0x801F73C4`); the creature's own `+0x1F` reads `0` in the
    /// `gimard_burning_attack` capture, so the size class is the victim's.
    fn summon_walk_to_victim(&mut self, slot: u8, victim: u8) -> bool {
        let Some(a) = self.actors.get(usize::from(slot)) else {
            return true;
        };
        let pos = (a.move_state.world_x, a.move_state.world_z);
        if self.creature_range_metric(pos, 0, victim) == 0 {
            return true;
        }
        let (vx, vz) = self.battle_seat_of(usize::from(victim));
        // `FUN_80019B28(victim, creature) + 0x800` (`0x801F73A4..0x801F73C4`).
        let bearing =
            legaia_engine_battle_vm::battle_action::bearing_12bit_approx(vz, vx, pos.1, pos.0);
        let facing = bearing.wrapping_add(0x800) & 0xFFF;
        // The walk is the creature clip's own root motion: the anim tick's
        // positive-speed term steps it along this facing while the range
        // poll against its target `+0x1DD` (the victim -
        // `gimard_burning_attack` reads `3`) still fails
        // (`World::drive_playing_root_motion`). Stepping it here as well
        // walked it twice - 58 units a tick against retail's ~30 (the
        // capture's creature `+0x21D = 4` halves the clip's speed). The
        // measured constant stays for a creature with no root speed to
        // play (a headless seat with no clip).
        let root_driven = self
            .battle_playing_root_motion(usize::from(slot))
            .is_some_and(|(speed, _)| speed > 0);
        if let Some(a) = self.actors.get_mut(usize::from(slot)) {
            a.battle.facing_angle = facing;
            a.battle.active_target = victim;
            if a.battle.queued_anim != 1 {
                a.battle.queued_anim = 1;
            }
            if root_driven {
                return false;
            }
            let ms = &mut a.move_state;
            let (dx, dz) = (
                i32::from(vx) - i32::from(ms.world_x),
                i32::from(vz) - i32::from(ms.world_z),
            );
            if dx.abs() + dz.abs() <= SUMMON_DIRECTED_WALK_STEP {
                ms.world_x = vx;
                ms.world_z = vz;
            } else {
                let (sin, cos) = legaia_engine_system::trig::trig12(facing);
                ms.world_x = (i32::from(ms.world_x)
                    + ((i32::from(sin) * SUMMON_DIRECTED_WALK_STEP) >> 12))
                    as i16;
                ms.world_z = (i32::from(ms.world_z)
                    + ((i32::from(cos) * SUMMON_DIRECTED_WALK_STEP) >> 12))
                    as i16;
            }
        }
        let Some(a) = self.actors.get(usize::from(slot)) else {
            return true;
        };
        let pos = (a.move_state.world_x, a.move_state.world_z);
        self.creature_range_metric(pos, 0, victim) == 0 || pos == (vx, vz)
    }

    /// Retire the summon creature: the seat goes inactive so both hosts
    /// stop drawing it (native: the `active` gate of the battle draw loop;
    /// browser: the transform row's `active` float).
    pub(in crate::world) fn despawn_summon_actor(&mut self) {
        if let Some(slot) = self.casting.summon_actor_slot.take()
            && let Some(a) = self.actors.get_mut(slot as usize)
        {
            a.active = false;
            a.battle_animation = None;
            a.pose_frame = None;
            a.battle.queued_anim = 0;
            a.battle.current_anim = 0;
        }
    }
}

//! The capture stager and the caster stage runs: tick, arming the capture cast
//! module, and stepping and closing the caster stages.

use super::*;

impl World {
    /// One tick of the **capture band's** resident module - the engine body
    /// behind `BattleActionHost::capture_stager_tick`, and the twin of
    /// [`Self::summon_stager_tick`] on the other dispatcher.
    ///
    /// Retail's battle phase `0x70` re-enters `FUN_801F2160` every frame
    /// (`jal` at `0x801E50C8`) and holds while it returns non-zero
    /// (`bne v0,zero` at `0x801E50D0`); the return is the selected slot-B
    /// module tick's own return. So this runs the resident module's code half
    /// ([`Self::run_cast_module_code`]) and reports what it reported.
    ///
    /// The one deviation, and it is deliberate: a module whose tick body is
    /// **not** ported reports "not busy" rather than
    /// [`CastModuleCodeRun::busy`]'s seeded `true`. Holding on an unported
    /// row would park the band forever, which is a softlock and not a
    /// fidelity gain; [`CastModuleCodeRun::tick_ported`] is what distinguishes
    /// the two.
    ///
    /// PORT: FUN_801F2160 (the dispatch seam; the per-module tick bodies are
    /// [`legaia_engine_battle_vm::cast_module_ticks`])
    pub fn capture_stager_tick(&mut self) -> bool {
        let Some(spell_id) = self.casting.capture_spell else {
            return false;
        };
        // The caster's own stages the body's port does not write: the
        // wind-up plays first, the module's arms run with the caster on its
        // park, and the close releases the park once they are done.
        match self.step_caster_stages() {
            CasterStageGate::Hold => return true,
            CasterStageGate::Done => {
                self.casting.capture_spell = None;
                return false;
            }
            CasterStageGate::RunModule => {}
        }
        let arm = self.casting.module_phase;
        let Some(run) = self.run_cast_module_code(spell_id, arm) else {
            if self.close_caster_stages() {
                return true;
            }
            self.casting.capture_spell = None;
            return false;
        };
        // `0x70` frames nothing of its own: the camera is the module's.
        if let Some(cam) = self.battle.camera.as_mut() {
            if let Some(shot) = run.camera_shot {
                let (pose, raw_z) = shot.pose();
                cam.arm_module_shot(pose, raw_z, u32::from(shot.frames));
            }
            if let Some(d) = run.capture_drift {
                cam.drift_module(d.pitch, d.yaw, d.tr_y, d.tr_z);
                if d.tr_x != 0 {
                    cam.drift_module_tr_x(d.tr_x);
                }
            }
        }
        if run.tick_ported && run.busy {
            return true;
        }
        if self.close_caster_stages() {
            return true;
        }
        // The band is leaving `0x70`; the module stops being re-entered.
        self.casting.capture_spell = None;
        self.casting.fatal_banner = None;
        false
    }

    /// Arm [`Self::capture_stager_tick`] for `spell_id`'s module - the pager
    /// seam retail runs at its `0x6F` exit, where `sb zero,0x279(v0)`
    /// (`0x801E5048`) also zeroes the module phase before phase `0x70` starts
    /// ticking it.
    pub(in crate::world) fn arm_capture_cast_module(&mut self, spell_id: u8) {
        if self.cast_module_for(spell_id).is_none() {
            return;
        }
        self.casting.module_phase = 0;
        self.casting.module_ctx_278 = 0;
        self.casting.module_skips_fold = false;
        self.casting.fatal_decision = None;
        self.casting.fatal_banner = None;
        self.casting.module_swordie = Default::default();
        self.casting.module_rain = Default::default();
        self.casting.module_settle_countdown = 0;
        self.casting.module_cam = Default::default();
        self.casting.module_pres = Default::default();
        self.casting.module_beam_counter = 0;
        self.casting.module_beam_live = false;
        self.casting.module_hit_arm = None;
        self.casting.module_settle_ticks = 0;
        self.casting.capture_spell = Some(spell_id);
        self.casting.caster_stages = self.caster_stage_run_for(spell_id);
        self.emit_cast_module_voice(spell_id);
    }

    /// The [`CasterStageRun`] a capture-class cast of `spell_id` by the
    /// acting seat owes, when its body's port stages nothing on the caster
    /// ([`vm::cast_module_ticks::capture_caster_stages`]). Only a monster seat
    /// with installed clips plays one, and a stage naming an entry the
    /// caster's record does not carry is dropped - retail would index past
    /// the record's offset array there (the Gobu Gobu Curse fault on
    /// `docs/subsystems/cast-module.md`).
    fn caster_stage_run_for(&self, spell_id: u8) -> Option<CasterStageRun> {
        let entry = self.cast_module_for(spell_id)?;
        let slot = self.battle_ctx.active_actor;
        let actor = self.actors.get(usize::from(slot))?;
        let seat_monster = (actor.battle_monster_id? & 0xFF) as u8;
        let installed = actor.battle_action_clips.as_ref()?;
        let approach =
            legaia_engine_battle_vm::cast_module_ticks::capture_body_approaches(entry, spell_id)
                .then_some(actor.battle.active_target)
                .filter(|&t| t < 8 && usize::from(t) < self.actors.len() && t != slot);
        let clips: Vec<u8> = legaia_engine_battle_vm::cast_module_ticks::capture_caster_stages(
            entry,
            spell_id,
            seat_monster,
            self.battle_first_monster_byte(),
        )
        .unwrap_or_default()
        .into_iter()
        .filter(|&c| {
            installed
                .get(usize::from(c))
                .is_some_and(|c| c.as_ref().is_some_and(|c| c.frame_count > 0))
        })
        .collect();
        (!clips.is_empty() || approach.is_some()).then_some(CasterStageRun {
            slot,
            clips,
            cursor: 0,
            approach,
            phase: if approach.is_some() {
                CasterStagePhase::Approach
            } else {
                CasterStagePhase::Staging
            },
            ticks: 0,
        })
    }

    /// Advance the [`CasterStageRun`] by one band tick.
    fn step_caster_stages(&mut self) -> CasterStageGate {
        let Some(mut run) = self.casting.caster_stages.take() else {
            return CasterStageGate::RunModule;
        };
        let Some(a) = self.actors.get_mut(usize::from(run.slot)) else {
            return CasterStageGate::RunModule;
        };
        if run.phase != CasterStagePhase::Module {
            run.ticks = run.ticks.saturating_add(1);
            if run.ticks > CASTER_STAGE_TICK_LIMIT {
                // A clip that never committed: hand the band back rather
                // than hold it.
                if let Some(p) = a.battle_animation.as_mut() {
                    p.release_loop_window();
                }
                a.battle.queued_anim = 0;
                return if run.phase == CasterStagePhase::Closing {
                    CasterStageGate::Done
                } else {
                    CasterStageGate::RunModule
                };
            }
        }
        let gate = match run.phase {
            CasterStagePhase::Approach => {
                let victim = run.approach.unwrap_or(run.slot);
                if self.battle_range_metric(run.slot, victim) == 0 {
                    // Arrived. A body whose port stages its own strike takes
                    // the band from here; the rest go on to their stages.
                    run.ticks = 0;
                    let a = &mut self.actors[usize::from(run.slot)];
                    if a.battle.queued_anim == CAPTURE_WALK_ENTRY {
                        a.battle.queued_anim = 0;
                    }
                    if run.clips.is_empty() {
                        run.phase = CasterStagePhase::Module;
                        self.casting.caster_stages = Some(run);
                        return CasterStageGate::RunModule;
                    }
                    run.phase = CasterStagePhase::Staging;
                    self.casting.caster_stages = Some(run);
                    return CasterStageGate::Hold;
                }
                // `FUN_80019B28(victim, caster) + 0x800` onto the caster's
                // facing each tick, then the walk stage (`li v1,0x1;
                // sb v1,0x1da(<caster>)`, e.g. `0x801F72C8` in PROT 0952).
                let (vx, vz) = self.battle_seat_of(usize::from(victim));
                let a = &mut self.actors[usize::from(run.slot)];
                let bearing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                    vz,
                    vx,
                    a.move_state.world_z,
                    a.move_state.world_x,
                );
                a.battle.facing_angle = bearing.wrapping_add(0x800) & 0xFFF;
                a.battle.queued_anim = CAPTURE_WALK_ENTRY;
                CasterStageGate::Hold
            }
            CasterStagePhase::Staging => {
                let want = run.clips[run.cursor];
                if run.ticks == 1 {
                    a.battle.queued_anim = want;
                    CasterStageGate::Hold
                } else if a.battle.current_anim == want && caster_clip_settled(a) {
                    if run.cursor + 1 < run.clips.len() {
                        // A wind-up's park is released for the stage that
                        // follows it.
                        if let Some(p) = a.battle_animation.as_mut() {
                            p.release_loop_window();
                        }
                        run.cursor += 1;
                        a.battle.queued_anim = run.clips[run.cursor];
                        CasterStageGate::Hold
                    } else {
                        run.phase = CasterStagePhase::Module;
                        CasterStageGate::RunModule
                    }
                } else {
                    CasterStageGate::Hold
                }
            }
            CasterStagePhase::Module => CasterStageGate::RunModule,
            CasterStagePhase::Closing if run.clips.is_empty() => CasterStageGate::Done,
            CasterStagePhase::Closing => {
                if a.battle.current_anim == 0 {
                    return CasterStageGate::Done;
                }
                CasterStageGate::Hold
            }
        };
        self.casting.caster_stages = Some(run);
        gate
    }

    /// The module reported done: release the caster's park and stage idle
    /// behind it. `true` when a [`CasterStageRun`] is now closing (the band
    /// holds until the caster is back on idle).
    fn close_caster_stages(&mut self) -> bool {
        let Some(run) = self.casting.caster_stages.as_mut() else {
            return false;
        };
        if run.phase != CasterStagePhase::Module || run.clips.is_empty() {
            return false;
        }
        run.phase = CasterStagePhase::Closing;
        if let Some(a) = self.actors.get_mut(usize::from(run.slot)) {
            if let Some(p) = a.battle_animation.as_mut() {
                p.release_loop_window();
            }
            a.battle.queued_anim = 0;
            if a.battle.current_anim == 0 {
                self.casting.caster_stages = None;
                return false;
            }
        }
        true
    }
}

//! The capture stager and the caster stage runs: tick, arming the capture cast
//! module, and stepping and closing the caster stages.

use super::*;

impl World {
    /// The capture-class module a cast of `spell_id` pages, when the body
    /// its trampoline picks has its spawns seated per arm
    /// ([`legaia_engine_cast::cast_module_camera::capture_spawns_per_arm`]).
    pub(in crate::world) fn capture_per_arm_entry(&self, spell_id: u8) -> Option<u32> {
        let e = self.cast_module_for(spell_id)?;
        let body = legaia_engine_cast::cast_module_ticks::capture_tick_body(e, spell_id);
        legaia_engine_cast::cast_module_camera::capture_spawns_per_arm(e, body).then_some(e)
    }

    /// An arm's writes through the handles of parts it seated earlier: the
    /// records it halts, and the one it carries onto the victim's ground point.
    fn apply_capture_arm_part_writes(&mut self, entry: u32, arm: u8) {
        use legaia_engine_cast::cast_module_camera as cmc;
        let halted = cmc::capture_arm_halts_records(entry, arm);
        let moved = cmc::capture_arm_moves_to_victim(entry, arm);
        if halted.is_empty() && moved.is_none() {
            return;
        }
        let victim = self
            .casting
            .module_cam
            .victim_slot
            .and_then(|s| self.actors.get(usize::from(s)))
            .map(|v| (v.move_state.world_x, v.move_state.world_z));
        let Some(scene) = self.casting.active_summon.as_mut() else {
            return;
        };
        for part in &mut scene.parts {
            let Some(tag) = part.tag else { continue };
            if halted.contains(&tag) {
                part.finished = true;
            }
            if moved == Some(tag)
                && let Some((x, z)) = victim
            {
                part.state.world_x = x;
                part.state.world_z = z;
            }
        }
    }

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
    /// [`legaia_engine_cast::cast_module_ticks`])
    pub fn capture_stager_tick(&mut self) -> bool {
        let Some(spell_id) = self.casting.capture_spell else {
            return false;
        };
        // The records the pager noted go in on the band's first tick.
        if let Some(origin) = self.casting.capture_fx_pending.take() {
            self.spawn_cast_module_fx(spell_id, origin);
        }
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
        // A per-arm capture module's stager-only records go in as its body
        // first ticks, inside `0x70` - not at the pager.
        let per_arm = self.capture_per_arm_entry(spell_id);
        // ... unless its stager is ported, in which case its parts' own
        // op-`0x20` calls seat them (`Self::run_part_stager_calls`).
        if arm == 0
            && self.casting.active_summon.is_none()
            && per_arm.is_some_and(|e| !Self::runs_part_stager_calls(e))
        {
            let origin = self
                .actors
                .get(usize::from(self.battle_ctx.active_actor))
                .map_or([0; 3], |a| {
                    [
                        a.move_state.world_x,
                        a.move_state.world_y,
                        a.move_state.world_z,
                    ]
                });
            self.spawn_cast_module_stager_records(spell_id, origin);
        }
        let Some(run) = self.run_cast_module_code(spell_id, arm) else {
            if self.close_caster_stages() {
                return true;
            }
            self.casting.capture_spell = None;
            return false;
        };
        if run.phase != arm || !run.busy {
            self.stage_caster_on_arm_pass(arm);
        }
        // A module whose arms' spawns are ported seats each on the pass
        // its arm makes the call.
        if per_arm == Some(run.prot_entry) {
            use legaia_engine_cast::cast_module_camera as cmc;
            // An arm that seats on every pass (PROT 0962's rays): the roll
            // is `rand() % 0x1000` over the zeroed stack trio's `z`. A pass
            // is one battle frame - the frame driver re-enters the module
            // once per `VSync(step)` - while the module here runs on every
            // display tick, so the call is made on the tick that opens a
            // battle frame. Seated per tick, `cort_evolved_ultra_charge_mid_cast`
            // held 127 live rays against retail's 33.
            let every = cmc::capture_arm_spawns_every(run.prot_entry, arm, spell_id);
            let opens_pass = !self.casting.module_pass_repeat
                && self
                    .battle
                    .frame_clock
                    .ticks_into_frame(self.clock.display_frames)
                    == 0;
            if !every.is_empty() && opens_pass {
                let roll = (self.next_rand() % 0x1000) as i16;
                self.stage_module_arm_spawns(
                    run.prot_entry,
                    every,
                    run.camera_shot,
                    Some(([0; 3], [0, 0, roll])),
                );
            }
            // An arm that spawns on the passes its body's own frame counter
            // picks (PROT 0940's sparks), the counter read before its bump.
            if opens_pass && let Some(c) = cmc::capture_arm_counted(run.prot_entry, arm) {
                let n = self.casting.module_pass_counter;
                if let Some((id, mask)) = c.effect
                    && n & mask == 0
                    && let Some(a) = self.actors.get(usize::from(self.battle_ctx.active_actor))
                {
                    let pos = [
                        a.move_state.world_x,
                        a.move_state.world_y,
                        a.move_state.world_z,
                    ];
                    let facing = a.battle.facing_angle;
                    self.try_spawn_effect(id, pos, facing);
                }
                if n & c.mask == 0 {
                    self.stage_module_arm_spawns(run.prot_entry, c.spawns, run.camera_shot, None);
                }
                self.casting.module_pass_counter = n.wrapping_add(1);
            }
            if let Some(m) = run.vram_move {
                self.battle.vram_moves.push(
                    legaia_engine_field::script_vram_move::ScriptVramMove {
                        src: (m.src.0 as i16, m.src.1 as i16),
                        size: (m.size.0 as i16, m.size.1 as i16),
                        dst: (m.dst.0 as i16, m.dst.1 as i16),
                    },
                );
            }
            // PROT 0935's arms 4 and 5: two draws for the point, a third for
            // the yaw (`0x801F7480..0x801F7690`).
            if let Some(w) = self.casting.module_rock_burst.take()
                && opens_pass
                && let Some(caster) = self.actors.get(usize::from(self.battle_ctx.active_actor))
            {
                let c = (caster.move_state.world_x, caster.move_state.world_z);
                let dx = (self.next_rand() % 1023) as i32;
                let dz = (self.next_rand() % 1023) as i32;
                let yaw = (self.next_rand() % 4095) as i16;
                let (x, z) = cmc::rock_point(c, w, dx, dz);
                let seated: Vec<cmc::ModuleSpawn> = cmc::ROCK_RECORDS
                    .iter()
                    .map(|&r| cmc::ModuleSpawn {
                        record: cmc::SpawnRecord::Module(r),
                        anchor: cmc::SpawnAnchor::LiteralScaled {
                            pos: [x, 0, z],
                            rot: [0, yaw, 0],
                            scale: 0x1000,
                        },
                    })
                    .collect();
                self.stage_module_arm_spawns(run.prot_entry, &seated, run.camera_shot, None);
            }
            if let Some((records, rot)) = self.casting.module_ring.take() {
                let mut used = [false; 8];
                for i in 0..8u16 {
                    // `rand() % 8` until an unused slot comes up; its slot times
                    // `0x40` lands on each record's `+0x54`, `+0x42 = 1`.
                    let mut slot = 0u16;
                    for _ in 0..0x400 {
                        slot = (self.next_rand() % 8) as u16;
                        if !used[usize::from(slot)] {
                            used[usize::from(slot)] = true;
                            break;
                        }
                    }
                    let (s, c) = legaia_engine_system::trig::trig12(i << 9);
                    for record in records {
                        self.casting.seat_stager_spawn(
                            run.prot_entry,
                            &legaia_engine_cast::cast_module_ticks::StagerSpawn {
                                record,
                                pos: [s / 2, 0, c / 2],
                                rot,
                                pokes: vec![(0x54, slot << 6), (0x42, 1)],
                                patches: Vec::new(),
                                halve_speed: false,
                                scale: 0x1000,
                            },
                        );
                    }
                }
            }
            // PROT 0953's arm 6: two trail records at the origin a battle
            // frame, each on three draws stored over it
            // (`0x801F711C..0x801F71F4`).
            if let Some((record, first)) = self.casting.module_trail.take()
                && opens_pass
            {
                for n in first..first.wrapping_add(2) {
                    let spin = (self.next_rand() % 3) as i16 - 1;
                    let rate = (self.next_rand() % 0x400) as u16 + 0x80;
                    let roll = n
                        .wrapping_mul(0x600)
                        .wrapping_add((self.next_rand() % 0x200) as u16);
                    self.casting.seat_stager_spawn(
                        run.prot_entry,
                        &legaia_engine_cast::cast_module_ticks::StagerSpawn {
                            record,
                            pos: [0; 3],
                            rot: [0; 3],
                            pokes: vec![(0x84, spin as u16), (0xB4, rate), (0x28, roll)],
                            patches: Vec::new(),
                            halve_speed: false,
                            scale: 0x1000,
                        },
                    );
                }
            }
            if let Some(records) = self.casting.module_spray.take()
                && opens_pass
            {
                let x = 0x80 - (self.next_rand() % 257) as i16;
                let y = 0x80 - (self.next_rand() % 257) as i16;
                let seated: Vec<cmc::ModuleSpawn> = records
                    .iter()
                    .map(|&r| cmc::ModuleSpawn {
                        record: cmc::SpawnRecord::Module(r),
                        anchor: cmc::SpawnAnchor::LiteralScaled {
                            pos: [x, y, 0x100],
                            rot: [0; 3],
                            scale: 0x1000,
                        },
                    })
                    .collect();
                self.stage_module_arm_spawns(run.prot_entry, &seated, run.camera_shot, None);
            }
            if let Some((record, count)) = self.casting.module_scatter.take()
                && opens_pass
            {
                let mut seated = Vec::with_capacity(usize::from(count));
                for _ in 0..count {
                    let x = (self.next_rand() % 64) as i16 - 32;
                    let y = (self.next_rand() % 64) as i16 - 16;
                    let roll = (self.next_rand() % 4095) as i16;
                    let pitch = (self.next_rand() % 4095) as i16;
                    let yaw = (self.next_rand() % 4095) as i16;
                    seated.push(cmc::ModuleSpawn {
                        record: cmc::SpawnRecord::Module(record),
                        anchor: cmc::SpawnAnchor::LiteralScaled {
                            pos: [x, y, 0x100],
                            rot: [pitch, yaw, roll],
                            scale: 0x1000,
                        },
                    });
                }
                self.stage_module_arm_spawns(run.prot_entry, &seated, run.camera_shot, None);
            }
            // PROT 0957's arms 4 to 6: four pairs a pass, each pair on two
            // draws (`0x801F70AC..0x801F713C`).
            if std::mem::take(&mut self.casting.module_scatter_pairs) && opens_pass {
                let mut seated = Vec::with_capacity(usize::from(cmc::STORM_SCATTER_PAIRS) * 2);
                for _ in 0..cmc::STORM_SCATTER_PAIRS {
                    let x = 0x80 - (self.next_rand() % 255) as i16;
                    let y = 0x80 - (self.next_rand() % 255) as i16;
                    self.casting.module_scatter_y = y;
                    for record in cmc::STORM_SCATTER_RECORDS {
                        seated.push(cmc::ModuleSpawn {
                            record: cmc::SpawnRecord::Module(record),
                            anchor: cmc::SpawnAnchor::LiteralScaled {
                                pos: [x, y, 0x100],
                                rot: [0, 0, 0],
                                scale: 0x1000,
                            },
                        });
                    }
                }
                self.stage_module_arm_spawns(run.prot_entry, &seated, run.camera_shot, None);
            }
            if std::mem::take(&mut self.casting.module_ribbons_on_scatter_y)
                && let Some(ring) = self.battle.cast_ribbons.as_mut()
            {
                for p in ring.points.iter_mut().take(usize::from(ring.count)) {
                    p[1] = self.casting.module_scatter_y;
                }
            }
            if run.phase != arm || !run.busy {
                // PROT 0957's table is Thunder Storm's arms: Death Game's stand
                // under the same numbers and flush nothing.
                let death_game = run.prot_entry == 957 && self.casting.capture_spell == Some(0x76);
                if cmc::capture_arm_flushes_pool_for(run.prot_entry, arm, spell_id) && !death_game {
                    self.casting
                        .flush_module_part_pool_sparing(cmc::capture_arm_flush_spares(
                            run.prot_entry,
                            arm,
                        ));
                }
                self.apply_capture_arm_part_writes(run.prot_entry, arm);
                if cmc::capture_arm_resets_pass_counter(run.prot_entry, arm) {
                    self.casting.module_pass_counter = 0;
                }
                // Water Hazard's ring goes in ahead of the arm's last call.
                if run.prot_entry == 956
                    && arm == 2
                    && spell_id == 0x71
                    && let Some(c) = self.actors.get(usize::from(self.battle_ctx.active_actor))
                {
                    let ring = cmc::water_hazard_ring(
                        (c.move_state.world_x, c.move_state.world_z),
                        (c.battle.facing_angle & 0xFFF) as i16,
                    );
                    self.stage_module_arm_spawns(run.prot_entry, &ring, run.camera_shot, None);
                }
                let spawns = cmc::capture_arm_spawns_in(
                    run.prot_entry,
                    arm,
                    spell_id,
                    self.battle_first_monster_byte(),
                );
                if !spawns.is_empty() {
                    self.stage_module_arm_spawns(run.prot_entry, spawns, run.camera_shot, None);
                }
                // Dark Typhoon's ring follows the arm's two calls.
                if run.prot_entry == 942
                    && arm == 4
                    && spell_id == 0xAA
                    && let Some(c) = self.actors.get(usize::from(self.battle_ctx.active_actor))
                {
                    let ring = cmc::dark_typhoon_ring(
                        (c.move_state.world_x, c.move_state.world_z),
                        (c.battle.facing_angle & 0xFFF) as i16,
                        [0, (c.battle.facing_angle & 0xFFF) as i16, 0],
                    );
                    self.stage_module_arm_spawns(run.prot_entry, &ring, run.camera_shot, None);
                }
                let on_party = cmc::capture_arm_party_records(run.prot_entry, arm, spell_id);
                if !on_party.is_empty() {
                    let spawns: Vec<_> = (0..usize::from(self.party.party_count))
                        .filter_map(|i| self.actors.get(i))
                        .flat_map(|a| {
                            let pos = [
                                a.move_state.world_x,
                                a.move_state.world_y,
                                a.move_state.world_z,
                            ];
                            let rot = [0, (a.battle.facing_angle & 0xFFF) as i16, 0];
                            on_party.iter().map(move |&record| cmc::ModuleSpawn {
                                record: cmc::SpawnRecord::Module(record),
                                anchor: cmc::SpawnAnchor::LiteralScaled {
                                    pos,
                                    rot,
                                    scale: 0x1000,
                                },
                            })
                        })
                        .collect();
                    self.stage_module_arm_spawns(run.prot_entry, &spawns, run.camera_shot, None);
                }
                // Gigaton Press's debris ring follows its five caster records
                // on the same pass (drawn ahead of the arm's hit).
                let debris = std::mem::take(&mut self.casting.module_debris);
                if !debris.is_empty() {
                    self.stage_module_arm_spawns(run.prot_entry, &debris, run.camera_shot, None);
                }
            } else if !self.casting.module_debris.is_empty() {
                // A director's per-pass calls on a held arm (PROT 0947's
                // trail): one battle frame, one call.
                let debris = std::mem::take(&mut self.casting.module_debris);
                if opens_pass {
                    self.stage_module_arm_spawns(run.prot_entry, &debris, run.camera_shot, None);
                }
            }
        }
        // `0x70` frames nothing of its own: the camera is the module's.
        let style = self.battle_ctx.camera_variant;
        if let Some(cam) = self.battle.camera.as_mut() {
            if run.capture_drift_first || run.capture_pose_set.is_some() || run.capture_focus_z != 0
            {
                // A director that stores into the globals, or says so, runs
                // retail's order: the arm's drift, then its pass stores, then
                // the shot its builder tweens from them (PROT 0959, 0960).
                if let Some(d) = run.capture_drift {
                    cam.drift_module(d.pitch, d.yaw, d.tr_y, d.tr_z);
                    if d.tr_x != 0 {
                        cam.drift_module_tr_x(d.tr_x);
                    }
                }
                if run.capture_focus_z != 0 {
                    cam.drift_module_focus_z(run.capture_focus_z);
                }
                if let Some(set) = run.capture_pose_set {
                    cam.set_module_pose(
                        set.yaw.map(f32::from),
                        set.tr_y.map(|v| v as f32),
                        set.tr_z.map(|v| v as f32),
                        set.focus_xz.map(|[x, z]| [f32::from(x), f32::from(z)]),
                    );
                }
                if let Some(shot) = run.camera_shot {
                    let (pose, raw_z) = shot.pose();
                    cam.arm_module_shot(pose, raw_z, u32::from(shot.frames));
                }
            } else {
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
            // The arm's framing-case calls come last in its pass (PROT 0962
            // `0xA2`: case 6 on the walk, case 8 from the strike on).
            if let Some(f) = run.camera_follow {
                let actor = legaia_engine_battle_vm::battle_cam_script::BattleCamActor {
                    facing: i32::from(f.seat.facing),
                    world: [
                        f32::from(f.seat.x),
                        f32::from(f.seat.y),
                        f32::from(f.seat.z),
                    ],
                    height: None,
                };
                cam.arm_module_follow(actor, f.yaw_base, f.depth_raw, f.style.unwrap_or(style));
            }
            if run.camera_end_frame {
                cam.arm_module_end_frame();
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
        self.casting.module_pass_counter = 0;
        self.casting.module_handover_waits = false;
        self.casting.module_cam = Default::default();
        self.casting.module_pres = Default::default();
        self.casting.module_beam_counter = 0;
        self.casting.module_beam_live = false;
        self.casting.module_hit_arm = None;
        self.casting.module_settle_ticks = 0;
        // A module with pre-hits keeps the readout flag down from its first
        // popup, which the HUD reads a frame late: hold from the start, and let
        // the band's fold release it.
        self.battle.combo_cluster_held = self
            .cast_module_for(spell_id)
            .is_some_and(|e| matches!(e, 908 | 915 | 928 | 932 | 933 | 934));
        self.casting.capture_spell = Some(spell_id);
        self.casting.caster_stages = self.caster_stage_run_for(spell_id);
        self.emit_cast_module_voice(spell_id);
    }

    /// The [`CasterStageRun`] a capture-class cast of `spell_id` by the
    /// acting seat owes, when its body's port stages nothing on the caster
    /// ([`legaia_engine_cast::cast_module_ticks::capture_caster_stages`]). Only a monster seat
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
        // A body whose arm 1 is the walk (`capture_module_walks`) walks on
        // its own phase `1`; the band's leg would hold it on phase `0`.
        let approach =
            (legaia_engine_cast::cast_module_ticks::capture_body_approaches(entry, spell_id)
                && !legaia_engine_cast::cast_module_ticks::capture_module_walks(entry, spell_id))
            .then_some(actor.battle.active_target)
            .filter(|&t| t < 8 && usize::from(t) < self.actors.len() && t != slot);
        let staged = legaia_engine_cast::cast_module_ticks::capture_caster_stages(
            entry,
            spell_id,
            seat_monster,
            self.battle_first_monster_byte(),
        )
        .unwrap_or_default();
        let arms =
            legaia_engine_cast::cast_module_ticks::capture_caster_stage_arms(entry, spell_id)
                .filter(|a| a.len() == staged.len());
        let (clips, at_arms): (Vec<u8>, Vec<u8>) = staged
            .into_iter()
            .enumerate()
            .filter(|&(_, c)| {
                installed
                    .get(usize::from(c))
                    .is_some_and(|c| c.as_ref().is_some_and(|c| c.frame_count > 0))
            })
            .map(|(i, c)| (c, arms.map_or(u8::MAX, |a| a[i])))
            .unzip();
        let placed = arms.is_some();
        (!clips.is_empty() || approach.is_some()).then_some(CasterStageRun {
            slot,
            clips,
            at_arms: if placed { at_arms } else { Vec::new() },
            cursor: 0,
            approach,
            phase: if approach.is_some() {
                CasterStagePhase::Approach
            } else if placed {
                CasterStagePhase::Module
            } else {
                CasterStagePhase::Staging
            },
            ticks: 0,
        })
    }

    /// The walk-in's arrival path: thirty-two `FUN_80050BB8(caster, victim)`
    /// calls (`sltiu v0,s0,0x20`, PROT 0951 `0x5B` at `0x801F7A84..0x801F7A9C`),
    /// which step the caster back off the victim's body while the two body
    /// pairs overlap, and the victim's saved pair put back at the arm's tail
    /// (`0x801F7AB8..0x801F7AD8`). Scythe Wind's retail recording steps
    /// `(-26, +77)` on this pass, from `(376, -502)` to `(350, -425)`.
    ///
    /// `false` for a body that walks without this path.
    fn settle_capture_approach(&mut self, caster: u8, victim: u8) -> bool {
        use legaia_engine_battle_vm::battle_separation::{SepActor, push_apart};
        let Some(spell) = self.casting.capture_spell else {
            return false;
        };
        let Some(entry) = self.cast_module_for(spell) else {
            return false;
        };
        let body = legaia_engine_cast::cast_module_ticks::capture_tick_body(entry, spell);
        if !legaia_engine_cast::cast_module_camera::capture_approach_follows(entry, body) {
            return false;
        }
        let sep = |w: &World, slot: u8| {
            let i = usize::from(slot);
            let (x, z) = w.battle_seat_of(i);
            SepActor {
                radius: w.battle_separation_radius(i),
                x,
                z,
                acc_x: w.actors[i].move_state.world_x as u16,
                acc_z: w.actors[i].move_state.world_z as u16,
            }
        };
        let (mut a, mut b) = (sep(self, caster), sep(self, victim));
        let bearing =
            legaia_engine_battle_vm::battle_action::bearing_12bit_approx(b.z, b.x, a.z, a.x);
        let (sin, cos) = legaia_engine_system::trig::trig12(bearing.wrapping_add(0x800) & 0xFFF);
        for _ in 0..0x20 {
            push_apart(&mut a, &mut b, sin, cos);
        }
        let ms = &mut self.actors[usize::from(caster)].move_state;
        ms.world_x = a.acc_x as i16;
        ms.world_z = a.acc_z as i16;
        true
    }

    /// Arm 0's one-time stores on a walk-in the approach leg stands in for,
    /// since the leg holds the module and arm 0 never runs: the victim turned
    /// to face the caster (`0x801F7934..0x801F7940` in PROT 0951 `0x5B`), the
    /// yaw counter ([`capture_approach_yaw_base`]) and the arm's VRAM move.
    ///
    /// The bodies that store the counter draw the framing style beside it,
    /// `ctx[+0xD] = rand() % 4` (`jal 0x80056798` / `sb v0,0xd(a0)` at
    /// `0x801F71D0..0x801F71F8` in PROT 0952 `0x5C`, the same at `0x801F6AC4`,
    /// and in PROT 0951 at `0x801F6AE0` and `0x801F78A0`), ahead of the arm's
    /// case 6. That draw is returned: it frames this pass, where every later
    /// pass reads the `1` state `0x70` pins ahead of the module tick.
    ///
    /// [`capture_approach_yaw_base`]: legaia_engine_cast::cast_module_camera::capture_approach_yaw_base
    fn open_capture_approach(
        &mut self,
        caster: u8,
        victim: u8,
        entry: u32,
        body: Option<u32>,
    ) -> Option<u8> {
        if let Some(reach) =
            legaia_engine_cast::cast_module_camera::capture_approach_reach(entry, body)
        {
            self.store_caster_reach(caster, reach);
        }
        let facing = self.actors[usize::from(caster)].battle.facing_angle;
        if let Some(v) = self.actors.get_mut(usize::from(victim)) {
            v.battle.facing_angle = facing.wrapping_add(0x800) & 0xFFF;
        }
        let mut style = None;
        if let Some(yaw) =
            legaia_engine_cast::cast_module_camera::capture_approach_yaw_base(entry, body)
        {
            let roll = (self.next_rand() % 4) as u8;
            // Capture harness: the roll a retail capture holds lands over the
            // engine's own, the draw itself made (`camera_variant_pin`).
            let draw = self.battle_ctx.camera_variant_pin.unwrap_or(roll);
            self.battle_ctx.camera_variant = draw;
            style = Some(draw);
            if let Some(cam) = self.battle.camera.as_mut() {
                cam.set_action_yaw_base(yaw);
            }
        }
        let Some(director) = body.and_then(|b| {
            legaia_engine_cast::cast_module_camera::capture_camera_director(entry, b)
        }) else {
            return style;
        };
        let seats = self.module_cam_seats(caster, victim);
        let mut scratch = legaia_engine_cast::cast_module_camera::ModuleCamState::default();
        if let Some(m) = director(&mut scratch, 0, seats).vram_move {
            self.battle
                .vram_moves
                .push(legaia_engine_field::script_vram_move::ScriptVramMove {
                    src: (m.src.0 as i16, m.src.1 as i16),
                    size: (m.size.0 as i16, m.size.1 as i16),
                    dst: (m.dst.0 as i16, m.dst.1 as i16),
                });
        }
        style
    }

    /// `FUN_80019B28(victim, caster) + 0x800` onto the caster's facing: the
    /// turn a walk-in arm makes every pass.
    fn turn_caster_onto(&mut self, caster: u8, victim: u8) {
        let (vx, vz) = self.battle_seat_of(usize::from(victim));
        let a = &mut self.actors[usize::from(caster)];
        let bearing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
            vz,
            vx,
            a.move_state.world_z,
            a.move_state.world_x,
        );
        a.battle.facing_angle = bearing.wrapping_add(0x800) & 0xFFF;
    }

    /// A walk-in arm's pass ahead of its range poll, for a body whose arms
    /// frame the caster through case 6 (`capture_approach_follows`): the
    /// caster turned onto the victim and, on arm 0, the arm's own stores
    /// ([`Self::open_capture_approach`]) and phase `1`. Returns the style the
    /// pass's case 6 frames on, `None` for any other body.
    fn open_capture_walk_pass(&mut self, caster: u8, victim: u8) -> Option<u8> {
        let spell = self.casting.capture_spell?;
        let e = self.cast_module_for(spell)?;
        let body = legaia_engine_cast::cast_module_ticks::capture_tick_body(e, spell);
        if !legaia_engine_cast::cast_module_camera::capture_approach_follows(e, body) {
            return None;
        }
        self.turn_caster_onto(caster, victim);
        let drawn = if self.casting.module_phase == 0 {
            self.open_capture_approach(caster, victim, e, body)
        } else {
            None
        };
        // Arm 0 stores phase `1` on its first pass and arm 1 polls the walk
        // from there, so the walk runs on phase `1`.
        self.casting.module_phase = 1;
        Some(drawn.unwrap_or(self.battle_ctx.camera_variant))
    }

    /// The walk-in arm's `FUN_801D5854(ctx[+0x13], 6)` on the caster, on the
    /// style `style` (`ctx[+0xD]` as the arm reads it).
    fn follow_capture_walk(&mut self, caster: u8, victim: u8, style: u8) {
        let seats = self.module_cam_seats(caster, victim);
        if let Some(cam) = self.battle.camera.as_mut() {
            let yaw_base = cam.action_yaw_base();
            let actor = legaia_engine_battle_vm::battle_cam_script::BattleCamActor {
                facing: i32::from(seats.caster.facing),
                world: [
                    f32::from(seats.caster.x),
                    f32::from(seats.caster.y),
                    f32::from(seats.caster.z),
                ],
                height: None,
            };
            cam.arm_module_follow(actor, yaw_base, seats.depth_raw, style);
        }
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
                // A body whose walk arms frame the caster through the action
                // SM's case 6 (`capture_approach_follows`) turns it and, on
                // arm 0, makes the arm's stores ahead of the range poll, so
                // a caster already in reach takes them too: PROT 0952's arm 0
                // turns both seats at `0x801F7250..0x801F7270`, polls at
                // `0x801F7278`, and reaches the case 6 at `0x801F72E4` from
                // either side of the poll.
                let walk_in = self.open_capture_walk_pass(run.slot, victim);
                if self.battle_range_metric(run.slot, victim) == 0 {
                    // Arrived. A body whose port stages its own strike takes
                    // the band from here; the rest go on to their stages.
                    run.ticks = 0;
                    // A body that walks on case 6 stages its next clip on the
                    // next pass, so the walk's root motion steps once more
                    // (`+0x34` reads `350` on Scythe Wind's next pass, one
                    // `+38` past the arrival's `312`).
                    if !self.settle_capture_approach(run.slot, victim) {
                        let a = &mut self.actors[usize::from(run.slot)];
                        if a.battle.queued_anim == CAPTURE_WALK_ENTRY {
                            a.battle.queued_anim = 0;
                        }
                    }
                    // The arrival pass frames the pushed-off seat.
                    if let Some(style) = walk_in {
                        self.follow_capture_walk(run.slot, victim, style);
                    }
                    if run.clips.is_empty() || !run.at_arms.is_empty() {
                        run.phase = CasterStagePhase::Module;
                        self.casting.caster_stages = Some(run);
                        return CasterStageGate::RunModule;
                    }
                    run.phase = CasterStagePhase::Staging;
                    self.casting.caster_stages = Some(run);
                    return CasterStageGate::Hold;
                }
                if walk_in.is_none() {
                    self.turn_caster_onto(run.slot, victim);
                }
                // The walk stage (`li v1,0x1; sb v1,0x1da(<caster>)`, e.g.
                // `0x801F72C8` in PROT 0952).
                self.actors[usize::from(run.slot)].battle.queued_anim = CAPTURE_WALK_ENTRY;
                if let Some(style) = walk_in {
                    self.follow_capture_walk(run.slot, victim, style);
                }
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

    /// The module's arm `arm` passed this tick: stage the caster clips the
    /// body stores in that arm ([`CasterStageRun::at_arms`]). Each goes in
    /// behind whatever the caster plays, its park released, and the arms run
    /// on while it plays - the module stages and moves on in the same pass.
    fn stage_caster_on_arm_pass(&mut self, arm: u8) {
        let Some(run) = self.casting.caster_stages.as_mut() else {
            return;
        };
        if run.phase != CasterStagePhase::Module || run.at_arms.is_empty() {
            return;
        }
        let Some(i) = run.at_arms.iter().position(|&a| a == arm) else {
            return;
        };
        let (slot, clip) = (run.slot, run.clips[i]);
        run.cursor = i;
        if let Some(a) = self.actors.get_mut(usize::from(slot)) {
            if let Some(p) = a.battle_animation.as_mut() {
                p.release_loop_window();
            }
            a.battle.queued_anim = clip;
        }
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

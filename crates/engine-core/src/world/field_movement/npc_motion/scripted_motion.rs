//! Script-driven NPC and actor motions: start, the run-anim carry and the per-
//! frame motion ticks.

use super::*;

impl World {
    /// Start a field NPC walking to world `(tx, tz)` through the motion VM -
    /// the engine's start-motion kernel for the MAN-placed actor set. Mirrors
    /// the retail start shape: write the walk target onto the actor and reset
    /// the glide state so the per-frame motion stepper picks it up (retail's
    /// `FUN_800358c0` writes the target into the actor `+0xA`/`+0xC` + subobj
    /// mirrors and clears the `+0x20` glide cursor; the per-frame consumer is
    /// the motion VM `FUN_8003774C`, ported in
    /// [`legaia_engine_motion_vm::motion_vm`]). Returns `false` (and does nothing)
    /// when `slot` is not an installed field NPC - the retail actor-list
    /// search miss, which returns 0.
    ///
    /// Every leg started here is *scripted*: it runs even while
    /// [`crate::world::FieldNpcState::animate`] is off and even during a dialogue
    /// (the interaction partner executing its own prologue walk), and ends
    /// where it lands.
    ///
    /// REF: FUN_800358c0, FUN_8003774C
    pub fn start_field_npc_motion(&mut self, slot: u8, tx: i16, tz: i16) -> bool {
        let Some(&(cx, cz)) = self.npcs.positions.get(&slot) else {
            return false;
        };
        // Faithful glide speed: the placement's own `0x4C 0x51` motion-op
        // base step (retail `FUN_8003774C` `4 << bits`), derived at scene load
        // into `field_npc_glide_speeds`; the stand-in `FIELD_NPC_MOTION_SPEED`
        // is the fallback for a placement with no decodable motion leg.
        let speed = self
            .npcs
            .glide_speeds
            .get(&slot)
            .copied()
            .unwrap_or(FIELD_NPC_MOTION_SPEED);
        self.npcs.motions.insert(
            slot,
            FieldNpcMotion {
                state: legaia_engine_motion_vm::motion_vm::MotionState {
                    world_x: cx,
                    world_y: 0,
                    world_z: cz,
                    speed,
                    // Seed from the facing the NPC is standing in (retail
                    // reads the live `+0x26`), so a leg that never moves - or
                    // one whose first frame is blocked - keeps it instead of
                    // snapping to the compass origin.
                    yaw: (self.npcs.heading(slot) & 0x0FFF) as u16,
                    ..Default::default()
                },
                target: (tx, tz),
            },
        );
        true
    }

    /// Attach the `4C 51` record's byte-`+4` **move-anim id** to a just-started
    /// NPC glide leg. Retail's run dispatch writes that byte to the actor's
    /// `+0x5C` anim slot (consumed by the anim-stream stepper `FUN_800204F8`),
    /// so the walk plays its named move clip instead of gliding in a frozen
    /// pose. The engine surfaces it as a [`crate::world::FieldNpcState::anim_cues`] entry -
    /// the same shape the cross-context `A2` ExecMove raises - keyed by the
    /// placement slot. A zero id carries no clip (retail's `+0x5C = 0` is the
    /// "no move-anim" sentinel, not clip `-1`).
    ///
    /// A request for the move the slot is already playing raises nothing:
    /// the consumer restarts a clip only when `+0x5C` differs from the
    /// playing `+0x5E` (`0x80020570..0x800205A8`), and a cue here would
    /// restart it at frame `0` on every host. That matters because the
    /// ambient walk ops request their walk move on every step, so without
    /// the test a walker's cycle never got past its first frames.
    ///
    /// REF: FUN_80024E08, FUN_800204F8 (actor `+0x5C` anim-slot consumer)
    pub(crate) fn carry_npc_run_anim(&mut self, slot: u8, move_id: u8) {
        if move_id != 0 && self.npcs.clip_current.get(&slot) != Some(&move_id) {
            self.npcs.anim_cues.insert(slot, (1, move_id, Vec::new()));
        }
    }

    /// Step every in-flight field-NPC walk leg one frame through the ported
    /// motion VM, writing each NPC's new position back into
    /// [`crate::world::FieldNpcState::positions`] - so the moving NPC's
    /// ±40-unit collision box ([`Self::field_actor_dir_blocked`]) and its
    /// interact box ([`Self::field_interact_probe_slot`]) follow the live
    /// position, exactly as retail probes the live `+0x14`/`+0x18` rather
    /// than the spawn anchor.
    ///
    /// Every leg here is **scripted**: a cutscene timeline's cross-context
    /// walk or an actor-VM `start_motion`. Each one ends where it lands, and none is gated by
    /// [`crate::world::FieldNpcState::animate`] - a script started it. There
    /// is no `0x4C 0x51` leg: that op is an instant seat wherever it runs,
    /// and a placement's own `0x4C 0x51` ops are instant
    /// story-branch **seats** that the scene-entry pre-run already applied,
    /// and a villager's ambient wandering is its tail-section-1 stream
    /// (`World::tick_field_npc_ambient`), so nothing walks between seats.
    ///
    /// REF: FUN_8003774C
    pub(crate) fn tick_field_npc_motions(&mut self) {
        self.tick_field_npc_face_legs();
        // A walk-to-tile leg moves on the game-frame grid: the `0x47` arm
        // spends `0x80 * DAT_1F800393 / (4 << bits)` a game frame
        // (`FUN_8003774C`, the step read at `0x80037864`), nothing between.
        let grid = self.clock.script_frame_delta();
        if grid == 0 {
            return;
        }
        let slots: Vec<u8> = self.npcs.motions.keys().copied().collect();
        for slot in slots {
            let Some(motion) = self.npcs.motions.get_mut(&slot) else {
                continue;
            };
            let target = legaia_engine_motion_vm::motion_vm::MotionTarget {
                x: motion.target.0,
                y: 0,
                z: motion.target.1,
                id: 0,
            };
            let base = motion.state.speed;
            motion.state.speed = base.saturating_mul(grid);
            let result = legaia_engine_motion_vm::motion_vm::step(
                &mut motion.state,
                target,
                &FIELD_NPC_MOTION_PROGRAM,
            );
            motion.state.speed = base;
            let pos = (motion.state.world_x, motion.state.world_z);
            // The walker's heading is the one the `0x47` step itself snapped
            // onto the eight-point compass - written **once per leg** at the
            // first moving frame and again on a step-direction change, the
            // retail write pattern (runtime-pinned: a walk leg holds one
            // heading for its whole run). Gating the copy on the VM's own
            // write keeps a heading some other writer posed (the interact
            // face-the-speaker bearing) standing while a leg idles unmoved.
            if motion.state.yaw_written {
                self.npcs.headings.insert(slot, motion.state.yaw as i16);
            }
            self.npcs.positions.insert(slot, pos);
            if result == legaia_engine_motion_vm::motion_vm::StepResult::Done {
                self.npcs.motions.remove(&slot);
            }
        }
    }

    /// Actor-VM glide start (`MotionAt` / `EffectMotion` → `start_motion`,
    /// retail `FUN_800358c0`): record the target on the actor and install a
    /// motion-VM leg gliding the actor's sprite position
    /// (`move_state.world_x` / `world_y`) toward it, stepped once per tick by
    /// [`Self::tick_actor_motions`]. The retail kernel writes the target into
    /// the actor `+0xA`/`+0xC` and its subobj mirrors and clears the `+0x20`
    /// glide cursor; the per-frame glide is the motion-VM pursue step.
    ///
    /// REF: FUN_800358c0
    pub(crate) fn start_actor_motion(&mut self, actor_id: u8, target: ActorVmPosition) {
        let Some(actor) = self.actors.get(actor_id as usize) else {
            return;
        };
        if !actor.active {
            return;
        }
        let (cx, cy) = (actor.move_state.world_x, actor.move_state.world_y);
        self.actors[actor_id as usize].motion_target = Some(target);
        self.move_vm.actor_motions.insert(
            actor_id,
            FieldNpcMotion {
                state: legaia_engine_motion_vm::motion_vm::MotionState {
                    world_x: cx,
                    // The sprite-actor glide runs in the actor VM's packed
                    // (x, y) plane; the motion VM's XZ pursue step maps
                    // y → z here.
                    world_z: cy,
                    speed: FIELD_NPC_MOTION_SPEED,
                    ..Default::default()
                },
                target: (target.x, target.y),
            },
        );
    }

    /// Step every actor-VM glide ([`Self::start_actor_motion`]) one frame
    /// through the motion VM, writing back into the actor's `move_state`.
    /// Finished or stale (despawned-actor) glides are dropped.
    ///
    /// REF: FUN_8003774C
    pub(crate) fn tick_actor_motions(&mut self) {
        if self.move_vm.actor_motions.is_empty() {
            return;
        }
        let slots: Vec<u8> = self.move_vm.actor_motions.keys().copied().collect();
        for slot in slots {
            let alive = self
                .actors
                .get(slot as usize)
                .is_some_and(|actor| actor.active);
            if !alive {
                self.move_vm.actor_motions.remove(&slot);
                continue;
            }
            let Some(motion) = self.move_vm.actor_motions.get_mut(&slot) else {
                continue;
            };
            let target = legaia_engine_motion_vm::motion_vm::MotionTarget {
                x: motion.target.0,
                y: 0,
                z: motion.target.1,
                id: 0,
            };
            let result = legaia_engine_motion_vm::motion_vm::step(
                &mut motion.state,
                target,
                &FIELD_NPC_MOTION_PROGRAM,
            );
            let (nx, ny) = (motion.state.world_x, motion.state.world_z);
            let actor = &mut self.actors[slot as usize];
            actor.move_state.world_x = nx;
            actor.move_state.world_y = ny;
            if result == legaia_engine_motion_vm::motion_vm::StepResult::Done {
                self.move_vm.actor_motions.remove(&slot);
            }
        }
    }
}

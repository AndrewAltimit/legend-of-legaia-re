//! Per-frame field eases and timers: the text-balloon width, register ramps,
//! the camera offset ease, timer actors and eased moves.

use super::*;

impl World {
    /// Commit a host font measurement of the live `4C E1` balloon's line, so
    /// the record carries the centred `x` retail computes at spawn
    /// (`X = (0x140 - width) >> 1`).
    ///
    /// Retail measures inside `FUN_8003C764` because its font metrics are in
    /// the same address space; the engine's atlas is host-side, so the
    /// measurement arrives from the draw layer instead
    /// (`legaia_engine_ui::text_balloon_text_width`). Idempotent - the width
    /// is committed once per balloon and a later call is ignored, which is
    /// what keeps a host that measures every frame from re-centring a
    /// balloon mid-life.
    ///
    /// Returns the pen to draw at, or `None` when no balloon is live.
    ///
    /// REF: FUN_8003C764 (`0x8003C7C0..0x8003C7DC`, the measure + centre)
    pub fn commit_text_balloon_width(&mut self, text_width_px: i16) -> Option<(i32, i32)> {
        let balloon = self.cutscene.text_balloon.as_mut()?;
        if balloon.x.is_none() {
            balloon.center_with_width(text_width_px);
        }
        balloon.pen()
    }

    /// The live `4C E1` balloon's raw page bytes while it is past its startup
    /// tick and still running - i.e. exactly the frames retail's handler
    /// reaches `FUN_80036888`. `None` otherwise.
    ///
    /// The startup band (`timer < 1`) draws nothing in retail, so a host that
    /// keys on `text_balloon.is_some()` shows the balloon one frame early.
    pub fn text_balloon_drawing(&self) -> Option<&[u8]> {
        let b = self.cutscene.text_balloon.as_ref()?;
        (!b.killed && b.timer >= 1 && b.timer < b.total).then_some(b.text.as_slice())
    }

    /// Run every live camera-register zone ramp (field-VM op `0x43`
    /// sub-3..6) for one frame - the port of `FUN_80037018`'s per-actor tick,
    /// driven off the player's world position.
    ///
    /// Retail runs one of these actors per spawned ramp off the effect-actor
    /// list; the engine holds the records on [`legaia_engine_field::camera_rig::CameraRig::register_ramps`] and
    /// steps them here. What a tick writes lands in
    /// [`legaia_engine_field::camera_rig::CameraRig::registers`], the four field camera-configuration
    /// registers `0x8007B60C`/`B610`/`B614`/`B618`.
    ///
    /// Two retail gates come first, both inside
    /// [`legaia_engine_motion_vm::ambient_motion::zone_ramp_tick`]: the
    /// player-engaged flag (`_DAT_8007C364[+0x10] & 0x80000`, host-substituted
    /// by "a dialog engagement is live", the same substitution the `4C E1`
    /// balloon uses) and the scratch system lock (`_DAT_1F800394 & 0x400`,
    /// which has no engine counterpart and is passed clear).
    ///
    /// Nothing counts down in a zone ramp, so a record never completes - it
    /// tracks the player and runs backwards when he walks back. Retail clears
    /// them on the MAN loader's retire sweep, which is
    /// [`World::reset_for_scene_entry`] here. Two arms do drop a record:
    /// an out-of-range destination width (retail sets the actor's own
    /// `+0x10 |= 8` yield bit instead of writing) and a degenerate `z_lo ==
    /// z_hi` window (retail divides by the zero span and executes the MIPS
    /// `break 0x1C00` trap - the port drops the record rather than reproducing
    /// a CPU exception; no on-disc ramp is authored that way).
    ///
    /// REF: FUN_80037018
    pub fn tick_register_ramps(&mut self) {
        if self.camera.register_ramps.is_empty() {
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let Some(actor) = self.actors.get(slot as usize) else {
            return;
        };
        let (px, pz) = (actor.move_state.world_x, actor.move_state.world_z);
        let engaged = self.dialogue_owns_input();
        let mut writes: Vec<(legaia_engine_field::register_ramp::RampSlot, i32)> = Vec::new();
        let mut retire: Vec<usize> = Vec::new();
        for (i, ramp) in self.camera.register_ramps.iter().enumerate() {
            match ramp.tick(px, pz, engaged) {
                legaia_engine_motion_vm::ambient_motion::ZoneRampTick::Write { value, .. } => {
                    writes.push((ramp.slot, value));
                }
                legaia_engine_motion_vm::ambient_motion::ZoneRampTick::Retire
                | legaia_engine_motion_vm::ambient_motion::ZoneRampTick::DivideByZero => {
                    retire.push(i)
                }
                legaia_engine_motion_vm::ambient_motion::ZoneRampTick::Idle => {}
            }
        }
        for (slot, value) in writes {
            self.camera.registers.set(slot, value);
        }
        for i in retire.into_iter().rev() {
            self.camera.register_ramps.remove(i);
        }
    }

    /// Run every live **frame-delta timer actor** for one frame: the
    /// cinematic bar envelope, the eased moves, and the floor-height-ladder
    /// oscillators.
    ///
    /// These are the three plain templates in the field overlay's own table
    /// (`0x801F2858` / `0x801F2840` / `0x801F27EC`), and they are one method
    /// because retail runs all three off the same actor list on the same
    /// frame delta - the engine holds each family on its own `World` field
    /// instead of a shared pool, but the cadence has to stay identical or a
    /// wipe and a move spawned by the same script beat would drift apart.
    ///
    /// `frame_delta` is retail's `DAT_1F800393`, the same scalar the physics
    /// and colour-tween passes carry.
    ///
    /// REF: FUN_801DD784, FUN_801DD4C4, FUN_801DA930 (the kernels live in
    /// [`legaia_engine_field::field_actor_timers`])
    /// The player actor's `+0x16` footing height, or `0` when there is no
    /// live player actor. The subtrahend of the camera-offset easing's target.
    pub fn camera_ease_player_footing(&self) -> i16 {
        self.player_actor_slot
            .map(usize::from)
            .filter(|&slot| slot < self.actors.len() && self.actors[slot].active)
            .map_or(0, |slot| self.actors[slot].move_state.world_y)
    }

    /// One frame of the camera vertical-offset easing.
    ///
    /// REF: FUN_801DA390 - this is the per-frame call site retail runs off the
    /// field frame pump; the ported kernel is
    /// [`legaia_engine_field::camera_ease::ease_camera_offset`], which carries the tag.
    ///
    /// Supplies the kernel's six actor-side inputs from the engine's own
    /// player actor. Two of them stand in for retail slots the engine does not
    /// carry: `+0x1E` / `+0x20` become the previous tick's `(world_y,
    /// world_z)`, so the settle test still answers "has the actor stopped
    /// moving in Y and Z". The pad word is [`legaia_engine_system::story_flag_state::StoryFlagState::story_flags`] - the same
    /// `_DAT_1F800394` scratchpad word retail reads the input lock and the
    /// fast-arm bit out of - and `_DAT_8007B850` is passed as `0`, which
    /// leaves the fast arm disengaged (see the comment at the call).
    ///
    /// The result is observable engine state, not a camera input:
    /// [`crate::world::camera_world`] still drives the rendered view from its float
    /// controller, and choosing between them is a fidelity-mode decision.
    pub fn tick_camera_offset_ease(&mut self) {
        let (y, z) = match self
            .player_actor_slot
            .map(usize::from)
            .filter(|&slot| slot < self.actors.len() && self.actors[slot].active)
        {
            Some(slot) => {
                let ms = &self.actors[slot].move_state;
                (ms.world_y, ms.world_z)
            }
            None => {
                self.camera.ease_prev_yz = None;
                return;
            }
        };
        let (prev_y, prev_z) = self.camera.ease_prev_yz.unwrap_or((y, z));
        self.camera.offset_ease = legaia_engine_field::camera_ease::ease_camera_offset(
            legaia_engine_field::camera_ease::CameraEaseInput {
                pad: self.flags.story_flags,
                scene_target: self.camera.scene_offset as u16,
                player_footing: y as u16,
                footing_settled: prev_y,
                z,
                z_target: prev_z,
                // `_DAT_8007B850` has no engine mirror, so the fast arm
                // never engages. It is a shortcut, not a behaviour: with it
                // clear the adaptive arm reaches the same destination in the
                // same 12-units-a-frame cap, only without pinning the step
                // while the gap is small.
                fast_flags: 0,
                current: self.camera.offset_ease,
            },
        );
        self.camera.ease_prev_yz = Some((y, z));
    }

    pub fn tick_field_timer_actors(&mut self, frame_delta: u8) {
        // The bar envelope. It retires itself at phase 3, and the published
        // height is what both hosts' screen-prim pass reads.
        if let Some(bars) = self.presentation.cinematic_bars.as_mut() {
            self.presentation.cinematic_bar = bars.step(frame_delta);
            if bars.retired {
                self.presentation.cinematic_bars = None;
                self.presentation.cinematic_bar = 0;
            }
        } else {
            self.presentation.cinematic_bar = 0;
        }

        // The eased moves. Retail writes the target's `+0x14/+0x16/+0x18`
        // straight through the back-link; the engine writes the same triple
        // wherever that target lives.
        if !self.field_vm.eased_moves.is_empty() {
            let mut records = std::mem::take(&mut self.field_vm.eased_moves);
            for rec in records.iter_mut() {
                let frame = rec.ease.step(frame_delta, rec.target_flags);
                self.apply_eased_move(rec.target, &frame);
            }
            records.retain(|r| !r.ease.retired);
            // A spawn that landed during this pass appended to the (empty)
            // live list, so splice rather than overwrite.
            records.append(&mut self.field_vm.eased_moves);
            self.field_vm.eased_moves = records;
            // Drop the `+0x8E` latch once the last mirrored player move has
            // retired. Retail's byte is sticky and its reader gates on the
            // actor flag `+0x10 & 0x20000000` instead, which a script clears;
            // the engine has no writer for that bit on a pool actor, so a
            // sticky latch here would pin the player's Y for the rest of the
            // scene - a softlock class, not a fidelity gain. The armed window
            // is otherwise identical: every frame of the move, and no other.
            if !self
                .field_vm
                .eased_moves
                .iter()
                .any(|r| matches!(r.target, crate::world::EasedMoveTarget::Player))
            {
                self.locomotion.eased_mirror_y = None;
            }
        }

        // The floor-height ladder. Each record owns one rung; a rung index
        // past the LUT is retail writing off the end of a 16-entry array,
        // which the port declines to do.
        if !self.terrain.floor_tier_bobs.is_empty() {
            let mut bobs = std::mem::take(&mut self.terrain.floor_tier_bobs);
            crate::world::step_floor_ladder(
                &mut bobs,
                &mut self.terrain.floor_height_lut,
                frame_delta,
            );
            bobs.append(&mut self.terrain.floor_tier_bobs);
            self.terrain.floor_tier_bobs = bobs;
        }
    }

    /// Write one eased-move frame through its `+0x90` back-link.
    ///
    /// The engine's two addressable targets are the player's pool slot and a
    /// scene NPC placement, which is exactly the pair the neighbouring
    /// `move_to` host resolves - see [`crate::world::EasedMoveTarget`].
    ///
    /// The `+0x8E` **inverted-Y mirror** is published too, into
    /// [`crate::world::FieldLocomotion::eased_mirror_y`]. It used to be dropped
    /// here for want of a consumer; the consumer is retail's own, and it was
    /// mis-read rather than missing. `FUN_8003BC08`'s height arm tests the
    /// same `0x20000000` flag before either of its ground-height arms
    /// (`0x8003BC4C..0x8003BC64`) and writes `-(+0x8E)` into the actor's
    /// `+0x16`, which is the Y **position** the eased move itself writes - so
    /// the mirror is a hold: it re-asserts the scripted Y against the
    /// per-frame floor follow. The engine's two height controllers
    /// (`World::locomotion.vertical_settle` and `World::locomotion.follow_terrain_height`)
    /// are the ports of that routine's other two arms and both stand down
    /// while the latch is armed.
    ///
    /// Only the player half is published. A scene NPC placement has no
    /// per-frame height controller in this engine (its Y is baked at scene
    /// build by [`legaia_scene_formats::field_objects::Placement::world_y`]), so a
    /// mirror on one would be a write with no reader - the same reason the
    /// whole field was withheld before, now true of one target instead of
    /// both.
    fn apply_eased_move(
        &mut self,
        target: crate::world::EasedMoveTarget,
        frame: &legaia_engine_field::field_actor_timers::EasedMoveFrame,
    ) {
        match target {
            crate::world::EasedMoveTarget::Player => {
                // Published whether or not a seat resolves: the latch is a
                // property of the move, and retail's store goes through the
                // back-link ahead of anything that reads the seat.
                self.locomotion.eased_mirror_y = frame.mirror_y;
                let Some(slot) = self.player_actor_slot else {
                    return;
                };
                let Some(actor) = self.actors.get_mut(slot as usize) else {
                    return;
                };
                if let Some(x) = frame.axis[0] {
                    actor.move_state.world_x = x;
                    actor.physics.world_x = x;
                }
                if let Some(y) = frame.axis[1] {
                    actor.physics.world_y = y;
                }
                if let Some(z) = frame.axis[2] {
                    actor.move_state.world_z = z;
                    actor.physics.world_z = z;
                }
                if let Some(x) = frame.axis[0] {
                    self.field_ctx.world_x = x as u16;
                }
                if let Some(z) = frame.axis[2] {
                    self.field_ctx.world_z = z as u16;
                }
            }
            crate::world::EasedMoveTarget::Placement(slot) => {
                let cur = self.npcs.positions.get(&slot).copied();
                let (mut x, mut z) = cur.unwrap_or((0, 0));
                if let Some(nx) = frame.axis[0] {
                    x = nx;
                }
                if let Some(nz) = frame.axis[2] {
                    z = nz;
                }
                self.npcs.positions.insert(slot, (x, z));
            }
        }
    }
}

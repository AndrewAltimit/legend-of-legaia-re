//! `Camera` per-frame step: the stream tick, the globals tick and the camera script step.

use super::*;

impl Camera {
    /// [`Self::tick`] on the world's `rand()` stream: the follow ease's shake
    /// pair draws from `World::rng_state` in tick order, as retail's
    /// `FUN_801DB510` draws from the one BIOS seed. The shared
    /// `crate::frame_step::camera_after_world_tick` runs this one.
    pub fn tick_on_stream<W: CameraWorld + ?Sized>(&mut self, world: &mut W) {
        self.zone.shake_seed = world.rng_state();
        self.tick(world);
        world.set_rng_state(self.zone.shake_seed);
    }

    /// Per-frame tick. Reads the world to update `eye` / `look_at` based on
    /// `mode`. Pure function over the world - engines call after
    /// [`World::tick`] each frame.
    ///
    /// [`World::tick`]: legaia_engine_core::world::World::tick
    pub fn tick<W: CameraWorld + ?Sized>(&mut self, world: &W) {
        self.tick_globals(world);
        match self.mode {
            CameraMode::Follow => {
                if let Some((ax, ay, az)) = world.active_actor_pos(self.follow_slot as usize) {
                    let tx = ax as f32;
                    let ty = ay as f32;
                    let tz = az as f32;
                    self.look_at = [tx, ty + self.follow_height, tz];
                    // Effective yaw = scripted yaw + the user's manual orbit
                    // (compass sense: forward = (sin, cos)). Distance preset
                    // scales the eye-back distance only. Defaults (orbit 0,
                    // Retail) keep this arithmetic bit-identical to the
                    // historical `yaw`/`follow_distance` form.
                    let yaw = self.yaw + self.manual_orbit;
                    let dist = self.follow_distance * self.distance.scale() * self.manual_zoom;
                    self.eye = [
                        tx - dist * yaw.sin(),
                        ty + self.follow_height,
                        tz - dist * yaw.cos(),
                    ];
                }
            }
            CameraMode::Static | CameraMode::Cinematic => {
                // No per-frame motion - keep eye/look_at at whatever the last
                // event configured.
            }
        }
    }

    /// Advance the retail camera globals one frame: run any in-flight mover
    /// glide, then let the follow camera write the focus it owns.
    ///
    /// The mover is clocked in **display frames**, not sim ticks - retail
    /// credits `DAT_1F800393` (the adaptive frame-skip factor) per logic tick,
    /// which banks exactly one unit per display frame, making every authored
    /// `apply` a duration in 60 Hz frames. `World::clock.display_frames` is the
    /// engine's display-frame counter, so diffing it is the faithful clock
    /// (the same thing the renderer's glide does).
    ///
    /// In [`CameraMode::Follow`] with no glide in flight, the follow camera
    /// owns the focus globals: `FUN_801DBE9C` stores the **negated** anchor
    /// position (`_DAT_80089118 = -(anchor+0x14)`,
    /// `_DAT_80089120 = -(anchor+0x18)`). Writing it here is what makes a
    /// free-roam field frame comparable against a retail capture, which
    /// samples those same globals whether a cutscene is running or not.
    ///
    /// PORT: FUN_801DC0BC
    /// REF: FUN_801DBE9C
    fn tick_globals<W: CameraWorld + ?Sized>(&mut self, world: &W) {
        let now = world.display_frames();
        let dt = now.saturating_sub(self.last_field_frame) as i32;
        self.last_field_frame = now;

        let gliding = if let Some(mv) = self.mover.as_mut() {
            let arrived = mv.tick(dt);
            self.globals.0 = mv.values();
            if arrived {
                self.mover = None;
            }
            true
        } else {
            false
        };

        // The follow camera only owns the focus in free-roam. A cutscene's
        // staged focus must survive the gaps BETWEEN its beats, not just the
        // frames a glide happens to be in flight: retail's `opdeene` holds two
        // distinct focus values across the whole scene, so a writeback gated
        // only on `!gliding` re-pins the focus to the player on every settled
        // frame and turns those two values into ~1000.
        let scripted = gliding || self.script_owns_focus || world.cutscene_timeline_active();
        // A player arc whose release watcher runs the follow camera
        // (`FUN_801D5D60` calling `FUN_801DB510(player)` + `FUN_801DAA50`
        // every frame the arc flies) hands a cutscene's shot back to the
        // follow step for the arc's duration. A glide in flight keeps the
        // frame: it is the shot the script is actively moving.
        let arc_follow = !gliding && world.script_arc_follow_camera();
        let scripted = scripted && !arc_follow;
        // In a zone-camera scene the focus is the follow ease's to write, and
        // only on the legs that write it (see [`Self::zone_follow_tick`]): a
        // standing or movement-locked player leaves it where it was.
        let zone_scene = zone_camera_scene(world);
        if !scripted
            && !zone_scene
            && self.mode == CameraMode::Follow
            && let Some((ax, _, az)) = world.active_actor_pos(self.follow_slot as usize)
        {
            self.globals.0[6] = -ax;
            self.globals.0[8] = -az;
        }

        // The zone-driven follow camera: retail's per-scene / per-tile camera
        // parameters composed into the same ten globals, then eased. It runs
        // in any walkable scene with terrain loaded (the zone table and the
        // walk-region table are what it queries) and only while nothing
        // scripted owns the shot - the kingdom overworld included: retail's
        // overworld is an ordinary mode-`0x03` field-run scene, and on all
        // three resident overworld states the live pitch / yaw / eye trio /
        // `H` equal the follow composer's staging descriptor at `0x801F3580`
        // field for field (see [`zone_camera_scene`]).
        if zone_scene {
            // A scripted shot handing the camera back snaps. Retail's
            // scripts do this themselves through the `[4C 39]` / `[4C 3E]`
            // arms (`FUN_801DB8EC`), which is how every walkable
            // post-opening state in the library holds a settled follow pose
            // (live == staging) rather than an ease in flight from the
            // cinematic shot. Those arms are wired now
            // ([`CameraZoneRequest`]); the hand-back edge
            // stays as the port's backstop for a shot the script drops
            // without one, and it snaps from the resident block rather than
            // re-querying (retail's hand-back does not re-query either).
            // The arc's follow is an ease, not a hand-back: retail's
            // watcher calls the ease, never the snap.
            if self.zone.prev_scripted && !scripted && !arc_follow {
                self.zone.snap_pending = true;
                self.zone.prev_player = None;
            }
            self.zone.prev_scripted = scripted;
            if !scripted && self.mode == CameraMode::Follow {
                self.zone_follow_tick(world, dt);
            } else if gliding && self.mode == CameraMode::Follow {
                // A scripted glide in a free-roam scene drives these same
                // globals from the zone pose it was primed with; the follow
                // view keeps reading them rather than its fallback.
                self.zone.active = !self.zone.snap_pending;
            }
            return;
        }

        // A world without field terrain (unit worlds, the ramp oracles):
        // camera-register zone ramps (field-VM op `0x43` sub-3..6) own four
        // of these ten axes while the player stands in an authored zone. The
        // field-overlay camera composer reads all four straight into the same
        // camera descriptor these globals mirror - see
        // [`crate::register_ramp::RampSlot::camera_axis`] for the per-register
        // store sites. (In a terrain-bearing scene the same four registers
        // are folded into the parameter block instead, which is where retail
        // keeps them - see [`Self::zone_follow_tick`].)
        //
        // Gated on a register having actually been written: with no ramp in
        // the scene the file still holds `CAMERA_ZONE_DEFAULTS`, whose
        // `0x4000` eye-back is *not* the `16420` this camera resets to, so an
        // ungated feed would re-frame every ramp-free scene. A scripted glide
        // still wins - it is the shot the script staged.
        // REF: FUN_801DABA4 (the field-overlay camera composer)
        if !gliding && world.camera_registers().written() {
            for slot in crate::register_ramp::RampSlot::ALL {
                let axis = slot.camera_axis();
                let v = world.camera_registers().get(slot);
                // Retail's eye-back store is a halfword whose sign the
                // composer folds into the yaw (it picks which side of the
                // player the orbit sits on), so the depth axis takes the
                // magnitude; the port's follow camera has no side flip.
                self.globals.0[axis] = if axis == 5 { v.abs() } else { v };
            }
        }
    }
}

impl Camera {
    /// Drive the cinematic motion script for one tick. Optional layer above
    /// [`Camera::tick`] - engines that want to pre-bake camera paths upload
    /// motion-VM bytecode and call this each frame.
    pub fn tick_script(&mut self, bytecode: &[u8]) -> StepResult {
        step(&mut self.motion_state, self.motion_target, bytecode)
    }
}

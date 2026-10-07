//! Player free-movement locomotion: pad direction decode, the per-frame step, ledge hops, vertical follow, the player clip picks and collision-advanced steps.
//! Split out of `field_movement.rs`.

use super::*;

impl World {
    /// Decode this frame's held d-pad into a camera-relative movement
    /// direction and an 8-direction heading angle. Returns
    /// `(dir_bits, heading)` where `dir_bits` uses the retail post-remap
    /// convention (`0x1000` = Z+, `0x4000` = Z-, `0x2000` = X+, `0x8000` =
    /// X-) and `heading` is a PSX 12-bit angle (`4096` = full turn).
    /// `dir_bits == 0` means no direction is held.
    ///
    /// The raw screen direction (up / down / left / right) is remapped through
    /// retail's eighth-turn ring ([`Self::remap_pad_direction`], the port of
    /// `func_0x800467e8`) with the octant derived from
    /// [`crate::world::FieldLocomotion::camera_azimuth`] - retail reads a
    /// scene-authored octant instead - so "screen up" always walks away from
    /// the camera, including on the diagonal cameras a quadrant decode cannot
    /// express.
    pub(super) fn decode_field_direction(&self) -> (u16, i16) {
        let up = self.input.pressed(input::PadButton::Up);
        let down = self.input.pressed(input::PadButton::Down);
        let left = self.input.pressed(input::PadButton::Left);
        let right = self.input.pressed(input::PadButton::Right);

        // Retail's raw d-pad direction nibble, the mask `FUN_800467E8` takes.
        // Opposite keys cancel: a d-pad cannot press both, but a keyboard can,
        // and retail's linear ring scan would fall out at index 8 and walk the
        // camera's own forward direction for such a mask.
        let mut held = 0u16;
        if up != down {
            held |= if up { 0x1000 } else { 0x4000 };
        }
        if right != left {
            held |= if right { 0x2000 } else { 0x8000 };
        }
        if held == 0 {
            return (0, 0);
        }

        // Rotate the held mask around the eight-direction compass ring, the
        // way retail does ([`Self::remap_pad_direction`]). Rotation `0` is the
        // identity: screen-up walks world `Z+`, screen-right walks `X+`.
        let dir = Self::remap_pad_direction(held, self.field_pad_ring_rotation()) & 0xF000;

        let wx = if dir & 0x2000 != 0 {
            1
        } else if dir & 0x8000 != 0 {
            -1
        } else {
            0
        };
        let wz = if dir & 0x1000 != 0 {
            1
        } else if dir & 0x4000 != 0 {
            -1
        } else {
            0
        };
        let bits = dir;

        // Heading: atan2(wx, wz) in 12-bit units. Z+ = 0, X+ = quarter turn.
        let heading = (((wx as f32).atan2(wz as f32) / std::f32::consts::TAU * 4096.0).round()
            as i32
            & 0x0FFF) as i16;
        (bits, heading)
    }

    /// Continuous (non-quantised) camera-relative movement decode for the
    /// opt-in [`crate::world::FieldLocomotion::precise_movement`] mode. Returns
    /// `(world_dir, dir_bits, heading)` where `world_dir` is the unnormalised
    /// world-space XZ movement vector, `dir_bits` is the sign-derived retail
    /// direction mask (kept for the facing / animation / touch consumers that
    /// key on [`crate::world::FieldLocomotion::last_move_dir_bits`]), and `heading` is the continuous
    /// PSX 12-bit angle. `None` when no direction is held.
    ///
    /// Differences from [`Self::decode_field_direction`] (the retail path):
    /// the camera azimuth rotates the screen vector at full angular
    /// resolution instead of snapping to the nearest 90°, and a deflected
    /// analog stick ([`crate::input::InputState::lstick`], PSX `[-127, 127]`
    /// axes, +Y down) supplies an arbitrary screen angle - digital keys are
    /// the 8-way fallback when the stick rests inside the deadzone.
    pub(super) fn decode_field_direction_precise(&self) -> Option<((f32, f32), u16, i16)> {
        const STICK_DEADZONE: i32 = 24;
        let (lx, ly) = self.input.lstick();
        let (sx, sy) = if (lx as i32).pow(2) + (ly as i32).pow(2) >= STICK_DEADZONE.pow(2) {
            // Stick +Y is down (PSX convention); screen forward is up.
            (lx as f32 / 127.0, -(ly as f32) / 127.0)
        } else {
            let mut sx = 0.0f32;
            let mut sy = 0.0f32;
            if self.input.pressed(input::PadButton::Up) {
                sy += 1.0;
            }
            if self.input.pressed(input::PadButton::Down) {
                sy -= 1.0;
            }
            if self.input.pressed(input::PadButton::Right) {
                sx += 1.0;
            }
            if self.input.pressed(input::PadButton::Left) {
                sx -= 1.0;
            }
            (sx, sy)
        };
        if sx == 0.0 && sy == 0.0 {
            return None;
        }
        // Rotate the screen vector by the camera azimuth continuously.
        // Azimuth 0 = identity (screen-up -> +Z, screen-right -> +X); the
        // quadrant table in `decode_field_direction` is this rotation
        // sampled at the four cardinal angles.
        let az = self.locomotion.camera_azimuth as f32 / 4096.0 * std::f32::consts::TAU;
        let (sin, cos) = az.sin_cos();
        let wx = sx * cos + sy * sin;
        let wz = -sx * sin + sy * cos;
        let mut bits = 0u16;
        if wz > f32::EPSILON {
            bits |= 0x1000; // Z+
        } else if wz < -f32::EPSILON {
            bits |= 0x4000; // Z-
        }
        if wx > f32::EPSILON {
            bits |= 0x2000; // X+
        } else if wx < -f32::EPSILON {
            bits |= 0x8000; // X-
        }
        if bits == 0 {
            return None;
        }
        let heading =
            ((wx.atan2(wz) / std::f32::consts::TAU * 4096.0).round() as i32 & 0x0FFF) as i16;
        Some(((wx, wz), bits, heading))
    }

    /// Free-movement locomotion step - the engine-side port of
    /// `FUN_801d01b0` (field overlay `overlay_0897`).
    ///
    /// PORT: FUN_801d01b0
    ///
    /// Reads this frame's
    /// pad, turns it into a camera-relative direction + facing, and
    /// advances the player actor in 2-unit increments with per-axis
    /// collision against [`crate::world::FieldTerrain::collision_grid`].
    ///
    /// No-ops when there is no player actor, while a dialog box is up (the
    /// field VM owns the frame), while the tile-board minigame is installed
    /// (that mode runs its own digital stepper), or while the player's
    /// movement-disabled flag (`+0x10 & 0x80000`) is set (encounter queued
    /// / cutscene owns the player). Reads only pad bits + grid + actor
    /// state, so it is deterministic across identical pad streams.
    ///
    /// # Call cadence, and why it is not `frame_step`-gated
    ///
    /// Retail calls `FUN_801D01B0` **once per game tick** - unconditionally,
    /// from the field frame pump `FUN_801D1344` (`jal 0x801D01B0` at
    /// `0x801D16F4`) - and a game tick spans `DAT_1F800393` vsyncs. The call's
    /// travel budget is scaled by that same byte
    /// (`0x801D0564..0x801D05C4`), so the *wall* speed is cadence-invariant:
    /// `base_step` units per **vsync**, i.e. `base_step * 60` units per second,
    /// at every cadence the resolver can pick.
    ///
    /// This port takes the fine-grained half of that identity - one call per
    /// vsync with the scalar ([`crate::world::MoveVmGlobals::ramp_ratio`]) at `1` - because a
    /// sim tick is one retail display frame (see [`World::tick`]). Same wall
    /// speed, twice the intermediate poses at retail's field floor of 2.
    /// Gating it on [`crate::world::FrameClock::display_frame_step`] would be a tautology under
    /// that denomination and a 0.6x slowdown under any other.
    ///
    /// | base step | selector | units/vsync | units/second |
    /// |---|---|---|---|
    /// | `5` | forced slow | 5 | 300 |
    /// | `8` | walk (default) | 8 | 480 |
    /// | `0xC` | run | 12 | 720 |
    /// | `0x18` | debug turbo | 24 | 1440 |
    ///
    /// (`player[+0x72]` is `0x1000` = 1.0 for the field player, so the
    /// `>> 12` multiplier drops out; the diagonal normalise trims x0.75.)
    pub fn step_field_locomotion(&mut self) {
        // Retail `0x801d0550` clears the step-delta pair before the frame's
        // direction decode, so an input-free (or fully wall-blocked) frame
        // leaves `(0, 0)` behind and the ledge-hop trigger stays quiet.
        self.locomotion.step_delta = (0, 0);
        // The player tick drains the post-warp hold before any of its gates
        // (`FUN_801D1344` at `0x801D1618..0x801D1630`).
        let ratio = self.move_vm.ramp_ratio.max(1);
        vm::field_warp_tile::drain_post_warp_hold(&mut self.locomotion.warp, ratio);
        // BOTH dialogue channels, through the shared predicate. The ordinary
        // NPC talk runs the field-VM inline runner, which holds a box open
        // without a `current_dialog` whenever the record selects its segment
        // from a prologue - so a `current_dialog`-only test left the pad
        // walking the player around under the box.
        if self.dialogue_owns_input() || self.board.grid.is_some() {
            return;
        }
        // A committed battle (an encounter's intro transition, a latched
        // scripted fight) freezes the pad controller outright. Retail raises
        // the player's `+0x10 |= 0x80000` in the roll itself (`FUN_801D9E1C`)
        // and then pages the intro overlay (PROT 0979) over the field
        // overlay's head, which holds the frame pump `FUN_801D1344` that calls
        // this controller - so nothing walks, and nothing is touched, between
        // the commit and the battle. Walking on here let the player bump a
        // prop in the transition's last frames: its run raised the engaged
        // bit, battle entry dropped the run, and nothing ever cleared the
        // bit again (`town0b`, the player frozen at tile (37, 40)).
        // REF: FUN_801D9E1C, FUN_801D1344
        if self.field_scripts_held_for_battle() {
            return;
        }
        // Lock pad-driven locomotion while an opening-cutscene timeline owns
        // the scene (the establishing camera sweep + name-entry). During the
        // sweep the script drives the lead actor through its own MoveTo ops;
        // the pad must not also walk the player out from under the cinematic
        // camera. Releases the frame the timeline drops (matches retail, where
        // free-roam control returns only after the opening choreography ends).
        // A concurrent helper record holds the pad the same way (retail's
        // engaged bit is raised for every context the script runner steps).
        if self.script_context_engages_player() {
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return;
        }
        if self.actors[slot].move_state.flags & 0x0008_0000 != 0 {
            return;
        }
        // A kind-0 warp in flight, and the hold its landing leaves, keep the
        // pad controller off entirely (`0x801D16C8..0x801D16E4`): the player
        // stands through the fade instead of walking out of it - and stands
        // idle, because the warp is not a movement lock, so the system
        // channel's per-tick reset of the clip base still runs after each
        // settle ([`Self::tick_field_system_channel_clip_reset`]) and leaves
        // the idle base `2` for the next one.
        if vm::field_warp_tile::pad_suppressed(&self.locomotion.warp) {
            return;
        }

        // Opt-in precise mode swaps the quantised d-pad remap for the
        // continuous decode; the default path is bit-identical to the
        // historical quantised behaviour.
        let precise = if self.locomotion.precise_movement {
            self.decode_field_direction_precise()
        } else {
            None
        };
        let (dir_bits, heading) = if self.locomotion.precise_movement {
            precise.map(|(_, b, h)| (b, h)).unwrap_or((0, 0))
        } else {
            self.decode_field_direction()
        };
        self.locomotion.last_move_dir_bits = dir_bits;
        // The clip base (`_DAT_8007BDD8`) the settle tail strides into the
        // player's clip: idle, walk or run off this frame's direction and base
        // step, the scene sentinel under `_DAT_8007B6A8`
        // (`0x801D0424..0x801D04A4`). The same frames raise the party-bank bit.
        if let Some(base) = vm::field_player_clip::locomotion_clip_base(
            self.locomotion.player_clip,
            dir_bits,
            self.field_base_step(),
            self.party.scene_save_allowed,
        ) {
            self.locomotion.clip_base = base;
            self.locomotion.player_party_bank = true;
            if let Some(anim) = &mut self.locomotion.player_anim {
                anim.pad_drove_this_frame = true;
            }
        }
        if dir_bits == 0 {
            // Input released: drop any precise sub-step remainder so a later
            // hold starts clean.
            self.locomotion.precise_move_carry = (0.0, 0.0);
            return;
        }
        self.actors[slot].move_state.render_26 = heading;

        // speed = ((base_step * player[+0x72]) >> 12) * DAT_1f800393.
        let mult = self.actors[slot].move_state.field_72 as i32;
        let ratio = i32::from(ratio);
        let mut speed = ((self.field_base_step() * mult) >> 12) * ratio;
        // Diagonal normalise (camera mode 4, both axes pressed): x0.75.
        // The precise path normalises its vector instead (below), so the
        // fixed cut only applies to the quantised path.
        let z_pressed = dir_bits & 0x5000 != 0;
        let x_pressed = dir_bits & 0xA000 != 0;
        if precise.is_none() && z_pressed && x_pressed {
            speed -= speed >> 2;
        }
        if speed <= 0 {
            return;
        }

        // A held direction is a movement frame for the locomotion animation
        // even when the step is wall-blocked (retail walks in place).
        if let Some(anim) = &mut self.locomotion.player_anim {
            anim.moved_this_frame = true;
        }

        let before = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x, ms.world_z)
        };
        if let Some(((wx, wz), _, _)) = precise {
            self.advance_with_collision_vector(slot, wx, wz, speed);
        } else {
            // Retail runs the camera-remapped mask through the wall-slide
            // resolver before the step loop (`jal 0x80046494` at
            // `0x801D03EC`, result kept in `s0` at `0x801D0404`), so the
            // mask the per-axis loop walks can carry a perpendicular bit
            // the pad never asked for - the skid along a wall. The heading
            // and the diagonal speed cut stay on the HELD mask; only the
            // step reads the resolved one.
            let step_bits = self.resolve_field_slide(dir_bits, before.0, before.1);
            self.advance_with_collision(slot, step_bits, speed);
        }
        // Walk-regen accumulator (retail `_DAT_801F2274`): a frame in which
        // the step actually committed counts as walked.
        //
        // The FILL is retail's, and it lives in this very function - the tail
        // at `0x801D0910..0x801D0928` (`lui a0,0x801f` / `lbu v1,0x393(v1)`
        // with `v1 = 0x1F800000` / `lw v0,0x2274(a0)` / `addu v0,v0,v1` /
        // `sw v0,0x2274(a0)`) adds `DAT_1F800393` to it, behind the
        // step-delta-non-zero test at `0x801D08F4..0x801D090C`. So retail
        // credits one unit per **vsync** whose step committed, matching the
        // per-vsync denomination the rest of the controller uses; adding
        // `field_frame_step` (`1`) once per sim tick is the same rate.
        // The DRAIN is retail-pinned too (`> 0x20` gate, `-= 0x20`, the three
        // per-member bumps). See [`World::tick_field_walk_regen`].
        {
            let ms = &self.actors[slot].move_state;
            if (ms.world_x, ms.world_z) != before {
                self.locomotion.walk_regen_steps = self
                    .locomotion
                    .walk_regen_steps
                    .saturating_add(self.clock.display_frame_step as i32);
            }
        }

        // Walk-touch dispatch (retail: the per-sub-step touch check inside
        // `FUN_801d01b0`, posting `FUN_801d5b5c` on a static-entity contact
        // with no button press): post a touched placement's walk-touch event.
        self.check_field_walk_touch();
        // The sibling post the same probe makes: the motion VM's one-slot
        // touch mailbox, which wakes a bumped NPC out of its ambient wait.
        self.post_ambient_motion_touch();

        // Terrain follow (gated): after the X/Z step commits, snap the
        // player's Y to the per-scene floor elevation at the new tile. Done
        // here rather than inside the shared `advance_with_collision` so the
        // world-map walk path (which collides through the same routine but
        // derives height from the continent grid) is unaffected. No-op height
        // 0 until a scene supplies a floor LUT.
        if self.locomotion.follow_terrain_height && !self.player_script_arc_live() {
            let y = match self.field_actor_mirrored_y(slot) {
                Some(mirror) => i32::from(mirror),
                None => {
                    let (x, z) = {
                        let ms = &self.actors[slot].move_state;
                        (ms.world_x as i32, ms.world_z as i32)
                    };
                    self.sample_field_floor_height(x, z)
                }
            };
            self.actors[slot].move_state.world_y = y as i16;
        }
    }

    /// The `+0x8E` inverted-Y override for `slot`, if one is armed.
    ///
    /// Retail's field-actor driver `FUN_8003BC08` picks one of three height
    /// laws per frame off the actor flag word `+0x10`, and this is the first
    /// of them (`0x8003BC4C..0x8003BC64`):
    ///
    /// ```text
    /// 8003bc4c  lui  v0,0x2000        ; flags & 0x20000000
    /// 8003bc54  beq  v0,zero,...      ;   clear -> the two ground arms
    /// 8003bc5c  lhu  v0,0x8e(s1)      ; the mirror halfword
    /// 8003bc60  j    0x8003bcf4
    /// 8003bc64  _subu v0,zero,v0      ; ...negated
    /// 8003bcf4  sh   v0,0x16(s1)      ; -> the actor's Y position
    /// ```
    ///
    /// The branch jumps **past** both ground arms, so an armed mirror is an
    /// override and not a bias: the floor is not sampled at all that frame.
    /// [`crate::world::FieldLocomotion::eased_mirror_y`] is where the eased-move tick publishes
    /// the halfword; the double negation (`-Y` stored, `-(+0x8E)` read back)
    /// is retail's, and it lands the actor on the eased Y.
    ///
    /// PORT: FUN_8003BC08 (`0x8003BC4C..0x8003BC64` + `0x8003BCF4`, the
    /// mirror arm of the height dispatch)
    pub(super) fn field_actor_mirrored_y(&self, slot: usize) -> Option<i16> {
        if self.player_actor_slot? as usize != slot {
            return None;
        }
        self.locomotion.eased_mirror_y.map(|m| m.wrapping_neg())
    }

    /// The height retail's ledge classifier measures its rise **from**: the
    /// actor's footing, i.e. what `param_1 + 0x16` holds by the time
    /// `FUN_801d1878` reads it.
    ///
    /// REF: FUN_801d1ba0, FUN_80019278
    ///
    /// That value is not free-floating. `FUN_801d1ba0` glides `+0x16` toward
    /// `FUN_80019278(actor)` - the floor under the actor's *current* position -
    /// every field frame, clamped to `+-rate`
    /// (`0x801D1C30..0x801D1C68`: `jal 0x80019278`, `subu a0, v0, v1`, the two
    /// `slt` clamps, `sh v0, 0x16(s1)`), and only *then* calls the classifier
    /// (`0x801D1CB0`). So retail's `rise` is the **local step ahead**, never the
    /// absolute floor elevation: a player standing on flat ground at any tier
    /// has `+0x16` equal to that tier's height and classifies a rise of zero.
    ///
    /// Pinned by the wall-press captures: both park the player on `town0c`'s
    /// `-192` floor and both carry `player + 0x16 == -192`, byte-equal to what
    /// [`Self::sample_field_floor_height`] returns under them.
    ///
    /// The engine only maintains `world_y` as a footing when one of its two
    /// height controllers is on - [`crate::world::FieldLocomotion::vertical_settle`] (retail's
    /// glide, ported in [`Self::step_field_vertical`]) or
    /// [`crate::world::FieldLocomotion::follow_terrain_height`] (the snap the walk path applies, and
    /// the `play-window` default). With both off, `world_y` is left untouched
    /// at whatever placed the actor - an invariant the locomotion oracles pin -
    /// so it carries no footing at all and reading it here would make every
    /// non-zero floor tier look like a ledge. In that configuration the footing
    /// comes from the sampler retail's settle targets, which is the value the
    /// glide converges to.
    pub(super) fn field_actor_footing(&self, slot: usize, x: i32, z: i32) -> i32 {
        if self.locomotion.vertical_settle || self.locomotion.follow_terrain_height {
            self.actors[slot].move_state.world_y as i32
        } else {
            self.sample_field_floor_height(x, z)
        }
    }

    /// Ledge-hop probe + post: retail `FUN_801d1878` (field overlay
    /// `overlay_0897`, 202 instructions at file offset `0x3060`).
    ///
    /// PORT: FUN_801d1878
    /// REF: FUN_801cfe4c, FUN_80019278, FUN_801d2404
    ///
    /// Decides whether the actor may hop onto (or down off) the ledge it is
    /// walking into, and posts the hop into [`crate::world::FieldLocomotion::ledge_hop`].
    /// Returns `true` when a hop was started - retail's `v0`.
    ///
    /// The probe direction is [`crate::world::FieldLocomotion::step_delta`], the last
    /// *committed* sub-step direction, scaled by 4 (retail `s1 = dx << 2`).
    /// Two forward points are tested against the collision grid through
    /// [`Self::field_tile_is_wall`]:
    ///
    /// | Point | Offset | Retail |
    /// |---|---|---|
    /// | near | `pos + 2 * delta * 4` (64 units) | `0x801d18b0..0x801d1984` |
    /// | far | `pos + 3 * delta * 4` (96 units) | `0x801d198c..0x801d1a5c` |
    ///
    /// **Both must be clear.** A wall at either kills the hop - the actor is
    /// walking into a wall, not up a step, and retail returns `0` without
    /// touching anything.
    ///
    /// The wall test is the same sub-cell derivation as `FUN_801cfe4c`, not
    /// merely a similar one: retail inlines it here, and the inlined copy is
    /// instruction-for-instruction identical to the standalone routine -
    /// same `(z >> 6) + 2` / `((x + 0x3f) >> 6) - 1` biases, same
    /// `row = (zc + sign) >> 1` stride-`0x80` index, same
    /// `quad = (zc & 1) << 1 | (xc & 1)` selector, same high-nibble read.
    /// So [`Self::field_tile_is_wall`] is reused rather than re-derived.
    ///
    /// With both points clear the near point's floor height decides the
    /// class, against [`FIELD_HOP_UP_THRESHOLD_DOWNWARD`] /
    /// [`FIELD_HOP_DOWN_THRESHOLD_DOWNWARD`]; a height inside that band is
    /// flat ground and starts no hop.
    ///
    /// **The two wall probes are not on their own what refuses a wall press.**
    /// They refuse only once the actor is close enough for a probe to cross
    /// into the wall's sub-cell, and the sub-cell grid is coarse: at the
    /// `rimelm_wall_press_left` capture the wall is sub-cell column `27`, the
    /// walk rests the player at `1838`, and both probes still read the open
    /// column `28` from `1892` outward. An actor walking in from further out
    /// gets frames where this gate says yes. What refuses the hop on those
    /// frames is the height band: on approach the floor ahead is the floor
    /// underfoot, so the rise is `0`, inside the dead band. That only holds
    /// if the rise is measured from the actor's **footing** - see
    /// [`Self::field_actor_footing`], which is where retail's floor-glued
    /// `+0x16` comes from. Since the arc runs no collision, one frame that
    /// passes both gates in error puts the player inside the wall.
    ///
    /// The same two points are then cleared through the actor/prop sweep
    /// `FUN_801cfc40` ([`Self::field_actor_point_blocked`], `0x801D1A8C` /
    /// `0x801D1AB0`) before the floor is sampled. That routine takes one
    /// point per call, so the hop gets retail's exact points rather than the
    /// walk controller's compass footprint.
    pub fn try_field_ledge_hop(&mut self, slot: usize) -> bool {
        if slot >= self.actors.len() || !self.actors[slot].active {
            return false;
        }
        let (dx, dz) = self.locomotion.step_delta;
        if dx == 0 && dz == 0 {
            return false;
        }
        // retail `s1 = dx << 2` / `s0 = dz << 2`
        let sx = dx as i32 * 4;
        let sz = dz as i32 * 4;
        let (x, z) = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x as i32, ms.world_z as i32)
        };
        let clamp = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        // Near (2x) then far (3x); either wall refuses the hop.
        for mul in [2, 3] {
            let px = clamp(x + mul * sx);
            let pz = clamp(z + mul * sz);
            if self.field_tile_is_wall(px, pz) {
                return false;
            }
        }
        // Actor / prop clearance at the SAME two points, in retail's order:
        // `FUN_801cfc40` at `2 * s` (`0x801D1A8C`) then at `3 * s`
        // (`0x801D1AB0`), each a bare non-zero refusing the hop.
        for mul in [2, 3] {
            if self.field_actor_point_blocked(x + mul * sx, z + mul * sz) {
                return false;
            }
        }
        // Retail samples the floor one step-delta ahead by temporarily
        // moving the actor, calling `FUN_80019278`, and restoring the
        // position; the engine's sampler takes the point directly.
        let probe_y = self.sample_field_floor_height(clamp(x + sx) as i32, clamp(z + sz) as i32);
        let rise = probe_y - self.field_actor_footing(slot, x, z);
        // World Y grows downward, so the *numerically* larger floor ahead is
        // the lower one: retail's `>= +0x61` arm is the drop (apex `0x10`),
        // its `< -0x60` arm the step up (apex `0x18`, the taller arc that
        // clears the lip). `0x801D1B14..0x801D1B44`.
        let kind = if rise >= FIELD_HOP_DOWN_THRESHOLD_DOWNWARD {
            0x10
        } else if rise < FIELD_HOP_UP_THRESHOLD_DOWNWARD {
            0x18
        } else {
            return false; // flat ground - nothing to hop
        };
        self.start_field_ledge_hop(
            slot,
            (clamp(x + 3 * sx), clamp(probe_y), clamp(z + 3 * sz)),
            kind,
        );
        true
    }

    /// Arc setup: retail `FUN_801d2404` (field overlay `overlay_0897`, 122
    /// instructions at file offset `0x3BEC`), the sole `jal` target of
    /// [`Self::try_field_ledge_hop`]'s tail.
    ///
    /// PORT: FUN_801d2404
    ///
    /// Retail spawns two pool actors here - the arc helper carrying the
    /// Bezier and the paired helper carrying the phase clip - and raises the
    /// player's movement-lock bit `+0x10 & 0x80000` so the walk controller
    /// and the vertical settle both yield for the flight. The engine has no
    /// actor pool, so the two clips are stored on the world's
    /// [`crate::world::FieldLocomotion::ledge_hop`] session instead; everything else is the
    /// retail body, including the arithmetic, which lives in
    /// [`legaia_engine_vm::field_ledge_hop_arc::build_hop_arc`].
    ///
    /// `apex` is the class byte the classifier picked - retail passes it
    /// straight through as `a1` - and the clip is always `0x10` frames
    /// (`a2`).
    pub(super) fn start_field_ledge_hop(
        &mut self,
        slot: usize,
        target: (i16, i16, i16),
        apex: u16,
    ) {
        let start = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x, ms.world_y, ms.world_z)
        };
        let arc = hop_arc::build_hop_arc(
            start,
            hop_arc::HopTarget {
                x: target.0,
                y: target.1,
                z: target.2,
            },
            apex as i16,
            FIELD_HOP_CLIP_FRAMES,
        );
        // The player cannot steer mid-hop: retail's `0x801D25A8..0x801D25B8`
        // ORs `0x80000` into the player context's `+0x10`, and the phase
        // machine's end arm is the only thing that clears it again.
        self.actors[slot].move_state.flags |= 0x0008_0000;
        self.locomotion.ledge_hop = Some(FieldLedgeHop {
            target_x: target.0,
            target_y: target.1,
            target_z: target.2,
            kind: apex,
            arc,
            phase: hop_arc::HopSession {
                cursor: 0,
                extent: FIELD_HOP_CLIP_FRAMES,
            },
            landed: false,
            finished: false,
            sfx: None,
        });
    }

    /// Advance a live hop one frame - the pair of pool-actor ticks retail
    /// runs off the two helper templates the setup allocated:
    ///
    /// * the **arc** helper's `FUN_801d5c08` (template `0x801F227C`), which
    ///   steps `+0x9C` by `+0x9E * DAT_1F800393`, evaluates the Bezier and
    ///   writes the result into the parent actor's `+0x14 / +0x16 / +0x18` -
    ///   the parent being the player, back-linked at setup;
    /// * the **paired** helper's `FUN_801d2298` (template `0x801F2294`), the
    ///   phase machine: take-off cue `0x2A` plus the airborne flag
    ///   `0x200000` on the zero frame, the flag drop as the cursor crosses
    ///   the extent, and at `extent + 6` the movement-lock release plus the
    ///   landing cue `0x29`.
    ///
    /// Returns `true` while a session owns the frame - the caller must not
    /// then settle or re-probe, exactly as retail's `0x80000` gate makes
    /// `FUN_801d1ba0` yield. A finished session is reaped on the next call
    /// rather than immediately, so the landing frame's cue and position stay
    /// readable for the frame they happen on.
    ///
    /// PORT: FUN_801d5c08
    /// REF: FUN_801d2298, FUN_801e45bc
    pub(super) fn tick_field_ledge_hop(&mut self, slot: usize) -> bool {
        let Some(mut hop) = self.locomotion.ledge_hop else {
            return false;
        };
        if hop.finished {
            self.locomotion.ledge_hop = None;
            return false;
        }
        hop.sfx = None;
        // `DAT_1F800393`, the frame-delta scalar both ticks pace on. The arc
        // multiplies it into the cursor step; the phase machine adds it raw.
        let scalar = self.move_vm.ramp_ratio.max(1);
        if !hop.landed {
            let tick = hop_arc::advance_hop_arc(&mut hop.arc, scalar);
            let ms = &mut self.actors[slot].move_state;
            ms.world_x = tick.position.0;
            ms.world_y = tick.position.1;
            ms.world_z = tick.position.2;
            hop.landed = tick.arrived;
        }
        let phase = hop_arc::advance_hop_session(&mut hop.phase, scalar);
        hop.sfx = phase.sfx;
        // The phase machine's clip-base stamps (`6` take-off, `7` landing,
        // `1` tear-down) - the hop and land clips the settle tail picks.
        if let Some(base) = phase.phase {
            self.locomotion.clip_base = base as u16;
        }
        let ms = &mut self.actors[slot].move_state;
        ms.flags |= phase.player_flags_set;
        ms.flags &= !phase.player_flags_clear;
        // `+0x62` bit 8 - retail's anim-active marker, raised at take-off and
        // at the crossing, cleared by the end arm. `local_flags` is that
        // half-word.
        ms.local_flags |= phase.player_anim_set;
        ms.local_flags &= !phase.player_anim_clear;
        hop.finished = phase.finished;
        self.locomotion.ledge_hop = Some(hop);
        true
    }

    /// Per-frame vertical settle + ledge-hop trigger: retail `FUN_801d1ba0`
    /// (field overlay PROT 0897, `0x801D1BA0..0x801D1EC0` - two hundred and
    /// one instructions, file offset `0x3388`).
    ///
    /// PORT: FUN_801d1ba0 (the glide, the hop gate and the anim-clip tail)
    /// REF: FUN_80019278, FUN_801d1878, FUN_801d1ec4, FUN_800204F8
    ///
    /// Glides the actor's height toward the floor beneath it at a
    /// frame-rate-scaled rate, then - when the frame is free to hop and the
    /// actor walked - asks [`Self::try_field_ledge_hop`] whether the step
    /// ahead is a ledge.
    ///
    /// Retail's gates (`0x801d1bb4..0x801d1d78`) decide the **hop**, not the
    /// glide:
    ///
    /// - `+0x10 & 0x80000` (the movement lock [`Self::step_field_locomotion`]
    ///   honours) or scratchpad `_DAT_1F800394 & 0x400` sends the routine to
    ///   `0x801D1CC8`, which still glides (and first promotes a `+0x9E` of `0`
    ///   to the grounded `0x10`) but never hops. An earlier reading here had
    ///   the lock skip the settle outright; it only skips the hop.
    /// - `+0x9e` neither `0` nor `0x10`: no glide and no hop (mid-hop, or a
    ///   scripted motion owns the actor). The engine's player carries no
    ///   `+0x9E`, so it is treated as grounded.
    /// - The hop additionally needs `_DAT_8007B6B0 <= 0` (the kind-0 warp
    ///   timer) and `_DAT_8007B6B4 == 0` (the post-warp hold)
    ///   (`0x801D1C6C..0x801D1C88`), both in
    ///   [`crate::world::FieldLocomotion::warp`], and no dialogue owning the
    ///   input.
    ///
    /// Retail then runs a tail on every frame that did not start a hop:
    /// `jal 0x801D1EC4` (the walk-on dispatcher, which the scene host runs),
    /// and an anim-clip pick into `+0x5C` from the clip base `_DAT_8007BDD8`
    /// before `FUN_800204F8` - [`Self::field_settle_clip_tail`]. The base's
    /// main writer is the pad step itself (`FUN_801D01B0` at
    /// `0x801D0424..0x801D04A4`); `FUN_801D1EC4` writes it on one arm only.
    ///
    /// The glide rate is `delta_scalar * 12`, halved when `+0x10 & 0x2000`
    /// is set (retail `sra s0, 1` - the slow-fall class). The height step is
    /// clamped to `+-rate`, so a tall drop takes several frames rather than
    /// snapping; that clamp is the whole reason this is a controller and not
    /// a one-line assignment.
    ///
    /// This is the retail sibling of [`crate::world::FieldLocomotion::follow_terrain_height`], which
    /// snaps instead of gliding. The snap stays authoritative when set, and
    /// the glide runs only behind its own opt-in
    /// [`crate::world::FieldLocomotion::vertical_settle`] - the engine's default is that Y is
    /// left untouched, an invariant the locomotion oracles pin, so the
    /// retail glide cannot become the default without rewriting them.
    ///
    /// The **ledge-hop trigger below is not gated on any of that**: it runs
    /// off the step delta on every field frame, which is what makes this
    /// controller worth having wired even at the engine's flat-Y default.
    ///
    /// A hop already in flight takes the frame instead. Retail reaches the
    /// same place by a different route - the hop's own `0x80000` movement
    /// lock trips this function's first gate while the two helper actors tick
    /// out of the pool - but the engine has no pool, so the hop tick runs
    /// from here, ahead of that gate.
    pub fn step_field_vertical(&mut self, slot: usize) {
        if slot >= self.actors.len() || !self.actors[slot].active {
            self.locomotion.ledge_hop = None;
            return;
        }
        if self.tick_field_ledge_hop(slot) {
            // Retail's hop holds the movement lock, which sends the settle to
            // its no-hop arm - and that arm still ends in the clip tail.
            self.field_settle_clip_tail();
            return;
        }
        // A scripted arc (op `0x43` sub-0/1/A/B) owns the player's height
        // until it lands - retail's arc helper writes `+0x16` every frame.
        if self.player_script_arc_live() {
            return;
        }
        let flags = self.actors[slot].move_state.flags;
        // The movement lock routes to the no-hop glide arm (`0x801D1CC8`).
        let hop_allowed = flags & 0x0008_0000 == 0 && !self.dialogue_owns_input();
        // Retail rate: `delta_scalar * 3 << 2`, halved for the `0x2000`
        // slow-fall class.
        let scalar = self.move_vm.ramp_ratio.max(1) as i32;
        let mut rate = scalar * 12;
        if flags & 0x2000 != 0 {
            rate >>= 1;
        }
        // The `+0x8E` mirror arm comes first and jumps past both ground arms
        // - see [`Self::field_actor_mirrored_y`]. A scripted eased move that
        // carries the flag holds the actor's Y outright; no glide, no sample.
        let ladder_moved = self.terrain.floor_height_lut != self.locomotion.ladder_seen;
        self.locomotion.ladder_seen = self.terrain.floor_height_lut;
        if let Some(mirror) = self.field_actor_mirrored_y(slot) {
            self.actors[slot].move_state.world_y = mirror;
        } else if self.locomotion.follow_terrain_height && ladder_moved {
            // The snap's standing-still half: the field VM moved the floor
            // ladder under the player (op `0x4C` nibble 9), so the footing
            // follows it - what retail's glide converges to, at a per-frame
            // wave step well inside the glide's rate.
            let (x, z) = {
                let ms = &self.actors[slot].move_state;
                (ms.world_x as i32, ms.world_z as i32)
            };
            let floor = self.sample_field_floor_height(x, z);
            self.actors[slot].move_state.world_y =
                floor.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        } else if self.locomotion.vertical_settle && !self.locomotion.follow_terrain_height {
            let (x, z, y) = {
                let ms = &self.actors[slot].move_state;
                (ms.world_x as i32, ms.world_z as i32, ms.world_y as i32)
            };
            let floor = self.sample_field_floor_height(x, z);
            let step = (floor - y).clamp(-rate, rate);
            if step != 0 {
                let ny = (y + step).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                self.actors[slot].move_state.world_y = ny;
            }
        }
        // Retail gates the hop on the step-delta pair being non-zero - i.e.
        // the actor actually walked this frame - and on the warp pair: no
        // warp in flight, no post-warp hold (`0x801D1C6C..0x801D1C88`).
        let (dx, dz) = self.locomotion.step_delta;
        let warp_quiet = !vm::field_warp_tile::pad_suppressed(&self.locomotion.warp);
        if hop_allowed && warp_quiet && (dx != 0 || dz != 0) && self.try_field_ledge_hop(slot) {
            // A frame that starts a hop returns before the tail
            // (`j 0x801D1EB0` at `0x801D1CC0`).
            return;
        }
        self.field_settle_clip_tail();
    }

    /// The system channel's per-tick clip-base reset: `FUN_80039B7C` stores
    /// the idle base `2` into `_DAT_8007BDD8` (`0x80039D90..0x80039D94`) on
    /// the arm its actor's `+0x9C == 0` and the scene control block's `+0xA`
    /// counter `< 2` select. Only the base is written; the party-bank bit is
    /// untouched.
    ///
    /// REF: FUN_80039B7C (the idle-base store of its `+0x9C == 0` arm, run
    /// for the system channel `0x8007E694`; the rest of the routine is the
    /// per-actor script / dialogue stepper)
    pub(crate) fn field_system_channel_clip_reset(&mut self) {
        self.locomotion.clip_base = vm::field_player_clip::BASE_IDLE;
    }

    /// Whether the field player is **movement-locked** at the point of the
    /// tick where retail's system channel runs - the gate its call site
    /// reads. `FUN_801DA51C` calls `FUN_80039B7C` for the system channel
    /// (`jal` at `0x801DA7BC`) only when the channel's own `+0x10 & 0x100`
    /// (script running) is up or the player's `+0x10 & 0x80000` is clear
    /// (`0x801DA78C..0x801DA7AC`); an interaction that starts raises that bit
    /// on the player (`0x80039DB8..0x80039DD4`) and holds it for its length.
    ///
    /// The port has no single player lock word for every retail holder, so
    /// the predicate names each one it models: the lock bit itself, an open
    /// dialogue, a cutscene timeline, the tile board, a ledge hop and a
    /// scripted arc. A kind-0 warp is not among them - the warp clears the
    /// lock when it arms.
    pub(crate) fn field_player_movement_locked(&self) -> bool {
        let Some(slot) = self.player_actor_slot else {
            return true;
        };
        let Some(actor) = self.actors.get(slot as usize) else {
            return true;
        };
        !actor.active
            || actor.move_state.flags & 0x0008_0000 != 0
            || self.dialogue_owns_input()
            || self.script_context_engages_player()
            || self.board.grid.is_some()
            || self.locomotion.ledge_hop.is_some()
            || self.player_script_arc_live()
    }

    /// One tick of the system channel's clip-base reset, run after the
    /// player's settle has read the base - the order retail's frame runs
    /// them in (the settle's reads at `0x801D1D8C` / `0x801D1E08`, then the
    /// store at `0x80039D94` from `FUN_801DA51C`).
    ///
    /// It is skipped while the player is movement-locked
    /// ([`Self::field_player_movement_locked`]): retail does not call the
    /// system channel then, and even when the channel's script is running
    /// the scene control block's `+0xA` interaction count is `2` or more
    /// through a conversation, which closes the store's own gate. So the
    /// reset reaches the settle on exactly the ticks the pad controller is
    /// skipped without a lock - a kind-0 warp - and, one tick earlier, on the
    /// tick a conversation opens: that tick's store is what the settle binds
    /// for the whole conversation, so a player who opened it running or
    /// walking stands idle through it rather than running in place.
    ///
    /// Retail capture (`s4_rimelm_door_transition`, Down + Cross into the
    /// `P1[16]` talk): the pad step stores run `3` at `0x801D0498` and the
    /// system channel `2` at `0x80039D94` on the talk's first tick; the
    /// player's clip id reads `3` for that tick and `2` for every later one,
    /// with no further store to the base while the box is open and the
    /// count at `2`. On the first page of the same conversation, a poke of
    /// the base to walk `1` is bound by the next settle and kept - no store
    /// arrives to undo it.
    ///
    /// REF: FUN_801DA51C (the system-channel call and its lock gate),
    /// FUN_80039B7C (the store)
    pub(crate) fn tick_field_system_channel_clip_reset(&mut self) {
        if self.field_player_movement_locked() {
            return;
        }
        self.field_system_channel_clip_reset();
    }

    /// The anim-clip tail of the settle (`FUN_801D1BA0` at
    /// `0x801D1D88..0x801D1EAC`): stride the clip base into the leader's
    /// bank, store the player's clip id, and hand the picked clip to the
    /// player's clip player - a party-bank slot, or a scene-bank record when
    /// the pick binds from the scene's own bundle (the op-`4C CE` override
    /// [`crate::world::FieldLocomotion::clip_override`], the `99` scene
    /// sentinel, or a player whose party-bank bit is down).
    ///
    /// Scratchpad `0x1F800394 & 0x400` (the bind block) has no writer the
    /// port models, so the bind always runs.
    ///
    /// REF: FUN_801D1BA0 (the tail is ported as
    /// [`vm::field_player_clip::settle_clip_pick`])
    pub(crate) fn field_settle_clip_tail(&mut self) {
        let leader = self.locomotion.player_anim.as_ref().map_or(0, |a| a.leader);
        let pick = vm::field_player_clip::settle_clip_pick(
            self.locomotion.clip_base,
            leader,
            self.locomotion.clip_override,
            self.locomotion.player_party_bank,
            false,
        );
        self.apply_player_clip_pick(&pick, leader);
    }

    /// A clip id written straight into the player's `+0x5C` as
    /// `base + leader * 7`, with the base stored alongside, then bound through
    /// the selector with whatever party-bank bit the player carries - the
    /// shape the tile board's walker uses (`0x801EF998..0x801EF9D0` for the
    /// run base `3` on an accepted step, `0x801EFAC0..0x801EFAEC` for the idle
    /// base `2` on arrival). Unlike the settle's pick it reads neither the
    /// `4C CE` override nor the `99` sentinel.
    ///
    /// REF: FUN_801EF2B0 (the two walker clip stores), FUN_800204F8
    pub(crate) fn field_player_strided_clip(&mut self, base: u16) {
        let leader = self.locomotion.player_anim.as_ref().map_or(0, |a| a.leader);
        self.locomotion.clip_base = base;
        let flag = self.locomotion.player_party_bank;
        let pick = vm::field_player_clip::SettleClipPick {
            clip: base.wrapping_add(leader.wrapping_mul(vm::field_player_clip::BANK_STRIDE)),
            party_flag: flag,
            binds: true,
            bind_party_flag: flag,
        };
        self.apply_player_clip_pick(&pick, leader);
    }

    /// Store one clip pick on the player and hand what it binds to the clip
    /// player. Shared by the settle tail and the script arms that aim a clip
    /// at the player ([`Self::field_player_script_clip`]).
    pub(super) fn apply_player_clip_pick(
        &mut self,
        pick: &vm::field_player_clip::SettleClipPick,
        leader: u16,
    ) {
        self.locomotion.player_clip = pick.clip as i16;
        self.locomotion.player_party_bank = pick.party_flag;
        if !pick.binds {
            return;
        }
        let slot = vm::field_player_clip::party_bank_slot(pick, leader);
        let scene = match pick.bound() {
            Some((vm::field_player_clip::ClipBank::Scene, record)) => Some(record),
            _ => None,
        };
        if let Some(anim) = &mut self.locomotion.player_anim {
            anim.select_retail_slot(slot);
            anim.select_scene_record(scene);
        }
    }

    /// A script writing a flag bit of the **player** object: `B1 F8 <bit>` /
    /// `B2 F8 <bit>` (op `0x31` `CFLAG_SET` / `0x32` `CFLAG_CLR` with the
    /// extended target `0xF8`). Retail's extended prologue resolves `0xF8`
    /// through `FUN_8003C83C` to the player object `_DAT_8007C364`, so the
    /// arm's `ctx[+0x10] |= / &= ~(1 << bit)` lands on the player's `+0x10`
    /// word rather than on the calling script's context.
    ///
    /// Bit `24` (`0x01000000`) is the player's **party-bank** bit
    /// ([`vm::field_player_clip::PARTY_BANK_FLAG`]), which the settle tail's
    /// clip pick reads: raised, the clip base strides into the leader's
    /// locomotion bank; dropped, it binds a scene-bank record instead. The
    /// port keeps that bit as [`crate::world::FieldLocomotion::player_party_bank`],
    /// so this routes it there and reports the op handled. Every other bit
    /// returns `false` and stays on the caller's context, because the port
    /// has no single player `+0x10` word those readers consult.
    ///
    /// REF: FUN_801DE840 (ops `0x31` / `0x32`), FUN_8003C83C (the `0xF8` arm)
    pub fn field_player_cflag(&mut self, bit: u8, set: bool) -> bool {
        if 1u32 << (bit & 0x1F) != vm::field_player_clip::PARTY_BANK_FLAG {
            return false;
        }
        self.locomotion.player_party_bank = set;
        true
    }

    /// A script re-staging the **player's** model: op `4C 50` with the
    /// extended target `0xF8` (`CC F8 50 lo hi`). Retail's arm writes the
    /// resolved actor, which `0xF8` makes the player object: `value >= 0xF0`
    /// raises its party-bank bit `+0x10 & 0x01000000`, any other value clears
    /// it (`0x801E17AC..0x801E1824`), and `FUN_80024E08` zeroes its clip id
    /// `+0x5C` and re-stages its mesh from pool slot `value`.
    ///
    /// The bit is [`crate::world::FieldLocomotion::player_party_bank`], which
    /// the next settle's pick reads - so `jagaroom`'s `CC F8 50 26 00` drops
    /// the player off its party bank until the `B1 F8 18` that follows puts
    /// it back, and `urudre1`'s `CC F8 50 5D 00` leaves it down for the
    /// scene. The model id lands on
    /// [`crate::world::FieldLocomotion::player_live_model`] for the hosts'
    /// player mesh: a change raises
    /// [`crate::world::FieldLocomotion::player_rig_dirty`], and both play
    /// hosts drain it ([`Self::take_player_rig_change`]) and rebuild the rig
    /// from [`crate::scene::SceneHost::player_rig_mesh`].
    ///
    /// REF: FUN_80024E08 (the re-stage; the port for a placement is
    /// `FieldHostImpl::op4c_n5_sub0_set_actor_model`), FUN_8003C83C
    pub fn field_player_set_model(&mut self, value: i16) -> bool {
        self.locomotion.player_party_bank = i32::from(value) >= 0xF0;
        self.locomotion.player_clip = 0;
        if self.locomotion.player_live_model != Some(value) {
            self.locomotion.player_rig_dirty = true;
        }
        self.locomotion.player_live_model = Some(value);
        true
    }

    /// Take the "the player's model changed" signal a mid-scene `CC F8 50`
    /// raises. A host that sees `true` rebuilds the player's rig from
    /// [`crate::scene::SceneHost::player_rig_mesh`].
    pub fn take_player_rig_change(&mut self) -> bool {
        std::mem::take(&mut self.locomotion.player_rig_dirty)
    }

    /// The scene-record one-shot a script ExecMove on the player
    /// (`A2 F8 <id>`) queues for the hosts to draw over idle / walk: the
    /// picked clip id, when the pick binds the **scene** bank. `None` with the
    /// party-bank bit up - the id then strides into the leader's locomotion
    /// bank (`FUN_800204F8`'s party arm), which the settle tail's slot pick
    /// plays; queueing scene record `id - 1` there played a clip of another
    /// skeleton over the hero, and the body came apart. Clips `1` / `2` (the
    /// walk / idle moves) never queue.
    ///
    /// REF: FUN_800204F8 (`0x80020524..0x800205A8`, the bank select)
    pub(crate) fn player_move_cue(
        &self,
        pick: &vm::field_player_clip::SettleClipPick,
    ) -> Option<u8> {
        let Some((vm::field_player_clip::ClipBank::Scene, _)) = pick.bound() else {
            return None;
        };
        u8::try_from(pick.clip).ok().filter(|&id| id > 2)
    }

    /// A script aiming a clip at the **player**: op `0x22` `EXEC_MOVE`
    /// (`0x801DE998..0x801DEAB8`) and the player arm of op `4C 51`
    /// (`0x801E1954..0x801E1A3C`) both test the context against the player
    /// pointer `_DAT_8007C364`, store their clip operand into the clip base
    /// `_DAT_8007BDD8`, and run the settle tail's pick on it at once -
    /// `99` names scene record `leader`, a party-flagged actor strides into
    /// the leader's bank (or, under the `4C CE` override, into the scene
    /// bank at the override), and an unflagged one binds scene record
    /// `move_id - 1`. Neither arm tests the settle's bind block, so the bind
    /// always runs.
    ///
    /// REF: FUN_801DE840 (the two player arms; the pick is
    /// [`vm::field_player_clip::settle_clip_pick`])
    pub fn field_player_script_clip(
        &mut self,
        move_id: u8,
    ) -> vm::field_player_clip::SettleClipPick {
        self.locomotion.clip_base = u16::from(move_id);
        let leader = self.locomotion.player_anim.as_ref().map_or(0, |a| a.leader);
        let pick = vm::field_player_clip::settle_clip_pick(
            self.locomotion.clip_base,
            leader,
            self.locomotion.clip_override,
            self.locomotion.player_party_bank,
            false,
        );
        self.apply_player_clip_pick(&pick, leader);
        pick
    }

    /// Advance actor `slot` by `speed` world units in the direction encoded by
    /// `dir_bits` (post-remap convention: `0x1000`=Z+, `0x4000`=Z-,
    /// `0x2000`=X+, `0x8000`=X-), stepping `FIELD_STEP_UNIT` at a time and
    /// committing only the axes that stay off a wall in
    /// [`crate::world::FieldTerrain::collision_grid`]. X collision uses the just-committed Z
    /// so a diagonal move can't tunnel through a wall corner.
    ///
    /// Shared by [`Self::step_field_locomotion`] and
    /// `Self::step_world_map_locomotion`: retail `FUN_801d01b0` is the same
    /// routine in both the field and world-map-walk overlays, and both collide
    /// against the same `_DAT_1f8003ec + 0x4000` walkability grid.
    ///
    /// With [`crate::world::FieldLocomotion::leading_edge_wall_probes`] set, each axis instead blocks
    /// on retail's three-probe leading-edge footprint taken at the CURRENT
    /// position ([`Self::field_dir_blocked`]) - the retail standoff - and
    /// commits the step whenever the edge is clear. The default candidate-
    /// centre test is kept (off-flag) for the locomotion oracles and the
    /// BFS nav drivers. With [`crate::world::FieldNpcState::solid`] set, each axis takes
    /// its actor gate from the combined [`Self::field_actor_dir_blocked`]
    /// instead - both `FUN_801cfc40` entity classes in one test - so a field
    /// NPC's body box blocks the step as well: retail gates a step on the
    /// actor bits and the wall bit together (`FUN_801cfe4c` returning any of
    /// `1`/`2`/`4` refuses the 2-unit step).
    ///
    /// **Placed props block unconditionally** ([`Self::field_prop_dir_probe`]):
    /// retail's placed-object actors always sit in the collision candidate
    /// list (`FUN_801CF754`), so a closed door is solid until its touch pass
    /// runs `31 00`. A static-class prop hit also records the touched prop
    /// into [`crate::world::FieldPropState::pending_touch`] - the same probe both refuses the
    /// step and posts the touch (`FUN_801D01B0`'s bit-`4` auto-post of
    /// `FUN_801D5B5C`).
    pub fn advance_with_collision(&mut self, slot: usize, dir_bits: u16, speed: i32) {
        let edge = self.locomotion.leading_edge_wall_probes;
        let solid_npcs = self.npcs.solid;
        let mut remaining = speed;
        while remaining > 0 {
            let ms = &self.actors[slot].move_state;
            let (cx, cz) = (ms.world_x, ms.world_z);
            // Z axis.
            if dir_bits & 0x1000 != 0 {
                let nz = cz.saturating_add(FIELD_STEP_UNIT as i16);
                let actors = self.probe_actors_for_step(cx, cz, 2, solid_npcs);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz, 2)
                } else {
                    self.field_tile_is_wall(cx, nz)
                } || actors;
                if !blocked {
                    self.actors[slot].move_state.world_z = nz;
                    self.locomotion.step_delta.1 = FIELD_PROBE_DELTA;
                }
            } else if dir_bits & 0x4000 != 0 {
                let nz = cz.saturating_sub(FIELD_STEP_UNIT as i16);
                let actors = self.probe_actors_for_step(cx, cz, 0, solid_npcs);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz, 0)
                } else {
                    self.field_tile_is_wall(cx, nz)
                } || actors;
                if !blocked {
                    self.actors[slot].move_state.world_z = nz;
                    self.locomotion.step_delta.1 = -FIELD_PROBE_DELTA;
                }
            }
            // X axis (re-read X in case Z committed; X collision uses the
            // committed Z so footprints don't tunnel diagonally).
            let cz2 = self.actors[slot].move_state.world_z;
            if dir_bits & 0x2000 != 0 {
                let nx = cx.saturating_add(FIELD_STEP_UNIT as i16);
                let actors = self.probe_actors_for_step(cx, cz2, 3, solid_npcs);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz2, 3)
                } else {
                    self.field_tile_is_wall(nx, cz2)
                } || actors;
                if !blocked {
                    self.actors[slot].move_state.world_x = nx;
                    self.locomotion.step_delta.0 = FIELD_PROBE_DELTA;
                }
            } else if dir_bits & 0x8000 != 0 {
                let nx = cx.saturating_sub(FIELD_STEP_UNIT as i16);
                let actors = self.probe_actors_for_step(cx, cz2, 1, solid_npcs);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz2, 1)
                } else {
                    self.field_tile_is_wall(nx, cz2)
                } || actors;
                if !blocked {
                    self.actors[slot].move_state.world_x = nx;
                    self.locomotion.step_delta.0 = -FIELD_PROBE_DELTA;
                }
            }
            remaining -= FIELD_STEP_UNIT;
        }
    }

    /// Advance actor `slot` by `speed` world units along the arbitrary
    /// ground-plane direction `(wx, wz)` - the [`crate::world::FieldLocomotion::precise_movement`]
    /// sibling of [`Self::advance_with_collision`]. The vector is
    /// normalised, split into per-axis distances, and walked in the same
    /// `FIELD_STEP_UNIT` sub-steps through the same per-axis collision
    /// probes (each sub-step is one single-axis `advance_with_collision`
    /// call, Z before X, so X collision sees the just-committed Z exactly
    /// like the quantised path). Sub-`FIELD_STEP_UNIT` remainders persist in
    /// [`crate::world::FieldLocomotion::precise_move_carry`] so shallow angles keep their exact
    /// slope across frames instead of rounding each frame's minor axis to
    /// zero.
    pub fn advance_with_collision_vector(&mut self, slot: usize, wx: f32, wz: f32, speed: i32) {
        let len = (wx * wx + wz * wz).sqrt();
        if len <= f32::EPSILON || speed <= 0 {
            return;
        }
        let step = FIELD_STEP_UNIT as f32;
        let mut ax = self.locomotion.precise_move_carry.0 + wx / len * speed as f32;
        let mut az = self.locomotion.precise_move_carry.1 + wz / len * speed as f32;
        while ax.abs() >= step || az.abs() >= step {
            if az.abs() >= step {
                let bit = if az > 0.0 { 0x1000 } else { 0x4000 };
                self.advance_with_collision(slot, bit, FIELD_STEP_UNIT);
                az -= step * az.signum();
            }
            if ax.abs() >= step {
                let bit = if ax > 0.0 { 0x2000 } else { 0x8000 };
                self.advance_with_collision(slot, bit, FIELD_STEP_UNIT);
                ax -= step * ax.signum();
            }
        }
        self.locomotion.precise_move_carry = (ax, az);
    }

    /// One movement sub-step's **actor-collision** gate - the engine's
    /// analogue of the `FUN_801cfc40` calls `FUN_801cfe4c` makes before its
    /// wall probes, whose result bits `1` (moving actor) and `4` (static
    /// entity) refuse the 2-unit step exactly like the wall bit `2`.
    ///
    /// REF: FUN_801cfc40, FUN_801cfe4c
    ///
    /// Always runs the prop half first, because that arm is the one that
    /// latches the static-class touch anchor
    /// ([`Self::probe_props_for_step`]) the locomotion auto-posts, and
    /// because placed props are solid unconditionally (see
    /// [`Self::advance_with_collision`]). When `solid_npcs` is set the gate
    /// takes retail's **combined** answer through
    /// [`Self::field_actor_dir_blocked`] - both entity classes in one test,
    /// the shape `FUN_801cfe4c` consumes - so a village NPC blocks the step
    /// as well. The two forms agree by construction
    /// (`field_actor_dir_blocked` is `field_npc_dir_blocked ||
    /// field_prop_dir_probe(..).blocked`), so the flag only ever adds the
    /// moving-actor arm.
    pub(super) fn probe_actors_for_step(
        &mut self,
        x: i16,
        z: i16,
        dir: usize,
        solid_npcs: bool,
    ) -> bool {
        let prop = self.probe_props_for_step(x, z, dir);
        prop || (solid_npcs && self.field_actor_dir_blocked(x, z, dir))
    }

    /// One movement sub-step's prop probe: blocks on any solid prop box hit
    /// and latches a static-class touch into [`crate::world::FieldPropState::pending_touch`]
    /// (drained by [`Self::tick_prop_interactions`]). Returns whether the
    /// step is prop-blocked.
    pub(super) fn probe_props_for_step(&mut self, x: i16, z: i16, dir: usize) -> bool {
        let probe = self.field_prop_dir_probe(x, z, dir);
        if let Some(anchor) = probe.touch
            && self.props.pending_touch.is_none()
        {
            self.props.pending_touch = Some(anchor);
        }
        probe.blocked
    }

    /// Step the player one navigation frame toward world position `(tx, tz)`,
    /// using the same per-axis field collision as pad locomotion
    /// ([`Self::advance_with_collision`]) but a world-space direction. Returns
    /// `true` once the player is within `tol` units of the target on both axes.
    ///
    /// This is the auto-navigation primitive a driver loops (following a path of
    /// waypoints) to walk the player to a target - e.g. the v0.1 oracle walking
    /// from the cold-boot spawn to the sparring partner before talking to it.
    /// It drives the real locomotion stepping/collision, just without the pad →
    /// camera-relative remap. No-op without an active player actor.
    pub fn nav_step_toward(&mut self, tx: i16, tz: i16, tol: i16) -> bool {
        let Some(slot) = self.player_actor_slot else {
            return false;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return false;
        }
        let (cx, cz) = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x, ms.world_z)
        };
        if (cx - tx).abs() <= tol && (cz - tz).abs() <= tol {
            return true;
        }
        let mut dir = 0u16;
        let (mut wx, mut wz) = (0i32, 0i32);
        if tz > cz {
            dir |= 0x1000; // Z+
            wz = 1;
        } else if tz < cz {
            dir |= 0x4000; // Z-
            wz = -1;
        }
        if tx > cx {
            dir |= 0x2000; // X+
            wx = 1;
        } else if tx < cx {
            dir |= 0x8000; // X-
            wx = -1;
        }
        if dir != 0 {
            self.locomotion.last_move_dir_bits = dir;
            // Walking sets the heading, exactly as the pad path does (retail
            // locomotion writes the facing every moved frame) - so a nav walk
            // leaves the player facing its travel direction and the interact
            // probe ([`Self::field_interact_probe_slot`]) sees the same state
            // a pad walk would produce.
            self.actors[slot].move_state.render_26 =
                (((wx as f32).atan2(wz as f32) / std::f32::consts::TAU * 4096.0).round() as i32
                    & 0x0FFF) as i16;
            // A nav step is a movement frame for the locomotion animation,
            // same as a held pad direction.
            if let Some(anim) = &mut self.locomotion.player_anim {
                anim.moved_this_frame = true;
            }
            self.advance_with_collision(slot, dir, FIELD_BASE_STEP);
        }
        false
    }
}

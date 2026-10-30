//! The action-movement enhancement on the field player: the platformer
//! controller of [`legaia_engine_field::action_movement`] in place of the
//! retail walk and its settle, behind [`crate::world::WorldToggles::action_movement`].

use super::*;
use legaia_engine_field::action_movement::{
    ActionAnim, ActionBody, ActionInput, ActionStep, ActionTerrain, STICK_DEADZONE, STICK_FULL,
};

/// The action model's jump button. Square: the field reads it only as the
/// port's alternate run button ([`crate::world::config::FIELD_RUN_BUTTON_MASK_DEFAULT`]),
/// which the action model drops from its sprint mask. Cross / L1 still talk,
/// Circle / L2 still cancel and Triangle still opens the menu.
pub const ACTION_JUMP_BUTTON: input::PadButton = input::PadButton::Square;

/// The action model's dash button. R2: the field reads it for nothing.
pub const ACTION_DASH_BUTTON: input::PadButton = input::PadButton::R2;

/// [`ActionTerrain`] over the live world: the walk grid's wall bits, the
/// placed-prop and NPC body probes, and `FUN_80019278`'s floor sample.
struct WorldTerrain<'a> {
    world: &'a mut World,
}

impl ActionTerrain for WorldTerrain<'_> {
    fn step_blocked(&mut self, x: i16, z: i16, dir: usize) -> bool {
        self.world.field_unit_step_blocked(x, z, dir)
    }

    fn floor_y(&mut self, x: i32, z: i32) -> i32 {
        self.world.sample_field_floor_height(x, z)
    }
}

impl World {
    /// Whether this frame's player step ran the action model.
    pub fn action_movement_drove(&self) -> bool {
        self.locomotion.action_drove
    }

    /// The action model's body, once it has driven the player.
    pub fn action_movement_body(&self) -> Option<&ActionBody> {
        self.locomotion.action_body.as_ref()
    }

    /// The action model's last step.
    pub fn action_movement_last_step(&self) -> Option<&ActionStep> {
        self.locomotion.action_last.as_ref()
    }

    /// Whether a one-unit step from `(x, z)` along probe direction `dir`
    /// (`0` Z-, `1` X-, `2` Z+, `3` X+) is refused - one axis of
    /// [`Self::advance_with_collision`] at a one-unit stride: the
    /// leading-edge footprint or the candidate-centre wall test, then the
    /// prop and (when solid) NPC probes, and nothing under the
    /// walk-through-walls cheat. A touched prop is latched as it is there.
    pub(crate) fn field_unit_step_blocked(&mut self, x: i16, z: i16, dir: usize) -> bool {
        let solid_npcs = self.npcs.solid;
        let actors = self.probe_actors_for_step(x, z, dir, solid_npcs);
        if self.locomotion.walk_through_walls {
            return false;
        }
        let wall = if self.locomotion.leading_edge_wall_probes {
            self.field_dir_blocked(x, z, dir)
        } else {
            let (nx, nz) = match dir {
                0 => (x, z.saturating_sub(1)),
                1 => (x.saturating_sub(1), z),
                2 => (x, z.saturating_add(1)),
                _ => (x.saturating_add(1), z),
            };
            self.field_tile_is_wall(nx, nz)
        };
        wall || actors
    }

    /// Whether the action model may take the player this frame: the switch
    /// is on, the world is on a field map (not a kingdom map, whose walk is
    /// its own), and nothing scripted owns the player's height - a scripted
    /// arc, a ledge hop in flight or the `+0x8E` mirror. The pad-lock gates
    /// are [`Self::step_field_locomotion`]'s own and come first.
    pub(super) fn action_movement_applies(&self, slot: usize) -> bool {
        self.toggles.action_movement
            && self.mode == legaia_engine_mode::mode::SceneMode::Field
            && !self.party.scene_save_allowed
            && self.locomotion.ledge_hop.is_none()
            && !self.player_script_arc_live()
            && self.field_actor_mirrored_y(slot).is_none()
    }

    /// This frame's controls for the action model.
    fn action_movement_input(&self) -> ActionInput {
        let (lx, ly) = self.input.lstick();
        let (lx, ly) = (i32::from(lx), i32::from(ly));
        let stick = if lx * lx + ly * ly >= STICK_DEADZONE * STICK_DEADZONE {
            // Stick +Y is down (PSX); the kernel's screen +Y is up.
            (lx, -ly)
        } else {
            let axis = |pos: input::PadButton, neg: input::PadButton| match (
                self.input.pressed(pos),
                self.input.pressed(neg),
            ) {
                (true, false) => STICK_FULL,
                (false, true) => -STICK_FULL,
                _ => 0,
            };
            (
                axis(input::PadButton::Right, input::PadButton::Left),
                axis(input::PadButton::Up, input::PadButton::Down),
            )
        };
        // The run button inverts the Field Move option, as on the retail
        // walk, but without the jump button in its mask.
        let sprint_mask = self.locomotion.run_button_mask & !ACTION_JUMP_BUTTON.mask();
        let sprint_button = self.input.pad() & sprint_mask != 0;
        ActionInput {
            stick,
            camera_azimuth: self.locomotion.camera_azimuth,
            jump_held: self.input.pressed(ACTION_JUMP_BUTTON),
            jump_pressed: self.input.just_pressed(ACTION_JUMP_BUTTON),
            dash_pressed: self.input.just_pressed(ACTION_DASH_BUTTON),
            sprint_held: sprint_button != self.locomotion.run_default,
        }
    }

    /// One frame of the action model on the player in `slot`, in place of
    /// the retail walk below [`Self::step_field_locomotion`]'s gates. It
    /// owns all three coordinates and the facing, and keeps the walk's
    /// side effects: the direction mask and step delta the touch probes
    /// read, the walk-regen fill, the walk-touch and ambient-touch posts,
    /// and a clip base for the settle tail's pick - idle `2`, run `3`, and
    /// the ledge hop's airborne `6` / landing `7` for the air states.
    pub(super) fn step_field_action_movement(&mut self, slot: usize) {
        let (x, y, z, heading) = {
            let ms = &self.actors[slot].move_state;
            (
                i32::from(ms.world_x),
                i32::from(ms.world_y),
                i32::from(ms.world_z),
                ms.render_26 as u16,
            )
        };
        let mut body = self
            .locomotion
            .action_body
            .unwrap_or_else(|| ActionBody::at(x, y, z, heading));
        body.sync_to(x, y, z, heading);
        let input = self.action_movement_input();
        let params = self.locomotion.action_params;
        let step = body.step(&params, &input, &mut WorldTerrain { world: self });
        let (nx, ny, nz) = body.unit_pos();
        let clamp = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        {
            let ms = &mut self.actors[slot].move_state;
            ms.world_x = clamp(nx);
            ms.world_y = clamp(ny);
            ms.world_z = clamp(nz);
            ms.render_26 = step.heading as i16;
        }
        // Keep the body on the stored (clamped) coordinates so the next
        // frame's sync sees its own write.
        body.sync_to(
            i32::from(clamp(nx)),
            i32::from(clamp(ny)),
            i32::from(clamp(nz)),
            step.heading,
        );
        self.locomotion.action_body = Some(body);
        self.locomotion.action_last = Some(step);
        self.locomotion.action_drove = true;
        self.locomotion.last_move_dir_bits = step.dir_bits;
        let probe = |d: i32| match d.signum() {
            1 => FIELD_PROBE_DELTA,
            -1 => -FIELD_PROBE_DELTA,
            _ => 0,
        };
        self.locomotion.step_delta = (probe(nx - x), probe(nz - z));

        if step.jumped {
            self.push_action_sfx(hop_arc::hop_sfx::TAKE_OFF);
        }
        if step.landed {
            self.push_action_sfx(hop_arc::hop_sfx::LAND);
        }
        self.action_movement_clip(&step);

        if step.moved && body.grounded {
            self.locomotion.walk_regen_steps = self
                .locomotion
                .walk_regen_steps
                .saturating_add(self.clock.display_frame_step as i32);
        }
        if step.steering {
            self.check_field_walk_touch();
            self.post_ambient_motion_touch();
        }
    }

    fn push_action_sfx(&mut self, id: u8) {
        self.audio
            .sfx_ring_ops
            .push(legaia_engine_system::sound_state::SfxRingOp::WriteSlot(
                0,
                i16::from(id),
            ));
    }

    /// The clip base for the action model's state. Idle and run go through
    /// the retail pick (so the `_DAT_8007B6A8` scene sentinel and the
    /// non-positive-clip gate hold as they do for the walk); the air states
    /// take the ledge hop's two bases.
    fn action_movement_clip(&mut self, step: &ActionStep) {
        let moving = matches!(step.anim, ActionAnim::Run | ActionAnim::Dash);
        let dir = if moving { 0x1000 } else { 0 };
        let Some(base) = legaia_engine_field::field_player_clip::locomotion_clip_base(
            self.locomotion.player_clip,
            dir,
            crate::world::config::FIELD_BASE_STEP_RUN,
            self.party.scene_save_allowed,
        ) else {
            return;
        };
        let base = match step.anim {
            ActionAnim::Jump | ActionAnim::Fall => hop_arc::hop_phase::AIRBORNE as u16,
            ActionAnim::Land => hop_arc::hop_phase::LANDING as u16,
            _ => base,
        };
        self.locomotion.clip_base = base;
        self.locomotion.player_party_bank = true;
        self.locomotion.player_clip_rate = legaia_engine_field::field_anim::CLIP_RATE as i16;
        if let Some(anim) = &mut self.locomotion.player_anim {
            anim.pad_drove_this_frame = true;
            anim.moved_this_frame |= moving || step.moved;
        }
    }

    /// The action model's hand-back: a frame it did not drive leaves no
    /// body hanging in the air. An airborne body is set down on the floor
    /// under it (unless a script owns the height), and with the switch off
    /// the body is dropped so the next engagement starts from rest.
    pub(super) fn settle_action_body(&mut self, slot: usize) {
        if self.locomotion.action_drove {
            return;
        }
        let Some(body) = self.locomotion.action_body else {
            return;
        };
        let scripted = self.player_script_arc_live()
            || self.locomotion.ledge_hop.is_some()
            || self.field_actor_mirrored_y(slot).is_some();
        if body.airborne() && !scripted && slot < self.actors.len() {
            let (x, z) = {
                let ms = &self.actors[slot].move_state;
                (i32::from(ms.world_x), i32::from(ms.world_z))
            };
            let floor = self.sample_field_floor_height(x, z);
            self.actors[slot].move_state.world_y =
                floor.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
        if body.airborne() || !self.toggles.action_movement {
            self.locomotion.action_body = None;
        }
    }
}

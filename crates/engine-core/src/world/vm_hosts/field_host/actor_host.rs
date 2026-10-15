//! `FieldActorHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldActorHost for FieldHostImpl<'a> {
    // Op `43 07` (arm `0x801DF6F8`): write row `face_id` of `DAT_8007BE60`
    // (`+8` = the u32 at operand `+1`, `+0..+6` the four halfwords at
    // `+5..+0xC`), store the id in `+0x6D`, and ramp `+0x7A` with
    // `FUN_8003C5F0(+0x7A, 2, ..)`: `0 -> 0x1000` over `target` frames, or
    // `0x1000 -> 0` over `-target` (`0x801DF75C..0x801DF79C`).
    fn actor_face_rotation_setup(
        &mut self,
        ctx: &mut FieldCtx,
        face_id: u8,
        payload_4: u32,
        params: [u16; 4],
        target: i16,
    ) {
        self.world.npcs.face_rows.insert(
            face_id,
            crate::world::FieldFaceRow {
                offset: params.map(|v| v as i16),
                flags: payload_4,
            },
        );
        let Some(HeadingOwner::Placement(slot)) = self.world.heading_owner(ctx) else {
            return;
        };
        let (start, end, frames) = if target > 0 {
            (0, 0x1000, i32::from(target))
        } else {
            (0x1000, 0, -i32::from(target))
        };
        let face = self.world.npcs.faces.entry(slot).or_insert((face_id, 0));
        face.0 = face_id;
        use legaia_engine_motion_vm::ambient_motion::{Ramp, RampKind};
        self.world.npcs.face_ramps.install(Ramp {
            dest: 0x7A,
            owner: u32::from(slot),
            start,
            end,
            total: frames,
            remaining: frames,
            kind: RampKind::U16,
        });
    }

    // Op `43 08` (`0x801DF7AC`): `+0x6D = 0`, `+0x7A = 0` - the actor draws
    // undeformed again. A ramp still live keeps writing `+0x7A`, as retail's.
    fn actor_face_reset(&mut self, ctx: &mut FieldCtx) {
        if let Some(HeadingOwner::Placement(slot)) = self.world.heading_owner(ctx)
            && let Some(face) = self.world.npcs.faces.get_mut(&slot)
        {
            *face = (0, 0);
        }
    }

    // Op `4C C0` (arm `0x801E260C`): seat a placement's clip at a random
    // phase - see [`World::randomize_npc_clip_phase`].
    fn randomize_clip_phase(&mut self, ctx: &mut FieldCtx) -> bool {
        match self.world.heading_owner(ctx) {
            Some(HeadingOwner::Placement(slot)) => self.world.randomize_npc_clip_phase(slot),
            _ => false,
        }
    }

    fn face_compass(&mut self, ctx: &mut FieldCtx, index: u8, player: bool) {
        Self::face_compass(self, ctx, index, player)
    }

    /// Op `4C 45` with `ticks == 0`: the side buffer's look object and
    /// angles, written at once (`0x801E12BC..0x801E12F4`). See
    /// [`legaia_engine_field::actor_look`].
    ///
    /// PORT: FUN_801DE840 (the nibble-4 sub-5 immediate arm)
    fn write_actor_look(&mut self, ctx: &mut FieldCtx, b1: u8, w94: i16, w96: i16, w98: i16) {
        if let Some(key) = self.look_key(ctx) {
            self.world.npcs.looks.write(key, b1, [w94, w96, w98]);
        }
    }

    /// Op `4C 45` with `ticks != 0`: the object at once, one ramp per angle
    /// that changes (`0x801E12F8..0x801E138C`).
    ///
    /// PORT: FUN_801DE840 (the nibble-4 sub-5 ramp arm)
    fn ramp_actor_look(
        &mut self,
        ctx: &mut FieldCtx,
        b1: u8,
        w94: i16,
        w96: i16,
        w98: i16,
        ticks: u16,
    ) {
        if let Some(key) = self.look_key(ctx) {
            self.world.npcs.looks.ramp(key, b1, [w94, w96, w98], ticks);
        }
    }

    // `4C 48`'s immediate arm: the context's `+0x26` is the actor's drawn
    // heading. nilboa's Delilas pair (`P1[4]` / `P1[5]`) stand at `0x300`
    // from their spawn prologue's `4C 48 00 03 00 00`.
    // REF: FUN_801DE840 (nibble-4 sub-8)
    fn set_actor_heading(&mut self, ctx: &mut FieldCtx, value: u16) {
        match self.world.heading_owner(ctx) {
            Some(HeadingOwner::Player) => {
                if let Some(a) = self
                    .world
                    .player_actor_slot
                    .and_then(|s| self.world.actors.get_mut(usize::from(s)))
                {
                    a.move_state.render_26 = (value as i16).wrapping_add(0x800);
                }
            }
            Some(HeadingOwner::Placement(slot)) => {
                self.world.npcs.heading_ramps.free_owner(u32::from(slot));
                self.world
                    .npcs
                    .headings
                    .insert(slot, (value as i16).wrapping_add(0x800));
            }
            None => {}
        }
    }

    fn ramp_ctx_slot(&mut self, ctx: &mut FieldCtx, sub: u8, target: i16, ticks: u16) {
        Self::ramp_ctx_slot(self, ctx, sub, target, ticks)
    }

    fn tween_actor_position(&mut self, ctx: &mut FieldCtx, x: u16, y: u16, z: u16, ticks: u16) {
        Self::tween_actor_position(self, ctx, x, y, z, ticks)
    }

    fn setup_animation(&mut self, _ctx: &mut FieldCtx, count: u8, base_id: u8, frames: &[u8]) {
        // Op `0x4B` on a placement's own context: its VDF morph lanes
        // (`World::arm_npc_morph`), not a clip - the arm writes the `+0xB0`
        // sub-entry bytes and the `+0xB8` / `+0xC8` ramp velocities and
        // touches neither `+0x5C` nor the clip pointer.
        // PORT: FUN_801DE840 (the op-`0x4B` arm)
        let owner = match (
            self.world.field_vm.executing_channel,
            self.world.field_vm.executing_object,
        ) {
            (Some(slot), _) => Some(crate::world::MorphOwner::Placement(slot)),
            (None, Some(record)) => Some(crate::world::MorphOwner::Object(record)),
            (None, None) => None,
        };
        if let Some(owner) = owner {
            self.world.arm_field_morph(owner, count, base_id, frames);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::SetupAnimation {
                count,
                base_id,
                frames: frames.to_vec(),
            });
    }

    fn spawn_arc_jump(
        &mut self,
        _ctx: &FieldCtx,
        ext: Option<u8>,
        req: &legaia_engine_vm::field_ledge_hop_arc::ScriptArcRequest,
    ) {
        // The watcher's release context: the arced NPC's own channel, or -
        // for a player arc - the channel that ran the op, which retail halts
        // with the player (`0x801DF3F0..0x801DF408`).
        let release = self.world.field_vm.executing_channel;
        if let Some(actor) = self.world.resolve_script_actor(ext) {
            self.world.start_field_script_arc(actor, req, release);
        }
    }

    fn move_to(&mut self, ctx: &mut FieldCtx, world_x: u16, world_z: u16, is_player: bool) {
        Self::move_to(self, ctx, world_x, world_z, is_player)
    }

    fn exec_move(&mut self, ctx: &mut FieldCtx, move_id: u8) {
        Self::exec_move(self, ctx, move_id)
    }

    fn set_actor_model(&mut self, ctx: &mut FieldCtx, value: i16, _high: bool) {
        Self::set_actor_model(self, ctx, value, _high)
    }

    fn run_to_tile(
        &mut self,
        ctx: &mut FieldCtx,
        world_x: u16,
        world_z: u16,
        depth_byte: u8,
        move_id: u8,
        is_player: bool,
    ) {
        Self::run_to_tile(self, ctx, world_x, world_z, depth_byte, move_id, is_player)
    }

    fn script_table_teleport(&mut self, ctx: &mut FieldCtx) {
        Self::script_table_teleport(self, ctx)
    }
}

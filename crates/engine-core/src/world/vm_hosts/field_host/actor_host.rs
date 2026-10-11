//! `FieldActorHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldActorHost for FieldHostImpl<'a> {
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

//! `FieldActorFxHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldActorFxHost for FieldHostImpl<'a> {
    fn set_actor_tint(
        &mut self,
        ctx: &mut FieldCtx,
        target: Option<u8>,
        colour: u32,
        blend: u16,
        ticks: u16,
    ) {
        Self::set_actor_tint(self, ctx, target, colour, blend, ticks)
    }

    // `4C 86` / `4C 87` - the reflection controller's install and teardown,
    // consecutive slots of the nibble-8 sub-table.
    // REF: FUN_801E573C, FUN_8003CF40
    fn install_reflection(&mut self, ctx: &mut FieldCtx, source_id: u8, words: [i16; 6]) -> bool {
        // Retail's `a0` is the executing context itself - `FUN_801DE840`'s
        // third argument - so the script that issues the op is the mirror
        // image, and the operand byte names what it reflects.
        let ctx_is_player = self.ctx_is_player(ctx);
        self.world
            .spawn_reflection_controller(ctx_is_player, source_id, words)
            .is_some()
    }

    fn retire_reflections(&mut self) {
        self.world.retire_reflection_controllers();
    }

    // Op `0x34` sub-1: the attached light (`FUN_801E5668`), seated on the
    // actor the op runs against. See `world/field_script_actors.rs`.
    fn spawn_attached_light(
        &mut self,
        _ctx: &FieldCtx,
        ext: Option<u8>,
        spawn: &legaia_engine_vm::field_actor_billboard::AttachedSpriteSpawn,
        script: Option<&[u8]>,
    ) -> bool {
        self.world.spawn_field_attached_light(ext, spawn, script)
    }

    fn effect_anim_trigger(&mut self, ctx: &mut FieldCtx, arg: u8) {
        Self::effect_anim_trigger(self, ctx, arg)
    }

    /// Op `0x4C 0x61` - scripted CLUT-cell effect (one-shot cell write /
    /// cross-fade spawn). Decodes the 14-byte operand payload and queues the
    /// effect on the world; [`World::step_clut_fx`] applies it against the
    /// host's software VRAM on the retail game-tick cadence.
    ///
    /// PORT: FUN_801E4C58
    fn spawn_clut_cell_fx(&mut self, _ctx: &mut FieldCtx, payload: [u8; 14]) {
        self.world.spawn_clut_cell_fx(&payload);
    }
}

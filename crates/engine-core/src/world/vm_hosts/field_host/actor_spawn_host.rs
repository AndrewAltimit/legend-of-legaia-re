//! `FieldActorSpawnHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldActorSpawnHost for FieldHostImpl<'a> {
    // Op `43 0D` / `43 0F` (`FUN_801DE7BC`): spawn a screen-frame actor -
    // see [`crate::world::ScreenFrame`].
    fn op43_subdf_alloc_actor(&mut self, _sub_op: u8, mode: u8, args: [u8; 4]) {
        self.world
            .presentation
            .screen_frames
            .push(crate::world::ScreenFrame {
                width: args[0],
                rgb: [args[1], args[2], args[3]],
                ot: mode,
            });
    }

    // Op `34 2x` (`0x801E001C`): hand the context's attached light a new
    // keyframe script - see [`World::rescript_field_attached_light`].
    fn op34_sub2_capture_pc(
        &mut self,
        _ctx: &FieldCtx,
        ext: Option<u8>,
        block: Option<&[u8]>,
    ) -> bool {
        match block {
            Some(b) => self.world.rescript_field_attached_light(ext, b),
            None => false,
        }
    }

    // Op `43 0E`: retire the first live screen frame.
    fn op43_sube_mark_actor_flag_8(&mut self, _ctx: &mut FieldCtx) {
        if !self.world.presentation.screen_frames.is_empty() {
            self.world.presentation.screen_frames.remove(0);
        }
    }

    // Field-VM op `4C EB`: "run the next op only if this actor exists".
    // Retail resolves the id through `FUN_8003C83C`, the actor-list walk:
    // `0xF8` short-circuits to the player object, any other id matches a
    // live context's `+0x50` script id - the scene's channels here. The
    // trait default (always a miss) skipped every guarded op, so koin3's
    // entry script never raised the bit-3 flag on the dance hall's props.
    // REF: FUN_8003C83C
    fn actor_guard(&mut self, actor_id: u8) -> Option<()> {
        let found = if actor_id == 0xF8 {
            self.world.player_actor_slot.is_some()
        } else {
            legaia_engine_field::field_channels::resolve_target(self.world.channel_view(), actor_id)
                .is_some()
        };
        found.then_some(())
    }

    // -- the three frame-delta timer templates ---------------------------
    //
    // Each spawner allocates one pool actor from a plain template and fills
    // in its clock; the engine keeps the record on `World` instead and steps
    // it in `World::tick_field_timer_actors`. Kernels live in
    // `legaia_engine_field::field_actor_timers`.

    /// Op `0x43` sub-`0xC` - the cinematic bar emitter (`FUN_801DE754`
    /// allocating `0x801F2858`, tick `FUN_801DD784`).
    ///
    /// The three operand bytes are the phase durations in ticks: bars close,
    /// hold, bars open. Retail allocates a fresh actor per call and the two
    /// would then both emit; the engine keeps one, because a second envelope
    /// over the first is a script defect rather than an effect.
    fn spawn_shutter_blackout(&mut self, b1: u8, b2: u8, b3: u8) {
        self.world.presentation.cinematic_bars =
            Some(legaia_engine_field::field_actor_timers::ShutterBars::spawn(
                i16::from(b1),
                i16::from(b2),
                i16::from(b3),
            ));
        self.world.presentation.cinematic_bar = 0;
    }

    fn spawn_scene_record(&mut self, global_index: u8) {
        // Record the spawn request for SceneHost::tick to resolve against the
        // current scene MAN (the partition math needs the MAN header, which
        // the VM borrow precludes touching here). Retail spawns the record as
        // a sibling context immediately; the one-frame deferral is invisible
        // because a fresh context first runs on the NEXT frame slice anyway.
        // A bounded queue (not a single slot) so a second spawn issued while
        // another record executes is not dropped - retail's context table
        // holds several concurrent spawned records.
        // REF: FUN_8003BDE0
        if self.world.field_vm.pending_record_spawns.len() < crate::world::SPAWNED_CONTEXT_SLOTS {
            self.world.field_vm.pending_record_spawns.push(global_index);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::SpawnRecord { global_index });
    }

    // Sub-op 0x14: the actor clone. The whole body is
    // `World::spawn_actor_clone` (retail `FUN_801D835C` plus the arm's own
    // `FUN_8003C83C` resolve); the clone then ticks itself out through
    // `World::tick_handler_actors`.
    // REF: FUN_801D835C
    fn clone_actor(&mut self, src_id: u8, tint_rgb: u32, fade_rate: i16) {
        self.world.spawn_actor_clone(src_id, tint_rgb, fade_rate);
    }

    fn allocate_child_actors(&mut self, _ctx: &mut FieldCtx, count: u8, tail: &[u8]) {
        Self::allocate_child_actors(self, _ctx, count, tail)
    }

    fn spawn_morph_actor(&mut self, b1: u8, words: [i16; 3]) {
        Self::spawn_morph_actor(self, b1, words)
    }
}

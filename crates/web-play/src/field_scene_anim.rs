//! The bundle type-6 **CLUT-walk** runner both browser pages step: the play
//! runtime over its scene host's VRAM, and the field-scene viewer
//! (`legaia-web-viewer`'s `field_scene`) over its headless live scene.

/// The bundle type-6 **CLUT-walk table** runner (`legaia_scene_formats::clut_walk`,
/// `FUN_8001ada4` case 0xB - water / waterfall shimmer, 12 carriers; see
/// `docs/subsystems/field-ambient-fx.md`), shared by the map viewer and the
/// play runtime. It runs on the retail game-tick clock: a game tick every
/// [`legaia_engine_vm::frame_clock::FrameClock::frame_step`] vsyncs
/// (`DAT_1F800393`; 2 in towns, 3 on the overworld).
///
/// The other VRAM writers - the ambient move-VM tree and the scripted CLUT
/// effects - are the world's, stepped through
/// `World::step_field_vram_effects` by whichever host owns the world (the
/// play runtime's scene host, the map viewer's
/// [`legaia_engine_scene_host::scene_live::LiveScene`]).
pub struct FieldSceneAnim {
    /// The CLUT-walk shimmer (the slot-5 / type-6 walker or the legacy
    /// ocean-head fallback), stepped by the one engine kernel the native
    /// window runs (`legaia_engine_scene::clut_walk_anim`).
    clut: Option<legaia_engine_scene::clut_walk_anim::ClutWalkAnim>,
    /// Vsyncs per game tick (retail `DAT_1F800393`).
    frame_step: u8,
    /// Vsyncs banked toward the next game tick.
    vsync_accum: u8,
}

impl FieldSceneAnim {
    /// A runner over an installed CLUT walker. On both pages the live
    /// `World` carries the ambient move-VM tree (spawned at scene entry and
    /// drained per sim tick), so only the CLUT-walk half runs here.
    pub fn clut_only(
        clut: legaia_engine_scene::clut_walk_anim::ClutWalkAnim,
        frame_step: u8,
    ) -> FieldSceneAnim {
        FieldSceneAnim {
            clut: Some(clut),
            frame_step,
            vsync_accum: 0,
        }
    }

    /// Which CLUT animator is installed: `1` the walker, `2` the legacy
    /// ocean-head cycle, `0` none (the two are exclusive). Read by
    /// `LegaiaRuntime::play_field_anim_kind`.
    pub(crate) fn kind_code(&self) -> u32 {
        use legaia_engine_scene::clut_walk_anim::ClutWalkAnim;
        match self.clut {
            Some(ClutWalkAnim::Walk { .. }) => 1,
            Some(ClutWalkAnim::Ocean { .. }) => 2,
            None => 0,
        }
    }

    /// Re-point the animator at the world's **live** vsyncs-per-game-tick
    /// (`World::clock.frame_step`, retail `DAT_1F800393`).
    ///
    /// The play runtime snapshotted this at scene rebuild while the native
    /// window reads it off the world every frame. Today the two agree (the
    /// scene loader installs the per-mode floor and only the un-wired
    /// adaptive resolver `World::resolve_frame_step` can raise it), so this
    /// is the shape of the drift rather than a visible one - but a snapshot
    /// is exactly what makes wiring that resolver change one host only.
    pub fn set_frame_step(&mut self, frame_step: u8) {
        self.frame_step = frame_step.max(1);
    }

    /// Advance `vsyncs` retail vsyncs and apply any due VRAM writes to
    /// `vram`. Returns `true` when texels changed.
    pub fn tick(&mut self, vsyncs: u32, vram: &mut legaia_tim::Vram) -> bool {
        let dt = u32::from(self.frame_step.max(1));
        let mut game_ticks = 0u32;
        for _ in 0..vsyncs.min(64) {
            self.vsync_accum += 1;
            if u32::from(self.vsync_accum) >= dt {
                self.vsync_accum = 0;
                game_ticks += 1;
            }
        }
        if game_ticks == 0 {
            return false;
        }
        let mut wrote = false;
        // The CLUT-walk shimmer, one engine step per game tick.
        if let Some(clut) = self.clut.as_mut() {
            for _ in 0..game_ticks {
                wrote |= clut.game_tick(dt, vram);
            }
        }
        wrote
    }

    /// CLUT-walk entry count, for the UI status line.
    pub fn walker_entries(&self) -> usize {
        match self.clut.as_ref() {
            Some(legaia_engine_scene::clut_walk_anim::ClutWalkAnim::Walk { table, .. }) => {
                table.entries.len()
            }
            _ => 0,
        }
    }
}

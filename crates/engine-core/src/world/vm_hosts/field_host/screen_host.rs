//! `FieldScreenHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldScreenHost for FieldHostImpl<'a> {
    fn write_or_ramp_global_slot(&mut self, sub: u8, target: i32, ticks: u16) {
        let _ = ticks;
        // Sub-D: the audio duck `_DAT_8007B910 = (value * _DAT_8008457C) >>
        // 12`, a fraction of the configured level in `1/0x1000` - handed to
        // the session's duck as a percentage. The director ramps it its own
        // way (one retail unit a frame), not over the op's `ticks`.
        if sub == 0xD {
            let pct = (i64::from(target) * 100 / 0x1000).clamp(0, 255) as u8;
            self.world.audio.field_duck_pct = Some(pct);
            return;
        }
        let Some(ctrl) = self.world.world_map.ctrl.as_mut() else {
            return;
        };
        let v = target as u32;
        match sub {
            0xA => ctrl.horizon_params.0 = v,
            0xB => ctrl.horizon_params.1 = v,
            0xC => ctrl.horizon_params.2 = v,
            _ => {}
        }
    }

    // Op-0x43 screen-effect widget sub-ops (the PROT-0900 mask / sprite /
    // panel / letterbox family, exercised by the ten ending scenes).
    // Each routes to the world's widget host; the Field / Cutscene tick
    // advances the widgets and publishes `World::presentation.fx_frame`.
    // REF: FUN_801F8004 / FUN_801F8D4C / FUN_801F88FC / FUN_801F8E6C /
    // FUN_801F8F28 (spawn + control APIs)
    fn spawn_sprite_widget(&mut self, payload: &[u8]) {
        self.world.presentation.fx.sprite_spawn(payload);
    }

    fn tween_mask_rect(&mut self, words: [u16; 5]) {
        self.world.presentation.fx.mask_rect(words);
    }

    fn configure_letterbox(&mut self, payload: &[u8]) {
        self.world.presentation.fx.letterbox_config(payload);
    }

    fn spawn_panel_widget(&mut self, payload: &[u8; 13]) {
        self.world.presentation.fx.panel_spawn(payload);
    }

    fn move_panel_widget(&mut self, words: [i16; 4]) {
        self.world.presentation.fx.panel_move(words);
    }

    fn scene_fade(&mut self, op0_word: u16, op1_word: u16) -> SceneFadeResult {
        Self::scene_fade(self, op0_word, op1_word)
    }

    fn set_effect_colour(&mut self, op0: u8, rgb: [u8; 3], intensity: i16) {
        Self::set_effect_colour(self, op0, rgb, intensity)
    }

    fn set_scene_colour(&mut self, op0: u8, payload: &[u8; 5]) {
        Self::set_scene_colour(self, op0, payload)
    }

    // Op `0x4C` outer-nibble-3 sub-0 / sub-1: the ambient-particle master
    // gate `_DAT_8007B854`. Retail keeps it in one word that the emitter
    // re-reads every frame; the port keeps a copy on each live emitter
    // element, so the single write fans out over the channel here. Both hosts
    // run the channel off `World::tick_cutscene_elements`, so this one sink
    // serves native and browser alike.
    fn set_ambient_particle_gate(&mut self, enabled: bool) {
        self.world.set_ambient_particles_enabled(enabled);
    }

    /// Op `0x4C 0xDB` - spawn the single-source CLUT blend fade
    /// (`FUN_801E57F0` -> handler `FUN_801E4D8C`). `bytecode` starts at the
    /// `0xDB` byte, the record pointer retail parks at actor `+0x90`; the
    /// fade runs on [`World::step_clut_fx`]'s game-tick bank against the
    /// host's software VRAM.
    ///
    /// REF: FUN_801E57F0
    fn spawn_clut_blend_fade(&mut self, bytecode: &[u8]) {
        self.world.spawn_clut_blend_fx(bytecode);
    }

    /// Op `0x4C 0x60` - literal-operand VRAM `MoveImage`. The six words are
    /// `[src_x, src_y, w, h, dst_x, dst_y]`; retail's handler arm hands them
    /// straight to the libgpu `MoveImage` wrapper. Queued on the world;
    /// [`World::apply_script_vram_moves`] drains the queue against the
    /// host's software VRAM. Retail's known user: the one-shot face-frame
    /// stamps onto the player texture atlas (town01's opening record stamps
    /// the Noa blink/mouth cells - see `docs/formats/character-mesh.md`).
    ///
    /// REF: FUN_80058490 (consumer: `jal` at 0x801E1B84; ported as
    /// `legaia_tim::Vram::move_image`)
    /// REF: FUN_801DE840 (sub-0x60 handler arm 0x801E1B28..0x801E1B90),
    /// FUN_8003CE9C (misaligned-u16 operand reads)
    fn vram_move_image(&mut self, words: [i16; 6]) {
        self.world.queue_script_vram_move(words);
    }

    /// Op `4C D4` - set the mask bit on a 16x1 VRAM run's non-zero words.
    /// `teien` `P1[0]` runs it over every CLUT of rows 505..507 before it
    /// installs the HSV cycler (`34 30 06`) that darkens them for the night
    /// garden; without the bit, the entries the cycler takes to black read
    /// `0x0000` and the hedge texels behind them go transparent.
    fn vram_stp_set(&mut self, vram_x: u16, vram_y: u16) {
        self.world
            .ambient
            .script_vram_stp
            .push((vram_x, vram_y, true));
    }

    /// Op `4C D5` - clear the mask bit on a 16x1 VRAM run (every word but
    /// `0x8000`).
    fn vram_stp_clear(&mut self, vram_x: u16, vram_y: u16) {
        self.world
            .ambient
            .script_vram_stp
            .push((vram_x, vram_y, false));
    }

    /// Op `0x43` sub-`0x12` - the GP0 `0x80` VRAM rectangle copy, after the
    /// VM has resolved the arm's two-page split into one or two
    /// `FUN_800468A4` calls. Queued on the world the same way the `4C 60`
    /// `MoveImage` family is; [`World::apply_vram_rect_copies`] runs each
    /// call through the retail enqueue and executes the resulting packet
    /// against the host's software VRAM.
    ///
    /// REF: FUN_800468a4 (the enqueue the drain runs each call through)
    fn queue_vram_rect_copies(&mut self, calls: &[vm::vram_rect_copy::RectCopyCall]) {
        self.world.queue_vram_rect_copies(calls);
    }

    /// `[4C, 0x8A, ...]` - the field light: the angle trio
    /// `_DAT_8007B780..84` and the back colour `_DAT_8007B788` the
    /// light-source TMD rows shade through
    /// ([`legaia_render_kernels::field_light`]). `town01`'s `P1[0]` sets a white
    /// back colour; `koin3`'s cutscene records drop it to black and back.
    fn set_field_light(&mut self, slots: [i16; 3], packed: u32) {
        self.world.presentation.field_light =
            legaia_render_kernels::field_light::FieldLight::from_op_4c_8a(slots, packed);
    }
}

#[cfg(test)]
mod audio_duck_tests {
    use crate::world::World;
    use crate::world::vm_hosts::{FieldHostImpl, field_step_routed};
    use legaia_engine_vm::field::FieldCtx;

    /// `4C 4D 00 0C 00 00`: `_DAT_8007B910 = (0xC00 * level) >> 12`, three
    /// quarters of the configured level, handed to the session as `75`%;
    /// the MAN loader's reset brings it back to `100`.
    #[test]
    fn op_4c_4d_ducks_the_audio_and_a_scene_load_restores_it() {
        let mut w = World::new();
        let mut ctx = FieldCtx::default();
        let mut host = FieldHostImpl { world: &mut w };
        field_step_routed(
            &mut host,
            &mut ctx,
            &[0x4C, 0x4D, 0x00, 0x0C, 0x00, 0x00],
            0,
        );
        assert_eq!(w.take_field_audio_duck(), Some(75));
        assert_eq!(w.take_field_audio_duck(), None);
        w.man_load_actor_reset();
        assert_eq!(w.take_field_audio_duck(), Some(100));
    }
}

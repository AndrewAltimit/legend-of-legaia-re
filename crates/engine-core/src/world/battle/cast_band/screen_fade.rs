//! Cast-band screen draws: the cross beam and the screen-fade quads.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// PROT 0948's beam counter while its builder ran on this module tick -
    /// the one input `legaia_render_kernels::cast_beam::cross_beam_prims` builds
    /// the frame's beam packets from, on both hosts. `None` outside arm 3.
    pub fn cross_beam_draw(&self) -> Option<i32> {
        (self.mode == SceneMode::Battle && self.casting.module_beam_live)
            .then_some(self.casting.module_beam_counter)
    }

    /// PROT 0910's (Swordie) packet builders on the module's last pass -
    /// the split panels and the slash streak
    /// ([`legaia_engine_cast::seru_magic_ticks::SwordieDraw`]), which
    /// `legaia_render_kernels::cast_swordie` builds for both hosts. `None`
    /// outside a live Swordie cast.
    pub fn swordie_draw(&self) -> Option<legaia_engine_cast::seru_magic_ticks::SwordieDraw> {
        let st = self.casting.summon_stager.as_ref()?;
        (self.mode == SceneMode::Battle
            && self.cast_module_for(st.spell_id)
                == Some(legaia_engine_cast::seru_magic_ticks::SWORDIE_ENTRY))
        .then_some(self.casting.module_swordie.draw)
    }

    /// The full-screen fade quad to composite this frame, as
    /// `(rgb 0xRRGGBB, abr_mode, ot_index)` for
    /// `legaia_render_kernels::screen_prim::fade_prim` - `None` while no fade is
    /// live or its start delay is still running (retail's tick returns `-1`
    /// and draws nothing). The ABR mode is the template's kind word and the
    /// OT index the id `FUN_80024E80` stamped (`AddPrim(ot + id*4, ..)` in
    /// `FUN_80024EE4`).
    pub fn screen_fade_draw(&self) -> Option<(u32, u8, u32)> {
        fade_draw(self.presentation.fade.as_ref()?)
    }

    /// Every live full-screen fade quad this frame, the world's fade first
    /// and then the module fades running beside it
    /// ([`crate::world::ScreenFxState::module_fades`], and the fades the
    /// presentation hook spawned), each as
    /// [`Self::screen_fade_draw`] gives it. Both hosts composite the list.
    pub fn screen_fade_draws(&self) -> Vec<(u32, u8, u32)> {
        self.presentation
            .fade
            .iter()
            .chain(self.presentation.module_fades.iter())
            .chain(self.casting.module_pres.fades.iter().map(|f| &f.state))
            .filter_map(fade_draw)
            .collect()
    }
}

/// One fade's quad, `None` while its start delay runs.
fn fade_draw(f: &legaia_engine_system::fade::FadeState) -> Option<(u32, u8, u32)> {
    if !f.visible() {
        return None;
    }
    let [r, g, b] = f.rgb();
    Some((
        (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
        f.kind.clamp(0, 3) as u8,
        f.mode[2].max(0) as u32,
    ))
}

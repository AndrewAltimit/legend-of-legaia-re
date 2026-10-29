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

    /// Step every live screen fade `vsyncs` on, as the world tick steps them
    /// (`frame_tick::mode_tick`), outside a tick. A capture harness seat: a
    /// retail state saved past the flip shows its fades one pass nearer the
    /// RAM than the rest of its frame, since the fade actor links its quad
    /// into the table of the pass it steps for.
    pub fn lead_screen_fades(&mut self, vsyncs: u16) {
        let n = u8::try_from(vsyncs).unwrap_or(u8::MAX);
        if n == 0 {
            return;
        }
        if let Some(fade) = &mut self.presentation.fade
            && !fade.step_vsyncs(n)
            && !fade.holds_at_end()
        {
            self.presentation.fade = None;
        }
        self.presentation
            .module_fades
            .retain_mut(|f| f.step_vsyncs(n) || f.holds_at_end());
        self.step_battle_entry_fade(n);
        self.casting.step_cast_hook_fades(n);
    }

    /// Step the stage loader's entry fade-in `vsyncs` on, dropping it when
    /// its ramp and one-frame hold have run (hold `1`).
    pub(crate) fn step_battle_entry_fade(&mut self, vsyncs: u8) {
        if let Some(f) = &mut self.battle.entry_fade
            && !f.step_vsyncs(vsyncs)
        {
            self.battle.entry_fade = None;
        }
    }

    /// Every live full-screen fade quad this frame, the world's fade first
    /// and then the module fades running beside it
    /// ([`crate::world::ScreenFxState::module_fades`], and the fades the
    /// presentation hook spawned), each as
    /// [`Self::screen_fade_draw`] gives it. Both hosts composite the list.
    ///
    /// The battle's entry fade ([`Self::battle_entry_fade_draw`]) leads it:
    /// battle init spawns it before any other fade actor of the fight.
    pub fn screen_fade_draws(&self) -> Vec<(u32, u8, u32)> {
        self.battle_entry_fade_draw()
            .into_iter()
            .chain(
                self.presentation
                    .fade
                    .iter()
                    .chain(self.presentation.module_fades.iter())
                    .chain(self.casting.module_pres.fades.iter().map(|f| &f.state))
                    .filter_map(fade_draw),
            )
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

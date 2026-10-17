//! The band word `ctx[+0x6D8]` across the summon band's hand-over.
//!
//! Retail has one halfword there. The action SM arms it to `0x78` at `0x34`
//! and drains it by the frame step at `0x35`, and three player-Seru modules
//! keep their arm countdown in the same word: PROT 0904 (Theeder,
//! `sh v0,0x6d8(s1)` at `0x801F6DE8`, drains at `0x801F6FD4..0x801F6FE0`),
//! PROT 0907 (Nighto, `0x6d8(s5)` from `0x801F7444`) and PROT 0908 (Zenoir,
//! `addiu s8,v1,0x6d8` at `0x801F6A44`, every `0x0(s8)` access after it).
//! `0x35` re-reads the word after the stager call and leaves only once it is
//! negative, so each re-seed keeps the band in `0x35`
//! ([`legaia_engine_battle_vm::battle_action`]'s `summon_sustain`).
//!
//! The engine keeps those modules' countdown on the director's
//! [`ModuleCamState::countdown`](legaia_engine_cast::cast_module_camera::ModuleCamState)
//! and the SM's on `BattleActionCtx::frame_timer`; this file is the seam that
//! makes them one word while the SM is in `0x34` / `0x35`.

use super::*;

/// The slot-B modules whose arm countdown is `ctx[+0x6D8]` in the arms that
/// run while the band sits in `0x35`. PROT 0906 / 0909 / 0910 / 0916 also
/// store to the word, but their retail captures show no `0x35` / `0x36`
/// split against the engine and their engine countdowns are not yet checked
/// against the word, so they keep the SM's own value.
pub(in crate::world) const BAND_WORD_MODULES: [u32; 3] = [904, 907, 908];

impl World {
    /// Whether the resident summon module keeps its countdown in the band
    /// word ([`BAND_WORD_MODULES`]).
    fn band_word_shared(&self) -> bool {
        self.casting
            .summon_stager
            .as_ref()
            .and_then(|st| self.cast_module_for(st.spell_id))
            .is_some_and(|e| BAND_WORD_MODULES.contains(&e))
    }

    /// The SM's band word going into a `0x34` / `0x35` stager call.
    ///
    /// PORT: FUN_801E295C (`0x801E4B20` / `0x801E4BDC` / `0x801E4C78`: the
    /// stores the stager call sees)
    pub(in crate::world) fn summon_band_word_in(&mut self, word: i16) {
        self.casting.band_word_live = Some(word);
        if self.band_word_shared() {
            self.casting.module_cam.countdown.0 = i32::from(word);
        }
    }

    /// The band word as the stager call left it, for the SM's re-read.
    ///
    /// PORT: FUN_801E295C (`lh v0,0x2(s7)` at `0x801E4C84`, the `0x35` exit
    /// test after the stager call)
    pub(in crate::world) fn summon_band_word_out(&mut self) -> Option<i16> {
        self.casting.band_word_live = None;
        self.band_word_shared()
            .then_some(self.casting.module_cam.countdown.0 as i16)
    }
}

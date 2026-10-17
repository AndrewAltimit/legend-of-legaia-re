//! The world's side of the slot-2 / slot-6 SFX bank residency: the per-tick
//! sync hosts call, and field-VM op `0x36` sub `3`'s audio release. The
//! residency kernel itself is [`legaia_engine_mode::sfx_residency`].

use legaia_engine_mode::sfx_residency::SharedRegionBank;

use super::*;

impl World {
    /// Advance the slot-2 / slot-6 residency to the world's current mode and
    /// scene ([`legaia_engine_mode::sfx_residency::SfxBankResidency::sync`]), and return the bank the shared
    /// region should hold. Hosts call it once per tick and restage the region
    /// when the answer changes.
    pub fn sync_sfx_residency(&mut self) -> Option<SharedRegionBank> {
        let side_band = self
            .side_band_bank()
            .filter(|b| b.slot == 6)
            .map(|b| (b.request, b.prot_entry));
        let mode = self.mode;
        self.audio
            .residency
            .sync(mode, &self.active_scene_label, side_band)
    }

    /// The bank the shared slot-2 / slot-6 region holds, as of the last
    /// [`Self::sync_sfx_residency`].
    pub fn shared_sfx_bank(&self) -> Option<SharedRegionBank> {
        self.audio.residency.shared
    }

    /// Run field-VM op `0x36` sub `3` - `FUN_801D8450` - against the engine:
    /// replay [`legaia_engine_field::field_audio_release::field_audio_release_steps`] in
    /// retail's order. The two voice stops cross to the host
    /// ([`Self::take_sfx_voice_stops`]); the slot-6 close and the latch clear
    /// land on [`legaia_engine_mode::sfx_residency::SfxBankResidency::side_band_teardown`]; `_DAT_8007BA88` has
    /// no engine cell (nothing in the engine raises the forced-channel latch).
    pub fn release_field_audio(&mut self) {
        use legaia_engine_field::field_audio_release::{FIELD_CUE_GLOBAL_B, ReleaseStep};
        for step in legaia_engine_field::field_audio_release::field_audio_release_steps() {
            match step {
                ReleaseStep::StopVoice(v) => self.audio.sfx_voice_stops.push(v as u8),
                ReleaseStep::ReleaseVabSlot(_) => self.audio.residency.side_band_teardown(),
                ReleaseStep::ClearGlobal(FIELD_CUE_GLOBAL_B) => {
                    self.audio.residency.field_bank_latch = false;
                }
                ReleaseStep::ClearGlobal(_) => {}
            }
        }
    }

    /// Drain the SPU voices field-VM ops asked the host to stop this tick.
    pub fn take_sfx_voice_stops(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.audio.sfx_voice_stops)
    }

    /// Drain the voices a minigame keyed directly this tick
    /// ([`crate::world::AudioState::sfx_voice_keys`]). Each host keys them
    /// through its explicit voice-attr door (`key_on_voice_attr`).
    pub fn take_sfx_voice_keys(
        &mut self,
    ) -> Vec<legaia_engine_minigames::other_game_overlay::VoiceAttrCue> {
        std::mem::take(&mut self.audio.sfx_voice_keys)
    }

    /// Drain the positional emitters' key / volume / release calls this tick
    /// ([`crate::world::AudioState::sfx_emitter_ops`]). The audio director
    /// resolves each cue's descriptor and bank and drives the voices.
    pub fn take_sfx_emitter_ops(&mut self) -> Vec<legaia_engine_vm::sfx_emitter::EmitterOp> {
        std::mem::take(&mut self.audio.sfx_emitter_ops)
    }

    /// Install the camera focus the emitters listen from, as retail stores
    /// it (`_DAT_80089118` / `_DAT_80089120`, negated). The session writes
    /// the frame's camera here before the world tick.
    pub fn set_sound_listener_focus(&mut self, focus_stored_x: i32, focus_stored_z: i32) {
        self.audio.listener_focus = (focus_stored_x, focus_stored_z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_mode::mode::SceneMode;
    use legaia_engine_mode::sfx_residency::minigame_slot2_bank;

    fn world_in(mode: SceneMode, scene: &str) -> World {
        let mut w = World::new();
        w.mode = mode;
        w.active_scene_label = scene.into();
        w
    }

    #[test]
    fn field_entry_loads_the_field_bank_once() {
        let mut w = world_in(SceneMode::Field, "town01");
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::FIELD));
        assert!(w.audio.residency.field_bank_latch);
        w.active_scene_label = "town02".into();
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::FIELD));
    }

    #[test]
    fn battle_swaps_the_region_and_the_field_reloads_on_return() {
        let mut w = world_in(SceneMode::Field, "town01");
        w.sync_sfx_residency();
        w.mode = SceneMode::Battle;
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::CLASS2));
        assert!(!w.audio.residency.field_bank_latch);
        w.mode = SceneMode::Field;
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::FIELD));
    }

    #[test]
    fn a_minigame_takes_slot_two_and_clears_the_latch() {
        let mut w = world_in(SceneMode::Field, "town01");
        w.sync_sfx_residency();
        w.mode = SceneMode::Fishing;
        let b = w.sync_sfx_residency().unwrap();
        assert_eq!((b.slot, b.prot_entry), (2, 1197));
        w.mode = SceneMode::Field;
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::FIELD));
    }

    #[test]
    fn baka_leaves_slot_six_open_over_the_class2_samples() {
        let mut w = world_in(SceneMode::Field, "town01");
        w.sync_sfx_residency();
        assert!(w.audio.residency.slot_open(6));
        w.mode = SceneMode::BakaFighter;
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::CLASS2));
        let r = &w.audio.residency;
        assert!(r.slot_open(2) && r.slot_open(6), "both enables up");
        assert_eq!(r.stale_open_slot(), Some(6));
        assert!(!r.field_bank_latch);
        // The field init closes 2 and reloads 0876 into 6.
        w.mode = SceneMode::Field;
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::FIELD));
        let r = &w.audio.residency;
        assert!(!r.slot_open(2) && r.slot_open(6));
        assert_eq!(r.stale_open_slot(), None);
    }

    #[test]
    fn a_dome_leg_is_a_battle_for_the_region() {
        let mut w = world_in(SceneMode::Field, "town01");
        w.sync_sfx_residency();
        w.mode = SceneMode::MuscleDome;
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::CLASS2));
        let r = &w.audio.residency;
        assert!(r.slot_open(2) && !r.slot_open(6), "the 0x14 close arm ran");
        assert_eq!(r.stale_open_slot(), None);
        assert_eq!(minigame_slot2_bank(SceneMode::MuscleDome), None);
    }

    #[test]
    fn battle_closes_slot_six() {
        let mut w = world_in(SceneMode::Field, "town01");
        w.sync_sfx_residency();
        w.mode = SceneMode::Battle;
        w.sync_sfx_residency();
        assert!(!w.audio.residency.slot_open(6));
        assert!(w.audio.residency.slot_open(2));
    }

    #[test]
    fn the_pause_menu_leaves_the_region_alone() {
        let mut w = world_in(SceneMode::Field, "town01");
        w.sync_sfx_residency();
        w.mode = SceneMode::Menu;
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::FIELD));
        w.mode = SceneMode::Field;
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::FIELD));
        assert!(w.audio.residency.field_bank_latch);
    }

    #[test]
    fn teardown_silences_slot_six_until_the_next_field_init() {
        let mut w = world_in(SceneMode::Field, "town01");
        w.sync_sfx_residency();
        w.audio.residency.side_band_teardown();
        assert_eq!(w.sync_sfx_residency(), None);
        w.active_scene_label = "town02".into();
        assert_eq!(w.sync_sfx_residency(), Some(SharedRegionBank::FIELD));
    }

    #[test]
    fn op_36_sub_3_stops_the_top_voices_and_closes_slot_six() {
        let mut w = world_in(SceneMode::Field, "town01");
        w.sync_sfx_residency();
        w.release_field_audio();
        assert_eq!(w.take_sfx_voice_stops(), vec![0x17, 0x16]);
        assert!(!w.audio.residency.field_bank_latch);
        assert_eq!(w.shared_sfx_bank(), None);
    }
}

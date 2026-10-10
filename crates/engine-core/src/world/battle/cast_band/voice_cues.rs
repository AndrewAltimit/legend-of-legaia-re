//! Cast voice and XA cues: the module voice emit, the battle XA cue and the
//! per-round voice listing.

use super::*;

impl World {
    /// Raise the paged module's **own** CD-XA voice - the head cue every
    /// slot-B image calls the dispatcher `FUN_8004FCC8` with near its head
    /// (`docs/subsystems/cast-module.md`, "The cast's own CD-XA voice"),
    /// scanned off the module's bytes at cast time
    /// ([`legaia_engine_vm::battle_cast_cue::module_head_cue`]) and run
    /// through the dispatcher's CD-XA arm
    /// ([`legaia_engine_vm::battle_cast_cue::admit_voice_cue`]).
    ///
    /// Retail raises it from inside the module's first tick, after the
    /// module's own poll of the side-band stage byte; the port raises it at
    /// the arming seam, the frame the module becomes resident - which is
    /// the same frame, since the port's side-band is resident rather than
    /// streamed (`VoiceCueGates::side_band_stage` is `0` here for that
    /// reason). The other gate is live: a cast inside the previous clip's
    /// read span (`battle_xa_busy_frames`, retail's `gp+0x91C`) plays no
    /// voice, exactly as `FUN_8003DE7C(1)` declines it.
    ///
    /// The request lands on [`crate::world::AudioState::battle_xa_cues`],
    /// the `(clip_slot, channel, dur)` channel both hosts already play
    /// through their XA lane, and arms the busy span the way the melee
    /// kernel's clip start does. Nothing here touches gameplay state; a
    /// disc-free build (no pool, no span table) emits nothing.
    ///
    /// REF: FUN_8004FCC8 (the CD-XA arm), FUN_8003D53C (the starter the
    /// request stands for)
    pub(super) fn emit_cast_module_voice(&mut self, spell_id: u8) {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return;
        };
        let Some(pool) = self.casting.effect_pool.clone() else {
            return;
        };
        let Some(module) = pool.module(entry) else {
            return;
        };
        let Some(head) = vm::battle_cast_cue::module_head_cue(&module.bytes) else {
            return;
        };
        let id = head.resolve(|| self.next_rand());
        self.emit_battle_xa_cue(id);
    }

    /// Run one CD-XA cue id through the dispatcher's CD-XA arm
    /// ([`legaia_engine_vm::battle_cast_cue::admit_voice_cue`]) onto
    /// [`crate::world::AudioState::battle_xa_cues`] - the half of
    /// [`Self::emit_cast_module_voice`] after the module's head cue is
    /// resolved, and what a module arm's own `FUN_8004FCC8(id >= 0x100)` call
    /// raises.
    ///
    /// REF: FUN_8004FCC8 (the CD-XA arm)
    pub(in crate::world) fn emit_battle_xa_cue(&mut self, id: u16) {
        let raw = self
            .audio
            .xa_cue_durations
            .as_deref()
            .and_then(|t| t.get(usize::from(id).wrapping_sub(0x100)).copied());
        let gates = vm::battle_cast_cue::VoiceCueGates {
            side_band_stage: 0,
            clip_span_left: self.audio.battle_xa_busy_frames,
        };
        if let vm::battle_cast_cue::VoiceCueVerdict::Play(req) =
            vm::battle_cast_cue::admit_voice_cue(id, gates, raw)
        {
            // The starter holds the drive for the clip's read span
            // (`FUN_8003D53C`; the same latch `push_battle_xa_cue` arms for
            // the melee kernel's clips).
            self.audio.battle_xa_busy_frames = req.duration_sectors.min(u16::MAX as u32) as u16;
            self.audio.battle_xa_cues.push(crate::sfx_cue::XaVoiceClip {
                clip: req.clip_slot,
                channel: req.channel,
                duration_sectors: req.duration_sectors,
            });
        }
    }

    /// Every cast-voice clip `spell_id`'s module can raise - the
    /// [`Self::emit_cast_module_voice`] resolution without its gates or its
    /// coin flip: a literal head cue gives one request, a random one
    /// (`base + rand() % span`, PROT 0936 / 0937) one per candidate. Empty
    /// when the module, its head cue or the span table is missing.
    pub(in crate::world) fn cast_module_voice_candidates(
        &self,
        spell_id: u8,
    ) -> Vec<crate::sfx_cue::XaVoiceClip> {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return Vec::new();
        };
        let Some(module) = self
            .casting
            .effect_pool
            .as_ref()
            .and_then(|pool| pool.module(entry))
        else {
            return Vec::new();
        };
        let ids: Vec<u16> = match vm::battle_cast_cue::module_head_cue(&module.bytes) {
            Some(vm::battle_cast_cue::ModuleHeadCue::Literal(id)) => vec![id],
            Some(vm::battle_cast_cue::ModuleHeadCue::Random { base, span }) => {
                (0..u16::from(span.max(1)))
                    .map(|k| base.wrapping_add(k))
                    .collect()
            }
            None => Vec::new(),
        };
        let open = vm::battle_cast_cue::VoiceCueGates {
            side_band_stage: 0,
            clip_span_left: 0,
        };
        ids.into_iter()
            .filter_map(|id| {
                let raw = self
                    .audio
                    .xa_cue_durations
                    .as_deref()
                    .and_then(|t| t.get(usize::from(id).wrapping_sub(0x100)).copied());
                match vm::battle_cast_cue::admit_voice_cue(id, open, raw) {
                    vm::battle_cast_cue::VoiceCueVerdict::Play(req) => {
                        Some(crate::sfx_cue::XaVoiceClip {
                            clip: req.clip_slot,
                            channel: req.channel,
                            duration_sectors: req.duration_sectors,
                        })
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// List the cast voices the round's committed party spells may raise onto
    /// [`crate::world::AudioState::battle_xa_prestage`] - called at the
    /// round's start, before the first action dispatches.
    pub fn list_round_cast_voices(&mut self) {
        let spells: Vec<u8> = self
            .battle
            .round_flow
            .pending
            .iter()
            .filter_map(|p| match p {
                Some(crate::battle_round::PendingPartyAction::Spell { spell_id, .. }) => {
                    Some(*spell_id)
                }
                _ => None,
            })
            .collect();
        for spell in spells {
            for clip in self.cast_module_voice_candidates(spell) {
                if !self.audio.battle_xa_prestage.contains(&clip) {
                    self.audio.battle_xa_prestage.push(clip);
                }
            }
        }
    }
}

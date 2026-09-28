//! The field's CD-XA voice leg: one-shot clip starts raised outside a battle.
//!
//! Retail has one clip starter, `FUN_8003D53C(clip, channel, dur)`, and two
//! field callers of it that the engine models: the field VM's op `0x36`
//! bit-15-clear arm (`0x801E0420`, operands straight out of the scene MAN)
//! and the scripted-scene programs' voice state
//! ([`crate::field_actor_program`]). Before this leg existed both were
//! computed and dropped - the op pushed an event no host read, and
//! [`crate::world::World::tick_scene_programs`] counted its XA legs into a
//! frame report nobody consumed - so no scripted field voice line played on
//! either host.
//!
//! The requests use the battle's clip-request shape
//! ([`crate::sfx_cue::XaVoiceClip`]), so a host plays them through the same
//! `play_xa_clip` it already drives for the battle grunt and sting; both
//! hosts drain the queue in their field SFX routing (`route_field_sfx`).
//!
//! The other field XA call, `FUN_80019794(clip)` (op `0x36` with
//! `sel & 0x7FFF == 0`, and the scripted-scene stream state), is **not** a
//! stream start: its body `FUN_8003EAE4` issues `CdlSetloc` + `CdlSeekL`
//! (`li a0,0x15` at `0x8003EB68`) and never a read, so it only parks the drive
//! head on the file the next one-shot will read. The engine has no drive, so
//! that leg has nothing to do.
//!
//! REF: FUN_8003D53C (the starter), FUN_8003EAE4 (the seek-ahead)

use super::*;

/// Every CD-XA one-shot the field scripts in `man` can start: each op `0x36`
/// whose first operand has bit 15 clear and a non-zero selector - the arm
/// that calls `FUN_8003D53C(arg >> 3, arg & 7, sel)` (`0x801E0420`) - over
/// every record of every partition, deduplicated, in script order.
///
/// A linear decode of each record from its first opcode; it stops at a
/// record's first decode error, so an op past message text the walk cannot
/// resync over is missed (the clip then stages on first use, as before), and
/// a hit is a lead rather than a proof - which is all an advisory prestage
/// list needs. Empty when `man` does not parse.
pub fn scene_xa_prestage(man: &[u8]) -> Vec<crate::sfx_cue::XaVoiceClip> {
    use legaia_asset::field_disasm::{DisasmError, InsnInfo, LinearWalker, man_script_spans};
    let Ok(man_file) = legaia_asset::man_section::parse(man) else {
        return Vec::new();
    };
    let mut out: Vec<crate::sfx_cue::XaVoiceClip> = Vec::new();
    for (_, _, start, pc0, len) in man_script_spans(&man_file, man) {
        let Some(body) = man.get(start..start + len) else {
            continue;
        };
        for step in LinearWalker::new(body, pc0) {
            match step {
                Ok(insn) => {
                    let InsnInfo::SceneFade { word0, word1 } = insn.info else {
                        continue;
                    };
                    if word0 & 0x8000 != 0 || word0 == 0 {
                        continue;
                    }
                    let arg = word1 as i16;
                    let clip = crate::sfx_cue::XaVoiceClip {
                        clip: u32::from((arg >> 3) as u8),
                        channel: u32::from((arg & 7) as u8),
                        duration_sectors: u32::from(word0),
                    };
                    if !out.contains(&clip) {
                        out.push(clip);
                    }
                }
                Err((_, err)) => {
                    if !matches!(err, DisasmError::EndOfStream { .. }) {
                        break;
                    }
                }
            }
        }
    }
    out
}

impl From<crate::baka_fighter_chrome::XaCue> for crate::sfx_cue::XaVoiceClip {
    fn from(c: crate::baka_fighter_chrome::XaCue) -> Self {
        Self {
            clip: u32::from(c.clip),
            channel: u32::from(c.chan),
            duration_sectors: u32::from(c.dur),
        }
    }
}

impl From<crate::muscle_ringside::HubXaCue> for crate::sfx_cue::XaVoiceClip {
    fn from(c: crate::muscle_ringside::HubXaCue) -> Self {
        Self {
            clip: u32::from(c.clip),
            channel: u32::from(c.channel),
            duration_sectors: u32::from(c.duration_sectors),
        }
    }
}

impl World {
    /// Append a minigame's CD-XA lines to the prestage list both hosts drain
    /// ([`Self::drain_field_xa_prestage`]): the Baka Fighter chrome's
    /// announcer lines ([`crate::baka_fighter::BakaFight::take_xa_prestage`])
    /// and the dome hub's two
    /// ([`crate::muscle_ringside::hub_xa_prestage`]). Skips a clip already
    /// listed.
    pub fn queue_xa_prestage<C: Into<crate::sfx_cue::XaVoiceClip>>(
        &mut self,
        clips: impl IntoIterator<Item = C>,
    ) {
        for c in clips {
            let c = c.into();
            if !self.audio.field_xa_prestage.contains(&c) {
                self.audio.field_xa_prestage.push(c);
            }
        }
    }

    /// Take the loaded scene's field CD-XA prestage list
    /// ([`crate::world::AudioState::field_xa_prestage`]). A host that decodes
    /// clips asynchronously (the browser page) stages each ahead of its op;
    /// one that reads the disc synchronously (the native window) drains and
    /// drops it.
    pub fn drain_field_xa_prestage(&mut self) -> Vec<crate::sfx_cue::XaVoiceClip> {
        std::mem::take(&mut self.audio.field_xa_prestage)
    }

    /// Queue one field CD-XA clip start and hold the modelled drive busy for
    /// its read span (`dur` vsyncs, the battle leg's convention - see
    /// [`crate::world::AudioState::battle_xa_busy_frames`]).
    ///
    /// A zero `dur` is not a clip: retail's op `0x36` routes `sel & 0x7FFF ==
    /// 0` to the seek-ahead instead, so nothing is queued.
    pub fn push_field_xa_cue(&mut self, clip: u8, channel: u8, dur: u16) {
        if dur == 0 {
            return;
        }
        self.audio.field_xa_busy_frames = dur;
        self.audio.field_xa_cues.push(crate::sfx_cue::XaVoiceClip {
            clip: u32::from(clip),
            channel: u32::from(channel),
            duration_sectors: u32::from(dur),
        });
    }

    /// Take this tick's field CD-XA clip requests, oldest first. Both hosts
    /// call it from their field SFX routing and play each request through
    /// their `play_xa_clip`.
    pub fn drain_field_xa_cues(&mut self) -> Vec<crate::sfx_cue::XaVoiceClip> {
        std::mem::take(&mut self.audio.field_xa_cues)
    }

    /// `_DAT_8007BC20 != 0` as the field reads it: a one-shot is still
    /// inside its read span.
    pub fn field_xa_busy(&self) -> bool {
        self.audio.field_xa_busy_frames != 0
    }

    /// One vsync of the modelled drive (run by `World::tick`).
    pub(in crate::world) fn tick_field_xa_busy(&mut self) {
        self.audio.field_xa_busy_frames = self.audio.field_xa_busy_frames.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn duel_cfg(roster_id: usize) -> crate::baka_fighter::FighterConfig {
        crate::baka_fighter::FighterConfig {
            roster_id,
            damage_mod: 100,
            def_tiers: [0, 0, 0],
            crit_chance: 0,
            atk_tiers: [0, 0, 0],
            attack_power: [0, 10, 10, 10, 0],
            gold_reward: 30,
            ai_pattern: vec![1, 2, 3],
        }
    }

    /// Entering the duel lists the chrome's announcer lines on the prestage
    /// list both hosts drain, so the page stages them ahead of the frames
    /// that start them; the list is not re-queued on a quiet tick.
    #[test]
    fn entering_the_duel_queues_the_announcer_prestage() {
        let mut w = World::default();
        let fight = crate::baka_fighter::BakaFight::new(duel_cfg(0), duel_cfg(1), [2, 2], 1);
        w.enter_baka_fighter(fight);
        let listed = w.drain_field_xa_prestage();
        let want: Vec<crate::sfx_cue::XaVoiceClip> =
            crate::baka_fighter_chrome::announcer_xa_prestage(0)
                .into_iter()
                .map(Into::into)
                .collect();
        assert_eq!(listed, want);
        w.tick();
        assert!(
            w.drain_field_xa_prestage().is_empty(),
            "round 0 already listed"
        );
    }

    /// A clip already on the list is not listed again (a second dome leg
    /// re-queues the hub lines).
    #[test]
    fn queued_clips_are_deduplicated() {
        let mut w = World::default();
        w.queue_xa_prestage(crate::muscle_ringside::hub_xa_prestage());
        w.queue_xa_prestage(crate::muscle_ringside::hub_xa_prestage());
        assert_eq!(w.drain_field_xa_prestage().len(), 2);
    }

    #[test]
    fn a_field_clip_queues_and_holds_the_drive_for_its_span() {
        let mut w = World::default();
        w.push_field_xa_cue(0x10, 7, 3);
        assert!(w.field_xa_busy());
        assert_eq!(
            w.drain_field_xa_cues(),
            vec![crate::sfx_cue::XaVoiceClip {
                clip: 0x10,
                channel: 7,
                duration_sectors: 3
            }]
        );
        for _ in 0..3 {
            w.tick();
        }
        assert!(!w.field_xa_busy(), "the span elapses in `dur` world ticks");
    }

    #[test]
    fn a_zero_span_is_the_seek_ahead_and_queues_nothing() {
        let mut w = World::default();
        w.push_field_xa_cue(0x10, 0, 0);
        assert!(w.drain_field_xa_cues().is_empty());
        assert!(!w.field_xa_busy());
    }
}

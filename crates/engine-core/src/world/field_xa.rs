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

impl World {
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

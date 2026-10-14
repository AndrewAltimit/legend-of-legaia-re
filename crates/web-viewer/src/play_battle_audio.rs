//! The audio half of the play page's per-frame battle tick - the browser
//! twin of the native window's `window/battle.rs` event / cue block.
//!
//! The routing is one session step on both hosts
//! (`legaia_engine_session::BootSession::route_battle_audio`): the duck
//! target off the tick's `DuckAudioLevel` events, the strike / cast SFX cues
//! through the director's `enqueue_battle_cues`, the round's cast-voice
//! prestage, the one-shot CD-XA clips and the arts-voice shouts. This module
//! drains the tick's battle events for it and keeps the readout's count.

use crate::runtime::LegaiaRuntime;

impl LegaiaRuntime {
    /// Drain this tick's battle audio queues, once per
    /// [`LegaiaRuntime::tick_battle_presentation`], so nothing accumulates
    /// across frames.
    pub(crate) fn drain_battle_audio_cues(&mut self) {
        let Some(session) = self.scene_host.session_mut() else {
            return;
        };
        // Typed battle events. **Observation only** - the live battle loop
        // owns the gameplay fold and re-publishes the stream, so folding
        // again here would apply an art strike's HP twice. The one event a
        // host's audio reads is the duck.
        let events = session.host.world.drain_battle_events();
        // Counted whether or not a director exists to hear them.
        self.sfx.queued += session.route_battle_audio(&events);
    }
}

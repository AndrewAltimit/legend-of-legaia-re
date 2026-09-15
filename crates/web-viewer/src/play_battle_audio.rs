//! The audio half of the play page's per-frame battle tick - the browser
//! twin of the native window's `window/battle.rs` event / cue block
//! (`drain_and_log_battle_events` + `bgm.enqueue_sfx` / `play_art_shout` /
//! `play_xa_clip` / `set_duck_pct`).
//!
//! Every queue the world publishes for a host's audio is drained here once
//! per [`LegaiaRuntime::tick_battle_presentation`], whether or not this host
//! can sound it yet, so nothing accumulates across frames.

use crate::runtime::LegaiaRuntime;

impl LegaiaRuntime {
    /// Drain this tick's battle audio queues.
    pub(crate) fn drain_battle_audio_cues(&mut self) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        // Typed battle events. **Observation only** - the live battle loop
        // owns the gameplay fold and re-publishes the stream, so folding
        // again here would apply an art strike's HP twice.
        let _events = host.world.drain_battle_events();
        // Battle strike SFX cues route into the page's existing delay
        // scheduler (`crate::play_sfx`); the arts-voice shouts are CD-XA
        // clips this host has no demuxed channel bank for yet, so they are
        // drained (the world must not accumulate them) and dropped.
        let cues = host.world.drain_battle_sfx_cues();
        let _ = host.world.drain_battle_shout_cues();
        // NOT WIRED (browser): the melee grunt / attack sting are CD-XA clip
        // requests (`drain_battle_xa_cues`), and the play page has no XA
        // lane at all - the same gap that drops the arts shouts above.
        let _ = host.world.drain_battle_xa_cues();
        // `enqueue_sfx` needs `&mut self`, so fire after the host borrow ends.
        for cue in cues {
            if let Ok(id) = u8::try_from(cue.kind) {
                self.enqueue_sfx(id, cue.timing_frames);
            }
        }
    }
}

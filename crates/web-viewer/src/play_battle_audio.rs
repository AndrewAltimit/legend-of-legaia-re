//! The audio half of the play page's per-frame battle tick - the browser
//! twin of the native window's `window/battle.rs` event / cue block
//! (`drain_and_log_battle_events` + `bgm.enqueue_sfx` / `play_art_shout` /
//! `play_xa_clip` / `set_duck_pct` / `stage_transient_sfx_vab`).
//!
//! Every queue the world publishes for a host's audio is drained here once
//! per [`LegaiaRuntime::tick_battle_presentation`], so nothing accumulates
//! across frames, and each is routed the way the native block routes it:
//!
//! * `DuckAudioLevel` events set the duck target ([`crate::play_sfx`] ramps
//!   it one retail unit per tick and re-applies it to the sequencer);
//! * strike / cast SFX cues go into the page's frame-timed scheduler at
//!   their full `u16` id with `(actor, target)` riding along - classified
//!   at fire time, never truncated ([`crate::play_sfx::route_cue`]);
//! * a results-frame `LEVEL_UP_CUE` first stages the PROT 0889 reward bank
//!   transiently behind the BGM, as retail loads it at results time;
//! * arts-voice shouts and the melee grunt / attack-sting clip requests go
//!   to the CD-XA lane ([`crate::play_xa`]).

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::battle_events::BattleEvent;
use legaia_engine_core::world::LEVEL_UP_CUE;

impl LegaiaRuntime {
    /// Drain this tick's battle audio queues.
    pub(crate) fn drain_battle_audio_cues(&mut self) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        // Typed battle events. **Observation only** - the live battle loop
        // owns the gameplay fold and re-publishes the stream, so folding
        // again here would apply an art strike's HP twice. The one event a
        // host's audio reads is the duck: the summon / capture arms lower
        // the BGM to 75% of its reference, the Done band's `0x51` arm
        // raises it back.
        let mut duck_pct = None;
        for ev in host.world.drain_battle_events() {
            if let BattleEvent::DuckAudioLevel { target_pct } = ev {
                duck_pct = Some(target_pct);
            }
        }
        let cues = host.world.drain_battle_sfx_cues();
        let shouts = host.world.drain_battle_shout_cues();
        let xa_cues = host.world.drain_battle_xa_cues();
        // Everything below needs `&mut self`, so it runs after the host
        // borrow ends.
        if let Some(pct) = duck_pct {
            self.set_duck_pct(pct);
        }
        // The level-up jingle's bank (PROT 0889, cue `0x50`, category 11)
        // is loaded at results time in retail and lives nowhere resident in
        // the page's SFX region; stage it transiently behind the BGM the
        // moment the results frame asks for it, so the cue keys the real
        // sample instead of the class-2 fallback's sibling.
        if cues.iter().any(|c| c.kind == LEVEL_UP_CUE) && !self.stage_transient_reward_bank() {
            crate::console_log("play SFX: level-up jingle bank (PROT 0889) did not stage");
        }
        for cue in &cues {
            self.enqueue_battle_cue(cue.kind, cue.timing_frames, cue.actor_slot, cue.target_slot);
        }
        for xa in &xa_cues {
            self.play_xa_clip(xa.clip, xa.channel, xa.duration_sectors);
        }
        for shout in &shouts {
            self.play_art_shout(shout.cslot, shout.action);
        }
    }
}

//! The audio half of the play page's per-frame battle tick - the browser
//! twin of the native window's `window/battle.rs` event / cue block
//! (`drain_and_log_battle_events` + `bgm.enqueue_sfx` / `play_art_shout` /
//! `play_xa_clip` / `set_duck_pct` / `stage_transient_sfx_vab`).
//!
//! Every queue the world publishes for a host's audio is drained here once
//! per [`LegaiaRuntime::tick_battle_presentation`], so nothing accumulates
//! across frames, and each is routed the way the native block routes it:
//!
//! * `DuckAudioLevel` events set the director's duck target (its
//!   `tick_audio_frame` ramps it one retail unit per tick);
//! * strike / cast SFX cues go through the director's
//!   `enqueue_battle_cues`, the native window's call - full `u16` ids,
//!   classified at fire time, and a results-frame `LEVEL_UP_CUE` first
//!   parks the PROT 0889 reward bank behind the BGM;
//! * arts-voice shouts and the melee grunt / attack-sting clip requests go
//!   to the CD-XA lane ([`crate::play_xa`]).

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::battle_events::BattleEvent;

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
        let xa_prestage = host.world.drain_battle_xa_prestage();
        // The duck target, then the strike / cast cues through the
        // director's shared router (a results-frame `LEVEL_UP_CUE` parks the
        // reward bank first). The duck ramp and the drain run in
        // `tick_sfx`'s frame tail, after this. Counted whether or not a
        // director exists to hear them.
        self.sfx.queued += cues.len() as u32;
        if self.audio_director().is_some()
            && let (Some(d), Some(host)) = (self.director.as_mut(), self.scene_host.as_ref())
        {
            if let Some(pct) = duck_pct {
                d.set_duck_pct(pct);
            }
            d.enqueue_battle_cues(&cues, &host.index);
        }
        // Stage the round's cast voices ahead of their casts: the page slices
        // and decodes a clip the bank lacks asynchronously, so a voice first
        // asked for at the cast itself sounded a frame or more late.
        for xa in &xa_prestage {
            self.prestage_xa_clip(xa.clip, xa.channel, xa.duration_sectors);
        }
        for xa in &xa_cues {
            self.play_xa_clip(xa.clip, xa.channel, xa.duration_sectors);
        }
        for shout in &shouts {
            self.play_art_shout(shout.cslot, shout.action);
        }
    }
}

//! Audio-side state: BGM selection and resume, the SFX cue / delay slots, sound-bank handshakes and the battle SFX / XA / shout cue queues.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Audio-side state: BGM selection and resume, the SFX cue / delay slots, sound-bank handshakes and the battle SFX / XA / shout cue queues.
pub struct AudioState {
    /// "Sound bank ready" gate.
    pub sound_bank_ready: bool,
    /// Per-strike battle sound cues surfaced this frame for the host to play
    /// through its SFX bank (the art-record `HitCue` sound cues that
    /// [`World::fold_battle_event`] resolves from an `ApplyArtStrike` outcome -
    /// previously dropped). Cosmetic, like [`crate::world::BattleState::hit_fx`]: no gameplay
    /// state depends on them. Drained via [`World::drain_battle_sfx_cues`];
    /// cleared on battle exit.
    pub battle_sfx_cues: Vec<BattleSfxCue>,
    /// CD-XA one-shot clip requests the battle raised this tick - the
    /// `FUN_8003D53C(clip, channel, dur)` calls the melee kernel makes (the
    /// per-character `XA30` grunt) and the party voice leg of the sound
    /// funnel resolves (`XA27` for `0x10C`). Drained by the hosts into the
    /// XA mixing path ([`World::drain_battle_xa_cues`]).
    pub battle_xa_cues: Vec<crate::sfx_cue::XaVoiceClip>,
    /// Frames the modelled CD drive stays busy after a clip start - the
    /// read span in vsyncs (`dur * 2.5` sectors at 150/s = `dur / 60` s). The
    /// funnel's voice leg drops a request while it is non-zero
    /// (`FUN_8003DE7C(1) != 0` at `0x8004FE9C`), so two `0x10C` stings
    /// inside one read span collapse to the first. Counted down once per
    /// battle tick.
    pub battle_xa_busy_frames: u16,
    /// The static `SCUS_942.54` XA cue duration table (`DAT_800788B8`) the
    /// voice legs read (`legaia_asset::xa_cue_table`); installed at boot,
    /// `None` on a disc-free build (a voice cue then requests no span and
    /// is dropped).
    pub xa_cue_durations: Option<Vec<u16>>,
    /// Tactical-Arts **shout** cues queued this frame - one per executed
    /// party art carrying a real action constant, pushed on the art's
    /// animation-start frame (see [`crate::battle_events::BattleShoutCue`]).
    /// Cosmetic: the host resolves each against the arts-voice tables + XA
    /// clip banks and plays the CD-XA shout; nothing here mutates gameplay
    /// state. Drained via [`World::drain_battle_shout_cues`]; cleared on
    /// battle exit.
    pub battle_shout_cues: Vec<crate::battle_events::BattleShoutCue>,
    /// Last BGM the field VM started (op 0x35 sub-1 / sub-9). `None` until
    /// a scene starts one. Updated synchronously when the VM emits the
    /// corresponding `Bgm` event.
    pub current_bgm: Option<u16>,
    /// BGM id to swap to when a live-loop encounter begins, restored to the
    /// field track when the battle ends. `None` (the default) leaves music
    /// untouched across the Battle transition - set it via
    /// [`World::set_battle_bgm`] to enable the swap. The swap is routed as an
    /// ordinary `FieldEvent::Bgm` start (sub-op 1), so the host's existing
    /// BGM director resolves the SEQ and cross-fades exactly like a field
    /// op-`0x35` start.
    pub battle_bgm: Option<u16>,
    /// Field track stashed at battle entry so `World::restore_field_bgm`
    /// can resume it after the encounter. Managed by the swap helpers; not
    /// meant to be set directly.
    pub field_bgm_resume: Option<u16>,
    /// `true` while the battle track is playing (set by
    /// `World::swap_to_battle_bgm`, cleared by
    /// `World::restore_field_bgm`). Guards against double-swap / spurious
    /// restore.
    pub battle_bgm_active: bool,
    /// Retail's timed sound-source auto-release (`gp+0x808`/`0x814`/`0x81C`),
    /// serviced by the frame-begin driver. Advanced by [`World::tick`] on the
    /// sim ticks that map to a retail vsync, by [`crate::world::FrameClock::frame_step`] - the
    /// same cadence-invariant clock every other retail duration uses.
    ///
    /// REF: FUN_800267FC, FUN_8001698C
    pub sound_release: crate::sound_state::SoundReleaseTimer,
    /// Set by [`World::tick`] on the frame [`crate::world::AudioState::sound_release`] expires;
    /// hosts drain it with [`World::take_pending_sound_release`] and stop the
    /// bound voice. Retail does the stop inline through libsnd
    /// (`FUN_8002657C` + `FUN_80064370`), which the engine replaces with its
    /// own voice pool - so the port surfaces the *event*, not the teardown.
    pub pending_sound_release: bool,
    /// The five `gp` cells retail's **arm** half writes alongside
    /// [`crate::world::AudioState::sound_release`] (`gp+0x80C`/`0x810`), latched by
    /// [`World::arm_sound_release`]. `None` until the field VM's BGM op
    /// sub-`5` arms the release.
    ///
    /// REF: FUN_800267A8
    pub sound_arm: Option<crate::scus_leaf_kernels::TimedSoundArm>,
    /// The per-slot SFX-cue delay table (`DAT_8007C338`) the field VM's op
    /// `0x36` sub-`4` writes.
    ///
    /// REF: FUN_80035BAC
    pub sfx_cue_delays: crate::scus_leaf_kernels::SfxCueDelays,
    /// The slot the SFX enqueue last parked (`gp+0x15A`) - the index
    /// [`crate::world::AudioState::sfx_cue_delays`] is written through.
    pub sfx_parked_slot: i16,
    /// The enqueue's round-robin write cursor (`gp+0x158`), wrapping at
    /// [`crate::scus_leaf_kernels::SFX_CUE_SLOTS`].
    pub sfx_cue_cursor: i16,
    /// The side-band sound-bank request / acknowledge pair
    /// `_DAT_8007BABC` / `_DAT_8007BAA0`, which op `0x36`'s bit-15 subs
    /// `1` and `2` drive and whose settled state gates sub `0` and the
    /// whole bit-15-clear XA arm.
    ///
    /// REF: FUN_800243F0
    pub sound_stream: crate::scus_leaf_kernels::SoundStreamRequest,
    /// `_DAT_8007B868` - the dev/dual-mode gate. Retail boots it `0` and no
    /// static writer ever sets it non-zero, so the engine keeps it `0`; the
    /// field applies it as retail does (it *skips* op `0x36`'s whole
    /// bit-15-set arm and *bypasses* the bit-15-clear arm's stream barrier).
    pub dual_mode_gate: i32,
    /// The one-shot sound-detach latch (`gp+0x804`). Idempotent: the mode-INIT
    /// chain can call it repeatedly and only the first has any effect.
    ///
    /// REF: FUN_8002689C
    pub sound_detach: crate::sound_state::SoundDetachLatch,
}

impl AudioState {
    pub fn new() -> Self {
        Self {
            sound_bank_ready: true,
            battle_sfx_cues: Vec::new(),
            battle_xa_cues: Vec::new(),
            battle_xa_busy_frames: 0,
            xa_cue_durations: None,
            battle_shout_cues: Vec::new(),
            current_bgm: None,
            battle_bgm: None,
            field_bgm_resume: None,
            battle_bgm_active: false,
            sound_release: crate::sound_state::SoundReleaseTimer::default(),
            pending_sound_release: false,
            sound_arm: None,
            sfx_cue_delays: crate::scus_leaf_kernels::SfxCueDelays::new(
                crate::scus_leaf_kernels::SFX_CUE_SLOTS,
            ),
            sfx_parked_slot: 0,
            sfx_cue_cursor: 0,
            // Retail's field init writes `(8, -1)` (`0x801D6880`) and
            // `FUN_800243F0` latches it settled on the next frame. The
            // engine's bank loads are synchronous - there is no in-flight
            // window - so the pair is born settled instead, the same way the
            // BGM barrier is satisfied on arrival.
            sound_stream: crate::scus_leaf_kernels::SoundStreamRequest::IDLE_PAIR,
            dual_mode_gate: 0,
            sound_detach: crate::sound_state::SoundDetachLatch::default(),
        }
    }
}

impl Default for AudioState {
    fn default() -> Self {
        Self::new()
    }
}

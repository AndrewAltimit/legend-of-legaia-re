//! Audio-trace oracle plumbing shared between the `legaia-engine audio-trace`
//! subcommand and the disc-gated `audio_trace` integration test.
//!
//! Mirrors the shape of [`crate::mode_trace_oracle`] but the diff axis is
//! the SPU's voice-activity state instead of the engine's high-level
//! dispatcher. The retail side lifts a single-frame snapshot from a
//! mednafen save state's `SPU` section; the engine side ticks a
//! [`NativeSession`] and runs a private headless SPU + sequencer in parallel,
//! sampling voice / master / reverb state each frame.
//!
//! **Asymmetry.** Two known mismatches both sides explicitly model:
//!
//! 1. **Headless engine SPU.** [`NativeSession`] only attaches a real cpal
//!    `AudioOut` when `enable_audio = true`, which fails in CI (no audio
//!    device). The oracle constructs a standalone
//!    [`legaia_engine_audio::Spu`] + [`legaia_engine_audio::Sequencer`] in
//!    parallel with the NativeSession's headless tick and routes
//!    scene-resolved BGM events into it via a private
//!    [`TraceBgmDirector`]. This isn't bit-identical to the retail SPU,
//!    but it captures the same voice-activity envelope: which channel
//!    allocations happen when, which voices the sequencer key-ons.
//! 2. **Single retail frame vs. windowed engine.** Save states freeze one
//!    SPU cycle. The engine tick produces `frames + 1` records. The
//!    convergence rule is "at least one engine frame matches retail's
//!    active-voice mask", parallel to [`crate::mode_trace_oracle`]'s
//!    "any engine frame matches retail's `(scene_mode, active_scene)`".
//!
//! JSONL is the wire format - one record per line of `(frame,
//! sequencer_playhead_ticks, sequencer_finished, master_volume,
//! voice_active_mask, voices)`.

use std::path::Path;

use anyhow::{Context, Result};
use legaia_engine_audio::{Sequencer, Spu, SpuAllocator, VabBank};
use legaia_engine_scene_host::scene_host::BgmDirector;
use legaia_seq::Seq;
use serde::{Deserialize, Serialize};

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::BootConfig;

mod divergence;
mod engine_trace;
mod key_on_census;
mod runtime_trace;
#[cfg(test)]
mod tests;
mod trace_director;
mod voice_allocation;

pub use divergence::*;
pub use engine_trace::*;
pub use key_on_census::*;
pub use runtime_trace::*;
pub use trace_director::*;
pub use voice_allocation::*;

/// Number of PSX SPU voices. Mirrors [`legaia_engine_audio::spu::NUM_VOICES`]
/// and [`legaia_mednafen::SPU_NUM_VOICES`]. Re-exported so downstream code
/// doesn't need to depend on engine-audio just to size an array.
pub const NUM_VOICES: usize = 24;

/// One per-voice snapshot.
///
/// Both emitters fill the same fields:
///   - Engine: reads its private [`legaia_engine_audio::Spu`].
///   - Retail: reads [`legaia_mednafen::PsxSpu::voice_state`].
///
/// Mednafen's ADSR phase enum doesn't map 1:1 onto the engine-audio model
/// (mednafen splits Release into multiple sub-phases), so the field carries
/// raw integers. The actionable signal is `active` (phase != Off).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceTraceFrame {
    /// `true` if the voice is producing audible output this cycle.
    pub active: bool,
    /// Voice's start address into SPU RAM. `None` when the voice has never
    /// been programmed (default state).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub start_addr: Option<u32>,
    /// Latched loop-back address.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub loop_addr: Option<u32>,
    /// Pitch register (libspu `0x1000` = unity). Allocator-independent -
    /// unlike `start_addr` this *is* comparable across the two sides, since
    /// it encodes (note, tone centre, tone shift) and nothing about where a
    /// sample landed in SPU RAM.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pitch: Option<u16>,
    /// Live ADSR envelope level, `0..=0x7FFF`. This is the word `active` is
    /// derived from on all three emitters; carrying it too turns "is this
    /// voice sounding" into "how loudly".
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub env_level: Option<u16>,
    /// Per-voice output volume, left. Retail: the `0x1F801Cn0` register
    /// (PCSX ports blob) or mednafen's `Sweep[0].Current`. Engine:
    /// `Voice::vol_left`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub vol_left: Option<i16>,
    /// Per-voice output volume, right.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub vol_right: Option<i16>,
    /// The two libspu ADSR config words packed `adsr1 | adsr2 << 16`. Two
    /// voices carrying the same word were programmed from the same VAB tone,
    /// which is the closest thing the SPU keeps to a program id.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub adsr_control: Option<u32>,
    /// `EON` bit for this voice - `true` when its output feeds the reverb
    /// tank.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reverb_send: Option<bool>,
    /// Running count of key-ons this voice has taken (engine:
    /// `Voice::key_on_count`). The difference between two frames is the
    /// exact number of key-ons in between, including a re-key of a voice
    /// that was still sounding - which the `active` edge cannot see. `None`
    /// on both retail emitters: an SPU snapshot keeps no key-on history (the
    /// retail count comes from `scripts/pcsx-redux/autorun_keyon_census.lua`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub key_ons: Option<u32>,
}

/// One sample of the SPU's voice-activity state.
///
/// Like [`legaia_engine_session::mode_trace::ModeTraceFrame`], fields the sampler
/// can't fill are `None` rather than zeroed so downstream diff tools can
/// distinguish "didn't observe" from "observed 0".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioTraceFrame {
    /// Frame counter. Engine: wall-clock frame from [`NativeSession::frames`].
    /// Retail: always 0 (save states are single-frame).
    pub frame: u64,
    /// Sequencer playhead in PPQN ticks. Engine: from
    /// [`legaia_engine_audio::Sequencer::playhead_ticks`]. Retail: `None`
    /// (the SPU section doesn't carry sequencer state - that lives in the
    /// CPU-side libsnd workspace, only reachable via an external Lua probe).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sequencer_playhead_ticks: Option<u64>,
    /// `true` if the engine's sequencer has run off the end (no looping).
    /// Retail: `None`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sequencer_finished: Option<bool>,
    /// Master volume `(left, right)`. Engine: from-scratch model's
    /// `master_left/right`. Retail: mednafen's
    /// `(GlobalSweep[0/1]).Current` accumulator.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub master_volume: Option<(i16, i16)>,
    /// libspu reverb **mode** selector (`0 = Off`, `4 = Studio C`, ...).
    /// Engine-only: `Spu::reverb_mode_raw`. Neither retail emitter can fill
    /// it - the hardware keeps no mode number, only the 32 coefficient
    /// registers a mode expands to - so it is `None` on both retail paths.
    ///
    /// **This field used to carry three different quantities under one
    /// name**: the engine's mode byte, mednafen's `Reverb_Mode` sub-entry
    /// (which is really `EON`), and the PCSX-Redux extractor's read of
    /// SPU offset `0x1AA` (which is really `SPUCNT`). Comparing them
    /// produced the "engine 0 vs retail 0xC081, so retail routes voices 0, 7,
    /// 14 and 15" reading. The three quantities now have three fields:
    /// [`Self::reverb_eon`], [`Self::spu_control`] and this one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reverb_mode: Option<u32>,
    /// Per-voice reverb-enable mask (`EON`, SPU `0x1F801D98`/`0x9A`), 24
    /// significant bits. Engine: derived from `Voice::reverb_send`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reverb_eon: Option<u32>,
    /// Reverb output depth `(vLOUT, vROUT)` - SPU `0x1F801D84`/`0x86`, what
    /// libspu `SpuSetReverbDepth` writes. Not part of a mode preset.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reverb_depth: Option<(i16, i16)>,
    /// Reverb work-area base in bytes (`mBASE * 8`). The work-area size is
    /// `0x80000 - base`, which identifies the preset independently of the
    /// coefficient block.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reverb_work_area: Option<u32>,
    /// `SPUCNT` (SPU `0x1F801DAA`): bit 15 SPU enable, bit 14 unmute, bit 7
    /// reverb master enable, bit 0 CD audio. Retail-only - the engine models
    /// no control register.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub spu_control: Option<u16>,
    /// 24-bit mask: bit N set iff voice N is `active`. Convergence-axis
    /// shorthand for the per-voice array; the array is canonical, the mask
    /// is for fast comparison and human-readable diffs.
    pub active_voice_mask: u32,
    /// Per-voice state. Indexed 0..[`NUM_VOICES`].
    pub voices: Vec<VoiceTraceFrame>,
    /// Engine only: the SFX cue ids the frame's producers wrote into the
    /// ring (`World::take_sfx_ring_ops` - pushes, replaces and direct slot
    /// stores, in order). The census side is the retail drainer's cue
    /// key-ons (`autorun_keyon_census.lua`, `cuekey` rows).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub sfx_ring: Vec<i16>,
}

impl AudioTraceFrame {
    /// Empty frame at a given frame index - all voices off, no master
    /// volume, no sequencer. Useful as the engine's pre-boot record.
    pub fn quiescent(frame: u64) -> Self {
        Self {
            frame,
            sequencer_playhead_ticks: None,
            sequencer_finished: None,
            master_volume: None,
            reverb_mode: None,
            reverb_eon: None,
            reverb_depth: None,
            reverb_work_area: None,
            spu_control: None,
            sfx_ring: Vec::new(),
            active_voice_mask: 0,
            voices: vec![VoiceTraceFrame::default(); NUM_VOICES],
        }
    }
}

/// Configuration for [`build_engine_audio_trace`].
#[derive(Debug, Clone)]
pub struct AudioTraceBuildOptions {
    /// CDNAME scene name.
    pub scene: String,
    /// Optional BGM id started against the private sequencer before the
    /// trace loop begins. `None` lets the field VM drive playback through
    /// the [`TraceBgmDirector`] - the usual case. Most scenes' prescripts
    /// emit op `0x35` (BGM start) within the first few frames, so the
    /// retail-active mask materialises naturally; pass `Some(id)` only for
    /// scenes whose prescripts don't kick off audio or for manual
    /// override.
    pub bgm_id: Option<u16>,
    /// Microseconds per engine frame. 60 Hz default (16_666.67).
    pub us_per_frame: f64,
    /// Number of frames to tick. Output has `frames + 1` records.
    pub frames: u64,
    /// Keep [`Self::bgm_id`] playing: the scene's own op-`0x35` BGM events
    /// are drained and dropped instead of switching the track. For pairing
    /// with a retail capture whose story state selected a different track
    /// than the port's free-roam staging does.
    pub pin_bgm: bool,
}

impl Default for AudioTraceBuildOptions {
    fn default() -> Self {
        Self {
            scene: legaia_engine_session::session::DEFAULT_BOOT_SCENE.to_string(),
            bgm_id: None,
            us_per_frame: 1_000_000.0 / 60.0,
            frames: 60,
            pin_bgm: false,
        }
    }
}

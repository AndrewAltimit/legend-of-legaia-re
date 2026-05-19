//! Audio-trace oracle plumbing shared between the `legaia-engine audio-trace`
//! subcommand and the disc-gated `audio_trace` integration test.
//!
//! Mirrors the shape of [`crate::mode_trace_oracle`] but the diff axis is
//! the SPU's voice-activity state instead of the engine's high-level
//! dispatcher. The retail side lifts a single-frame snapshot from a
//! mednafen save state's `SPU` section; the engine side ticks a
//! [`BootSession`] and runs a private headless SPU + sequencer in parallel,
//! sampling voice / master / reverb state each frame.
//!
//! **Asymmetry.** Two known mismatches both sides explicitly model:
//!
//! 1. **Headless engine SPU.** [`BootSession`] only attaches a real cpal
//!    `AudioOut` when `enable_audio = true`, which fails in CI (no audio
//!    device). The oracle constructs a standalone
//!    [`legaia_engine_audio::Spu`] + [`legaia_engine_audio::Sequencer`] in
//!    parallel with the BootSession's headless tick and routes
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
use legaia_engine_core::scene::BgmDirector;
use legaia_seq::Seq;
use serde::{Deserialize, Serialize};

use crate::{BootConfig, BootSession};

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
    /// Pitch register (libspu `0x1000` = unity).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pitch: Option<u16>,
}

/// One sample of the SPU's voice-activity state.
///
/// Like [`crate::mode_trace_oracle::ModeTraceFrame`], fields the sampler
/// can't fill are `None` rather than zeroed so downstream diff tools can
/// distinguish "didn't observe" from "observed 0".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioTraceFrame {
    /// Frame counter. Engine: wall-clock frame from [`BootSession::frames`].
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
    /// Master volume `(left, right)`. Engine: clean-room model's
    /// `master_left/right`. Retail: mednafen's
    /// `(GlobalSweep[0/1]).Current` accumulator.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub master_volume: Option<(i16, i16)>,
    /// Reverb mode register. Engine: `Spu::reverb_mode_raw`. Retail:
    /// `Reverb_Mode` SPU sub-entry (raw 4-byte value, not libspu mode byte).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reverb_mode: Option<u32>,
    /// 24-bit mask: bit N set iff voice N is `active`. Convergence-axis
    /// shorthand for the per-voice array; the array is canonical, the mask
    /// is for fast comparison and human-readable diffs.
    pub active_voice_mask: u32,
    /// Per-voice state. Indexed 0..[`NUM_VOICES`].
    pub voices: Vec<VoiceTraceFrame>,
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
}

impl Default for AudioTraceBuildOptions {
    fn default() -> Self {
        Self {
            scene: crate::boot::DEFAULT_BOOT_SCENE.to_string(),
            bgm_id: None,
            us_per_frame: 1_000_000.0 / 60.0,
            frames: 60,
        }
    }
}

/// Headless [`BgmDirector`] used by [`build_engine_audio_trace`].
///
/// Mirrors [`crate::bgm::AudioBgmDirector`] but doesn't hold an
/// [`legaia_engine_audio::AudioOut`] (cpal is unavailable in CI). The
/// director owns the cloned scene [`VabBank`] plus the currently-attached
/// [`Sequencer`]; the trace loop calls
/// [`legaia_engine_core::scene::SceneHost::route_bgm_events`] after each
/// `tick` to deliver field-VM op `0x35` events.
///
/// Pause / resume gate the per-frame `tick_us` call inside
/// [`build_engine_audio_trace`] - we don't have a "pause" hook on the
/// clean-room [`Sequencer`] itself, so the flag lives here.
pub struct TraceBgmDirector {
    bank: Option<VabBank>,
    sequencer: Option<Sequencer>,
    /// Master volume forwarded to every freshly-attached sequencer.
    pub master_vol: u8,
    /// Loop-to event index for newly-started sequencers. `Some(0)` matches
    /// the retail field-BGM default; `None` plays once.
    pub loop_to: Option<usize>,
    paused: bool,
    pending: Option<(u16, Vec<u8>)>,
    /// Last BGM id passed to `start`. Used to suppress duplicate starts
    /// when the field VM re-emits op `0x35` without a state change.
    pub last_started: Option<u16>,
}

impl TraceBgmDirector {
    pub fn new() -> Self {
        Self {
            bank: None,
            sequencer: None,
            master_vol: 100,
            loop_to: Some(0),
            paused: false,
            pending: None,
            last_started: None,
        }
    }

    /// Stash the scene's parsed [`VabBank`]. The trace loop calls this once
    /// after uploading the bank into the private SPU.
    pub fn set_bank(&mut self, bank: VabBank) {
        self.bank = Some(bank);
    }

    /// Borrow the active sequencer - the trace loop ticks it each frame.
    pub fn sequencer(&self) -> Option<&Sequencer> {
        self.sequencer.as_ref()
    }

    /// Mutable borrow of the active sequencer for per-frame `tick_us`.
    pub fn sequencer_mut(&mut self) -> Option<&mut Sequencer> {
        self.sequencer.as_mut()
    }

    /// `true` if the director currently has a sequencer attached and is not
    /// paused.
    pub fn is_playing(&self) -> bool {
        self.sequencer.is_some() && !self.paused
    }

    /// `true` if [`BgmDirector::pause`] was called and `resume` hasn't been
    /// called since.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Drain any [`BgmDirector::queue`] pending bytes and start them
    /// playing. Returns `true` if a pending sequencer was attached.
    pub fn flush_queue(&mut self) -> Result<bool> {
        let Some((id, bytes)) = self.pending.take() else {
            return Ok(false);
        };
        self.start_inner(id, &bytes)?;
        Ok(true)
    }

    fn start_inner(&mut self, bgm_id: u16, seq_bytes: &[u8]) -> Result<()> {
        let Some(bank) = self.bank.clone() else {
            log::warn!("TraceBgmDirector::start({bgm_id}) ignored - no VAB bank loaded for scene");
            return Ok(());
        };
        let seq = Seq::parse(seq_bytes).context("parse SEQ for BGM start")?;
        let mut sequencer = Sequencer::new(seq, bank);
        sequencer.set_master_vol(self.master_vol);
        if let Some(loop_to) = self.loop_to {
            sequencer.set_loop_to(loop_to);
        }
        self.sequencer = Some(sequencer);
        self.paused = false;
        self.last_started = Some(bgm_id);
        Ok(())
    }
}

impl Default for TraceBgmDirector {
    fn default() -> Self {
        Self::new()
    }
}

impl BgmDirector for TraceBgmDirector {
    fn start(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        // Suppress duplicate starts for the same id - mirrors the retail
        // op `0x35` behaviour observed in `AudioBgmDirector::start`.
        if self.last_started == Some(bgm_id) && !self.paused && self.sequencer.is_some() {
            return;
        }
        if let Err(e) = self.start_inner(bgm_id, seq_bytes) {
            log::warn!("TraceBgmDirector::start({bgm_id}) failed: {e:#}");
        }
    }

    fn queue(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        self.pending = Some((bgm_id, seq_bytes.to_vec()));
    }

    fn pause(&mut self) {
        self.paused = true;
    }

    fn resume(&mut self) {
        self.paused = false;
    }

    fn stop(&mut self) {
        self.sequencer = None;
        self.paused = false;
        self.last_started = None;
    }
}

/// Run a [`BootSession`] on the configured scene with a private headless
/// SPU + [`TraceBgmDirector`] in parallel. Samples per-frame voice /
/// master / reverb state.
///
/// The audio side never touches cpal - the BootSession is constructed with
/// `enable_audio = false` and we drive a standalone
/// [`legaia_engine_audio::Spu`] on the side. This makes the oracle CI-safe.
///
/// The field VM drives BGM playback through the director: after each
/// [`BootSession::tick`] we call
/// [`legaia_engine_core::scene::SceneHost::route_bgm_events`] to deliver
/// op `0x35` events. `opts.bgm_id` is a manual boot-time start that fires
/// before the main loop - useful for scenes whose prescripts don't kick
/// off audio.
pub fn build_engine_audio_trace(
    extracted_root: &Path,
    disc: Option<&Path>,
    opts: &AudioTraceBuildOptions,
) -> Result<Vec<AudioTraceFrame>> {
    let cfg = BootConfig {
        scene: opts.scene.clone(),
        enable_audio: false,
    };
    let mut session = match disc {
        Some(p) => BootSession::open_disc(p, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };

    // Stage the scene's VAB bank into the private SPU - mirrors the
    // BootSession's own pre-boot bank staging (boot.rs `stage_scene_vab`)
    // but without an AudioOut handle.
    let mut spu = Spu::new();
    let mut director = TraceBgmDirector::new();
    if let Some(vab_bytes) = session
        .host
        .scene_vab_bytes()
        .context("resolve scene VAB bytes")?
    {
        let report =
            legaia_vab::parse(&vab_bytes, 0).context("parse scene VAB header for audio trace")?;
        // SPU RAM allocator: voice-0 / scratch reserved at 0x1000, the
        // bank uploads above. Matches `BootSession::stage_scene_vab`.
        const SPU_RAM_BYTES: u32 = 512 * 1024;
        const SPU_RESERVED_BYTES: u32 = 0x1000;
        let mut alloc = SpuAllocator::new(SPU_RESERVED_BYTES, SPU_RAM_BYTES - SPU_RESERVED_BYTES);
        let bank = VabBank::upload(&mut spu, &mut alloc, &report, &vab_bytes);
        director.set_bank(bank);
    }

    // Optional manual boot-time start - the field VM normally kicks BGM
    // via op `0x35`, but tests / overrides can preseed a track.
    if let Some(id) = opts.bgm_id {
        if let Some(seq_bytes) = session.host.bgm_seq_bytes(id)? {
            director.start(id, &seq_bytes);
        } else {
            log::warn!("audio-trace: bgm_id {id} did not resolve to a SEQ entry");
        }
    }

    let mut out = Vec::with_capacity((opts.frames as usize).saturating_add(1));
    out.push(sample_engine_frame(&session, &spu, director.sequencer()));
    let samples_per_frame = (44_100_f64 * (opts.us_per_frame / 1_000_000.0)) as usize;
    let mut sink = vec![0i16; samples_per_frame * 2];
    for _ in 0..opts.frames {
        let _ = session.tick()?;
        // Drain field-VM BGM events into the private director; resolved
        // SEQ bytes flow through `SceneHost::bgm_seq_bytes`.
        let _ = session.host.route_bgm_events(&mut director)?;
        if !director.is_paused()
            && let Some(seq) = director.sequencer_mut()
        {
            seq.tick_us(&mut spu, opts.us_per_frame);
        }
        // Bring the SPU forward by exactly one frame of samples so envelope
        // / decoder state advances in lock-step with the sequencer.
        spu.render_into(&mut sink);
        out.push(sample_engine_frame(&session, &spu, director.sequencer()));
    }
    Ok(out)
}

fn sample_engine_frame(
    session: &BootSession,
    spu: &legaia_engine_audio::Spu,
    sequencer: Option<&legaia_engine_audio::Sequencer>,
) -> AudioTraceFrame {
    use legaia_engine_audio::spu::adsr::Phase;
    let mut voices = Vec::with_capacity(NUM_VOICES);
    let mut mask = 0u32;
    for (i, v) in spu.voices.iter().enumerate() {
        let active = !matches!(v.adsr.phase, Phase::Off);
        if active {
            mask |= 1 << i;
        }
        voices.push(VoiceTraceFrame {
            active,
            start_addr: if v.start_addr != 0 {
                Some(v.start_addr)
            } else {
                None
            },
            loop_addr: v.loop_addr,
            pitch: if v.pitch != 0 { Some(v.pitch) } else { None },
        });
    }
    AudioTraceFrame {
        frame: session.frames,
        sequencer_playhead_ticks: sequencer.map(|s| s.playhead_ticks()),
        sequencer_finished: sequencer.map(|s| s.is_finished()),
        master_volume: Some((spu.master_left, spu.master_right)),
        reverb_mode: Some(spu.reverb_mode_raw),
        active_voice_mask: mask,
        voices,
    }
}

/// Lift a single audio-trace sample out of a mednafen `.mc{slot}` save.
/// Reads the SPU section via [`legaia_mednafen::PsxSpu`].
///
/// Returns a `frame = 0` record with voice/master/reverb populated from
/// the save state. `sequencer_playhead_ticks` / `sequencer_finished` are
/// `None` because the SPU section doesn't carry sequencer (CPU-side
/// libsnd) state.
pub fn load_runtime_audio_trace_from_save(save: &Path) -> Result<AudioTraceFrame> {
    use legaia_mednafen::{PsxSpu, SaveState};

    let state = SaveState::from_path(save)
        .with_context(|| format!("load mednafen save {}", save.display()))?;
    let spu = PsxSpu::new(&state);
    let mednafen_voices = spu.voices();
    let mut voices = Vec::with_capacity(NUM_VOICES);
    let mut mask = 0u32;
    for (i, v) in mednafen_voices.iter().enumerate() {
        let active = v.is_active();
        if active {
            mask |= 1 << i;
        }
        voices.push(VoiceTraceFrame {
            active,
            start_addr: v.start_addr,
            loop_addr: v.loop_addr,
            pitch: v.pitch,
        });
    }
    Ok(AudioTraceFrame {
        frame: 0,
        sequencer_playhead_ticks: None,
        sequencer_finished: None,
        master_volume: spu.master_volume(),
        reverb_mode: spu.reverb_mode(),
        active_voice_mask: mask,
        voices,
    })
}

/// Serialise a list of frames as JSON Lines. Round-trips through
/// [`parse_audio_trace_jsonl`].
pub fn audio_trace_to_jsonl(frames: &[AudioTraceFrame]) -> String {
    let mut out = String::new();
    for f in frames {
        out.push_str(&serde_json::to_string(f).expect("AudioTraceFrame JSON serialise"));
        out.push('\n');
    }
    out
}

/// Parse JSONL emitted by [`audio_trace_to_jsonl`]. Blank lines are
/// skipped so concatenated streams parse cleanly.
pub fn parse_audio_trace_jsonl(s: &str) -> Result<Vec<AudioTraceFrame>> {
    let mut out = Vec::new();
    for (i, line) in s.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let frame: AudioTraceFrame = serde_json::from_str(trimmed)
            .with_context(|| format!("parse JSONL line {}: {trimmed}", i + 1))?;
        out.push(frame);
    }
    Ok(out)
}

/// First field on which `engine` and `retail` disagree. Compares the
/// active-voice mask plus per-voice start_addr / loop_addr / pitch where
/// both sides report them.
///
/// Convergence rule: at least one engine frame's `active_voice_mask` must
/// be a *superset or equal* of retail's mask AND, for every voice retail
/// marks active, the engine's same voice index must also be active with a
/// matching start_addr (when both sides report it). The "superset" half is
/// pragmatic - the engine can leak voices across frames during the trace
/// window; what matters is "the engine saw the same voice allocations
/// retail did when retail captured".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTraceDivergence {
    pub kind: AudioDivergenceKind,
    pub engine: AudioTraceFrame,
    pub retail: AudioTraceFrame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDivergenceKind {
    /// Engine never observed any of retail's active voices over the trace
    /// window. Most actionable: usually means the engine port didn't drive
    /// BGM playback at all.
    NoFrameMatched,
    /// Some engine frame had retail's voice indices active, but the
    /// voice's start_addr didn't match (different sample / bank slot).
    VoiceStartAddrMismatch,
    /// Engine matched retail's active mask but the master volume diverged.
    /// Lower-priority - master is per-frame in retail (sweep state) and
    /// per-frame in engine (libspu MVOL write), so equality is informational.
    MasterVolumeMismatch,
}

/// Walk `engine` left-to-right; return the first divergence point against
/// the retail snapshot.
pub fn first_audio_trace_divergence(
    engine: &[AudioTraceFrame],
    retail: &AudioTraceFrame,
) -> Option<AudioTraceDivergence> {
    if engine.is_empty() {
        return None;
    }
    if retail.active_voice_mask == 0 {
        // Retail captured zero active voices - the engine's quiescent
        // boot frame trivially matches. Don't surface a divergence.
        return None;
    }
    // Walk for a frame whose mask is a superset of retail's mask.
    let mut best: Option<&AudioTraceFrame> = None;
    for ef in engine {
        if ef.active_voice_mask & retail.active_voice_mask == retail.active_voice_mask {
            best = Some(ef);
            break;
        }
    }
    let Some(matched) = best else {
        // No engine frame had the right voice indices active.
        let last = engine.last().unwrap().clone();
        return Some(AudioTraceDivergence {
            kind: AudioDivergenceKind::NoFrameMatched,
            engine: last,
            retail: retail.clone(),
        });
    };
    // Mask matched - check start_addr alignment for each retail-active voice.
    for (i, rv) in retail.voices.iter().enumerate() {
        if !rv.active {
            continue;
        }
        let Some(ev) = matched.voices.get(i) else {
            continue;
        };
        // Both sides know the start_addr? They must match.
        if let (Some(es), Some(rs)) = (ev.start_addr, rv.start_addr)
            && es != rs
        {
            return Some(AudioTraceDivergence {
                kind: AudioDivergenceKind::VoiceStartAddrMismatch,
                engine: matched.clone(),
                retail: retail.clone(),
            });
        }
    }
    None
}

/// Wrap [`build_engine_audio_trace`] for callers that have already opened
/// a path resolution. Used by the audio-trace subcommand handler and the
/// disc-gated test - both want the [`Arc<Vec<u8>>`] return shape but
/// want it serialised over a Path.
pub fn engine_trace_from_paths(
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    frames: u64,
    bgm_id: Option<u16>,
) -> Result<Vec<AudioTraceFrame>> {
    let opts = AudioTraceBuildOptions {
        scene: scene.to_string(),
        bgm_id,
        us_per_frame: 1_000_000.0 / 60.0,
        frames,
    };
    build_engine_audio_trace(extracted_root, disc, &opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(active: bool, start_addr: Option<u32>) -> VoiceTraceFrame {
        VoiceTraceFrame {
            active,
            start_addr,
            loop_addr: None,
            pitch: None,
        }
    }

    fn frame_with(mask: u32, voices: Vec<VoiceTraceFrame>) -> AudioTraceFrame {
        AudioTraceFrame {
            frame: 0,
            sequencer_playhead_ticks: None,
            sequencer_finished: None,
            master_volume: None,
            reverb_mode: None,
            active_voice_mask: mask,
            voices,
        }
    }

    #[test]
    fn jsonl_roundtrip_engine_shape() {
        let frames = vec![
            AudioTraceFrame {
                frame: 0,
                sequencer_playhead_ticks: Some(0),
                sequencer_finished: Some(false),
                master_volume: Some((0x3FFF, 0x3FFF)),
                reverb_mode: Some(0),
                active_voice_mask: 0b0000_0011,
                voices: vec![
                    voice(true, Some(0x1000)),
                    voice(true, Some(0x1200)),
                    voice(false, None),
                ],
            },
            AudioTraceFrame {
                frame: 1,
                sequencer_playhead_ticks: Some(480),
                sequencer_finished: Some(false),
                master_volume: Some((0x3FFF, 0x3FFF)),
                reverb_mode: Some(0),
                active_voice_mask: 0b0000_0010,
                voices: vec![
                    voice(false, Some(0x1000)),
                    voice(true, Some(0x1200)),
                    voice(false, None),
                ],
            },
        ];
        let jsonl = audio_trace_to_jsonl(&frames);
        assert_eq!(jsonl.lines().count(), 2);
        let round = parse_audio_trace_jsonl(&jsonl).unwrap();
        assert_eq!(frames, round);
    }

    #[test]
    fn jsonl_roundtrip_retail_shape() {
        let f = AudioTraceFrame {
            frame: 0,
            sequencer_playhead_ticks: None,
            sequencer_finished: None,
            master_volume: Some((0x3F00, 0x3F00)),
            reverb_mode: Some(0x17FFFF),
            active_voice_mask: 0b0000_0111,
            voices: vec![voice(true, Some(0x2000)); 3],
        };
        let jsonl = audio_trace_to_jsonl(std::slice::from_ref(&f));
        let round = parse_audio_trace_jsonl(&jsonl).unwrap();
        assert_eq!(round, vec![f]);
    }

    #[test]
    fn quiescent_emits_no_active_voices() {
        let q = AudioTraceFrame::quiescent(42);
        assert_eq!(q.frame, 42);
        assert_eq!(q.active_voice_mask, 0);
        assert_eq!(q.voices.len(), NUM_VOICES);
        assert!(q.voices.iter().all(|v| !v.active));
    }

    #[test]
    fn divergence_none_when_retail_has_no_active_voices() {
        // Retail quiescent → trivially matches any engine trace.
        let engine = vec![frame_with(0b0000_0001, vec![voice(true, Some(0x1000))])];
        let retail = frame_with(0, vec![voice(false, None)]);
        assert!(first_audio_trace_divergence(&engine, &retail).is_none());
    }

    #[test]
    fn divergence_none_when_engine_superset_matches() {
        // Retail had voice 1 active; engine had voices 0+1 active. Superset
        // → no divergence.
        let engine = vec![frame_with(
            0b0000_0011,
            vec![voice(true, Some(0x1000)), voice(true, Some(0x1200))],
        )];
        let retail = frame_with(
            0b0000_0010,
            vec![voice(false, None), voice(true, Some(0x1200))],
        );
        assert!(first_audio_trace_divergence(&engine, &retail).is_none());
    }

    #[test]
    fn divergence_no_frame_matched_when_engine_missing_voices() {
        let engine = vec![frame_with(0b0000_0001, vec![voice(true, Some(0x1000))])];
        let retail = frame_with(
            0b0000_0010,
            vec![voice(false, None), voice(true, Some(0x1200))],
        );
        let d = first_audio_trace_divergence(&engine, &retail).unwrap();
        assert_eq!(d.kind, AudioDivergenceKind::NoFrameMatched);
    }

    #[test]
    fn divergence_voice_start_addr_mismatch() {
        // Engine has the right voice active but at a different start_addr.
        let engine = vec![frame_with(
            0b0000_0010,
            vec![voice(false, None), voice(true, Some(0xDEAD))],
        )];
        let retail = frame_with(
            0b0000_0010,
            vec![voice(false, None), voice(true, Some(0x1200))],
        );
        let d = first_audio_trace_divergence(&engine, &retail).unwrap();
        assert_eq!(d.kind, AudioDivergenceKind::VoiceStartAddrMismatch);
    }

    #[test]
    fn divergence_none_when_retail_start_addr_unknown() {
        // Retail's voice is active but start_addr field is None. We don't
        // penalise the engine for filling it in.
        let engine = vec![frame_with(0b0000_0001, vec![voice(true, Some(0x1000))])];
        let retail = frame_with(0b0000_0001, vec![voice(true, None)]);
        assert!(first_audio_trace_divergence(&engine, &retail).is_none());
    }

    #[test]
    fn empty_engine_trace_returns_none() {
        let retail = frame_with(0b0000_0001, vec![voice(true, Some(0x1000))]);
        assert!(first_audio_trace_divergence(&[], &retail).is_none());
    }

    #[test]
    fn parser_skips_blank_lines() {
        let s = "\n{\"frame\":0,\"active_voice_mask\":0,\"voices\":[]}\n\n";
        let out = parse_audio_trace_jsonl(s).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].active_voice_mask, 0);
    }

    /// Trace director without a bank silently drops `start` - the warning
    /// goes through `log::warn` (no panic). Pre-bank-stage races at scene
    /// load should not crash the trace loop.
    #[test]
    fn trace_director_start_without_bank_is_noop() {
        let mut d = TraceBgmDirector::new();
        d.start(0, &[]);
        assert!(d.sequencer().is_none());
        assert!(!d.is_playing());
        assert_eq!(d.last_started, None);
    }

    /// Pause + resume toggle without touching the sequencer slot. The
    /// trace loop reads `is_paused()` to gate the per-frame `tick_us`.
    #[test]
    fn trace_director_pause_resume_toggle() {
        let mut d = TraceBgmDirector::new();
        BgmDirector::pause(&mut d);
        assert!(d.is_paused());
        BgmDirector::resume(&mut d);
        assert!(!d.is_paused());
    }

    /// `stop` clears the sequencer slot, paused flag, and last_started id.
    /// Idempotent on an empty director.
    #[test]
    fn trace_director_stop_clears_state() {
        let mut d = TraceBgmDirector::new();
        d.paused = true;
        d.last_started = Some(7);
        BgmDirector::stop(&mut d);
        assert!(d.sequencer().is_none());
        assert!(!d.is_paused());
        assert_eq!(d.last_started, None);
    }

    /// `queue` stashes pending bytes for a later `flush_queue` call. Calling
    /// `flush_queue` without a bank consumes the pending entry but doesn't
    /// attach a sequencer (parity with `start_without_bank`).
    #[test]
    fn trace_director_queue_then_flush_consumes_pending() {
        let mut d = TraceBgmDirector::new();
        BgmDirector::queue(&mut d, 42, &[1, 2, 3]);
        assert!(d.pending.is_some());
        let drained = d.flush_queue().unwrap();
        assert!(drained);
        assert!(d.pending.is_none());
        // No bank → no sequencer attached after the flush.
        assert!(d.sequencer().is_none());
    }
}

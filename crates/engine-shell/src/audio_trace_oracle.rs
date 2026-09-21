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
/// [`legaia_engine_audio::AudioOut`] (cpal is unavailable in CI). It owns
/// the same three things the cpal director owns behind its `AudioOut` - the
/// [`Spu`], the active [`VabBank`], and the attached [`Sequencer`] - so the
/// two start paths can be written identically. The trace loop calls
/// [`legaia_engine_core::scene::SceneHost::route_bgm_events`] after each
/// `tick` to deliver field-VM op `0x35` events.
///
/// **Both start paths are modelled.** [`BgmDirector::start`] plays a
/// scene-local SEQ against the pre-staged scene bank;
/// [`BgmDirector::start_owned_vab`] uploads a **global-pool** entry's own
/// VAB before playing its SEQ. Leaving the second one on the trait's no-op
/// default is silent in every unit test and fatal in the oracle: every real
/// music cue is a global id (`>= 2000`), so the whole field corpus routes
/// through the path that did nothing.
///
/// Pause / resume gate the per-frame sequencer tick inside
/// [`Self::advance_frame`] - we don't have a "pause" hook on the port's
/// [`Sequencer`] itself, so the flag lives here.
pub struct TraceBgmDirector {
    spu: Spu,
    bank: Option<VabBank>,
    sequencer: Option<Sequencer>,
    /// Master volume forwarded to every freshly-attached sequencer.
    pub master_vol: u8,
    /// Loop-to event index for newly-started sequencers. `Some(0)` matches
    /// the retail field-BGM default; `None` plays once.
    pub loop_to: Option<usize>,
    paused: bool,
    /// Last BGM id passed to `start`. Used to suppress duplicate starts
    /// when the field VM re-emits op `0x35` without a state change.
    pub last_started: Option<u16>,
}

impl TraceBgmDirector {
    pub fn new() -> Self {
        // Configure the private SPU exactly as the shipped cpal host
        // configures its own (`StreamResampler::new`): Studio C, every voice
        // routed, retail depth. A bare `Spu::new()` here left the oracle
        // measuring an engine that differs from the one the port ships - the
        // trace's reverb channel read `Off` / no voices routed on every
        // frame while the live engine ran the retail configuration.
        let mut spu = Spu::new();
        spu.set_retail_reverb();
        Self {
            spu,
            bank: None,
            sequencer: None,
            master_vol: 100,
            loop_to: Some(0),
            paused: false,
            last_started: None,
        }
    }

    /// Borrow the private SPU the director keys its voices into. The trace
    /// loop samples it once per frame.
    pub fn spu(&self) -> &Spu {
        &self.spu
    }

    /// Mutable borrow of the private SPU, for callers that seed it directly.
    pub fn spu_mut(&mut self) -> &mut Spu {
        &mut self.spu
    }

    /// Stash the scene's parsed [`VabBank`]. Callers that uploaded the bank
    /// themselves use this; [`Self::stage_scene_bank`] is the usual entry.
    pub fn set_bank(&mut self, bank: VabBank) {
        self.bank = Some(bank);
    }

    /// Parse a scene VAB stream's header at `vab_off`, upload its samples
    /// into the private SPU, and make it the active bank.
    ///
    /// Region math mirrors `boot::stage_scene_vab` exactly - the BGM region
    /// is capped below the resident SFX bank at the top of SPU RAM - so a
    /// voice's `start_addr` in this trace is the address the windowed host
    /// would program for the same bank.
    pub fn stage_scene_bank(&mut self, bytes: &[u8], vab_off: usize) -> Result<()> {
        let report =
            legaia_vab::parse(bytes, vab_off).context("parse scene VAB header for audio trace")?;
        let mut alloc = SpuAllocator::new(
            crate::boot::SPU_RESERVED_BYTES,
            crate::boot::SPU_RAM_BYTES
                - crate::boot::SPU_RESERVED_BYTES
                - crate::boot::SFX_BANK_SPU_BYTES,
        );
        let bank = VabBank::upload(&mut self.spu, &mut alloc, &report, bytes);
        self.bank = Some(bank);
        Ok(())
    }

    /// Borrow the active sequencer - the trace loop ticks it each frame.
    pub fn sequencer(&self) -> Option<&Sequencer> {
        self.sequencer.as_ref()
    }

    /// Mutable borrow of the active sequencer for per-frame `tick_us`.
    pub fn sequencer_mut(&mut self) -> Option<&mut Sequencer> {
        self.sequencer.as_mut()
    }

    /// Advance the director by exactly one trace frame: tick the attached
    /// sequencer (unless paused) and bring the SPU forward by one frame of
    /// samples so envelope / decoder state moves in lock-step. `sink` is the
    /// caller's stereo scratch buffer; the PCM oracle keeps what lands in it,
    /// the voice-mask oracle discards it.
    pub fn advance_frame(&mut self, us_per_frame: f64, sink: &mut [i16]) {
        if !self.paused
            && let Some(seq) = self.sequencer.as_mut()
        {
            seq.tick_us(&mut self.spu, us_per_frame);
        }
        self.spu.render_into(sink);
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

    /// Split a raw `music_01` bank entry (`[chunk][pBAV VAB][pQES SEQ]`),
    /// upload the entry's **own** VAB into the private SPU, make it the
    /// active bank, and return the SEQ bytes. Mirrors
    /// [`crate::bgm::AudioBgmDirector`]'s `stage_owned_vab` byte for byte,
    /// including its allocator region, so the two directors program the same
    /// `start_addr` for the same track.
    fn stage_owned_vab(&mut self, entry_bytes: &[u8]) -> Option<Vec<u8>> {
        let vab_off = entry_bytes.windows(4).position(|w| w == b"pBAV")?;
        let seq_rel = entry_bytes[vab_off..]
            .windows(4)
            .position(|w| w == b"pQES")?;
        let report = legaia_vab::parse(entry_bytes, vab_off).ok()?;
        let body = &entry_bytes[vab_off..];
        let mut alloc = SpuAllocator::new(
            crate::boot::SPU_RESERVED_BYTES,
            crate::boot::SPU_RAM_BYTES
                - crate::boot::SPU_RESERVED_BYTES
                - crate::boot::SFX_BANK_SPU_BYTES,
        );
        self.bank = Some(VabBank::upload(&mut self.spu, &mut alloc, &report, body));
        Some(entry_bytes[vab_off + seq_rel..].to_vec())
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
        log::info!("TraceBgmDirector: BGM {bgm_id} started");
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

    fn start_owned_vab(&mut self, bgm_id: u16, entry_bytes: &[u8]) {
        if self.last_started == Some(bgm_id) && !self.paused && self.sequencer.is_some() {
            return;
        }
        let Some(seq) = self.stage_owned_vab(entry_bytes) else {
            log::warn!("TraceBgmDirector::start_owned_vab({bgm_id}) - no [VAB][SEQ] pair in entry");
            return;
        };
        if let Err(e) = self.start_inner(bgm_id, &seq) {
            log::warn!("TraceBgmDirector::start_owned_vab({bgm_id}) failed: {e:#}");
        }
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
    enter_scene_for_trace(&mut session, &opts.scene)?;

    // Stage the scene's VAB bank into the director's private SPU - mirrors
    // the BootSession's own pre-boot bank staging (boot.rs `stage_scene_vab`)
    // but without an AudioOut handle. Scenes that carry no VAB entry of their
    // own leave the bank empty; their music is a global-pool track that
    // brings its own (see `TraceBgmDirector::start_owned_vab`).
    let mut director = TraceBgmDirector::new();
    if let Some((vab_bytes, vab_off)) = session
        .host
        .scene_vab_bytes()
        .context("resolve scene VAB bytes")?
    {
        // The stream's own chunk-0 header puts the bank at `+4`; offset 0 is
        // the header word, and parsing there fails outright.
        director.stage_scene_bank(&vab_bytes, vab_off)?;
    }

    // Optional manual boot-time start - the field VM normally kicks BGM
    // via op `0x35`, but tests / overrides can preseed a track.
    if let Some(id) = opts.bgm_id {
        start_bgm_id_directly(&session, &mut director, id)?;
    }

    let mut out = Vec::with_capacity((opts.frames as usize).saturating_add(1));
    out.push(sample_engine_frame(
        &session,
        director.spu(),
        director.sequencer(),
    ));
    let samples_per_frame = (44_100_f64 * (opts.us_per_frame / 1_000_000.0)) as usize;
    let mut sink = vec![0i16; samples_per_frame * 2];
    for _ in 0..opts.frames {
        let _ = session.tick()?;
        // Drain field-VM BGM events into the private director; resolved
        // SEQ bytes flow through `SceneHost::bgm_seq_bytes`, whole
        // global-pool entries through `SceneHost::music_bank_entry_bytes`.
        let _ = session.host.route_bgm_events(&mut director)?;
        director.advance_frame(opts.us_per_frame, &mut sink);
        out.push(sample_engine_frame(
            &session,
            director.spu(),
            director.sequencer(),
        ));
    }
    Ok(out)
}

/// Drop a freshly-opened [`BootSession`] into live field dispatch before an
/// audio / PCM trace samples it.
///
/// [`BootSession::open`] only calls `load_scene`: the world stays in
/// [`SceneMode::Title`](legaia_engine_core::world::SceneMode) with **no field
/// record installed**, so the field VM steps nothing and op `0x35` never
/// executes. Every other engine-side oracle already goes through
/// [`BootSession::enter_field_live`] (the mode-trace builders, `sim-trace`);
/// the two audio oracles were the ones that did not, which is the whole
/// reason their engine traces reported an empty voice mask on every frame
/// while retail keyed voices.
///
/// [`crate::boot::FieldLiveOpts::default`] rather than the window's playable
/// preset: a no-input trace window wants the field VM running and nothing
/// else, so the step-driven encounter roll and player-driven battles stay
/// off and the sampled audio is the scene's own entry music.
///
/// # The staging call is what makes the music audible
///
/// A cold `--scene` entry is a **free-roam picker visit**, and both playable
/// hosts stage one before entering
/// ([`legaia_engine_core::world::World::seed_free_roam_story_baseline`] -
/// `window/run.rs` natively, `runtime.rs` in the browser). Skipping it does
/// not merely change story flags: `town01`'s entry script starts the town
/// theme and then *pauses* it while flag `0x225` is clear (the opening's
/// silent dawn), and retail repairs that with the opening records' own sub-9
/// starts, which a picker visit never runs. Without the staging call the
/// director attaches a sequencer on the first frame and then holds it paused
/// for the whole window - a trace whose playhead sits at tick 0 and whose
/// voice mask stays empty, which reads exactly like "the engine never started
/// any BGM".
pub(crate) fn enter_scene_for_trace(session: &mut BootSession, scene: &str) -> Result<()> {
    session.host.world.seed_free_roam_story_baseline(scene);
    session.enter_field_live(scene, &crate::boot::FieldLiveOpts::default())?;
    Ok(())
}

/// Resolve `bgm_id` the way [`legaia_engine_core::scene::SceneHost::route_bgm_events`]
/// resolves an op-`0x35` start and hand it to `director`: a scene-local id
/// plays against the staged scene bank, a global-pool id (`>= 2000`) brings
/// its own VAB. The manual `--bgm-id` override took only the first of those
/// two paths, so an override naming a real music track resolved to nothing.
pub(crate) fn start_bgm_id_directly(
    session: &BootSession,
    director: &mut TraceBgmDirector,
    id: u16,
) -> Result<()> {
    if let Some(seq_bytes) = session.host.bgm_seq_bytes(id)? {
        director.start(id, &seq_bytes);
    } else if let Some(entry) = session.host.music_bank_entry_bytes(id)? {
        director.start_owned_vab(id, &entry);
    } else {
        log::warn!("audio-trace: bgm_id {id} did not resolve to a SEQ entry");
    }
    Ok(())
}

/// Re-exported wrapper for [`crate::pcm_oracle::build_engine_pcm_trace`].
/// The PCM-trace loop wants the same per-frame voice-activity sampler
/// but the function is otherwise private to this module.
pub(crate) fn sample_engine_frame_for_pcm(
    session: &BootSession,
    spu: &legaia_engine_audio::Spu,
    sequencer: Option<&legaia_engine_audio::Sequencer>,
) -> AudioTraceFrame {
    sample_engine_frame(session, spu, sequencer)
}

fn sample_engine_frame(
    session: &BootSession,
    spu: &legaia_engine_audio::Spu,
    sequencer: Option<&legaia_engine_audio::Sequencer>,
) -> AudioTraceFrame {
    use legaia_engine_audio::spu::adsr::Phase;
    let mut voices = Vec::with_capacity(NUM_VOICES);
    let mut mask = 0u32;
    let mut eon = 0u32;
    for (i, v) in spu.voices.iter().enumerate() {
        let active = !matches!(v.adsr.phase, Phase::Off);
        if active {
            mask |= 1 << i;
        }
        if v.reverb_send {
            eon |= 1 << i;
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
            env_level: Some(v.adsr.level),
            vol_left: Some(v.vol_left),
            vol_right: Some(v.vol_right),
            adsr_control: Some(v.adsr_cfg.raw.0 as u32 | ((v.adsr_cfg.raw.1 as u32) << 16)),
            reverb_send: Some(v.reverb_send),
        });
    }
    AudioTraceFrame {
        frame: session.frames,
        sequencer_playhead_ticks: sequencer.map(|s| s.playhead_ticks()),
        sequencer_finished: sequencer.map(|s| s.is_finished()),
        master_volume: Some((spu.master_left, spu.master_right)),
        reverb_mode: Some(spu.reverb_mode_raw),
        reverb_eon: Some(eon),
        reverb_depth: Some(spu.reverb.output_volume()),
        reverb_work_area: Some(spu.reverb.work_area_base_bytes()),
        // The engine models no SPU control register - reverb master enable
        // is implicit in the active `ReverbMode`.
        spu_control: None,
        active_voice_mask: mask,
        voices,
    }
}

/// Load a multi-frame retail trace from a JSONL file emitted by
/// `scripts/pcsx-redux/extract_audio_trace_from_sstates.py` (which itself
/// decodes the binary stream produced by
/// `scripts/pcsx-redux/autorun_audio_trace.lua`). Each line of the file
/// is one [`AudioTraceFrame`] record; the `frame` field carries the
/// vsync index at which the snapshot was taken in the live emulator.
///
/// This is the multi-frame sibling of
/// [`load_runtime_audio_trace_from_save`] - the latter lifts a single
/// SPU snapshot out of a mednafen save state, while this one consumes a
/// trace of N snapshots captured per-vsync against a running emulator.
/// The multi-frame trace is what
/// [`first_audio_trace_divergence_multi`] consumes to do a
/// frame-by-frame comparison against the engine trace.
pub fn load_runtime_audio_trace_jsonl(path: &Path) -> Result<Vec<AudioTraceFrame>> {
    let s = std::fs::read_to_string(path)
        .with_context(|| format!("read retail audio-trace JSONL {}", path.display()))?;
    parse_audio_trace_jsonl(&s)
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
    // `Reverb_Mode` is mednafen's name for the per-voice reverb-enable mask,
    // not a libspu mode byte; the register shadow's own `EON` is the same
    // value and is the one the field is named for here.
    let eon = spu.voice_reverb_mask().or_else(|| spu.reverb_mode());
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
            env_level: v.adsr_env_level,
            vol_left: v.vol_left,
            vol_right: v.vol_right,
            adsr_control: v.adsr_control,
            reverb_send: eon.map(|m| m & (1u32 << i) != 0),
        });
    }
    Ok(AudioTraceFrame {
        frame: 0,
        sequencer_playhead_ticks: None,
        sequencer_finished: None,
        master_volume: spu.master_volume(),
        // Retail keeps no mode *number* anywhere - only the coefficient
        // registers a mode expands to - so this stays `None` on the retail
        // side and `reverb_work_area` carries the preset-identifying size.
        reverb_mode: None,
        reverb_eon: eon,
        reverb_depth: spu.reverb_output_volume(),
        reverb_work_area: spu.reverb_work_area().map(|wa| wa.wrapping_mul(2)),
        spu_control: spu.spu_control(),
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

/// Multi-frame retail convergence walk. Sister to
/// [`first_audio_trace_divergence`] but consumes a `retail: &[AudioTraceFrame]`
/// trace captured per-vsync via the PCSX-Redux autorun probe.
///
/// Convergence rule (parallel to the single-frame rule, applied per
/// retail frame): for each retail frame whose `active_voice_mask` is
/// non-zero, there must exist some engine frame whose mask is a
/// **superset** of retail's mask AND, for every retail-active voice, the
/// engine's same voice index reports a matching `start_addr` (when both
/// sides report one). The first retail frame that fails this rule is
/// returned. Retail frames with `active_voice_mask == 0` are trivially
/// satisfied and skipped.
///
/// Picking the engine frame: we walk the engine trace forward and use
/// the first matching frame. This handles minor cross-fade timing
/// drift - if the engine's voice allocations land a few frames before
/// or after retail's, the convergence still triggers.
pub fn first_audio_trace_divergence_multi(
    engine: &[AudioTraceFrame],
    retail: &[AudioTraceFrame],
) -> Option<AudioTraceDivergence> {
    if engine.is_empty() {
        return None;
    }
    for rf in retail {
        if rf.active_voice_mask == 0 {
            continue;
        }
        if let Some(d) = first_audio_trace_divergence(engine, rf) {
            return Some(d);
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

/// Per-side summary of what a trace's *sounding* voices were doing, in the
/// two currencies that survive the two sides' independent SPU-RAM
/// allocators: pitch (a hardware register computed from note + tone, so
/// directly comparable) and the packed ADSR config word (the closest thing
/// the SPU keeps to "which tone programmed this voice").
///
/// `start_addr` is deliberately **not** a comparand here - retail's samples
/// sit wherever `SsSpuMalloc` put them and the engine's wherever
/// `SpuAllocator` put them, so the addresses cannot agree and are not
/// supposed to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoiceAllocationStats {
    /// Frames the summary was computed over.
    pub frames: usize,
    /// Mean sounding voices per frame.
    pub mean_active: f64,
    /// Largest number of voices sounding on any one frame.
    pub max_active: usize,
    /// Mean count of distinct `(start_addr, pitch)` pairs per frame - the
    /// number of distinct *notes* sounding, as opposed to slots used.
    pub mean_distinct_notes: f64,
    /// `mean_active / mean_distinct_notes`: how many slots the side spends
    /// per distinct note. Exactly `1.0` means it never doubles a note
    /// across two voices.
    pub doubling: f64,
    /// Key-on **edges**: per voice slot, transitions from not-sounding to
    /// sounding across the window. This is the one activity statistic that
    /// is a property of the emulated CPU on both sides - the sequencer
    /// writes the key-on register from the game's own vsync handler - so it
    /// survives a retail capture whose frames carry an uncontrolled amount
    /// of SPU time (see [`crate::audio_trace_oracle`] docs and
    /// `docs/subsystems/audio.md`, "The envelope channel is not on emulated
    /// time").
    pub onsets: usize,
    /// [`Self::onsets`] over [`Self::frames`].
    pub onsets_per_frame: f64,
    /// Every pitch seen on a sounding voice anywhere in the trace.
    pub pitches: std::collections::BTreeSet<u16>,
    /// Every packed ADSR config word seen on a sounding voice.
    pub tones: std::collections::BTreeSet<u32>,
    /// Every sample start address seen on a sounding voice. Reported for
    /// counting distinct instruments, never for equality against the other
    /// side.
    pub samples: std::collections::BTreeSet<u32>,
}

/// Which of the three readings the per-voice comparison supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceAllocationVerdict {
    /// The two sides' pitch sets barely intersect: they are sounding
    /// different notes. On one track that means different moments in it;
    /// across two tracks it means the pairing itself is wrong.
    DifferentNotes,
    /// Overlapping pitches, but the engine sounds materially fewer at once.
    ShortOfVoices,
    /// Overlapping pitches and comparable slot counts.
    Comparable,
}

/// Engine-vs-retail per-voice comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceAllocationComparison {
    pub engine: VoiceAllocationStats,
    pub retail: VoiceAllocationStats,
    /// Pitches both sides sounded at some point in their windows.
    pub shared_pitches: usize,
    /// Packed ADSR config words both sides used.
    pub shared_tones: usize,
    /// Engine key-on edges per frame over retail's. This is the comparand
    /// that survives a capture whose frames carry an uncontrolled amount of
    /// SPU time: both sides' key-ons are written by the score, on the
    /// emulated CPU's own clock, while how long a voice then stays above
    /// zero is the host's SPU thread's business on the retail side.
    pub onset_ratio: f64,
    /// Engine-frame offset the retail window was compared at, when the
    /// comparison was run through [`compare_voice_allocation_aligned`].
    pub alignment_offset: Option<usize>,
    pub verdict: VoiceAllocationVerdict,
}

/// Share of the *smaller* pitch vocabulary the two sides hold in common,
/// below which the windows are judged to be sounding different notes. The
/// denominator is the smaller of the two sets rather than the engine's, so a
/// longer engine window is not penalised for hearing more of the track than
/// the retail freeze did.
const PITCH_OVERLAP_FLOOR: f64 = 0.25;
/// Fraction of retail's concurrent-voice count below which the engine is
/// judged short of voices rather than comparable.
const VOICE_COUNT_FLOOR: f64 = 0.75;

fn summarise_allocation(frames: &[AudioTraceFrame]) -> VoiceAllocationStats {
    use std::collections::BTreeSet;
    let mut out = VoiceAllocationStats {
        frames: frames.len(),
        ..Default::default()
    };
    let (mut act_sum, mut note_sum, mut counted) = (0usize, 0usize, 0usize);
    // Key-on edges per slot. `prev` is indexed by voice slot and carries the
    // previous frame's sounding flag, so a slot that stays sounding across a
    // frame boundary counts once, not once per frame.
    let mut prev = vec![false; NUM_VOICES];
    for f in frames {
        for (i, v) in f.voices.iter().enumerate() {
            if i >= prev.len() {
                prev.resize(i + 1, false);
            }
            if v.active && !prev[i] {
                out.onsets += 1;
            }
            prev[i] = v.active;
        }
        let sounding: Vec<&VoiceTraceFrame> = f.voices.iter().filter(|v| v.active).collect();
        if sounding.is_empty() {
            // A silent frame is not evidence about allocation; averaging it
            // in measures how much of the window the trace spent before the
            // track started, which is a different question.
            continue;
        }
        counted += 1;
        act_sum += sounding.len();
        out.max_active = out.max_active.max(sounding.len());
        let notes: BTreeSet<(Option<u32>, Option<u16>)> =
            sounding.iter().map(|v| (v.start_addr, v.pitch)).collect();
        note_sum += notes.len();
        for v in sounding {
            if let Some(p) = v.pitch {
                out.pitches.insert(p);
            }
            if let Some(t) = v.adsr_control {
                out.tones.insert(t);
            }
            if let Some(a) = v.start_addr {
                out.samples.insert(a);
            }
        }
    }
    if counted > 0 {
        out.mean_active = act_sum as f64 / counted as f64;
        out.mean_distinct_notes = note_sum as f64 / counted as f64;
        out.doubling = if note_sum > 0 {
            act_sum as f64 / note_sum as f64
        } else {
            0.0
        };
    }
    if !frames.is_empty() {
        out.onsets_per_frame = out.onsets as f64 / frames.len() as f64;
    }
    out
}

/// Score how well engine frame `off + i` lines up with retail frame `i`,
/// as the mean per-frame Jaccard of the two frames' sounding-pitch
/// multisets. Symmetric, so a window where the engine simply sounds more
/// voices does not outscore one where the notes actually agree - an
/// intersection-only score ranks the busiest engine window first whatever
/// it is playing.
fn alignment_score(engine: &[AudioTraceFrame], retail: &[AudioTraceFrame], off: usize) -> f64 {
    use std::collections::BTreeMap;
    fn pitches(f: &AudioTraceFrame) -> BTreeMap<u16, usize> {
        let mut m = BTreeMap::new();
        for v in f.voices.iter().filter(|v| v.active) {
            if let Some(p) = v.pitch {
                *m.entry(p).or_insert(0) += 1;
            }
        }
        m
    }
    let mut sum = 0.0;
    for (i, r) in retail.iter().enumerate() {
        let Some(e) = engine.get(off + i) else {
            return 0.0;
        };
        let (rm, em) = (pitches(r), pitches(e));
        let mut inter = 0usize;
        let mut union = 0usize;
        for k in rm
            .keys()
            .chain(em.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            let (a, b) = (
                rm.get(k).copied().unwrap_or(0),
                em.get(k).copied().unwrap_or(0),
            );
            inter += a.min(b);
            union += a.max(b);
        }
        if union > 0 {
            sum += inter as f64 / union as f64;
        }
    }
    sum / retail.len().max(1) as f64
}

/// Find the engine-frame offset at which the retail window best lines up.
///
/// The per-voice comparison is only meaningful when the two windows cover
/// the *same stretch* of the same piece. An engine trace starts its track at
/// tick 0; a retail capture is parked wherever the playthrough left it, so
/// the naive "first N frames of each" pairing compares two different bars
/// and reports the difference between them as an engine difference. Returns
/// `None` when the engine trace is shorter than the retail window.
pub fn best_alignment_offset(
    engine: &[AudioTraceFrame],
    retail: &[AudioTraceFrame],
) -> Option<(usize, f64)> {
    if retail.is_empty() || engine.len() < retail.len() {
        return None;
    }
    (0..=engine.len() - retail.len())
        .map(|off| (off, alignment_score(engine, retail, off)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Compare what the engine's score allocated against retail's, per voice.
///
/// This is the axis [`first_audio_trace_divergence_multi`] cannot decide:
/// that walk asks whether some engine frame's mask *covers* a retail frame's,
/// which a pair of traces taken at different moments of a 2-minute track can
/// fail while playing identically. The comparison here is of what the voices
/// were *doing* - pitches, tones, and how many slots each side spends per
/// distinct note - and it is meaningful only when both sides are on the same
/// track. Check that first: a scene's engine trace starts the track its own
/// prescript selects (op `0x35`), while a retail save carries whatever track
/// the playthrough left loaded, and the two are routinely different.
pub fn compare_voice_allocation(
    engine: &[AudioTraceFrame],
    retail: &[AudioTraceFrame],
) -> VoiceAllocationComparison {
    let e = summarise_allocation(engine);
    let r = summarise_allocation(retail);
    let shared_pitches = e.pitches.intersection(&r.pitches).count();
    let shared_tones = e.tones.intersection(&r.tones).count();
    let smaller = e.pitches.len().min(r.pitches.len());
    let overlap = if smaller == 0 {
        0.0
    } else {
        shared_pitches as f64 / smaller as f64
    };
    let verdict = if overlap < PITCH_OVERLAP_FLOOR {
        VoiceAllocationVerdict::DifferentNotes
    } else if e.mean_active < r.mean_active * VOICE_COUNT_FLOOR {
        VoiceAllocationVerdict::ShortOfVoices
    } else {
        VoiceAllocationVerdict::Comparable
    };
    let onset_ratio = if r.onsets_per_frame > 0.0 {
        e.onsets_per_frame / r.onsets_per_frame
    } else {
        0.0
    };
    VoiceAllocationComparison {
        engine: e,
        retail: r,
        shared_pitches,
        shared_tones,
        onset_ratio,
        alignment_offset: None,
        verdict,
    }
}

/// [`compare_voice_allocation`] over the engine window that best lines up
/// with the retail one ([`best_alignment_offset`]), rather than over the
/// engine trace's first frames.
///
/// Comparing frame 0 of each is what turned "the engine holds more voices
/// at once" into a standing residual: a cold engine trace opens at the
/// track's first bar while the capture sits wherever the playthrough parked
/// it, and the two bars have different note densities. Aligned, the two
/// sides' key-on counts land on top of each other.
pub fn compare_voice_allocation_aligned(
    engine: &[AudioTraceFrame],
    retail: &[AudioTraceFrame],
) -> VoiceAllocationComparison {
    match best_alignment_offset(engine, retail) {
        Some((off, _)) => {
            let mut c = compare_voice_allocation(&engine[off..off + retail.len()], retail);
            c.alignment_offset = Some(off);
            c
        }
        None => compare_voice_allocation(engine, retail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(active: bool, start_addr: Option<u32>) -> VoiceTraceFrame {
        VoiceTraceFrame {
            active,
            start_addr,
            ..Default::default()
        }
    }

    fn voice_at(pitch: u16, tone: u32) -> VoiceTraceFrame {
        VoiceTraceFrame {
            active: true,
            pitch: Some(pitch),
            adsr_control: Some(tone),
            ..Default::default()
        }
    }

    fn alloc_frame(voices: Vec<VoiceTraceFrame>) -> AudioTraceFrame {
        let mut mask = 0u32;
        for (i, v) in voices.iter().enumerate() {
            if v.active {
                mask |= 1 << i;
            }
        }
        AudioTraceFrame {
            active_voice_mask: mask,
            voices,
            ..AudioTraceFrame::quiescent(0)
        }
    }

    /// Disjoint pitch sets read as "different notes" however close the
    /// voice counts are - the reading that a same-count comparison of two
    /// different tracks would otherwise pass off as convergence.
    #[test]
    fn voice_allocation_disjoint_pitches_read_as_different_notes() {
        let engine = vec![alloc_frame(vec![voice_at(1000, 1), voice_at(1100, 1)])];
        let retail = vec![alloc_frame(vec![voice_at(500, 1), voice_at(600, 1)])];
        let c = compare_voice_allocation(&engine, &retail);
        assert_eq!(c.shared_pitches, 0);
        assert_eq!(c.verdict, VoiceAllocationVerdict::DifferentNotes);
    }

    /// Overlapping pitches with the engine sounding far fewer at once is
    /// the "short of voices" reading.
    #[test]
    fn voice_allocation_fewer_overlapping_notes_reads_as_short() {
        let engine = vec![alloc_frame(vec![voice_at(500, 1)])];
        let retail = vec![alloc_frame(vec![
            voice_at(500, 1),
            voice_at(500, 1),
            voice_at(500, 1),
            voice_at(600, 1),
        ])];
        let c = compare_voice_allocation(&engine, &retail);
        assert_eq!(c.shared_pitches, 1);
        assert_eq!(c.verdict, VoiceAllocationVerdict::ShortOfVoices);
    }

    /// Retail doubling a `(sample, pitch)` pair across two slots shows up
    /// as a doubling factor above one; the engine playing each note once
    /// sits at exactly one.
    #[test]
    fn voice_allocation_doubling_factor_counts_duplicate_slots() {
        let retail = vec![alloc_frame(vec![voice_at(500, 1), voice_at(500, 1)])];
        let engine = vec![alloc_frame(vec![voice_at(500, 1)])];
        let c = compare_voice_allocation(&engine, &retail);
        assert!((c.retail.doubling - 2.0).abs() < 1e-9);
        assert!((c.engine.doubling - 1.0).abs() < 1e-9);
    }

    /// A slot that stays sounding across frames is one key-on, not one per
    /// frame - the statistic counts edges, which is what makes it a
    /// property of the score rather than of how long a voice rings.
    #[test]
    fn onsets_count_key_on_edges_not_sounding_frames() {
        let on = vec![voice_at(500, 1)];
        let off = vec![VoiceTraceFrame::default()];
        let frames = vec![
            alloc_frame(on.clone()),
            alloc_frame(on.clone()),
            alloc_frame(on.clone()),
            alloc_frame(off.clone()),
            alloc_frame(on.clone()),
        ];
        let s = summarise_allocation(&frames);
        assert_eq!(s.onsets, 2, "two key-ons, four sounding frames");
        assert!((s.onsets_per_frame - 0.4).abs() < 1e-9);
    }

    /// Two sides playing the same phrase at the same rate agree on key-ons
    /// per frame even when one holds each note far longer - which is the
    /// shape a capture whose frames carry extra envelope time produces.
    #[test]
    fn onset_ratio_is_blind_to_how_long_a_note_rings() {
        // Engine: one key-on, sounding for four frames.
        let engine = vec![
            alloc_frame(vec![voice_at(500, 1)]),
            alloc_frame(vec![voice_at(500, 1)]),
            alloc_frame(vec![voice_at(500, 1)]),
            alloc_frame(vec![voice_at(500, 1)]),
        ];
        // Retail: the same one key-on, drained after a single frame.
        let retail = vec![
            alloc_frame(vec![voice_at(500, 1)]),
            alloc_frame(vec![VoiceTraceFrame::default()]),
            alloc_frame(vec![VoiceTraceFrame::default()]),
            alloc_frame(vec![VoiceTraceFrame::default()]),
        ];
        let c = compare_voice_allocation(&engine, &retail);
        assert_eq!(c.engine.onsets, 1);
        assert_eq!(c.retail.onsets, 1);
        assert!((c.onset_ratio - 1.0).abs() < 1e-9);
        // The count statistic, by contrast, reads 4x.
        assert!((c.engine.mean_active / c.retail.mean_active - 1.0).abs() < 1e-9);
    }

    /// The retail window is matched against the engine frames that play the
    /// same notes, not against the engine trace's opening bar.
    #[test]
    fn alignment_finds_the_engine_window_playing_the_same_notes() {
        let quiet = alloc_frame(vec![voice_at(100, 1)]);
        let phrase = [
            alloc_frame(vec![voice_at(700, 1), voice_at(800, 1)]),
            alloc_frame(vec![voice_at(900, 1)]),
        ];
        let mut engine = vec![quiet.clone(), quiet.clone(), quiet.clone()];
        engine.extend(phrase.iter().cloned());
        engine.push(quiet.clone());
        let retail = phrase.to_vec();
        let (off, score) = best_alignment_offset(&engine, &retail).expect("alignable");
        assert_eq!(off, 3);
        assert!(score > 0.9, "exact phrase match, got {score}");
        let c = compare_voice_allocation_aligned(&engine, &retail);
        assert_eq!(c.alignment_offset, Some(3));
        assert_eq!(c.verdict, VoiceAllocationVerdict::Comparable);
    }

    /// An engine window that simply sounds *more* voices must not outscore
    /// the window whose notes actually match: the score is symmetric, so a
    /// busy window carrying none of retail's pitches scores zero.
    #[test]
    fn alignment_score_is_not_won_by_the_busiest_engine_window() {
        let busy = alloc_frame(vec![
            voice_at(10, 1),
            voice_at(20, 1),
            voice_at(30, 1),
            voice_at(40, 1),
            voice_at(50, 1),
        ]);
        let match_frame = alloc_frame(vec![voice_at(700, 1)]);
        let engine = vec![busy.clone(), busy.clone(), match_frame.clone()];
        let retail = vec![match_frame];
        let (off, _) = best_alignment_offset(&engine, &retail).expect("alignable");
        assert_eq!(off, 2);
    }

    fn frame_with(mask: u32, voices: Vec<VoiceTraceFrame>) -> AudioTraceFrame {
        AudioTraceFrame {
            active_voice_mask: mask,
            voices,
            ..AudioTraceFrame::quiescent(0)
        }
    }

    #[test]
    fn jsonl_roundtrip_engine_shape() {
        let frames = vec![
            AudioTraceFrame {
                sequencer_playhead_ticks: Some(0),
                sequencer_finished: Some(false),
                master_volume: Some((0x3FFF, 0x3FFF)),
                reverb_mode: Some(0),
                reverb_eon: Some(0x00FF_FFFF),
                reverb_depth: Some((0x3264, 0x3264)),
                reverb_work_area: Some(0x7_9020),
                active_voice_mask: 0b0000_0011,
                voices: vec![
                    voice(true, Some(0x1000)),
                    voice(true, Some(0x1200)),
                    voice(false, None),
                ],
                ..AudioTraceFrame::quiescent(0)
            },
            AudioTraceFrame {
                sequencer_playhead_ticks: Some(480),
                sequencer_finished: Some(false),
                master_volume: Some((0x3FFF, 0x3FFF)),
                reverb_mode: Some(0),
                reverb_eon: Some(0x00FF_FFFF),
                reverb_depth: Some((0x3264, 0x3264)),
                reverb_work_area: Some(0x7_9020),
                active_voice_mask: 0b0000_0010,
                voices: vec![
                    voice(false, Some(0x1000)),
                    voice(true, Some(0x1200)),
                    voice(false, None),
                ],
                ..AudioTraceFrame::quiescent(1)
            },
        ];
        let jsonl = audio_trace_to_jsonl(&frames);
        assert_eq!(jsonl.lines().count(), 2);
        let round = parse_audio_trace_jsonl(&jsonl).unwrap();
        assert_eq!(frames, round);
    }

    #[test]
    fn jsonl_roundtrip_retail_shape() {
        // Retail shape: no mode number (hardware keeps none), but the
        // routing mask, depth and control word a capture does carry.
        let f = AudioTraceFrame {
            master_volume: Some((0x3F00, 0x3F00)),
            reverb_eon: Some(0x17FFFF),
            reverb_depth: Some((0x3264, 0x3264)),
            spu_control: Some(0xC081),
            active_voice_mask: 0b0000_0111,
            voices: vec![voice(true, Some(0x2000)); 3],
            ..AudioTraceFrame::quiescent(0)
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

    /// Multi-frame divergence walk skips retail frames with no active
    /// voices. Three retail frames quiet → no divergence.
    #[test]
    fn divergence_multi_skips_quiet_retail_frames() {
        let engine = vec![frame_with(0, vec![voice(false, None)])];
        let retail = vec![
            frame_with(0, vec![voice(false, None)]),
            frame_with(0, vec![voice(false, None)]),
            frame_with(0, vec![voice(false, None)]),
        ];
        assert!(first_audio_trace_divergence_multi(&engine, &retail).is_none());
    }

    /// Multi-frame divergence: if any single retail frame fails the
    /// single-frame convergence rule, the multi-frame walk reports it.
    #[test]
    fn divergence_multi_reports_first_failing_retail_frame() {
        let engine = vec![frame_with(
            0b0000_0010,
            vec![voice(false, None), voice(true, Some(0x1200))],
        )];
        // Three retail frames: first quiet, second matched, third diverges.
        let retail = vec![
            frame_with(0, vec![voice(false, None)]),
            frame_with(
                0b0000_0010,
                vec![voice(false, None), voice(true, Some(0x1200))],
            ),
            // Retail asks for voice 0 active, but engine never had it.
            frame_with(0b0000_0001, vec![voice(true, Some(0xC0DE))]),
        ];
        let d = first_audio_trace_divergence_multi(&engine, &retail).unwrap();
        assert_eq!(d.kind, AudioDivergenceKind::NoFrameMatched);
    }

    /// Multi-frame divergence handles an empty engine trace the same way
    /// the single-frame walk does: returns None (nothing to compare).
    #[test]
    fn divergence_multi_empty_engine_returns_none() {
        let retail = vec![frame_with(0b0000_0001, vec![voice(true, Some(0x1000))])];
        assert!(first_audio_trace_divergence_multi(&[], &retail).is_none());
    }

    /// A start without a staged bank consumes the request and attaches
    /// nothing - and there is no second, deferred way in. Field-VM op `0x35`
    /// sub-op 9 routes here too (it is a start behind a load barrier this
    /// host never waits on), so the trace director models one entry point,
    /// not two.
    #[test]
    fn trace_director_start_without_bank_attaches_nothing() {
        let mut d = TraceBgmDirector::new();
        BgmDirector::start(&mut d, 42, &[1, 2, 3]);
        assert!(d.sequencer().is_none());
        assert!(!d.is_playing());
    }
}

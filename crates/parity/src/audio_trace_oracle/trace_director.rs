//! The oracle's recording BGM director: a [`BgmDirector`] that owns its own
//! SPU and sequencer so the engine side of the trace can be sampled a frame at
//! a time.

use super::*;

/// Headless [`BgmDirector`] used by [`build_engine_audio_trace`].
///
/// Mirrors [`legaia_engine_session::AudioDirector`] but doesn't hold an
/// [`legaia_engine_audio::AudioOut`] (cpal is unavailable in CI). It owns
/// the same three things the cpal director owns behind its `AudioOut` - the
/// [`Spu`], the active [`VabBank`], and the attached [`Sequencer`] - so the
/// two start paths can be written identically. The trace loop calls
/// [`legaia_engine_scene_host::scene_host::SceneHost::route_bgm_events`] after each
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
    pub(super) spu: Spu,
    pub(super) bank: Option<VabBank>,
    pub(super) sequencer: Option<Sequencer>,
    /// Master volume forwarded to every freshly-attached sequencer.
    pub master_vol: u8,
    /// Loop-to event index for newly-started sequencers. `Some(0)` matches
    /// the retail field-BGM default; `None` plays once.
    pub loop_to: Option<usize>,
    pub(super) paused: bool,
    /// Last BGM id passed to `start`. Used to suppress duplicate starts
    /// when the field VM re-emits op `0x35` without a state change.
    pub last_started: Option<u16>,
}

impl TraceBgmDirector {
    pub fn new() -> Self {
        // Configure the private SPU exactly as the shipped cpal host
        // configures its own (`StreamResampler::new`): Studio C at the retail
        // depth, with each voice's send set per keyed tone (`mode & 4`). A bare `Spu::new()` here left the oracle
        // measuring an engine that differs from the one the port ships - the
        // trace's reverb channel read `Off` / no voices routed on every
        // frame while the live engine ran the retail configuration.
        let mut spu = Spu::new();
        spu.set_retail_reverb();
        Self {
            spu,
            bank: None,
            sequencer: None,
            master_vol: legaia_engine_audio::sequencer::RETAIL_BGM_SEQ_VOL,
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
    /// Region math mirrors the play hosts' BGM region exactly - the BGM region
    /// is capped below the resident SFX bank at the top of SPU RAM - so a
    /// voice's `start_addr` in this trace is the address the windowed host
    /// would program for the same bank.
    pub fn stage_scene_bank(&mut self, bytes: &[u8], vab_off: usize) -> Result<()> {
        let report =
            legaia_vab::parse(bytes, vab_off).context("parse scene VAB header for audio trace")?;
        let mut alloc = SpuAllocator::new(
            legaia_engine_session::session::SPU_RESERVED_BYTES,
            legaia_engine_session::session::SPU_RAM_BYTES
                - legaia_engine_session::session::SPU_RESERVED_BYTES
                - legaia_engine_session::session::SFX_BANK_SPU_BYTES,
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
    /// [`legaia_engine_session::AudioDirector`]'s `stage_owned_vab` byte for byte,
    /// including its allocator region, so the two directors program the same
    /// `start_addr` for the same track.
    pub(super) fn stage_owned_vab(&mut self, entry_bytes: &[u8]) -> Option<Vec<u8>> {
        // The installer walk's split (type-0 bank, type-2 score), not a
        // magic hunt - see `chunk_install::owned_bank_offsets`.
        let split = legaia_engine_system::chunk_install::owned_bank_offsets(entry_bytes)?;
        let vab_off = split.vab;
        let report = legaia_vab::parse(entry_bytes, vab_off).ok()?;
        let body = &entry_bytes[vab_off..];
        // The shared placement kernel, so a bank over the BGM region (the
        // ending theme's) lands where the live director puts it.
        let staged =
            legaia_engine_audio::spu_layout::upload_owned_bank(&mut self.spu, &report, body);
        self.bank = Some(staged.bank);
        Some(entry_bytes[split.seq..].to_vec())
    }

    pub(super) fn start_inner(&mut self, bgm_id: u16, seq_bytes: &[u8]) -> Result<()> {
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
        // op `0x35` behaviour observed in `NativeAudioDirector::start`.
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

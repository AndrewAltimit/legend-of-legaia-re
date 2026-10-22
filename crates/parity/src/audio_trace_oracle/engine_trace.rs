//! The engine side of the audio trace: boot a session, enter the scene,
//! start its BGM and sample one [`AudioTraceFrame`] per frame.

use super::*;

/// Run a [`NativeSession`] on the configured scene with a private headless
/// SPU + [`TraceBgmDirector`] in parallel. Samples per-frame voice /
/// master / reverb state.
///
/// The audio side never touches cpal - the NativeSession is constructed with
/// `enable_audio = false` and we drive a standalone
/// [`legaia_engine_audio::Spu`] on the side. This makes the oracle CI-safe.
///
/// The field VM drives BGM playback through the director: after each
/// [`NativeSession::tick`] we call
/// [`legaia_engine_scene_host::scene_host::SceneHost::route_bgm_events`] to deliver
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
        Some(p) => NativeSession::open_disc(p, &cfg)?,
        None => NativeSession::open(extracted_root, &cfg)?,
    };
    enter_scene_for_trace(&mut session, &opts.scene)?;

    // Stage the scene's VAB bank into the director's private SPU, for this
    // oracle's own scene-local sweep (the play hosts stage no scene bank:
    // retail loads a bank only with its track). Scenes that carry no VAB entry of their
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
    // Sink for the scene's own BGM events under `pin_bgm`.
    let mut discard = TraceBgmDirector::new();
    for _ in 0..opts.frames {
        let _ = session.tick()?;
        // Drain field-VM BGM events into the private director; resolved
        // SEQ bytes flow through `SceneHost::bgm_seq_bytes`, whole
        // global-pool entries through `SceneHost::music_bank_entry_bytes`.
        if opts.pin_bgm {
            let _ = session.host.route_bgm_events(&mut discard)?;
        } else {
            let _ = session.host.route_bgm_events(&mut director)?;
        }
        director.advance_frame(opts.us_per_frame, &mut sink);
        let mut frame = sample_engine_frame(&session, director.spu(), director.sequencer());
        // Drain the SFX ring the hosts drain every tick; left alone, a long
        // trace grows the queue without bound.
        frame.sfx_ring = session
            .host
            .world
            .take_sfx_ring_ops()
            .into_iter()
            .filter_map(|op| {
                use legaia_engine_system::sound_state::SfxRingOp;
                match op {
                    SfxRingOp::Push(id) | SfxRingOp::ReplaceLast(id) => Some(id),
                    SfxRingOp::WriteSlot(_, id) | SfxRingOp::ArmSlot(_, id, _) => Some(id),
                    SfxRingOp::SetLastDelay(_) | SfxRingOp::ReleaseSustained => None,
                }
            })
            .collect();
        out.push(frame);
    }
    Ok(out)
}

/// Drop a freshly-opened [`NativeSession`] into live field dispatch before an
/// audio / PCM trace samples it.
///
/// [`NativeSession::open`] only calls `load_scene`: the world stays in
/// [`SceneMode::Title`](legaia_engine_mode::mode::SceneMode) with **no field
/// record installed**, so the field VM steps nothing and op `0x35` never
/// executes. Every other engine-side oracle already goes through
/// [`NativeSession::enter_field_live`] (the mode-trace builders, `sim-trace`);
/// the two audio oracles were the ones that did not, which is the whole
/// reason their engine traces reported an empty voice mask on every frame
/// while retail keyed voices.
///
/// [`legaia_engine_session::session::FieldLiveOpts::default`] rather than the window's playable
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
pub(crate) fn enter_scene_for_trace(session: &mut NativeSession, scene: &str) -> Result<()> {
    session.host.world.seed_free_roam_story_baseline(scene);
    session.enter_field_live(
        scene,
        &legaia_engine_session::session::FieldLiveOpts::default(),
    )?;
    Ok(())
}

/// Resolve `bgm_id` the way [`legaia_engine_scene_host::scene_host::SceneHost::route_bgm_events`]
/// resolves an op-`0x35` start and hand it to `director`: a scene-local id
/// plays against the staged scene bank, a global-pool id (`>= 2000`) brings
/// its own VAB. The manual `--bgm-id` override took only the first of those
/// two paths, so an override naming a real music track resolved to nothing.
pub(crate) fn start_bgm_id_directly(
    session: &NativeSession,
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

/// One track on its own: start `bgm_id` against a fresh
/// [`TraceBgmDirector`] at sequence volume `seq_vol` and sample its SPU for
/// `frames` 60 Hz frames, with no world tick in between.
///
/// `session` only resolves the track's bytes. This is the pinned trace of
/// [`build_engine_audio_trace`] (`pin_bgm`, which drains and drops the
/// scene's own BGM events) without the session step, so a corpus sweep can
/// afford traces long enough to sound every note of a track. `frame` counts
/// from `0`. `None` when the id resolves to no track.
///
/// With `whole_track`, `frames` is a cap: the trace ends
/// [`WHOLE_TRACK_TAIL_FRAMES`] after the score loops back a second time (or
/// runs off its end). One pass is not enough: channel state - a pitch-bend
/// wheel, a pan, a volume - carries across the loop, so the second pass can
/// key notes the first never did; the third replays the second.
pub fn build_pinned_track_trace(
    session: &NativeSession,
    bgm_id: u16,
    seq_vol: u8,
    frames: u64,
    whole_track: bool,
) -> Result<Option<Vec<AudioTraceFrame>>> {
    let mut director = TraceBgmDirector::new();
    director.master_vol = seq_vol;
    start_bgm_id_directly(session, &mut director, bgm_id)?;
    if director.sequencer().is_none() {
        return Ok(None);
    }
    let us_per_frame = 1_000_000.0 / 60.0;
    let samples_per_frame = (44_100_f64 * (us_per_frame / 1_000_000.0)) as usize;
    let mut sink = vec![0i16; samples_per_frame * 2];
    let mut out = Vec::new();
    let mut end = frames;
    for f in 0..=frames {
        if f > end {
            break;
        }
        if f > 0 {
            director.advance_frame(us_per_frame, &mut sink);
        }
        let mut frame = sample_engine_frame(session, director.spu(), director.sequencer());
        frame.frame = f;
        out.push(frame);
        if whole_track
            && end == frames
            && director
                .sequencer()
                .is_some_and(|s| s.loop_count() > 1 || s.is_finished())
        {
            end = (f + WHOLE_TRACK_TAIL_FRAMES).min(frames);
        }
    }
    Ok(Some(out))
}

/// How long [`build_pinned_track_trace`] keeps sampling after the score
/// loops: long enough for the last notes' release tails to drain.
pub const WHOLE_TRACK_TAIL_FRAMES: u64 = 600;

/// Re-exported wrapper for [`crate::pcm_oracle::build_engine_pcm_trace`].
/// The PCM-trace loop wants the same per-frame voice-activity sampler
/// but the function is otherwise private to this module.
pub(crate) fn sample_engine_frame_for_pcm(
    session: &NativeSession,
    spu: &legaia_engine_audio::Spu,
    sequencer: Option<&legaia_engine_audio::Sequencer>,
) -> AudioTraceFrame {
    sample_engine_frame(session, spu, sequencer)
}

pub(super) fn sample_engine_frame(
    session: &NativeSession,
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
            key_ons: Some(v.key_on_count()),
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
        sfx_ring: Vec::new(),
        active_voice_mask: mask,
        voices,
    }
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
        pin_bgm: false,
    };
    build_engine_audio_trace(extracted_root, disc, &opts)
}

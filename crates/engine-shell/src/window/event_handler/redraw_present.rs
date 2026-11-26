//! The redraw's submit half: the draw census diagnostic, the per-draw
//! marks staged on the renderer, the screenshot harness (sweep and
//! single-shot capture) and the present.

use super::super::*;

/// What the frame presents: the scene alone, or the scene with the
/// field-to-battle transition's screen primitives composited over it.
///
/// A function rather than an inline expression because the screenshot
/// harness has to capture the *same* target it presents - capturing a bare
/// `Scene` there drops every transition style from the PNGs.
pub(super) fn present_target<'a>(
    scene: &'a RenderScene<'a>,
    prims: &'a [legaia_render_kernels::screen_prim::ScreenPrim],
    under_overlay: &'a [legaia_render_kernels::screen_prim::ScreenPrim],
) -> RenderTarget<'a> {
    if prims.is_empty() && under_overlay.is_empty() {
        RenderTarget::Scene(scene)
    } else {
        RenderTarget::SceneWithScreenPrims {
            scene,
            prims,
            under_overlay,
        }
    }
}

/// The frame's per-draw marks, each by draw index.
pub(super) struct DrawMarks {
    pub clip: Vec<(usize, legaia_engine_render::DrawClip)>,
    pub color_clip: Vec<(usize, legaia_engine_render::DrawClip)>,
    pub nclip: Vec<(usize, u32)>,
    pub color_nclip: Vec<(usize, u32)>,
    pub order: Vec<(usize, u8)>,
    pub color_order: Vec<(usize, u8)>,
}

/// Stage the per-draw object-effect clips, NCLIP and order words on the renderer.
pub(super) fn stage_draw_marks(
    r: &legaia_engine_render::Renderer,
    draws_len: usize,
    color_draws_len: usize,
    marks: DrawMarks,
) {
    let DrawMarks {
        clip: clip_marks,
        color_clip: color_clip_marks,
        nclip: nclip_marks,
        color_nclip: color_nclip_marks,
        order: order_marks,
        color_order: color_order_marks,
    } = marks;
    let mut tex = vec![None; draws_len];
    for (i, c) in clip_marks {
        if let Some(slot) = tex.get_mut(i) {
            *slot = Some(c);
        }
    }
    let mut col = vec![None; color_draws_len];
    for (i, c) in color_clip_marks {
        if let Some(slot) = col.get_mut(i) {
            *slot = Some(c);
        }
    }
    r.set_draw_clips(tex, col);
    let mut nclip = vec![None; draws_len];
    for (i, m) in nclip_marks {
        if let Some(slot) = nclip.get_mut(i) {
            *slot = Some(m);
        }
    }
    r.set_draw_nclip(nclip);
    let mut color_nclip = vec![None; color_draws_len];
    for (i, m) in color_nclip_marks {
        if let Some(slot) = color_nclip.get_mut(i) {
            *slot = Some(m);
        }
    }
    r.set_color_draw_nclip(color_nclip);
    let mut order = vec![0; draws_len];
    for (i, o) in order_marks {
        if let Some(slot) = order.get_mut(i) {
            *slot = o;
        }
    }
    let mut color_order = vec![0; color_draws_len];
    for (i, o) in color_order_marks {
        if let Some(slot) = color_order.get_mut(i) {
            *slot = o;
        }
    }
    r.set_draw_order(order, color_order);
}

/// `LEGAIA_SWEEP_TRACE=1`: beside each `--screenshot-every` PNG, the world's
/// [`host_trace`](legaia_engine_session::host_trace) sample as text, which
/// `scripts/ci/host-pair.mjs` compares with the page's
/// `debug_host_trace_text` at the same tick.
pub(super) fn write_sweep_trace(
    screenshot: Option<&ScreenshotConfig>,
    tick_no: u64,
    sweep_next_tick: u64,
    session: &legaia_engine_session::native::NativeSession,
    menu: &legaia_engine_menu_runtime::menu_runtime::MenuRuntime,
) {
    let Some(sw) = screenshot.and_then(|sc| sc.sweep.as_ref()) else {
        return;
    };
    if tick_no < sweep_next_tick || std::env::var_os("LEGAIA_SWEEP_TRACE").is_none() {
        return;
    }
    let path = sw.dir.join(format!("tick_{tick_no:05}.trace"));
    let text = legaia_engine_session::host_trace::sample_text(session, menu);
    if let Err(e) = std::fs::write(&path, text) {
        eprintln!("sweep trace {}: {e}", path.display());
    }
}

/// The `--screenshot-every` sweep: capture this frame into the sweep dir
/// when its tick is due, and exit after the last one.
pub(super) fn capture_sweep_frame<'a>(
    r: &legaia_engine_render::Renderer,
    target: &dyn Fn() -> RenderTarget<'a>,
    screenshot: Option<&ScreenshotConfig>,
    tick_no: u64,
    sweep_next_tick: &mut u64,
) {
    // Periodic sweep (`--screenshot-every`): capture a frame every N
    // ticks into the sweep dir (named for the tick), keep running,
    // and exit after the capture at/past `--screenshot-last-tick`.
    // Redraws can drain up to 4 ticks, so the cadence is tracked via
    // `sweep_next_tick` rather than a modulo on the tick counter.
    if let Some(sw) = screenshot.and_then(|sc| sc.sweep.as_ref())
        && tick_no >= *sweep_next_tick
    {
        let path = sw.dir.join(format!("tick_{:05}.png", tick_no));
        let last_tick = sw.last_tick;
        *sweep_next_tick = tick_no + sw.every;
        match r.capture_rgba_settled(target) {
            Ok(img) => match write_capture_png(&path, &img) {
                Ok(()) => {
                    println!(
                        "[ok] screenshot {} ({}x{}) at tick {}",
                        path.display(),
                        img.width,
                        img.height,
                        tick_no
                    );
                }
                Err(e) => {
                    eprintln!("screenshot write failed: {e:#}");
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("screenshot capture failed: {e:#}");
                std::process::exit(1);
            }
        }
        if last_tick.is_some_and(|lt| tick_no >= lt) {
            std::process::exit(0);
        }
    }
}

/// Raised when the cast-sequence recorder starts its film (the cast's begin,
/// or the capture's gate), which `LEGAIA_LEADIN_TABLE_COST` reads to end the
/// lead-in.
pub(super) static CAST_SEQ_STARTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl PlayWindowApp {
    /// `LEGAIA_DIAG_DRAWS=<path>`: write this frame's textured draws as
    /// texture families.
    pub(super) fn write_draw_census(&self, draws: &[SceneDraw<'_>]) {
        // `LEGAIA_DIAG_DRAWS=<path>`: this frame's textured draws as
        // texture families (`draw_census`), rewritten every frame so the
        // file holds the captured one.
        if let Some(d) = self.draw_census.as_ref()
            && let Some(path) = std::env::var_os("LEGAIA_DIAG_DRAWS")
        {
            type Census = legaia_engine_system::draw_census::MeshCensus;
            let mut by_ptr: std::collections::HashMap<
                *const legaia_engine_render::UploadedVramMesh,
                &Census,
            > = std::collections::HashMap::new();
            for (m, c) in self.meshes.iter().zip(&d.meshes) {
                if let Some(c) = c {
                    by_ptr.insert(m as *const _, c);
                }
            }
            for (m, c) in self.field_lit.meshes.iter().zip(&d.lit) {
                by_ptr.insert(m as *const _, c);
            }
            for (idx, m) in &self.field_morph_live {
                if let Some(c) = d.morph.get(idx) {
                    by_ptr.insert(m as *const _, c);
                }
            }
            if let (Some((_, Some(m))), Some(c)) = (&self.ground_crop, &d.ground_crop) {
                by_ptr.insert(m as *const _, c);
            }
            if let (Some(m), Some(c)) = (&self.ground_heightfield, &d.ground) {
                by_ptr.insert(m as *const _, c);
            }
            let census_draws = || {
                draws.iter().filter_map(|dr| {
                    by_ptr
                        .get(&(dr.mesh as *const _))
                        .map(|c| (*c, dr.mvp.to_cols_array()))
                })
            };
            let rows = legaia_engine_system::draw_census::family_rows(census_draws(), 320.0, 240.0);
            // `LEGAIA_DIAG_DRAW_TRIS=<clut hex>`: that family's triangles,
            // one per line, at `<path>.tris`.
            if let Some(cba) = std::env::var("LEGAIA_DIAG_DRAW_TRIS")
                .ok()
                .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            {
                let mut tp = path.clone();
                tp.push(".tris");
                let _ = std::fs::write(
                    tp,
                    legaia_engine_system::draw_census::family_tris_jsonl(
                        census_draws(),
                        320.0,
                        240.0,
                        cba,
                    ),
                );
            }
            // The CPU VRAM the field pass samples, beside it
            // (`<path>.vram`, 1 MiB little-endian), so a family whose
            // count and colour agree but whose pixels part can be read
            // against the state's own VRAM.
            if let Some(v) = self.cpu_vram_base.as_ref() {
                let mut vp = path.clone();
                vp.push(".vram");
                let _ = std::fs::write(vp, v.as_bytes());
            }
            let _ = std::fs::write(
                path,
                legaia_engine_system::draw_census::family_rows_jsonl(&rows),
            );
        }
    }

    /// Tell the renderer whether this frame builds on an earlier one, and how
    /// far back, from this frame's world: ahead of every draw of it, the
    /// sweep's and the framebuffer grab's captures as well as the present.
    /// Asked after the present instead, a capture drew with the frame
    /// before's answer, so on the tick a cast's persisting run started or
    /// changed depth the sweep's PNG read a cleared or older page while the
    /// presented frame, and the play page, did not (the host pair's
    /// `cast_thunder`, tick `869`).
    pub(super) fn arm_frame_persist(&self, r: &legaia_engine_render::Renderer) {
        r.set_frame_ring_keep(self.session.host.world.cast_module_resident());
        r.set_frame_persist(self.session.host.world.cast_frame_persists());
        r.set_frame_persist_depth(self.session.host.world.cast_frame_persist_vsyncs());
    }

    /// The single-shot screenshot harness (phase gate, capture, exit) or
    /// the present. [`Self::arm_frame_persist`] has run for this frame.
    pub(super) fn present_frame(
        &self,
        r: &legaia_engine_render::Renderer,
        scene: &RenderScene<'_>,
        screen_prims: &[legaia_render_kernels::screen_prim::ScreenPrim],
        light_prims: &[legaia_render_kernels::screen_prim::ScreenPrim],
    ) {
        let target = |scene| present_target(scene, screen_prims, light_prims);
        #[cfg(feature = "oracles")]
        self.record_audio_wav();
        self.capture_cast_sequence_frame(r, &|| target(scene));
        self.record_sfx_log();
        // Screenshot harness: at the target tick, read the frame back
        // offscreen and exit instead of presenting to the window.
        let gated = self.screenshot.as_ref().is_some_and(|sc| {
            sc.path.is_some()
                && (sc.phase_gate.is_some()
                    || sc.arrival_gate.is_some()
                    || sc.script_gate.is_some()
                    || sc.battle_drive.is_some())
        });
        if gated
            && !self.capture_phase_met()
            && self
                .screenshot
                .as_ref()
                .is_some_and(|sc| self.tick_no >= sc.capture_tick)
        {
            eprintln!(
                "phase gate not met by tick {} (action state 0x{:02X})",
                self.tick_no, self.session.host.world.battle_ctx.action_state
            );
            std::process::exit(3);
        }
        let capture_due = self.screenshot.as_ref().is_some_and(|sc| {
            sc.path.is_some()
                && if gated {
                    self.capture_phase_met()
                } else {
                    self.tick_no >= sc.capture_tick
                }
        });
        if capture_due {
            eprintln!("capture at tick {}", self.tick_no);
            self.diag_capture_state();
            self.note_display_retake();
            let path = self
                .screenshot
                .as_ref()
                .and_then(|sc| sc.path.clone())
                .unwrap();
            // A frame the scene drew: opaque, and reproduced by a second
            // readback (`Renderer::capture_rgba_settled` - a readback under
            // GPU contention has come back with a zero band over its top
            // rows, twice running).
            let capture = r.capture_rgba_settled(&|| target(scene));
            match capture {
                Ok(img) => match write_capture_png(&path, &img) {
                    Ok(()) => {
                        println!(
                            "[ok] screenshot {} ({}x{}) at tick {}",
                            path.display(),
                            img.width,
                            img.height,
                            self.tick_no
                        );
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("screenshot write failed: {e:#}");
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("screenshot capture failed: {e:#}");
                    std::process::exit(1);
                }
            }
        }
        // A capture child's frames before its capture reach nobody: the
        // capture and the recorder each draw their own frame from the scene
        // description (`encode_frame` stages everything it reads), and
        // `Renderer::render` leaves nothing a later frame samples - except
        // through a cast module, whose wash frames build on the frames before
        // them (`Renderer::set_frame_persist`).
        if capture_child_skips_present() && !self.session.host.world.cast_module_resident() {
            r.flush_frame_uploads();
            return;
        }
        if let Err(e) = r.render(target(scene)) {
            log::error!("render: {e:#}");
        }
    }
}

/// `LEGAIA_CAPTURE_NO_PRESENT=1` (the retail comparison's image child): frames
/// are not drawn to the window, only to the capture. Most of a capture child's
/// GPU time and all of its display traffic otherwise goes on frames nobody
/// sees.
fn capture_child_skips_present() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LEGAIA_CAPTURE_NO_PRESENT").is_some())
}

/// The `LEGAIA_DIAG_CAST_SEQ` recorder's run state
/// ([`PlayWindowApp::capture_cast_sequence_frame`]).
struct CastSeqRecorder {
    dir: std::path::PathBuf,
    /// Shot interval in ticks (read only by the `oracles` shot path).
    #[cfg_attr(not(feature = "oracles"), allow(dead_code))]
    every: u64,
    tail: u64,
    max: u64,
    meta: std::io::BufWriter<std::fs::File>,
    /// `cam.csv`: the live camera pose per tick.
    cam: std::io::BufWriter<std::fs::File>,
    /// `body.csv`: the acting seat's clip, cursor and live pair per tick.
    body: std::io::BufWriter<std::fs::File>,
    /// `sfx.csv`: the sound cues that came due each tick (the audio
    /// director's command record), the engine half of the retail probe's
    /// cue-ring file. Empty without an audio director (`--no-audio`).
    sfx: std::io::BufWriter<std::fs::File>,
    /// `xa.csv`: the CD-XA clip requests each tick, `tick,st7,phase,clip,channel,dur`.
    xa: std::io::BufWriter<std::fs::File>,
    /// `silent.csv`: the ring cues that came due and keyed no voice
    /// ([`AudioDirector::silent_ring`]), `tick,st7,phase,id`. Not scored: a
    /// listening check beside `sfx.csv`.
    ///
    /// [`AudioDirector::silent_ring`]: legaia_engine_session::audio_director::AudioDirector::silent_ring
    silent: std::io::BufWriter<std::fs::File>,
    /// Silent ring cues already read.
    silent_seen: u64,
    /// Director commands already read ([`AudioCommandLog::count`]).
    ///
    /// [`AudioCommandLog::count`]: legaia_engine_session::audio_director::AudioCommandLog::count
    sfx_seen: u64,
    /// Only a cast by this acting seat starts the recording
    /// (`LEGAIA_DIAG_CAST_SEQ_SEAT`); any seat when unset.
    seat: Option<u8>,
    /// `LEGAIA_DIAG_CAST_SEQ_BEGIN=gate`: start on the tick the capture's own
    /// gate holds instead of on cast begin, for a retail state that holds an
    /// action already running (a monster's strike): both recordings then open
    /// on the same instant. The child takes its frame later
    /// (`LEGAIA_DIAG_CAPTURE_TICK`).
    begin_on_gate: bool,
    start: Option<u64>,
    done: Option<u64>,
    shots: u64,
}

/// The `LEGAIA_DIAG_AUDIO_WAV` recorder's run state
/// ([`PlayWindowApp::record_audio_wav`]).
#[cfg(feature = "oracles")]
struct AudioWavRecorder {
    path: std::path::PathBuf,
    /// Stereo frames to keep from the gate on.
    frames: usize,
    /// The interleaved mix from the gate on.
    pcm: Vec<i16>,
    /// The audio level the director applied on each recorded tick
    /// (`AudioDirector::duck_applied`, `DAT_8007B6EC`), written beside the WAV
    /// as `<path>.duck`: the duck the score is to be rendered under.
    duck: Vec<u8>,
    /// Whether the module's arm waited on the CD drive on each recorded tick
    /// (`World::cast_drive_waiting`), written beside the WAV as
    /// `<path>.drive`: where the cast's seek wait sits, which the PCM cast
    /// channel aligns the voice after within the seek window.
    drive: Vec<u8>,
    /// The band each recorded tick stood in, written beside the WAV as
    /// `<path>.band`: `action_state phase stage read_left xa_busy`, the action SM's
    /// state, the module phase, the side-band stage `ctx[+0x276]`, the stand-in's read time left
    /// and the CD-XA one-shot word, for lining the engine's cast up against a
    /// frame-stepped retail timeline (`scripts/mednafen/drive-timeline.py`).
    band: Vec<String>,
    /// The last tick pulled, so a redraw that ran no tick pulls nothing.
    last_tick: Option<u64>,
    /// Fractional output frames carried between ticks.
    carry: f64,
    /// The tick the capture's gate first held.
    start: Option<u64>,
    /// `LEGAIA_DIAG_AUDIO_WAV_DONE_TAIL`: stop this many ticks past the action
    /// SM's Done band instead of at the length.
    done_tail: Option<u64>,
    /// The tick the Done band was first seen.
    done: Option<u64>,
    /// `LEGAIA_DIAG_AUDIO_WAV_ARM=state,phase,word`: the state's instant inside
    /// a summon band, as the step track names it. The recording opens on the
    /// first gated pass at or past it: under `(state, phase)` with the band word
    /// `ctx[+0x6D8]` at or below `word`, or any later state or phase.
    arm: Option<(u8, u8, i16)>,
    /// Whether the recording opened inside the Done band: a state taken
    /// there (`zora_glare_petrify_post`, `0x51`) stops on the band's exit,
    /// not on the gate's own tick.
    opened_in_done: bool,
}

/// Action state `0x28`, cast begin: the state every Magic action - a party
/// summon, a monster spell, a capture-class cast - passes through first.
const CAST_BEGIN_STATE: u8 = 0x28;
/// Action state `0x3C`: where a heal-class Magic action begins instead.
const HEAL_BEGIN_STATE: u8 = 0x3C;

impl PlayWindowApp {
    /// `LEGAIA_DIAG_AUDIO_WAV=<path>` instrument: the engine half of the
    /// real-PCM `cast` channel (`docs/subsystems/audio.md`, real retail PCM).
    ///
    /// The capture child routes its director to no device
    /// (`LEGAIA_AUDIO_DETACHED=1`), so nothing pulls the mix core. This pulls
    /// it once a tick from the first tick on, one console vsync of output a
    /// tick (`pcm_retail::RETAIL_VSYNC_SAMPLES`, carried), so every cue the
    /// fight keys sounds and decays on the clock retail's recording runs on.
    /// What the capture's own gate sees from its first tick on is kept:
    /// `LEGAIA_DIAG_AUDIO_WAV_SECONDS` (default `10`) of it, written as a
    /// 44.1 kHz stereo WAV, and the child exits `0`. The music bus is at `0`
    /// unless `LEGAIA_DIAG_AUDIO_WAV_BUS=all`: its voices still take their
    /// voices and their reverb send, only their output is silent, because the
    /// score's own phase is the `track` channel's to stand. The master volume
    /// is the unscaled mix. With `LEGAIA_DIAG_AUDIO_WAV_DONE_TAIL=<ticks>` the
    /// recording stops that many ticks past the action SM's Done band
    /// (`0x50` / `0x51` / `0x5A` / `0x65`), where a cast's own sound ends and the
    /// fight's next turn begins; a recording that opens inside that band stops
    /// that far past its exit. The child must not take its frame on the gate
    /// (`LEGAIA_DIAG_CAPTURE_TICK` past the window).
    #[cfg(feature = "oracles")]
    fn record_audio_wav(&self) {
        use legaia_engine_audio::AudioSink as _;
        static REC: std::sync::OnceLock<Option<std::sync::Mutex<AudioWavRecorder>>> =
            std::sync::OnceLock::new();
        let Some(director) = self.session.bgm.as_ref() else {
            return;
        };
        let Some(rec) = REC.get_or_init(|| {
            let path = std::path::PathBuf::from(std::env::var_os("LEGAIA_DIAG_AUDIO_WAV")?);
            let seconds: f64 = std::env::var("LEGAIA_DIAG_AUDIO_WAV_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10.0);
            let sink = director.audio();
            if std::env::var("LEGAIA_DIAG_AUDIO_WAV_BUS").as_deref() != Ok("all") {
                sink.set_bus_volumes(0, legaia_engine_audio::VOLUME_LEVEL_UNITY);
            }
            sink.set_master_volume(100);
            Some(std::sync::Mutex::new(AudioWavRecorder {
                path,
                frames: (seconds * f64::from(legaia_engine_audio::SPU_INTERNAL_RATE)) as usize,
                pcm: Vec::new(),
                duck: Vec::new(),
                drive: Vec::new(),
                band: Vec::new(),
                last_tick: None,
                carry: 0.0,
                start: None,
                done_tail: std::env::var("LEGAIA_DIAG_AUDIO_WAV_DONE_TAIL")
                    .ok()
                    .and_then(|v| v.parse().ok()),
                done: None,
                arm: std::env::var("LEGAIA_DIAG_AUDIO_WAV_ARM")
                    .ok()
                    .and_then(|v| {
                        let f: Vec<i32> =
                            v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                        match f[..] {
                            [s, p, w] => Some((s as u8, p as u8, w as i16)),
                            _ => None,
                        }
                    }),
                opened_in_done: false,
            }))
        }) else {
            return;
        };
        let Ok(mut rec) = rec.lock() else {
            return;
        };
        let tick = self.tick_no;
        if rec.last_tick == Some(tick) {
            return;
        }
        let ticks = rec.last_tick.map_or(1, |t| tick.saturating_sub(t).max(1));
        rec.last_tick = Some(tick);
        let at_arm = rec.arm.is_none_or(|(state, phase, word)| {
            let w = &self.session.host.world;
            let (s, p) = (w.battle_ctx.action_state, w.casting.module_phase);
            s > state
                || (s == state && (p > phase || (p == phase && w.battle_ctx.frame_timer <= word)))
        });
        if rec.start.is_none() && at_arm && self.capture_gate_met() {
            let w = &self.session.host.world;
            eprintln!(
                "audio wav: the capture's gate at tick {tick} (action {:#04x}, side-band stage {}, read left {:?}, drive waiting {})",
                w.battle_ctx.action_state,
                w.battle_ctx.menu_open,
                w.casting.sideband_read_left,
                w.cast_drive_waiting()
            );
            rec.start = Some(tick);
        }
        let want = ticks as f64 * legaia_parity::pcm_retail::RETAIL_VSYNC_SAMPLES + rec.carry;
        let n = want as usize;
        rec.carry = want - n as f64;
        let keep = rec.start.is_some();
        if keep {
            rec.duck.push(director.duck_applied());
            rec.drive
                .push(u8::from(self.session.host.world.cast_drive_waiting()));
            let w = &self.session.host.world;
            rec.band.push(format!(
                "{} {} {} {} {}",
                w.battle_ctx.action_state,
                w.casting.module_phase,
                w.battle_ctx.menu_open,
                w.casting.sideband_read_left.map_or(-1, i32::from),
                w.audio.battle_xa_busy_frames
            ));
        }
        let pcm = &mut rec.pcm;
        director.audio().with_core(|core| {
            for _ in 0..n {
                let (l, r) = core.next_frame();
                if keep {
                    pcm.push(l);
                    pcm.push(r);
                }
            }
        });
        let rec = &mut *rec;
        let world = &self.session.host.world;
        let in_done = world.mode == SceneMode::Battle
            && matches!(world.battle_ctx.action_state, 0x50 | 0x51 | 0x5A | 0x65);
        if rec.start == Some(tick) {
            rec.opened_in_done = in_done;
        }
        if rec.start.is_some() && rec.done.is_none() && (in_done != rec.opened_in_done) {
            rec.done = Some(tick);
        }
        let done = rec
            .done
            .zip(rec.done_tail)
            .is_some_and(|(d, tail)| tick >= d + tail);
        if done || rec.pcm.len() >= rec.frames * 2 {
            rec.pcm.truncate(rec.frames * 2);
            let duck: String = rec.duck.iter().map(|l| format!("{l}\n")).collect();
            let _ = std::fs::write(rec.path.with_extension("duck"), duck);
            let drive: String = rec.drive.iter().map(|d| format!("{d}\n")).collect();
            let _ = std::fs::write(rec.path.with_extension("drive"), drive);
            let band: String = rec.band.iter().map(|b| format!("{b}\n")).collect();
            let _ = std::fs::write(rec.path.with_extension("band"), band);
            match legaia_parity::pcm_oracle::write_wav(&rec.path, &rec.pcm) {
                Ok(()) => {
                    println!(
                        "[ok] audio wav {} from tick {:?}",
                        rec.path.display(),
                        rec.start
                    );
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("audio wav: write {}: {e:#}", rec.path.display());
                    std::process::exit(1);
                }
            }
        }
    }

    /// `LEGAIA_DIAG_CAST_SEQ=<dir>` instrument: the engine side of the
    /// per-cast frame-sequence oracle (`docs/tooling/cast-sequence-oracle.md`).
    ///
    /// From the first tick the action SM holds cast begin (`0x28`), for the
    /// acting seat `LEGAIA_DIAG_CAST_SEQ_SEAT` when set, it writes one `meta.csv` row per tick - the action
    /// state, the module phase, the countdown and camera clocks the capture
    /// gates read, every seat's HP - and every
    /// `LEGAIA_DIAG_CAST_SEQ_EVERY` (default `1`) ticks the frame, already
    /// cropped and box-filtered to the compared `320 x 224`
    /// (`e_<tick>.png`). It exits `LEGAIA_DIAG_CAST_SEQ_TAIL` (default `60`)
    /// ticks past the Done band (`0x50` / `0x51` / `0x5A`), or after
    /// `LEGAIA_DIAG_CAST_SEQ_MAX` (default `2000`) frames. The run must be a
    /// `--screenshot` child, which is what makes it one tick per redraw.
    /// `LEGAIA_DIAG_SFX_LOG=<path>`: every sound cue the audio director was
    /// handed, one `tick,id,kind,mode,menu` row each - `queue` off the delay
    /// scheduler (the hosts' screen blips, `AudioDirector::queue_ui_cue`),
    /// `ring` off the retail ring, `now` a direct blip - with the scene mode
    /// and whether the pause menu is open. Whatever the scene, so the pause,
    /// shop and save screens' blips can be filmed against retail's cue ring.
    /// A harness reading: a `--screenshot` child runs one tick a redraw.
    fn record_sfx_log(&self) {
        use std::io::Write as _;
        /// The open log and the director commands already read.
        type SfxLog = std::sync::Mutex<(std::io::BufWriter<std::fs::File>, u64)>;
        static LOG: std::sync::OnceLock<Option<SfxLog>> = std::sync::OnceLock::new();
        let Some(log) = LOG.get_or_init(|| {
            let path = std::env::var_os("LEGAIA_DIAG_SFX_LOG")?;
            let mut f = std::io::BufWriter::new(std::fs::File::create(path).ok()?);
            writeln!(f, "tick,id,kind,mode,menu").ok()?;
            Some(std::sync::Mutex::new((f, 0)))
        }) else {
            return;
        };
        let (Ok(mut log), Some(d)) = (log.lock(), self.session.bgm.as_ref()) else {
            return;
        };
        let commands = d.commands();
        let fresh = commands.count().saturating_sub(log.1) as usize;
        log.1 = commands.count();
        let recent: Vec<_> = commands.recent().copied().collect();
        let mode = self.session.host.world.mode;
        let menu = u8::from(self.session.field_menu_is_open());
        let tick = self.tick_no;
        use legaia_engine_session::audio_director::AudioCommand as C;
        for c in &recent[recent.len().saturating_sub(fresh)..] {
            let (id, kind) = match *c {
                C::SfxDue(id) => (i32::from(id), "queue"),
                C::RingDue(id) => (i32::from(id), "ring"),
                C::SfxNow(id) => (i32::from(id), "now"),
                _ => continue,
            };
            let _ = writeln!(log.0, "{tick},{id},{kind},{mode:?},{menu}");
        }
        let _ = log.0.flush();
    }

    fn capture_cast_sequence_frame<'a>(
        &self,
        r: &legaia_engine_render::Renderer,
        target: &dyn Fn() -> RenderTarget<'a>,
    ) {
        static REC: std::sync::OnceLock<Option<std::sync::Mutex<CastSeqRecorder>>> =
            std::sync::OnceLock::new();
        let Some(rec) = REC.get_or_init(|| {
            let dir = std::path::PathBuf::from(std::env::var_os("LEGAIA_DIAG_CAST_SEQ")?);
            std::fs::create_dir_all(&dir).ok()?;
            let num = |k: &str, d: u64| {
                std::env::var(k)
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(d)
            };
            #[cfg(not(feature = "oracles"))]
            eprintln!(
                "cast sequence: built without the `oracles` feature - no frame shots \
                 (cargo build -p legaia-engine-shell --features oracles)"
            );
            let mut meta = std::io::BufWriter::new(std::fs::File::create(dir.join("meta.csv")).ok()?);
            use std::io::Write as _;
            let _ = writeln!(
                meta,
                "tick,st7,phase,acting,frames,c6d8,cd,yaw,c87c,fade_white,fade_age,hp0,hp1,hp2,hp3,hp4,hp5,hp6,hp7,shot,rand,drive_seek_arm,drive_poll_arm,drive_busy,fade_level,guard,cost,parts,ghosts,over,work,wash,linked,lk_stage,lk_grid,lk_bodies,lk_ghosts,lk_shadows,lk_fx,lk_sprites,lk_trails,walk"
            );
            let mut cam = std::io::BufWriter::new(std::fs::File::create(dir.join("cam.csv")).ok()?);
            let _ = writeln!(cam, "tick,st7,phase,pitch,yaw,trx,try,trz,fx,fy,fz,c6da,step");
            let mut body = std::io::BufWriter::new(std::fs::File::create(dir.join("body.csv")).ok()?);
            let _ = writeln!(body, "tick,st7,acting,clip,queued,looping,cursor,lx,lz,bx,bz,facing,tlx,tlz,tbx,tbz,tfacing");
            let mut sfx = std::io::BufWriter::new(std::fs::File::create(dir.join("sfx.csv")).ok()?);
            let _ = writeln!(sfx, "tick,st7,phase,id,kind");
            let mut xa = std::io::BufWriter::new(std::fs::File::create(dir.join("xa.csv")).ok()?);
            let _ = writeln!(xa, "tick,st7,phase,clip,channel,dur");
            let mut silent = std::io::BufWriter::new(std::fs::File::create(dir.join("silent.csv")).ok()?);
            let _ = writeln!(silent, "tick,st7,phase,id");
            if self.session.bgm.is_none() {
                eprintln!(
                    "cast sequence: no audio director (--no-audio) - sfx.csv stays empty \
                     (LEGAIA_AUDIO_DETACHED=1 without --no-audio routes audio to no device)"
                );
            }
            Some(std::sync::Mutex::new(CastSeqRecorder {
                cam,
                body,
                sfx,
                xa,
                silent,
                silent_seen: 0,
                sfx_seen: 0,
                every: num("LEGAIA_DIAG_CAST_SEQ_EVERY", 1).max(1),
                tail: num("LEGAIA_DIAG_CAST_SEQ_TAIL", 60),
                max: num("LEGAIA_DIAG_CAST_SEQ_MAX", 2000),
                seat: std::env::var("LEGAIA_DIAG_CAST_SEQ_SEAT")
                    .ok()
                    .and_then(|v| v.parse().ok()),
                begin_on_gate: std::env::var("LEGAIA_DIAG_CAST_SEQ_BEGIN").as_deref() == Ok("gate"),
                dir,
                meta,
                start: None,
                done: None,
                shots: 0,
            }))
        }) else {
            return;
        };
        let Ok(mut rec) = rec.lock() else {
            return;
        };
        // The cues the director was handed since the last tick: the due
        // halves (`SfxDue` off the delay scheduler, `RingDue` off the retail
        // ring, `SfxNow` a blip), read before any early return so a cue
        // before the cast never lands on its first tick.
        let mut due: Vec<(i32, &'static str)> = Vec::new();
        let mut clips: Vec<(u32, u32, u32)> = Vec::new();
        let mut silent: Vec<i16> = Vec::new();
        if let Some(d) = self.session.bgm.as_ref() {
            let log = d.silent_ring();
            let fresh = log.count().saturating_sub(rec.silent_seen) as usize;
            rec.silent_seen = log.count();
            let recent: Vec<i16> = log.recent().copied().collect();
            silent.extend_from_slice(&recent[recent.len().saturating_sub(fresh)..]);
        }
        if let Some(d) = self.session.bgm.as_ref() {
            let log = d.commands();
            let fresh = log.count().saturating_sub(rec.sfx_seen);
            rec.sfx_seen = log.count();
            let recent: Vec<_> = log.recent().copied().collect();
            if fresh as usize > recent.len() {
                eprintln!("cast sequence: {fresh} audio commands in one tick, the oldest unread");
            }
            use legaia_engine_session::audio_director::AudioCommand as C;
            for c in &recent[recent.len().saturating_sub(fresh as usize)..] {
                match *c {
                    C::SfxDue(id) => due.push((i32::from(id), "queue")),
                    C::RingDue(id) => due.push((i32::from(id), "ring")),
                    C::SfxNow(id) => due.push((i32::from(id), "now")),
                    C::XaClip(clip, channel, dur) => clips.push((clip, channel, dur)),
                    _ => {}
                }
            }
        }
        let w = &self.session.host.world;
        if w.mode != SceneMode::Battle {
            return;
        }
        let st = w.battle_ctx.action_state;
        // The engine packs the party ahead of the monsters; retail seats the
        // party on `0..=2` and the monsters from `3`. The recorder speaks
        // retail's seats, the numbering the retail side writes.
        let pc = w.party.party_count.clamp(1, 3) as usize;
        let seat_of = |i: usize| if i < pc { i } else { 3 + i - pc };
        let acting = seat_of(usize::from(w.battle_ctx.active_actor)) as u8;
        let tick = self.tick_no;
        let start = match rec.start {
            Some(s) => s,
            None if rec.begin_on_gate => {
                if !self.capture_gate_met() {
                    // `LEGAIA_DIAG_CAST_SEQ_PRE=1`: the acting seat's walk-in
                    // up to the gate, one line a tick in the attack band (the
                    // engine half of the probe's `pre.csv`).
                    if std::env::var_os("LEGAIA_DIAG_CAST_SEQ_PRE").is_some()
                        && (0x0C..=0x20).contains(&st)
                        && let Some(a) = w.actors.get(usize::from(w.battle_ctx.active_actor))
                    {
                        let p = a.battle_animation.as_ref();
                        let t = w.actors.get(usize::from(a.battle.active_target));
                        eprintln!(
                            "cast sequence pre: {tick},{st},{acting},{},{},{},{},{},{},{},{},{},{},{},{}",
                            a.battle.current_anim,
                            a.battle.queued_anim,
                            a.battle.flag_bits.0,
                            p.map_or(-1, |p| i32::from(p.cursor_sixteenths())),
                            a.move_state.world_x,
                            a.move_state.world_z,
                            a.battle.facing_angle,
                            t.map_or(0, |t| t.move_state.world_x),
                            t.map_or(0, |t| t.move_state.world_z),
                            w.battle.camera.as_ref().map_or(-1, |c| c.action_yaw_base()),
                            w.battle.frame_clock.step,
                            a.battle.strike_index,
                        );
                    }
                    return;
                }
                eprintln!("cast sequence: the capture's gate at tick {tick} (seat {acting})");
                rec.start = Some(tick);
                CAST_SEQ_STARTED.store(true, std::sync::atomic::Ordering::Relaxed);
                tick
            }
            None if (st == CAST_BEGIN_STATE || st == HEAL_BEGIN_STATE)
                && rec.seat.is_none_or(|s| s == acting) =>
            {
                eprintln!("cast sequence: cast begin at tick {tick} (seat {acting})");
                rec.start = Some(tick);
                CAST_SEQ_STARTED.store(true, std::sync::atomic::Ordering::Relaxed);
                tick
            }
            None => return,
        };
        // `0x65`: the Spikefish flute's flee leaves the band for the run band.
        if rec.done.is_none() && matches!(st, 0x50 | 0x51 | 0x5A | 0x65) {
            eprintln!("cast sequence: done band at tick {tick} (state 0x{st:02X})");
            rec.done = Some(tick);
        }
        // The shots crop and box-filter through the parity crate's compared
        // frame (`retail_compare_image`), so they need the `oracles` feature;
        // without it the recorder writes `meta.csv` / `cam.csv` only.
        #[cfg_attr(not(feature = "oracles"), allow(unused_mut))]
        let mut shot = String::new();
        #[cfg(not(feature = "oracles"))]
        let _ = (r, target);
        #[cfg(feature = "oracles")]
        if (tick - start).is_multiple_of(rec.every) && rec.shots < rec.max {
            let capture = r.capture_rgba_settled(target);
            match capture.and_then(|img| {
                legaia_parity::retail_compare_image::Frame::from_engine_capture(
                    &img.rgba,
                    img.width as usize,
                    img.height as usize,
                )
            }) {
                Ok(f) => {
                    let name = format!("e_{tick:05}.png");
                    let rgba: Vec<u8> = f
                        .rgb
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .flat_map(|p| [p[0], p[1], p[2], 255])
                        .collect();
                    let img = CaptureImage {
                        rgba,
                        width: legaia_parity::retail_compare_image::FRAME_W as u32,
                        height: legaia_parity::retail_compare_image::FRAME_H as u32,
                    };
                    match write_recorder_png(&rec.dir.join(&name), &img) {
                        Ok(()) => {
                            rec.shots += 1;
                            shot = name;
                        }
                        Err(e) => eprintln!("cast sequence: write {name}: {e:#}"),
                    }
                }
                Err(e) => eprintln!("cast sequence: capture at tick {tick}: {e:#}"),
            }
        }
        let mut hp = vec!["-1".to_string(); 8];
        for (i, a) in w.actors.iter().enumerate().take(pc + 5) {
            if let Some(slot) = hp.get_mut(seat_of(i)) {
                *slot = a.battle.hp.to_string();
            }
        }
        // `fade_level`: the fade colour's brightest channel on this tick, the
        // oracle's cue that the screen shows the fade (`cast_sequence`'s
        // `FADE_FRAMES_BACK`).
        let (fade_white, fade_age, fade_level) = w
            .presentation
            .fade
            .as_ref()
            .filter(|f| f.kind == 1)
            .map_or((-1, -1, -1), |f| {
                (
                    i32::from(f.target_rgb()[0] == 0xFF),
                    i32::from(f.age_vsyncs()),
                    i32::from(f.rgb().into_iter().max().unwrap_or(0)),
                )
            });
        let accum = w
            .battle
            .camera
            .as_ref()
            .map_or(-1, |c| i64::from(c.close_up_accum()));
        // The module's CD-seek wait (`World::arm_module_drive`): its seek and
        // poll arms and the vsyncs the modelled drive still owes, `-1` for a
        // module with no seek-and-poll pair. The oracle's pairing reads the
        // wait's window off these (`cast_sequence::DriveWait`).
        let (seek_arm, poll_arm, busy) = w.casting.drive_arms.map_or((-1, -1, -1), |(a, _)| {
            (
                i32::from(a.seek_arm),
                i32::from(a.poll_arm),
                w.casting.drive.busy as i32,
            )
        });
        // `guard`: the party seats in the Spirit stance (`+0x1DE == 4`) as a
        // `;` list, `-` for none - the probe's `guard=` read per tick.
        let guard = {
            let g: Vec<String> = (0..3)
                .filter(|&s| w.battle.guarding.get(s).copied().unwrap_or(false))
                .map(|s| s.to_string())
                .collect();
            if g.is_empty() {
                "-".to_string()
            } else {
                g.join(";")
            }
        };
        // `over`: `1` on a tick past its battle frame's step, a vsync the
        // frame overran (`BattleFrameClock::span_extra`). Its timers drained
        // no game time there, so the oracle's engine clock skips it as it
        // skips retail's slow vsyncs (`cast_sequence::Side::live_clock`).
        let over = {
            let fc = &w.battle.frame_clock;
            u8::from(fc.ticks_into_frame(w.clock.display_frames) >= u64::from(fc.step.max(1)))
        };
        use std::io::Write as _;
        let _ = writeln!(
            rec.meta,
            "{tick},{st},{},{acting},{},{},{},{},{accum},{fade_white},{fade_age},{},{shot},{},{seek_arm},{poll_arm},{busy},{fade_level},{guard},{},{},{},{over},{},{},{},{},{}",
            w.casting.module_phase,
            w.clock.display_frames,
            w.battle_ctx.frame_timer,
            w.casting.module_cam.countdown.0,
            w.casting.module_cam.yaw_base,
            hp.join(","),
            w.rng_state,
            w.battle.frame_cost_ring.newest(),
            w.battle_frame_load().0,
            w.battle_frame_load().1,
            w.battle.frame_work,
            {
                let p = &w.casting.module_pres;
                if p.wash_live && p.wash.armed() {
                    i32::from(p.wash.mode)
                } else {
                    -1
                }
            },
            w.battle.linked_polys.map_or(-1, i64::from),
            // The census by class (`battle_link_census::LinkCount`), for
            // fitting a frame-cost term per class.
            {
                let c = &self.session.host.battle_link_last;
                format!(
                    "{},{},{},{},{},{},{},{}",
                    c.stage, c.grid, c.bodies, c.ghosts, c.shadows, c.effects, c.sprites, c.trails
                )
            },
            // The effect walker's price on the newest priced pass.
            w.battle.effect_walk_cost,
        );
        // `units.csv`: the census's count of the frame by the derived
        // pricing's units (`derived_cost::DerivedUnits`), beside `meta.csv`'s
        // links, for holding the derived price against retail's frames.
        {
            use std::io::Write as _;
            let path = rec.dir.join("units.csv");
            let fresh = !path.exists();
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                if fresh {
                    let _ = writeln!(
                        f,
                        "tick,st7,phase,tris_linked,tris_off,tris_culled,quads_linked,quads_off,quads_culled,objects,bodies,body_objects,ghosts,ghost_objects,grid_cells,builds,segments,trails,floor_clip,stage_linked,stage_off,stage_culled,stage_near,body_linked,body_off,body_culled,body_near"
                    );
                }
                let lc = &self.session.host.battle_link_last;
                let u = lc.units;
                let p = u.prims;
                let fates = |t: &legaia_engine_battle_loop::derived_cost::PrimTally| {
                    format!(
                        "{},{},{},{}",
                        t.tris_linked + t.quads_linked,
                        t.tris_off_screen + t.quads_off_screen,
                        t.tris_culled + t.quads_culled,
                        t.near_culled
                    )
                };
                let _ = writeln!(
                    f,
                    "{tick},{st},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    w.casting.module_phase,
                    p.tris_linked,
                    p.tris_off_screen,
                    p.tris_culled,
                    p.quads_linked,
                    p.quads_off_screen,
                    p.quads_culled,
                    u.objects,
                    u.bodies,
                    u.body_objects,
                    u.ghosts,
                    u.ghost_objects,
                    u.grid_cells.map_or(-1, i64::from),
                    u.builds,
                    u.build_segments,
                    u.trail_packets,
                    u.floor_clip_prims,
                    fates(&lc.stage_tally),
                    fates(&lc.body_tally),
                );
            }
        }
        // `LEGAIA_DIAG_CAST_BODIES=1`: `bodies.csv`, every battle body a tick
        // (tick, seat, monster flag, drawn, triangles, raw position, radius,
        // HP): what a frame-cost term reads of the shot beside `cam.csv`'s
        // pose and `LEGAIA_DIAG_GPU_COST`'s linked polygons.
        if std::env::var_os("LEGAIA_DIAG_CAST_BODIES").is_some() {
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(rec.dir.join("bodies.csv"))
            {
                for (i, a) in w.actors.iter().enumerate() {
                    if !a.active {
                        continue;
                    }
                    let drawn = a.battle.render_color != 0 || a.battle.render_flag == 0;
                    let tris: u32 = match a.battle_monster_id {
                        Some(m) => a
                            .battle_body_triangles
                            .filter(|&(id, _)| id == m)
                            .map_or(0, |(_, t)| u32::from(t)),
                        None => a.battle_object_triangles.as_deref().map_or(0, |o| {
                            let d = a.battle_drawn_objects.as_deref();
                            o.iter()
                                .enumerate()
                                .filter(|&(k, _)| {
                                    d.is_none_or(|d| d.get(k).copied().unwrap_or(false))
                                })
                                .map(|(_, &t)| u32::from(t))
                                .sum()
                        }),
                    };
                    let _ = writeln!(
                        f,
                        "{tick},{i},{},{},{tris},{},{},{},{},{}",
                        u8::from(a.battle_monster_id.is_some()),
                        u8::from(drawn),
                        a.move_state.world_x,
                        a.move_state.world_y,
                        a.move_state.world_z,
                        a.battle.render_radius.unwrap_or(-1),
                        a.battle.hp,
                    );
                }
            }
        }
        // `LEGAIA_DIAG_CAST_VRAM_AT=<tick>[,<tick>..]`: the battle VRAM at
        // those ticks (`vram_<tick>.bin`, 1 MiB BGR555), for a byte comparison
        // with a retail recording's `sstate_vram.py` image.
        if std::env::var("LEGAIA_DIAG_CAST_VRAM_AT")
            .is_ok_and(|v| v.split(',').any(|t| t.trim() == tick.to_string()))
            && let Some(v) = self.battle_vram.as_ref()
        {
            let _ = std::fs::write(rec.dir.join(format!("vram_{tick}.bin")), v.as_bytes());
        }
        // Beside it, `fxpool.csv`: every live child of the effect pool
        // (`_DAT_8007BD30`) each tick - slot, frame count and cursor, the
        // integer position - the engine half of the probe's file of the
        // same name: where the burning bodies' puffs sit.
        if std::env::var_os("LEGAIA_DIAG_CAST_PARTS").is_some()
            && let Ok(f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(rec.dir.join("fxpool.csv"))
        {
            let mut f = std::io::BufWriter::new(f);
            for (i, c) in w.effect_pool.children.iter().enumerate() {
                if c.frame_count == 0 {
                    continue;
                }
                let _ = writeln!(
                    f,
                    "{tick},{i},{},{},{},{},{},{}",
                    c.frame_count,
                    c.frame_cursor,
                    c.pos[0] >> 8,
                    c.pos[1] >> 8,
                    c.pos[2] >> 8,
                    c.anim_id,
                );
            }
        }
        // And `pose.csv`: the summon creature's playing pose each tick - its
        // cursor, live pair, facing and every stream part's translation, the
        // engine half of the probe's `--pose-objects`.
        if std::env::var_os("LEGAIA_DIAG_CAST_PARTS").is_some()
            && let Some(slot) = w.casting.summon_actor_slot.map(usize::from)
            && let Some(objs) = w.battle_stream_pose_translations(slot)
            && let Some(a) = w.actors.get(slot)
            && let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(rec.dir.join("pose.csv"))
        {
            let objs: Vec<String> = objs
                .iter()
                .map(|t| format!("{}:{}:{}", t[0], t[1], t[2]))
                .collect();
            let _ = writeln!(
                f,
                "{tick},{},{},{},{},{},{},{}",
                a.battle_animation
                    .as_ref()
                    .map_or(-1, |p| i32::from(p.cursor_sixteenths())),
                a.move_state.world_x,
                a.move_state.world_y,
                a.move_state.world_z,
                a.battle.facing_angle,
                objs.len(),
                objs.join(" "),
            );
        }
        // `LEGAIA_DIAG_CAST_PARTS=1`: every live part of the cast's scene
        // graph each tick (`parts.csv`) - its record, draw words and
        // channels, the engine half of a part-pool side-by-side.
        if std::env::var_os("LEGAIA_DIAG_CAST_PARTS").is_some()
            && let Some(sc) = w.casting.active_summon.as_ref()
            && let Ok(f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(rec.dir.join("parts.csv"))
        {
            let mut f = std::io::BufWriter::new(f);
            for (i, part) in sc.parts.iter().enumerate() {
                let st = &part.state;
                let h = |o: usize| st.actor_u16(o) as i16;
                let _ = writeln!(
                    f,
                    "{tick},{i},{},{},{:#010x},{},{:#06x},{:#06x},{:#010x},{:#010x},{},{},{},{},{},{},{},{},{},{:#06x},{},{:#06x},{:#06x},{:#06x},{:#06x},{},{:#010x},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    part.model_sel,
                    u8::from(part.finished),
                    st.field_74,
                    st.field_78,
                    st.actor_u16(0x56),
                    st.actor_u16(0x9E),
                    u32::from(st.actor_u16(0xA0)) | u32::from(st.actor_u16(0xA2)) << 16,
                    u32::from(st.actor_u16(0xA4)) | u32::from(st.actor_u16(0xA6)) << 16,
                    h(0xB4),
                    h(0xB6),
                    h(0xB8),
                    h(0xBA),
                    st.world_x,
                    st.world_y,
                    st.world_z,
                    st.move_substate,
                    st.field_72,
                    st.field_52,
                    st.actor_u16(0x9C) as i16,
                    st.actor_u16(0xA8),
                    st.actor_u16(0xAC),
                    st.actor_u16(0xB0),
                    st.actor_u16(0xB2),
                    legaia_engine_effects::effect_default_arm::default_arm_vram_mesh(st)
                        .map_or(0, |m| m.positions.len()),
                    part.tag.unwrap_or(0),
                    st.flags,
                    st.render_24,
                    st.render_26,
                    st.render_28,
                    st.field_7a,
                    st.tween_src_x,
                    st.tween_src_y,
                    st.tween_src_z,
                    st.field_9c,
                    h(0xBC),
                    h(0xBE),
                    h(0xC8),
                    h(0xCA),
                );
            }
        }
        // `fxparts.csv` beside it (no header): every live part of the
        // battle's other scene graphs - `tick,scene,index,finished,flags,
        // u0 (+0xA8),x,y,z` with scene `m` (move FX), `a<n>` (effect-script
        // action FX) or `s` (stage) - so a melee burst's lifetime reads
        // part by part against the probe's `parts.csv`.
        if std::env::var_os("LEGAIA_DIAG_CAST_PARTS").is_some()
            && let Ok(f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(rec.dir.join("fxparts.csv"))
        {
            let mut f = std::io::BufWriter::new(f);
            let c = &w.casting;
            let scenes = c
                .active_move_fx
                .iter()
                .map(|s| ("m".to_string(), s))
                .chain(
                    c.active_action_fx
                        .iter()
                        .enumerate()
                        .map(|(n, s)| (format!("a{n}"), s)),
                )
                .chain(c.stage_fx.iter().map(|s| ("s".to_string(), s)));
            for (name, sc) in scenes {
                for (i, part) in sc.parts.iter().enumerate() {
                    let st = &part.state;
                    let _ = writeln!(
                        f,
                        "{tick},{name},{i},{},{:#06x},{:#06x},{},{},{},{:#010x}",
                        u8::from(part.finished),
                        st.flags,
                        st.actor_u16(0xA8),
                        st.world_x,
                        st.world_y,
                        st.world_z,
                        part.tag.unwrap_or(0),
                    );
                }
            }
        }
        for (id, kind) in &due {
            let _ = writeln!(
                rec.sfx,
                "{tick},{st},{},{id},{kind}",
                w.casting.module_phase
            );
        }
        for id in &silent {
            let _ = writeln!(rec.silent, "{tick},{st},{},{id}", w.casting.module_phase);
        }
        for (clip, channel, dur) in &clips {
            let _ = writeln!(
                rec.xa,
                "{tick},{st},{},{clip},{channel},{dur}",
                w.casting.module_phase
            );
        }
        // The live camera pose each tick (`cam.csv`), the series the retail
        // probe writes beside it: a module shot's tween reads off it.
        {
            let p = w.battle_cam_pose();
            let _ = writeln!(
                rec.cam,
                "{tick},{st},{},{},{},{},{},{},{},{},{},{},{}",
                w.casting.module_phase,
                (p.pitch as i32).rem_euclid(4096),
                (p.yaw as i32).rem_euclid(4096),
                p.tr[0] as i32,
                p.tr[1] as i32,
                p.tr[2] as i32,
                p.focus[0] as i32,
                p.focus[1] as i32,
                p.focus[2] as i32,
                w.battle.camera.as_ref().map_or(-1, |c| c.action_yaw_base()),
                w.battle.frame_clock.step,
            );
        }
        // The acting seat's clip each tick (`body.csv`), the engine half of
        // the retail probe's file of the same name: `+0x1D9`, `+0x1DA`, the
        // cursor in sixteenths of a keyframe and the live pair.
        if let Some(a) = w.actors.get(usize::from(w.battle_ctx.active_actor)) {
            let p = a.battle_animation.as_ref();
            // The body pair `+0x3C` / `+0x40`, the facing `+0x46`, and the
            // target's live and body pairs: the probe's `bx` / `bz` beside
            // the pair a strike's heading is re-aimed at.
            let pairs = |b: &legaia_engine_core::world::Actor| {
                let (bx, bz) = b
                    .battle
                    .seat
                    .unwrap_or((b.move_state.world_x, b.move_state.world_z));
                (b.move_state.world_x, b.move_state.world_z, bx, bz)
            };
            let (_, _, bx, bz) = pairs(a);
            let body = (bx, bz);
            let tgt_actor = w.actors.get(usize::from(a.battle.active_target));
            let tgt = tgt_actor.map_or((0, 0, 0, 0), pairs);
            let tfacing = tgt_actor.map_or(-1, |t| i32::from(t.battle.facing_angle));
            let _ = writeln!(
                rec.body,
                "{tick},{st},{acting},{},{},{},{},{},{},{},{},{},{},{},{},{},{tfacing}",
                a.battle.current_anim,
                a.battle.queued_anim,
                p.map_or(-1, |p| i32::from(p.is_looping())),
                p.map_or(-1, |p| i32::from(p.cursor_sixteenths())),
                a.move_state.world_x,
                a.move_state.world_z,
                body.0,
                body.1,
                a.battle.facing_angle,
                tgt.0,
                tgt.1,
                tgt.2,
                tgt.3,
            );
        }
        // `seats.csv` (no header): per tick, then per retail seat 0..7 the
        // clip, staged byte and cue-track cursor (`+0x1D9` / `+0x1DA` /
        // `+0x1F6`) and the reaction channel's entry - the engine half of the
        // probe's file: a missing anim cue is read off it.
        if let Ok(f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(rec.dir.join("seats.csv"))
        {
            let mut f = std::io::BufWriter::new(f);
            // A fourth column per seat: the reaction channel's entry (`-1`
            // idle), where a staged hit reaction plays in the engine.
            let mut cols = vec!["-1,-1,-1,-1".to_string(); 8];
            for (i, a) in w.actors.iter().enumerate().take(pc + 5) {
                if let Some(c) = cols.get_mut(seat_of(i)) {
                    *c = format!(
                        "{},{},{},{}",
                        a.battle.current_anim,
                        a.battle.queued_anim,
                        a.battle_anim_cue_cursor,
                        a.battle_reaction_entry.map_or(-1, i32::from)
                    );
                }
            }
            // Retail's creature is pool slot 7; the engine seats it elsewhere.
            if let Some(a) = w
                .casting
                .summon_actor_slot
                .and_then(|c| w.actors.get(usize::from(c)))
            {
                cols[7] = format!(
                    "{},{},{},{}",
                    a.battle.current_anim,
                    a.battle.queued_anim,
                    a.battle_anim_cue_cursor,
                    a.battle_reaction_entry.map_or(-1, i32::from)
                );
            }
            let _ = writeln!(f, "{tick},{}", cols.join(","));
        }
        // `rates.csv` (no header): per tick, then per retail seat 0..7 the
        // animation rate `+0x21D` and the node cursor `+0x68` (`-1` with no
        // clip playing) - the engine half of the probe's file, off which a
        // reaction's pace reads (a knockdown a module halves).
        if let Ok(f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(rec.dir.join("rates.csv"))
        {
            let mut f = std::io::BufWriter::new(f);
            let rate_cols = |a: &legaia_engine_core::world::Actor| {
                format!(
                    "{},{}",
                    a.battle.anim_rate.get(),
                    a.battle_animation
                        .as_ref()
                        .map_or(-1, |p| i32::from(p.cursor_sixteenths()))
                )
            };
            let mut cols = vec!["-1,-1".to_string(); 8];
            for (i, a) in w.actors.iter().enumerate().take(pc + 5) {
                if let Some(c) = cols.get_mut(seat_of(i)) {
                    *c = rate_cols(a);
                }
            }
            if let Some(a) = w
                .casting
                .summon_actor_slot
                .and_then(|c| w.actors.get(usize::from(c)))
            {
                cols[7] = rate_cols(a);
            }
            let _ = writeln!(f, "{tick},{}", cols.join(","));
        }
        // `creature_pose.csv` (no header, `LEGAIA_DIAG_CAST_PARTS`): the
        // creature's decoded objects per tick, a retail RAM image's pose pool
        // `ctx+0x6F4` (twelve bytes an object) after the creature's draw.
        if std::env::var_os("LEGAIA_DIAG_CAST_PARTS").is_some()
            && let Some(a) = w
                .casting
                .summon_actor_slot
                .and_then(|c| w.actors.get(usize::from(c)))
            && let Some(frame) = a.pose_frame.as_ref()
            && let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(rec.dir.join("creature_pose.csv"))
        {
            use std::io::Write as _;
            let cursor = a
                .battle_animation
                .as_ref()
                .map_or(-1, |p| i32::from(p.cursor_sixteenths()));
            let row: Vec<String> = frame
                .bone_outputs
                .iter()
                .map(|(p, r)| format!("{} {} {} {} {} {}", p[0], p[1], p[2], r[0], r[1], r[2]))
                .collect();
            let _ = writeln!(
                f,
                "{tick},{},{cursor},{}",
                a.battle.current_anim,
                row.join(",")
            );
        }
        // `creature.csv` (no header): the summon creature's clip, staged byte,
        // cursor in sixteenths and stand per tick, a retail RAM image's
        // `+0x1D9`, `+0x1DA`, node `+0x68` and `+0x34..+0x38`.
        if let Some(a) = w
            .casting
            .summon_actor_slot
            .and_then(|c| w.actors.get(usize::from(c)))
            && let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(rec.dir.join("creature.csv"))
        {
            use std::io::Write as _;
            let _ = writeln!(
                f,
                "{tick},{},{},{},{},{},{},{},{},{},{}",
                a.battle.current_anim,
                a.battle.queued_anim,
                a.battle_animation
                    .as_ref()
                    .map_or(-1, |p| i32::from(p.cursor_sixteenths())),
                a.move_state.world_x,
                a.move_state.world_y,
                a.move_state.world_z,
                a.battle.facing_angle,
                a.battle_animation.as_ref().map_or(0, |p| p.root_speed()),
                a.battle.active_target,
                a.battle.anim_rate.get(),
            );
        }
        // A `--screenshot` child exits on its capture tick without reaching
        // the recorder's own end: keep the files whole on every tick.
        let _ = rec.meta.flush();
        let _ = rec.cam.flush();
        let _ = rec.body.flush();
        let _ = rec.sfx.flush();
        let _ = rec.silent.flush();
        let _ = rec.xa.flush();
        if rec.done.is_some_and(|d| tick >= d + rec.tail) || rec.shots >= rec.max {
            let _ = rec.meta.flush();
            let _ = rec.cam.flush();
            let _ = rec.body.flush();
            println!(
                "[ok] cast sequence: {} frames in {} (ticks {start}..={tick})",
                rec.shots,
                rec.dir.display()
            );
            std::process::exit(0);
        }
    }

    /// A lagged battle drive's gate ([`ScreenshotConfig::display_lag`]):
    /// when the drive's action state already held `lag` ticks back, name that
    /// tick beside the frame (`<shot>.retake`) - the frame retail's display
    /// shows - for the caller to re-run the child on. A gate under `lag`
    /// ticks into its state names none: its displayed frame belongs to the
    /// state before, which a drive does not reproduce.
    fn note_display_retake(&self) {
        let Some(sc) = self.screenshot.as_ref() else {
            return;
        };
        if sc.display_lag == 0 || std::env::var_os("LEGAIA_DIAG_CAPTURE_TICK").is_some() {
            return;
        }
        // A cast gate with a lag (the battle-frame cadence's `0x32` capture)
        // names the tick the lag back whatever state it held: on that cadence
        // every tick of a pass shows the pass, so the frame is retail's
        // displayed one even across the state's entry.
        if let (Some(path), Some(_)) = (sc.path.as_ref(), sc.phase_gate.as_ref()) {
            if let Some(want) = self.tick_no.checked_sub(sc.display_lag) {
                eprintln!("display lag: retake at tick {want} (cast gate)");
                let _ = std::fs::write(path.with_extension("retake"), retake_note(sc, want));
            }
            return;
        }
        let (Some(path), Some(since)) = (sc.path.as_ref(), sc.state_since.get()) else {
            return;
        };
        // The RAM holds a pass's result and the display the result two
        // passes before it, so the retake counts back from the pass whose
        // parts are the RAM's when the gate opens on it or within a pass
        // past it: the module countdown's gate can open a pass late
        // (`battle_nilboa_neo_grude_power_up_mid_cast`, the gate's pass a
        // pass past the RAM's parts). A harness seat.
        let pass = self.session.host.world.battle.frame_clock.span();
        // The state's first pass still draws the state before it (the pass
        // that flipped the byte draws its old frame), so a retake that lands
        // on that pass has left the state; such a capture keeps the gate's
        // own frame (`battle_garmel_p2_a434_h6`: retake `0.615`, gate `0.940`).
        let base = sc
            .part_age_run
            .get()
            .filter(|&(_, last)| last <= self.tick_no && self.tick_no - last <= pass)
            .map_or(self.tick_no, |(first, _)| first);
        if let Some(want) = base
            .checked_sub(sc.display_lag)
            .filter(|&t| t >= since + pass || sc.retake_shot_only || sc.retake_across_entry)
        {
            // A magic-band shot the camera armed after the retake's tick
            // framed none of the frames before it: the retake draws the
            // framing it replaced, so it borrows no target off the gate's.
            let w = &self.session.host.world;
            let shot_after = w
                .battle
                .camera
                .as_ref()
                .and_then(|c| c.spell_shot_armed_at())
                .is_some_and(|at| at > w.clock.display_frames - (self.tick_no - want));
            if sc.retake_shot_only && !shot_after {
                return;
            }
            eprintln!("display lag: retake at tick {want} (state since {since})");
            let note = if shot_after {
                want.to_string()
            } else {
                retake_note(sc, want)
            };
            let _ = std::fs::write(path.with_extension("retake"), note);
        }
    }
}

/// The `<shot>.retake` note: the tick to take the frame on, then - when the
/// battle camera was aligned on the gate tick - the framing its law armed
/// there, which the retake's alignment tests retail's endpoints against
/// (`LEGAIA_BATTLE_CAM_GATE_TARGET`).
fn retake_note(sc: &super::super::launch_options::ScreenshotConfig, want: u64) -> String {
    match sc.gate_cam_target.get() {
        Some(t) => format!(
            "{want}\n{}",
            legaia_engine_capture::camera_align::cam_pose_to_env(&t)
        ),
        None => want.to_string(),
    }
}

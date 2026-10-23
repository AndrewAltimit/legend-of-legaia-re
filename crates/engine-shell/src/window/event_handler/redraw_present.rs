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

/// Stage the per-draw object-effect clips and NCLIP words on the renderer.
pub(super) fn stage_draw_marks(
    r: &legaia_engine_render::Renderer,
    draws_len: usize,
    color_draws_len: usize,
    clip_marks: Vec<(usize, legaia_engine_render::DrawClip)>,
    color_clip_marks: Vec<(usize, legaia_engine_render::DrawClip)>,
    nclip_marks: Vec<(usize, u32)>,
) {
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

    /// The single-shot screenshot harness (phase gate, capture, exit) or
    /// the present.
    pub(super) fn present_frame(
        &self,
        r: &legaia_engine_render::Renderer,
        scene: &RenderScene<'_>,
        screen_prims: &[legaia_render_kernels::screen_prim::ScreenPrim],
        light_prims: &[legaia_render_kernels::screen_prim::ScreenPrim],
    ) {
        let target = |scene| present_target(scene, screen_prims, light_prims);
        self.capture_cast_sequence_frame(r, &|| target(scene));
        // Screenshot harness: at the target tick, read the frame back
        // offscreen and exit instead of presenting to the window.
        let gated = self.screenshot.as_ref().is_some_and(|sc| {
            sc.path.is_some()
                && (sc.phase_gate.is_some()
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
        if let Err(e) = r.render(target(scene)) {
            log::error!("render: {e:#}");
        }
    }
}

/// The `LEGAIA_DIAG_CAST_SEQ` recorder's run state
/// ([`PlayWindowApp::capture_cast_sequence_frame`]).
struct CastSeqRecorder {
    dir: std::path::PathBuf,
    every: u64,
    tail: u64,
    max: u64,
    meta: std::io::BufWriter<std::fs::File>,
    /// `cam.csv`: the live camera pose per tick.
    cam: std::io::BufWriter<std::fs::File>,
    /// Only a cast by this acting seat starts the recording
    /// (`LEGAIA_DIAG_CAST_SEQ_SEAT`); any seat when unset.
    seat: Option<u8>,
    start: Option<u64>,
    done: Option<u64>,
    shots: u64,
}

/// Action state `0x28`, cast begin: the state every Magic action - a party
/// summon, a monster spell, a capture-class cast - passes through first.
const CAST_BEGIN_STATE: u8 = 0x28;

impl PlayWindowApp {
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
            let mut meta = std::io::BufWriter::new(std::fs::File::create(dir.join("meta.csv")).ok()?);
            use std::io::Write as _;
            let _ = writeln!(
                meta,
                "tick,st7,phase,acting,frames,c6d8,cd,yaw,c87c,fade_white,fade_age,hp0,hp1,hp2,hp3,hp4,hp5,hp6,hp7,shot"
            );
            let mut cam = std::io::BufWriter::new(std::fs::File::create(dir.join("cam.csv")).ok()?);
            let _ = writeln!(cam, "tick,st7,phase,pitch,yaw,trx,try,trz");
            Some(std::sync::Mutex::new(CastSeqRecorder {
                cam,
                every: num("LEGAIA_DIAG_CAST_SEQ_EVERY", 1).max(1),
                tail: num("LEGAIA_DIAG_CAST_SEQ_TAIL", 60),
                max: num("LEGAIA_DIAG_CAST_SEQ_MAX", 2000),
                seat: std::env::var("LEGAIA_DIAG_CAST_SEQ_SEAT")
                    .ok()
                    .and_then(|v| v.parse().ok()),
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
            None if st == CAST_BEGIN_STATE && rec.seat.is_none_or(|s| s == acting) => {
                eprintln!("cast sequence: cast begin at tick {tick} (seat {acting})");
                rec.start = Some(tick);
                tick
            }
            None => return,
        };
        if rec.done.is_none() && matches!(st, 0x50 | 0x51 | 0x5A) {
            eprintln!("cast sequence: done band at tick {tick} (state 0x{st:02X})");
            rec.done = Some(tick);
        }
        let mut shot = String::new();
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
                    match write_capture_png(&rec.dir.join(&name), &img) {
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
        let (fade_white, fade_age) =
            w.presentation
                .fade
                .as_ref()
                .filter(|f| f.kind == 1)
                .map_or((-1, -1), |f| {
                    (
                        i32::from(f.target_rgb()[0] == 0xFF),
                        i32::from(f.age_vsyncs()),
                    )
                });
        let accum = w
            .battle
            .camera
            .as_ref()
            .map_or(-1, |c| i64::from(c.close_up_accum()));
        use std::io::Write as _;
        let _ = writeln!(
            rec.meta,
            "{tick},{st},{},{acting},{},{},{},{},{accum},{fade_white},{fade_age},{},{shot}",
            w.casting.module_phase,
            w.clock.display_frames,
            w.battle_ctx.frame_timer,
            w.casting.module_cam.countdown.0,
            w.casting.module_cam.yaw_base,
            hp.join(","),
        );
        // The live camera pose each tick (`cam.csv`), the series the retail
        // probe writes beside it: a module shot's tween reads off it.
        {
            let p = w.battle_cam_pose();
            let _ = writeln!(
                rec.cam,
                "{tick},{st},{},{},{},{},{},{}",
                w.casting.module_phase,
                (p.pitch as i32).rem_euclid(4096),
                (p.yaw as i32).rem_euclid(4096),
                p.tr[0] as i32,
                p.tr[1] as i32,
                p.tr[2] as i32,
            );
        }
        if rec.done.is_some_and(|d| tick >= d + rec.tail) || rec.shots >= rec.max {
            let _ = rec.meta.flush();
            let _ = rec.cam.flush();
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
        if let Some(want) = self
            .tick_no
            .checked_sub(sc.display_lag)
            .filter(|&t| t >= since)
        {
            eprintln!("display lag: retake at tick {want} (state since {since})");
            let _ = std::fs::write(path.with_extension("retake"), retake_note(sc, want));
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

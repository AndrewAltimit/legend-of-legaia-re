//! The redraw's submit half: the draw census diagnostic, the per-draw
//! marks staged on the renderer, the screenshot harness (sweep and
//! single-shot capture) and the present - split out of `handle_redraw`.

use super::super::*;
use super::redraw::present_target;

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
pub(super) fn capture_sweep_frame(
    r: &legaia_engine_render::Renderer,
    target: RenderTarget<'_>,
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
        match r.capture_rgba(target) {
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
            type Census = legaia_engine_core::draw_census::MeshCensus;
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
            let rows = legaia_engine_core::draw_census::family_rows(census_draws(), 320.0, 240.0);
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
                    legaia_engine_core::draw_census::family_tris_jsonl(
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
                legaia_engine_core::draw_census::family_rows_jsonl(&rows),
            );
        }
    }

    /// The single-shot screenshot harness (phase gate, capture, exit) or
    /// the present.
    pub(super) fn present_frame(
        &self,
        r: &legaia_engine_render::Renderer,
        scene: &RenderScene<'_>,
        screen_prims: &[legaia_engine_render::screen_overlay::ScreenPrim],
        light_prims: &[legaia_engine_render::screen_overlay::ScreenPrim],
    ) {
        let target = |scene| present_target(scene, screen_prims, light_prims);
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
            let path = self
                .screenshot
                .as_ref()
                .and_then(|sc| sc.path.clone())
                .unwrap();
            // Read the frame back until two consecutive readbacks of
            // this same frame agree. Re-rendering an unchanged target is
            // deterministic, yet under heavy machine load a readback
            // has come back with its top rows still zero - a black band
            // of up to 15 rows whose lower edge steps every 32 columns
            // (GPU tiles), the rest of the frame byte-identical - on a
            // few runs in ten of the retail-compare corpus (never with
            // the child run alone; see `Renderer::capture_rgba`).
            // A band is not a frame the scene drew, so the capture is
            // the first one a second readback reproduces.
            let mut capture = r.capture_rgba(target(scene));
            for retry in 1..=4 {
                let Ok(prev) = &capture else { break };
                match r.capture_rgba(target(scene)) {
                    Ok(next) if next.rgba == prev.rgba => break,
                    next => {
                        eprintln!(
                            "screenshot readback {retry} disagreed with the previous one; reading again"
                        );
                        capture = next;
                    }
                }
            }
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

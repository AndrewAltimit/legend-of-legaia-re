//! The opt-in battle frame cost's host half: price the frame this redraw
//! draws on the GPU model (`legaia_engine_system::gpu_draw_cost`) and hand
//! the wait to the world, whose next battle pass takes the larger of it and
//! its CPU term (`WorldToggles::battle_gpu_cost_step`,
//! `World::note_drawn_table_cost`).
//!
//! What is priced: every textured and untextured scene draw through its own
//! view-projection and NCLIP word (the meshes' CPU copies,
//! `Renderer::set_keep_cpu_meshes`), and every screen primitive as a quad
//! packet. The text and sprite overlays (the HUD's panels and digits) are not.

use super::super::*;
use legaia_engine_system::gpu_draw_cost::{self as gc, Faces, MeshStreams, PolyKind, TableCost};

/// `LEGAIA_DIAG_GPU_COST=<path>`: the per-frame cost log, priced whether or
/// not the frame step reads it. Read once.
pub(in crate::window) fn gpu_cost_diag_path() -> Option<&'static std::path::Path> {
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| std::env::var_os("LEGAIA_DIAG_GPU_COST").map(Into::into))
        .as_deref()
}

/// The faces a draw keeps under its per-draw NCLIP word, else the frame's.
fn faces_of(marks: &[(usize, u32)], i: usize, frame_word: u32) -> Faces {
    let word = marks
        .iter()
        .rev()
        .find(|(k, _)| *k == i)
        .map_or(frame_word, |(_, w)| *w);
    Faces::of_word(word)
}

fn streams(cpu: &legaia_engine_render::MeshCpuCopy) -> MeshStreams<'_> {
    MeshStreams {
        positions: &cpu.positions,
        words: &cpu.words,
        colors: &cpu.colors,
        indices: &cpu.indices,
        textured: cpu.textured,
    }
}

/// The mesh draws' part of a battle frame's table.
pub(super) fn price_scene_draws(
    draws: &[SceneDraw<'_>],
    color_draws: &[ColorSceneDraw<'_>],
    nclip: &[(usize, u32)],
    color_nclip: &[(usize, u32)],
    frame_word: u32,
) -> TableCost {
    let mut cost = TableCost::new(gc::BATTLE_OT_LEN);
    for (i, d) in draws.iter().enumerate() {
        if let Some(cpu) = d.mesh.cpu_copy() {
            let m = d.mvp.to_cols_array();
            gc::add_mesh_draw(&mut cost, streams(cpu), &m, faces_of(nclip, i, frame_word));
        }
    }
    for (i, d) in color_draws.iter().enumerate() {
        if let Some(cpu) = d.mesh.cpu_copy() {
            let m = d.mvp.to_cols_array();
            gc::add_mesh_draw(
                &mut cost,
                streams(cpu),
                &m,
                faces_of(color_nclip, i, frame_word),
            );
        }
    }
    cost
}

/// The screen primitives' part: each a four-corner packet in the display's
/// own coordinates.
pub(super) fn price_screen_prims(
    cost: &mut TableCost,
    prims: &[legaia_render_kernels::screen_prim::ScreenPrim],
) {
    use legaia_render_kernels::screen_prim::ScreenPrim;
    for p in prims {
        let (shaded, textured, semi) = match p {
            ScreenPrim::Textured(q) => (q.gouraud.is_some(), true, q.semi_transparent),
            ScreenPrim::Flat(q) => (q.gouraud.is_some(), false, q.semi_transparent),
        };
        let v = p.corners().map(|(x, y)| [i32::from(x), i32::from(y)]);
        cost.add_quad(
            v,
            PolyKind {
                quad: true,
                shaded,
                textured,
                semi,
            },
        );
    }
}

impl PlayWindowApp {
    /// Whether this redraw prices its battle frame: for the world's frame
    /// step (`WorldToggles::battle_gpu_cost_step`) or for the diagnostic
    /// alone ([`gpu_cost_diag_path`]), in a battle.
    pub(super) fn gpu_cost_pricing(&self) -> bool {
        let w = &self.session.host.world;
        (w.toggles.battle_gpu_cost_step
            || w.toggles.battle_poly_frame_cost
            || gpu_cost_diag_path().is_some())
            && w.mode == SceneMode::Battle
    }

    /// Hand the priced frame to the world. `LEGAIA_DIAG_GPU_COST=<path>`
    /// appends a row a frame: the tick, the battle frame, action state, module
    /// phase and frame step, the draw and DMA hblanks and the polygons, beside
    /// a retail `gpu.csv`'s `draw` / `dma` / `polys`.
    pub(super) fn note_battle_gpu_cost(&mut self, cost: TableCost) {
        let (draw, dma) = cost.hblanks();
        if let Some(path) = gpu_cost_diag_path() {
            use std::io::Write;
            let new = !path.exists();
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                if new {
                    let _ = writeln!(f, "tick,frame,state,phase,step,draw,dma,polys");
                }
                let w = &self.session.host.world;
                let _ = writeln!(
                    f,
                    "{},{},{},{},{},{draw:.1},{dma:.1},{}",
                    self.tick_no,
                    w.battle_frame_id(),
                    w.battle_ctx.action_state,
                    w.casting.module_phase,
                    w.battle.frame_clock.step,
                    cost.polys
                );
            }
        }
        // Stored either way; the next pass reads it only under the toggle.
        self.session
            .host
            .world
            .note_drawn_table_cost(cost.total_hblanks());
        self.session.host.world.note_drawn_polys(cost.polys);
    }
}

//! `asset-viewer battle-scene` boots a battle scene driven by the
//! engine-vm battle-action state machine and renders the assembled party +
//! monster meshes under the orbit camera with a live SM HUD.

use crate::cli::Bundle;
use crate::common::{WorldActorMesh, keymap_pad, mesh_aabb};
use anyhow::{Context, Result};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_render::glam::{Mat4, Vec3};
use legaia_engine_render::{
    RenderTarget, Scene as RenderScene, SceneDraw, TextDraw, TextOverlay, UploadedFontAtlas,
    UploadedVram,
    legaia_tim::Vram,
    text_draws_for,
    window::{EngineWindow, orbit_camera_mvp},
};
use legaia_font::Font;
use std::path::{Path, PathBuf};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowId;

/// Boot a battle scene driven by the engine-vm battle-action state machine.
///
/// Loads battle bundle TMDs, builds a `World` in `SceneMode::Battle` with
/// 3 party + 5 monster slots, queues the supplied action ID, and ticks
/// the world per frame. HUD shows: current `ActionState` (decoded into
/// the human-readable variant name), queued action, per-slot liveness,
/// and any battle-end cause emitted by the SM.
///
/// Inputs:
///  - Triangle: cycle queued_action through 0..=5 (Tactical / Item /
///    Magic / Attack / Spirit / Run).
///  - Cross: re-trigger the SM by writing `ActionState::Begin` into the
///    ctx (useful when the SM lands in a Stay-forever state).
///  - Esc: quit.
pub(crate) fn run_battle_scene(extracted_root: &Path, queued_action: u8) -> Result<()> {
    use legaia_engine_core::world::{SceneMode, World};

    let prot_path = extracted_root.join("PROT.DAT");
    if !prot_path.exists() {
        anyhow::bail!("missing {} (run legaia-extract first)", prot_path.display());
    }
    let font = Font::load_from_extracted(extracted_root).with_context(|| {
        format!(
            "load extracted font under {} (run `legaia-extract` - it writes extracted/font/ - or `font-extract --disc <bin>`)",
            extracted_root.display()
        )
    })?;
    let bundle_dirs = Bundle::Battle.dirs(extracted_root);
    let bundle_dirs_ref: Vec<&Path> = bundle_dirs.iter().map(|p| p.as_path()).collect();
    let (vram, tim_count) = legaia_tmd::vram_targeted::build_vram_from_dirs(&bundle_dirs_ref);
    log::info!(
        "battle bundle: {} TIM(s) across {} dir(s)",
        tim_count,
        bundle_dirs.len()
    );
    // Pull TMDs from the same bundle dirs the field viewer pulls from a
    // scene; cap at the 8-slot battle actor table.
    let mut tmd_paths: Vec<PathBuf> = Vec::new();
    for dir in &bundle_dirs {
        let entries = std::fs::read_dir(dir).ok();
        if let Some(entries) = entries {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "tmd") {
                    tmd_paths.push(p);
                    if tmd_paths.len() >= 8 {
                        break;
                    }
                }
            }
        }
        if tmd_paths.len() >= 8 {
            break;
        }
    }
    let actor_count = tmd_paths.len();
    log::info!("battle scene: {actor_count} actor TMDs loaded from battle bundle");

    let mut world = World::default();
    // 3 party + however many monsters the bundle gave us, capped to 5.
    let party = 3.min(actor_count) as u8;
    let monsters = actor_count.saturating_sub(party as usize).min(5) as u8;
    world.enter_battle(party, monsters);
    let _ = SceneMode::Battle; // import kept stable for readers
    // Queue the requested action - enter_battle seeded action_state at Begin.
    world.battle_ctx.queued_action = queued_action;

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = BattleSceneApp {
        title: format!("Battle scene - {actor_count} actors, queued_action={queued_action}"),
        actor_count,
        win: EngineWindow::new(),
        font,
        font_atlas: None,
        vram_cpu: Some(vram),
        uploaded_vram: None,
        tmd_paths,
        meshes: Vec::new(),
        world,
        // Bound the retail seat extent (X to +-900, Z to +-1400 - see
        // engine-core::battle_seats).
        scene_aabb: ([-1000.0, -200.0, -1500.0], [1000.0, 600.0, 1500.0]),
        input: InputState::new(),
        last_dt_ms: 16,
        battle_stats: BattleSmStats::default(),
        prev_input_pad: 0,
    };
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Per-frame battle-action StepOutcome counts + last cause for HUD.
#[derive(Default, Clone)]
struct BattleSmStats {
    stay: u64,
    transition: u64,
    complete: u64,
    unknown: u64,
    last_state_byte: u8,
    last_to_state_byte: u8,
    last_complete_cause: Option<legaia_engine_vm::battle_action::BattleEndCause>,
}

/// Battle-mode viewer state. Shape mirrors `FieldApp` minus the per-record
/// cycle bookkeeping (battles are a single state machine, not a record
/// sequence).
struct BattleSceneApp {
    title: String,
    actor_count: usize,
    win: EngineWindow,
    font: Font,
    font_atlas: Option<UploadedFontAtlas>,
    vram_cpu: Option<Vram>,
    uploaded_vram: Option<UploadedVram>,
    tmd_paths: Vec<PathBuf>,
    meshes: Vec<WorldActorMesh>,
    world: legaia_engine_core::world::World,
    scene_aabb: ([f32; 3], [f32; 3]),
    input: InputState,
    last_dt_ms: u32,
    battle_stats: BattleSmStats,
    /// Pad-mask snapshot from the previous tick - used for edge-trigger
    /// detection on Triangle / Cross.
    prev_input_pad: u16,
}

impl BattleSceneApp {
    fn upload_assets(&mut self) {
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        for path in &self.tmd_paths {
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("skip TMD {}: read error: {e}", path.display());
                    continue;
                }
            };
            let tmd = match legaia_tmd::parse(&bytes) {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("skip TMD {}: parse error: {e}", path.display());
                    continue;
                }
            };
            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &bytes);
            if vmesh.indices.is_empty() {
                continue;
            }
            let aabb = mesh_aabb(&vmesh.positions);
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.colors,
                &vmesh.indices,
            ) {
                Ok(mesh) => self.meshes.push(WorldActorMesh {
                    mesh,
                    aabb_lo: aabb.0,
                    aabb_hi: aabb.1,
                }),
                Err(e) => log::warn!("skip TMD {}: upload error: {e}", path.display()),
            }
            if self.meshes.len() >= self.actor_count {
                break;
            }
        }
        if let Some(vram_cpu) = self.vram_cpu.take() {
            match r.upload_vram(&vram_cpu) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("VRAM upload failed: {e:#}"),
            }
        }
        match r.upload_font(&self.font) {
            Ok(a) => self.font_atlas = Some(a),
            Err(e) => log::error!("font atlas upload failed: {e:#}"),
        }
    }

    fn tick_battle_frame(&mut self) {
        use legaia_engine_vm::battle_action::StepOutcome;
        let outcome = self.world.tick();
        match outcome {
            Some(StepOutcome::Stay) => {
                self.battle_stats.stay += 1;
            }
            Some(StepOutcome::Transition { from, to }) => {
                self.battle_stats.transition += 1;
                self.battle_stats.last_state_byte = from;
                self.battle_stats.last_to_state_byte = to;
            }
            Some(StepOutcome::BattleComplete) => {
                self.battle_stats.complete += 1;
                self.battle_stats.last_complete_cause = self.world.battle_end;
            }
            Some(StepOutcome::UnknownState { state }) => {
                self.battle_stats.unknown += 1;
                self.battle_stats.last_state_byte = state;
            }
            None => {}
        }
    }

    /// Edge-trigger handler: on Triangle press cycle queued_action; on
    /// Cross press re-seed the SM at Begin so we can watch it run again.
    fn handle_edges(&mut self) {
        let pad = self.input.pad();
        let pressed_now = pad & !self.prev_input_pad;
        if pressed_now & PadButton::Triangle.mask() != 0 {
            let next = (self.world.battle_ctx.queued_action + 1) % 6;
            self.world.battle_ctx.queued_action = next;
            log::info!("queued_action -> {next}");
        }
        if pressed_now & PadButton::Cross.mask() != 0 {
            self.world.battle_ctx.action_state =
                legaia_engine_vm::battle_action::ActionState::Begin.as_byte();
            log::info!("re-seeding SM at ActionState::Begin");
        }
        self.prev_input_pad = pad;
    }

    fn camera_mvp(&self, aspect: f32) -> Mat4 {
        orbit_camera_mvp(
            self.scene_aabb.0,
            self.scene_aabb.1,
            0.15,
            0.45,
            self.win.elapsed_secs(),
            aspect,
        )
    }

    fn actor_model(&self, slot: usize) -> Mat4 {
        let a = &self.world.actors[slot];
        let pos = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        Mat4::from_translation(pos) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
    }

    fn build_hud(&self) -> Vec<TextDraw> {
        if self.font_atlas.is_none() {
            return Vec::new();
        }
        use legaia_engine_vm::battle_action::ActionState;

        let mut out = Vec::new();
        let white = [1.0, 1.0, 1.0, 1.0];
        let dim = [0.7, 0.85, 1.0, 1.0];
        let warn = [1.0, 0.6, 0.4, 1.0];

        let line1 = format!("battle  actors {}", self.actor_count);
        out.extend(text_draws_for(
            &self.font.layout_ascii(&line1),
            (8, 8),
            white,
        ));
        let fps = 1000u32.checked_div(self.last_dt_ms).unwrap_or(0);
        let line2 = format!(
            "frame {}   {:>3} fps   t {:.1}s",
            self.world.frame,
            fps,
            self.win.elapsed_secs()
        );
        out.extend(text_draws_for(
            &self.font.layout_ascii(&line2),
            (8, 26),
            dim,
        ));

        let state_byte = self.world.battle_ctx.action_state;
        let state_name = ActionState::from_byte(state_byte)
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|| format!("Unknown(0x{state_byte:02X})"));
        let line3 = format!(
            "state {} (0x{:02X})  queued {}",
            state_name, state_byte, self.world.battle_ctx.queued_action
        );
        out.extend(text_draws_for(
            &self.font.layout_ascii(&line3),
            (8, 44),
            white,
        ));

        let line4 = format!(
            "stay {}  transition {}  complete {}  unknown {}",
            self.battle_stats.stay,
            self.battle_stats.transition,
            self.battle_stats.complete,
            self.battle_stats.unknown,
        );
        let color = if self.battle_stats.unknown > 0 {
            warn
        } else {
            dim
        };
        out.extend(text_draws_for(
            &self.font.layout_ascii(&line4),
            (8, 62),
            color,
        ));

        let mut alive = String::with_capacity(40);
        alive.push_str("alive ");
        for i in 0..self.actor_count.min(8) {
            alive.push_str(&format!(
                "{}{} ",
                if i < 3 { 'P' } else { 'M' },
                self.world.actors[i].battle.liveness
            ));
        }
        out.extend(text_draws_for(
            &self.font.layout_ascii(&alive),
            (8, 80),
            dim,
        ));

        if let Some(cause) = self.battle_stats.last_complete_cause {
            let line = format!("battle complete: {cause:?}");
            out.extend(text_draws_for(
                &self.font.layout_ascii(&line),
                (8, 98),
                warn,
            ));
        }

        out
    }
}

impl ApplicationHandler for BattleSceneApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if !self.win.open(evl, &self.title) {
            return;
        }
        self.upload_assets();
        self.win.request_redraw();
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => evl.exit(),
            WindowEvent::Resized(size) => self.win.handle_resize(size.width, size.height),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        ..
                    },
                ..
            } => {
                if matches!(code, KeyCode::Escape) && state == ElementState::Pressed {
                    evl.exit();
                    return;
                }
                if let Some(button) = keymap_pad(code) {
                    let mut mask = self.input.pad();
                    if state == ElementState::Pressed {
                        mask |= button.mask();
                    } else {
                        mask &= !button.mask();
                    }
                    self.input.set_pad(mask);
                }
            }
            WindowEvent::RedrawRequested => {
                let dt = self.win.advance_tick(1000);
                self.last_dt_ms = dt.as_millis().min(1000) as u32;
                let target_frames = EngineWindow::frames_for(dt, 8);
                self.handle_edges();
                for _ in 0..target_frames {
                    self.tick_battle_frame();
                }
                if let (Some(r), Some(vram), Some(atlas)) = (
                    self.win.renderer.as_ref(),
                    self.uploaded_vram.as_ref(),
                    self.font_atlas.as_ref(),
                ) {
                    let (w, h) = r.surface_size();
                    let aspect = w as f32 / h.max(1) as f32;
                    let cam = self.camera_mvp(aspect);
                    let draws: Vec<SceneDraw<'_>> = self
                        .meshes
                        .iter()
                        .enumerate()
                        .map(|(slot, m)| SceneDraw {
                            mesh: &m.mesh,
                            mvp: cam * self.actor_model(slot),
                        })
                        .collect();
                    let hud = self.build_hud();
                    let overlay = TextOverlay { atlas, draws: &hud };
                    let scene = RenderScene {
                        vram,
                        draws: &draws,
                        color_draws: &[],
                        overlay_lines: None,
                        overlay_sprites: None,
                        overlay_sprites_2: None,
                        overlay_text: Some(&overlay),
                        clear_color: None,
                    };
                    if let Err(e) = r.render(RenderTarget::Scene(&scene)) {
                        log::error!("render error: {e:#}");
                    }
                }
                self.win.request_redraw();
            }
            _ => {}
        }
    }
}

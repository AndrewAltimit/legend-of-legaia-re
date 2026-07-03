//! `asset-viewer world <SCENE>` runs the engine-core `World` composite over a
//! real CDNAME scene. It loads up to N TMDs from `<extracted>/tmd_scan/<entry>/`
//! for entries inside the scene's CDNAME block, builds VRAM from the matching
//! `tim_scan/` dirs, spawns one actor per TMD, and ticks the World at ~60 Hz.
//!
//! Each actor's MVP is computed from its `move_state.world_x/y/z`. The default
//! path animates positions analytically (sinusoidal orbit) so the multi-actor
//! renderer is exercised without depending on real per-scene move bytecode.
//! `--with-move-vm` instead loads a synthetic `WORLD_SET → WAIT_SET → HALT`
//! program per actor so the move-VM port runs every tick.

use crate::common::{WorldActorMesh, collect_scene_tim_dirs, collect_scene_tmds, mesh_aabb};
use anyhow::{Context, Result};
use legaia_engine_render::glam::{Mat4, Vec3};
use legaia_engine_render::{
    RenderTarget, Scene as RenderScene, SceneDraw, UploadedVram,
    legaia_tim::Vram,
    window::{EngineWindow, orbit_camera_mvp},
};
use std::path::{Path, PathBuf};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowId;

pub(crate) fn run_world(
    scene_name: &str,
    max_actors: usize,
    extracted_root: &Path,
    with_move_vm: bool,
) -> Result<()> {
    use legaia_engine_core::scene::ProtIndex;
    use legaia_engine_core::world::{SceneMode, World};

    if max_actors == 0 {
        anyhow::bail!("max_actors must be >= 1");
    }

    let prot_path = extracted_root.join("PROT.DAT");
    let cdname_path = extracted_root.join("CDNAME.TXT");
    if !prot_path.exists() {
        anyhow::bail!("missing {} (run legaia-extract first)", prot_path.display());
    }
    if !cdname_path.exists() {
        anyhow::bail!(
            "missing {} (run legaia-extract first)",
            cdname_path.display()
        );
    }
    let index = ProtIndex::open_extracted(extracted_root)
        .with_context(|| format!("open ProtIndex at {}", extracted_root.display()))?;
    let (start, end) = index.block_range(scene_name).ok_or_else(|| {
        anyhow::anyhow!(
            "scene '{}' not found in CDNAME map at {}",
            scene_name,
            cdname_path.display()
        )
    })?;
    log::info!(
        "scene '{}' covers PROT [{}..{}) ({} entries)",
        scene_name,
        start,
        end,
        end - start
    );

    // Collect TMDs from the scene block's tmd_scan/ subdirs.
    let tmd_paths = collect_scene_tmds(extracted_root, start, end);
    if tmd_paths.is_empty() {
        anyhow::bail!(
            "no TMDs found in tmd_scan for scene '{}' (PROT block {}..{})",
            scene_name,
            start,
            end
        );
    }
    let actor_count = tmd_paths.len().min(max_actors);
    log::info!(
        "loaded {} TMD(s) under tmd_scan; spawning {} actor(s)",
        tmd_paths.len(),
        actor_count
    );

    // Build a shared VRAM from every tim_scan/ dir in the scene block.
    let tim_dirs = collect_scene_tim_dirs(extracted_root, start, end);
    let tim_dir_refs: Vec<&Path> = tim_dirs.iter().map(|p| p.as_path()).collect();
    let (vram, tim_count) = legaia_tmd::vram_targeted::build_vram_from_dirs(&tim_dir_refs);
    log::info!(
        "built VRAM from {} TIM(s) across {} tim_scan dir(s)",
        tim_count,
        tim_dirs.len()
    );

    // Build the world composite + spawn the actors with the picked TMDs.
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    let radius = 800.0_f32;
    for i in 0..actor_count {
        let theta = (i as f32) * std::f32::consts::TAU / (actor_count as f32);
        let x = (radius * theta.cos()) as i16;
        let z = (radius * theta.sin()) as i16;
        let actor = world.spawn_actor(i);
        actor.move_state.world_x = x;
        actor.move_state.world_y = 0;
        actor.move_state.world_z = z;
        if with_move_vm {
            // Synthetic: WORLD_SET (x, y, z) → WAIT_SET 8 → HALT.
            world.set_move_bytecode(
                i,
                Some(vec![0x0007, x as u16, 0, z as u16, 0x0009, 8, 0x0008]),
            );
        }
    }

    // Hand off to the windowing loop.
    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = WorldApp {
        title: format!(
            "World - scene '{}' [{} actor(s), tim_count={}]",
            scene_name, actor_count, tim_count
        ),
        win: EngineWindow::new(),
        vram_cpu: Some(vram),
        uploaded_vram: None,
        tmd_paths,
        meshes: Vec::new(),
        world,
        actor_count,
        with_move_vm,
        scene_aabb: ([-radius, -200.0, -radius], [radius, 600.0, radius]),
    };
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Multi-actor world viewer state. Owned by the winit event loop.
struct WorldApp {
    title: String,
    win: EngineWindow,
    vram_cpu: Option<Vram>,
    uploaded_vram: Option<UploadedVram>,
    tmd_paths: Vec<PathBuf>,
    meshes: Vec<WorldActorMesh>,
    world: legaia_engine_core::world::World,
    actor_count: usize,
    with_move_vm: bool,
    /// Synthetic AABB enclosing every spawn point - drives the camera.
    scene_aabb: ([f32; 3], [f32; 3]),
}

impl WorldApp {
    fn upload_meshes(&mut self) {
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
                log::warn!(
                    "skip TMD {}: zero triangles after primitive walk",
                    path.display()
                );
                continue;
            }
            let aabb = mesh_aabb(&vmesh.positions);
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
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
        if self.meshes.is_empty() {
            log::error!("no TMDs uploaded successfully - nothing to draw");
        }
        if let Some(vram_cpu) = self.vram_cpu.take() {
            match r.upload_vram(&vram_cpu) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("VRAM upload failed: {e:#}"),
            }
        }
        // Recompute scene AABB from the union of every mesh's local AABB +
        // its current position so the camera frames the whole scene.
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for (slot, m) in self.meshes.iter().enumerate() {
            let actor = &self.world.actors[slot];
            let cx = actor.move_state.world_x as f32;
            let cy = actor.move_state.world_y as f32;
            let cz = actor.move_state.world_z as f32;
            for ax in 0..3 {
                let lo_world = [m.aabb_lo[0] + cx, m.aabb_lo[1] + cy, m.aabb_lo[2] + cz][ax];
                let hi_world = [m.aabb_hi[0] + cx, m.aabb_hi[1] + cy, m.aabb_hi[2] + cz][ax];
                if lo_world < lo[ax] {
                    lo[ax] = lo_world;
                }
                if hi_world > hi[ax] {
                    hi[ax] = hi_world;
                }
            }
        }
        if lo[0].is_finite() && hi[0].is_finite() {
            self.scene_aabb = (lo, hi);
        }
    }

    /// Compute the camera MVP for this frame. Orbits the scene center.
    fn camera_mvp(&self, aspect: f32) -> Mat4 {
        orbit_camera_mvp(
            self.scene_aabb.0,
            self.scene_aabb.1,
            0.25,
            0.4,
            self.win.elapsed_secs(),
            aspect,
        )
    }

    /// Per-actor model matrix. PSX has Y-down geometry - flip Y in the
    /// model so the meshes appear right-side-up in the Y-up camera.
    fn actor_model(&self, slot: usize) -> Mat4 {
        let a = &self.world.actors[slot];
        let pos = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        // Slight per-actor spin so individual meshes are visibly animated.
        let spin = self.win.elapsed_secs() * 0.6 + (slot as f32) * std::f32::consts::FRAC_PI_2;
        Mat4::from_translation(pos)
            * Mat4::from_rotation_y(spin)
            * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
    }
}

impl ApplicationHandler for WorldApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if !self.win.open(evl, &self.title) {
            return;
        }
        self.upload_meshes();
        self.win.request_redraw();
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => evl.exit(),
            WindowEvent::Resized(size) => self.win.handle_resize(size.width, size.height),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => evl.exit(),
            WindowEvent::RedrawRequested => {
                let dt = self.win.advance_tick(1000);
                let target_frames = EngineWindow::frames_for(dt, 8);
                for _ in 0..target_frames {
                    self.world.tick();
                }
                // Analytic motion for the demo: gently orbit each actor's
                // initial position. Move-VM mode (when wired in) drives
                // positions through the move bytecode instead, so only
                // animate analytically when the VM isn't.
                if !self.with_move_vm {
                    let t = self.win.elapsed_secs();
                    for slot in 0..self.actor_count {
                        let actor = &mut self.world.actors[slot];
                        let theta = (slot as f32) * std::f32::consts::TAU
                            / (self.actor_count as f32)
                            + t * 0.3;
                        let r = 800.0_f32;
                        actor.move_state.world_x = (r * theta.cos()) as i16;
                        actor.move_state.world_z = (r * theta.sin()) as i16;
                        actor.move_state.world_y = (60.0 * (t + slot as f32).sin()) as i16;
                    }
                }
                if let (Some(r), Some(vram)) =
                    (self.win.renderer.as_ref(), self.uploaded_vram.as_ref())
                {
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
                    let scene = RenderScene {
                        vram,
                        draws: &draws,
                        color_draws: &[],
                        overlay_lines: None,
                        overlay_sprites: None,
                        overlay_sprites_2: None,
                        overlay_text: None,
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

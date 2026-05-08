//! Top-level engine driver. The "single command" that turns extracted-disc
//! bytes into a runtime view of any CDNAME scene.
//!
//! Subcommands:
//!
//! - `info` — headless one-line summary of a scene's resolved asset chain.
//! - `list-scenes` — every CDNAME scene name with its PROT range.
//! - `play` — headless engine tick: world + camera + audio, no window.
//! - `play-window` — windowed engine: opens a wgpu surface, renders scene
//!   TMDs against the software PSX VRAM each frame. Input: arrows = D-pad,
//!   Z = Cross, Esc = quit.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use glam::{Mat4, Vec3};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneTickEvent};
use legaia_engine_core::scene_assets::SceneAssets;
use legaia_engine_core::scene_resources::SceneResources;
use legaia_engine_core::world::SceneMode;
use legaia_engine_core::world_map::WorldMapController;
use legaia_engine_render::{
    RenderTarget, Scene as RenderScene, SceneDraw, TextDraw, TextOverlay, UploadedFontAtlas,
    UploadedVram, UploadedVramMesh, text_draws_for,
    window::{EngineWindow, orbit_camera_mvp},
};
use legaia_engine_shell::{BootConfig, BootSession};
use legaia_font::Font;
use legaia_prot::cdname;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowId;

#[derive(Parser, Debug)]
#[command(
    name = "legaia-engine",
    about = "Top-level driver for the Legaia clean-room engine. Boots a CDNAME scene from extracted PROT bytes."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Build [`SceneResources`] for one scene and print a summary line. Use
    /// this to verify the asset chain produces the right state without
    /// firing up the windowed viewer.
    Info {
        /// CDNAME scene name (e.g. `town01`, `dolk`, `cave01`).
        #[arg(long)]
        scene: String,
        /// Extracted-root directory containing `PROT.DAT` + `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
    },
    /// List every distinct scene name the CDNAME map exposes, with the
    /// PROT entry range each one covers.
    ListScenes {
        /// Extracted-root directory containing `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
    },
    /// Save the current world's empty/default party to a slot file.
    /// Useful as a smoke test for the disk save path; engines drive this
    /// from menu mode at runtime.
    Save {
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        #[arg(long, default_value = "saves")]
        save_dir: PathBuf,
        #[arg(long, default_value_t = 0)]
        slot: u8,
        /// Number of party-record entries to materialise in the save.
        #[arg(long, default_value_t = 3)]
        party_size: usize,
    },
    /// Load a slot file into a fresh world and print the resulting roster
    /// shape. Mirror of `save` for round-trip testing.
    Load {
        #[arg(long, default_value = "saves")]
        save_dir: PathBuf,
        #[arg(long, default_value_t = 0)]
        slot: u8,
    },
    /// Boot the engine into a scene and tick it for `frames` frames.
    /// Drives the field VM, camera, BGM director, and per-actor move VMs;
    /// logs scene transitions and the per-frame BGM events. No window —
    /// for that, use `asset-viewer field <scene>`.
    Play {
        /// Starting scene name. Default: `town01`.
        #[arg(long, default_value = "town01")]
        scene: String,
        /// Extracted-root directory.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Number of engine frames to run before exiting. `0` runs
        /// indefinitely.
        #[arg(long, default_value_t = 600)]
        frames: u64,
        /// Disable audio output. Useful for CI / headless smoke tests
        /// where cpal can't enumerate a device.
        #[arg(long, default_value_t = false)]
        no_audio: bool,
        /// Per-frame sleep in milliseconds. Default 16 ms ≈ 60 FPS for a
        /// realtime feel; set to `0` for "as fast as possible" smoke runs.
        #[arg(long, default_value_t = 16)]
        frame_ms: u64,
    },
    /// Open a window, boot a scene, and run the engine with rendering.
    /// Accepts keyboard input (arrows = D-pad, Z = Cross, Esc = quit).
    PlayWindow {
        /// Starting scene name. Default: `town01`.
        #[arg(long, default_value = "town01")]
        scene: String,
        /// Extracted-root directory.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Disable audio output.
        #[arg(long, default_value_t = false)]
        no_audio: bool,
        /// Enable world-map mode: installs a WorldMapController and shows
        /// the top-view camera globals in the HUD. Arrow keys scroll the
        /// top-view camera; Q/W adjust azimuth; A/S adjust zoom.
        #[arg(long, default_value_t = false)]
        world_map: bool,
    },
    /// Open a window and play back a raw PSX STR video file (2048-byte sectors,
    /// no CD subheaders) using the MDEC decoder.  Audio is not yet wired;
    /// video frames are rendered fullscreen at ~15 FPS (one frame per tick).
    ///
    /// Accepts raw STR data files written by `legaia-extract` or extracted
    /// directly from Mode 2 Form 1 CD sectors.  The Legaia-specific mapping
    /// from PROT entry to STR data is not yet traced; supply a raw file path.
    PlayStr {
        /// Path to a raw STR file (2048-byte sectors, no subheaders).
        #[arg()]
        str_file: PathBuf,
        /// Window width.
        #[arg(long, default_value_t = 640)]
        width: u32,
        /// Window height.
        #[arg(long, default_value_t = 480)]
        height: u32,
    },
    /// Show or update the keyboard-to-pad-button input mapping.
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCmd {
    /// Print the current input mapping to stdout.
    Show {
        /// Path to the TOML config file (default: `legaia-input.toml`).
        #[arg(long, default_value = "legaia-input.toml")]
        config_file: PathBuf,
    },
    /// Set a single key binding. KEY is the user-friendly key name (e.g.
    /// `Z`, `Up`, `Enter`, `RShift`); BUTTON is the PSX pad button name
    /// (e.g. `Cross`, `Circle`, `Start`, `L1`).
    Set {
        /// Binding in KEY=BUTTON form, e.g. `--binding Z=Cross`.
        #[arg(long)]
        binding: String,
        /// Path to the TOML config file (default: `legaia-input.toml`).
        #[arg(long, default_value = "legaia-input.toml")]
        config_file: PathBuf,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info {
            scene,
            extracted_root,
        } => cmd_info(&scene, &extracted_root),
        Cmd::ListScenes { extracted_root } => cmd_list_scenes(&extracted_root),
        Cmd::Play {
            scene,
            extracted_root,
            frames,
            no_audio,
            frame_ms,
        } => cmd_play(&scene, &extracted_root, frames, !no_audio, frame_ms),
        Cmd::PlayWindow {
            scene,
            extracted_root,
            no_audio,
            world_map,
        } => cmd_play_window(&scene, &extracted_root, !no_audio, world_map),
        Cmd::Save {
            extracted_root,
            save_dir,
            slot,
            party_size,
        } => cmd_save(&extracted_root, &save_dir, slot, party_size),
        Cmd::Load { save_dir, slot } => cmd_load(&save_dir, slot),
        Cmd::PlayStr {
            str_file,
            width,
            height,
        } => cmd_play_str(&str_file, width, height),
        Cmd::Config { cmd } => cmd_config(cmd),
    }
}

fn cmd_info(scene_name: &str, extracted_root: &std::path::Path) -> Result<()> {
    let prot = extracted_root.join("PROT.DAT");
    let cdname_path = extracted_root.join("CDNAME.TXT");
    if !prot.exists() {
        anyhow::bail!("missing {} (run `legaia-extract` first)", prot.display());
    }
    if !cdname_path.exists() {
        anyhow::bail!(
            "missing {} (run `legaia-extract` first)",
            cdname_path.display()
        );
    }

    let index = ProtIndex::open_extracted(extracted_root)
        .with_context(|| format!("open ProtIndex at {}", extracted_root.display()))?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;
    let assets = SceneAssets::build(&scene);
    let resources = SceneResources::build(&scene)?;

    println!("scene '{}'", scene.name);
    println!(
        "  CDNAME range:           PROT [{}..{})",
        scene.start, scene.end
    );
    println!("  entries swept:          {}", scene.entries.len());
    println!(
        "  TIMs uploaded to VRAM:  {} (parse failures: {})",
        resources.tim_count, resources.tim_parse_failures
    );
    println!("  TMDs parsed:            {}", resources.tmds.len());
    println!(
        "  MES container:          {}",
        if assets.mes.is_some() {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "  SEQ entries (raw):      {} (in stream wrappers: {})",
        assets.seq_entries.len(),
        assets.seq_in_stream_entries.len()
    );
    println!("  VAB entries:            {}", assets.vab_entries.len());
    println!("  Event-script records:   {}", assets.event_records.len());
    Ok(())
}

fn cmd_play(
    scene: &str,
    extracted_root: &std::path::Path,
    frames: u64,
    enable_audio: bool,
    frame_ms: u64,
) -> Result<()> {
    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio,
    };
    let mut session = BootSession::open(extracted_root, &cfg)?;
    println!(
        "play: scene='{}' frames={} audio={} (entries={}, MES={}, VAB={}, SEQ={})",
        scene,
        if frames == 0 {
            "∞".into()
        } else {
            frames.to_string()
        },
        if session.audio.is_some() { "on" } else { "off" },
        session
            .host
            .scene
            .as_ref()
            .map(|s| s.entries.len())
            .unwrap_or(0),
        if session
            .host
            .assets
            .as_ref()
            .map(|a| a.mes.is_some())
            .unwrap_or(false)
        {
            "yes"
        } else {
            "no"
        },
        session
            .host
            .assets
            .as_ref()
            .map(|a| a.vab_entries.len())
            .unwrap_or(0),
        session
            .host
            .assets
            .as_ref()
            .map(|a| a.seq_entries.len() + a.seq_in_stream_entries.len())
            .unwrap_or(0),
    );

    let mut transitions = 0u64;
    let mut bgm_events = 0u64;
    let mut last_log = 0u64;
    let mut tick_count = 0u64;
    while frames == 0 || tick_count < frames {
        let event = session.tick()?;
        match event {
            SceneTickEvent::SceneEntered { name } => {
                transitions += 1;
                println!("frame {}: entered scene '{}'", tick_count, name);
            }
            SceneTickEvent::UnknownMapId { map_id } => {
                println!(
                    "frame {}: scene_transition({}) had no mapped scene",
                    tick_count, map_id
                );
            }
            SceneTickEvent::Stepped => {}
        }
        if let Some(bgm) = session.bgm.as_ref()
            && bgm.last_started.is_some()
        {
            bgm_events = bgm_events.max(1);
        }
        if tick_count - last_log >= 60 {
            last_log = tick_count;
            log::info!(
                "frame {}: world.frame={}, transitions={}, bgm_started={}",
                tick_count,
                session.host.world.frame,
                transitions,
                bgm_events
            );
        }
        if frame_ms > 0 {
            std::thread::sleep(Duration::from_millis(frame_ms));
        }
        tick_count += 1;
    }
    println!(
        "exit: ticked {} frames, world.frame={}, transitions={}",
        tick_count, session.host.world.frame, transitions
    );
    Ok(())
}

fn cmd_save(
    extracted_root: &std::path::Path,
    save_dir: &std::path::Path,
    slot: u8,
    party_size: usize,
) -> Result<()> {
    use legaia_engine_core::menu_runtime::MenuRuntime;
    use legaia_engine_core::world::World;
    use legaia_save::{CharacterRecord, Party};

    let _ = extracted_root;
    let mut world = World::default();
    let members = (0..party_size).map(|_| CharacterRecord::zeroed()).collect();
    world.load_party(Party { members });
    world.story_flags = 0;
    world.money = 0;
    let runtime = MenuRuntime::new(save_dir.to_path_buf());
    let path = runtime.save_to_slot(&mut world, slot)?;
    let sf = world.save_full();
    println!(
        "saved slot {} to {} (party={}, story_flags={:#010X}, money={}, inventory={})",
        slot,
        path.display(),
        sf.party.members.len(),
        sf.ext.story_flags,
        sf.ext.money,
        sf.ext.inventory.len()
    );
    Ok(())
}

fn cmd_load(save_dir: &std::path::Path, slot: u8) -> Result<()> {
    use legaia_engine_core::menu_runtime::MenuRuntime;
    use legaia_engine_core::world::World;

    let runtime = MenuRuntime::new(save_dir.to_path_buf());
    let mut world = World::default();
    let path = runtime.load_from_slot(&mut world, slot)?;
    println!(
        "loaded slot {} from {} (party={}, story_flags={:#010X}, money={}, inventory={}, actors={})",
        slot,
        path.display(),
        world.roster.members.len(),
        world.story_flags,
        world.money,
        world.inventory.len(),
        world.actors.iter().filter(|a| a.active).count()
    );
    Ok(())
}

fn cmd_list_scenes(extracted_root: &std::path::Path) -> Result<()> {
    let cdname_path = extracted_root.join("CDNAME.TXT");
    if !cdname_path.exists() {
        anyhow::bail!(
            "missing {} (run `legaia-extract` first)",
            cdname_path.display()
        );
    }
    let map =
        cdname::parse(&cdname_path).with_context(|| format!("parse {}", cdname_path.display()))?;

    let mut names: Vec<String> = map.values().cloned().collect();
    names.sort();
    names.dedup();

    println!("{} distinct scene names:", names.len());
    for name in &names {
        if let Some((start, end)) = cdname::block_range_for_name(&map, name) {
            println!(
                "  {:<24} PROT [{}..{}) ({} entries)",
                name,
                start,
                end,
                end - start
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

fn cmd_config(cmd: ConfigCmd) -> Result<()> {
    use legaia_engine_core::input::Mapping;
    match cmd {
        ConfigCmd::Show { config_file } => {
            let mapping = Mapping::load_or_default(&config_file);
            let mut pairs: Vec<_> = mapping.bindings.iter().collect();
            pairs.sort_by_key(|(k, _)| k.as_str());
            println!("input mapping ({})", config_file.display());
            for (key, btn) in &pairs {
                println!("  {key:<12} → {btn}");
            }
        }
        ConfigCmd::Set {
            binding,
            config_file,
        } => {
            let Some((key, btn)) = binding.split_once('=') else {
                anyhow::bail!("--binding must be KEY=BUTTON (e.g. Z=Cross)");
            };
            let key = key.trim().to_string();
            let btn = btn.trim().to_string();
            // Validate that the button name is known.
            if legaia_engine_core::input::PadButton::from_name(&btn).is_none() {
                anyhow::bail!(
                    "unknown pad button '{}'; valid names: Select L3 R3 Start Up Right Down Left L2 R2 L1 R1 Triangle Circle Cross Square",
                    btn
                );
            }
            let mut mapping = Mapping::load_or_default(&config_file);
            mapping.bindings.insert(key.clone(), btn.clone());
            mapping.save(&config_file)?;
            println!("binding saved: {key} → {btn} ({})", config_file.display());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// play-window
// ---------------------------------------------------------------------------

/// Windowed engine runner state. Owned by the winit event loop.
struct PlayWindowApp {
    session: BootSession,
    font: Font,
    /// Pre-built scene resources (VRAM + TMDs). Consumed by `upload_assets`
    /// when the renderer is first attached; `None` after that.
    scene_res: Option<SceneResources>,
    win: EngineWindow,
    font_atlas: Option<UploadedFontAtlas>,
    uploaded_vram: Option<UploadedVram>,
    meshes: Vec<UploadedVramMesh>,
    /// Retained TMD data (struct + raw bytes) parallel to `meshes`, used to
    /// re-pose animated actor meshes each frame via `tmd_to_vram_mesh_posed`.
    scene_tmd_data: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
    scene_aabb: ([f32; 3], [f32; 3]),
    /// Current held-button bitmask (PSX pad encoding). Updated per key event.
    pad: u16,
    /// Input binding loaded from file (or default).
    mapping: legaia_engine_core::input::Mapping,
    /// World-map camera controller. `Some` when `--world-map` was passed;
    /// ticked each frame alongside the session.
    world_map_ctrl: Option<WorldMapController>,
    /// Pad state from the previous frame — used to compute newly-pressed bits
    /// for the world-map toggle combo.
    prev_pad: u16,
}

impl PlayWindowApp {
    fn upload_assets(&mut self) {
        let Some(res) = self.scene_res.take() else {
            return;
        };
        let (vram_opt, font_opt, meshes, tmd_data, lo, hi) = {
            let Some(r) = self.win.renderer.as_ref() else {
                self.scene_res = Some(res);
                return;
            };
            let vram = r
                .upload_vram(&res.vram)
                .map_err(|e| log::error!("VRAM upload: {e:#}"))
                .ok();
            let font = r
                .upload_font(&self.font)
                .map_err(|e| log::error!("font upload: {e:#}"))
                .ok();
            let mut meshes = Vec::new();
            let mut tmd_data: Vec<(legaia_tmd::Tmd, Vec<u8>)> = Vec::new();
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for rtmd in &res.tmds {
                let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&rtmd.tmd, &rtmd.raw);
                if vmesh.indices.is_empty() {
                    continue;
                }
                let (mlo, mhi) = vmesh.aabb();
                for ax in 0..3 {
                    if mlo[ax] < lo[ax] {
                        lo[ax] = mlo[ax];
                    }
                    if mhi[ax] > hi[ax] {
                        hi[ax] = mhi[ax];
                    }
                }
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                ) {
                    Ok(m) => {
                        tmd_data.push((rtmd.tmd.clone(), rtmd.raw.clone()));
                        meshes.push(m);
                    }
                    Err(e) => log::warn!("TMD upload skipped: {e:#}"),
                }
            }
            (vram, font, meshes, tmd_data, lo, hi)
        };
        if let Some(v) = vram_opt {
            self.uploaded_vram = Some(v);
        }
        if let Some(a) = font_opt {
            self.font_atlas = Some(a);
        }
        self.meshes = meshes;
        self.scene_tmd_data = tmd_data;
        if lo[0].is_finite() {
            self.scene_aabb = (lo, hi);
        }
        log::info!(
            "play-window: {} meshes uploaded, VRAM {}",
            self.meshes.len(),
            if self.uploaded_vram.is_some() {
                "ready"
            } else {
                "failed"
            }
        );
    }

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

    fn actor_model(&self, slot: usize) -> Mat4 {
        let a = &self.session.host.world.actors[slot];
        let pos = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        Mat4::from_translation(pos) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
    }

    fn build_hud(&self, _w: u32, _h: u32) -> Vec<TextDraw> {
        let Some(atlas) = &self.font_atlas else {
            return Vec::new();
        };
        let _ = atlas;
        let white = [1.0f32, 1.0, 1.0, 1.0];
        let dim = [0.7f32, 0.85, 1.0, 1.0];
        let scene_name = self
            .session
            .host
            .scene
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or("(none)");
        let line1 = format!(
            "scene {}  frame {}  meshes {}",
            scene_name,
            self.session.host.world.frame,
            self.meshes.len()
        );
        let layout1 = self.font.layout_ascii(&line1);
        let mut out = text_draws_for(&layout1, (8, 8), white);
        let audio_str = if self.session.audio.is_some() {
            "audio on"
        } else {
            "no audio"
        };
        let line2 = format!(
            "t {:.1}s  {}  arrows=dpad Z=X",
            self.win.elapsed_secs(),
            audio_str
        );
        let layout2 = self.font.layout_ascii(&line2);
        out.extend(text_draws_for(&layout2, (8, 26), dim));
        if let Some(ctrl) = &self.world_map_ctrl {
            let mode_str = if ctrl.is_top_view() {
                "top-view"
            } else {
                "walk"
            };
            let line3 = format!(
                "world-map {} | cam ({},{}) az {} zoom {}",
                mode_str, ctrl.camera_x, ctrl.camera_z, ctrl.azimuth, ctrl.zoom
            );
            let layout3 = self.font.layout_ascii(&line3);
            out.extend(text_draws_for(&layout3, (8, 44), white));
        }
        out
    }
}

impl ApplicationHandler for PlayWindowApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if !self.win.open(evl, "legaia-engine") {
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
                let key_name = keycode_to_name(code);
                if let Some(button) = self.mapping.pad_button_for_key(key_name) {
                    if state == ElementState::Pressed {
                        self.pad |= button.mask();
                    } else {
                        self.pad &= !button.mask();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let dt = self.win.advance_tick(100);
                // Drain up to 4 ticks per render frame so we never spiral
                // but can still catch up from minor vsync jitter.
                let ticks = self.win.drain_ticks(dt, 4);
                for _ in 0..ticks {
                    if let Err(e) = self.session.tick() {
                        log::error!("session tick: {e:#}");
                    }
                    if let Some(ctrl) = &mut self.world_map_ctrl {
                        let newly_pressed = self.pad & !self.prev_pad;
                        ctrl.tick(self.pad, newly_pressed);
                    }
                    self.prev_pad = self.pad;
                }
                if let (Some(r), Some(vram), Some(atlas)) = (
                    self.win.renderer.as_ref(),
                    self.uploaded_vram.as_ref(),
                    self.font_atlas.as_ref(),
                ) {
                    let (w, h) = r.surface_size();
                    let aspect = w as f32 / h.max(1) as f32;
                    let cam = self.camera_mvp(aspect);
                    // For each active actor with a tmd_binding and a current
                    // pose_frame, regenerate and re-upload the posed mesh.
                    // posed_overrides[i] replaces meshes[i] when present.
                    let mut posed_overrides: Vec<Option<UploadedVramMesh>> =
                        (0..self.scene_tmd_data.len()).map(|_| None).collect();
                    for actor in &self.session.host.world.actors {
                        if !actor.active {
                            continue;
                        }
                        let (Some(tmd_idx), Some(pose)) = (actor.tmd_binding, &actor.pose_frame)
                        else {
                            continue;
                        };
                        let Some((tmd, raw)) = self.scene_tmd_data.get(tmd_idx) else {
                            continue;
                        };
                        let vmesh =
                            legaia_tmd::mesh::tmd_to_vram_mesh_posed(tmd, raw, &pose.bone_outputs);
                        if vmesh.indices.is_empty() {
                            continue;
                        }
                        match r.upload_vram_mesh(
                            &vmesh.positions,
                            &vmesh.uvs,
                            &vmesh.cba_tsb,
                            &vmesh.normals,
                            &vmesh.indices,
                        ) {
                            Ok(m) => posed_overrides[tmd_idx] = Some(m),
                            Err(e) => log::warn!("posed mesh upload: {e:#}"),
                        }
                    }

                    let draws: Vec<SceneDraw<'_>> = self
                        .meshes
                        .iter()
                        .enumerate()
                        .map(|(i, static_m)| {
                            let mesh = posed_overrides
                                .get(i)
                                .and_then(|o| o.as_ref())
                                .unwrap_or(static_m);
                            SceneDraw {
                                mesh,
                                mvp: cam * self.actor_model(i),
                            }
                        })
                        .collect();
                    let hud = self.build_hud(w, h);
                    let overlay = TextOverlay { atlas, draws: &hud };
                    let scene = RenderScene {
                        vram,
                        draws: &draws,
                        overlay_lines: None,
                        overlay_sprites: None,
                        overlay_text: Some(&overlay),
                    };
                    if let Err(e) = r.render(RenderTarget::Scene(&scene)) {
                        log::error!("render: {e:#}");
                    }
                }
                self.win.request_redraw();
            }
            _ => {}
        }
    }
}

/// Map a winit `KeyCode` to the user-friendly key name used in
/// [`legaia_engine_core::input::Mapping`]. Returns `""` for keys outside
/// the default set.
fn keycode_to_name(code: KeyCode) -> &'static str {
    match code {
        KeyCode::ArrowUp => "Up",
        KeyCode::ArrowDown => "Down",
        KeyCode::ArrowLeft => "Left",
        KeyCode::ArrowRight => "Right",
        KeyCode::KeyZ => "Z",
        KeyCode::KeyX => "X",
        KeyCode::KeyA => "A",
        KeyCode::KeyS => "S",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyW => "W",
        KeyCode::Enter => "Enter",
        KeyCode::ShiftRight => "RShift",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        _ => "",
    }
}

fn cmd_play_window(
    scene: &str,
    extracted_root: &Path,
    enable_audio: bool,
    world_map: bool,
) -> Result<()> {
    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio,
    };
    let mut session = BootSession::open(extracted_root, &cfg)?;
    if world_map {
        session.host.world.mode = SceneMode::WorldMap;
    }

    let scene_res = {
        let s = session
            .host
            .scene
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no scene loaded after BootSession::open"))?;
        SceneResources::build(s)?
    };
    log::info!(
        "play-window: scene '{}', {} TMDs, {} TIMs",
        scene,
        scene_res.tmds.len(),
        scene_res.tim_count
    );

    let font = Font::load_or_placeholder(extracted_root);

    let mapping = legaia_engine_core::input::Mapping::load_or_default(&std::path::PathBuf::from(
        "legaia-input.toml",
    ));
    let world_map_ctrl = if world_map {
        Some(WorldMapController::new())
    } else {
        None
    };
    let mut app = PlayWindowApp {
        session,
        font,
        scene_res: Some(scene_res),
        win: EngineWindow::new(),
        font_atlas: None,
        uploaded_vram: None,
        meshes: Vec::new(),
        scene_tmd_data: Vec::new(),
        scene_aabb: ([f32::NEG_INFINITY; 3], [f32::INFINITY; 3]),
        pad: 0,
        mapping,
        world_map_ctrl,
        prev_pad: 0,
    };

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

// ── STR video player ────────────────────────────────────────────────────────

fn cmd_play_str(str_file: &Path, _win_width: u32, _win_height: u32) -> Result<()> {
    use legaia_mdec::{MdecDecoder, VideoFrame, str_sector::StrFrameAssembler};

    let data = std::fs::read(str_file).with_context(|| format!("read {}", str_file.display()))?;
    if data.len() % 2048 != 0 {
        log::warn!(
            "play-str: file size {} is not a multiple of 2048",
            data.len()
        );
    }
    let n_sectors = data.len() / 2048;

    // Pre-decode all frames into RGBA buffers.
    let mut asm = StrFrameAssembler::new();
    let mut frames: Vec<VideoFrame> = Vec::new();
    for i in 0..n_sectors {
        let sector = &data[i * 2048..(i + 1) * 2048];
        if let Some((hdr, bs)) = asm.push_sector(sector)? {
            let dec = MdecDecoder::new(hdr.width as u32, hdr.height as u32);
            match dec.decode_frame(&bs) {
                Ok(rgba) => frames.push(VideoFrame {
                    rgba,
                    width: hdr.width as u32,
                    height: hdr.height as u32,
                    frame_number: hdr.frame_number as u32,
                }),
                Err(e) => log::warn!("frame {}: decode error: {e}", hdr.frame_number),
            }
        }
    }
    if frames.is_empty() {
        anyhow::bail!("no video frames found in {}", str_file.display());
    }
    println!(
        "play-str: {} frames, {}×{}",
        frames.len(),
        frames[0].width,
        frames[0].height
    );

    let mut app = StrPlayerApp {
        win: EngineWindow::new(),
        frames,
        frame_idx: 0,
        uploaded: None,
    };
    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

struct StrPlayerApp {
    win: EngineWindow,
    frames: Vec<legaia_mdec::VideoFrame>,
    frame_idx: usize,
    uploaded: Option<legaia_engine_render::UploadedTexture>,
}

impl ApplicationHandler for StrPlayerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.win.open(event_loop, "legaia-engine play-str");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.win.handle_resize(size.width, size.height);
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.win.renderer() {
                    if self.frame_idx < self.frames.len() {
                        let f = &self.frames[self.frame_idx];
                        match renderer.upload_texture(&f.rgba, f.width, f.height) {
                            Ok(tex) => {
                                self.uploaded = Some(tex);
                            }
                            Err(e) => log::warn!("upload: {e}"),
                        }
                        self.frame_idx += 1;
                    }
                    if let Some(tex) = &self.uploaded {
                        let _ = renderer.render(RenderTarget::Texture(tex));
                    } else {
                        let _ = renderer.render(RenderTarget::Clear);
                    }
                }
                self.win.request_redraw();
            }
            _ => {}
        }
    }
}

//! `asset-viewer field <SCENE>` boots a CDNAME scene as a playable
//! field-mode demo: uploads the scene's TMDs as actors, wires input, drives
//! the field VM over the scene's event-script records, and overlays a HUD
//! with VM telemetry + a dialog panel.

use crate::common::{
    WorldActorMesh, collect_scene_tim_dirs, collect_scene_tmds, keymap_pad, mesh_aabb,
    pad_button_label,
};
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
use std::path::PathBuf;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowId;

pub(crate) fn run_field(
    scene_name: &str,
    max_actors: usize,
    extracted_root: &std::path::Path,
    initial_record: usize,
    cycle_records: bool,
) -> Result<()> {
    use legaia_engine_core::scene::{ProtIndex, Scene};
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
    let font = Font::load_from_extracted(extracted_root).with_context(|| {
        format!(
            "load extracted font under {} (run legaia-extract first?)",
            extracted_root.display()
        )
    })?;
    let index = ProtIndex::open_extracted(extracted_root)
        .with_context(|| format!("open ProtIndex at {}", extracted_root.display()))?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;
    let (start, end) = (scene.start, scene.end);
    log::info!(
        "scene '{}' covers PROT [{}..{}) ({} entries)",
        scene_name,
        start,
        end,
        end - start
    );
    let tmd_paths = collect_scene_tmds(extracted_root, start, end);
    let actor_count = tmd_paths.len().min(max_actors);
    let tim_dirs = collect_scene_tim_dirs(extracted_root, start, end);
    let tim_dir_refs: Vec<&std::path::Path> = tim_dirs.iter().map(|p| p.as_path()).collect();
    let (vram, tim_count) = legaia_tmd::vram_targeted::build_vram_from_dirs(&tim_dir_refs);
    log::info!(
        "field scene: {} actors over {} TIM(s) across {} tim_scan dir(s)",
        actor_count,
        tim_count,
        tim_dirs.len()
    );

    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    let radius = 800.0_f32;
    for i in 0..actor_count {
        let theta = (i as f32) * std::f32::consts::TAU / (actor_count.max(1) as f32);
        let x = (radius * theta.cos()) as i16;
        let z = (radius * theta.sin()) as i16;
        let actor = world.spawn_actor(i);
        actor.move_state.world_x = x;
        actor.move_state.world_y = 0;
        actor.move_state.world_z = z;
    }

    // Pre-extract event-script record bytes so the FieldApp can switch
    // records during playback without holding the Scene's borrow.
    let event_scripts_summary = scene.find_event_scripts().map(|es| EventScriptSet {
        entry_idx: es.entry_idx,
        records: (0..es.len())
            .map(|i| es.record(i).map(|s| s.to_vec()).unwrap_or_default())
            .collect(),
    });
    // Pull the MES container if the scene has one. Cloning the SceneMes
    // (which owns its bytes) decouples the FieldApp from `scene`'s
    // borrow lifetime so the panel can outlive the loader's local data.
    let scene_assets = legaia_engine_core::scene_assets::SceneAssets::build(&scene);
    let scene_mes = scene_assets.mes.clone();
    if scene_mes.is_some() {
        log::info!("field scene: scene MES container present - dialog opcodes will render");
    } else {
        log::info!("field scene: no MES container - dialog opcodes will resolve to a placeholder");
    }
    if let Some(es) = &event_scripts_summary {
        log::info!(
            "field scene: event-script entry PROT {} carries {} record(s)",
            es.entry_idx,
            es.records.len()
        );
        if let Some(first) = es.records.get(initial_record) {
            world.load_field_record(first);
            log::info!(
                "field VM: loaded record {} ({} bytes), pc={}",
                initial_record,
                first.len(),
                world.field_pc
            );
        } else {
            log::warn!(
                "field VM: record {} out of range (have {}); VM will idle",
                initial_record,
                es.records.len()
            );
        }
    } else {
        log::info!("field scene: no event-script entry; field VM idles");
    }

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = FieldApp {
        title: format!(
            "Field - scene '{}' [{} actors, tim_count={}]",
            scene_name, actor_count, tim_count
        ),
        scene_name: scene_name.to_string(),
        scene_range: (start, end),
        actor_count,
        win: EngineWindow::new(),
        font,
        font_atlas: None,
        vram_cpu: Some(vram),
        uploaded_vram: None,
        tmd_paths,
        meshes: Vec::new(),
        world,
        scene_aabb: ([-radius, -200.0, -radius], [radius, 600.0, radius]),
        input: InputState::new(),
        last_dt_ms: 16,
        event_scripts: event_scripts_summary,
        current_record: initial_record,
        cycle_records,
        vm_stats: FieldVmStats::default(),
        scene_mes,
        active_dialog: None,
        prev_input_pad: 0,
    };
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Pre-flattened event-script bundle held by the field viewer.
///
/// Holding the original `EventScripts<'_>` would require borrowing into
/// the `Scene` for the lifetime of the event loop - easier to copy each
/// record's bytes once at startup. Records are tens-to-hundreds of bytes
/// each so the duplication cost is trivial.
struct EventScriptSet {
    entry_idx: u32,
    records: Vec<Vec<u8>>,
}

/// Running tally of field-VM step outcomes plus the most recent activity.
///
/// Per-opcode histogram is dumped to stdout on session end so a single
/// playthrough surfaces which opcodes a scene's prescript actually uses -
/// useful for naturalistic discovery of remaining Pending sub-cases.
#[derive(Clone)]
struct FieldVmStats {
    advance: u64,
    yield_: u64,
    halt: u64,
    pending: u64,
    unknown: u64,
    last_pending_op: Option<u8>,
    last_unknown_op: Option<u8>,
    last_pc_before: usize,
    last_pc_after: usize,
    /// Opcode byte at `pc_before` for every recorded step. Indexed by
    /// opcode value. Pending / Unknown opcodes are tallied here too so
    /// the histogram surfaces every dispatch the VM saw.
    opcode_histogram: [u64; 256],
    /// Most recent opcode byte at `pc_before`. `None` before the first step.
    last_opcode: Option<u8>,
    /// Per-FieldHost callback count, aggregated from `World::drain_field_events`.
    /// Keys are short stringly tags so the HUD can dump a compact summary.
    field_event_counts: std::collections::BTreeMap<&'static str, u64>,
}

impl Default for FieldVmStats {
    fn default() -> Self {
        Self {
            advance: 0,
            yield_: 0,
            halt: 0,
            pending: 0,
            unknown: 0,
            last_pending_op: None,
            last_unknown_op: None,
            last_pc_before: 0,
            last_pc_after: 0,
            opcode_histogram: [0u64; 256],
            last_opcode: None,
            field_event_counts: std::collections::BTreeMap::new(),
        }
    }
}

/// Short kind tag for a [`legaia_engine_core::field_events::FieldEvent`].
///
/// Used as the histogram key in [`FieldVmStats::field_event_counts`].
fn field_event_tag(e: &legaia_engine_core::field_events::FieldEvent) -> &'static str {
    use legaia_engine_core::field_events::FieldEvent as E;
    match e {
        E::Bgm { .. } => "Bgm",
        E::GiveItem { .. } => "GiveItem",
        E::OpenDialog { .. } => "OpenDialog",
        E::DialogDismissed => "DialogDismissed",
        E::AddMoney { .. } => "AddMoney",
        E::SetItemCount { .. } => "SetItemCount",
        E::PartyAdd { .. } => "PartyAdd",
        E::PartyRemove { .. } => "PartyRemove",
        E::FieldInteract { .. } => "FieldInteract",
        E::SceneRegisterWrite { .. } => "SceneRegisterWrite",
        E::SetPartyLeader { .. } => "SetPartyLeader",
        E::CameraConfigure { .. } => "CameraConfigure",
        E::CameraLoad { .. } => "CameraLoad",
        E::CameraSave => "CameraSave",
        E::CameraApply => "CameraApply",
        E::SetupAnimation { .. } => "SetupAnimation",
        E::RenderCfgLong { .. } => "RenderCfgLong",
        E::RenderCfgShort { .. } => "RenderCfgShort",
        E::SpawnRecord { .. } => "SpawnRecord",
        E::EffectAnimTrigger { .. } => "EffectAnimTrigger",
        E::SceneFade { .. } => "SceneFade",
        E::ColorFade { .. } => "ColorFade",
        E::MenuCtrl { .. } => "MenuCtrl",
        E::MenuRefresh => "MenuRefresh",
        E::MoveTo { .. } => "MoveTo",
        E::ExecMove { .. } => "ExecMove",
        E::FmvTrigger { .. } => "FmvTrigger",
        E::ScriptedEncounter { .. } => "ScriptedEncounter",
        E::ActorAllocate { .. } => "ActorAllocate",
        E::ActorSpawned { .. } => "ActorSpawned",
        E::ActorSpawnFailed { .. } => "ActorSpawnFailed",
        E::WorldMapTransition { .. } => "WorldMapTransition",
    }
}

impl FieldVmStats {
    /// Top-10 opcodes by frequency, sorted descending. Each tuple is
    /// `(opcode, count)`. Returns up to 10 entries; opcodes with zero
    /// count are excluded.
    fn top_opcodes(&self) -> Vec<(u8, u64)> {
        let mut pairs: Vec<(u8, u64)> = self
            .opcode_histogram
            .iter()
            .enumerate()
            .filter_map(|(op, &c)| if c > 0 { Some((op as u8, c)) } else { None })
            .collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        pairs.truncate(10);
        pairs
    }

    /// Total number of recorded steps (sum across every status).
    fn total_steps(&self) -> u64 {
        self.advance + self.yield_ + self.halt + self.pending + self.unknown
    }
}

/// Field-mode viewer state. Owned by the winit event loop.
struct FieldApp {
    title: String,
    scene_name: String,
    scene_range: (u32, u32),
    actor_count: usize,
    win: EngineWindow,
    font: Font,
    font_atlas: Option<UploadedFontAtlas>,
    vram_cpu: Option<Vram>,
    uploaded_vram: Option<UploadedVram>,
    tmd_paths: Vec<PathBuf>,
    meshes: Vec<WorldActorMesh>,
    world: legaia_engine_core::world::World,
    /// Synthetic AABB enclosing every spawn point - drives the camera.
    scene_aabb: ([f32; 3], [f32; 3]),
    input: InputState,
    /// Last per-frame delta in ms, smoothed for the FPS HUD readout.
    last_dt_ms: u32,
    /// Pre-extracted event-script records (one per `EventScripts::record`).
    /// `None` when the scene carries no event-script entry.
    event_scripts: Option<EventScriptSet>,
    /// Active record index inside `event_scripts.records`.
    current_record: usize,
    /// When the active record halts or hits Unknown, advance to the next
    /// record and reload it into the field VM.
    cycle_records: bool,
    /// Running tally of step outcomes for HUD display.
    vm_stats: FieldVmStats,
    /// Snapshot of the scene's MES container, used to build a dialog
    /// panel when the field VM emits an `OpenDialog` request. `None` when
    /// the scene has no MES entry.
    scene_mes: Option<legaia_engine_core::scene_assets::SceneMes>,
    /// Live dialog panel - populated when the field VM hits opcode 0x3F
    /// and cleared when the user dismisses the box. Drives the
    /// per-frame dialog-window text under the HUD.
    active_dialog: Option<legaia_engine_core::dialog::OwnedDialogPanel>,
    /// Edge-trigger cache for Cross - used to advance one page per press
    /// instead of "advance every frame Cross is held."
    prev_input_pad: u16,
}

impl FieldApp {
    /// Upload TMDs as actor meshes plus the shared VRAM and font atlas.
    /// Must be called once a renderer is attached.
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

    /// Per-frame world tick that captures the field-VM `StepResult` for HUD
    /// display + auto-advances records when configured.
    ///
    /// Mirrors `World::tick`'s mode dispatch but breaks out the field-VM
    /// step so the viewer can observe the result. Effect-pool + per-actor
    /// move-VM ticks still run unconditionally so the rest of the world
    /// behaves identically to `World::tick`.
    fn tick_field_frame(&mut self) {
        use legaia_engine_vm::field::StepResult as FieldStepResult;

        self.world.frame += 1;
        self.world.tick_effects();
        self.world.tick_move_vms();

        let pc_before = self.world.field_pc;
        let last_opcode = self
            .world
            .field_bytecode
            .get(pc_before)
            .copied()
            .filter(|_| !self.world.field_bytecode.is_empty());
        let result = self.world.step_field();
        let pc_after = self.world.field_pc;

        self.vm_stats.last_pc_before = pc_before;
        self.vm_stats.last_pc_after = pc_after;
        if let Some(op) = last_opcode {
            self.vm_stats.opcode_histogram[op as usize] =
                self.vm_stats.opcode_histogram[op as usize].saturating_add(1);
            self.vm_stats.last_opcode = Some(op);
        }
        // Drain queued actor-spawn requests (field-VM op `0x4C 0x80`) into
        // real actor slots before the event-tag histogram pass so the
        // emitted `ActorSpawned` / `ActorSpawnFailed` events surface in the
        // HUD alongside the `ActorAllocate` event that produced them.
        self.world
            .materialize_actor_spawns(legaia_engine_core::world::FIELD_SPAWN_START_SLOT);
        // Aggregate every FieldHost callback the step emitted by tag so the
        // HUD / session-end summary surface which retail behaviours fired.
        for event in self.world.drain_field_events() {
            let tag = field_event_tag(&event);
            *self.vm_stats.field_event_counts.entry(tag).or_default() += 1;
        }

        let mut should_cycle = false;
        match result {
            Some(FieldStepResult::Advance { .. }) => self.vm_stats.advance += 1,
            Some(FieldStepResult::Yield { .. }) => self.vm_stats.yield_ += 1,
            Some(FieldStepResult::Halt { .. }) => {
                self.vm_stats.halt += 1;
                should_cycle = true;
            }
            Some(FieldStepResult::Pending { opcode, .. }) => {
                self.vm_stats.pending += 1;
                self.vm_stats.last_pending_op = Some(opcode);
            }
            Some(FieldStepResult::Unknown { opcode, .. }) => {
                self.vm_stats.unknown += 1;
                self.vm_stats.last_unknown_op = Some(opcode);
                should_cycle = true;
            }
            None => {}
        }

        if should_cycle && self.cycle_records {
            self.advance_to_next_record();
        }

        // After the field VM step, see if a dialog request was raised.
        // We never overwrite an active panel mid-conversation - the panel
        // owns the page until the user dismisses it.
        self.maybe_open_dialog();
        self.tick_active_dialog();
    }

    /// If `world.current_dialog` carries a pending request and no panel is
    /// active yet, build one from the scene's MES container.
    fn maybe_open_dialog(&mut self) {
        if self.active_dialog.is_some() {
            return;
        }
        let Some(req) = self.world.current_dialog.as_ref() else {
            return;
        };
        let Some(mes) = self.scene_mes.as_ref() else {
            // No MES → drop the request so we don't loop forever, log
            // once with an op-summary so the gap is visible in HUD.
            log::warn!(
                "field VM: OpenDialog text_id={:#x} but scene has no MES container",
                req.text_id
            );
            self.world.current_dialog = None;
            return;
        };
        if let Some(mut panel) =
            legaia_engine_core::dialog::OwnedDialogPanel::from_scene_mes(mes, req.text_id)
        {
            panel.set_glyphs_per_frame(2);
            self.active_dialog = Some(panel);
            log::debug!(
                "field VM: opened dialog text_id={:#x} (depth={})",
                req.text_id,
                req.depth_id
            );
        } else {
            log::warn!(
                "field VM: text_id {:#x} out of MES range; clearing request",
                req.text_id
            );
            self.world.current_dialog = None;
        }
    }

    /// Tick the active dialog panel one frame. When the panel hits Done,
    /// clear it and the world's request so the field VM can resume.
    fn tick_active_dialog(&mut self) {
        let Some(panel) = self.active_dialog.as_mut() else {
            return;
        };
        panel.tick();
        if panel.is_done() {
            self.active_dialog = None;
            self.world.current_dialog = None;
        }
    }

    /// Move to the next event-script record and reload it into the field VM.
    /// Wraps around to record 0 at the end so a single session keeps cycling.
    fn advance_to_next_record(&mut self) {
        let Some(es) = &self.event_scripts else {
            return;
        };
        if es.records.is_empty() {
            return;
        }
        let next = (self.current_record + 1) % es.records.len();
        self.current_record = next;
        if let Some(bytes) = es.records.get(next) {
            self.world.load_field_record(bytes);
            log::debug!(
                "field VM: cycled to record {} ({} bytes)",
                next,
                bytes.len()
            );
        }
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

    /// Dump field-VM telemetry to stdout when the viewer is closing.
    ///
    /// Includes the per-opcode histogram (top-10 by count) and the
    /// FieldHost callback tally. Idempotent for clean shutdown paths
    /// that may fire both Escape and CloseRequested.
    fn print_session_summary(&self) {
        if self.event_scripts.is_none() {
            return;
        }
        let total = self.vm_stats.total_steps();
        if total == 0 {
            return;
        }
        eprintln!(
            "\n[field-vm session] scene '{}'  frames={}  steps={}",
            self.scene_name, self.world.frame, total,
        );
        eprintln!(
            "  advance={}  yield={}  halt={}  pending={}  unknown={}",
            self.vm_stats.advance,
            self.vm_stats.yield_,
            self.vm_stats.halt,
            self.vm_stats.pending,
            self.vm_stats.unknown,
        );
        let top = self.vm_stats.top_opcodes();
        if !top.is_empty() {
            eprintln!("  top opcodes (op=count):");
            for (op, c) in top {
                eprintln!("    0x{op:02X}={c}");
            }
        }
        if !self.vm_stats.field_event_counts.is_empty() {
            eprintln!("  host callbacks fired:");
            for (tag, c) in &self.vm_stats.field_event_counts {
                eprintln!("    {tag}={c}");
            }
        }
    }

    fn actor_model(&self, slot: usize) -> Mat4 {
        let a = &self.world.actors[slot];
        let pos = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        let spin = self.win.elapsed_secs() * 0.6 + (slot as f32) * std::f32::consts::FRAC_PI_2;
        Mat4::from_translation(pos)
            * Mat4::from_rotation_y(spin)
            * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
    }

    /// Build the HUD overlay for this frame. White text on the upper-left
    /// shows scene name + frame info; the bottom strip shows the live pad
    /// state. Returns an empty list if the font atlas hasn't uploaded yet.
    fn build_hud(&self) -> Vec<TextDraw> {
        if self.font_atlas.is_none() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let white = [1.0, 1.0, 1.0, 1.0];
        let dim = [0.7, 0.85, 1.0, 1.0];

        let line1 = format!(
            "scene {}  prot[{}..{})  actors {}",
            self.scene_name, self.scene_range.0, self.scene_range.1, self.actor_count
        );
        let layout1 = self.font.layout_ascii(&line1);
        out.extend(text_draws_for(&layout1, (8, 8), white));

        let fps = 1000u32.checked_div(self.last_dt_ms).unwrap_or(0);
        let line2 = format!(
            "frame {}   {:>3} fps   t {:.1}s",
            self.world.frame,
            fps,
            self.win.elapsed_secs()
        );
        let layout2 = self.font.layout_ascii(&line2);
        out.extend(text_draws_for(&layout2, (8, 26), dim));

        // Field-VM diagnostic block - only meaningful when an event-script
        // entry was loaded. Shows record cursor, PC, and a histogram of
        // step outcomes so missing FieldHost hooks are visible at a glance.
        if let Some(es) = &self.event_scripts {
            let line3 = format!(
                "fieldVM rec {}/{}  pc {} -> {}  bc {}b",
                self.current_record + 1,
                es.records.len(),
                self.vm_stats.last_pc_before,
                self.vm_stats.last_pc_after,
                self.world.field_bytecode.len(),
            );
            let layout3 = self.font.layout_ascii(&line3);
            out.extend(text_draws_for(&layout3, (8, 44), dim));

            let line4 = format!(
                "adv {}  yld {}  halt {}  pending {}  unknown {}",
                self.vm_stats.advance,
                self.vm_stats.yield_,
                self.vm_stats.halt,
                self.vm_stats.pending,
                self.vm_stats.unknown,
            );
            let warn = if self.vm_stats.pending > 0 || self.vm_stats.unknown > 0 {
                [1.0, 0.6, 0.4, 1.0]
            } else {
                dim
            };
            let layout4 = self.font.layout_ascii(&line4);
            out.extend(text_draws_for(&layout4, (8, 62), warn));

            let last_pending = self
                .vm_stats
                .last_pending_op
                .map(|op| format!("0x{op:02X}"))
                .unwrap_or_else(|| "-".into());
            let last_unknown = self
                .vm_stats
                .last_unknown_op
                .map(|op| format!("0x{op:02X}"))
                .unwrap_or_else(|| "-".into());
            let line5 = format!(
                "last-pending {}  last-unknown {}  cycle {}",
                last_pending,
                last_unknown,
                if self.cycle_records { "on" } else { "off" }
            );
            let layout5 = self.font.layout_ascii(&line5);
            out.extend(text_draws_for(&layout5, (8, 80), dim));

            // Last opcode + top-5 opcode histogram so naturalistic
            // playthroughs surface which ops the prescript actually uses.
            let last_op_str = self
                .vm_stats
                .last_opcode
                .map(|op| format!("0x{op:02X}"))
                .unwrap_or_else(|| "-".into());
            let mut top = self.vm_stats.top_opcodes();
            top.truncate(5);
            let top_str = top
                .into_iter()
                .map(|(op, c)| format!("0x{op:02X}={c}"))
                .collect::<Vec<_>>()
                .join(" ");
            let line6 = format!("last-op {last_op_str}  top5 {top_str}");
            let layout6 = self.font.layout_ascii(&line6);
            out.extend(text_draws_for(&layout6, (8, 98), dim));

            if !self.vm_stats.field_event_counts.is_empty() {
                let events_str = self
                    .vm_stats
                    .field_event_counts
                    .iter()
                    .map(|(tag, c)| format!("{tag}={c}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let line7 = format!("hooks {events_str}");
                let layout7 = self.font.layout_ascii(&line7);
                out.extend(text_draws_for(&layout7, (8, 116), dim));
            }
        }

        // Pad state - show one cell per logical button.
        let buttons = [
            PadButton::Up,
            PadButton::Down,
            PadButton::Left,
            PadButton::Right,
            PadButton::Cross,
            PadButton::Circle,
            PadButton::Square,
            PadButton::Triangle,
            PadButton::Start,
            PadButton::Select,
            PadButton::L1,
            PadButton::R1,
            PadButton::L2,
            PadButton::R2,
        ];
        let mut pad_str = String::with_capacity(64);
        for b in buttons {
            let label = pad_button_label(b);
            if self.input.pressed(b) {
                pad_str.push_str(label);
            } else {
                for _ in 0..label.len() {
                    pad_str.push('-');
                }
            }
            pad_str.push(' ');
        }
        let layout3 = self.font.layout_ascii(&pad_str);
        let (sw, h) = self
            .win
            .renderer()
            .map(|r| r.surface_size())
            .unwrap_or((960, 720));
        out.extend(text_draws_for(
            &layout3,
            (8, (h as i32) - 24),
            [0.95, 0.95, 0.6, 1.0],
        ));

        // Dialog overlay - when a panel is active, lay the page glyphs out
        // through the same atlas the rest of the HUD uses, in a 2/3-width
        // box anchored near the lower third of the surface (matches the
        // retail dialog window's screen position).
        if let Some(panel) = self.active_dialog.as_ref() {
            let bytes = panel.page_bytes();
            let layout = self.font.layout_ascii(
                &bytes
                    .iter()
                    .map(|&b| {
                        if (0x20..=0x7E).contains(&b) {
                            b as char
                        } else {
                            '?'
                        }
                    })
                    .collect::<String>(),
            );
            // Pen position: 1/8 from left, ~70% down. Matches the
            // single-line dialog box layout the retail field VM emits.
            let pen_x = (sw as i32) / 8;
            let pen_y = ((h as i32) * 7) / 10;
            out.extend(text_draws_for(
                &layout,
                (pen_x, pen_y),
                [1.0, 1.0, 1.0, 1.0],
            ));
            // Tiny "press X to advance" hint when paused.
            if panel.is_waiting_for_input() {
                let layout = self.font.layout_ascii(">> X");
                out.extend(text_draws_for(
                    &layout,
                    (pen_x, pen_y + 22),
                    [0.7, 0.85, 1.0, 1.0],
                ));
            }
        }

        out
    }
}

impl ApplicationHandler for FieldApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if !self.win.open(evl, &self.title) {
            return;
        }
        self.upload_assets();
        self.win.request_redraw();
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.print_session_summary();
                evl.exit();
            }
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
                    self.print_session_summary();
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
                    // Defer the edge update until the next tick; we only
                    // mutate the held-mask snapshot here so multiple keys
                    // pressed within one frame all coalesce into the same
                    // tick.
                    self.input.set_pad(mask);
                }
            }
            WindowEvent::RedrawRequested => {
                let dt = self.win.advance_tick(1000);
                self.last_dt_ms = dt.as_millis().min(1000) as u32;
                let target_frames = EngineWindow::frames_for(dt, 8);
                for _ in 0..target_frames {
                    self.tick_field_frame();
                }
                // Cross edge → advance dialog page if the panel is paused.
                let pad_now = self.input.pad();
                let pressed_now = pad_now & !self.prev_input_pad;
                if pressed_now & PadButton::Cross.mask() != 0
                    && let Some(panel) = self.active_dialog.as_mut()
                    && panel.is_waiting_for_input()
                {
                    panel.advance_page();
                }
                self.prev_input_pad = pad_now;
                // Move the player slot 0 on D-pad input - gives the demo a
                // visible response to keyboard / gamepad input even with no
                // field bytecode loaded. Dialog blocks movement so the
                // pacing matches the retail engine's "field paused while
                // dialog is open" behavior.
                let dialog_open = self.active_dialog.is_some();
                let speed = 6.0_f32;
                if !dialog_open && self.actor_count > 0 {
                    let actor = &mut self.world.actors[0];
                    if self.input.pressed(PadButton::Right) {
                        actor.move_state.world_x = (actor.move_state.world_x as f32 + speed) as i16;
                    }
                    if self.input.pressed(PadButton::Left) {
                        actor.move_state.world_x = (actor.move_state.world_x as f32 - speed) as i16;
                    }
                    if self.input.pressed(PadButton::Up) {
                        actor.move_state.world_z = (actor.move_state.world_z as f32 - speed) as i16;
                    }
                    if self.input.pressed(PadButton::Down) {
                        actor.move_state.world_z = (actor.move_state.world_z as f32 + speed) as i16;
                    }
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

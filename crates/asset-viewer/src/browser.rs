//! The `App` shell for the single-file / PROT-browser / TMD-browser /
//! stage-browser modes: window + renderer + audio host driving a
//! navigable sequence of [`Display`]s.

use crate::common::{mesh_aabb, short_path};
use crate::display::{Display, MeshView, display_for_prot_entry};
use crate::stage_view::load_stage_for_view;
use crate::tmd_view::{TmdViewData, load_tmd_for_view};
use legaia_engine_audio::AudioOut;
use legaia_engine_render::{
    RenderTarget, Renderer, UploadedLines, UploadedMesh, UploadedTexture, UploadedVram,
    UploadedVramMesh,
};
use legaia_prot::{archive::Archive, cdname};
use std::path::PathBuf;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

pub(crate) struct App {
    pub(crate) window: Option<Arc<Window>>,
    pub(crate) renderer: Option<Renderer>,
    pub(crate) audio: Option<AudioOut>,
    pub(crate) texture: Option<UploadedTexture>,
    pub(crate) mesh: Option<UploadedMesh>,
    /// VRAM-mesh upload (TMD + multi-TIM VRAM). Coexists with `mesh_view`.
    pub(crate) vram_mesh: Option<UploadedVramMesh>,
    /// PSX VRAM bound to the current `vram_mesh`.
    pub(crate) vram: Option<UploadedVram>,
    /// Wireframe upload for the stage-geometry viewer.
    pub(crate) lines: Option<UploadedLines>,
    pub(crate) mesh_view: Option<MeshView>,
    /// Initial display (set by main before the event loop starts).
    pub(crate) pending: Option<Display>,
    /// PROT browse state. None for single-file modes.
    pub(crate) browser: Option<Browser>,
    /// TMD directory-walk state. None for single-file `tmd` mode.
    pub(crate) mesh_browser: Option<MeshBrowser>,
    /// Stage-geometry directory-walk state. None for single-file `stage` mode.
    pub(crate) stage_browser: Option<StageBrowser>,
}

/// Directory walk for the `stage <DIR>` mode. Holds the list of files that
/// scanned positively as stage-geometry (so navigation skips entries with
/// no table) plus a CDNAME map for nicer titles.
pub(crate) struct StageBrowser {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) current: usize,
    pub(crate) root_label: String,
}

impl StageBrowser {
    fn count(&self) -> usize {
        self.paths.len()
    }
}

/// Directory walk for the `tmd <DIR>` mode. Holds the resolved list of
/// `.tmd` files so navigation is just an index step.
pub(crate) struct MeshBrowser {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) current: usize,
    /// User-supplied root, kept for window-title context.
    pub(crate) root_label: String,
    /// Extra TIM dirs to overlay before each mesh's sibling tim_scan dir.
    /// Constant for the lifetime of the browser; set from `--vram-extra-dir`.
    pub(crate) vram_extras: Vec<PathBuf>,
    /// `--no-textures` / `--flat-shaded`: when true the VRAM path is
    /// suppressed and meshes always render as unlit flat geometry.
    pub(crate) no_textures: bool,
}

impl MeshBrowser {
    fn count(&self) -> usize {
        self.paths.len()
    }
}

pub(crate) struct Browser {
    pub(crate) archive: Archive,
    pub(crate) cdname: Option<cdname::IndexMap>,
    pub(crate) current: u32,
    pub(crate) last_count: u32,
}

impl Browser {
    fn name_for(&self, idx: u32) -> String {
        match self.cdname.as_ref().and_then(|m| cdname::block_for(m, idx)) {
            Some(name) => format!("{:04}_{}", idx, name),
            None => format!("{:04}", idx),
        }
    }

    fn entry_count(&self) -> u32 {
        self.archive.entries.len() as u32
    }
}

impl App {
    fn apply(&mut self, display: Display) {
        if let Some(w) = &self.window {
            w.set_title(&display.title);
        }
        if let Some(r) = &self.renderer {
            self.texture = match display.image {
                Some((rgba, w, h)) => match r.upload_texture(&rgba, w, h) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        log::error!("upload texture failed: {e:#}");
                        None
                    }
                },
                None => None,
            };
            (self.mesh, self.mesh_view) = match display.mesh {
                Some((positions, indices)) => {
                    let aabb = mesh_aabb(&positions);
                    match r.upload_mesh(&positions, &indices) {
                        Ok(m) => (Some(m), Some(MeshView::from_aabb(aabb.0, aabb.1))),
                        Err(e) => {
                            log::error!("upload mesh failed: {e:#}");
                            (None, None)
                        }
                    }
                }
                None => (None, None),
            };
            // Replace any prior VRAM-mesh/VRAM pair with the new one (or
            // clear, if the next display has no VRAM mesh).
            (self.vram_mesh, self.vram) = match display.vram_mesh {
                Some(payload) => {
                    let aabb = mesh_aabb(&payload.positions);
                    let mesh_res = r.upload_vram_mesh(
                        &payload.positions,
                        &payload.uvs,
                        &payload.cba_tsb,
                        &payload.normals,
                        &payload.indices,
                    );
                    let vram_res = r.upload_vram(&payload.vram);
                    match (mesh_res, vram_res) {
                        (Ok(m), Ok(v)) => {
                            // Frame the camera on the VRAM mesh's AABB.
                            self.mesh_view = Some(MeshView::from_aabb(aabb.0, aabb.1));
                            (Some(m), Some(v))
                        }
                        (mesh_err, vram_err) => {
                            if let Err(e) = mesh_err {
                                log::error!("upload vram mesh failed: {e:#}");
                            }
                            if let Err(e) = vram_err {
                                log::error!("upload vram failed: {e:#}");
                            }
                            (None, None)
                        }
                    }
                }
                None => (None, None),
            };
            // Wireframe / stage-geometry upload.
            self.lines = match display.lines {
                Some(payload) => {
                    let aabb = mesh_aabb(&payload.positions);
                    match r.upload_lines(&payload.positions, &payload.colors, &payload.indices) {
                        Ok(l) => {
                            self.mesh_view = Some(MeshView::from_aabb(aabb.0, aabb.1));
                            Some(l)
                        }
                        Err(e) => {
                            log::error!("upload lines failed: {e:#}");
                            None
                        }
                    }
                }
                None => None,
            };
        }
        if let (Some(a), Some((pcm, rate))) = (&self.audio, display.audio) {
            a.play_pcm_mono(pcm, rate);
        } else if let Some(a) = &self.audio {
            a.stop();
        }
    }

    fn show_browser_current(&mut self) {
        let Some(b) = self.browser.as_mut() else {
            return;
        };
        let count = b.entry_count();
        let cursor = b.current.min(count.saturating_sub(1));
        let entry = b.archive.entries[cursor as usize].clone();
        let name = b.name_for(cursor);
        let mut buf = Vec::new();
        let display = match b.archive.read_entry(&entry, &mut buf) {
            Ok(()) => display_for_prot_entry(&name, &buf)
                .unwrap_or_else(|| Display::empty(format!("{} (read failed)", name))),
            Err(e) => Display::empty(format!("{} (io error: {e})", name)),
        };
        b.current = cursor;
        b.last_count = count;
        let help = " - [N]ext [P]rev [PgDn]+10 [PgUp]-10 [Esc] quit";
        let mut display = display;
        display.title.push_str(help);
        self.apply(display);
    }

    fn step(&mut self, delta: i32) {
        if self.browser.is_some() {
            self.step_prot(delta);
        } else if self.mesh_browser.is_some() {
            self.step_mesh(delta);
        } else if self.stage_browser.is_some() {
            self.step_stage(delta);
        }
    }

    fn step_stage(&mut self, delta: i32) {
        let Some(sb) = self.stage_browser.as_mut() else {
            return;
        };
        let count = sb.count() as i32;
        if count == 0 {
            return;
        }
        let next = (sb.current as i32 + delta).rem_euclid(count);
        sb.current = next as usize;
        self.show_stage_current();
    }

    fn show_stage_current(&mut self) {
        let (path, label, idx, total) = {
            let Some(sb) = self.stage_browser.as_ref() else {
                return;
            };
            (
                sb.paths[sb.current].clone(),
                sb.root_label.clone(),
                sb.current + 1,
                sb.paths.len(),
            )
        };
        let display = match load_stage_for_view(&path) {
            Ok(payload) => {
                let title = format!(
                    "STAGE [{}/{}] {}  ({} verts, {} lines)  - {}  [N]ext [P]rev [PgDn]+10 [PgUp]-10 [Esc] quit",
                    idx,
                    total,
                    short_path(&path),
                    payload.positions.len(),
                    payload.indices.len() / 2,
                    label,
                );
                Display {
                    title,
                    image: None,
                    audio: None,
                    mesh: None,
                    vram_mesh: None,
                    lines: Some(payload),
                }
            }
            Err(e) => Display::empty(format!(
                "STAGE [{}/{}] {} (load failed: {e})",
                idx,
                total,
                short_path(&path),
            )),
        };
        self.apply(display);
    }

    fn step_prot(&mut self, delta: i32) {
        let Some(b) = self.browser.as_mut() else {
            return;
        };
        let count = b.entry_count() as i32;
        if count == 0 {
            return;
        }
        let next = (b.current as i32 + delta).rem_euclid(count);
        b.current = next as u32;
        self.show_browser_current();
    }

    fn step_mesh(&mut self, delta: i32) {
        let Some(mb) = self.mesh_browser.as_mut() else {
            return;
        };
        let count = mb.count() as i32;
        if count == 0 {
            return;
        }
        let next = (mb.current as i32 + delta).rem_euclid(count);
        mb.current = next as usize;
        self.show_mesh_current();
    }

    fn show_mesh_current(&mut self) {
        let (path, label, idx, total, extras, no_textures) = {
            let Some(mb) = self.mesh_browser.as_ref() else {
                return;
            };
            let path = mb.paths[mb.current].clone();
            (
                path,
                mb.root_label.clone(),
                mb.current + 1,
                mb.paths.len(),
                mb.vram_extras.clone(),
                mb.no_textures,
            )
        };
        let display = match load_tmd_for_view(&path, &extras, no_textures) {
            Ok(TmdViewData::Flat { positions, indices }) => {
                let title = format!(
                    "TMD [{}/{}] {}  ({} verts, {} tris) untextured  - {}  [N]ext [P]rev [PgDn]+10 [PgUp]-10 [Esc] quit",
                    idx,
                    total,
                    short_path(&path),
                    positions.len(),
                    indices.len() / 3,
                    label,
                );
                Display {
                    title,
                    image: None,
                    audio: None,
                    mesh: Some((positions, indices)),
                    vram_mesh: None,
                    lines: None,
                }
            }
            Ok(TmdViewData::Vram(payload)) => {
                let title = format!(
                    "TMD [{}/{}] {}  ({} tri-verts) vram={} TIMs from {}  - {}  [N]ext [P]rev [PgDn]+10 [PgUp]-10 [Esc] quit",
                    idx,
                    total,
                    short_path(&path),
                    payload.indices.len() / 3,
                    payload.tim_count,
                    payload.tim_dir_label,
                    label,
                );
                Display {
                    title,
                    image: None,
                    audio: None,
                    mesh: None,
                    vram_mesh: Some(payload),
                    lines: None,
                }
            }
            Err(e) => Display::empty(format!(
                "TMD [{}/{}] {} (load failed: {e})",
                idx,
                total,
                short_path(&path),
            )),
        };
        self.apply(display);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("legaia asset viewer")
            .with_inner_size(winit::dpi::LogicalSize::new(1024, 768));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let size = window.inner_size();
        let renderer =
            Renderer::new(window.clone(), size.width, size.height).expect("create renderer");
        self.window = Some(window);
        self.renderer = Some(renderer);
        // Audio is best-effort: if no device is available the viewer still
        // renders images.
        match AudioOut::new() {
            Ok(a) => self.audio = Some(a),
            Err(e) => log::warn!("audio init failed (continuing without): {e:#}"),
        }
        if let Some(d) = self.pending.take() {
            self.apply(d);
        }
        if self.browser.is_some() {
            self.show_browser_current();
        }
        if self.mesh_browser.is_some() {
            self.show_mesh_current();
        }
        if self.stage_browser.is_some() {
            self.show_stage_current();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(code),
                        ..
                    },
                ..
            } => match code {
                KeyCode::Escape => event_loop.exit(),
                KeyCode::ArrowRight | KeyCode::KeyN | KeyCode::Space => self.step(1),
                KeyCode::ArrowLeft | KeyCode::KeyP => self.step(-1),
                KeyCode::PageDown => self.step(10),
                KeyCode::PageUp => self.step(-10),
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                if let Some(r) = &self.renderer {
                    // Priority: VRAM mesh > flat mesh > wireframe > 2D texture > clear.
                    let target = if let (Some(vm), Some(vram), Some(view)) = (
                        self.vram_mesh.as_ref(),
                        self.vram.as_ref(),
                        self.mesh_view.as_ref(),
                    ) {
                        let (w, h) = r.surface_size();
                        let aspect = w as f32 / h.max(1) as f32;
                        RenderTarget::VramMesh {
                            mesh: vm,
                            vram,
                            mvp: view.mvp(aspect),
                        }
                    } else if let (Some(mesh), Some(view)) =
                        (self.mesh.as_ref(), self.mesh_view.as_ref())
                    {
                        let (w, h) = r.surface_size();
                        let aspect = w as f32 / h.max(1) as f32;
                        RenderTarget::Mesh {
                            mesh,
                            mvp: view.mvp(aspect),
                        }
                    } else if let (Some(lines), Some(view)) =
                        (self.lines.as_ref(), self.mesh_view.as_ref())
                    {
                        let (w, h) = r.surface_size();
                        let aspect = w as f32 / h.max(1) as f32;
                        RenderTarget::Lines {
                            mesh: lines,
                            mvp: view.mvp(aspect),
                        }
                    } else if let Some(t) = self.texture.as_ref() {
                        RenderTarget::Texture(t)
                    } else {
                        RenderTarget::Clear
                    };
                    if let Err(e) = r.render(target) {
                        log::error!("render error: {e:#}");
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

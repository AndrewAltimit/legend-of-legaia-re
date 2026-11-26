//! The `App` shell for the single-file / PROT-browser / TMD-browser /
//! stage-browser modes: window + renderer + audio host driving a
//! navigable sequence of [`Display`]s.

use crate::display::{Display, MeshView, display_for_prot_entry};
use crate::goto::{BROWSE_KEYS, GotoInput, goto_target};
use crate::mesh_bounds::mesh_aabb;
use crate::scene_files::short_path;
use crate::stage_view::load_stage_for_view;
use crate::tmd_view::{TmdViewData, load_tmd_for_view};
use legaia_engine_audio::AudioOut;
use legaia_engine_render::{
    RenderTarget, Renderer, TextOverlay, UploadedFontAtlas, UploadedLines, UploadedMesh,
    UploadedTexture, UploadedVram, UploadedVramMesh,
};
use legaia_engine_ui::{TextDraw, text_draws_for};
use legaia_font::Font;
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
    /// Why the window could not start (no window, no usable GPU adapter):
    /// `main` reports it once the event loop has exited.
    pub(crate) fatal: Option<anyhow::Error>,
    /// A number being typed to jump to (`goto`).
    pub(crate) goto: GotoInput,
    /// The current display's title, without the go-to prompt.
    pub(crate) base_title: String,
    /// The dialog font from an extracted tree near the paths the viewer was
    /// given ([`find_font`]): the key help is drawn with it. Without one the
    /// keys stay in the window title, where a long entry name cut them off.
    pub(crate) font: Option<Font>,
    pub(crate) font_atlas: Option<UploadedFontAtlas>,
    /// The key help over the window: shown at start, closed by any key,
    /// reopened by `H` / `F1`.
    pub(crate) help: bool,
    /// The current display plays a sound and draws nothing (a VAB sample).
    pub(crate) sound_only: bool,
}

/// The dialog font of the first extracted tree found at or above `candidates`
/// (each an argument path, or `extracted` under the working directory).
pub(crate) fn find_font(candidates: &[PathBuf]) -> Option<Font> {
    candidates
        .iter()
        .flat_map(|c| c.ancestors())
        .find(|dir| dir.join("font").is_dir())
        .and_then(|dir| Font::load_from_extracted(dir).ok())
}

/// The help's lines: the entry's full name and the keys that work now.
fn help_lines(title: &str, browsing: bool) -> Vec<(String, bool)> {
    let mut out = vec![(title.to_string(), true), (String::new(), false)];
    if browsing {
        for l in [
            "N, P, Left / Right arrow, Space: next and previous",
            "PgDn / PgUp: ten at a time    Home / End: first and last",
            "Type a number and Enter: jump there (Backspace, Esc cancel)",
        ] {
            out.push((l.to_string(), false));
        }
    }
    out.push(("H or F1: this help    Esc: quit".to_string(), false));
    out.push((String::new(), false));
    out.push(("Any key closes this help.".to_string(), false));
    out
}

/// The lines over an entry that has nothing to draw: its name, what the
/// window is doing instead (a sound playing, or nothing this viewer can show),
/// the keys onward, and the go-to prompt while a number is typed. Without
/// them such an entry is an empty window, and the name and the typed number
/// live only in a title bar that fullscreen, a tiling desktop or a small
/// window hides.
fn blank_note_lines(
    title: &str,
    sound_only: bool,
    browsing: bool,
    prompt: Option<String>,
) -> Vec<(String, bool)> {
    let mut out = vec![(title.to_string(), true), (String::new(), false)];
    out.push((
        if sound_only {
            "Sound only: playing the bank's first sample."
        } else {
            "Nothing in this entry this viewer can draw or play."
        }
        .to_string(),
        false,
    ));
    out.push((
        if browsing {
            "N / P: next and previous    H: keys"
        } else {
            "H: keys    Esc: quit"
        }
        .to_string(),
        false,
    ));
    if let Some(p) = prompt {
        out.push((String::new(), false));
        out.push((p, true));
    }
    out
}

/// Break `text` into lines of at most `max` characters, at spaces where it can.
fn wrap(text: &str, max: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ') {
        if !cur.is_empty() && cur.len() + 1 + word.len() > max {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
        while cur.len() > max {
            let rest = cur.split_off(max);
            lines.push(std::mem::replace(&mut cur, rest));
        }
    }
    lines.push(cur);
    lines
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
    /// Per-stage battle-backdrop mirror table from `SCUS_942.54`, when the
    /// executable was found. `None` falls the preview back to the default
    /// half-turn second copy.
    pub(crate) backdrop_mirror: Option<legaia_scene_formats::battle_backdrop::MirrorXTable>,
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
    /// The keys named in a browse title: the help's key while the font can
    /// draw it, else the whole list.
    fn keys_hint(&self) -> &'static str {
        if self.font.is_some() {
            "H: keys"
        } else {
            BROWSE_KEYS
        }
    }

    /// The title without its own pointer to the help (the overlays name
    /// the keys themselves).
    fn bare_title(&self) -> &str {
        self.base_title
            .strip_suffix(self.keys_hint())
            .map(|t| t.trim_end_matches([' ', '-']))
            .unwrap_or(&self.base_title)
    }

    /// The help overlay's draws.
    fn help_draws(&self, surface: (u32, u32)) -> Vec<TextDraw> {
        let lines = help_lines(self.bare_title(), self.browse_len().is_some());
        self.text_draws(&lines, surface)
    }

    /// The note drawn over an entry with nothing to draw
    /// ([`blank_note_lines`]).
    fn blank_note_draws(&self, surface: (u32, u32)) -> Vec<TextDraw> {
        let what = if self.browser.is_some() {
            "PROT entry"
        } else {
            "item"
        };
        let lines = blank_note_lines(
            self.bare_title(),
            self.sound_only,
            self.browse_len().is_some(),
            self.goto.prompt(what),
        );
        self.text_draws(&lines, surface)
    }

    /// `lines` (text, strong) in the dialog font, wrapped to the window and
    /// scaled to stay legible on a large one.
    fn text_draws(&self, lines: &[(String, bool)], surface: (u32, u32)) -> Vec<TextDraw> {
        let Some(font) = &self.font else {
            return Vec::new();
        };
        let scale = (surface.1 / 360).max(1) as i32;
        let max_chars = ((surface.0 as i32 / scale - 32) / 7).max(20) as usize;
        let mut out = Vec::new();
        let mut y = 16;
        for (line, strong) in lines {
            let color = if *strong {
                [1.0, 0.92, 0.55, 1.0]
            } else {
                [0.88, 0.9, 0.95, 1.0]
            };
            for part in wrap(line, max_chars) {
                for mut d in text_draws_for(&font.layout_ascii(&part), (16, y), color) {
                    d.dst = (
                        d.dst.0 * scale,
                        d.dst.1 * scale,
                        d.dst.2 * scale as u32,
                        d.dst.3 * scale as u32,
                    );
                    out.push(d);
                }
                y += 16;
            }
        }
        out
    }

    /// The window title: the display's own, led by the go-to prompt while
    /// a number is typed.
    fn refresh_title(&self) {
        let what = if self.browser.is_some() {
            "PROT entry"
        } else {
            "item"
        };
        let title = match self.goto.prompt(what) {
            Some(p) => format!("{p}  |  {}", self.base_title),
            None => self.base_title.clone(),
        };
        if let Some(w) = &self.window {
            w.set_title(&title);
        }
    }

    fn apply(&mut self, display: Display) {
        self.base_title = display.title.clone();
        self.sound_only = display.audio.is_some();
        self.refresh_title();
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
                        &payload.colors,
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
        let second_copy = b
            .backdrop_mirror
            .as_ref()
            .map(|t| t.second_copy_for_prot_index(cursor));
        let mut buf = Vec::new();
        let display = match b.archive.read_entry(&entry, &mut buf) {
            Ok(()) => display_for_prot_entry(&name, &buf, second_copy)
                .unwrap_or_else(|| Display::empty(format!("{} (nothing to show)", name))),
            Err(e) => Display::empty(format!("{} (could not read the entry: {e:#})", name)),
        };
        b.current = cursor;
        b.last_count = count;
        let mut display = display;
        display.title = format!(
            "{}  (of 0..{})  -  {}",
            display.title,
            count.saturating_sub(1),
            self.keys_hint()
        );
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

    /// The number of items the active browse mode walks, and whether its
    /// titles number them from 1 (the directory walks) or by PROT index.
    fn browse_len(&self) -> Option<(usize, bool)> {
        if let Some(b) = &self.browser {
            Some((b.entry_count() as usize, false))
        } else if let Some(mb) = &self.mesh_browser {
            Some((mb.count(), true))
        } else {
            self.stage_browser.as_ref().map(|sb| (sb.count(), true))
        }
    }

    /// Show item `pos` (a position in the active browse mode's list).
    fn show_at(&mut self, pos: usize) {
        if let Some(b) = self.browser.as_mut() {
            b.current = pos as u32;
            self.show_browser_current();
        } else if let Some(mb) = self.mesh_browser.as_mut() {
            mb.current = pos;
            self.show_mesh_current();
        } else if let Some(sb) = self.stage_browser.as_mut() {
            sb.current = pos;
            self.show_stage_current();
        }
    }

    /// Enter: jump to the typed number, or say it is out of range.
    fn goto_typed(&mut self) {
        let Some(typed) = self.goto.take() else {
            return;
        };
        let Some((count, one_based)) = self.browse_len() else {
            return;
        };
        match goto_target(typed, count, one_based) {
            Some(pos) => self.show_at(pos),
            None => {
                let range = if one_based {
                    format!("1..={count}")
                } else {
                    format!("0..={}", count.saturating_sub(1))
                };
                self.base_title = format!(
                    "No {typed} here (the range is {range})  |  {}",
                    self.base_title
                );
                self.refresh_title();
            }
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
                    "STAGE [{}/{}] {}  ({} verts, {} lines)  - {}  {}",
                    idx,
                    total,
                    short_path(&path),
                    payload.positions.len(),
                    payload.indices.len() / 2,
                    label,
                    self.keys_hint(),
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
                "STAGE [{}/{}] {} (load failed: {e:#})",
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
                    "TMD [{}/{}] {}  ({} verts, {} tris) untextured  - {}  {}",
                    idx,
                    total,
                    short_path(&path),
                    positions.len(),
                    indices.len() / 3,
                    label,
                    self.keys_hint(),
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
                    "TMD [{}/{}] {}  ({} tri-verts) vram={} TIMs from {}{}  - {}  {}",
                    idx,
                    total,
                    short_path(&path),
                    payload.indices.len() / 3,
                    payload.tim_count,
                    payload.tim_dir_label,
                    payload.dark_note(),
                    label,
                    self.keys_hint(),
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
                "TMD [{}/{}] {} (load failed: {e:#})",
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
        // A failure here (no display, no GPU adapter the renderer can use)
        // ends the run with a sentence instead of a panic backtrace.
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.fatal = Some(anyhow::anyhow!("could not open a window: {e}"));
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        let renderer = match Renderer::new(window.clone(), size.width, size.height) {
            Ok(r) => r,
            Err(e) => {
                self.fatal = Some(anyhow::anyhow!(
                    "could not start the renderer ({e:#}); the viewer needs a graphics \
                     driver with Vulkan, Metal, DirectX 12 or OpenGL support"
                ));
                event_loop.exit();
                return;
            }
        };
        if let Some(font) = &self.font {
            match renderer.upload_font(font) {
                Ok(a) => self.font_atlas = Some(a),
                Err(e) => log::warn!("font atlas upload failed (help stays in the title): {e:#}"),
            }
        }
        self.window = Some(window);
        self.renderer = Some(renderer);
        // Audio is best-effort: if no device is available the viewer still
        // renders images.
        match AudioOut::new() {
            Ok(a) => {
                use legaia_engine_audio::AudioSink;
                a.set_master_volume(legaia_engine_audio::DESKTOP_MASTER_VOLUME_DEFAULT);
                self.audio = Some(a);
            }
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
                KeyCode::KeyH | KeyCode::F1 if self.font.is_some() => self.help = !self.help,
                // While the help is up, any key closes it and does nothing else.
                _ if self.help && self.font_atlas.is_some() => self.help = false,
                // Esc first cancels a number being typed, then quits.
                KeyCode::Escape => {
                    if self.goto.cancel() {
                        self.refresh_title();
                    } else {
                        event_loop.exit();
                    }
                }
                KeyCode::ArrowRight | KeyCode::KeyN | KeyCode::Space => self.step(1),
                KeyCode::ArrowLeft | KeyCode::KeyP => self.step(-1),
                KeyCode::PageDown => self.step(10),
                KeyCode::PageUp => self.step(-10),
                KeyCode::Home if self.browse_len().is_some() => self.show_at(0),
                KeyCode::End => {
                    if let Some((n, _)) = self.browse_len().filter(|(n, _)| *n > 0) {
                        self.show_at(n - 1);
                    }
                }
                KeyCode::Enter | KeyCode::NumpadEnter => self.goto_typed(),
                KeyCode::Backspace => {
                    self.goto.backspace();
                    self.refresh_title();
                }
                code => {
                    if let Some(d) = digit_of(code)
                        && self.browse_len().is_some()
                    {
                        self.goto.push(d);
                        self.refresh_title();
                    }
                }
            },
            WindowEvent::RedrawRequested => {
                if let (true, Some(r), Some(atlas)) =
                    (self.help, &self.renderer, self.font_atlas.as_ref())
                {
                    let draws = self.help_draws(r.surface_size());
                    let overlay = TextOverlay {
                        atlas,
                        draws: &draws,
                        blend: &[],
                    };
                    if let Err(e) = r.render(RenderTarget::TextOnly(&overlay)) {
                        log::error!("render error: {e:#}");
                    }
                } else if let Some(r) = &self.renderer {
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
                    // Nothing to draw: say so, rather than leave the window
                    // empty.
                    let note = match (&target, self.font_atlas.as_ref()) {
                        (RenderTarget::Clear, Some(atlas)) => {
                            Some((atlas, self.blank_note_draws(r.surface_size())))
                        }
                        _ => None,
                    };
                    let result = match &note {
                        Some((atlas, draws)) => r.render(RenderTarget::TextOnly(&TextOverlay {
                            atlas,
                            draws,
                            blend: &[],
                        })),
                        None => r.render(target),
                    };
                    if let Err(e) = result {
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

/// The digit a number-row or keypad key types.
fn digit_of(code: KeyCode) -> Option<u8> {
    use KeyCode::*;
    Some(match code {
        Digit0 | Numpad0 => 0,
        Digit1 | Numpad1 => 1,
        Digit2 | Numpad2 => 2,
        Digit3 | Numpad3 => 3,
        Digit4 | Numpad4 => 4,
        Digit5 | Numpad5 => 5,
        Digit6 | Numpad6 => 6,
        Digit7 | Numpad7 => 7,
        Digit8 | Numpad8 => 8,
        Digit9 | Numpad9 => 9,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry with nothing to draw names itself, says why the window is
    /// empty, names the keys onward and echoes a number being typed.
    #[test]
    fn a_blank_entry_says_what_it_is_and_how_to_move_on() {
        let lines = blank_note_lines("0004_town01", false, true, None);
        assert_eq!(lines[0], ("0004_town01".to_string(), true));
        assert!(
            lines
                .iter()
                .any(|(l, _)| l.starts_with("Nothing in this entry"))
        );
        assert!(lines.iter().any(|(l, _)| l.starts_with("N / P")));
        let sound = blank_note_lines("0012_vab", true, true, Some("Go to 7_".into()));
        assert!(sound.iter().any(|(l, _)| l.starts_with("Sound only")));
        assert_eq!(sound.last(), Some(&("Go to 7_".to_string(), true)));
        let single = blank_note_lines("x.tim", false, false, None);
        assert!(single.iter().any(|(l, _)| l.contains("Esc: quit")));
    }
}

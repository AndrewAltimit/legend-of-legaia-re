//! `asset-viewer dialog` renders a MES dialog blob: walks the bytecode
//! interpreter through `DialogPlayer`, types out a page glyph-by-glyph, and
//! blits it into a centered dialog box via the extracted font.

use anyhow::{Context, Result};
use legaia_engine_render::{
    RenderTarget, Renderer, TextDraw, TextOverlay, UploadedFontAtlas, text_draws_for,
};
use legaia_font::Font;
use std::path::Path;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

/// Boot the dialog runner. Loads the MES blob, picks the message at
/// `message_index`, and hands it to a winit-driven [`DialogApp`].
pub(crate) fn run_dialog(
    path: &Path,
    message_index: usize,
    extracted_root: &Path,
    glyphs_per_frame: u8,
) -> Result<()> {
    let buf = std::fs::read(path).with_context(|| format!("read MES {}", path.display()))?;
    // Parse the blob to confirm it's a compact MES with an offset table; we
    // re-parse inside `DialogApp` so the interpreter borrows `buf` directly.
    let blob = legaia_mes::parse(&buf).with_context(|| format!("parse MES {}", path.display()))?;
    let table_len = blob
        .offset_table
        .as_ref()
        .map(|t| t.len())
        .unwrap_or_default();
    if table_len == 0 {
        anyhow::bail!(
            "MES {} has no offset table - only Compact-format blobs are renderable",
            path.display()
        );
    }
    if message_index >= table_len {
        anyhow::bail!(
            "message {} out of range (offset table has {} entries)",
            message_index,
            table_len,
        );
    }

    let font = Font::load_from_extracted(extracted_root).with_context(|| {
        format!(
            "load extracted font under {} (run `font-extract` first?)",
            extracted_root.display()
        )
    })?;

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = DialogApp {
        title: format!("MES {} - message {}", path.display(), message_index),
        path_label: path.display().to_string(),
        message_index,
        message_count: table_len,
        buf,
        font,
        glyphs_per_frame: glyphs_per_frame.max(1),
        page_glyphs: Vec::new(),
        log: Vec::new(),
        page_break: false,
        done: false,
        frame_count: 0,
        window: None,
        renderer: None,
        font_atlas: None,
    };
    app.reset_player()
        .with_context(|| "build initial dialog player")?;
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Dialog viewer state. Owns the MES buffer, the font, and a running
/// [`legaia_mes::DialogPlayer`] that emits one event per render frame.
struct DialogApp {
    title: String,
    path_label: String,
    message_index: usize,
    message_count: usize,
    buf: Vec<u8>,
    font: Font,
    glyphs_per_frame: u8,
    /// Glyph bytes that have been "typed out" so far on the current page.
    /// Reset on page break dismissal.
    page_glyphs: Vec<u8>,
    /// Last 3 control / unknown events for the status line.
    log: Vec<String>,
    page_break: bool,
    done: bool,
    frame_count: u64,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    font_atlas: Option<UploadedFontAtlas>,
}

impl DialogApp {
    /// Pull events from a freshly-built [`legaia_mes::DialogPlayer`] until
    /// the stream blocks (page break, done, idle, or the buffer ends), then
    /// return how many glyphs were appended this frame.
    ///
    /// Each `tick` of `DialogPlayer` advances at most one event when
    /// pacing allows, so we run it once per frame and stash the result in
    /// `page_glyphs` / `log`.
    fn step_player(&mut self) {
        if self.done || self.page_break {
            return;
        }
        // Build the player on each frame: cheap, and avoids a self-referential
        // borrow of `self.buf`.
        let mut player = match self.build_player() {
            Ok(p) => p,
            Err(e) => {
                log::error!("rebuild dialog player: {e:#}");
                self.done = true;
                return;
            }
        };
        // Replay glyphs already emitted this page so the player is in sync -
        // but only the count, not the events: the interpreter resumes from
        // the beginning of the message, so we discard `self.page_glyphs.len()`
        // events to fast-forward.
        for _ in 0..self.page_glyphs.len() {
            let _ = player.tick();
        }
        // Do one pacing tick: emit at most one new event.
        match player.tick() {
            legaia_mes::PlayerState::Idle => {}
            legaia_mes::PlayerState::Glyph(g) => self.page_glyphs.push(g),
            legaia_mes::PlayerState::WideGlyph(op, arg) => {
                // Wide glyphs are rendered as one tile; until the
                // wide-tile font lookup is wired we push the arg byte
                // so the visible page advances and log the pair.
                self.page_glyphs.push(arg);
                self.push_log(format!("[WIDE {:02X} {:02X}]", op, arg));
            }
            legaia_mes::PlayerState::PageBreak => {
                self.page_break = true;
                self.push_log("[PAGE]".to_string());
            }
            legaia_mes::PlayerState::WaitingForInput => {
                self.page_break = true;
            }
            legaia_mes::PlayerState::Control(ev) => {
                self.push_log(format!("{ev:?}"));
            }
            legaia_mes::PlayerState::Done => {
                self.done = true;
                self.push_log("[END]".to_string());
            }
        }
    }

    fn build_player(&self) -> Result<legaia_mes::DialogPlayer<'_>> {
        // `Interpreter::new_compact` borrows `&MesBlob`, which would tie the
        // returned player's lifetime to a local. Recompute the PC from the
        // offset table and use `Interpreter::new_at` to avoid the borrow.
        let blob = legaia_mes::parse(&self.buf)?;
        let table = blob.offset_table.as_ref().ok_or_else(|| {
            anyhow::anyhow!("MES blob has no offset table - only Compact format supported")
        })?;
        let entry = table
            .get(self.message_index)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("message index {} out of range", self.message_index))?;
        let bytecode_off = blob
            .bytecode_offset
            .ok_or_else(|| anyhow::anyhow!("MES blob has no bytecode offset (not Compact?)"))?;
        let pc = bytecode_off + entry as usize;
        if pc >= self.buf.len() {
            anyhow::bail!("computed pc {pc} past buffer end {}", self.buf.len());
        }
        let interp = legaia_mes::Interpreter::new_at(&self.buf, pc);
        let mut player = legaia_mes::DialogPlayer::new(interp);
        player.set_glyphs_per_frame(self.glyphs_per_frame);
        Ok(player)
    }

    /// Reset the per-page state for `self.message_index`. Called at startup
    /// and whenever the user picks a different message.
    fn reset_player(&mut self) -> Result<()> {
        // Validate the message index is reachable now.
        let _ = self.build_player()?;
        self.page_glyphs.clear();
        self.log.clear();
        self.page_break = false;
        self.done = false;
        Ok(())
    }

    fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > 3 {
            let drain = self.log.len() - 3;
            self.log.drain(..drain);
        }
    }

    /// Build the per-frame [`TextDraw`] list: the dialog window's text plus
    /// a status footer.
    fn build_text(&self, surface: (u32, u32)) -> Vec<TextDraw> {
        let Some(_atlas) = &self.font_atlas else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let white = [1.0, 1.0, 1.0, 1.0];
        let dim = [0.7, 0.85, 1.0, 1.0];
        let yellow = [1.0, 0.95, 0.55, 1.0];

        // Top-left header.
        let header = format!(
            "MES  msg {} / {}  ({} bytes)  fr {}",
            self.message_index,
            self.message_count,
            self.buf.len(),
            self.frame_count
        );
        let layout = self.font.layout_ascii(&header);
        out.extend(text_draws_for(&layout, (8, 8), dim));

        // Path on second line.
        let layout = self.font.layout_ascii(&self.path_label);
        out.extend(text_draws_for(&layout, (8, 26), dim));

        // Centered "dialog box" - pen near the lower-third of the surface.
        let (w, h) = surface;
        let pen_x = ((w as i32) / 6).max(16);
        let pen_y = ((h as i32) * 2 / 3).max(64);
        let layout = self.font.layout(&self.page_glyphs);
        out.extend(text_draws_for(&layout, (pen_x, pen_y), white));

        // Status footer.
        let footer = if self.done {
            "[end of message]   N: next message    P: previous   R: reset    Esc: quit".to_string()
        } else if self.page_break {
            "[page break]   Z / Enter: continue    R: reset    Esc: quit".to_string()
        } else {
            format!(
                "playing... {} glyphs   pace {} fr/glyph    R: reset    Esc: quit",
                self.page_glyphs.len(),
                self.glyphs_per_frame,
            )
        };
        let layout = self.font.layout_ascii(&footer);
        out.extend(text_draws_for(&layout, (8, (h as i32) - 36), yellow));

        // Recent control log.
        let mut log_y = (h as i32) - 22;
        for line in self.log.iter().rev().take(3) {
            let layout = self.font.layout_ascii(line);
            out.extend(text_draws_for(&layout, (8, log_y), dim));
            log_y -= 14;
        }
        out
    }

    fn upload_font(&mut self) {
        let Some(r) = &self.renderer else {
            return;
        };
        match r.upload_font(&self.font) {
            Ok(a) => self.font_atlas = Some(a),
            Err(e) => log::error!("font atlas upload failed: {e:#}"),
        }
    }

    fn advance_page(&mut self) {
        if self.page_break {
            self.page_break = false;
            // Drop the typed-out glyphs of the prior page so the player
            // continues into the next page from a fresh visual state.
            self.page_glyphs.clear();
        }
    }

    fn jump_message(&mut self, delta: i32) {
        let next = (self.message_index as i32) + delta;
        if next < 0 || (next as usize) >= self.message_count {
            return;
        }
        self.message_index = next as usize;
        if let Err(e) = self.reset_player() {
            log::warn!("can't jump to message {next}: {e:#}");
        }
    }
}

impl ApplicationHandler for DialogApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(720.0, 480.0));
        let window = match evl.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("create_window: {e:#}");
                evl.exit();
                return;
            }
        };
        let size = window.inner_size();
        let renderer = match Renderer::new(window.clone(), size.width, size.height) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Renderer::new: {e:#}");
                evl.exit();
                return;
            }
        };
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.upload_font();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => evl.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match code {
                KeyCode::Escape => evl.exit(),
                KeyCode::KeyZ | KeyCode::Enter => self.advance_page(),
                KeyCode::KeyR => {
                    if let Err(e) = self.reset_player() {
                        log::warn!("reset failed: {e:#}");
                    }
                }
                KeyCode::KeyN | KeyCode::PageDown => self.jump_message(1),
                KeyCode::KeyP | KeyCode::PageUp => self.jump_message(-1),
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                self.frame_count += 1;
                self.step_player();
                if let (Some(r), Some(atlas)) = (&self.renderer, self.font_atlas.as_ref()) {
                    let (w, h) = r.surface_size();
                    let draws = self.build_text((w, h));
                    let overlay = TextOverlay {
                        atlas,
                        draws: &draws,
                    };
                    if let Err(e) = r.render(RenderTarget::TextOnly(&overlay)) {
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

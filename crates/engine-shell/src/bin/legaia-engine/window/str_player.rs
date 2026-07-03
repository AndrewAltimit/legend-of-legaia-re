//! Extracted from `window.rs` (mechanical split; behavior-preserving).
//!
//! The `play-str` MDEC movie player (`StrPlayerApp`) plus its entry point
//! (`cmd_play_str`) and the ISO-path resolver shared with the boot cutscene
//! driver.

use super::*;

pub(crate) fn cmd_play_str(
    str_file: &Path,
    disc: Option<&Path>,
    _win_width: u32,
    _win_height: u32,
) -> Result<()> {
    use legaia_engine_shell::cutscene_av::{
        CutsceneAudio, decode_str_av_from_disc, decode_str_video_only,
    };

    // With a disc image the STR is read as raw 2352-byte sectors so its
    // interleaved XA audio track comes along; without one we play the
    // (video-only) Form-1 extract from the filesystem.
    let (frames, timing, audio): (_, _, Option<CutsceneAudio>) = if let Some(disc_path) = disc {
        let (lba, size) = resolve_iso_file(disc_path, str_file)?;
        let count = size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let av = decode_str_av_from_disc(disc_path, lba, count)
            .with_context(|| format!("decode STR {} from disc", str_file.display()))?;
        (av.frames, av.timing, av.audio)
    } else {
        let (f, t) = decode_str_video_only(str_file)?;
        (f, t, None)
    };
    if frames.is_empty() {
        anyhow::bail!("no video frames found in {}", str_file.display());
    }
    println!(
        "play-str: {} frames, {}×{}, {:.2} fps, audio: {}",
        frames.len(),
        frames[0].width,
        frames[0].height,
        timing.fps,
        match &audio {
            Some(a) => format!(
                "{:.1} kHz {} ({:.1}s)",
                a.sample_rate as f64 / 1000.0,
                if matches!(a.channels, legaia_xa::Channels::Stereo) {
                    "stereo"
                } else {
                    "mono"
                },
                a.duration_secs()
            ),
            None => "none".into(),
        }
    );

    // Open the audio device only when there is a track to play. A device
    // failure (CI / headless) degrades to wall-clock-paced video, not an error.
    let audio_out = if audio.is_some() {
        match legaia_engine_audio::AudioOut::new() {
            Ok(a) => Some(a),
            Err(e) => {
                log::warn!("play-str: audio device unavailable ({e:#}); playing video only");
                None
            }
        }
    } else {
        None
    };

    let mut app = StrPlayerApp {
        win: EngineWindow::new(),
        frames,
        uploaded: None,
        frame_period: timing.frame_period(),
        clock: None,
        audio_out,
        pending_audio: audio,
    };
    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Resolve an ISO9660 path inside a disc image to its `(lba, size)`. Matches
/// case-insensitively and tolerates a leading slash. Errors if not found.
pub(crate) fn resolve_iso_file(disc_path: &Path, iso_path: &Path) -> Result<(u32, u32)> {
    use legaia_iso::iso9660;
    let want = iso_path
        .to_string_lossy()
        .trim_start_matches('/')
        .replace('\\', "/")
        .to_ascii_uppercase();
    let mut disc = legaia_iso::raw::RawDisc::open(disc_path)
        .with_context(|| format!("open disc {}", disc_path.display()))?;
    let vol = iso9660::read_volume(&mut disc).context("read ISO volume")?;
    let files = iso9660::walk_files(&mut disc, &vol.root).context("walk ISO files")?;
    files
        .into_iter()
        .find(|(p, _)| p.to_ascii_uppercase() == want)
        .map(|(_, rec)| (rec.lba, rec.size))
        .ok_or_else(|| anyhow::anyhow!("{} not found on disc {}", want, disc_path.display()))
}

struct StrPlayerApp {
    win: EngineWindow,
    frames: Vec<legaia_mdec::VideoFrame>,
    uploaded: Option<legaia_engine_render::UploadedTexture>,
    /// Wall-clock duration to hold each frame (from the stream's detected fps).
    frame_period: std::time::Duration,
    /// When playback started; the wall-clock fallback frame index is
    /// `elapsed / frame_period` (used only when no audio track is playing).
    clock: Option<std::time::Instant>,
    /// Live audio output, present only when an interleaved XA track decoded
    /// and the device opened. The video clock reads its cursor for A/V sync.
    /// Owned solely by the player (single-threaded), so no `Arc` is needed.
    audio_out: Option<legaia_engine_audio::AudioOut>,
    /// The decoded audio track, staged into `audio_out` on the first redraw so
    /// the audio cursor and the video start together. Taken once.
    pending_audio: Option<legaia_engine_shell::cutscene_av::CutsceneAudio>,
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
                // Stage the audio on the first redraw so its cursor (the video
                // clock) starts when the picture does.
                if let (Some(out), Some(track)) =
                    (self.audio_out.as_ref(), self.pending_audio.take())
                {
                    out.play_xa(track.pcm, track.sample_rate, track.channels, false, 0x4000);
                }
                // A/V sync: drive the visible frame off the audio cursor when a
                // track is playing (audio is the hardware-paced master clock);
                // otherwise pace off wall-clock. Once the due frame passes the
                // last decoded frame, playback is done and the window closes.
                let now = std::time::Instant::now();
                let start = *self.clock.get_or_insert(now);
                let wall = now.duration_since(start).as_secs_f64();
                let audio_secs = self.audio_out.as_ref().and_then(|o| o.xa_cursor_secs());
                let due = legaia_engine_shell::cutscene_av::due_video_frame(
                    audio_secs,
                    wall,
                    self.frame_period.as_secs_f64(),
                );
                if due >= self.frames.len() {
                    if let Some(out) = self.audio_out.as_ref() {
                        out.stop_xa();
                    }
                    event_loop.exit();
                    return;
                }
                if let Some(renderer) = self.win.renderer() {
                    let f = &self.frames[due];
                    match renderer.upload_texture(&f.rgba, f.width, f.height) {
                        Ok(tex) => self.uploaded = Some(tex),
                        Err(e) => log::warn!("upload: {e}"),
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

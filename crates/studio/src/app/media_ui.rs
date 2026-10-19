//! The "Movies & voice" mode: the disc's CD-XA audio files, channel by
//! channel, and its STR movies with their soundtrack.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use eframe::egui::{self, ColorImage, RichText, TextureHandle, TextureOptions};
use legaia_mdec::str_av::StrAv;

use super::App;
use crate::Source;
use crate::library::IsoFile;
use crate::media::{self, MediaKind, XaTrack};

enum Loaded {
    Xa(Vec<XaTrack>),
    Str(Arc<StrAv>),
}

#[derive(Default)]
pub(crate) struct MediaState {
    /// The chosen file, by ISO path.
    sel: Option<String>,
    loading: Option<(String, Receiver<Result<Loaded, String>>)>,
    loaded: Option<(String, Result<Loaded, String>)>,
    frame: usize,
    playing: bool,
    /// Wall clock and frame the movie started from (no soundtrack case).
    since: Option<(Instant, usize)>,
    tex: Option<(usize, TextureHandle)>,
    /// Scripted `LEGAIA_STUDIO_MEDIA` applied.
    auto_done: bool,
}

impl MediaState {
    /// `true` while a file is being read (the scripted screenshot waits).
    pub(crate) fn busy(&self) -> bool {
        self.loading.is_some()
    }
}

impl App {
    fn media_auto(&mut self) {
        if self.media.auto_done {
            return;
        }
        self.media.auto_done = true;
        let Some(spec) = self.auto.media.clone() else {
            return;
        };
        let (path, frame) = match spec.split_once('@') {
            Some((p, f)) => (p.to_string(), f.trim().parse().unwrap_or(0)),
            None => (spec, 0),
        };
        self.media.sel = Some(path);
        self.media.frame = frame;
    }

    fn media_start_load(&mut self, bin: &std::path::Path, f: &IsoFile) {
        let (tx, rx) = channel();
        let (bin, f2) = (bin.to_path_buf(), f.clone());
        std::thread::spawn(move || {
            let res = match media::kind_of(&f2.path) {
                Some(MediaKind::Xa) => media::xa_channels(&bin, &f2).and_then(|chs| {
                    chs.iter()
                        .map(media::decode_channel)
                        .collect::<anyhow::Result<Vec<_>>>()
                        .map(Loaded::Xa)
                }),
                Some(MediaKind::Str) => {
                    media::str_movie(&bin, &f2).map(|m| Loaded::Str(Arc::new(m)))
                }
                None => Err(anyhow::anyhow!("not a streamed media file")),
            };
            let _ = tx.send(res.map_err(|e| format!("{e:#}")));
        });
        self.media.loading = Some((f.path.clone(), rx));
        self.media.tex = None;
        self.media.playing = false;
    }

    pub(super) fn media_mode(&mut self, ctx: &egui::Context) {
        let Some(lib) = self.studio.library.clone() else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Open a disc image first.");
            });
            return;
        };
        let Source::Disc(bin) = lib.source.clone() else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(
                    "CD-XA audio and STR movies are read as raw Mode 2 sectors, which only a disc \
                     image has; an extracted folder's copies lost their Form 2 audio. Open the \
                     disc image to preview them.",
                );
            });
            return;
        };
        self.media_auto();
        let mut files: Vec<IsoFile> = lib
            .iso_files
            .iter()
            .filter(|f| media::kind_of(&f.path).is_some())
            .cloned()
            .collect();
        files.sort_by_key(|f| media::natural_key(&f.path));

        let mut pick = None;
        egui::SidePanel::left("media_files")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (title, kind) in
                            [("Movies", MediaKind::Str), ("CD-XA audio", MediaKind::Xa)]
                        {
                            ui.heading(title);
                            for f in files
                                .iter()
                                .filter(|f| media::kind_of(&f.path) == Some(kind))
                            {
                                let sel = self.media.sel.as_deref() == Some(f.path.as_str());
                                let text = format!(
                                    "{:<16} {:>9}",
                                    f.path,
                                    super::human_size(f.size as u64)
                                );
                                if ui
                                    .selectable_label(sel, RichText::new(text).monospace())
                                    .clicked()
                                {
                                    pick = Some(f.path.clone());
                                }
                            }
                            ui.separator();
                        }
                    });
            });
        if let Some(p) = pick
            && self.media.sel.as_deref() != Some(p.as_str())
        {
            self.player.stop();
            self.media.sel = Some(p);
            self.media.frame = 0;
        }

        // Start or pick up the read of the chosen file.
        if let Some(sel) = self.media.sel.clone() {
            let have = self.media.loaded.as_ref().is_some_and(|(p, _)| *p == sel)
                || self.media.loading.as_ref().is_some_and(|(p, _)| *p == sel);
            if !have && let Some(f) = files.iter().find(|f| f.path == sel) {
                self.media_start_load(&bin, f);
            }
        }
        if let Some((p, rx)) = &self.media.loading
            && let Ok(res) = rx.try_recv()
        {
            self.media.loaded = Some((p.clone(), res));
            self.media.loading = None;
        }
        if self.media.loading.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(sel) = self.media.sel.clone() else {
                ui.label("Pick a movie or an XA audio file on the left.");
                ui.label(
                    RichText::new(
                        "Movies decode with the Iki MDEC decoder and play with their XA soundtrack; \
                         XA files hold several channels each (voice lines, streamed music).",
                    )
                    .weak(),
                );
                return;
            };
            ui.heading(&sel);
            if self.media.loading.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading sectors...");
                });
                return;
            }
            enum View {
                Failed(String),
                Xa(Vec<(u8, u8, u32, legaia_xa::Channels, f64)>),
                Str(Arc<StrAv>),
            }
            let view = match &self.media.loaded {
                None => return,
                Some((_, Err(e))) => View::Failed(e.clone()),
                Some((_, Ok(Loaded::Xa(tracks)))) => View::Xa(
                    tracks
                        .iter()
                        .map(|t| (t.file_no, t.ch_no, t.sample_rate, t.channels, t.duration_secs()))
                        .collect(),
                ),
                Some((_, Ok(Loaded::Str(m)))) => View::Str(Arc::clone(m)),
            };
            match view {
                View::Failed(e) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                View::Xa(tracks) => self.xa_tracks(ui, &sel, &tracks),
                View::Str(m) => self.str_view(ui, &sel, &m),
            }
        });
    }

    fn xa_tracks(
        &mut self,
        ui: &mut egui::Ui,
        sel: &str,
        tracks: &[(u8, u8, u32, legaia_xa::Channels, f64)],
    ) {
        ui.horizontal(|ui| {
            match &self.player.now_playing {
                Some(n) => {
                    ui.label(format!("Playing: {n}"));
                    if let Some(t) = self.player.stream_secs() {
                        ui.label(format!("{t:.1} s"));
                    }
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(200));
                }
                None => {
                    ui.label("Stopped");
                }
            }
            if ui.button("Stop").clicked() {
                self.player.stop();
            }
        });
        ui.label(format!("{} channels", tracks.len()));
        ui.separator();
        let stem = sel.rsplit('/').next().unwrap_or(sel).replace('.', "_");
        let mut action: Option<(usize, bool)> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, (file_no, ch_no, rate, channels, secs)) in tracks.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.monospace(format!(
                        "file {file_no:2} ch {ch_no:2}  {:<6} {rate} Hz  {:>6.1} s",
                        if matches!(channels, legaia_xa::Channels::Stereo) {
                            "stereo"
                        } else {
                            "mono"
                        },
                        secs
                    ));
                    if ui.small_button("Play").clicked() {
                        action = Some((i, false));
                    }
                    if ui.small_button("WAV...").clicked() {
                        action = Some((i, true));
                    }
                });
            }
        });
        let Some((i, export)) = action else { return };
        let Some((_, Ok(Loaded::Xa(all)))) = &self.media.loaded else {
            return;
        };
        let t = &all[i];
        let (pcm, rate, ch, file_no, ch_no) =
            (t.pcm.clone(), t.sample_rate, t.channels, t.file_no, t.ch_no);
        let label = format!("{sel} file {file_no} ch {ch_no}");
        if export {
            let name = format!("{stem}_f{file_no}_c{ch_no}.wav");
            if let Some(out) = self.export_target(&name, "wav") {
                let res = media::write_wav(&out, &pcm, ch, rate);
                self.report("WAV", res, &out);
            }
        } else if let Err(e) = self.player.play_stream(pcm, rate, ch, label) {
            self.studio.error = Some(format!("{e:#}"));
        }
    }

    fn str_view(&mut self, ui: &mut egui::Ui, sel: &str, m: &StrAv) {
        let n = m.frames.len();
        if n == 0 {
            ui.label("No video frames assembled from this file.");
            return;
        }
        let period = m.frame_period_secs();
        let (w, h) = m.size();
        ui.label(format!(
            "{n} frames, {w}x{h}, {:.1} fps, {}",
            1.0 / period,
            match &m.audio {
                Some(a) => format!(
                    "{} Hz {} soundtrack, {:.1} s",
                    a.sample_rate,
                    if a.stereo() { "stereo" } else { "mono" },
                    a.duration_secs()
                ),
                None => "no soundtrack".into(),
            }
        ));
        let st = &mut self.media;
        st.frame = st.frame.min(n - 1);
        let mut start: Option<usize> = None;
        let mut stop = false;
        let mut export = false;
        ui.horizontal(|ui| {
            let label = if st.playing { "Pause" } else { "Play" };
            if ui.button(label).clicked() {
                if st.playing {
                    stop = true;
                } else {
                    start = Some(st.frame);
                }
            }
            if ui
                .add(egui::Slider::new(&mut st.frame, 0..=n - 1).text("frame"))
                .changed()
            {
                stop = true;
            }
            ui.label(format!("{:.2} s", st.frame as f64 * period));
            if ui.button("Export frame PNG...").clicked() {
                export = true;
            }
        });
        if stop && st.playing {
            st.playing = false;
            self.player.stop();
        }
        if let Some(from) = start {
            let st = &mut self.media;
            st.playing = true;
            st.since = Some((Instant::now(), from));
            // The soundtrack resumes at the sample the starting frame shows.
            if let Some(a) = &m.audio
                && let Err(e) = self.player.play_stream(
                    media::soundtrack_from(a, from as f64 * period),
                    a.sample_rate,
                    a.channels,
                    format!("{sel} soundtrack"),
                )
            {
                self.studio.error = Some(format!("{e:#}"));
            }
        }
        let st = &mut self.media;
        if st.playing {
            let (t0, from) = st.since.unwrap_or((Instant::now(), 0));
            // Pace by the soundtrack's own clock when it is playing, the
            // way the engine's movie player locks frames to the XA cursor;
            // the stream started at the starting frame's time.
            let start = from as f64 * period;
            let secs = match self.player.stream_secs() {
                Some(s) => start + s,
                None => start + t0.elapsed().as_secs_f64(),
            };
            let f = (secs / period) as usize;
            if f >= n {
                st.playing = false;
                st.frame = n - 1;
            } else {
                st.frame = f;
            }
            ui.ctx().request_repaint();
        }
        let frame = st.frame;
        if st.tex.as_ref().map(|(k, _)| *k) != Some(frame) {
            st.tex = m.frames[frame].decode_rgba().map(|rgba| {
                let fw = m.frames[frame].width as usize;
                let fh = m.frames[frame].height as usize;
                let img = ColorImage::from_rgba_unmultiplied([fw, fh], &rgba);
                (
                    frame,
                    ui.ctx().load_texture("str", img, TextureOptions::LINEAR),
                )
            });
        }
        if export {
            let stem = sel.rsplit('/').next().unwrap_or(sel).replace('.', "_");
            let name = format!("{stem}_frame{frame:04}.png");
            if let Some(out) = self.export_target(&name, "png") {
                let f = &m.frames[frame];
                let res = f
                    .decode_rgba()
                    .ok_or_else(|| anyhow::anyhow!("frame {frame} does not decode"))
                    .and_then(|rgba| {
                        legaia_tim::write_png(&out, f.width as usize, f.height as usize, &rgba)
                    });
                self.report("PNG", res, &out);
            }
        }
        match &self.media.tex {
            Some((_, tex)) => {
                let avail = ui.available_size();
                let size = tex.size_vec2();
                let scale = (avail.x / size.x).min(avail.y / size.y).max(0.1);
                ui.image((tex.id(), size * scale));
            }
            None => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("frame {frame} does not decode"),
                );
            }
        }
    }
}

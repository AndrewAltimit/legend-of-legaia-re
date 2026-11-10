//! Audio preview through `legaia-engine-audio`: one-shot VAG samples and SEQ
//! playback on the from-scratch SPU + sequencer, opened lazily so a machine
//! without an audio device still runs the rest of the app.

use anyhow::{Context, Result};
use legaia_engine_audio::spu::ram::SPU_RAM_BYTES;
use legaia_engine_audio::{AudioOut, AudioSink, Sequencer, Spu, SpuAllocator, VabBank};

use crate::analyze::{Analysis, SeqRef, VabRef};

/// What kind of sound a [`Player`] has going, which decides where its
/// position comes from and what ends it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ClipKind {
    /// A one-shot sample on the preview voice: the wall clock is its
    /// position.
    Sample,
    /// Streamed PCM on the mixer's XA voice, started `offset` seconds into
    /// its source: the voice's cursor is its position.
    Stream { offset: f64 },
    /// A sequence: the sequencer says when it has run out; a looped one
    /// never does.
    Seq { looped: bool },
}

/// The sound a [`Player`] has going.
#[derive(Debug, Clone)]
struct Clip {
    kind: ClipKind,
    /// Its whole length in seconds, where one is known (not for a SEQ).
    total: Option<f64>,
    started: std::time::Instant,
}

/// The streamed PCM last started, kept so the transport can seek in it.
struct StreamSource {
    pcm: std::sync::Arc<Vec<i16>>,
    rate: u32,
    channels: psx_xa::Channels,
}

/// Where the playing sound is: seconds in, and its whole length when known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub secs: f64,
    pub total: Option<f64>,
}

impl Progress {
    /// How far through, `0.0..=1.0`; `None` when the length is not known.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total.filter(|t| *t > 0.0)?;
        Some((self.secs / total).clamp(0.0, 1.0) as f32)
    }

    /// `0:12 / 0:26`, or `0:12` when the length is not known.
    pub fn text(&self) -> String {
        match self.total {
            Some(t) => format!("{} / {}", clock(self.secs.min(t)), clock(t)),
            None => clock(self.secs),
        }
    }
}

/// Seconds as `m:ss` (`h:mm:ss` from an hour up), rounded down.
pub fn clock(secs: f64) -> String {
    let s = if secs.is_finite() && secs > 0.0 {
        secs as u64
    } else {
        0
    };
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The slack past a one-shot's length before it counts as ended: the device
/// buffer still holds its tail when the clock reaches the length.
const END_SLACK_SECS: f64 = 0.15;

/// Whether a sound at `secs` of `total` has ended. A sound with no known
/// length ends only when its source says so (`source_done`).
fn has_ended(secs: f64, total: Option<f64>, source_done: bool) -> bool {
    match total {
        Some(t) => secs >= t + END_SLACK_SECS,
        None => source_done,
    }
}

/// The sample frame a seek to `secs` lands on in a stream of `frames` frames
/// at `rate` Hz, held inside the stream.
fn seek_frame(secs: f64, rate: u32, frames: usize) -> usize {
    if !secs.is_finite() || secs <= 0.0 {
        return 0;
    }
    ((secs * f64::from(rate)) as usize).min(frames)
}

fn frames_per_sample(channels: psx_xa::Channels) -> usize {
    if matches!(channels, psx_xa::Channels::Stereo) {
        2
    } else {
        1
    }
}

#[derive(Default)]
pub struct Player {
    out: Option<AudioOut>,
    /// What is playing, for the UI ("sample 3", "SEQ @ 0x1234"); `None` once
    /// it has ended ([`Self::poll`]) or was stopped.
    pub now_playing: Option<String>,
    clip: Option<Clip>,
    stream: Option<StreamSource>,
    /// Output volume in percent; `None` is the desktop default
    /// ([`legaia_engine_audio::DESKTOP_MASTER_VOLUME_DEFAULT`]).
    volume: Option<u8>,
}

impl Player {
    fn out(&mut self) -> Result<&AudioOut> {
        if self.out.is_none() {
            let out = AudioOut::new().context("opening the audio output device")?;
            out.set_master_volume(self.volume());
            self.out = Some(out);
        }
        Ok(self.out.as_ref().expect("just opened"))
    }

    /// The output volume in percent of the full mix.
    pub fn volume(&self) -> u8 {
        self.volume
            .unwrap_or(legaia_engine_audio::DESKTOP_MASTER_VOLUME_DEFAULT)
    }

    /// Set the output volume (percent of the full mix), live if the output
    /// is already open.
    pub fn set_volume(&mut self, percent: u8) {
        let percent = percent.min(100);
        self.volume = Some(percent);
        if let Some(out) = &self.out {
            out.set_master_volume(percent);
        }
    }

    pub fn play_pcm(&mut self, pcm: Vec<i16>, rate: u32, label: String) -> Result<()> {
        self.stop();
        let total = (rate > 0).then(|| pcm.len() as f64 / f64::from(rate));
        self.out()?.play_pcm_mono(pcm, rate);
        self.now_playing = Some(label);
        self.clip = Some(Clip {
            kind: ClipKind::Sample,
            total,
            started: std::time::Instant::now(),
        });
        Ok(())
    }

    /// Play `seq` with the instruments of bank `vab`, both from entry `a`.
    pub fn play_seq(
        &mut self,
        a: &Analysis,
        vab: &VabRef,
        seq: &SeqRef,
        looped: bool,
    ) -> Result<()> {
        self.stop();
        let song = psx_seq::Seq::parse(&a.bytes[seq.offset..]).context("parse SEQ")?;
        let out = self.out()?;
        let bank = out.with_spu(|spu: &mut Spu| {
            let mut alloc = SpuAllocator::new(0x1000, SPU_RAM_BYTES as u32 - 0x1000);
            VabBank::upload(spu, &mut alloc, &vab.report, &a.bytes)
        });
        let mut sequencer = Sequencer::new(song, bank);
        if looped {
            sequencer.set_loop_to(0);
        }
        out.attach_sequencer(sequencer);
        self.now_playing = Some(format!(
            "SEQ @ 0x{:X} with VAB @ 0x{:X}",
            seq.offset, vab.offset
        ));
        self.clip = Some(Clip {
            kind: ClipKind::Seq { looped },
            total: None,
            started: std::time::Instant::now(),
        });
        Ok(())
    }

    /// Play streamed PCM (an XA channel or a movie soundtrack) on the
    /// mixer's XA voice, the one the engine streams CD audio through. Unlike
    /// [`Self::play_pcm`] it takes stereo and any length.
    pub fn play_stream(
        &mut self,
        pcm: Vec<i16>,
        rate: u32,
        channels: psx_xa::Channels,
        label: String,
    ) -> Result<()> {
        self.stop();
        let per = frames_per_sample(channels);
        let total = (rate > 0).then(|| (pcm.len() / per) as f64 / f64::from(rate));
        let pcm = std::sync::Arc::new(pcm);
        self.out()?
            .play_xa(pcm.as_ref().clone(), rate, channels, false, 0x4000);
        self.now_playing = Some(label);
        self.clip = Some(Clip {
            kind: ClipKind::Stream { offset: 0.0 },
            total,
            started: std::time::Instant::now(),
        });
        self.stream = Some(StreamSource {
            pcm,
            rate,
            channels,
        });
        Ok(())
    }

    /// Move the playing stream ([`Self::play_stream`]) to `secs` from its
    /// start. Nothing happens with no stream playing.
    pub fn seek_stream(&mut self, secs: f64) {
        let (Some(src), Some(clip), Some(out)) = (&self.stream, &mut self.clip, &self.out) else {
            return;
        };
        if !matches!(clip.kind, ClipKind::Stream { .. }) {
            return;
        }
        let per = frames_per_sample(src.channels);
        let frame = seek_frame(secs, src.rate, src.pcm.len() / per);
        out.play_xa(
            src.pcm[frame * per..].to_vec(),
            src.rate,
            src.channels,
            false,
            0x4000,
        );
        clip.kind = ClipKind::Stream {
            offset: frame as f64 / f64::from(src.rate.max(1)),
        };
    }

    /// Whether the playing sound is a stream [`Self::seek_stream`] can move.
    pub fn can_seek(&self) -> bool {
        self.stream.is_some()
            && matches!(
                self.clip.as_ref().map(|c| c.kind),
                Some(ClipKind::Stream { .. })
            )
    }

    /// Where the playing sound is, `None` with nothing playing.
    pub fn progress(&self) -> Option<Progress> {
        let clip = self.clip.as_ref()?;
        let secs = match clip.kind {
            ClipKind::Stream { offset } => {
                offset
                    + self
                        .stream_secs()
                        .unwrap_or_else(|| clip.started.elapsed().as_secs_f64())
            }
            ClipKind::Sample | ClipKind::Seq { .. } => clip.started.elapsed().as_secs_f64(),
        };
        Some(Progress {
            secs,
            total: clip.total,
        })
    }

    /// Notice a sound that has played out, so the UI stops saying it plays:
    /// a sample or a stream past its length, a sequence that is not looped
    /// and has finished. Call once a frame. `true` while something plays.
    pub fn poll(&mut self) -> bool {
        let Some(clip) = &self.clip else {
            return false;
        };
        let source_done = match clip.kind {
            ClipKind::Seq { looped } => {
                !looped && self.seq_progress().is_some_and(|(_, _, done)| done)
            }
            _ => false,
        };
        let secs = self.progress().map_or(0.0, |p| p.secs);
        if has_ended(secs, clip.total, source_done) {
            self.now_playing = None;
            self.clip = None;
            return false;
        }
        true
    }

    /// Seconds into the playing stream ([`Self::play_stream`]), the clock a
    /// movie preview paces its frames by.
    pub fn stream_secs(&self) -> Option<f64> {
        self.out.as_ref()?.xa_cursor_secs()
    }

    /// `(tick, bpm, finished)` of the playing SEQ.
    pub fn seq_progress(&self) -> Option<(u64, f32, bool)> {
        let p = self.out.as_ref()?.sequencer_progress()?;
        Some((p.tick, p.bpm, p.finished))
    }

    pub fn stop(&mut self) {
        if let Some(out) = &self.out {
            out.detach_sequencer();
            out.stop_xa();
            out.stop();
        }
        self.now_playing = None;
        self.clip = None;
        self.stream = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_reads_minutes_and_hours() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(9.9), "0:09");
        assert_eq!(clock(75.0), "1:15");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(-3.0), "0:00");
        assert_eq!(clock(f64::NAN), "0:00");
    }

    #[test]
    fn progress_text_never_runs_past_the_length() {
        let p = Progress {
            secs: 27.0,
            total: Some(26.3),
        };
        assert_eq!(p.text(), "0:26 / 0:26");
        assert_eq!(p.fraction(), Some(1.0));
        let open = Progress {
            secs: 12.0,
            total: None,
        };
        assert_eq!(open.text(), "0:12");
        assert_eq!(open.fraction(), None);
        let empty = Progress {
            secs: 0.0,
            total: Some(0.0),
        };
        assert_eq!(empty.fraction(), None);
    }

    #[test]
    fn a_one_shot_ends_just_past_its_length() {
        assert!(!has_ended(0.9, Some(1.0), false));
        assert!(!has_ended(1.0, Some(1.0), false));
        assert!(has_ended(1.0 + END_SLACK_SECS, Some(1.0), false));
        // A sequence has no length: only its sequencer ends it.
        assert!(!has_ended(600.0, None, false));
        assert!(has_ended(0.0, None, true));
    }

    #[test]
    fn a_seek_stays_inside_the_stream() {
        assert_eq!(seek_frame(-1.0, 37_800, 1000), 0);
        assert_eq!(seek_frame(f64::NAN, 37_800, 1000), 0);
        assert_eq!(seek_frame(0.01, 37_800, 1000), 378);
        assert_eq!(seek_frame(5.0, 37_800, 1000), 1000);
    }

    #[test]
    fn nothing_playing_has_no_progress() {
        let mut p = Player::default();
        assert!(p.progress().is_none());
        assert!(!p.poll());
        assert!(!p.can_seek());
        p.seek_stream(3.0);
        assert!(p.now_playing.is_none());
    }
}

//! Device-free audio sink - the cpal device's stand-in for a native test.
//!
//! [`crate::AudioOut`] is the only handle that carries the BGM half of the
//! mixing core: attach / pause / crossfade / swap all live behind a
//! `cpal::Stream`, and opening one needs a real output device. A headless
//! test therefore cannot tick the mixer at all, which is why the SFX enqueue,
//! the VAB upload and the sequencer's voice allocator have no test-side host
//! (see `docs/tooling/reach-triage.md`, the `engine-audio` (a) rows).
//!
//! [`TestAudioSink`] closes that: it owns the **same** [`crate::StreamResampler`]
//! the cpal callback owns and drives it by pulling frames, so every kernel on
//! the output path runs exactly as it does under a device - sequencer tick,
//! SPU tick, ADSR advance, master fade, XA mix, mono downmix, master mute. The
//! only thing missing is the callback that would hand the frames to hardware.
//!
//! Two rules this module exists to keep:
//!
//! 1. **No second copy of the mixing math.** Every method here delegates to
//!    the same private [`crate::StreamResampler`] method `AudioOut` delegates
//!    to. A sink with its own fade/attach logic would assert about itself
//!    rather than about the device path.
//! 2. **Measure at the output.** [`Self::render_frames`] returns a
//!    [`SinkMeasure`] over the emitted PCM, not a count of calls made. A wired
//!    kernel that runs and produces silence is indistinguishable from an
//!    unwired one at the call site; it is distinguishable at the samples.

use crate::{Sequencer, SequencerProgress, Spu, StreamResampler};

/// Video frames per second the PSX runs at, for [`TestAudioSink::render_video_frame`].
pub const VIDEO_HZ: u32 = 60;

/// What a stretch of rendered output actually contained.
///
/// Deliberately not a bool: "did any sample move" and "how loud did it get"
/// fail differently. A cue that resolves to a valid voice but an empty ADPCM
/// body yields `nonzero > 0, peak` tiny; a cue that never keyed yields all
/// zeros; a correct one yields a peak in the thousands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SinkMeasure {
    /// Output frames pulled.
    pub frames: usize,
    /// Loudest absolute sample across both channels.
    pub peak: i32,
    /// Frames where either channel was non-zero.
    pub nonzero: usize,
    /// Sum of `|l| + |r|` across every frame - the level integral, which
    /// separates "one click" from "a track playing".
    pub sum_abs: u64,
}

impl SinkMeasure {
    /// No sample in the window moved.
    pub fn is_silent(&self) -> bool {
        self.nonzero == 0
    }

    /// Mean absolute level per channel over the window (`0.0` for an empty
    /// window).
    pub fn mean_abs(&self) -> f64 {
        if self.frames == 0 {
            0.0
        } else {
            self.sum_abs as f64 / (self.frames as f64 * 2.0)
        }
    }

    fn accumulate(&mut self, (l, r): (i16, i16)) {
        self.frames += 1;
        if l != 0 || r != 0 {
            self.nonzero += 1;
        }
        // Widen before the absolute value: `i16::MIN.abs()` overflows i16, and
        // a full-scale negative sample is exactly the case a mixer produces.
        let (al, ar) = ((l as i32).abs(), (r as i32).abs());
        self.peak = self.peak.max(al).max(ar);
        self.sum_abs += al as u64 + ar as u64;
    }

    /// Fold another window into this one (used to total a per-frame loop).
    pub fn merge(&mut self, other: SinkMeasure) {
        self.frames += other.frames;
        self.peak = self.peak.max(other.peak);
        self.nonzero += other.nonzero;
        self.sum_abs += other.sum_abs;
    }
}

/// Device-free stand-in for [`crate::AudioOut`]: the same mixing core, pulled
/// by the caller instead of by an audio device.
///
/// Mirrors the `AudioOut` surface a host uses per frame, so a test can drive
/// the real BGM/SFX plumbing: stage a [`crate::VabBank`] through
/// [`Self::with_spu`], attach a [`Sequencer`], render a video frame's worth of
/// output, pause, resume, swap tracks, and read back what the speakers would
/// have received.
pub struct TestAudioSink {
    state: StreamResampler,
    device_rate: u32,
}

impl TestAudioSink {
    /// A sink emitting at `device_rate` Hz. Pass
    /// [`crate::SPU_INTERNAL_RATE`] (44 100) for 1:1 output with no
    /// resampling - the shape a parity assertion wants.
    pub fn new(device_rate: u32) -> Self {
        Self {
            state: StreamResampler::new(device_rate.max(1)),
            device_rate: device_rate.max(1),
        }
    }

    /// Output frames in one 60 Hz video frame at this sink's rate.
    pub fn frames_per_video_frame(&self) -> usize {
        (self.device_rate / VIDEO_HZ).max(1) as usize
    }

    /// Run a closure against the SPU model - the [`crate::AudioOut::with_spu`]
    /// mirror hosts stage VAB uploads and fire one-shot cues through.
    pub fn with_spu<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Spu) -> R,
    {
        f(&mut self.state.spu)
    }

    /// Install a sequencer immediately (mirror of
    /// [`crate::AudioOut::attach_sequencer`]).
    pub fn attach_sequencer(&mut self, seq: Sequencer) {
        self.state.attach_sequencer(seq);
    }

    /// Detach + key-off the active sequencer (mirror of
    /// [`crate::AudioOut::detach_sequencer`]).
    pub fn detach_sequencer(&mut self) {
        self.state.detach_sequencer();
    }

    /// Gate the sequencer clock without detaching it (mirror of
    /// [`crate::AudioOut::set_sequencer_paused`]). Sounding voices keep
    /// decaying through their ADSR envelopes, exactly as under the device.
    pub fn set_sequencer_paused(&mut self, paused: bool) {
        self.state.sequencer_paused = paused;
    }

    /// Whether the sequencer clock is currently gated.
    pub fn sequencer_paused(&self) -> bool {
        self.state.sequencer_paused
    }

    /// Cross-fade to `new_seq` (mirror of [`crate::AudioOut::crossfade_to`]).
    pub fn crossfade_to(&mut self, new_seq: Sequencer, fade_samples: u32) {
        self.state.crossfade_to(new_seq, fade_samples);
    }

    /// Hard-swap the BGM track (mirror of [`crate::AudioOut::swap_bgm`]).
    pub fn swap_bgm(&mut self, new_seq: Sequencer, fade_in_samples: u32) {
        self.state.swap_bgm(new_seq, fade_in_samples);
    }

    /// Sequencer progress snapshot, `None` when nothing is attached.
    pub fn sequencer_progress(&self) -> Option<SequencerProgress> {
        self.state.sequencer_progress()
    }

    /// Monaural downmix (the retail options screen's Stereo/Monaural).
    pub fn set_mono(&mut self, mono: bool) {
        self.state.mono = mono;
    }

    /// Master mute gate - output zeroes while everything keeps ticking.
    pub fn set_muted(&mut self, muted: bool) {
        self.state.muted = muted;
    }

    /// Pull one output frame - identical mixing math to the cpal callback.
    pub fn next_frame(&mut self) -> (i16, i16) {
        self.state.next_frame()
    }

    /// Pull `frames` output frames and report what they contained.
    pub fn render_frames(&mut self, frames: usize) -> SinkMeasure {
        let mut m = SinkMeasure::default();
        for _ in 0..frames {
            m.accumulate(self.state.next_frame());
        }
        m
    }

    /// Pull one 60 Hz video frame's worth of output.
    pub fn render_video_frame(&mut self) -> SinkMeasure {
        self.render_frames(self.frames_per_video_frame())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_sink_emits_silence_and_says_so() {
        let mut sink = TestAudioSink::new(crate::SPU_INTERNAL_RATE);
        let m = sink.render_video_frame();
        assert_eq!(m.frames, 735, "44100 / 60");
        assert!(m.is_silent(), "no source attached -> no output");
        assert_eq!(m.peak, 0);
        assert_eq!(m.mean_abs(), 0.0);
    }

    #[test]
    fn measure_separates_a_click_from_a_level() {
        let mut click = SinkMeasure::default();
        click.accumulate((8000, -8000));
        for _ in 0..7 {
            click.accumulate((0, 0));
        }
        let mut held = SinkMeasure::default();
        for _ in 0..8 {
            held.accumulate((4000, 4000));
        }
        assert!(click.peak > held.peak, "the click is louder at its peak");
        assert!(
            held.mean_abs() > click.mean_abs(),
            "but the held level integrates higher - which is what tells a \
             sounding track from one stray sample"
        );
        assert!(!click.is_silent() && !held.is_silent());
    }

    #[test]
    fn mute_zeroes_the_output_without_stopping_the_clock() {
        // The gate is on the emitted frame only: a muted sink still advances
        // the SPU, so unmuting resumes mid-stream rather than replaying.
        let mut sink = TestAudioSink::new(crate::SPU_INTERNAL_RATE);
        sink.set_muted(true);
        let m = sink.render_frames(64);
        assert!(m.is_silent());
        sink.set_muted(false);
        assert_eq!(sink.render_frames(1).frames, 1);
    }

    #[test]
    fn frames_per_video_frame_tracks_the_device_rate() {
        assert_eq!(TestAudioSink::new(44_100).frames_per_video_frame(), 735);
        assert_eq!(TestAudioSink::new(48_000).frames_per_video_frame(), 800);
        // Degenerate rates still yield a usable (non-zero) frame budget.
        assert_eq!(TestAudioSink::new(0).frames_per_video_frame(), 1);
    }
}

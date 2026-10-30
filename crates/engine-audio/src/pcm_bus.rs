//! The PCM bus: decoded clips mixed beside the SPU - an expansion pack's
//! own music tracks, stingers, sound effects and ambient loops.
//!
//! Retail has nothing here; the bus exists for content the disc does not
//! carry. It is mixed into the SPU-rate sum after the SPU's master fade (a
//! pack track fades itself) and before the host-rate resample, so mute,
//! master volume and the full downmix cover it like every other source.
//!
//! **Empty is the identity.** With no voice on the bus the mixer skips it
//! entirely, so a session with no pack renders bit-identical output - the
//! audio parity oracles never see it.
//!
//! Each voice reads its clip at the clip's own rate, stepping
//! `clip_rate / 44100` frames per SPU sample with linear interpolation, and
//! loops over `[loop_start, loop_end)` when it has a loop. Its level is the
//! product of a fixed gain (the pack's normalisation and `gain`), a
//! per-side pan pair that the host may move every frame (positional
//! ambient), a fade ramp, and the bus volume: the BGM option for
//! [`PcmBusKind::Music`], the SFX option for [`PcmBusKind::Sfx`]
//! ([`crate::Spu::bus_gain`]). Music also follows the battle duck.

use std::sync::Arc;

use crate::spu::voice::SPU_INTERNAL_RATE;

/// A decoded clip, shared between every voice that plays it.
#[derive(Debug, Clone, PartialEq)]
pub struct PcmClip {
    pub sample_rate: u32,
    /// `1` or `2`.
    pub channels: u8,
    /// Interleaved 16-bit samples.
    pub samples: Arc<[i16]>,
    /// `[start, end)` loop in frames; `None` plays once.
    pub loop_region: Option<(u32, u32)>,
    /// Fixed linear gain (normalisation times the pack's `gain`).
    pub gain: f32,
}

impl PcmClip {
    /// Sample frames.
    pub fn frames(&self) -> u32 {
        (self.samples.len() / usize::from(self.channels.max(1))) as u32
    }

    /// The frame `i` as a stereo pair (mono doubles), `(0, 0)` past the end.
    fn frame(&self, i: u32) -> (f32, f32) {
        let i = i as usize;
        if self.channels == 2 {
            match (self.samples.get(i * 2), self.samples.get(i * 2 + 1)) {
                (Some(&l), Some(&r)) => (f32::from(l), f32::from(r)),
                _ => (0.0, 0.0),
            }
        } else {
            let v = self.samples.get(i).map_or(0.0, |&v| f32::from(v));
            (v, v)
        }
    }
}

/// Which volume option a voice follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmBusKind {
    /// The BGM option and the battle duck.
    Music,
    /// The SFX option.
    Sfx,
}

/// What a voice is, for the host to find it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcmRole {
    /// The music slot: at most one live track, plus tracks fading out.
    Music,
    /// A one-shot sound.
    OneShot,
    /// A looping positional source, by the host's own key.
    Ambient(u32),
}

/// One clip playing.
#[derive(Debug, Clone)]
pub struct PcmVoice {
    clip: Arc<PcmClip>,
    role: PcmRole,
    kind: PcmBusKind,
    /// Read position in clip frames.
    cursor: f64,
    /// Clip frames per SPU sample.
    step: f64,
    /// Target pan pair `(left, right)`, `0.0..=1.0`.
    pan: (f32, f32),
    /// The pair as it sounds now, eased toward `pan` so a moving listener
    /// does not click.
    pan_now: (f32, f32),
    fade: f32,
    fade_target: f32,
    fade_step: f32,
    /// Remove the voice once a fade-out reaches silence.
    stop_at_silence: bool,
    paused: bool,
    done: bool,
}

/// The fraction of the way a voice's pan moves toward its target per SPU
/// sample: about 10 ms to settle.
const PAN_EASE: f32 = 1.0 / 441.0;

impl PcmVoice {
    fn new(clip: Arc<PcmClip>, role: PcmRole, kind: PcmBusKind, pan: (f32, f32)) -> Self {
        let step = f64::from(clip.sample_rate) / f64::from(SPU_INTERNAL_RATE);
        Self {
            clip,
            role,
            kind,
            cursor: 0.0,
            step,
            pan,
            pan_now: pan,
            fade: 1.0,
            fade_target: 1.0,
            fade_step: 0.0,
            stop_at_silence: false,
            paused: false,
            done: false,
        }
    }

    fn set_fade(&mut self, from: Option<f32>, to: f32, samples: u32) {
        if let Some(f) = from {
            self.fade = f;
        }
        self.fade_target = to;
        if samples == 0 {
            self.fade = to;
            self.fade_step = 0.0;
        } else {
            self.fade_step = (to - self.fade).abs() / samples as f32;
        }
    }

    /// One SPU-rate sample of this voice, before the bus volume.
    fn tick(&mut self) -> (f32, f32) {
        if self.done || self.paused {
            return (0.0, 0.0);
        }
        if self.fade_step > 0.0 {
            if self.fade < self.fade_target {
                self.fade = (self.fade + self.fade_step).min(self.fade_target);
            } else {
                self.fade = (self.fade - self.fade_step).max(self.fade_target);
            }
            if self.fade == self.fade_target {
                self.fade_step = 0.0;
            }
        }
        if self.stop_at_silence && self.fade <= 0.0 {
            self.done = true;
            return (0.0, 0.0);
        }
        self.pan_now.0 += (self.pan.0 - self.pan_now.0) * PAN_EASE;
        self.pan_now.1 += (self.pan.1 - self.pan_now.1) * PAN_EASE;
        let frames = self.clip.frames();
        let i = self.cursor.floor() as u32;
        let t = (self.cursor - self.cursor.floor()) as f32;
        let next = match self.clip.loop_region {
            Some((s, e)) if i + 1 >= e => s,
            _ => i + 1,
        };
        let a = self.clip.frame(i);
        let b = self.clip.frame(next);
        let (l, r) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        self.cursor += self.step;
        match self.clip.loop_region {
            Some((s, e)) if self.cursor >= f64::from(e) => {
                self.cursor = f64::from(s) + (self.cursor - f64::from(e));
            }
            None if self.cursor >= f64::from(frames) => self.done = true,
            _ => {}
        }
        let g = self.clip.gain * self.fade;
        (l * g * self.pan_now.0, r * g * self.pan_now.1)
    }
}

/// The bus: every live voice.
#[derive(Debug, Clone)]
pub struct PcmBus {
    voices: Vec<PcmVoice>,
    /// The battle duck's share of full volume, applied to music.
    music_duck: f32,
}

impl Default for PcmBus {
    fn default() -> Self {
        Self::new()
    }
}

impl PcmBus {
    pub fn new() -> Self {
        Self {
            voices: Vec::new(),
            music_duck: 1.0,
        }
    }

    /// No voice is live: the mixer skips the bus.
    pub fn is_empty(&self) -> bool {
        self.voices.is_empty()
    }

    /// Live voices.
    pub fn len(&self) -> usize {
        self.voices.len()
    }

    /// Start `clip` in the music slot, fading in over `fade_samples`; the
    /// track already there fades out over the same span (a cross-fade), or
    /// stops at once with no fade.
    pub fn play_music(&mut self, clip: Arc<PcmClip>, fade_samples: u32) {
        self.stop_music(fade_samples);
        let mut v = PcmVoice::new(clip, PcmRole::Music, PcmBusKind::Music, (1.0, 1.0));
        v.set_fade(
            Some(if fade_samples == 0 { 1.0 } else { 0.0 }),
            1.0,
            fade_samples,
        );
        self.voices.push(v);
    }

    /// Fade the music slot out over `fade_samples` (at once with `0`).
    pub fn stop_music(&mut self, fade_samples: u32) {
        for v in self.voices.iter_mut().filter(|v| v.role == PcmRole::Music) {
            if fade_samples == 0 {
                v.done = true;
            } else if !v.stop_at_silence {
                v.stop_at_silence = true;
                v.set_fade(None, 0.0, fade_samples);
            }
        }
        self.reap();
    }

    /// Whether a music track is live and not on its way out.
    pub fn music_playing(&self) -> bool {
        self.voices
            .iter()
            .any(|v| v.role == PcmRole::Music && !v.stop_at_silence && !v.done)
    }

    /// Hold or release the music slot (the BGM pause gate).
    pub fn set_music_paused(&mut self, paused: bool) {
        for v in self.voices.iter_mut().filter(|v| v.role == PcmRole::Music) {
            v.paused = paused;
        }
    }

    /// Put the music slot back on its first frame (the BGM re-attach).
    pub fn rewind_music(&mut self) {
        for v in self.voices.iter_mut().filter(|v| v.role == PcmRole::Music) {
            v.cursor = 0.0;
        }
    }

    /// The duck's share of full volume for music, `0.0..=1.0`.
    pub fn set_music_duck(&mut self, share: f32) {
        self.music_duck = share.clamp(0.0, 1.0);
    }

    /// Play `clip` once on the SFX bus at `pan`.
    pub fn play_once(&mut self, clip: Arc<PcmClip>, pan: (f32, f32)) {
        self.voices
            .push(PcmVoice::new(clip, PcmRole::OneShot, PcmBusKind::Sfx, pan));
    }

    /// Keep ambient source `key` sounding `clip` at `pan` (starting it,
    /// fading in, when it is not live). The clip should loop.
    pub fn set_ambient(&mut self, key: u32, clip: &Arc<PcmClip>, pan: (f32, f32)) {
        if let Some(v) = self
            .voices
            .iter_mut()
            .find(|v| v.role == PcmRole::Ambient(key) && !v.done)
        {
            v.pan = pan;
            if v.stop_at_silence {
                v.stop_at_silence = false;
                v.set_fade(None, 1.0, AMBIENT_FADE_SAMPLES);
            }
            return;
        }
        let mut v = PcmVoice::new(clip.clone(), PcmRole::Ambient(key), PcmBusKind::Sfx, pan);
        v.set_fade(Some(0.0), 1.0, AMBIENT_FADE_SAMPLES);
        self.voices.push(v);
    }

    /// Fade out every ambient source not in `keep`.
    pub fn retain_ambient(&mut self, keep: &[u32]) {
        for v in &mut self.voices {
            if let PcmRole::Ambient(k) = v.role
                && !keep.contains(&k)
                && !v.stop_at_silence
            {
                v.stop_at_silence = true;
                v.set_fade(None, 0.0, AMBIENT_FADE_SAMPLES);
            }
        }
    }

    /// Drop every voice at once.
    pub fn clear(&mut self) {
        self.voices.clear();
    }

    fn reap(&mut self) {
        self.voices.retain(|v| !v.done);
    }

    /// One SPU-rate sample of the bus at bus volumes `bus_gain` (Q1.14,
    /// `[music, sfx]`); `mono` sounds each voice at its mean on both sides.
    pub fn tick(&mut self, bus_gain: [u16; 2], mono: bool) -> (i32, i32) {
        let unity = f32::from(crate::spu::BUS_GAIN_UNITY);
        let music = f32::from(bus_gain[0]) / unity * self.music_duck;
        let sfx = f32::from(bus_gain[1]) / unity;
        let (mut l, mut r) = (0.0f32, 0.0f32);
        let mut any_done = false;
        for v in &mut self.voices {
            let (vl, vr) = v.tick();
            let g = match v.kind {
                PcmBusKind::Music => music,
                PcmBusKind::Sfx => sfx,
            };
            let (vl, vr) = if mono {
                let m = (vl + vr) * 0.5;
                (m, m)
            } else {
                (vl, vr)
            };
            l += vl * g;
            r += vr * g;
            any_done |= v.done;
        }
        if any_done {
            self.reap();
        }
        (l.round() as i32, r.round() as i32)
    }
}

/// The fade an ambient source takes to come in or go out: 100 ms.
pub const AMBIENT_FADE_SAMPLES: u32 = SPU_INTERNAL_RATE / 10;

/// Samples at the SPU rate for `frames` video frames (60 Hz).
pub fn frames_to_samples(frames: u16) -> u32 {
    u32::from(frames) * SPU_INTERNAL_RATE / 60
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(rate: u32, frames: usize, value: i16, loop_region: Option<(u32, u32)>) -> Arc<PcmClip> {
        Arc::new(PcmClip {
            sample_rate: rate,
            channels: 1,
            samples: vec![value; frames].into(),
            loop_region,
            gain: 1.0,
        })
    }

    const UNITY: [u16; 2] = [crate::spu::BUS_GAIN_UNITY; 2];

    #[test]
    fn a_one_shot_plays_its_length_and_leaves() {
        let mut b = PcmBus::new();
        b.play_once(clip(SPU_INTERNAL_RATE, 100, 1000, None), (1.0, 1.0));
        let mut n = 0;
        while !b.is_empty() {
            let (l, r) = b.tick(UNITY, false);
            if l != 0 {
                assert_eq!((l, r), (1000, 1000));
                n += 1;
            }
            assert!(n <= 100);
        }
        assert_eq!(n, 100);
    }

    #[test]
    fn a_looping_track_wraps_at_its_loop_end() {
        // A ramp 0..100 looping over [50, 100): after the first pass the
        // cursor never reads below 50.
        let samples: Vec<i16> = (0..100).collect();
        let c = Arc::new(PcmClip {
            sample_rate: SPU_INTERNAL_RATE,
            channels: 1,
            samples: samples.into(),
            loop_region: Some((50, 100)),
            gain: 1.0,
        });
        let mut b = PcmBus::new();
        b.play_music(c, 0);
        let out: Vec<i32> = (0..400).map(|_| b.tick(UNITY, false).0).collect();
        assert_eq!(&out[..100], &(0..100).collect::<Vec<_>>()[..]);
        assert!(out[100..].iter().all(|&v| (50..100).contains(&v)));
        assert_eq!(out[100], 50, "the wrap lands on loop_start");
        assert!(!b.is_empty(), "a loop never ends");
    }

    #[test]
    fn half_rate_clips_play_at_half_speed() {
        let mut b = PcmBus::new();
        b.play_once(clip(SPU_INTERNAL_RATE / 2, 50, 500, None), (1.0, 1.0));
        let mut n = 0;
        while !b.is_empty() && n < 1000 {
            b.tick(UNITY, false);
            n += 1;
        }
        assert_eq!(n, 100, "50 frames at half rate span 100 SPU samples");
    }

    #[test]
    fn music_cross_fades_and_the_old_track_leaves() {
        let mut b = PcmBus::new();
        b.play_music(clip(SPU_INTERNAL_RATE, 10_000, 1000, Some((0, 10_000))), 0);
        b.play_music(
            clip(SPU_INTERNAL_RATE, 10_000, 3000, Some((0, 10_000))),
            100,
        );
        assert_eq!(b.len(), 2);
        let first = b.tick(UNITY, false).0;
        assert!(
            first > 900 && first < 1100,
            "starts on the old track: {first}"
        );
        for _ in 0..200 {
            b.tick(UNITY, false);
        }
        assert_eq!(b.len(), 1, "the old track left at silence");
        assert_eq!(b.tick(UNITY, false).0, 3000);
    }

    #[test]
    fn the_bus_volumes_and_duck_scale_their_voices() {
        let mut b = PcmBus::new();
        b.play_music(clip(SPU_INTERNAL_RATE, 1000, 1000, Some((0, 1000))), 0);
        b.play_once(clip(SPU_INTERNAL_RATE, 1000, 1000, None), (1.0, 0.0));
        let half = crate::spu::BUS_GAIN_UNITY / 2;
        assert_eq!(b.tick([half, 0], false), (500, 500));
        assert_eq!(b.tick([0, half], false), (500, 0));
        b.set_music_duck(0.5);
        assert_eq!(b.tick([crate::spu::BUS_GAIN_UNITY, 0], false), (500, 500));
        assert_eq!(
            b.tick([0, crate::spu::BUS_GAIN_UNITY], true),
            (500, 500),
            "mono folds the pan"
        );
    }

    #[test]
    fn ambient_sources_follow_their_pan_and_fade_out_when_dropped() {
        let mut b = PcmBus::new();
        let c = clip(SPU_INTERNAL_RATE, 1000, 1000, Some((0, 1000)));
        b.set_ambient(7, &c, (1.0, 0.0));
        for _ in 0..AMBIENT_FADE_SAMPLES {
            b.tick(UNITY, false);
        }
        assert_eq!(b.tick(UNITY, false), (1000, 0));
        b.set_ambient(7, &c, (0.0, 1.0));
        for _ in 0..10_000 {
            b.tick(UNITY, false);
        }
        let (l, r) = b.tick(UNITY, false);
        assert!(l < 5 && r > 995, "eased to the new pan: {l} {r}");
        assert_eq!(b.len(), 1, "one voice per key");
        b.retain_ambient(&[]);
        for _ in 0..=AMBIENT_FADE_SAMPLES {
            b.tick(UNITY, false);
        }
        assert!(b.is_empty());
    }
}

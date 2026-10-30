//! An expansion pack's audio on the director: its own tracks, sounds and
//! ambient loops on the output's PCM bus (`legaia_engine_audio::pcm_bus`),
//! and the cross-fading starts its music zones ask for.
//!
//! A pack file is decoded once ([`legaia_expansion_format::audio_clip`],
//! the decoder the validator runs too), normalised and cached by
//! `(pack, file, loop)`. With no pack nothing here runs and the bus stays
//! empty, which the mixer skips - the retail output is untouched.

use super::*;

use legaia_engine_audio::PcmClip;
use legaia_engine_audio::pcm_bus::frames_to_samples;
use legaia_engine_expansion::audio::PackClip;

/// The click guard a retail start gives a pack track it cuts: two frames.
const CUT_FADE_SAMPLES: u32 = 1_470;

/// The director's pack-side state.
#[derive(Debug, Default)]
pub(super) struct PackAudioState {
    /// Decoded clips by `(pack, file, looped)`; `None` for a file that did
    /// not decode (logged once, not retried).
    clips: BTreeMap<(String, String, bool), Option<Arc<PcmClip>>>,
    /// A retail start waiting for the outgoing track's fade-out.
    deferred: Option<DeferredStart>,
    /// Ambient sources were sounding last frame.
    ambient_live: bool,
}

#[derive(Debug)]
struct DeferredStart {
    bgm_id: u16,
    entry: Vec<u8>,
    frames_left: u16,
}

/// A stable hash of a sound key, for the command record.
fn key_hash(key: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    h.finish()
}

impl<S: AudioSink> AudioDirector<S> {
    /// Decode (or fetch from the cache) `clip` as a bus clip; `looped`
    /// forces a loop (an ambient source), else a track loops and a sound
    /// plays once unless its `loop` says otherwise.
    fn pack_pcm(&mut self, clip: PackClip<'_>, track: bool, looped: bool) -> Option<Arc<PcmClip>> {
        let key = (clip.pack_id.to_string(), clip.def.file.clone(), looped);
        if let Some(c) = self.pack.clips.get(&key) {
            return c.clone();
        }
        let decoded = match legaia_expansion_format::audio_clip::decode(clip.bytes) {
            Ok(d) => {
                let mut def = clip.def.clone();
                if looped {
                    def.r#loop = Some(true);
                }
                let gain = def.gain * def.normalize_gain(d.rms());
                let loop_region = def.loop_region(d.frames(), d.loop_points, track);
                Some(Arc::new(PcmClip {
                    sample_rate: d.sample_rate,
                    channels: d.channels,
                    samples: d.samples.into(),
                    loop_region,
                    gain,
                }))
            }
            Err(e) => {
                log::warn!(
                    "expansion audio: assets/{} does not decode: {e:#}",
                    clip.def.file
                );
                None
            }
        };
        self.pack.clips.insert(key, decoded.clone());
        decoded
    }

    /// Start a pack track (see [`BgmDirector::start_pack_track`]): the SEQ
    /// in the slot fades out and leaves, the track already on the bus
    /// cross-fades, and the new one fades in over the same span.
    pub(super) fn start_pack_track_inner(
        &mut self,
        bgm_id: u16,
        clip: PackClip<'_>,
        fade_frames: u16,
    ) {
        self.commands
            .push(super::AudioCommand::PackTrack(bgm_id, fade_frames));
        if self.last_started == Some(bgm_id) && self.audio.with_pcm_bus(|b| b.music_playing()) {
            return;
        }
        let Some(pcm) = self.pack_pcm(clip, true, false) else {
            return;
        };
        self.pack.deferred = None;
        let samples = frames_to_samples(fade_frames);
        self.audio.fade_out_sequencer(samples);
        self.audio.set_sequencer_paused(false);
        self.audio.with_pcm_bus(|b| {
            b.set_music_paused(false);
            b.play_music(pcm, samples)
        });
        self.last_started = Some(bgm_id);
        log::info!("AudioDirector: pack track {bgm_id:#06x} started (fade {fade_frames})");
    }

    /// A retail start that cross-fades (see
    /// [`BgmDirector::start_owned_vab_faded`]). A pack track on the bus
    /// fades out under the new score; a score in the slot fades out first
    /// and the new one starts once it is silent, since the incoming bank is
    /// uploaded over the outgoing one's samples.
    pub(super) fn start_owned_vab_faded_inner(
        &mut self,
        bgm_id: u16,
        entry: &[u8],
        fade_frames: u16,
    ) {
        self.commands
            .push(super::AudioCommand::BgmFade(bgm_id, fade_frames));
        if self.last_started == Some(bgm_id) && self.is_playing() {
            return;
        }
        let samples = frames_to_samples(fade_frames);
        if fade_frames > 0 && self.is_playing() {
            self.audio.fade_out_sequencer(samples);
            self.last_started = None;
            self.pack.deferred = Some(DeferredStart {
                bgm_id,
                entry: entry.to_vec(),
                frames_left: fade_frames,
            });
            return;
        }
        self.audio.with_pcm_bus(|b| b.stop_music(samples));
        BgmDirector::start_owned_vab(self, bgm_id, entry);
    }

    /// One frame of the deferred start: once the outgoing score's fade has
    /// run, the incoming one starts.
    pub(super) fn tick_pack_deferred(&mut self) {
        let Some(d) = self.pack.deferred.as_mut() else {
            return;
        };
        d.frames_left = d.frames_left.saturating_sub(1);
        if d.frames_left > 0 {
            return;
        }
        let d = self.pack.deferred.take().expect("checked above");
        BgmDirector::start_owned_vab(self, d.bgm_id, &d.entry);
    }

    /// A retail start, stop or pause takes the music slot from a pack track:
    /// it leaves with a click guard, and a deferred start is dropped.
    pub(super) fn release_pack_music(&mut self) {
        self.pack.deferred = None;
        self.audio.with_pcm_bus(|b| b.stop_music(CUT_FADE_SAMPLES));
    }

    /// The battle duck reaches the bus's music too.
    pub(super) fn duck_pack_music(&mut self) {
        let share = f32::from(self.duck_applied) / f32::from(self.duck_ref.max(1));
        self.audio.with_pcm_bus(|b| b.set_music_duck(share));
    }

    /// One frame of the pack's sounds: play what the world raised and keep
    /// each ambient source at the level the world computed (fading out the
    /// ones it no longer lists). The identity with no pack.
    pub fn drive_pack_audio(&mut self, world: &mut legaia_engine_core::world::World) {
        let sounds = world.take_expansion_sounds();
        let Some(pack) = world.expansion.pack.clone() else {
            if self.pack.ambient_live {
                self.pack.ambient_live = false;
                self.audio.with_pcm_bus(|b| b.retain_ambient(&[]));
            }
            return;
        };
        for s in sounds {
            self.commands
                .push(super::AudioCommand::PackSound(key_hash(&s.key)));
            let Some(clip) = legaia_engine_expansion::audio::sound_clip(&pack, &s.key) else {
                continue;
            };
            if let Some(pcm) = self.pack_pcm(clip, false, false) {
                self.audio.with_pcm_bus(|b| b.play_once(pcm, s.pan));
            }
        }
        let levels = world.expansion_ambient_levels().to_vec();
        if levels.is_empty() && !self.pack.ambient_live {
            return;
        }
        let mut keep = Vec::with_capacity(levels.len());
        for l in &levels {
            let Some(clip) = legaia_engine_expansion::audio::sound_clip(&pack, &l.sound) else {
                continue;
            };
            if let Some(pcm) = self.pack_pcm(clip, false, true) {
                self.audio
                    .with_pcm_bus(|b| b.set_ambient(l.key, &pcm, l.pan));
                keep.push(l.key);
            }
        }
        self.audio.with_pcm_bus(|b| b.retain_ambient(&keep));
        self.pack.ambient_live = !keep.is_empty();
    }

    /// Whether the bus holds a live pack track (for tests and traces).
    pub fn pack_music_playing(&self) -> bool {
        self.audio.with_pcm_bus(|b| b.music_playing())
    }

    /// Voices live on the PCM bus (for tests and traces).
    pub fn pack_voices(&self) -> usize {
        self.audio.with_pcm_bus(|b| b.len())
    }

    /// Whether a retail start waits on a fade-out.
    pub fn pack_start_deferred(&self) -> bool {
        self.pack.deferred.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_audio::TestAudioSink;
    use legaia_expansion_format::audio::ClipDef;
    use legaia_expansion_format::audio_clip::encode_wav;

    // Single-threaded like both hosts; the director takes its sink by `Arc`.
    #[allow(clippy::arc_with_non_send_sync)]
    fn headless() -> AudioDirector<TestAudioSink> {
        AudioDirector::new(Arc::new(TestAudioSink::new(
            legaia_engine_audio::SPU_INTERNAL_RATE,
        )))
    }

    fn def(file: &str) -> ClipDef {
        ClipDef {
            file: file.into(),
            gain: 1.0,
            normalize: true,
            r#loop: None,
            loop_start: None,
            loop_end: None,
        }
    }

    /// A 440 Hz tone, `frames` long, at `amp` of full scale.
    fn tone(rate: u32, frames: usize, amp: f32) -> Vec<u8> {
        let pcm: Vec<i16> = (0..frames)
            .map(|i| {
                let t = i as f32 / rate as f32;
                ((t * 440.0 * std::f32::consts::TAU).sin() * amp * 32767.0) as i16
            })
            .collect();
        encode_wav(rate, 1, &pcm, None)
    }

    fn clip<'a>(bytes: &'a [u8], def: &'a ClipDef) -> PackClip<'a> {
        PackClip {
            pack_id: "p",
            bytes,
            def,
        }
    }

    #[test]
    fn a_pack_track_loops_at_the_reference_level() {
        let mut d = headless();
        // Half a second of a quiet tone (RMS ~0.035): normalisation lifts it
        // toward the reference, and the loop keeps it going past its end.
        let wav = tone(22050, 11025, 0.05);
        let def = def("a.wav");
        d.start_pack_track(0xE000, clip(&wav, &def), 0);
        assert!(d.pack_music_playing());
        let m = d.audio().render_frames(44100 * 2);
        // Never silent past a zero crossing: the loop has no gap.
        assert!(
            m.nonzero * 1000 >= m.frames * 999,
            "two seconds of a half-second loop: {m:?}"
        );
        // A sine's mean |x| is 0.9 of its RMS: the mixed level sits at the
        // reference RMS (`REFERENCE_RMS` of full scale) within a few percent.
        let reference = legaia_expansion_format::audio::REFERENCE_RMS as f64 * 32768.0 * 0.9;
        let ratio = m.mean_abs() / reference;
        assert!(
            (0.9..1.1).contains(&ratio),
            "level {} vs {reference}",
            m.mean_abs()
        );
        // A re-emitted start of the same track keeps its playhead.
        d.start_pack_track(0xE000, clip(&wav, &def), 0);
        assert_eq!(d.pack_voices(), 1);
    }

    #[test]
    fn tracks_cross_fade_and_a_retail_stop_takes_the_slot() {
        let mut d = headless();
        let a = tone(22050, 22050, 0.2);
        let b = tone(44100, 44100, 0.2);
        let (da, db) = (def("a.wav"), def("b.wav"));
        d.start_pack_track(0xE000, clip(&a, &da), 0);
        d.audio().render_frames(4410);
        d.start_pack_track(0xE001, clip(&b, &db), 30);
        assert_eq!(d.pack_voices(), 2, "both sound across the fade");
        d.audio().render_frames(44100);
        assert_eq!(d.pack_voices(), 1, "the old track left at the fade's end");
        assert_eq!(d.last_started, Some(0xE001));
        BgmDirector::stop(&mut d);
        d.audio().render_frames(4410);
        assert_eq!(d.pack_voices(), 0);
        assert!(d.audio().render_frames(4410).is_silent());
    }

    #[test]
    fn pause_holds_the_pack_track_and_resume_replays_it() {
        let mut d = headless();
        let a = tone(22050, 22050, 0.2);
        let da = def("a.wav");
        d.start_pack_track(0xE000, clip(&a, &da), 0);
        BgmDirector::pause(&mut d);
        assert!(d.audio().render_frames(4410).is_silent());
        BgmDirector::resume(&mut d);
        assert!(!d.audio().render_frames(4410).is_silent());
    }

    #[test]
    fn a_file_that_does_not_decode_is_skipped_once() {
        let mut d = headless();
        let da = def("bad.wav");
        d.start_pack_track(0xE000, clip(b"RIFF\0\0\0\0WAVEjunk", &da), 0);
        assert!(!d.pack_music_playing());
        assert_eq!(d.pack.clips.len(), 1, "remembered as failed");
    }

    #[test]
    fn a_world_with_no_pack_leaves_the_bus_empty() {
        let mut d = headless();
        let mut w = legaia_engine_core::world::World::default();
        for _ in 0..10 {
            d.drive_pack_audio(&mut w);
        }
        assert_eq!(d.pack_voices(), 0);
        assert!(d.audio().render_frames(4410).is_silent());
    }
}

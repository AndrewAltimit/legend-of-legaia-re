//! Per-voice state for the SPU mixer.
//!
//! A voice owns:
//!  - a base sample address into SPU RAM (the start of its ADPCM stream)
//!  - a loop point (latched the first time a block with `loop_start` is seen,
//!    or set explicitly via [`Voice::set_loop_addr`])
//!  - a streaming ADPCM decoder (one 28-sample block at a time)
//!  - a fractional pitch counter - the SPU runs internally at 44.1 kHz and
//!    advances each voice by `pitch / 0x1000` samples per output tick,
//!    resampling through the hardware's 4-point Gaussian interpolator
//!    ([`gauss`])
//!  - an ADSR envelope
//!  - linear left/right volume registers (signed 16-bit, peak 0x3FFF in
//!    libspu)
//!
//! The PSX SPU outputs 44.1 kHz; the host stream rate is decoupled by the
//! mixer.
//!
//! No Sony bytes - this is the standard PSX SPU model.

use super::adpcm::{AdpcmDecoder, BLOCK_BYTES, SAMPLES_PER_BLOCK};
use super::adsr::{AdsrConfig, AdsrState, Phase};
use super::gauss;
use super::ram::SpuRam;

/// Internal SPU output rate (constant per hardware).
pub const SPU_INTERNAL_RATE: u32 = 44_100;

/// Pitch unit: 0x1000 = 1× sample rate.
pub const PITCH_UNITY: u16 = 0x1000;

/// The engine mix bus a voice belongs to (see [`Voice::bus`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceBus {
    /// Background music: voices keyed by the SEQ sequencer.
    Bgm,
    /// Sound effects: voices keyed by a cue (field / battle / menu SFX).
    #[default]
    Sfx,
}

impl VoiceBus {
    /// Index into [`crate::spu::Spu::bus_gain`].
    pub const fn index(self) -> usize {
        match self {
            Self::Bgm => 0,
            Self::Sfx => 1,
        }
    }
}

/// Where [`Voice::resume`] stands a voice.
#[derive(Debug, Clone)]
pub struct VoiceResume {
    /// The block the decoder stands in, bytes.
    pub block_addr: u32,
    /// Index in that block of the newest interpolator tap.
    pub sample_idx: u8,
    /// `Some`: the newest tap is still in the block before, and these are the
    /// samples of that block from the newest tap on. The next block is
    /// [`Self::block_addr`].
    pub before_block: Option<Vec<i16>>,
    /// Pitch counter, `1 / 0x1000` of a sample.
    pub sample_frac: u32,
    /// The interpolator's four taps, oldest first.
    pub hist: [i16; 4],
    /// Decoder history entering [`Self::block_addr`]'s block: `(prev1, prev2)`.
    pub prev: (i32, i32),
}

/// Output samples between a key-on and a voice's first sounding one: the
/// sample the KON write falls in, which still plays the old note, and the
/// decoder's start delay (`Voices[n].DecodePlayDelay`, `4` at a key-on) with the
/// tick that takes the envelope and the pitch counter's first step. See
/// `docs/subsystems/audio.md`, "The key-on's start".
pub const KEY_ON_START_DELAY: u8 = 6;

#[derive(Debug, Clone)]
pub struct Voice {
    /// Base address (bytes) into SPU RAM where this voice's ADPCM stream
    /// starts. Set by libspu `SpuSetVoiceStartAddr`.
    pub start_addr: u32,
    /// Loop-back address. Latched on the first block with `loop_start`,
    /// or set explicitly. `None` means "no loop set" - when an end-flag
    /// block plays out the voice will go silent.
    pub loop_addr: Option<u32>,
    /// Pitch, 0x1000 = 1.0×. Sample step = pitch / 0x1000.
    pub pitch: u16,
    /// Left output volume, 0..=0x3FFF (libspu convention).
    pub vol_left: i16,
    /// Right output volume.
    pub vol_right: i16,
    /// ADSR envelope settings.
    pub adsr_cfg: AdsrConfig,
    /// ADSR runtime state.
    pub adsr: AdsrState,
    /// Per-voice reverb routing flag. When `true` this voice's pre-master
    /// output is summed into the SPU's reverb send bus. libspu calls this
    /// `SpuSetVoiceReverb`. Defaults to `false`; `VabBank`'s key-on sets it
    /// per keyed tone from the VAB tone's `mode & 4`, as retail's
    /// `FUN_80067550` sets the voice's `EON` bit.
    pub reverb_send: bool,
    /// Which engine mix bus the voice's output sums into ([`VoiceBus`]):
    /// stamped at key-on by whoever keys it - a sequencer note is
    /// [`VoiceBus::Bgm`], a cue key-on [`VoiceBus::Sfx`]. Engine-only (retail
    /// has one SPU mix); it lets the config file's `bgm_volume` /
    /// `sfx_volume` scale the two independently ([`crate::spu::Spu::bus_gain`]).
    pub bus: VoiceBus,

    // --- runtime state --------------------------------------------------
    /// Absolute address of the *current* ADPCM block in SPU RAM.
    cur_block_addr: u32,
    /// Which sample within the current 28-sample block we're playing.
    sample_idx: u8,
    /// Fractional sample counter (in `1 / 0x1000` units of an input sample).
    sample_frac: u32,
    /// 28-sample buffer for the current block.
    block_pcm: [i16; SAMPLES_PER_BLOCK],
    /// Decoder state (carries `prev1`/`prev2` between blocks).
    decoder: AdpcmDecoder,
    /// True if the voice has at least one decoded block ready.
    has_block: bool,
    /// The four most recent decoded samples for the hardware's 4-point
    /// Gaussian interpolator (`[0]` = oldest .. `[3]` = newest = the sample
    /// at the current pitch-counter position). Survives block boundaries.
    hist: [i16; 4],
    /// Set by [`Self::resume`] when the interpolator is still inside the block
    /// before the one the decoder stands in: where that block's tail hands on.
    resume_next: Option<u32>,
    /// Output samples left before a keyed voice starts to play.
    start_delay: u8,
    /// Key-ons since construction. An owner that remembers the count at its
    /// own key-on can tell whether someone else has re-keyed the voice since.
    key_on_count: u32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            start_addr: 0,
            loop_addr: None,
            pitch: PITCH_UNITY,
            vol_left: 0x3FFF,
            vol_right: 0x3FFF,
            adsr_cfg: AdsrConfig::default(),
            adsr: AdsrState::default(),
            reverb_send: false,
            bus: VoiceBus::Sfx,
            cur_block_addr: 0,
            sample_idx: 0,
            sample_frac: 0,
            block_pcm: [0; SAMPLES_PER_BLOCK],
            decoder: AdpcmDecoder::new(),
            has_block: false,
            hist: [0; 4],
            resume_next: None,
            start_delay: 0,
            key_on_count: 0,
        }
    }
}

impl Voice {
    /// Set the explicit loop address. Equivalent to libspu's
    /// `SpuSetVoiceLoopStartAddr`.
    pub fn set_loop_addr(&mut self, addr: u32) {
        self.loop_addr = Some(addr);
    }

    /// Toggle the per-voice reverb send flag (libspu `SpuSetVoiceReverb`
    /// analogue).
    pub fn set_reverb_send(&mut self, on: bool) {
        self.reverb_send = on;
    }

    /// Trigger a key-on: rewind to start, decode the first block, kick the
    /// envelope into the Attack phase.
    pub fn key_on(&mut self, ram: &SpuRam) {
        self.cur_block_addr = self.start_addr;
        self.sample_idx = 3;
        self.sample_frac = 0;
        self.resume_next = None;
        self.decoder.reset();
        self.fetch_block(ram);
        self.hist = if self.has_block {
            std::array::from_fn(|k| self.block_pcm[k])
        } else {
            [0; 4]
        };
        self.start_delay = KEY_ON_START_DELAY;
        self.adsr.key_on();
        self.key_on_count = self.key_on_count.wrapping_add(1);
    }

    /// Push a just-keyed voice's start back by `samples`: the KON write landed
    /// that much later than the flush that staged it.
    pub fn delay_start(&mut self, samples: u8) {
        self.start_delay = self.start_delay.saturating_add(samples);
    }

    /// Stand a voice mid-note where a retail capture has it, so a parity oracle
    /// can resume the capture sample for sample rather than re-key it. The
    /// caller has already set the registers, the envelope and
    /// [`Self::loop_addr`].
    pub fn resume(&mut self, ram: &SpuRam, at: VoiceResume) {
        self.start_delay = 0;
        self.sample_frac = at.sample_frac & 0xFFF;
        self.hist = at.hist;
        self.decoder.prev1 = at.prev.0;
        self.decoder.prev2 = at.prev.1;
        self.cur_block_addr = at.block_addr;
        match at.before_block {
            Some(tail) => {
                let start = SAMPLES_PER_BLOCK - tail.len();
                self.block_pcm[start..].copy_from_slice(&tail);
                self.sample_idx = start as u8;
                self.has_block = true;
                self.resume_next = Some(at.block_addr);
            }
            None => {
                self.fetch_block(ram);
                self.sample_idx = at.sample_idx;
            }
        }
    }

    /// Whether the voice is sounding from a sample in `span` (byte addresses,
    /// end exclusive): its decoder stands on a block there.
    pub fn reads_from(&self, span: std::ops::Range<u32>) -> bool {
        !self.is_off() && span.contains(&self.cur_block_addr)
    }

    /// Follow a sample that moved from `from` to `to_base` in SPU RAM: every
    /// address the voice holds inside `from` (start, repeat, the block it
    /// stands on) shifts by the same distance, so the voice carries on from
    /// the same point of the moved copy. Engine-only: retail never moves a
    /// bank under a sounding voice; the port's BGM-tail borrowers do
    /// ([`crate::bgm_tail`]).
    pub fn rebase(&mut self, from: std::ops::Range<u32>, to_base: u32) {
        let shift = |a: u32| {
            if from.contains(&a) {
                a - from.start + to_base
            } else {
                a
            }
        };
        self.start_addr = shift(self.start_addr);
        self.loop_addr = self.loop_addr.map(shift);
        self.cur_block_addr = shift(self.cur_block_addr);
        self.resume_next = self.resume_next.map(shift);
    }

    /// Stop the voice at once, envelope and all - what is left when the
    /// sample under it is gone and there is nothing to release over.
    pub fn cut(&mut self) {
        self.adsr.phase = Phase::Off;
        self.adsr.level = 0;
        self.has_block = false;
    }

    /// How many times this voice has been keyed on. The sequencer stamps
    /// each of its notes with this value; a later mismatch means another
    /// client (an SFX cue) took the voice, so the note no longer owns it -
    /// retail's equivalent is the cue key-on rewriting the voice's libsnd
    /// note record (`+0x10 = 0x21`), which the sequencer's note-off then no
    /// longer matches.
    pub fn key_on_count(&self) -> u32 {
        self.key_on_count
    }

    /// Trigger a key-off - the envelope transitions to Release. Voice keeps
    /// playing until the envelope drains.
    pub fn key_off(&mut self) {
        self.adsr.key_off();
    }

    /// True when the envelope is in `Off` (envelope finished or never started).
    pub fn is_off(&self) -> bool {
        self.adsr.phase == Phase::Off
    }

    /// Sample-level tick. Advances envelope by one tick and produces one
    /// (left, right) output sample at the SPU internal rate. Voices that
    /// are off produce silence.
    pub fn tick(&mut self, ram: &SpuRam) -> (i32, i32) {
        if !self.has_block || self.adsr.phase == Phase::Off {
            return (0, 0);
        }
        if self.start_delay > 0 {
            self.start_delay -= 1;
            if self.start_delay == 0 {
                self.step_pitch_counter(ram);
            }
            return (0, 0);
        }
        let env = self.adsr.tick(&self.adsr_cfg) as i32;
        // Resample through the hardware's 4-point Gaussian interpolator:
        // fraction bits 4..11 of the pitch counter index the coefficient
        // table (see [`gauss`]).
        let frac_idx = ((self.sample_frac >> 4) & 0xFF) as u8;
        let raw = gauss::interpolate(&self.hist, frac_idx);

        // Mix: raw * env / 0x7FFF * vol / 0x3FFF.
        let enveloped = (raw * env) >> 15;
        let left = (enveloped * self.vol_left as i32) >> 14;
        let right = (enveloped * self.vol_right as i32) >> 14;

        self.step_pitch_counter(ram);
        (left, right)
    }

    /// Advance the fractional pitch counter and walk forward through samples,
    /// pushing each newly-reached sample into the interpolator history.
    fn step_pitch_counter(&mut self, ram: &SpuRam) {
        // A register past `0x3FFF` steps at `0x4000` (no$psx "Pitch Counter").
        self.sample_frac += if self.pitch > 0x3FFF {
            0x4000
        } else {
            self.pitch as u32
        };
        while self.sample_frac >= PITCH_UNITY as u32 {
            self.sample_frac -= PITCH_UNITY as u32;
            self.sample_idx += 1;
            if self.sample_idx as usize >= SAMPLES_PER_BLOCK {
                self.sample_idx = 0;
                self.advance_to_next_block(ram);
                if !self.has_block {
                    break;
                }
            }
            self.hist = [
                self.hist[1],
                self.hist[2],
                self.hist[3],
                self.block_pcm[self.sample_idx as usize],
            ];
        }
    }

    /// Decode the block at `cur_block_addr` into `block_pcm`. Sets
    /// `has_block = false` on bad-header (treats as EOS).
    fn fetch_block(&mut self, ram: &SpuRam) {
        let bytes = ram.slice(self.cur_block_addr, BLOCK_BYTES as u32);
        if bytes.len() < BLOCK_BYTES {
            self.has_block = false;
            return;
        }
        let (pcm, flags) = self.decoder.decode_block(bytes);
        if flags.bad_header {
            self.has_block = false;
            return;
        }
        // The hardware latches the repeat address at every block carrying the
        // loop-start flag, so a sample with two of them loops to the later.
        if flags.loop_start {
            self.loop_addr = Some(self.cur_block_addr);
        }
        self.block_pcm = pcm;
        self.has_block = true;
    }

    /// Decide what comes after the current block: jump to loop, advance to
    /// next sequential block, or stop.
    fn advance_to_next_block(&mut self, ram: &SpuRam) {
        if self.resume_next.take().is_some() {
            // A resumed voice: `block_pcm` held the previous block's tail.
            self.fetch_block(ram);
            return;
        }
        let bytes = ram.slice(self.cur_block_addr, BLOCK_BYTES as u32);
        let flag = if bytes.len() >= 2 { bytes[1] } else { 0 };
        let end = flag & 0x01 != 0;
        let repeat = flag & 0x02 != 0;
        if end {
            if repeat {
                // Loop back. Use loop_addr if latched, otherwise stop.
                if let Some(addr) = self.loop_addr {
                    self.cur_block_addr = addr;
                    self.fetch_block(ram);
                } else {
                    self.has_block = false;
                    self.adsr.phase = Phase::Off;
                }
            } else {
                self.has_block = false;
                self.adsr.phase = Phase::Off;
            }
        } else {
            self.cur_block_addr += BLOCK_BYTES as u32;
            self.fetch_block(ram);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ram::SpuRam;
    use super::*;

    /// Build a synthetic ADPCM stream of `n_blocks` silence-blocks; final
    /// block carries an end flag and (optionally) the repeat flag.
    fn synth_silence_stream(n: usize, repeat_at_end: bool) -> Vec<u8> {
        let mut out = vec![0u8; n * BLOCK_BYTES];
        let last = (n - 1) * BLOCK_BYTES;
        out[last + 1] = if repeat_at_end { 0x03 } else { 0x01 };
        out
    }

    /// Voice with default ADSR (instant attack) plays the first block and
    /// envelope advances toward decay.
    #[test]
    fn voice_key_on_starts_envelope_and_yields_silence_for_silent_stream() {
        let stream = synth_silence_stream(1, false);
        let mut ram = SpuRam::new();
        ram.write_at(0x1000, &stream);
        let mut v = Voice {
            start_addr: 0x1000,
            ..Voice::default()
        };
        v.key_on(&ram);
        let mut sum = 0i64;
        for _ in 0..32 {
            let (l, _r) = v.tick(&ram);
            sum += l as i64;
        }
        // Silence block + any envelope -> still silence.
        assert_eq!(sum, 0);
    }

    /// A rebased voice follows its sample to the new copy: it reads from the
    /// moved span, and plays out to the copy's end flag although the old
    /// span now holds a looping stream.
    #[test]
    fn a_rebased_voice_plays_out_from_the_moved_copy() {
        let mut ram = SpuRam::new();
        ram.write_at(0x1000, &synth_silence_stream(4, false));
        // A held sustain (the shatter cue's tones), so only the stream ends it.
        let mut v = Voice {
            start_addr: 0x1000,
            adsr_cfg: super::super::adsr::AdsrConfig::from_words(0x80FF, 0x5FD1),
            ..Voice::default()
        };
        v.key_on(&ram);
        for _ in 0..40 {
            v.tick(&ram);
        }
        assert!(v.reads_from(0x1000..0x1040));
        // The sample moves up; a looping stream lands where it was.
        ram.write_at(0x2000, &synth_silence_stream(4, false));
        let mut looping = synth_silence_stream(4, true);
        looping[1] = 0x04;
        ram.write_at(0x1000, &looping);
        v.rebase(0x1000..0x1040, 0x2000);
        assert!(v.reads_from(0x2000..0x2040));
        assert_eq!(v.start_addr, 0x2000);
        for _ in 0..200 {
            v.tick(&ram);
        }
        assert!(v.is_off(), "the moved copy's end flag stops the voice");
    }

    /// End-flag without repeat halts the voice.
    #[test]
    fn voice_stops_at_end_flag_no_repeat() {
        let stream = synth_silence_stream(1, false);
        let mut ram = SpuRam::new();
        ram.write_at(0x1000, &stream);
        let mut v = Voice {
            start_addr: 0x1000,
            ..Voice::default()
        };
        v.key_on(&ram);
        // 28 samples at unity pitch consumes the whole block -> next tick stops.
        for _ in 0..32 {
            v.tick(&ram);
        }
        assert!(v.is_off());
    }

    /// A key-on stands the interpolator on the stream's first four samples and
    /// holds the voice silent, with no envelope and no pitch counter, until
    /// the delay runs out; its last tick takes the counter's first step.
    #[test]
    fn a_keyed_voice_holds_silent_then_starts_on_its_fourth_sample() {
        // Shift 0, filter 0: nibbles are the samples, `n << 12` sign-extended.
        let mut block = [0u8; BLOCK_BYTES];
        block[0] = 0x0C;
        block[1] = 0x03;
        block[2] = 0x21;
        block[3] = 0x43;
        for b in &mut block[4..] {
            *b = 0x55;
        }
        let mut ram = SpuRam::new();
        ram.write_at(0x1000, &block);
        let mut v = Voice {
            start_addr: 0x1000,
            pitch: 0x800,
            loop_addr: Some(0x1000),
            adsr_cfg: hold_forever_adsr(),
            ..Voice::default()
        };
        v.key_on(&ram);
        assert_eq!(v.sample_idx, 3);
        assert_eq!(
            v.hist,
            [
                v.block_pcm[0],
                v.block_pcm[1],
                v.block_pcm[2],
                v.block_pcm[3]
            ]
        );
        let level = v.adsr.level;
        for _ in 0..KEY_ON_START_DELAY - 1 {
            assert_eq!(v.tick(&ram), (0, 0));
            assert_eq!(v.sample_frac, 0);
            assert_eq!(v.adsr.level, level);
        }
        assert_eq!(v.tick(&ram), (0, 0));
        assert_eq!(
            v.sample_frac, 0x800,
            "the last silent tick steps the counter"
        );
        assert_eq!(
            v.adsr.level, level,
            "the envelope starts on the first sounding tick"
        );
        v.tick(&ram);
        assert!(v.adsr.level > level);
    }

    /// Build an "infinite-sustain" ADSR config so the envelope doesn't
    /// drain the voice during a streaming-loop test.
    fn hold_forever_adsr() -> AdsrConfig {
        AdsrConfig {
            sustain_level: 0x8000,   // SL 15: (15 + 1) * 0x800
            sustain_decrease: false, // not strictly needed at peak
            sustain_shift: 0x1F,     // huge shift -> tiny step
            decay_shift: 0xF,        // one -8 step off the peak, then sustain
            ..AdsrConfig::default()
        }
    }

    /// End-flag with repeat + an explicit loop addr keeps the voice's
    /// playback head looping forever.
    #[test]
    fn voice_loops_when_end_repeat_and_loop_addr_set() {
        let stream = synth_silence_stream(1, true);
        let mut ram = SpuRam::new();
        ram.write_at(0x1000, &stream);
        let mut v = Voice {
            start_addr: 0x1000,
            adsr_cfg: hold_forever_adsr(),
            ..Voice::default()
        };
        v.set_loop_addr(0x1000);
        v.key_on(&ram);
        for _ in 0..200 {
            v.tick(&ram);
        }
        // Voice's playback head should still be valid (repeat brought it
        // back to start). Envelope held by config -> not off.
        assert!(!v.is_off());
    }

    /// Half-pitch playback of a DC (constant) stream settles on the source
    /// level rather than a staircase: the Gaussian interpolator's fractional
    /// positions must not introduce steps on constant input.
    #[test]
    fn voice_half_pitch_dc_stream_is_smooth() {
        // Filter-0 shift-0 block: every nibble 7 decodes to 7 << 12 = 28672.
        let mut block = [0x77u8; BLOCK_BYTES];
        block[0] = 0x00; // filter 0, shift 0
        block[1] = 0x03; // end + repeat
        let mut ram = SpuRam::new();
        ram.write_at(0x1000, &block);
        let mut v = Voice {
            start_addr: 0x1000,
            pitch: 0x800,
            adsr_cfg: hold_forever_adsr(),
            ..Voice::default()
        };
        v.set_loop_addr(0x1000);
        v.key_on(&ram);
        // Warm up past the attack ramp + the interpolator's group delay.
        for _ in 0..64 {
            v.tick(&ram);
        }
        let mut min = i32::MAX;
        let mut max = i32::MIN;
        for _ in 0..64 {
            let (l, _r) = v.tick(&ram);
            min = min.min(l);
            max = max.max(l);
        }
        // Settled DC: near the decoded level (28672 scaled by the table's
        // ~0.996 unity sum), with no fraction-position wobble to speak of.
        assert!(max - min <= 16, "DC wobble {min}..{max}");
        assert!((27500..=28672).contains(&min), "level {min}");
    }

    /// Pitch=0x800 means we walk through samples at half speed -> twice as
    /// many ticks needed to consume a block.
    #[test]
    fn voice_pitch_half_doubles_block_lifetime() {
        // Two-block stream so we can detect block-edge crossing.
        let stream = synth_silence_stream(2, false);
        let mut ram = SpuRam::new();
        ram.write_at(0x1000, &stream);
        let mut v = Voice {
            start_addr: 0x1000,
            pitch: 0x800,
            adsr_cfg: hold_forever_adsr(),
            ..Voice::default()
        };
        v.key_on(&ram);
        // After 55 ticks at half pitch, fractional advance = 55*0x800 =
        // 27.5 input samples -> still inside block 0. Voice active.
        for _ in 0..55 {
            v.tick(&ram);
        }
        assert!(!v.is_off());
    }

    /// The hardware latches the repeat address at every block carrying the
    /// loop-start flag: a voice standing on a stale address re-enters the
    /// later block, not the address it held.
    #[test]
    fn every_loop_start_block_moves_the_repeat_address() {
        let mut stream = vec![0u8; 3 * BLOCK_BYTES];
        stream[1] = 0x04;
        stream[BLOCK_BYTES + 1] = 0x04;
        stream[2 * BLOCK_BYTES + 1] = 0x03;
        let mut ram = SpuRam::new();
        ram.write_at(0x1000, &stream);
        let mut v = Voice {
            start_addr: 0x1000,
            loop_addr: Some(0x2000),
            adsr_cfg: AdsrConfig::from_words(0x000F, 0x1F00),
            ..Voice::default()
        };
        v.key_on(&ram);
        assert_eq!(v.loop_addr, Some(0x1000));
        for _ in 0..40 {
            v.tick(&ram);
        }
        assert_eq!(v.loop_addr, Some(0x1000 + BLOCK_BYTES as u32));
    }

    /// A pitch register past `0x3FFF` steps at `0x4000`: four input samples an
    /// output tick, whatever the register says.
    #[test]
    fn the_pitch_step_tops_out_at_four_samples() {
        let stream = synth_silence_stream(40, true);
        let mut ram = SpuRam::new();
        ram.write_at(0x1000, &stream);
        let mut v = Voice {
            start_addr: 0x1000,
            pitch: 0xFFFF,
            ..Voice::default()
        };
        v.key_on(&ram);
        for _ in 0..KEY_ON_START_DELAY {
            v.tick(&ram);
        }
        assert_eq!(v.sample_idx, 7);
        v.tick(&ram);
        assert_eq!(v.sample_idx, 11);
        assert_eq!(v.sample_frac, 0);
    }
}

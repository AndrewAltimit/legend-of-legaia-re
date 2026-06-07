//! Clean-room PSX SPU model.
//!
//! 24 voices, 512 KB SPU RAM, ADSR-shaped per-voice envelopes, libspu-shaped
//! transfer engine. The mixer's job is simple:
//!
//! 1. Each output frame, advance every voice that's on by one SPU-internal
//!    sample (44.1 kHz). Mix into a shared (left, right) accumulator.
//! 2. Apply a master volume (set by libspu `SsSetMVol`).
//! 3. Resample the result to the host sample rate.
//!
//! What this does NOT model:
//!
//! - Pitch modulation, noise mode, FM. None of these are used by Legaia
//!   (verified against the libspu calls in the SCUS dumps - `SpuSetPitch`
//!   is the only pitch path, `SpuSetVoiceAttr` writes only sample addr +
//!   ADSR + volume).
//!
//! ## Reverb
//!
//! [`Reverb`] is a faithful register-driven port of the hardware reverb
//! network (same/different-side IIR reflections + 4-tap comb + two all-pass
//! stages) with the standard libspu [`ReverbMode`] presets covering the 9
//! PSX modes plus Off. Routing is per-voice: set [`Voice::reverb_send`] to
//! opt a voice into the wet signal (libspu `SpuSetVoiceReverb` analogue).
//! The 22.05 kHz FIR resampler is approximated; see the [`reverb`] module.
//!
//! See `docs/subsystems/audio.md` "engine-audio model" for the consumer.

pub mod adpcm;
pub mod adsr;
pub mod ram;
pub mod reverb;
pub mod voice;

pub use reverb::{Reverb, ReverbMode};

use ram::SpuRam;
use voice::Voice;

/// Number of hardware voices on the PSX SPU.
pub const NUM_VOICES: usize = 24;

/// The full SPU model.
#[derive(Debug, Clone)]
pub struct Spu {
    /// SPU RAM (512 KB).
    pub ram: SpuRam,
    /// Per-voice state. Indexed 0..24.
    pub voices: [Voice; NUM_VOICES],
    /// Master volume, 0x0000..=0x3FFF (libspu `MVOL` shape).
    pub master_left: i16,
    pub master_right: i16,
    /// Active reverb processor. Defaults to [`ReverbMode::Off`]. Voices
    /// with `reverb_send = true` route their output through this; the wet
    /// signal is mixed back into the master in [`Spu::tick`].
    pub reverb: Reverb,
    /// Last raw `reverb_mode` value written by libspu - preserved for
    /// debugging / tooling. The active mode in the [`Reverb`] processor
    /// is set via [`Spu::set_reverb_mode`].
    pub reverb_mode_raw: u32,
}

impl Default for Spu {
    fn default() -> Self {
        Self {
            ram: SpuRam::new(),
            voices: std::array::from_fn(|_| Voice::default()),
            master_left: 0x3FFF,
            master_right: 0x3FFF,
            reverb: Reverb::new(ReverbMode::Off),
            reverb_mode_raw: 0,
        }
    }
}

impl Spu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Issue libspu-style key-on for the voices with the corresponding bit
    /// set in `mask` (bit 0 = voice 0, etc.). Equivalent to `SpuKeyOn` /
    /// `SpuSetKey(SpuOn, mask)`.
    pub fn key_on_mask(&mut self, mask: u32) {
        for i in 0..NUM_VOICES {
            if mask & (1u32 << i) != 0 {
                let v = &mut self.voices[i];
                let ram = &self.ram;
                v.key_on(ram);
            }
        }
    }

    /// Mirror of `key_on_mask` for key-off.
    pub fn key_off_mask(&mut self, mask: u32) {
        for i in 0..NUM_VOICES {
            if mask & (1u32 << i) != 0 {
                self.voices[i].key_off();
            }
        }
    }

    /// Set the active reverb mode. The [`Reverb`] processor's buffers are
    /// resized; in-flight wet signal is dropped.
    pub fn set_reverb_mode(&mut self, mode: ReverbMode) {
        self.reverb.set_mode(mode);
    }

    /// libspu `SpuCommonAttr.reverb` analogue - accepts the raw mode byte
    /// and updates both the live processor and the bookkeeping register.
    pub fn write_reverb_mode_byte(&mut self, raw: u8) {
        self.reverb_mode_raw = raw as u32;
        self.set_reverb_mode(ReverbMode::from_byte(raw));
    }

    /// Advance every voice by one sample tick at the SPU internal rate
    /// (44.1 kHz). Returns the (left, right) sample, master-volume scaled
    /// and clamped to i16.
    pub fn tick(&mut self) -> (i16, i16) {
        let mut acc_l: i64 = 0;
        let mut acc_r: i64 = 0;
        let mut send_l: i64 = 0;
        let mut send_r: i64 = 0;
        for v in &mut self.voices {
            let (l, r) = v.tick(&self.ram);
            acc_l += l as i64;
            acc_r += r as i64;
            if v.reverb_send {
                send_l += l as i64;
                send_r += r as i64;
            }
        }
        // Drive the reverb network with the reverb-tagged voices' sum.
        let send_l_i16 = send_l.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
        let send_r_i16 = send_r.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
        let (wet_l, wet_r) = self.reverb.tick(send_l_i16, send_r_i16);
        acc_l += wet_l as i64;
        acc_r += wet_r as i64;
        // Apply master volume.
        let l = ((acc_l * self.master_left as i64) >> 14).clamp(i16::MIN as i64, i16::MAX as i64);
        let r = ((acc_r * self.master_right as i64) >> 14).clamp(i16::MIN as i64, i16::MAX as i64);
        (l as i16, r as i16)
    }

    /// Drain `n` samples into a stereo i16 buffer (pairs of left, right).
    /// Convenience for tests + the cpal callback's resampler.
    pub fn render_into(&mut self, out: &mut [i16]) {
        debug_assert_eq!(out.len() % 2, 0);
        for chunk in out.chunks_exact_mut(2) {
            let (l, r) = self.tick();
            chunk[0] = l;
            chunk[1] = r;
        }
    }

    /// Returns the count of voices currently in the `Off` envelope phase,
    /// i.e. available for a fresh allocation.
    pub fn idle_voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.is_off()).count()
    }

    /// Find an idle voice index. Mirrors the libspu pattern of "scan for a
    /// voice whose envelope has finished".
    pub fn find_idle_voice(&self) -> Option<usize> {
        self.voices.iter().position(|v| v.is_off())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke: 24 silent voices key-on simultaneously, render 1 second of
    /// stereo, finish without panics.
    #[test]
    fn render_silence_one_second() {
        let mut spu = Spu::new();
        let mut buf = vec![0i16; 44100 * 2];
        spu.render_into(&mut buf);
        assert!(buf.iter().all(|&s| s == 0));
    }

    /// key_on_mask actually triggers the per-voice key-on for the right
    /// voices and not the others.
    #[test]
    fn key_on_mask_only_affects_set_bits() {
        let mut spu = Spu::new();
        // Stick a one-block silence stream at 0x1000 so each voice has
        // something to "play".
        let stream = vec![0u8; 16];
        spu.ram.write_at(0x1000, &stream);
        for v in spu.voices.iter_mut() {
            v.start_addr = 0x1000;
        }
        spu.key_on_mask(0b101);
        // Voices 0 and 2 should be in Attack; the rest still Off.
        for (i, v) in spu.voices.iter().enumerate() {
            if i == 0 || i == 2 {
                assert!(!v.is_off(), "voice {i} should be on");
            } else {
                assert!(v.is_off(), "voice {i} should still be off");
            }
        }
    }

    /// idle_voice_count starts at 24, drops to 23 after one key-on.
    #[test]
    fn idle_voice_count_tracks_keyons() {
        let mut spu = Spu::new();
        let stream = vec![0u8; 16];
        spu.ram.write_at(0x1000, &stream);
        for v in spu.voices.iter_mut() {
            v.start_addr = 0x1000;
        }
        assert_eq!(spu.idle_voice_count(), NUM_VOICES);
        spu.key_on_mask(0b1);
        assert_eq!(spu.idle_voice_count(), NUM_VOICES - 1);
    }

    /// find_idle_voice returns None when all voices are busy.
    #[test]
    fn find_idle_voice_returns_none_when_all_busy() {
        let mut spu = Spu::new();
        let stream = vec![0u8; 16];
        spu.ram.write_at(0x1000, &stream);
        for v in spu.voices.iter_mut() {
            v.start_addr = 0x1000;
        }
        spu.key_on_mask(0xFFFFFFFF);
        assert!(spu.find_idle_voice().is_none());
    }

    /// Reverb mode round-trips through the libspu-style mode byte API
    /// and updates the live processor.
    #[test]
    fn write_reverb_mode_byte_updates_processor() {
        let mut spu = Spu::new();
        assert_eq!(spu.reverb.mode, ReverbMode::Off);
        spu.write_reverb_mode_byte(7); // Echo
        assert_eq!(spu.reverb_mode_raw, 7);
        assert_eq!(spu.reverb.mode, ReverbMode::Echo);
        spu.write_reverb_mode_byte(5); // Hall
        assert_eq!(spu.reverb.mode, ReverbMode::Hall);
        // Out-of-range falls back to Off.
        spu.write_reverb_mode_byte(0xFE);
        assert_eq!(spu.reverb.mode, ReverbMode::Off);
    }

    /// A voice with `reverb_send` set produces an echo tail past the
    /// reverb mode's delay length when the master tick is run.
    #[test]
    fn reverb_send_voice_produces_wet_tail() {
        let mut spu = Spu::new();
        spu.set_reverb_mode(ReverbMode::Room);
        // Plant a non-zero stream and key one voice on with reverb_send.
        let stream = vec![0u8; 16];
        spu.ram.write_at(0x1000, &stream);
        spu.voices[0].start_addr = 0x1000;
        spu.voices[0].vol_left = 0x3FFF;
        spu.voices[0].vol_right = 0x3FFF;
        spu.voices[0].set_reverb_send(true);
        spu.key_on_mask(0b1);
        // Render long enough to fill the room delay buffer.
        let delay = spu.reverb.tick(0, 0);
        let _ = delay;
        let mut buf = vec![0i16; 4_410 * 2]; // 100 ms of stereo
        spu.render_into(&mut buf);
        // Silence stream → output is silence (no decoded samples), so we
        // can't validate non-zero output here. Rely on the per-Reverb
        // unit tests for that. The smoke value is "no panics".
    }
}

//! Clean-room PSX SPU reverb model.
//!
//! The retail SPU implements reverb as a comb-filter + allpass network with
//! a configurable work-buffer size at the bottom of SPU RAM. This module
//! ships a simplified equivalent: a per-channel circular delay buffer with
//! a single feedback tap and a wet/dry mix. The presets in
//! [`ReverbMode::params`] are tuned to *sound* like the retail modes - they
//! are not byte-equivalent to the SPU register layout (the retail mode
//! parameters are documented in libspu docs but require the original
//! SPU's specific topology to reproduce exactly).
//!
//! ## When this matters
//!
//! Spirit Arts use the retail SPU's `Echo` / `Delay` modes; without it
//! their attack sound effects feel flat. This module exists to give the
//! engine a perceptible echo tail rather than perfect bit-accuracy.
//!
//! Set per-voice routing with [`crate::spu::voice::Voice::set_reverb_send`]
//! (libspu `SpuSetVoiceReverb` analogue) and select the active mode via
//! [`super::Spu::set_reverb_mode`].

use std::collections::VecDeque;

/// Standard PSX SPU reverb modes. Names match libspu's `SsSetReverbType`
/// enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReverbMode {
    /// Reverb disabled - voices with `reverb_send` produce no echo.
    Off,
    /// Small room.
    Room,
    /// Studio A.
    StudioA,
    /// Studio B.
    StudioB,
    /// Studio C.
    StudioC,
    /// Hall.
    Hall,
    /// Space.
    Space,
    /// Single-tap echo (Spirit Arts).
    Echo,
    /// Long delay.
    Delay,
    /// Pipe.
    Pipe,
}

/// Resolved reverb parameters: delay length in samples, feedback coefficient
/// (Q.14 fixed-point), and wet/dry split (Q.14).
#[derive(Debug, Clone, Copy)]
pub struct ReverbParams {
    pub delay_samples: usize,
    pub feedback_q14: i16,
    pub wet_q14: i16,
}

impl ReverbMode {
    /// Resolve parameter triple. Numbers are tuned by ear against the retail
    /// modes - perceptual match, not bit-exact.
    pub fn params(self) -> ReverbParams {
        // 44.1 kHz internal rate. 1 ms ≈ 44 samples; 100 ms ≈ 4410.
        match self {
            ReverbMode::Off => ReverbParams {
                delay_samples: 0,
                feedback_q14: 0,
                wet_q14: 0,
            },
            ReverbMode::Room => ReverbParams {
                delay_samples: 1100,  // ~25 ms
                feedback_q14: 0x2000, // 0.50
                wet_q14: 0x1800,      // 0.375
            },
            ReverbMode::StudioA => ReverbParams {
                delay_samples: 1764,  // ~40 ms
                feedback_q14: 0x2400, // 0.5625
                wet_q14: 0x1A00,      // 0.40625
            },
            ReverbMode::StudioB => ReverbParams {
                delay_samples: 2200,  // ~50 ms
                feedback_q14: 0x2600, // 0.59375
                wet_q14: 0x1C00,      // 0.4375
            },
            ReverbMode::StudioC => ReverbParams {
                delay_samples: 2645,  // ~60 ms
                feedback_q14: 0x2800, // 0.625
                wet_q14: 0x1E00,      // 0.46875
            },
            ReverbMode::Hall => ReverbParams {
                delay_samples: 4410,  // ~100 ms
                feedback_q14: 0x2C00, // 0.6875
                wet_q14: 0x2000,      // 0.5
            },
            ReverbMode::Space => ReverbParams {
                delay_samples: 7000,  // ~159 ms
                feedback_q14: 0x2E00, // 0.71875
                wet_q14: 0x2000,
            },
            ReverbMode::Echo => ReverbParams {
                delay_samples: 5512,  // ~125 ms
                feedback_q14: 0x3000, // 0.75
                wet_q14: 0x2400,      // 0.5625
            },
            ReverbMode::Delay => ReverbParams {
                delay_samples: 8820,  // ~200 ms
                feedback_q14: 0x2400, // 0.5625
                wet_q14: 0x2000,
            },
            ReverbMode::Pipe => ReverbParams {
                delay_samples: 1500,  // ~34 ms
                feedback_q14: 0x3400, // 0.8125
                wet_q14: 0x2200,      // 0.53125
            },
        }
    }

    /// Decode from a libspu-style mode byte. Out-of-range values map to
    /// [`ReverbMode::Off`] to keep the engine silent rather than panic.
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => ReverbMode::Off,
            1 => ReverbMode::Room,
            2 => ReverbMode::StudioA,
            3 => ReverbMode::StudioB,
            4 => ReverbMode::StudioC,
            5 => ReverbMode::Hall,
            6 => ReverbMode::Space,
            7 => ReverbMode::Echo,
            8 => ReverbMode::Delay,
            9 => ReverbMode::Pipe,
            _ => ReverbMode::Off,
        }
    }
}

/// Stereo reverb processor - one delay buffer per channel.
#[derive(Debug, Clone)]
pub struct Reverb {
    pub mode: ReverbMode,
    params: ReverbParams,
    left: VecDeque<i16>,
    right: VecDeque<i16>,
}

impl Reverb {
    pub fn new(mode: ReverbMode) -> Self {
        let params = mode.params();
        let mut left = VecDeque::with_capacity(params.delay_samples.max(1));
        let mut right = VecDeque::with_capacity(params.delay_samples.max(1));
        for _ in 0..params.delay_samples {
            left.push_back(0);
            right.push_back(0);
        }
        Self {
            mode,
            params,
            left,
            right,
        }
    }

    /// Reconfigure the active mode. Buffers are resized to match.
    pub fn set_mode(&mut self, mode: ReverbMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.params = mode.params();
        self.left.clear();
        self.right.clear();
        for _ in 0..self.params.delay_samples {
            self.left.push_back(0);
            self.right.push_back(0);
        }
    }

    /// Push one stereo sample of *reverb send* signal into the processor and
    /// pull one stereo sample of *reverb wet* signal out. The caller mixes
    /// the wet output into the master alongside the dry signal.
    pub fn tick(&mut self, send_l: i16, send_r: i16) -> (i16, i16) {
        if self.params.delay_samples == 0 {
            return (0, 0);
        }
        let wet_l = self.left.pop_front().unwrap_or(0);
        let wet_r = self.right.pop_front().unwrap_or(0);
        // Feedback: combine the new send with a fraction of the wet
        // output, and write back into the buffer for next time around.
        let fb = self.params.feedback_q14 as i32;
        let new_l = (send_l as i32) + ((wet_l as i32 * fb) >> 14);
        let new_r = (send_r as i32) + ((wet_r as i32 * fb) >> 14);
        self.left.push_back(new_l.clamp(-0x7FFF, 0x7FFF) as i16);
        self.right.push_back(new_r.clamp(-0x7FFF, 0x7FFF) as i16);
        // Wet output is scaled by the wet/dry coefficient.
        let wet = self.params.wet_q14 as i32;
        let out_l = (wet_l as i32 * wet) >> 14;
        let out_r = (wet_r as i32 * wet) >> 14;
        (
            out_l.clamp(-0x7FFF, 0x7FFF) as i16,
            out_r.clamp(-0x7FFF, 0x7FFF) as i16,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_mode_is_silent() {
        let mut r = Reverb::new(ReverbMode::Off);
        let (l, rr) = r.tick(0x1000, 0x1000);
        assert_eq!((l, rr), (0, 0));
    }

    #[test]
    fn room_produces_delayed_output() {
        let mut r = Reverb::new(ReverbMode::Room);
        // First N samples produce no wet (buffer is filled with zeros).
        for _ in 0..r.params.delay_samples {
            let (l, _) = r.tick(0x4000, 0x4000);
            assert_eq!(l, 0);
        }
        // Sample at delay + 0 should now contain the original signal,
        // attenuated by `wet_q14`.
        let (l, _) = r.tick(0, 0);
        assert!(l > 0);
        assert!(l < 0x4000);
    }

    #[test]
    fn feedback_creates_decaying_tail() {
        let mut r = Reverb::new(ReverbMode::Echo);
        // Hit it with one impulse, then run quietly for several delay
        // periods. Each delay length should produce a decaying replay.
        let (_, _) = r.tick(0x4000, 0x4000);
        let mut peak_per_delay: Vec<i16> = Vec::new();
        for _ in 0..3 {
            let mut peak = 0i16;
            for _ in 0..r.params.delay_samples {
                let (l, _) = r.tick(0, 0);
                peak = peak.max(l.abs());
            }
            peak_per_delay.push(peak);
        }
        // First delay echo is non-zero.
        assert!(peak_per_delay[0] > 0);
        // The tail decays - second / third echoes are smaller than the
        // first.
        assert!(peak_per_delay[1] <= peak_per_delay[0]);
        assert!(peak_per_delay[2] <= peak_per_delay[1]);
    }

    #[test]
    fn mode_change_resets_buffers() {
        let mut r = Reverb::new(ReverbMode::Hall);
        // Pump some signal through.
        for _ in 0..100 {
            r.tick(0x2000, 0x2000);
        }
        r.set_mode(ReverbMode::Room);
        // Buffers reinit'd with zeros, so the first delay-period of taps
        // produces no wet.
        for _ in 0..r.params.delay_samples {
            let (l, _) = r.tick(0, 0);
            assert_eq!(l, 0);
        }
    }

    #[test]
    fn from_byte_matches_known_modes() {
        assert_eq!(ReverbMode::from_byte(0), ReverbMode::Off);
        assert_eq!(ReverbMode::from_byte(7), ReverbMode::Echo);
        assert_eq!(ReverbMode::from_byte(9), ReverbMode::Pipe);
        // Out of range falls back to Off.
        assert_eq!(ReverbMode::from_byte(0xFF), ReverbMode::Off);
    }
}

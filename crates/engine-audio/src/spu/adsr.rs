//! PSX SPU ADSR envelope state machine.
//!
//! The PSX SPU drives each voice's volume through an Attack-Decay-Sustain-
//! Release envelope. The envelope counter is unsigned 16-bit (0..=0x7FFF
//! peak); each phase advances the counter by an amount derived from a
//! 7-bit "step + shift" rate plus a linear-vs-exponential mode bit.
//!
//! Layout of the two ADSR words (PSX libspu / nocash psx-spx):
//!
//! ```text
//!   ADSR1 (low 16 bits):
//!     bits 15:    attack mode      (0 = linear, 1 = exponential)
//!     bits 14..10: attack shift    (5 bits; larger -> slower)
//!     bits  9..8: attack step      (2 bits; +7 .. +4 added per tick: 7-step)
//!     bits  7..4: decay shift      (4 bits; decay always exponential, step=-8)
//!     bits  3..0: sustain level    (4 bits; (SL+1) << 11 = target counter)
//!
//!   ADSR2 (high 16 bits):
//!     bit 15: sustain mode         (0 = linear, 1 = exponential)
//!     bit 14: sustain direction    (0 = increase, 1 = decrease)
//!     bit 13: reserved
//!     bits 12..8: sustain shift    (5 bits)
//!     bits  7..6: sustain step     (2 bits; +7-step or -8+step depending on dir)
//!     bit  5: release mode         (0 = linear, 1 = exponential)
//!     bits  4..0: release shift    (5 bits)
//! ```
//!
//! Per-tick advance, given (mode, shift, step_bits, dir_sign):
//!
//! ```text
//!   step = (7 - step_bits) for increase, (-8 + step_bits) for decrease
//!   if shift < 11:  delta = step << (11 - shift)
//!   else:           delta = step >> (shift - 11)        (rounds toward 0)
//!   if exponential and increase and counter > 0x6000:
//!     delta >>= 2
//!   if exponential and decrease:
//!     delta = (delta * counter) >> 15
//!   counter = clamp(counter + dir_sign * delta, 0, 0x7FFF)
//! ```
//!
//! The increase/decrease distinction is carried by `step_bits` interpretation
//! plus the explicit direction sign (sustain can go either way).
//!
//! Source: this is the standard textbook PSX ADSR formula from the libspu
//! reference and nocash psx-spx; no Sony bytes here. The `crates/vab` parser
//! reads `adsr1`/`adsr2` directly off the VAB tone metadata (which is
//! game-data, not Sony-binary).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

#[derive(Debug, Clone, Copy)]
pub struct AdsrConfig {
    pub attack_exp: bool,
    pub attack_shift: u8,
    pub attack_step: u8,
    pub decay_shift: u8,
    pub sustain_level: u16,
    pub sustain_exp: bool,
    pub sustain_decrease: bool,
    pub sustain_shift: u8,
    pub sustain_step: u8,
    pub release_exp: bool,
    pub release_shift: u8,
}

impl AdsrConfig {
    /// Decode from `(adsr1, adsr2)` words as stored in VAB tone metadata.
    pub fn from_words(adsr1: u16, adsr2: u16) -> Self {
        Self {
            attack_exp: (adsr1 >> 15) & 1 != 0,
            attack_shift: ((adsr1 >> 10) & 0x1F) as u8,
            attack_step: ((adsr1 >> 8) & 0x03) as u8,
            decay_shift: ((adsr1 >> 4) & 0x0F) as u8,
            sustain_level: ((adsr1 & 0x0F) + 1) << 11, // 0x0800 .. 0x8000
            sustain_exp: (adsr2 >> 15) & 1 != 0,
            sustain_decrease: (adsr2 >> 14) & 1 != 0,
            sustain_shift: ((adsr2 >> 8) & 0x1F) as u8,
            sustain_step: ((adsr2 >> 6) & 0x03) as u8,
            release_exp: (adsr2 >> 5) & 1 != 0,
            release_shift: (adsr2 & 0x1F) as u8,
        }
    }
}

impl Default for AdsrConfig {
    fn default() -> Self {
        // Hardware reset: linear-attack-fast, no decay, sustain=peak,
        // linear-release-fast. Matches what an "unconfigured" voice would
        // produce: instant attack, no envelope shaping.
        Self {
            attack_exp: false,
            attack_shift: 0,
            attack_step: 0,
            decay_shift: 0,
            sustain_level: 0x8000,
            sustain_exp: false,
            sustain_decrease: true,
            sustain_shift: 0,
            sustain_step: 0,
            release_exp: false,
            release_shift: 0,
        }
    }
}

/// Per-voice ADSR runtime state.
#[derive(Debug, Clone, Copy)]
pub struct AdsrState {
    pub phase: Phase,
    /// Envelope level, 0..=0x7FFF.
    pub level: u16,
}

impl Default for AdsrState {
    fn default() -> Self {
        Self {
            phase: Phase::Off,
            level: 0,
        }
    }
}

impl AdsrState {
    pub fn key_on(&mut self) {
        self.phase = Phase::Attack;
        self.level = 0;
    }

    pub fn key_off(&mut self) {
        // libspu KeyOff transitions any phase to Release.
        if self.phase != Phase::Off {
            self.phase = Phase::Release;
        }
    }

    /// Advance the envelope by one sample tick. Returns the new level.
    pub fn tick(&mut self, cfg: &AdsrConfig) -> u16 {
        match self.phase {
            Phase::Off => {
                self.level = 0;
                return 0;
            }
            Phase::Attack => {
                let delta = compute_delta_increase(
                    cfg.attack_exp,
                    cfg.attack_shift,
                    cfg.attack_step,
                    self.level,
                );
                self.level = self.level.saturating_add(delta as u16).min(0x7FFF);
                if self.level >= 0x7FFF {
                    self.level = 0x7FFF;
                    self.phase = Phase::Decay;
                }
            }
            Phase::Decay => {
                // Decay is always exponential decrease with step=-8 (i.e. step_bits=0).
                let delta = compute_delta_exp_decrease(cfg.decay_shift, 0, self.level);
                self.level = self.level.saturating_sub(delta as u16);
                if self.level <= cfg.sustain_level {
                    self.level = cfg.sustain_level;
                    self.phase = Phase::Sustain;
                }
            }
            Phase::Sustain => {
                if cfg.sustain_decrease {
                    let delta = if cfg.sustain_exp {
                        compute_delta_exp_decrease(cfg.sustain_shift, cfg.sustain_step, self.level)
                    } else {
                        // Linear sustain-*decrease*: the step is negative
                        // (`-8 + step_bits`, magnitude `8 - step_bits`), the
                        // same StepValue table the exponential decrease uses -
                        // NOT the increase table (`7 - step_bits`). Passing the
                        // increase sign here would fade sustain ~1 step slow.
                        compute_delta_linear(cfg.sustain_shift, cfg.sustain_step, true)
                    };
                    self.level = self.level.saturating_sub(delta as u16);
                    if self.level == 0 {
                        self.phase = Phase::Off;
                    }
                } else {
                    let delta = compute_delta_increase(
                        cfg.sustain_exp,
                        cfg.sustain_shift,
                        cfg.sustain_step,
                        self.level,
                    );
                    self.level = self.level.saturating_add(delta as u16).min(0x7FFF);
                }
            }
            Phase::Release => {
                let delta = if cfg.release_exp {
                    compute_delta_exp_decrease(cfg.release_shift, 0, self.level)
                } else {
                    // Linear release always steps by the fixed `-8` StepValue
                    // (magnitude 8), so pass the decrease sign. The prior
                    // `false` used the `+7` increase magnitude, making a
                    // linear-release voice fade ~12.5% slow.
                    compute_delta_linear(cfg.release_shift, 0, true)
                };
                self.level = self.level.saturating_sub(delta as u16);
                if self.level == 0 {
                    self.phase = Phase::Off;
                }
            }
        }
        self.level
    }
}

/// Linear delta for increase (`negative=false`) or decrease (`negative=true`).
fn compute_delta_linear(shift: u8, step_bits: u8, negative: bool) -> i32 {
    let step = if negative {
        -(8 - step_bits as i32)
    } else {
        7 - step_bits as i32
    };
    if shift < 11 {
        step << (11 - shift) as u32
    } else {
        step >> (shift - 11) as u32
    }
    .abs()
}

/// Exponential-curve increase delta. Slows down past 0x6000, which gives the
/// characteristic libspu attack curve.
fn compute_delta_increase(exp: bool, shift: u8, step_bits: u8, level: u16) -> i32 {
    let mut delta = compute_delta_linear(shift, step_bits, false);
    if exp && level > 0x6000 {
        delta >>= 2;
    }
    delta
}

/// Exponential-curve decrease delta. Scales by `level/0x8000`, which gives
/// the libspu "fade exponentially toward zero" shape.
fn compute_delta_exp_decrease(shift: u8, step_bits: u8, level: u16) -> i32 {
    let base = compute_delta_linear(shift, step_bits, true);
    ((base as u32 * level as u32) >> 15) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default ADSR (all-zero shifts) ramps to peak in three ticks: each
    /// tick adds delta = 7 << 11 = 0x3800, so 0 -> 0x3800 -> 0x7000 ->
    /// (0x7000 + 0x3800).min(0x7FFF) = 0x7FFF -> Decay.
    #[test]
    fn default_adsr_attacks_to_peak_in_three_ticks() {
        let cfg = AdsrConfig::default();
        let mut s = AdsrState::default();
        s.key_on();
        assert_eq!(s.phase, Phase::Attack);
        assert_eq!(s.tick(&cfg), 0x3800);
        assert_eq!(s.phase, Phase::Attack);
        assert_eq!(s.tick(&cfg), 0x7000);
        assert_eq!(s.phase, Phase::Attack);
        let lvl = s.tick(&cfg);
        assert_eq!(lvl, 0x7FFF);
        assert_eq!(s.phase, Phase::Decay);
    }

    /// A configured ADSR with slow attack actually ramps slowly.
    #[test]
    fn slow_attack_takes_many_ticks() {
        let cfg = AdsrConfig {
            attack_shift: 10, // delta = 7 << 1 = 14 per tick
            ..AdsrConfig::default()
        };
        let mut s = AdsrState::default();
        s.key_on();
        for _ in 0..100 {
            s.tick(&cfg);
        }
        assert!(s.level > 0);
        assert!(s.level < 0x7FFF);
        assert_eq!(s.phase, Phase::Attack);
    }

    /// Decay drops to sustain level then stops.
    #[test]
    fn decay_stops_at_sustain_level() {
        // SL field = 0xF -> sustain_level = (0xF+1) << 11 = 0x8000... but
        // peak is 0x7FFF. Use SL=7 -> level = 0x4000.
        let cfg = AdsrConfig {
            attack_shift: 0,
            attack_step: 0,
            sustain_level: 0x4000,
            decay_shift: 4, // moderate decay
            ..AdsrConfig::default()
        };
        let mut s = AdsrState::default();
        s.key_on();
        for _ in 0..3 {
            s.tick(&cfg);
        }
        assert_eq!(s.phase, Phase::Decay);
        for _ in 0..2000 {
            s.tick(&cfg);
            if s.phase == Phase::Sustain {
                break;
            }
        }
        assert_eq!(s.phase, Phase::Sustain);
        assert_eq!(s.level, 0x4000);
    }

    /// KeyOff during sustain transitions to release and eventually goes off.
    #[test]
    fn release_takes_voice_to_off() {
        let cfg = AdsrConfig {
            sustain_level: 0x4000,
            decay_shift: 4,
            release_shift: 8,
            ..AdsrConfig::default()
        };
        let mut s = AdsrState::default();
        s.key_on();
        for _ in 0..2200 {
            s.tick(&cfg);
            if s.phase == Phase::Sustain {
                break;
            }
        }
        s.key_off();
        assert_eq!(s.phase, Phase::Release);
        for _ in 0..50_000 {
            s.tick(&cfg);
            if s.phase == Phase::Off {
                break;
            }
        }
        assert_eq!(s.phase, Phase::Off);
        assert_eq!(s.level, 0);
    }

    /// Linear release steps by the fixed `-8` StepValue (magnitude 8), not the
    /// `+7` increase magnitude. With `release_shift = 0` one tick subtracts
    /// `8 << (11 - 0) = 0x4000`, so peak `0x7FFF` drops to `0x3FFF`. The old
    /// (wrong) increase sign gave `7 << 11 = 0x3800` -> `0x47FF`.
    #[test]
    fn linear_release_uses_decrease_step_magnitude() {
        let cfg = AdsrConfig {
            release_exp: false,
            release_shift: 0,
            ..AdsrConfig::default()
        };
        let mut s = AdsrState {
            phase: Phase::Release,
            level: 0x7FFF,
        };
        assert_eq!(s.tick(&cfg), 0x3FFF);
    }

    /// Linear sustain-*decrease* uses the same `-8 + step_bits` decrease table.
    /// With `sustain_step = 0`, `sustain_shift = 0` one tick subtracts
    /// `8 << 11 = 0x4000` (not the `+7` increase `0x3800`).
    #[test]
    fn linear_sustain_decrease_uses_decrease_step_magnitude() {
        let cfg = AdsrConfig {
            sustain_exp: false,
            sustain_decrease: true,
            sustain_shift: 0,
            sustain_step: 0,
            ..AdsrConfig::default()
        };
        let mut s = AdsrState {
            phase: Phase::Sustain,
            level: 0x7FFF,
        };
        assert_eq!(s.tick(&cfg), 0x3FFF);
    }

    /// A higher `sustain_step` shrinks the linear-decrease magnitude by the
    /// decrease table (`8 - step_bits`): step_bits 3 -> magnitude 5, so one
    /// tick at shift 0 subtracts `5 << 11 = 0x2800`.
    #[test]
    fn linear_sustain_decrease_step_bits_scale_by_decrease_table() {
        let cfg = AdsrConfig {
            sustain_exp: false,
            sustain_decrease: true,
            sustain_shift: 0,
            sustain_step: 3,
            ..AdsrConfig::default()
        };
        let mut s = AdsrState {
            phase: Phase::Sustain,
            level: 0x7FFF,
        };
        assert_eq!(s.tick(&cfg), 0x7FFF - 0x2800);
    }

    /// AdsrConfig::from_words round-trips the bit layout we care about.
    #[test]
    fn adsr_config_decode_layout() {
        // adsr1 with attack_shift=5, attack_step=2, decay_shift=3, sl=4
        let adsr1 = (5u16 << 10) | (2 << 8) | (3 << 4) | 4;
        // adsr2 with sustain_dec=1, sustain_shift=10, release_shift=12
        let adsr2 = (1u16 << 14) | (10 << 8) | 12;
        let cfg = AdsrConfig::from_words(adsr1, adsr2);
        assert_eq!(cfg.attack_shift, 5);
        assert_eq!(cfg.attack_step, 2);
        assert_eq!(cfg.decay_shift, 3);
        assert_eq!(cfg.sustain_level, (4 + 1) << 11);
        assert!(cfg.sustain_decrease);
        assert_eq!(cfg.sustain_shift, 10);
        assert_eq!(cfg.release_shift, 12);
    }
}

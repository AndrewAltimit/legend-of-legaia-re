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
//! Per-step advance, given (mode, shift, step_bits, direction) - the
//! hardware's **cycle-wait** scheme (nocash psx-spx):
//!
//! ```text
//!   cycles = 1 << max(0, shift - 11)
//!   step   = (7 - step_bits) << max(0, 11 - shift)   for increase
//!            (-8 + step_bits) << max(0, 11 - shift)  for decrease
//!   if exponential and increase and counter > 0x6000:  cycles *= 4
//!   if exponential and decrease:  step = (step * counter) >> 15
//!   wait (cycles - 1) ticks, then counter = clamp(counter + step, 0, 0x7FFF)
//! ```
//!
//! Two properties of this scheme are load-bearing and were lost by the
//! earlier "per-tick delta" collapse (`delta = step >> (shift - 11)`,
//! magnitudes only):
//!
//! - a slow rate means a **longer wait**, never a zero step - `step >> 4`
//!   rounding to 0 froze every envelope with `shift >= 15`;
//! - the exponential-decrease scale runs on the **signed** step, and an
//!   arithmetic shift of a negative number floors at -1, so the tail always
//!   reaches 0. On positive magnitudes `(base * level) >> 15` floors at 0,
//!   parking every exponential release at a small nonzero level forever -
//!   which kept the voice busy, leaked the 24-voice pool dry in seconds of
//!   BGM, and silenced every note allocated after that.
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
    /// The raw `(adsr1, adsr2)` words this config was decoded from.
    /// Retained so a note trace can report tone selection in the same
    /// units the retail SPU registers carry (see [`crate::note_trace`]).
    pub raw: (u16, u16),
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
            raw: (adsr1, adsr2),
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
            raw: (0, 0),
        }
    }
}

/// Per-voice ADSR runtime state.
#[derive(Debug, Clone, Copy)]
pub struct AdsrState {
    pub phase: Phase,
    /// Envelope level, 0..=0x7FFF.
    pub level: u16,
    /// Remaining wait ticks before the next step applies (the hardware's
    /// `cycles - 1` countdown; slow rates wait longer, they never step by 0).
    wait: u32,
}

impl Default for AdsrState {
    fn default() -> Self {
        Self {
            phase: Phase::Off,
            level: 0,
            wait: 0,
        }
    }
}

impl AdsrState {
    pub fn key_on(&mut self) {
        self.phase = Phase::Attack;
        self.level = 0;
        self.wait = 0;
    }

    pub fn key_off(&mut self) {
        // libspu KeyOff transitions any phase to Release.
        if self.phase != Phase::Off {
            self.phase = Phase::Release;
            self.wait = 0;
        }
    }

    /// Advance the envelope by one sample tick. Returns the new level.
    pub fn tick(&mut self, cfg: &AdsrConfig) -> u16 {
        if self.phase == Phase::Off {
            self.level = 0;
            return 0;
        }
        // Cycle-wait countdown: a slow rate is a long wait between full-size
        // steps, never a zero-size step.
        if self.wait > 0 {
            self.wait -= 1;
            return self.level;
        }
        let spec = match self.phase {
            Phase::Off => unreachable!("handled above"),
            Phase::Attack => env_step(
                cfg.attack_exp,
                false,
                cfg.attack_shift,
                cfg.attack_step,
                self.level,
            ),
            // Decay is always exponential decrease with step=-8 (step_bits=0).
            Phase::Decay => env_step(true, true, cfg.decay_shift, 0, self.level),
            Phase::Sustain => env_step(
                cfg.sustain_exp,
                cfg.sustain_decrease,
                cfg.sustain_shift,
                cfg.sustain_step,
                self.level,
            ),
            Phase::Release => env_step(cfg.release_exp, true, cfg.release_shift, 0, self.level),
        };
        self.level = (i32::from(self.level) + spec.step).clamp(0, 0x7FFF) as u16;
        self.wait = spec.cycles.saturating_sub(1);
        match self.phase {
            Phase::Attack => {
                if self.level >= 0x7FFF {
                    self.level = 0x7FFF;
                    self.phase = Phase::Decay;
                    self.wait = 0;
                }
            }
            Phase::Decay => {
                if self.level <= cfg.sustain_level {
                    self.level = cfg.sustain_level.min(0x7FFF);
                    self.phase = Phase::Sustain;
                    self.wait = 0;
                }
            }
            Phase::Sustain => {
                if cfg.sustain_decrease && self.level == 0 {
                    self.phase = Phase::Off;
                    self.wait = 0;
                }
            }
            Phase::Release => {
                if self.level == 0 {
                    self.phase = Phase::Off;
                    self.wait = 0;
                }
            }
            Phase::Off => {}
        }
        self.level
    }
}

/// One envelope step: how many ticks it spans and the **signed** level delta
/// it applies at the end.
struct EnvStep {
    cycles: u32,
    step: i32,
}

/// The hardware rate resolver (nocash psx-spx "AdsrCycles / AdsrStep").
///
/// `decrease` selects the `-8 + step_bits` StepValue row (increase uses
/// `7 - step_bits`). The exponential-decrease scale multiplies the SIGNED
/// step by `level` and arithmetic-shifts right, so its floor is -1 - the
/// property that lets a release tail actually land on 0.
fn env_step(exp: bool, decrease: bool, shift: u8, step_bits: u8, level: u16) -> EnvStep {
    let mut cycles: u32 = 1 << (shift as u32).saturating_sub(11);
    let base = if decrease {
        -8 + step_bits as i32
    } else {
        7 - step_bits as i32
    };
    let mut step = base << 11u32.saturating_sub(shift as u32);
    if exp {
        if !decrease && level > 0x6000 {
            cycles = cycles.saturating_mul(4);
        }
        if decrease {
            step = (step * i32::from(level)) >> 15;
        }
    }
    EnvStep { cycles, step }
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
            wait: 0,
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
            wait: 0,
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
            wait: 0,
        };
        assert_eq!(s.tick(&cfg), 0x7FFF - 0x2800);
    }

    /// Every release configuration must land the envelope on 0 and free the
    /// voice - the leak regression. The exponential-decrease floor is the
    /// signed arithmetic shift's -1; the positive-magnitude version floored
    /// at 0 and parked the voice at a small nonzero level forever, draining
    /// the 24-voice pool dry within seconds of BGM.
    #[test]
    fn release_always_reaches_off_for_every_rate() {
        for &exp in &[false, true] {
            for shift in 0..=31u8 {
                let cfg = AdsrConfig {
                    release_exp: exp,
                    release_shift: shift,
                    ..AdsrConfig::default()
                };
                let mut s = AdsrState {
                    phase: Phase::Release,
                    level: 0x7FFF,
                    wait: 0,
                };
                // Generous budget: the slowest hardware release (exp,
                // shift 31) is minutes long; ticking steps of the wait
                // counter directly keeps the test O(steps), not O(ticks).
                let mut steps = 0u64;
                while s.phase != Phase::Off && steps < 5_000_000 {
                    s.wait = 0; // collapse the wait; only convergence matters
                    s.tick(&cfg);
                    steps += 1;
                }
                assert_eq!(
                    s.phase,
                    Phase::Off,
                    "release must finish (exp={exp} shift={shift}), stuck at level {}",
                    s.level
                );
            }
        }
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

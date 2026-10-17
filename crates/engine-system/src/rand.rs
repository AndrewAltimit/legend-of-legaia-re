//! The PSX BIOS `rand()` LCG, shared by every engine kernel that rolls
//! through it (the level-up stat jitter, the minigame rules engines).
//!
//! It lives here, below `engine-core`, so the minigame crate can draw from
//! the same generator `engine-battle::levelup` rolls through without
//! depending on the simulation crate. [`bios_rand_shape`] is the
//! stateless one-step form of the same recurrence.

/// Faithful PSX BIOS `rand()` (BIOS call `A(0x2F)`) - a 32-bit LCG.
///
/// `seed = seed × 0x41C6_4E6D + 0x3039; return (seed >> 16) & 0x7FFF`. This is
/// the generator the retail level-up applier `FUN_801E9504` draws from for the
/// per-level stat-growth jitter (`rand() % (2×jitter+1) − jitter`). The
/// *algorithm* is faithful; the seed at level-up time is runtime BIOS state the
/// engine can't recover from disc, so a bit-exact roll requires seeding from a
/// capture. Installed (opt-in) via `engine-core`'s
/// `LevelUpTracker::with_level_up_jitter`.
///
/// PORT: BIOS `rand`/`srand` (A-table 0x2F/0x30); consumed by FUN_801E9504.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiosRand {
    seed: u32,
}

impl BiosRand {
    /// Seed the generator. Mirrors BIOS `srand(seed)`.
    pub fn new(seed: u32) -> Self {
        Self { seed }
    }

    /// Advance and return the next 15-bit value (`0..=0x7FFF`), as BIOS `rand()`.
    pub fn next_u15(&mut self) -> u16 {
        self.seed = world_lcg_step(self.seed);
        ((self.seed >> 16) & 0x7FFF) as u16
    }
}

/// PsyQ-shape 32-bit linear congruential RNG. Returned value is the high-15
/// bits, in the range `0..=0x7FFF`. The seed is mutated in place.
///
/// Identical to PSX libc `rand()`, which is what the game uses
/// (`FUN_80056798`, `ghidra/scripts/funcs/80056798.txt`). For deterministic
/// replay the engine must seed this from the same boot-time source the
/// retail game uses; the precise source is currently the SPU master clock
/// at boot, captured in `_DAT_8007AE5C`.
pub fn psyq_rand_step(seed: &mut u32) -> u16 {
    *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    ((*seed >> 16) & 0x7FFF) as u16
}

/// The value BIOS `rand()` (`A(2Fh)`, the thunk `FUN_80056798`:
/// `li t2,0xA0; jr t2; li t1,0x2F`) returns for an LCG state: its high
/// half, `(state >> 16) & 0x7FFF`.
///
/// Every engine draw that stands in for a retail `jal 0x80056798` shapes
/// its generator's raw state through this before a consumer sees it. Retail
/// callers test the **low** bits of the result (`rand & 1`, `& 7`, `& 0xF`,
/// `% n`) and divide it as a 15-bit quantity; a raw 32-bit LCG state has a
/// low bit of period 2 and a low nibble of period 16, and a sign bit, so
/// handing it over unshaped turns coin flips into alternations, bursts a
/// `& 0xF` gate once every sixteen draws, and makes a signed `%` negative.
pub fn bios_rand_shape(state: u32) -> u32 {
    (state >> 16) & 0x7FFF
}

/// One raw step of the engine's **world** generator - the single stream
/// `World::next_rng` advances and every engine stand-in for a retail
/// `jal 0x80056798` draws from. Retail's `rand()` has one kernel seed shared
/// by SCUS and every overlay, so a subsystem that keeps a private seed
/// interleaves its draws differently from retail; lending this state in and
/// out (see [`world_rand`]) is how a subsystem that cannot hold `&mut World`
/// still draws in order on the one stream.
///
/// The step is the BIOS's own. `FUN_80056798` jumps to `A(2Fh)`, which in
/// the retail BIOS ROM (SCPH-5501, `0xBFC02200`) reads the seed word at
/// physical `0x9010`, multiplies it by `0x41C64E6D` (`lui 0x41C6`,
/// `ori 0x4E6D`, `multu`), adds `0x3039`, stores it back and returns
/// `(seed >> 16) & 0x7FFF`; `A(30h)` `srand` at `0xBFC02230` only stores the
/// word. So the stream position a retail state holds is its RAM word at
/// `0x80009010`, and the engine continues it exactly when seeded from it.
pub fn world_lcg_step(state: u32) -> u32 {
    state
        .wrapping_mul(BIOS_RAND_MUL)
        .wrapping_add(BIOS_RAND_ADD)
}

/// The BIOS `rand()` multiplier (`A(2Fh)`, `0xBFC02208..0xBFC0220C`).
pub const BIOS_RAND_MUL: u32 = 0x41C6_4E6D;
/// The BIOS `rand()` increment (`addiu v1,v1,0x3039` at `0xBFC0221C`).
pub const BIOS_RAND_ADD: u32 = 0x3039;
/// The BIOS `rand()` seed word in kernel RAM (`lw v1,-0x6FF0(0xA0010000)` at
/// `0xBFC02204`, physical `0x9010`).
pub const BIOS_RAND_SEED_ADDR: u32 = 0x8000_9010;

/// One retail-shaped `rand()` draw on a lent copy of the world stream:
/// advance `state` by [`world_lcg_step`] and shape it through
/// [`bios_rand_shape`] (`0..=0x7FFF`). The caller writes `state` back to the
/// world after its draws.
pub fn world_rand(state: &mut u32) -> u32 {
    *state = world_lcg_step(*state);
    bios_rand_shape(*state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psyq_rand_is_deterministic_from_seed() {
        let mut a = 0x12345678;
        let mut b = 0x12345678;
        // Ten draws from identical seeds produce identical values.
        for _ in 0..10 {
            assert_eq!(psyq_rand_step(&mut a), psyq_rand_step(&mut b));
        }
        // ...but two draws are not equal in general.
        let mut s = 0x12345678;
        let r1 = psyq_rand_step(&mut s);
        let r2 = psyq_rand_step(&mut s);
        assert_ne!(r1, r2);
    }

    #[test]
    fn psyq_rand_in_range() {
        let mut seed = 1;
        for _ in 0..1000 {
            let r = psyq_rand_step(&mut seed);
            assert!(r <= 0x7FFF);
        }
    }

    #[test]
    fn bios_rand_shape_is_psyq_rand_over_the_same_state() {
        // `psyq_rand_step` = one BIOS step then the shape; the shape alone over
        // any state is the same high-half read.
        let mut seed = 0x0BAD_F00D;
        for _ in 0..100 {
            let r = psyq_rand_step(&mut seed);
            assert_eq!(u32::from(r), bios_rand_shape(seed));
        }
        assert_eq!(bios_rand_shape(0xFFFF_FFFF), 0x7FFF);
        assert_eq!(bios_rand_shape(0x0000_FFFF), 0);
    }
}

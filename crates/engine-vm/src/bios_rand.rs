//! The PSX BIOS `rand()` LCG, shared by every engine kernel that rolls
//! through it (the level-up stat jitter, the minigame rules engines).
//!
//! It lives here, below `engine-core`, so the minigame crate can draw from
//! the same generator `engine-core::levelup` re-exports without depending on
//! the simulation crate. [`crate::battle_formulas::bios_rand_shape`] is the
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
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        ((self.seed >> 16) & 0x7FFF) as u16
    }
}

//! Holding a recorded cast's `rand()` stream where retail held it
//! (`LEGAIA_CAST_RAND_SEED`).
//!
//! Retail draws every `rand()` (BIOS `A(2Fh)`, `jal 0x80056798`) from one
//! seed word, `0x80009010`: `seed = seed * 0x41C64E6D + 0x3039`, the draw
//! `(seed >> 16) & 0x7FFF`. The engine's world stream is the same generator
//! (`World::next_rng`), but it reaches a cast's arms after another number of
//! draws than retail made, so an arm whose look rides on its draws - PROT
//! 0962 `0xA4` arm 3 picks each of its sixteen rocks by `rand() % 4` - plays
//! another pattern than the recording, and the cast-sequence oracle scores
//! the stream, not the port.
//!
//! The cast-sequence probe logs the seed word on every vsync (`rand` in its
//! `meta.csv`). This seed names one key of the cast - an action state and,
//! inside a module band, a module phase - and the seed retail held on its
//! **last** vsync under that key, the state the arm's pass draws from. The
//! engine child sets its world stream to it before every tick it opens under
//! the same key, so the pass that leaves the key draws as retail's did. A
//! play host never sets it; with it unset the engine runs its own stream.

use legaia_engine_core::world::World;

/// One cast key's recorded `rand()` seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastRandSeed {
    /// The action-SM state `ctx[+0x07]`.
    pub st7: u8,
    /// The module phase `ctx[+0x279]` (meaningful in the module bands).
    pub phase: u8,
    /// Retail's seed word `0x80009010` on the key's last vsync.
    pub seed: u32,
}

/// `st7:phase:seed`, each decimal.
pub fn cast_rand_seed_to_env(s: &CastRandSeed) -> String {
    format!("{}:{}:{}", s.st7, s.phase, s.seed)
}

/// Parse [`cast_rand_seed_to_env`]'s form.
pub fn cast_rand_seed_from_env(v: &str) -> Option<CastRandSeed> {
    let mut it = v.trim().split(':');
    let st7 = it.next()?.parse().ok()?;
    let phase = it.next()?.parse().ok()?;
    let seed = it.next()?.parse().ok()?;
    it.next()
        .is_none()
        .then_some(CastRandSeed { st7, phase, seed })
}

/// The action states whose key counts the module phase: the module bands,
/// as the cast-sequence driver's step and rand tracks key a frame.
const PHASE_KEYED_STATES: [u8; 3] = [0x35, 0x36, 0x70];

/// Before a tick: when the cast stands on the seed's key, set the world
/// stream to the recorded seed. Returns whether it did.
///
/// Outside the module bands the phase is not part of the key. Retail's
/// `ctx[+0x279]` keeps the last module's phase through the next action's
/// begin (`255`, or the arm a prior cast left it on), where a replay seated
/// from a save state reads `0`; matched on it, the cast-begin seat
/// (`0x28` with a stale phase) never fired and the film ran the engine's
/// own stream.
pub fn apply_cast_rand_seed(world: &mut World, s: &CastRandSeed) -> bool {
    if world.battle_ctx.action_state != s.st7
        || (PHASE_KEYED_STATES.contains(&s.st7) && world.casting.module_phase != s.phase)
    {
        return false;
    }
    world.rng_state = s.seed;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_env_form_round_trips() {
        let s = CastRandSeed {
            st7: 0x70,
            phase: 3,
            seed: 0x965F_5E2C,
        };
        assert_eq!(cast_rand_seed_from_env(&cast_rand_seed_to_env(&s)), Some(s));
        assert_eq!(cast_rand_seed_from_env("112:3"), None);
        assert_eq!(cast_rand_seed_from_env("112:3:1:9"), None);
    }

    /// The cast-begin key carries retail's stale `ctx[+0x279]`; outside a
    /// module band the seat fires on the state alone, inside one on the phase.
    #[test]
    fn the_phase_keys_only_the_module_bands() {
        let mut w = World::new();
        w.battle_ctx.action_state = 0x28;
        w.casting.module_phase = 0;
        let begin = CastRandSeed {
            st7: 0x28,
            phase: 0xFF,
            seed: 7,
        };
        assert!(apply_cast_rand_seed(&mut w, &begin));
        assert_eq!(w.rng_state, 7);
        w.battle_ctx.action_state = 0x70;
        w.casting.module_phase = 2;
        let arm = CastRandSeed {
            st7: 0x70,
            phase: 3,
            seed: 9,
        };
        assert!(!apply_cast_rand_seed(&mut w, &arm));
        w.casting.module_phase = 3;
        assert!(apply_cast_rand_seed(&mut w, &arm));
        assert_eq!(w.rng_state, 9);
    }

    /// Retail's seed word on PROT 0962 `0xA4`'s last arm-3 vsync, in a
    /// `koru_strip_forced` cast, makes one more draw and then the sixteen
    /// rock picks a RAM read of the same cast holds: the stream is the
    /// world generator's.
    #[test]
    fn the_recorded_seed_predicts_the_rock_picks() {
        let mut s = 0x965F_5E2Cu32;
        let mut draw = || {
            s = legaia_engine_system::rand::world_lcg_step(s);
            legaia_engine_system::rand::bios_rand_shape(s)
        };
        draw();
        let picks: Vec<u32> = (0..16).map(|_| draw() % 4).collect();
        assert_eq!(picks, [0, 2, 2, 0, 2, 2, 1, 3, 0, 3, 2, 3, 1, 1, 0, 0]);
    }
}

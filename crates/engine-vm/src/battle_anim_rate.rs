//! Battle **arts slow-motion**: the per-actor animation-rate scalar
//! (`actor[+0x21D]`) and the anim-commit arms that drive it.
//!
//! Retail's arts presentation slows the whole battle clock while an art
//! plays, and the mechanism is one byte per actor. The SCUS anim tick
//! `FUN_80047430` advances each render node's 12.4 anim cursor by
//! `(DAT_1F800393 * actor[+0x21D] * clip[+0x78]) >> 1` per game frame
//! (`>> 2` on the idle branch, `0x800476EC..0x80047764`), so the byte is a
//! per-actor time scale with `8` as normal speed. Every animation-driven
//! edge - swing pacing, root motion, the strike loop's per-clip gate -
//! stretches with it.
//!
//! The writers, all in the staged-anim commit `FUN_8004AD80`
//! (`see ghidra/scripts/funcs/8004ad80.txt`) and all on the **party** ladder
//! (the monster path branches clear of the whole block at `0x8004B6F4`):
//!
//! * **Any commit** on an actor whose rate is not `8` writes `4`
//!   (`0x8004B080..0x8004B090`) - a frozen or quarter-speed actor whose next
//!   clip commits rises to half speed.
//! * **The Super / Miracle SpecialStarter** (staged id `0x1A`): every actor
//!   slot is frozen (`rate 0`, loop at `0x8004B728..0x8004B748`) and the
//!   acting actor alone runs at quarter speed (`rate 2`, `0x8004B750`) - the
//!   dramatic freeze-frame dash. The same arm raises the `ARTS!!` banner
//!   byte `ctx[+0x28B]` and queues the per-character arts shout.
//! * **An art action constant** (staged id `>= 0x1B`, the ids the strike
//!   loop stages for each art in the chain): every actor slot drops to
//!   `rate 2` when the context's action-in-progress marker `ctx[+0x243]` is
//!   set, else `rate 4` (`0x8004BB78..0x8004BBA8`) - the whole-battle
//!   half-speed beat each art strike plays under.
//!
//! The restore is `FUN_801E93C8` (ported as
//! [`crate::battle_gauge_rearm::rearm_gauge`]), called from the action SM's
//! Done arm: once the acting actor's art clip has ended (party: current anim
//! id `< 0x10`; monster: committed record flag `+0x87 == 0`) every slot
//! returns to `8`.
//!
//! Direction-command swings (`0x0C..=0x0F`) and every other id below `0x1B`
//! stage no rate write, so a basic attack never slows the battle.
//!
//! PORT: FUN_8004AD80 (the rate-write arms; the id -> slot/record ladder is
//! `crate::anim_vm::resolve_staged_anim`, the reaction/commit body lives in
//! `engine-core`'s `commit_staged_battle_anim`)

/// Normal speed - the value `FUN_801E93C8` restores and the battle seating
/// (`FUN_800513F0`, from the scratchpad speed scalar `0x1F80037D`) seeds.
pub const RATE_NORMAL: u8 = 8;
/// Frozen (the SpecialStarter's everyone-else value).
pub const RATE_FROZEN: u8 = 0;
/// Quarter speed (the SpecialStarter's acting-actor value, and the strike
/// value under an armed `ctx[+0x243]`).
pub const RATE_QUARTER: u8 = 2;
/// Half speed (the strike value with `ctx[+0x243]` clear, and the decay
/// value any commit applies to a non-normal actor).
pub const RATE_HALF: u8 = 4;

/// First staged id that is an art **action constant** (the `0x1B..` band the
/// strike loop stages; `0x19` art starter and `0x1A` SpecialStarter sit
/// below it).
pub const ART_CONSTANT_BASE: u8 = 0x1B;
/// The Super / Miracle Art SpecialStarter staged id.
pub const SPECIAL_STARTER_ID: u8 = 0x1A;

/// The per-actor animation-rate byte (`actor[+0x21D]`), newtyped so
/// `Default` seeds [`RATE_NORMAL`] rather than a freeze.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimRate(pub u8);

impl Default for AnimRate {
    fn default() -> Self {
        Self(RATE_NORMAL)
    }
}

impl AnimRate {
    pub fn get(self) -> u8 {
        self.0
    }
}

/// What one staged-anim commit does to the battle's animation rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitRateEffect {
    /// No rate arm fired (id below `0x1A`, or a monster commit).
    None,
    /// Staged id `0x1A`: all slots [`RATE_FROZEN`], the acting actor
    /// [`RATE_QUARTER`].
    StarterFreeze,
    /// Staged id `>= 0x1B`: all slots to the carried rate
    /// ([`RATE_QUARTER`] when `ctx[+0x243]` is armed, else [`RATE_HALF`]).
    StrikeSlow { rate: u8 },
}

/// Resolve the rate arm a staged-anim commit fires - the
/// `0x8004B71C..0x8004BBB4` ladder keyed on the **raw** staged id (read
/// before the dynamic-slot rewrite). `is_party` gates the whole block
/// (retail: acting slot `< 3` at `0x8004B6F4`); `action_marker` is
/// `ctx[+0x243]`.
pub fn staged_commit_rate_effect(
    staged_raw: u8,
    is_party: bool,
    action_marker: bool,
) -> CommitRateEffect {
    if !is_party {
        return CommitRateEffect::None;
    }
    if staged_raw == SPECIAL_STARTER_ID {
        return CommitRateEffect::StarterFreeze;
    }
    if staged_raw >= ART_CONSTANT_BASE {
        return CommitRateEffect::StrikeSlow {
            rate: if action_marker {
                RATE_QUARTER
            } else {
                RATE_HALF
            },
        };
    }
    CommitRateEffect::None
}

/// The unconditional per-commit decay (`0x8004B080..0x8004B090`): a
/// committing actor whose rate is not [`RATE_NORMAL`] rises/falls to
/// [`RATE_HALF`]. Runs **before** the staged-id arms, so a starter commit
/// still freezes afterwards.
pub fn commit_rate_decay(rate: AnimRate) -> AnimRate {
    if rate.0 == RATE_NORMAL {
        rate
    } else {
        AnimRate(RATE_HALF)
    }
}

/// Scale a fixed-point anim-cursor step by the actor's rate, mirroring the
/// retail advance `(dt * rate * clip_rate) >> 1` (non-idle) / `>> 2` (idle)
/// against the engine's base step (which equals the retail idle advance at
/// [`RATE_NORMAL`]): `step * rate * (idle ? 1 : 2) / 8`.
pub fn scaled_anim_step(base_step: u32, rate: AnimRate, idle: bool) -> u32 {
    let mult = u32::from(rate.0) * if idle { 1 } else { 2 };
    base_step * mult / 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rate_is_normal_speed() {
        assert_eq!(AnimRate::default().get(), RATE_NORMAL);
    }

    #[test]
    fn swings_and_starters_below_0x1a_fire_no_arm() {
        for id in [0u8, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x19] {
            assert_eq!(
                staged_commit_rate_effect(id, true, false),
                CommitRateEffect::None,
                "id {id:#04x}"
            );
        }
    }

    #[test]
    fn special_starter_freezes_the_battle() {
        assert_eq!(
            staged_commit_rate_effect(SPECIAL_STARTER_ID, true, false),
            CommitRateEffect::StarterFreeze
        );
    }

    #[test]
    fn art_constants_slow_everyone_by_the_action_marker() {
        assert_eq!(
            staged_commit_rate_effect(0x1B, true, false),
            CommitRateEffect::StrikeSlow { rate: RATE_HALF }
        );
        assert_eq!(
            staged_commit_rate_effect(0x2B, true, true),
            CommitRateEffect::StrikeSlow { rate: RATE_QUARTER }
        );
    }

    #[test]
    fn monster_commits_never_slow_the_battle() {
        assert_eq!(
            staged_commit_rate_effect(SPECIAL_STARTER_ID, false, false),
            CommitRateEffect::None
        );
        assert_eq!(
            staged_commit_rate_effect(0x2B, false, true),
            CommitRateEffect::None
        );
    }

    #[test]
    fn commit_decay_lifts_only_non_normal_rates() {
        assert_eq!(commit_rate_decay(AnimRate(RATE_NORMAL)).get(), RATE_NORMAL);
        assert_eq!(commit_rate_decay(AnimRate(RATE_FROZEN)).get(), RATE_HALF);
        assert_eq!(commit_rate_decay(AnimRate(RATE_QUARTER)).get(), RATE_HALF);
    }

    #[test]
    fn scaled_step_matches_the_retail_shift_pair() {
        // Base step is the retail idle advance at rate 8: unchanged.
        assert_eq!(scaled_anim_step(64, AnimRate(8), true), 64);
        // Non-idle clips run double the idle branch (>>1 vs >>2).
        assert_eq!(scaled_anim_step(64, AnimRate(8), false), 128);
        // Half / quarter / frozen.
        assert_eq!(scaled_anim_step(64, AnimRate(4), false), 64);
        assert_eq!(scaled_anim_step(64, AnimRate(2), false), 32);
        assert_eq!(scaled_anim_step(64, AnimRate(0), false), 0);
    }
}

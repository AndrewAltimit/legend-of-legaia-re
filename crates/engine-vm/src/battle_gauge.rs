//! Battle HP/MP gauge-fill colour selector.
//!
//! PORT: FUN_80046A20 - HP/MP gauge colour-threshold selector (fragment)
//!
//! `FUN_80046A20` (`SCUS_942.54`, `funcs/80046a20.txt`) is the battle-scene
//! per-frame tick: it advances timers, drives the fade/hit-flash state machines,
//! streams the next monster archive, and refreshes the on-screen party gauges.
//! The bulk of it is render-packet fan-out and asset streaming that needs the
//! full battle runtime, so only the one self-contained arithmetic kernel is
//! lifted here: the decision that maps a party actor's current/maximum HP and MP
//! onto the gauge's fill-colour index.
//!
//! Per the disassembly at `0x80046AA8..0x80046D0C`, for each of the party actor
//! slots the tick reads:
//!
//! ```text
//!   cur_hp = *(u16)(actor + 0x172)
//!   max_hp = *(u16)(actor + 0x14E)
//!   cur_mp = *(u16)(actor + 0x174)
//!   max_mp = *(u16)(actor + 0x152)
//!   flag   = *(u16)(actor + 0x16E)   ; non-zero forces the whole gauge grey
//! ```
//!
//! and writes a colour index into each of four gauge-primitive slots
//! (`DAT_801C8FA0[..]`). The colour codes are the immediates loaded into
//! `t4..t8` at the top of the function:
//!
//! | code | retail reg | meaning                                    |
//! | ---- | ---------- | ------------------------------------------ |
//! | 2    | `t8`       | actor is dead (`cur_hp == 0`)              |
//! | 3    | `t7`       | status override active (`flag != 0`)       |
//! | 7    | `t4`       | fill `> 1/2` of maximum                     |
//! | 6    | `t5`       | fill `> 1/4` (and `<= 1/2`) of maximum      |
//! | 9    | `t6`       | fill `<= 1/4` of maximum                    |
//!
//! The thresholds are the exact retail comparisons: `(max >> 1) < cur` for the
//! high band and `(max >> 2) < cur` for the mid band, using unsigned `sltu`, so
//! they are floor-of-half and floor-of-quarter with a strict `<`.
//!
//! When the actor is dead every gauge slot is forced to `2`; when the status
//! flag is set (and the actor is alive) every slot is forced to `3`; otherwise
//! the HP and MP bars are coloured independently by their own fill ratio.
//!
//! REF: the engine's battle HUD is not yet wired (see `docs/subsystems/battle.md`);
//! this is a faithful, side-effect-free mirror of the retail threshold arithmetic
//! carrying no Sony bytes, in the same spirit as `battle_camera` / `battle_formulas`.

/// Gauge fill-colour index: actor is dead (`cur_hp == 0`).
pub const GAUGE_DEAD: u8 = 2;
/// Gauge fill-colour index: a status override is active (`flag != 0`).
pub const GAUGE_STATUS: u8 = 3;
/// Gauge fill-colour index: fill `> 1/2` of maximum.
pub const GAUGE_HIGH: u8 = 7;
/// Gauge fill-colour index: fill `> 1/4` (and `<= 1/2`) of maximum.
pub const GAUGE_MID: u8 = 6;
/// Gauge fill-colour index: fill `<= 1/4` of maximum.
pub const GAUGE_LOW: u8 = 9;

/// Colour index for a single bar from its current/maximum fill.
///
/// Mirrors the retail `(max >> 1) < cur` / `(max >> 2) < cur` unsigned
/// comparisons: strictly greater than floor-half yields [`GAUGE_HIGH`], strictly
/// greater than floor-quarter yields [`GAUGE_MID`], otherwise [`GAUGE_LOW`].
///
/// Note this is the *bar-fill* band only - it does not encode the dead or
/// status-override cases, which the whole-gauge selector applies first. A dead
/// actor (`cur == 0`) still lands in [`GAUGE_LOW`] here, matching retail, where
/// the `cur == 0` case is short-circuited before this ratio is ever evaluated.
#[inline]
pub fn bar_fill_color(cur: u16, max: u16) -> u8 {
    if (max >> 1) < cur {
        GAUGE_HIGH
    } else if (max >> 2) < cur {
        GAUGE_MID
    } else {
        GAUGE_LOW
    }
}

/// PORT: FUN_80046A20 - select the `(hp_color, mp_color)` gauge-fill indices for
/// one party actor.
///
/// - `cur_hp` / `max_hp`: actor fields `+0x172` / `+0x14E`.
/// - `cur_mp` / `max_mp`: actor fields `+0x174` / `+0x152`.
/// - `status_flag`: actor field `+0x16E`; when non-zero the whole gauge is
///   forced to [`GAUGE_STATUS`] (retail colour `3`).
///
/// Precedence follows the retail branch order: death (`cur_hp == 0`) first, then
/// the status override, then per-bar fill ratios.
pub fn gauge_colors(
    cur_hp: u16,
    max_hp: u16,
    cur_mp: u16,
    max_mp: u16,
    status_flag: u16,
) -> (u8, u8) {
    if cur_hp == 0 {
        (GAUGE_DEAD, GAUGE_DEAD)
    } else if status_flag != 0 {
        (GAUGE_STATUS, GAUGE_STATUS)
    } else {
        (
            bar_fill_color(cur_hp, max_hp),
            bar_fill_color(cur_mp, max_mp),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_actor_forces_both_bars_grey() {
        // cur_hp == 0 wins regardless of MP or status flag.
        assert_eq!(gauge_colors(0, 100, 40, 40, 0), (GAUGE_DEAD, GAUGE_DEAD));
        assert_eq!(gauge_colors(0, 100, 40, 40, 5), (GAUGE_DEAD, GAUGE_DEAD));
    }

    #[test]
    fn status_flag_forces_both_bars_when_alive() {
        assert_eq!(
            gauge_colors(80, 100, 40, 40, 1),
            (GAUGE_STATUS, GAUGE_STATUS)
        );
        // Any non-zero flag value, not just 1.
        assert_eq!(
            gauge_colors(1, 100, 0, 40, 0x1234),
            (GAUGE_STATUS, GAUGE_STATUS)
        );
    }

    #[test]
    fn high_band_is_strictly_above_floor_half() {
        // max=100 -> floor-half = 50. cur must be > 50 for HIGH.
        assert_eq!(bar_fill_color(51, 100), GAUGE_HIGH);
        assert_eq!(bar_fill_color(50, 100), GAUGE_MID); // exactly half -> not HIGH
        assert_eq!(bar_fill_color(100, 100), GAUGE_HIGH);
    }

    #[test]
    fn mid_band_is_strictly_above_floor_quarter() {
        // max=100 -> floor-quarter = 25. cur in (25, 50] -> MID.
        assert_eq!(bar_fill_color(26, 100), GAUGE_MID);
        assert_eq!(bar_fill_color(25, 100), GAUGE_LOW); // exactly quarter -> not MID
        assert_eq!(bar_fill_color(50, 100), GAUGE_MID);
    }

    #[test]
    fn low_band_at_or_below_quarter() {
        assert_eq!(bar_fill_color(25, 100), GAUGE_LOW);
        assert_eq!(bar_fill_color(1, 100), GAUGE_LOW);
    }

    #[test]
    fn floor_thresholds_use_integer_shift() {
        // max=7 -> >>1 = 3, >>2 = 1. cur>3 HIGH, cur in (1,3] MID, cur<=1 LOW.
        assert_eq!(bar_fill_color(4, 7), GAUGE_HIGH);
        assert_eq!(bar_fill_color(3, 7), GAUGE_MID);
        assert_eq!(bar_fill_color(2, 7), GAUGE_MID);
        assert_eq!(bar_fill_color(1, 7), GAUGE_LOW);
    }

    #[test]
    fn hp_and_mp_are_coloured_independently() {
        // Full HP, near-empty MP.
        assert_eq!(gauge_colors(100, 100, 1, 100, 0), (GAUGE_HIGH, GAUGE_LOW));
        // Near-empty HP, full MP.
        assert_eq!(gauge_colors(10, 100, 100, 100, 0), (GAUGE_LOW, GAUGE_HIGH));
        // Mid HP, mid MP.
        assert_eq!(gauge_colors(40, 100, 40, 100, 0), (GAUGE_MID, GAUGE_MID));
    }

    #[test]
    fn zero_max_bar_never_panics_and_reads_low_when_alive() {
        // max=0 -> both shifts are 0; any cur>0 is > 0 so HIGH.
        assert_eq!(bar_fill_color(1, 0), GAUGE_HIGH);
        // cur==0/max==0 -> LOW from the ratio, but the gauge selector maps
        // cur_hp==0 to DEAD before the ratio runs.
        assert_eq!(bar_fill_color(0, 0), GAUGE_LOW);
        assert_eq!(gauge_colors(0, 0, 0, 0, 0), (GAUGE_DEAD, GAUGE_DEAD));
    }
}

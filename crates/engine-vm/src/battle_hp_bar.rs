//! The HP / MP **bar ramp** - the machinery that moves an actor's displayed
//! bar value toward its live stat, and the invariant the action SM's `0x51`
//! exit gate ([`crate::battle_action::hp_bar_drain_pending`], retail
//! `FUN_801E7250`) is written against.
//!
//! Three fields per battle actor participate:
//!
//! | retail | meaning | port |
//! |---|---|---|
//! | `+0x14C` | live HP - the authoritative value every liveness test reads | [`crate::battle_action::BattleActor::hp`] |
//! | `+0x172` | **displayed** HP - what the bar widget draws | [`crate::battle_action::BattleActor::hp_display`] |
//! | `+0x10` | signed **pending-delta accumulator** - how much `+0x172` still owes | [`crate::battle_action::BattleActor::hp_bar_pending`] |
//!
//! The pair only converges through the accumulator. Everything here is
//! transcribed from the DISASSEMBLY, not the decompiled C:
//!
//! * the drain, `ghidra/scripts/funcs/80047430.txt` `0x800474E8..0x80047638`;
//! * the accumulating seed, `overlay_battle_action_801ec3e4.txt`
//!   `0x801EDB44..0x801EDB80`;
//! * the assigning seeds, `800402f4.txt` `0x800408FC` / `0x80040D28` /
//!   `0x800410BC`;
//! * the re-sync, `overlay_battle_action_801e752c.txt` `0x801E7600` /
//!   `0x801E7698`.
//!
//! # Why the accumulator is load-bearing
//!
//! The whole ramp sits behind one guard - `0x800474E8`
//! (`lw a0,0x10(s2); beq a0,zero,<skip>`). With a zero accumulator the bar is
//! **not touched at all**. So `hp != hp_display` with a zero accumulator is an
//! absorbing state on a party slot: the drain is the only thing that moves the
//! bar, and every later damage or heal adds its delta to both sides, so the
//! constant offset rides along. The action SM then parks forever in state
//! `0x51` on any party-targeted action - the endless-camera-orbit softlock
//! class described in `docs/subsystems/battle-action.md`.
//!
//! The one re-sync in the dumped battle corpus is the per-round status ticker
//! [`resync_display`] (`FUN_801E752C`), which force-assigns the display from
//! live HP after each of its own HP writes.

/// One quarter-step of the ramp: `acc / 4`, biased **away from zero** so a
/// non-zero accumulator always produces a non-zero step and the sequence
/// terminates for either sign.
///
/// Retail computes `(acc + 3) / 4` for a positive accumulator (`0x80047504`,
/// with the signed-division fix-up at `0x80047508`/`0x80047510`) and
/// `(acc - 3) / 4` for a negative one (`0x80047538`/`0x80047544`), both with C
/// truncation. That is exactly "divide by four, round away from zero".
///
/// PORT: FUN_80047430 (the quarter-step divide)
pub fn quarter_step(acc: i32) -> i32 {
    match acc.cmp(&0) {
        std::cmp::Ordering::Greater => acc.wrapping_add(3) / 4,
        std::cmp::Ordering::Less => acc.wrapping_sub(3) / 4,
        std::cmp::Ordering::Equal => 0,
    }
}

/// The result of one frame of ramp on one bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarStep {
    /// The new displayed value.
    pub display: u16,
    /// The accumulator remainder.
    pub pending: i32,
}

/// One frame of **party-slot** bar ramp: a quarter of the outstanding delta
/// moves out of the accumulator and into the displayed value.
///
/// Retail (`0x80047500..0x80047574`) subtracts the same step from both:
/// `+0x172 -= step` (halfword store, so it wraps) and `+0x10 -= step`.
///
/// PORT: FUN_80047430 (party arm of the HP-bar ramp)
pub fn party_bar_step(display: u16, pending: i32) -> BarStep {
    if pending == 0 {
        return BarStep { display, pending };
    }
    let step = quarter_step(pending);
    BarStep {
        display: display.wrapping_sub(step as u16),
        pending: pending - step,
    }
}

/// One frame of **monster-slot** bar ramp: the whole delta lands at once and
/// the accumulator is cleared.
///
/// Retail (`0x80047578..0x80047588`) reads the accumulator with `lhu` - the
/// **low halfword only** - subtracts it from the displayed value and stores
/// `0` back over the full word. That single-frame settle is the second reason
/// a monster target can never hold the `0x51` exit gate.
///
/// PORT: FUN_80047430 (monster arm of the HP-bar ramp)
pub fn monster_bar_step(display: u16, pending: i32) -> BarStep {
    if pending == 0 {
        return BarStep { display, pending };
    }
    BarStep {
        display: display.wrapping_sub(pending as u16),
        pending: 0,
    }
}

/// One frame of ramp for a slot, picking the arm retail picks: slots `0..=2`
/// are party slots and ramp a quarter at a time, everything else settles in
/// one frame.
///
/// The slot test is retail's `sltiu v0,s1,0x3` at `0x800474F4`.
///
/// PORT: FUN_80047430 (the `slot < 3` arm select)
pub fn bar_step_for_slot(slot: u8, display: u16, pending: i32) -> BarStep {
    if slot < 3 {
        party_bar_step(display, pending)
    } else {
        monster_bar_step(display, pending)
    }
}

/// **Accumulating** seed - the convention the Arms execution resolver
/// `FUN_801EC3E4` uses when a hit lands.
///
/// Retail `0x801EDB44..0x801EDB80`:
///
/// ```text
/// 801edb4c  lw   v0,0x10(v1)      ; acc
/// 801edb54  addu v0,v0,a0         ; acc += delta
/// 801edb58  sw   v0,0x10(v1)
/// 801edb64  lhu  a1,0x172(v1)     ; bar
/// 801edb68  lw   v0,0x10(v1)
/// 801edb70  slt  v0,a1,v0         ; bar < acc ?
/// 801edb7c  sw   a1,0x10(v1)      ;   -> acc = bar   (anti-overkill clamp)
/// ```
///
/// The clamp is what stops a lethal hit from asking the bar to travel further
/// than it can: the accumulator can never exceed the value currently on the
/// bar, so the ramp lands exactly on zero rather than wrapping through it.
/// Because the accumulator *adds*, a second hit landing mid-ramp keeps the
/// remainder of the first.
///
/// PORT: FUN_801EC3E4 (the `actor[+0x10]` accumulate + anti-overkill clamp)
pub fn accumulate_pending(pending: i32, display: u16, delta: i32) -> i32 {
    let acc = pending.wrapping_add(delta);
    let bar = i32::from(display);
    if bar < acc { bar } else { acc }
}

/// **Assigning** seed - the convention the damage-application primitive
/// `FUN_800402F4` uses.
///
/// `delta` is the **signed change applied to the stat**, the same `s4` the
/// routine folds into the stat halfword a few instructions earlier
/// (`0x800408A8`: `lhu v0,0x0(v1); addu v0,v0,s4; sh v0,0x0(v1)`) - so damage
/// arrives negative and a heal positive. Negating it gives the accumulator the
/// same positive-means-the-bar-falls sense [`accumulate_pending`] uses.
///
/// All three seed sites (`0x800408FC`, `0x80040D28`, `0x800410BC`) are
/// the same three instructions - sign-extend the halfword delta, negate,
/// store:
///
/// ```text
/// 800408e8  sll  v0,s4,0x10
/// 800408ec  sra  v0,v0,0x10      ; (i16) delta
/// 800408f4  subu v0,zero,v0      ; -delta
/// 800408fc  sw   v0,0x10(v1)     ; assign, not accumulate
/// ```
///
/// An assigning seed that lands while an accumulated drain is still in flight
/// **discards the remainder**, and the bar then stops short of live HP by
/// exactly that much - the desync shape the softlock needs. Which retail
/// sequence actually reaches that state is Unknown; no capture has produced it
/// without an external HP write.
///
/// PORT: FUN_800402F4 (the `actor[+0x10] = -delta` seeds)
pub fn assign_pending(delta: i16) -> i32 {
    -i32::from(delta)
}

/// The one re-sync in the dumped battle corpus: force-assign the displayed
/// value from live HP and drop any outstanding accumulator.
///
/// The per-round status ticker `FUN_801E752C` does this immediately after each
/// of its own HP writes - `0x801E7600` (the Venom arm) and `0x801E7698` (the
/// Toxic arm), both `lhu v1,0x14c(v0); sh v1,0x172(v0)`. It is why a desynced
/// party actor recovers as soon as a DoT ticks on it, and therefore why the
/// softlock is survivable rather than terminal for a statused party.
///
/// Retail writes only `+0x172`; the accumulator is left alone. The port clears
/// it too, because leaving a stale accumulator behind a freshly-assigned bar
/// would immediately re-open the mismatch the re-sync exists to close - retail
/// gets away with it because its ticker runs at a round boundary where no
/// drain is in flight.
///
/// PORT: FUN_801E752C (the `+0x172 = +0x14C` re-sync stores)
pub fn resync_display(hp: u16) -> BarStep {
    BarStep {
        display: hp,
        pending: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_step_rounds_away_from_zero() {
        // Positive: (acc+3)/4.
        assert_eq!(quarter_step(1), 1);
        assert_eq!(quarter_step(4), 1);
        assert_eq!(quarter_step(5), 2);
        assert_eq!(quarter_step(100), 25);
        // Negative: (acc-3)/4 == acc>>2 for acc < 0.
        assert_eq!(quarter_step(-1), -1);
        assert_eq!(quarter_step(-4), -1);
        assert_eq!(quarter_step(-5), -2);
        assert_eq!(quarter_step(-100), -25);
        assert_eq!(quarter_step(0), 0);
    }

    #[test]
    fn party_ramp_terminates_on_exactly_the_seeded_delta() {
        for seed in [1i32, 2, 3, 7, 40, 137, 999, 9999] {
            for sign in [1i32, -1] {
                let acc = seed * sign;
                let start: u16 = 5000;
                let mut st = BarStep {
                    display: start,
                    pending: acc,
                };
                let mut frames = 0;
                while st.pending != 0 {
                    st = party_bar_step(st.display, st.pending);
                    frames += 1;
                    assert!(frames < 200, "ramp did not terminate for acc={acc}");
                }
                // Total bar movement equals the seeded accumulator exactly.
                assert_eq!(
                    st.display,
                    start.wrapping_sub(acc as u16),
                    "acc={acc} settled short"
                );
            }
        }
    }

    #[test]
    fn monster_ramp_settles_in_one_frame() {
        let st = monster_bar_step(300, 120);
        assert_eq!(st.display, 180);
        assert_eq!(st.pending, 0);
        // The arm select: slot 3 is the first monster slot.
        assert_eq!(bar_step_for_slot(3, 300, 120), st);
        assert_ne!(bar_step_for_slot(2, 300, 120), st);
    }

    #[test]
    fn zero_accumulator_does_not_touch_the_bar() {
        // The `0x800474E8` guard: this is the absorbing state.
        let st = bar_step_for_slot(0, 123, 0);
        assert_eq!(st.display, 123);
        assert_eq!(st.pending, 0);
    }

    #[test]
    fn accumulate_keeps_the_remainder_and_clamps_at_the_bar() {
        // A second hit landing mid-ramp keeps the first hit's remainder.
        assert_eq!(accumulate_pending(30, 500, 20), 50);
        // Anti-overkill: the accumulator can never exceed the drawn bar.
        assert_eq!(accumulate_pending(0, 40, 999), 40);
        // A heal (negative delta) is not clamped - `bar < acc` is false.
        assert_eq!(accumulate_pending(0, 40, -60), -60);
    }

    #[test]
    fn assign_discards_the_in_flight_remainder() {
        // The shape that produces the desync: 30 still owed, a fresh
        // assigning seed of 12 damage (stat delta -12) replaces it outright
        // instead of adding to it.
        let pending = 30i32;
        assert_eq!(assign_pending(-12), 12);
        assert_ne!(assign_pending(-12), accumulate_pending(pending, 500, 12));
        // A heal (stat delta +20) drives the bar upward.
        assert_eq!(assign_pending(20), -20);
    }

    #[test]
    fn resync_closes_a_desync() {
        let st = resync_display(77);
        assert_eq!(st.display, 77);
        assert_eq!(st.pending, 0);
    }
}

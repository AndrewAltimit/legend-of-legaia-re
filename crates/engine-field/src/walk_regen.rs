//! Field walk-regen tick - the "recover while walking" accessory passives.
//!
//! Retail `FUN_801D0B90` (dialog overlay,
//! `ghidra/scripts/funcs/overlay_dialog_801d0b90.txt`) drains an accumulated
//! step counter (`_DAT_801F2274`, `0x20` per tick) and, for each party member
//! (count at `0x80084594`, member ids at `0x80084598`, records at stride
//! `0x414`), applies three independent flag-gated restore bumps. The gates
//! are bits `24..26` of the u32 at record `+0xF8` - word 1 of the 4-word
//! ability bitfield at `+0xF4` that the per-frame stat aggregator
//! `FUN_80042558` rebuilds from the equipped accessories - i.e. the
//! accessory-passive indices `0x38` / `0x39` / `0x3A`: **HP Walk** (Life
//! Source), **MP Walk** (Magic Source), **AP Walk** (Mettle Source). See
//! [`docs/formats/accessory-passive-table.md`].
//!
//! Each bump raises the pool's *current* u16 (record `+0x106` / `+0x10A` /
//! `+0x10E`) by a fixed step (8 / 2 / 1) and clamps it at the sibling u16
//! (`+0x104` / `+0x108` / `+0x10C`) - the per-frame **effective maximum**,
//! which `FUN_80042558` recomputes each frame from the base stats at
//! `+0x11C..` plus the percentage passives (so a `+10%` HP accessory raises
//! the regen ceiling too).
//!
//! The tail decrements a second counter (`_DAT_8007B600`) and, when it
//! reaches zero, arms a dialog-window callback (`_DAT_8007B450 =
//! 0x801F2278`, ctx flag `|= 0x80000`, list registration via
//! `FUN_80020DE0`). The port surfaces that edge as the return value; the
//! window descriptor itself is host-owned.

/// Step budget one tick consumes from the accumulated walk counter
/// (retail `_DAT_801F2274`). The tick only runs while the counter *exceeds*
/// this value.
pub const WALK_REGEN_STEP_COST: i32 = 0x20;

/// Ability-bitfield masks tested against the u32 at record `+0xF8` (word 1
/// of the `+0xF4` bitfield): bit `0x38 - 32 = 24` etc.
pub const HP_WALK_MASK: u32 = 0x0100_0000;
/// Ability bit `0x39` (MP Walk / Magic Source) as a word-1 mask.
pub const MP_WALK_MASK: u32 = 0x0200_0000;
/// Ability bit `0x3A` (AP Walk / Mettle Source) as a word-1 mask.
pub const AP_WALK_MASK: u32 = 0x0400_0000;

/// Per-tick restore steps: HP +8, MP +2, AP +1.
pub const HP_WALK_STEP: u16 = 8;
/// MP restore per tick.
pub const MP_WALK_STEP: u16 = 2;
/// AP (Spirit) restore per tick.
pub const AP_WALK_STEP: u16 = 1;

/// One resource pool as the tick sees it: `value` is the current amount
/// (record `+0x106` / `+0x10A` / `+0x10E`), `cap` the effective maximum the
/// stat aggregator recomputes per frame (`+0x104` / `+0x108` / `+0x10C`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WalkGauge {
    /// Current pool value (the bumped cell).
    pub value: u16,
    /// Effective maximum (the clamp cell).
    pub cap: u16,
}

impl WalkGauge {
    /// The retail bump: `value += step` (u16 wrap), then clamp down to `cap`
    /// whenever the result exceeds it - including a value already above the
    /// cap (a stale current after an effective-max drop snaps down).
    fn bump(&mut self, step: u16) {
        self.value = self.value.wrapping_add(step);
        if self.cap < self.value {
            self.value = self.cap;
        }
    }
}

/// One party member's view for the tick: the ability-bitfield word the
/// three gates test plus the HP / MP / AP gauge pairs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WalkRegenMember {
    /// Word 1 of the ability bitfield (u32 at record `+0xF8`; carries the
    /// walk-passive bits `0x38..=0x3A` as masks `0x1000000..0x4000000`).
    pub ability_hi: u32,
    /// HP pair (`+0x104` cap / `+0x106` current).
    pub hp: WalkGauge,
    /// MP pair (`+0x108` cap / `+0x10A` current).
    pub mp: WalkGauge,
    /// AP pair (`+0x10C` cap / `+0x10E` current).
    pub ap: WalkGauge,
}

// PORT: FUN_801D0B90 - dialog-overlay walk-regen tick: drains the step
// counter by 0x20 per call (only while it exceeds 0x20), applies the three
// ability-gated (step, cap) restore bumps per party member, and reports the
// dialog-window-callback edge of the secondary countdown.
/// One tick of the walk-regen state machine.
///
/// - `step_counter` mirrors `_DAT_801F2274`: the tick is a no-op unless the
///   counter exceeds [`WALK_REGEN_STEP_COST`]; when it runs, the cost is
///   subtracted first.
/// - `members` are the party's records in member-id order (the retail
///   `0x80084598` indirection resolved by the caller).
/// - `window_countdown` mirrors `_DAT_8007B600`: decremented once per
///   running tick when non-zero.
///
/// Returns `true` exactly on the tick where `window_countdown` reaches
/// zero: the edge where retail arms the dialog-window callback (the
/// caller's analogue of the `_DAT_8007B450` / ctx-flag / `FUN_80020DE0`
/// writes).
pub fn tick_walk_regen(
    step_counter: &mut i32,
    members: &mut [WalkRegenMember],
    window_countdown: &mut i32,
) -> bool {
    if *step_counter <= WALK_REGEN_STEP_COST {
        return false;
    }
    *step_counter -= WALK_REGEN_STEP_COST;
    for m in members.iter_mut() {
        if m.ability_hi & HP_WALK_MASK != 0 {
            m.hp.bump(HP_WALK_STEP);
        }
        if m.ability_hi & MP_WALK_MASK != 0 {
            m.mp.bump(MP_WALK_STEP);
        }
        if m.ability_hi & AP_WALK_MASK != 0 {
            m.ap.bump(AP_WALK_STEP);
        }
    }
    if *window_countdown != 0 {
        *window_countdown -= 1;
        if *window_countdown == 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(ability_hi: u32) -> WalkRegenMember {
        WalkRegenMember {
            ability_hi,
            hp: WalkGauge {
                value: 10,
                cap: 100,
            },
            mp: WalkGauge { value: 10, cap: 50 },
            ap: WalkGauge { value: 10, cap: 25 },
        }
    }

    #[test]
    fn countdown_at_or_below_cost_is_a_no_op() {
        // Retail gate is `0x20 < counter`, so exactly 0x20 does NOT tick.
        let mut members = [member(HP_WALK_MASK | MP_WALK_MASK | AP_WALK_MASK)];
        for start in [0i32, 0x10, WALK_REGEN_STEP_COST] {
            let mut counter = start;
            let mut window = 5;
            assert!(!tick_walk_regen(&mut counter, &mut members, &mut window));
            assert_eq!(counter, start, "counter untouched when gated");
            assert_eq!(window, 5, "window countdown untouched when gated");
            assert_eq!(members[0].hp.value, 10, "no bump when gated");
        }
    }

    #[test]
    fn running_tick_subtracts_the_step_cost() {
        let mut members = [];
        let mut counter = 0x21;
        let mut window = 0;
        assert!(!tick_walk_regen(&mut counter, &mut members, &mut window));
        assert_eq!(counter, 1);
    }

    #[test]
    fn each_flag_bumps_its_own_gauge_by_its_own_step() {
        let cases = [
            (HP_WALK_MASK, [10 + 8, 10, 10]),
            (MP_WALK_MASK, [10, 10 + 2, 10]),
            (AP_WALK_MASK, [10, 10, 10 + 1]),
        ];
        for (mask, expect) in cases {
            let mut members = [member(mask)];
            let mut counter = 0x40;
            let mut window = 0;
            tick_walk_regen(&mut counter, &mut members, &mut window);
            let m = &members[0];
            assert_eq!(
                [m.hp.value, m.mp.value, m.ap.value],
                expect,
                "mask {mask:#x} bumps exactly its own gauge"
            );
        }
    }

    #[test]
    fn bumps_clamp_at_the_cap_cell() {
        let mut members = [WalkRegenMember {
            ability_hi: HP_WALK_MASK | MP_WALK_MASK | AP_WALK_MASK,
            hp: WalkGauge {
                value: 97,
                cap: 100,
            }, // 97 + 8 -> clamp 100
            mp: WalkGauge { value: 49, cap: 50 }, // 49 + 2 -> clamp 50
            ap: WalkGauge { value: 25, cap: 25 }, // full stays full
        }];
        let mut counter = 0x40;
        let mut window = 0;
        tick_walk_regen(&mut counter, &mut members, &mut window);
        assert_eq!(
            members[0].hp,
            WalkGauge {
                value: 100,
                cap: 100
            }
        );
        assert_eq!(members[0].mp, WalkGauge { value: 50, cap: 50 });
        assert_eq!(members[0].ap, WalkGauge { value: 25, cap: 25 });
    }

    #[test]
    fn ungated_member_is_untouched_and_members_are_independent() {
        let mut members = [member(0), member(HP_WALK_MASK)];
        let mut counter = 0x40;
        let mut window = 0;
        tick_walk_regen(&mut counter, &mut members, &mut window);
        assert_eq!(members[0].hp.value, 10, "no passive, no regen");
        assert_eq!(members[1].hp.value, 18);
    }

    #[test]
    fn window_countdown_arms_exactly_on_the_zero_edge() {
        let mut members = [];
        let mut counter = 0x200;
        let mut window = 3;
        assert!(!tick_walk_regen(&mut counter, &mut members, &mut window));
        assert_eq!(window, 2);
        assert!(!tick_walk_regen(&mut counter, &mut members, &mut window));
        assert!(tick_walk_regen(&mut counter, &mut members, &mut window));
        assert_eq!(window, 0);
        // Already-zero countdown never re-arms.
        assert!(!tick_walk_regen(&mut counter, &mut members, &mut window));
        assert_eq!(window, 0);
    }
}

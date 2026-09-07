//! Action-Point ("AP") gauge for Tactical Arts command input.
//!
//! Each character has a per-turn AP budget that limits how many art
//! commands they can chain. When the player is in command-input mode,
//! every art slot dropped into the queue spends the slotted art's AP
//! cost; the queue stops accepting input once the remaining budget would
//! go below zero. Pressing the Spirit button during the command window
//! adds a `+5` bonus exactly once per turn.
//!
//! # This is an approximation, and it counts the wrong units
//!
//! Retail has no gauge shaped like this one. Its command gauge is a pool
//! at `ctx + 0x6DC` seeded from the acting actor's **AGL** (`actor +
//! 0x154`) - hundreds, not single digits - and each *direction* command
//! spends the per-`(character, weapon)` `+0x74` byte out of it (`0x1E` =
//! 30 for a favored weapon, 42 off-class, 54 for the Astral Sword). An
//! *art* is not paid from that pool at all: it is charged to the Spirit
//! gauge `actor + 0x170`.
//!
//! This module inverts both halves - directions are free here and art
//! units cost `1` - and its `4 + level/10` base is a fitted constant
//! that approximates `AGL / 30`, the command count a favored weapon
//! buys. Because it cannot vary with the equipped weapon, the whole
//! weapon-specialty mechanic is invisible to it.
//!
//! The full comparison, the disassembly that pins the pool source, and
//! what a faithful port needs (a reader for the `+0x74` byte in the
//! player battle files, which `legaia_asset::battle_data_pack`
//! documents but does not parse) are in
//! `docs/subsystems/arts-command-gauge.md`.
//!
//! REF: FUN_801D388C (gauge build: pool <- actor `+0x154`, cost <- `+0x74`)
//!
//! ## What lives where
//!
//! - This module owns the per-character AP state.
//! - The action-cost lookup ([`art_ap_cost`]) is a pure function - no
//!   shared state. It mirrors the per-action-byte AP table the retail
//!   engine reads from.
//! - [`ApGauge::can_afford`] is the gate the action-validator should
//!   call before committing a queued art.
//!
//! ## What it does NOT model
//!
//! - The mid-turn refund quirk (cancel-then-redo eats 1 AP in retail
//!   even though no commit fired) - engines that want this can wrap.
//! - Equipment that grants extra AP (e.g. some accessories give +1 base
//!   AP). Engines fold those into [`ApGauge::set_base_ap`].

use legaia_art::queue::{ActionConstant, ActionQueue};

/// Default base AP for level-1 characters. Approximates the command
/// count a level-1 favored-weapon turn buys (`AGL / 30`); see the module
/// note on units. Engines targeting non-vanilla balance can override.
pub const DEFAULT_BASE_AP: u8 = 4;

/// AP added when the player presses the Spirit button during command
/// input. Engine constant in this module's command units, not a pinned
/// retail value.
pub const SPIRIT_AP_BONUS: u8 = 5;

/// The three code immediates the party arts queue-builder picks its Spirit
/// multiplier from, keyed on how many of the caster's art rows it has
/// already visited: `li t4,0xb` at `0x801EF328`, `li t4,0xa` at
/// `0x801EF32C`, `li t4,0x6` at `0x801EF33C`
/// (`see ghidra/scripts/funcs/overlay_battle_action_801eed1c.txt`).
const ART_SPIRIT_MULTIPLIERS: [u16; 3] = [11, 10, 6];

/// **What an art body costs, in Spirit.** This is a different currency from
/// the rest of this module: the direction commands are spent out of the
/// per-turn command pool (`ctx+0x6DC`, seeded from AGL), while the *art* is
/// charged to the caster's Spirit gauge `actor[+0x170]`. Conflating the two
/// is the standing trap in this area - see
/// `docs/subsystems/arts-command-gauge.md` § What an art costs in AP.
///
/// **Retail stores no per-art AP cost anywhere on the disc.** The party arts
/// queue-builder `FUN_801EED1C` computes it from three code immediates keyed
/// on `rows_visited` - the art's ordinal among the rows the builder has
/// already walked for this character (`[sp+0x40]`, zeroed at `0x801EF300`,
/// bumped once per row at `0x801EF844`), **not** the art's display index. The
/// two differ whenever a character's grid skips rows: Noa's display-index-4
/// Vulture Blade is her third *visited* row, which is why it carries `10 x 5`
/// and not `6 x 5`.
///
/// The multiplier is halved first (`srl t4,t4,0x1` at `0x801EF378`, an
/// integer shift - so `11` and `10` both become `5`) when the actor's `0x800`
/// flag is set, and only then multiplied by the art's command count
/// (`mult t4,s1` / `mflo t7` at `0x801EF40C` for the affordability gate,
/// `mult t4,v0` / `mflo a2` at `0x801EF474` for the charge).
///
/// PORT: FUN_801EED1C (`0x801EF328`..`0x801EF474`, the Spirit-cost half)
pub fn art_spirit_cost(rows_visited: usize, command_count: u8, halved: bool) -> u16 {
    let mut mult = match rows_visited {
        0 => ART_SPIRIT_MULTIPLIERS[0],
        1..=3 => ART_SPIRIT_MULTIPLIERS[1],
        _ => ART_SPIRIT_MULTIPLIERS[2],
    };
    if halved {
        mult >>= 1;
    }
    mult * u16::from(command_count)
}

/// Total Spirit an entered turn charges: [`art_spirit_cost`] summed over the
/// arts the turn actually performs, each at its own ordinal in `catalog` -
/// the accumulator retail keeps in `actor[+0x224]` and spends once, at
/// `0x801E5D74` (`subu v0,v0,a0` / `sh v0,0x170(s3)`) in the battle-action
/// cleanup arm. The in-builder debit at `0x801EF498` is **not** the spend:
/// the builder's own tail undoes it at `0x801EF988`, and its only job is to
/// make a chained art's affordability gate account for what the earlier arts
/// in the same run already committed.
///
/// `catalog` is the caster's art rows **in the builder's walk order**
/// (ascending action constant, every row it visits, including the Miracle at
/// ordinal 0), paired with each row's command count. `performed` names the
/// arts the queue actually runs, in performed order; an art performed twice
/// is charged twice, at the same ordinal both times.
///
/// PORT: FUN_801E295C (state `0x50` cleanup: `+0x224` -> Spirit)
pub fn arts_turn_spirit_cost(
    catalog: &[(ActionConstant, u8)],
    performed: &[ActionConstant],
    halved: bool,
) -> u16 {
    performed
        .iter()
        .filter_map(|action| {
            let row = catalog.iter().position(|(a, _)| a == action)?;
            Some(art_spirit_cost(row, catalog[row].1, halved))
        })
        .fold(0u16, |acc, c| acc.saturating_add(c))
}

/// AP cost per [`ActionConstant`] when added to the queue.
///
/// Mapping derived from the action-constant catalogue:
///
/// | Range            | Cost | Notes |
/// |------------------|------|-------|
/// | `0x00` Nothing   | 0    | placeholder |
/// | `0x01..=0x05`    | 0    | system actions (Item / Magic / Attack / Spirit / Escape) |
/// | `0x06..=0x10`    | 0    | reserved animation slots |
/// | `0x11..=0x18`    | 0    | "Empty Slot" placeholders never appear in the queue |
/// | `0x19` Reg start | 1    | Regular Art Starter |
/// | `0x1A` Spc start | 1    | Special Art Starter |
/// | `0x1B..=0x32`    | 1    | per-character art body (each art unit costs 1 AP) |
/// | direction bytes  | 0    | Left / Right / Down / Up are free in the queue |
///
/// Direction bytes (`0x0C..=0x0F`) cost zero - they are routed through
/// the queue but only the surrounding starter+art pair pays.
pub fn art_ap_cost(action: ActionConstant) -> u8 {
    let b = action.as_byte();
    match b {
        0x00 => 0,
        0x01..=0x05 => 0, // system actions
        0x06..=0x0B => 0, // anim placeholders
        0x0C..=0x0F => 0, // directional bytes (free)
        0x10..=0x18 => 0, // anim / empty slots
        0x19 | 0x1A => 1, // Regular / Special Art Starter
        0x1B..=0x32 => 1, // per-character art body
        _ => 0,           // unknown - treat as free
    }
}

/// Total AP cost of a sequence of action constants. Sums per-byte
/// [`art_ap_cost`] values.
pub fn queue_ap_cost(queue: &ActionQueue) -> u32 {
    queue
        .actions()
        .iter()
        .copied()
        .map(|a| art_ap_cost(a) as u32)
        .sum()
}

/// Per-character AP gauge tracked across one battle turn.
#[derive(Debug, Clone, Copy)]
pub struct ApGauge {
    /// Base AP for the character at the start of the turn (typically 4).
    pub base_ap: u8,
    /// `true` if the character has pressed Spirit this turn (so the +5
    /// bonus has already been spent into [`Self::current_ap`]).
    pub spirit_charged: bool,
    /// Current AP balance - the queue checks against this. Decreases as
    /// arts are pushed; resets at turn start.
    pub current_ap: u8,
}

impl Default for ApGauge {
    fn default() -> Self {
        Self {
            base_ap: DEFAULT_BASE_AP,
            spirit_charged: false,
            current_ap: DEFAULT_BASE_AP,
        }
    }
}

impl ApGauge {
    /// Construct a gauge with an explicit base AP. Useful when a
    /// character-record-derived base differs from the default 4.
    pub fn with_base(base_ap: u8) -> Self {
        Self {
            base_ap,
            spirit_charged: false,
            current_ap: base_ap,
        }
    }

    /// Override the base AP. Subsequent [`Self::reset_for_turn`] resets
    /// to the new value; the current balance is left untouched.
    pub fn set_base_ap(&mut self, base_ap: u8) {
        self.base_ap = base_ap;
    }

    /// Reset for a new turn. Refills `current_ap` to `base_ap` and
    /// clears the Spirit-charged flag.
    pub fn reset_for_turn(&mut self) {
        self.current_ap = self.base_ap;
        self.spirit_charged = false;
    }

    /// Apply the Spirit-button charge. Idempotent within a turn - the
    /// retail engine refuses to add the bonus twice. Returns `true` if
    /// the bonus was applied this call, `false` if it was already
    /// charged.
    pub fn charge_spirit(&mut self) -> bool {
        if self.spirit_charged {
            return false;
        }
        self.spirit_charged = true;
        self.current_ap = self.current_ap.saturating_add(SPIRIT_AP_BONUS);
        true
    }

    /// `true` if `cost` AP would remain non-negative after spending.
    pub fn can_afford(&self, cost: u8) -> bool {
        self.current_ap >= cost
    }

    /// Try to spend `cost` AP. Returns `true` on success, `false` if
    /// insufficient (in which case the balance is untouched).
    pub fn try_spend(&mut self, cost: u8) -> bool {
        if !self.can_afford(cost) {
            return false;
        }
        self.current_ap -= cost;
        true
    }

    /// Refund `cost` AP back to the gauge - used when an art is removed
    /// from the queue (cancel-while-editing). Saturates at the full
    /// post-Spirit ceiling so cancel-spam can't grant infinite AP.
    pub fn refund(&mut self, cost: u8) {
        let ceiling = self.base_ap.saturating_add(if self.spirit_charged {
            SPIRIT_AP_BONUS
        } else {
            0
        });
        self.current_ap = self.current_ap.saturating_add(cost).min(ceiling);
    }

    /// Maximum AP the gauge could hold this turn (with Spirit if
    /// charged).
    pub fn ceiling(&self) -> u8 {
        self.base_ap.saturating_add(if self.spirit_charged {
            SPIRIT_AP_BONUS
        } else {
            0
        })
    }

    /// Try to push one action onto the queue, paying its AP cost.
    /// Returns `true` if the action was admitted, `false` if AP was
    /// insufficient (in which case the queue is untouched).
    pub fn try_push(&mut self, queue: &mut ActionQueue, action: ActionConstant) -> bool {
        let cost = art_ap_cost(action);
        if !self.try_spend(cost) {
            return false;
        }
        queue.push(action);
        true
    }
}

/// Compute the per-level AP base: `4 + (level / 10)`, capped at 10. The
/// base climbs by 1 every 10 levels - characters at level 1..9 have base
/// 4, 10..19 have base 5, etc., maxing at level 60 with base 10.
///
/// **Fitted, not pinned.** No retail site computes this. It stands in for
/// `AGL / arm_cost`, the command count retail's gauge admits, and is
/// wrong in kind rather than degree for an off-class weapon - see the
/// module note on units.
pub fn ap_base_for_level(level: u8) -> u8 {
    let raw = (DEFAULT_BASE_AP + level / 10) as u16;
    raw.min(10) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ac(b: u8) -> ActionConstant {
        ActionConstant::from_byte(b).unwrap()
    }

    #[test]
    fn default_gauge_has_4_ap() {
        let g = ApGauge::default();
        assert_eq!(g.base_ap, 4);
        assert_eq!(g.current_ap, 4);
        assert!(!g.spirit_charged);
    }

    #[test]
    fn art_costs_one_starter_costs_one() {
        assert_eq!(art_ap_cost(ac(0x19)), 1);
        assert_eq!(art_ap_cost(ac(0x1A)), 1);
        // Per-character art body
        assert_eq!(art_ap_cost(ac(0x1B)), 1);
        assert_eq!(art_ap_cost(ac(0x32)), 1);
    }

    #[test]
    fn directions_and_system_actions_are_free() {
        for &b in &[0x01, 0x02, 0x03, 0x04, 0x05, 0x0C, 0x0D, 0x0E, 0x0F] {
            assert_eq!(art_ap_cost(ac(b)), 0, "byte {b:#x}");
        }
    }

    #[test]
    fn spend_succeeds_when_in_budget() {
        let mut g = ApGauge::default();
        assert!(g.try_spend(2));
        assert_eq!(g.current_ap, 2);
    }

    #[test]
    fn spend_fails_when_over_budget() {
        let mut g = ApGauge::default();
        assert!(!g.try_spend(5));
        assert_eq!(g.current_ap, 4); // untouched
    }

    #[test]
    fn spirit_button_adds_5_ap_once_per_turn() {
        let mut g = ApGauge::default();
        assert!(g.charge_spirit());
        assert_eq!(g.current_ap, 9);
        assert!(g.spirit_charged);
        // Idempotent
        assert!(!g.charge_spirit());
        assert_eq!(g.current_ap, 9);
    }

    #[test]
    fn refund_caps_at_ceiling() {
        let mut g = ApGauge::default();
        g.try_spend(2); // 4 -> 2
        g.refund(10);
        // ceiling is 4 (no spirit), so balance caps at 4.
        assert_eq!(g.current_ap, 4);
    }

    #[test]
    fn refund_after_spirit_caps_at_9() {
        let mut g = ApGauge::default();
        g.charge_spirit();
        g.try_spend(5);
        g.refund(99);
        assert_eq!(g.current_ap, 9);
    }

    #[test]
    fn reset_for_turn_clears_spirit() {
        let mut g = ApGauge::default();
        g.charge_spirit();
        g.try_spend(7);
        g.reset_for_turn();
        assert_eq!(g.current_ap, 4);
        assert!(!g.spirit_charged);
    }

    #[test]
    fn try_push_pays_cost_and_appends() {
        let mut g = ApGauge::default();
        let mut q = ActionQueue::new();
        assert!(g.try_push(&mut q, ac(0x19))); // starter
        assert!(g.try_push(&mut q, ac(0x25))); // art
        assert!(g.try_push(&mut q, ac(0x0C))); // direction (free)
        // Start 4, used 2, dir is free.
        assert_eq!(g.current_ap, 2);
        assert_eq!(q.actions().len(), 3);
    }

    #[test]
    fn try_push_rejects_when_over_budget() {
        let mut g = ApGauge::with_base(1);
        let mut q = ActionQueue::new();
        assert!(g.try_push(&mut q, ac(0x19)));
        assert!(!g.try_push(&mut q, ac(0x25))); // out of AP
        assert_eq!(q.actions().len(), 1);
        assert_eq!(g.current_ap, 0);
    }

    #[test]
    fn queue_ap_cost_sums_correctly() {
        let mut q = ActionQueue::new();
        q.push(ac(0x19)); // 1
        q.push(ac(0x1B)); // 1
        q.push(ac(0x0C)); // 0
        q.push(ac(0x1A)); // 1
        q.push(ac(0x2B)); // 1
        assert_eq!(queue_ap_cost(&q), 4);
    }

    #[test]
    fn ap_base_per_level_steps_every_10() {
        assert_eq!(ap_base_for_level(1), 4);
        assert_eq!(ap_base_for_level(9), 4);
        assert_eq!(ap_base_for_level(10), 5);
        assert_eq!(ap_base_for_level(20), 6);
        assert_eq!(ap_base_for_level(50), 9);
        assert_eq!(ap_base_for_level(60), 10);
        // Capped at 10
        assert_eq!(ap_base_for_level(99), 10);
        assert_eq!(ap_base_for_level(255), 10);
    }

    #[test]
    fn ceiling_reflects_spirit_state() {
        let mut g = ApGauge::default();
        assert_eq!(g.ceiling(), 4);
        g.charge_spirit();
        assert_eq!(g.ceiling(), 9);
    }

    /// The three multiplier bands and their boundaries, straight off the
    /// builder's immediates (`0x801EF328` / `0x801EF32C` / `0x801EF33C`).
    #[test]
    fn art_spirit_multiplier_bands_follow_the_visit_count() {
        assert_eq!(art_spirit_cost(0, 1, false), 11);
        assert_eq!(art_spirit_cost(1, 1, false), 10);
        assert_eq!(art_spirit_cost(3, 1, false), 10);
        assert_eq!(art_spirit_cost(4, 1, false), 6);
        assert_eq!(art_spirit_cost(40, 1, false), 6);
    }

    /// Retail's own worked example: Noa's Vulture Blade is a five-command art
    /// at *visit* ordinal 2 (her grid skips display indices 2 and 3), so it
    /// costs `10 x 5`, not `6 x 5`. Reading the display index instead of the
    /// visit ordinal is exactly the mistake that produces the wrong one.
    #[test]
    fn vulture_blade_costs_ten_times_five() {
        assert_eq!(art_spirit_cost(2, 5, false), 50);
        assert_ne!(art_spirit_cost(4, 5, false), 50);
    }

    /// `srl t4,t4,0x1` halves the **multiplier**, before the multiply - so
    /// `11` and `10` both collapse to `5` and the halved price is not half
    /// the full one.
    #[test]
    fn the_0x800_flag_halves_the_multiplier_not_the_product() {
        assert_eq!(art_spirit_cost(0, 3, true), 15);
        assert_eq!(art_spirit_cost(1, 3, true), 15);
        assert_eq!(art_spirit_cost(4, 3, true), 9);
        // Not `art_spirit_cost(0, 3, false) / 2` == 16.
        assert_ne!(
            art_spirit_cost(0, 3, true),
            art_spirit_cost(0, 3, false) / 2
        );
    }

    #[test]
    fn a_turn_charges_each_performed_art_at_its_own_ordinal() {
        let a = |b: u8| ActionConstant::from_byte(b).unwrap();
        // Ordinals 0..3 of a synthetic catalog, command counts 2/3/4/5.
        let catalog = [(a(0x1B), 2), (a(0x1C), 3), (a(0x1D), 4), (a(0x1E), 5)];
        // Ordinal 0 -> 11 x 2, ordinal 2 -> 10 x 4.
        assert_eq!(
            arts_turn_spirit_cost(&catalog, &[a(0x1B), a(0x1D)], false),
            22 + 40
        );
        // An art performed twice pays twice, at the same ordinal.
        assert_eq!(
            arts_turn_spirit_cost(&catalog, &[a(0x1C), a(0x1C)], false),
            60
        );
        // A plain attack matches no art and charges nothing.
        assert_eq!(arts_turn_spirit_cost(&catalog, &[], false), 0);
        // An art outside the catalog contributes nothing rather than panicking.
        assert_eq!(arts_turn_spirit_cost(&catalog, &[a(0x2F)], false), 0);
    }
}

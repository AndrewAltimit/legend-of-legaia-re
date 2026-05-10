//! Action-Point ("AP") gauge for Tactical Arts command input.
//!
//! Each character has a per-turn AP budget that limits how many art
//! commands they can chain. The retail engine reads this from the
//! character record's `+0xC9` byte (`current_ap`) and `+0xCA` byte
//! (`bonus_ap` - the +5 charged by pressing Spirit). When the player is
//! in command-input mode, every art slot dropped into the queue spends
//! the slotted art's AP cost; the queue stops accepting input once the
//! remaining budget would go below zero.
//!
//! The retail base AP starts at 4 and grows by 1 each level milestone
//! (every 10 levels, capped at 10). Pressing the Spirit button during
//! the command window adds the `+5` bonus exactly once per turn.
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

/// Default base AP for level-1 characters. The retail value is 4;
/// engines targeting non-vanilla balance can override.
pub const DEFAULT_BASE_AP: u8 = 4;

/// AP added when the player presses the Spirit button during command
/// input. Retail value is 5; can be raised by some equipment.
pub const SPIRIT_AP_BONUS: u8 = 5;

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

/// Compute the per-level AP base.
///
/// Retail formula: `4 + (level / 10)`, capped at 10. The base climbs by
/// 1 every 10 levels - characters at level 1..9 have base 4, 10..19
/// have base 5, etc., maxing at level 60 with base 10.
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
}

//! Per-actor status-effect tracker.
//!
//! PORT: FUN_801E295C
//!
//! Tracks the set of status conditions afflicting each battle actor and
//! folds them down into per-turn ticks. The retail engine stores status
//! flags as a packed bitfield on the battle-actor struct (`+0x130` per
//! `ghidra/scripts/funcs/801E295C.txt` strain analysis) plus per-flag
//! turn counters and tick-damage values; the layout is per-flag and not
//! captured in any single overlay dump. This module mirrors the observed
//! semantics rather than reproducing the byte layout.
//!
//! The eight conditions the runtime distinguishes, named with the game's
//! in-game ailment terms. `byte` is the on-disc art-record `enemy_effect`
//! value. `Retail effect` is the published behaviour (the Legaia wiki status
//! pages, the project's ground-truth label source - see
//! [`docs/reference/gamedata.md`]); `Engine` flags where this clean-room model
//! diverges. **The wiki gives the qualitative mechanics + cure methods but
//! NOT the exact turn counts or HP-per-turn formulas, so the durations
//! ([`StatusKind::default_duration`]) and tick formulas below stay clean-room
//! approximations.**
//!
//! | Status    | byte | Retail effect (wiki)                                        | Engine |
//! |-----------|------|-------------------------------------------------------------|--------|
//! | `Toxic`   | `1`  | "Deadly Poison": HP drains faster than Venom AND ATK/DEF drop | DoT `max_hp/16` + ATK x0.875; DoT-vs-Venom magnitude and the DEF drop not modelled |
//! | `Numb`    | `2`  | Paralysis: cannot act; clears on being hit OR after some turns | NOT enforced yet (the old "50% skip" was never wired - retail is a full block, not a roll) |
//! | `Venom`   | `3`  | "Poison": HP drains (lesser than Toxic)                      | DoT `current_hp/8` |
//! | `Sleep`   | `4`  | Asleep; wakes when hit                                       | block + clear-on-hit (matches) |
//! | `Confuse` | `5`  | Acts uncontrollably / random target                         | random target (LoL-1 wiki page is a stub; modelled clean-room) |
//! | `Curse`   | `6`  | Blocks Magic (Magic Amulet protects)                        | blocks Magic (matches) |
//! | `Stone`   | `7`  | Petrification: cannot act, cannot be damaged, counts as defeated; lasts the whole battle (no in-battle cure; escape restores) | block only - the invulnerability, defeat-counting, and whole-battle duration are not modelled |
//! | `Faint`   | `8`  | KO at 0 HP: collapse, no actions; revived only by Phoenix / revive Magic | block + `until cured` (matches) |
//!
//! Engines drain pending [`StatusEvent`]s from [`StatusEffectTracker::tick_actor`]
//! and feed them back into their HUD / battle event log.

use legaia_art::record::EnemyEffect;

/// One kind of status-effect condition, named with the game's in-game ailment
/// terms. The mapping from the on-disc `enemy_effect` byte names bytes 1/2
/// directly (`EnemyEffect::Toxic`/`Numb`); bytes 3..=8 arrive as
/// `EnemyEffect::Other(_)`. Per-turn effects are clean-room approximations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusKind {
    /// Deadly poison: HP drains faster than Venom and ATK/DEF drop. Modelled
    /// as an HP tick (clean-room `max_hp / 16`) + an ATK penalty; the
    /// faster-than-Venom magnitude and the DEF penalty are not yet modelled.
    Toxic,
    /// Paralysis: the unit cannot act; clears on being hit or after some turns
    /// (a full block, NOT a probability roll - retail does not skip-by-chance).
    Numb,
    /// Standard poison: HP drains (lesser than Toxic). Clean-room
    /// `current_hp / 8` tick.
    Venom,
    /// Asleep; wakes when hit.
    Sleep,
    /// Acts uncontrollably against a random target.
    Confuse,
    /// Blocks Magic actions (the Magic Amulet protects against Curse attacks).
    Curse,
    /// Petrification: cannot act and cannot be damaged; petrified members count
    /// as defeated and Stone lasts the whole battle (no in-battle cure - a
    /// successful escape restores them). The engine models only the action
    /// block, not the invulnerability / defeat-counting / whole-battle duration.
    Stone,
    /// KO at 0 HP: the unit collapses and cannot act; revived only by a Phoenix
    /// or revive Magic. If the whole party Faints it is a Game Over.
    Faint,
}

impl StatusKind {
    /// Resolve a [`StatusKind`] from an art-record `EnemyEffect`. Returns
    /// `None` for [`EnemyEffect::None`] and unknown bytes outside the
    /// catalogued range. The retail consumer in the battle SM does the
    /// same - unknown bytes are dropped with no side-effect.
    pub fn from_enemy_effect(eff: EnemyEffect) -> Option<Self> {
        match eff {
            EnemyEffect::None => None,
            EnemyEffect::Toxic => Some(StatusKind::Toxic),
            EnemyEffect::Numb => Some(StatusKind::Numb),
            EnemyEffect::Other(3) => Some(StatusKind::Venom),
            EnemyEffect::Other(4) => Some(StatusKind::Sleep),
            EnemyEffect::Other(5) => Some(StatusKind::Confuse),
            EnemyEffect::Other(6) => Some(StatusKind::Curse),
            EnemyEffect::Other(7) => Some(StatusKind::Stone),
            EnemyEffect::Other(8) => Some(StatusKind::Faint),
            EnemyEffect::Other(_) => None,
        }
    }

    /// Default duration in turns for this kind. The retail engine uses
    /// per-status duration tables - these defaults match the most common
    /// observed value across the catalogued enemy attack scripts.
    pub fn default_duration(self) -> u8 {
        match self {
            StatusKind::Toxic => 4,
            StatusKind::Numb => 3,
            StatusKind::Venom => 6,
            StatusKind::Sleep => 3,
            StatusKind::Confuse => 3,
            StatusKind::Curse => 4,
            StatusKind::Stone => 1,
            StatusKind::Faint => 255, // until cured
        }
    }

    /// `true` if the kind blocks the actor from acting on its turn.
    pub fn blocks_actions(self) -> bool {
        matches!(
            self,
            StatusKind::Sleep | StatusKind::Stone | StatusKind::Faint
        )
    }

    /// `true` if the kind blocks Magic specifically.
    pub fn blocks_magic(self) -> bool {
        matches!(self, StatusKind::Curse | StatusKind::Faint)
    }

    /// `true` if being hit clears this status (Sleep wakes on damage).
    pub fn clears_on_damage(self) -> bool {
        matches!(self, StatusKind::Sleep)
    }
}

/// One active instance of a status condition on an actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusInstance {
    pub kind: StatusKind,
    /// Remaining turns before this instance expires. Zero means the
    /// instance ticks one more time and is then cleared.
    pub remaining_turns: u8,
}

impl StatusInstance {
    pub fn new(kind: StatusKind) -> Self {
        Self {
            kind,
            remaining_turns: kind.default_duration(),
        }
    }

    pub fn with_duration(kind: StatusKind, duration: u8) -> Self {
        Self {
            kind,
            remaining_turns: duration,
        }
    }
}

/// One per-tick event emitted by the status-effect tracker. Engines fold
/// these into their battle event stream (apply HP delta, surface a HUD
/// blink, clear an icon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusEvent {
    /// `kind` ticked and dealt `damage` HP loss to the actor.
    TickDamage {
        actor_slot: u8,
        kind: StatusKind,
        damage: u16,
    },
    /// Status `kind` expired this turn and is now cleared.
    Cleared { actor_slot: u8, kind: StatusKind },
    /// Status `kind` blocked the actor's turn (Sleep / Stone / Faint).
    Blocked { actor_slot: u8, kind: StatusKind },
    /// Status `kind` blocked the actor's Magic action (Curse / Faint).
    BlockedMagic { actor_slot: u8, kind: StatusKind },
}

impl StatusEvent {
    pub fn actor_slot(&self) -> u8 {
        match self {
            StatusEvent::TickDamage { actor_slot, .. }
            | StatusEvent::Cleared { actor_slot, .. }
            | StatusEvent::Blocked { actor_slot, .. }
            | StatusEvent::BlockedMagic { actor_slot, .. } => *actor_slot,
        }
    }
}

/// Per-battle status-effect tracker.
///
/// Indexed by actor slot. Actors not in any active status have an empty
/// vec; lookups for non-existent slots silently return defaults.
#[derive(Debug, Default, Clone)]
pub struct StatusEffectTracker {
    per_actor: Vec<Vec<StatusInstance>>,
    pending_events: Vec<StatusEvent>,
}

impl StatusEffectTracker {
    pub fn new() -> Self {
        Self::default()
    }

    fn slots_mut(&mut self, slot: u8) -> &mut Vec<StatusInstance> {
        let idx = slot as usize;
        if idx >= self.per_actor.len() {
            self.per_actor.resize(idx + 1, Vec::new());
        }
        &mut self.per_actor[idx]
    }

    fn slots(&self, slot: u8) -> &[StatusInstance] {
        self.per_actor
            .get(slot as usize)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Apply a new status condition. Idempotent - applying the same kind
    /// twice refreshes the timer to whichever is longer.
    pub fn apply(&mut self, slot: u8, kind: StatusKind) {
        self.apply_with_duration(slot, kind, kind.default_duration())
    }

    /// Variant that takes an explicit duration (for callers that captured
    /// per-attack duration overrides).
    pub fn apply_with_duration(&mut self, slot: u8, kind: StatusKind, duration: u8) {
        let v = self.slots_mut(slot);
        if let Some(existing) = v.iter_mut().find(|s| s.kind == kind) {
            existing.remaining_turns = existing.remaining_turns.max(duration);
        } else {
            v.push(StatusInstance::with_duration(kind, duration));
        }
    }

    /// Apply a status from the art-record `EnemyEffect` byte. Skips
    /// `EnemyEffect::None` and unrecognised `Other(_)` bytes.
    pub fn apply_from_enemy_effect(&mut self, slot: u8, eff: EnemyEffect) -> Option<StatusKind> {
        let kind = StatusKind::from_enemy_effect(eff)?;
        self.apply(slot, kind);
        Some(kind)
    }

    /// `true` if any status condition is currently active on `slot`.
    pub fn is_afflicted(&self, slot: u8) -> bool {
        !self.slots(slot).is_empty()
    }

    /// `true` if the actor has the specific `kind` active.
    pub fn has(&self, slot: u8, kind: StatusKind) -> bool {
        self.slots(slot).iter().any(|s| s.kind == kind)
    }

    /// Iterate over the active statuses on an actor.
    pub fn statuses(&self, slot: u8) -> &[StatusInstance] {
        self.slots(slot)
    }

    /// Manually clear a single status kind (for cure spells / items).
    /// Returns `true` if the status was present.
    pub fn cure(&mut self, slot: u8, kind: StatusKind) -> bool {
        let v = self.slots_mut(slot);
        let before = v.len();
        v.retain(|s| s.kind != kind);
        let cleared = v.len() != before;
        if cleared {
            self.pending_events.push(StatusEvent::Cleared {
                actor_slot: slot,
                kind,
            });
        }
        cleared
    }

    /// Clear every status kind on an actor (full-cure / revive).
    pub fn cure_all(&mut self, slot: u8) {
        let kinds: Vec<StatusKind> = self.slots(slot).iter().map(|s| s.kind).collect();
        for k in kinds {
            self.cure(slot, k);
        }
    }

    /// Clear-on-damage hook. Engines call this when an actor takes damage,
    /// so Sleep clears as it would in retail.
    pub fn on_damaged(&mut self, slot: u8) {
        let kinds: Vec<StatusKind> = self
            .slots(slot)
            .iter()
            .filter(|s| s.kind.clears_on_damage())
            .map(|s| s.kind)
            .collect();
        for k in kinds {
            self.cure(slot, k);
        }
    }

    /// Step every active status on `actor_slot` forward one turn. Computes
    /// per-turn tick damage based on `current_hp` / `max_hp` for damage-
    /// over-time conditions (Toxic, Venom), and decrements every
    /// instance's `remaining_turns`. Expired instances are cleared and a
    /// [`StatusEvent::Cleared`] is queued.
    ///
    /// Returns the total tick damage dealt this turn (for engines that
    /// want a single number to subtract); the per-status events are
    /// queued in [`Self::pending_events`] regardless.
    pub fn tick_actor(&mut self, actor_slot: u8, current_hp: u16, max_hp: u16) -> u16 {
        let mut total_damage = 0u16;
        let mut to_clear: Vec<StatusKind> = Vec::new();
        // Compute damages first to avoid holding a mutable borrow while
        // we push events.
        let snapshot: Vec<StatusInstance> = self.slots(actor_slot).to_vec();
        for inst in &snapshot {
            let dmg = match inst.kind {
                StatusKind::Toxic => toxic_tick_damage(max_hp),
                StatusKind::Venom => venom_tick_damage(current_hp),
                _ => 0,
            };
            if dmg > 0 {
                total_damage = total_damage.saturating_add(dmg);
                self.pending_events.push(StatusEvent::TickDamage {
                    actor_slot,
                    kind: inst.kind,
                    damage: dmg,
                });
            }
        }
        // Decrement timers and queue clears.
        let v = self.slots_mut(actor_slot);
        for inst in v.iter_mut() {
            if inst.remaining_turns == 0 {
                to_clear.push(inst.kind);
            } else {
                inst.remaining_turns = inst.remaining_turns.saturating_sub(1);
                if inst.remaining_turns == 0 {
                    to_clear.push(inst.kind);
                }
            }
        }
        for k in to_clear {
            self.cure(actor_slot, k);
        }
        total_damage
    }

    /// Test whether the actor is allowed to act this turn. Emits a
    /// [`StatusEvent::Blocked`] if any blocking status is active and
    /// returns `false`. Engines call this once per actor turn-start.
    pub fn check_can_act(&mut self, actor_slot: u8) -> bool {
        if let Some(blocker) = self
            .slots(actor_slot)
            .iter()
            .find(|s| s.kind.blocks_actions())
            .map(|s| s.kind)
        {
            self.pending_events.push(StatusEvent::Blocked {
                actor_slot,
                kind: blocker,
            });
            return false;
        }
        true
    }

    /// Test whether the actor can cast Magic this turn. Emits a
    /// [`StatusEvent::BlockedMagic`] when blocked.
    pub fn check_can_cast_magic(&mut self, actor_slot: u8) -> bool {
        if let Some(blocker) = self
            .slots(actor_slot)
            .iter()
            .find(|s| s.kind.blocks_magic())
            .map(|s| s.kind)
        {
            self.pending_events.push(StatusEvent::BlockedMagic {
                actor_slot,
                kind: blocker,
            });
            return false;
        }
        true
    }

    /// Drain queued events for engine consumption. Resets the queue.
    pub fn drain_events(&mut self) -> Vec<StatusEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// Total slot count tracked. Useful for sizing fixed arrays.
    pub fn actor_count(&self) -> usize {
        self.per_actor.len()
    }
}

/// Tick-damage formula for Toxic (clean-room `max_hp / 16`, floored at 1).
/// Toxic is the deadly poison (~2x Venom in-game); that relative magnitude is
/// not yet reflected here - the formula is a placeholder pending a disc trace.
pub fn toxic_tick_damage(max_hp: u16) -> u16 {
    (max_hp / 16).max(1)
}

/// Tick-damage formula for Venom (clean-room `current_hp / 8`, floored at 1).
pub fn venom_tick_damage(current_hp: u16) -> u16 {
    (current_hp / 8).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enemy_effect_byte_routes() {
        assert_eq!(
            StatusKind::from_enemy_effect(EnemyEffect::Toxic),
            Some(StatusKind::Toxic)
        );
        assert_eq!(
            StatusKind::from_enemy_effect(EnemyEffect::Numb),
            Some(StatusKind::Numb)
        );
        assert_eq!(
            StatusKind::from_enemy_effect(EnemyEffect::Other(3)),
            Some(StatusKind::Venom)
        );
        assert_eq!(
            StatusKind::from_enemy_effect(EnemyEffect::Other(8)),
            Some(StatusKind::Faint)
        );
        assert_eq!(StatusKind::from_enemy_effect(EnemyEffect::None), None);
        assert_eq!(StatusKind::from_enemy_effect(EnemyEffect::Other(99)), None);
    }

    #[test]
    fn apply_then_has_returns_true() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Toxic);
        assert!(t.has(0, StatusKind::Toxic));
        assert!(!t.has(0, StatusKind::Numb));
    }

    #[test]
    fn apply_idempotent_takes_longer_duration() {
        let mut t = StatusEffectTracker::new();
        t.apply_with_duration(0, StatusKind::Toxic, 2);
        t.apply_with_duration(0, StatusKind::Toxic, 5);
        let s = t.statuses(0);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].remaining_turns, 5);
    }

    #[test]
    fn apply_idempotent_keeps_longer_when_new_is_shorter() {
        let mut t = StatusEffectTracker::new();
        t.apply_with_duration(0, StatusKind::Toxic, 5);
        t.apply_with_duration(0, StatusKind::Toxic, 2);
        assert_eq!(t.statuses(0)[0].remaining_turns, 5);
    }

    #[test]
    fn cure_removes_and_emits_event() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Toxic);
        t.drain_events(); // flush the apply (no apply event but in case)
        assert!(t.cure(0, StatusKind::Toxic));
        assert!(!t.has(0, StatusKind::Toxic));
        let evs = t.drain_events();
        assert_eq!(evs.len(), 1);
        assert!(matches!(evs[0], StatusEvent::Cleared { .. }));
    }

    #[test]
    fn cure_all_clears_every_kind() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Toxic);
        t.apply(0, StatusKind::Numb);
        t.apply(0, StatusKind::Curse);
        t.cure_all(0);
        assert!(!t.is_afflicted(0));
    }

    #[test]
    fn burned_tick_dot_dropping_max_hp() {
        let mut t = StatusEffectTracker::new();
        t.apply_with_duration(0, StatusKind::Toxic, 3);
        let dmg = t.tick_actor(0, 100, 160);
        assert_eq!(dmg, 10); // 160 / 16
    }

    #[test]
    fn burned_floors_at_1() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Toxic);
        let dmg = t.tick_actor(0, 5, 5);
        assert_eq!(dmg, 1);
    }

    #[test]
    fn poison_tick_uses_current_hp() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Venom);
        let dmg = t.tick_actor(0, 80, 100);
        assert_eq!(dmg, 10); // 80 / 8
    }

    #[test]
    fn ticking_decrements_remaining_turns() {
        let mut t = StatusEffectTracker::new();
        t.apply_with_duration(0, StatusKind::Toxic, 2);
        t.tick_actor(0, 100, 160);
        assert_eq!(t.statuses(0)[0].remaining_turns, 1);
        t.tick_actor(0, 100, 160);
        // Cleared at zero
        assert!(!t.has(0, StatusKind::Toxic));
    }

    #[test]
    fn ticking_emits_cleared_event_at_expiry() {
        let mut t = StatusEffectTracker::new();
        t.apply_with_duration(0, StatusKind::Toxic, 1);
        t.drain_events();
        t.tick_actor(0, 100, 160);
        let evs = t.drain_events();
        assert!(evs.iter().any(|e| matches!(
            e,
            StatusEvent::Cleared {
                kind: StatusKind::Toxic,
                ..
            }
        )));
    }

    #[test]
    fn shock_does_not_deal_damage_on_tick() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Numb);
        let dmg = t.tick_actor(0, 100, 160);
        assert_eq!(dmg, 0);
    }

    #[test]
    fn check_can_act_emits_blocked_when_asleep() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Sleep);
        assert!(!t.check_can_act(0));
        let evs = t.drain_events();
        assert_eq!(evs.len(), 1);
        assert!(matches!(
            evs[0],
            StatusEvent::Blocked {
                kind: StatusKind::Sleep,
                ..
            }
        ));
    }

    #[test]
    fn check_can_act_passes_when_only_burned() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Toxic);
        assert!(t.check_can_act(0));
    }

    #[test]
    fn check_can_cast_magic_blocked_by_silence() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Curse);
        assert!(!t.check_can_cast_magic(0));
        let evs = t.drain_events();
        assert!(
            evs.iter()
                .any(|e| matches!(e, StatusEvent::BlockedMagic { .. }))
        );
    }

    #[test]
    fn check_can_cast_magic_blocked_by_petrify() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Faint);
        assert!(!t.check_can_cast_magic(0));
    }

    #[test]
    fn on_damaged_clears_sleep() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Sleep);
        t.apply(0, StatusKind::Toxic);
        t.on_damaged(0);
        assert!(!t.has(0, StatusKind::Sleep));
        assert!(t.has(0, StatusKind::Toxic));
    }

    #[test]
    fn apply_from_enemy_effect_routes_burned() {
        let mut t = StatusEffectTracker::new();
        let kind = t.apply_from_enemy_effect(2, EnemyEffect::Toxic);
        assert_eq!(kind, Some(StatusKind::Toxic));
        assert!(t.has(2, StatusKind::Toxic));
    }

    #[test]
    fn apply_from_enemy_effect_skips_none() {
        let mut t = StatusEffectTracker::new();
        let kind = t.apply_from_enemy_effect(0, EnemyEffect::None);
        assert_eq!(kind, None);
        assert!(!t.is_afflicted(0));
    }

    #[test]
    fn multiple_actors_tracked_independently() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Toxic);
        t.apply(3, StatusKind::Numb);
        assert!(t.has(0, StatusKind::Toxic));
        assert!(t.has(3, StatusKind::Numb));
        assert!(!t.has(0, StatusKind::Numb));
        assert!(!t.has(3, StatusKind::Toxic));
    }

    #[test]
    fn petrify_default_duration_is_huge() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Faint);
        let inst = t.statuses(0)[0];
        assert_eq!(inst.remaining_turns, 255);
    }

    #[test]
    fn no_op_for_empty_slot() {
        let mut t = StatusEffectTracker::new();
        let dmg = t.tick_actor(7, 100, 100);
        assert_eq!(dmg, 0);
        assert!(t.drain_events().is_empty());
    }

    #[test]
    fn stunned_clears_after_one_tick() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Stone);
        assert!(t.has(0, StatusKind::Stone));
        t.tick_actor(0, 100, 100);
        assert!(!t.has(0, StatusKind::Stone));
    }
}

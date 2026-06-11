//! Per-actor status-effect tracker.
//!
//! PORT: FUN_801E295C
//! PORT: FUN_801E752C (per-round Venom / Toxic DoT ticker - the exact
//!       tick arithmetic in [`toxic_tick_damage`] / [`venom_tick_damage`])
//! REF: FUN_801E7320 (Confuse retarget; ported as
//!      `legaia_engine_core::world` `resolve_monster_target`)
//! REF: FUN_801DD864 (the x9/10 / x7/10 combat-roll status scales - ported as
//! `battle_formulas::apply_status_weaken`; cited here for the table).
//! REF: FUN_801D0748 (round driver - its state `0x14` calls the DoT ticker
//! once per round, gated on the round counter).
//! REF: FUN_80047430 (sets the `+0x16E` `0x380` AI-delegation bits on party
//!      slots whose char record carries accessory-passive bit 45 - Rage /
//!      Evil Medallion)
//!
//! Tracks the set of status conditions afflicting each battle actor and
//! folds them down into per-turn ticks. The retail engine stores battle
//! status flags as the packed `u16` at battle-actor `+0x16E` (mirrored to
//! char record `+0x12E` for party slots by `FUN_80047430`): bit `1` =
//! Venom, bit `2` = Toxic, bits `8/0x10/0x20` = Rot (per-limb command
//! disable), bits `0x380` = AI-delegation (Rage / charm), bit `0x1000` =
//! Curse. This module mirrors the observed semantics on a per-kind
//! instance list rather than reproducing the byte layout.
//!
//! The conditions the runtime distinguishes, named with the game's
//! in-game ailment terms. `byte` is the on-disc art-record `enemy_effect`
//! value as the engine currently labels it. `Retail effect` is the pinned
//! behaviour where a dump pins it, else the published behaviour (the Legaia
//! wiki status pages - see [`docs/reference/gamedata.md`]); `Engine` flags
//! where this clean-room model diverges.
//!
//! | Status    | byte | Retail effect                                               | Engine |
//! |-----------|------|-------------------------------------------------------------|--------|
//! | `Toxic`   | `1`  | DoT `min(max_hp/16, 256)` per round, never lethal (clamps to `current_hp - 1`); suppresses Venom's tick; outgoing damage and guard rolls scale x7/10 (`FUN_801E752C` + `FUN_801DD864`) | exact (`toxic_tick_damage`); the roll scaling is `battle_formulas::apply_status_weaken` bit 2 (engine-core's stat resolver mirrors the same x7/10 at the stat line) |
//! | `Numb`    | `2`  | Paralysis: cannot act; clears on being hit OR after some turns (wiki; the enforcement site is not in the dumped corpus) | full block + clear-on-hit (same shape as Sleep) |
//! | `Venom`   | `3`  | DoT `min(max_hp/32, 128)` per round, never lethal; skipped while Toxic is active; rolls scale x9/10 (`FUN_801E752C` + `FUN_801DD864`) | exact (`venom_tick_damage`) |
//! | `Sleep`   | `4`  | Asleep; wakes when hit (wiki)                               | block + clear-on-hit |
//! | `Confuse` | `5`  | Acts uncontrollably. Pinned for monsters: the AI keeps its *rolled* action - including Magic casts - but every per-monster scripted-cast override is suppressed (`overlay_battle_action_801e9fd4` gates on `+0x16E & 0x380`), and the target re-rolls to the opposite side at ActionSeed (`FUN_801E7320`). For party members only the delegation flag is pinned (`FUN_80047430`, from the Rage accessory passive); the retail auto-pick for a delegated party member is not in the dumped corpus | monster: physical + casts retarget via the `FUN_801E7320` port; party: auto-physical stand-in (see `engine-core::world::battle`) until the retail party-side pick is captured |
//! | `Curse`   | `6`  | Blocks Magic; battle-actor bit `0x1000` (Magic Amulet protects) | blocks Magic (matches) |
//! | `Stone`   | `7`  | Petrification: cannot act, cannot be damaged, counts as defeated; no in-battle cure. Runtime representation capture-pinned: `+0x16E` bit `0x04` (a Glare before/after pair shows `0 -> 4` with HP untouched + the queued action category at `+0x1DE` cleared). On a successful party escape retail floors every party actor's `+0x14C` at 1 (`FUN_801E295C` case `0x64`), which un-defeats a petrified/KO'd member | block + whole-battle duration + invulnerability (core strikes) + counts-as-defeated + [`StatusEffectTracker::cure_stone_on_escape`] for the escape restore |
//! | `Faint`   | `8`  | KO at 0 HP: collapse, no actions; revived only by Phoenix / revive Magic | block + `until cured` (matches) |
//!
//! **Byte-map caveat.** The two pinned status appliers
//! (`overlay_battle_action_801ec3e4` ~line 3099, `overlay_battle_action_801e09f8`
//! ~line 1416 - the physical-strike and special-attack hit resolvers reading the
//! record's status byte) wire byte `3` -> the weak-DoT bit (`+0x16E |= 1`, 1/8
//! chance), byte `4` -> the strong-DoT bit (`|= 2`, 1/8), byte `5` -> a random
//! limb-disable bit (`1 << (rand%3 + 3)`, blocked by the Rot Guard / Master
//! Guard passives - i.e. retail byte `5` is **Rot**, not Confuse), and byte `6`
//! -> Curse (`|= 0x1000`, 1/4). Bytes `1`/`2` only install the lingering
//! status *visual* (`actor+0x21F` + tint) in these paths. That conflicts with
//! the engine's inherited byte naming above (`4` = Sleep, `5` = Confuse, from
//! external notes); the remap is left open until a runtime capture pins what
//! bytes `1`/`2` do mechanically - see `docs/subsystems/battle-formulas.md`.
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
    /// Deadly poison: HP drains faster than Venom and ATK/DEF drop. The HP
    /// tick is the exact `FUN_801E752C` strong-DoT arm
    /// ([`toxic_tick_damage`]: `min(max_hp/16, 256)`, clamped so it never
    /// kills); the combat-roll penalty is the `FUN_801DD864` x7/10 scale
    /// ([`crate::battle_formulas::apply_status_weaken`] bit 2). While Toxic
    /// is active Venom's tick is suppressed (the retail ticker's strong-DoT
    /// branch shadows the weak one).
    Toxic,
    /// Paralysis: the unit cannot act; clears on being hit or after some turns
    /// (a full block, NOT a probability roll). Enforced via [`Self::blocks_actions`]
    /// + [`Self::clears_on_damage`], same shape as Sleep.
    Numb,
    /// Standard poison: HP drains (lesser than Toxic). Exact `FUN_801E752C`
    /// weak-DoT arm ([`venom_tick_damage`]: `min(max_hp/32, 128)`, clamped so
    /// it never kills); rolls scale x9/10
    /// ([`crate::battle_formulas::apply_status_weaken`] bit 1).
    Venom,
    /// Asleep; wakes when hit.
    Sleep,
    /// Acts uncontrollably. Pinned for monsters: the rolled action is *kept*
    /// (a confused monster can still cast Magic - the picker's generic core
    /// runs; only the per-monster-id scripted-cast overrides are suppressed,
    /// `overlay_battle_action_801e9fd4` `& 0x380` guards) and the target
    /// re-rolls to the opposite side at ActionSeed (`FUN_801E7320`, ported as
    /// `engine-core::World::resolve_monster_target`). For a party member the
    /// dumps pin only the AI-delegation flag (`FUN_80047430` sets
    /// `+0x16E |= 0x380` from the Rage accessory passive); the retail action
    /// pick for a delegated party member is not in the dumped corpus, so the
    /// engine's auto-physical party behaviour is a stand-in.
    Confuse,
    /// Blocks Magic actions (the Magic Amulet protects against Curse attacks).
    Curse,
    /// Petrification: cannot act and cannot be damaged; petrified members count
    /// as defeated and Stone lasts the whole battle (no in-battle cure). The
    /// engine models the action block, a whole-battle duration
    /// ([`Self::default_duration`] = 255), invulnerability on the core
    /// combat-strike paths, counts-as-defeated in the wipe checks, and the
    /// on-escape restore ([`StatusEffectTracker::cure_stone_on_escape`],
    /// paired with the `FUN_801E295C` case-`0x64` party HP floor ported in
    /// [`crate::battle_action`]).
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
            // Stone has no in-battle cure - it lasts the whole battle. 255 is
            // effectively "until battle end" (no battle runs that many turns).
            StatusKind::Stone => 255,
            StatusKind::Faint => 255, // until cured
        }
    }

    /// `true` if the kind blocks the actor from acting on its turn. Numb is a
    /// full paralysis (the unit "cannot perform any action" per the wiki), so
    /// it blocks the turn outright - not a probability roll.
    pub fn blocks_actions(self) -> bool {
        matches!(
            self,
            StatusKind::Numb | StatusKind::Sleep | StatusKind::Stone | StatusKind::Faint
        )
    }

    /// `true` if the kind blocks Magic specifically.
    pub fn blocks_magic(self) -> bool {
        matches!(self, StatusKind::Curse | StatusKind::Faint)
    }

    /// `true` if being hit clears this status. Sleep wakes on damage, and Numb
    /// clears on being attacked too (the wiki: it wears off "by being attacked
    /// or enough turns passing").
    pub fn clears_on_damage(self) -> bool {
        matches!(self, StatusKind::Numb | StatusKind::Sleep)
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
    /// Status `kind` blocked the actor's turn (Numb / Sleep / Stone / Faint).
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

    /// Successful-escape restore: clears Stone on every tracked actor and
    /// returns how many actors were restored (a [`StatusEvent::Cleared`] is
    /// queued per restore).
    ///
    /// Models the published "a petrified member returns to normal when the
    /// party escapes" behaviour. The pinned retail side of the escape is the
    /// party HP floor in the run band - `FUN_801E295C` case `0x64`'s
    /// successful-escape branch walks the party slots and sets any
    /// `+0x14C == 0` actor to 1 (ported in [`crate::battle_action`]'s
    /// `RunBegin`); Stone's retail bit representation is not pinned in the
    /// dumped corpus, so this tracker-level clear carries the engine model.
    /// Engines call this when the battle ends with
    /// `BattleEndCause::Escaped`.
    ///
    /// REF: FUN_801E295C
    pub fn cure_stone_on_escape(&mut self) -> usize {
        let mut restored = 0;
        for slot in 0..self.per_actor.len() as u8 {
            if self.cure(slot, StatusKind::Stone) {
                restored += 1;
            }
        }
        restored
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
    /// The DoT arithmetic is the exact `FUN_801E752C` per-round ticker
    /// (called once per round by the round driver `FUN_801D0748` state
    /// `0x14`, skipped while the round counter `ctx[+0x28A]` is still 0):
    /// Toxic shadows Venom (the retail ticker's strong-DoT branch is taken
    /// first and the weak one only `else`-fires), a dead actor
    /// (`current_hp == 0`) doesn't tick, and the per-status damage never
    /// kills (clamped to `current_hp - 1`).
    ///
    /// Returns the total tick damage dealt this turn (for engines that
    /// want a single number to subtract); the per-status events are
    /// queued in [`Self::pending_events`] regardless.
    ///
    /// PORT: FUN_801E752C
    pub fn tick_actor(&mut self, actor_slot: u8, current_hp: u16, max_hp: u16) -> u16 {
        let mut total_damage = 0u16;
        let mut to_clear: Vec<StatusKind> = Vec::new();
        // Compute damages first to avoid holding a mutable borrow while
        // we push events.
        let snapshot: Vec<StatusInstance> = self.slots(actor_slot).to_vec();
        // A petrified actor can't be damaged, so its poison DoTs don't tick.
        let petrified = snapshot.iter().any(|s| s.kind == StatusKind::Stone);
        // Retail: the strong-DoT bit (Toxic) is tested first and the weak one
        // (Venom) only ticks when it is clear.
        let toxic_active = snapshot.iter().any(|s| s.kind == StatusKind::Toxic);
        for inst in &snapshot {
            let dmg = if petrified || current_hp == 0 {
                0
            } else {
                match inst.kind {
                    StatusKind::Toxic => toxic_tick_damage(current_hp, max_hp),
                    StatusKind::Venom if !toxic_active => venom_tick_damage(current_hp, max_hp),
                    _ => 0,
                }
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

/// Tick-damage formula for Toxic - the exact strong-DoT arm of the retail
/// per-round status ticker: `damage = max_hp >> 4`, clamped to
/// `current_hp - 1` when it would kill, then capped at `0x100` (256). Toxic
/// bites exactly twice Venom's fraction of *max* HP and can reduce the
/// target to 1 HP but never to 0.
///
/// PORT: FUN_801E752C (`ghidra/scripts/funcs/overlay_battle_action_801e752c.txt`,
/// the `+0x16E & 2` branch)
pub fn toxic_tick_damage(current_hp: u16, max_hp: u16) -> u16 {
    dot_tick_damage(current_hp, max_hp >> 4, 0x100)
}

/// Tick-damage formula for Venom - the exact weak-DoT arm of the retail
/// per-round status ticker: `damage = max_hp >> 5`, clamped to
/// `current_hp - 1` when it would kill, then capped at `0x80` (128).
///
/// PORT: FUN_801E752C (`ghidra/scripts/funcs/overlay_battle_action_801e752c.txt`,
/// the `+0x16E & 1` branch)
pub fn venom_tick_damage(current_hp: u16, max_hp: u16) -> u16 {
    dot_tick_damage(current_hp, max_hp >> 5, 0x80)
}

/// Shared DoT clamp shape of `FUN_801E752C`, in the retail order: the
/// never-kill clamp (`current_hp <= raw` -> `current_hp - 1`) applies
/// *before* the per-status cap, so a low-HP actor's tick is `current_hp - 1`
/// even when that exceeds nothing. A tiny `max_hp` legitimately produces a
/// zero tick (retail has no 1-damage floor; a zero tick draws no damage
/// popup).
fn dot_tick_damage(current_hp: u16, raw: u16, cap: u16) -> u16 {
    let mut dmg = raw;
    if current_hp <= dmg {
        dmg = current_hp.saturating_sub(1);
    }
    if dmg > cap {
        dmg = cap;
    }
    dmg
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
    fn toxic_tick_dot_is_max_hp_over_16() {
        let mut t = StatusEffectTracker::new();
        t.apply_with_duration(0, StatusKind::Toxic, 3);
        let dmg = t.tick_actor(0, 100, 160);
        assert_eq!(dmg, 10); // 160 >> 4
    }

    #[test]
    fn toxic_never_kills_clamps_to_current_minus_one() {
        // Retail: `if (cur <= raw) raw = cur - 1` BEFORE the cap. A 5-HP
        // actor with a big max takes 4, landing at 1 HP, never 0.
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Toxic);
        let dmg = t.tick_actor(0, 5, 4000);
        assert_eq!(dmg, 4);
    }

    #[test]
    fn toxic_caps_at_256_and_venom_at_128() {
        // toxic: 65535 >> 4 = 4095 -> capped to 0x100.
        assert_eq!(toxic_tick_damage(65535, 65535), 0x100);
        // venom: 65535 >> 5 = 2047 -> capped to 0x80.
        assert_eq!(venom_tick_damage(65535, 65535), 0x80);
    }

    #[test]
    fn never_kill_clamp_applies_before_the_cap() {
        // cur=200, max=10000: raw toxic = 625 >= cur -> 199, and the cap
        // does NOT shrink it further (199 < 256). Retail order.
        assert_eq!(toxic_tick_damage(200, 10000), 199);
    }

    #[test]
    fn tiny_max_hp_ticks_zero_no_floor() {
        // Retail has no 1-damage floor: max_hp 5 >> 4 == 0.
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Toxic);
        let dmg = t.tick_actor(0, 5, 5);
        assert_eq!(dmg, 0);
        // And a zero tick queues no TickDamage event (no damage popup).
        assert!(
            !t.drain_events()
                .iter()
                .any(|e| matches!(e, StatusEvent::TickDamage { .. }))
        );
    }

    #[test]
    fn poison_tick_is_max_hp_over_32() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Venom);
        let dmg = t.tick_actor(0, 80, 100);
        assert_eq!(dmg, 3); // 100 >> 5
    }

    #[test]
    fn toxic_suppresses_venom_tick() {
        // Retail's ticker takes the strong-DoT branch first; the weak one is
        // an `else`. With both active only Toxic's max/16 lands.
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Toxic);
        t.apply(0, StatusKind::Venom);
        let dmg = t.tick_actor(0, 1000, 1600);
        assert_eq!(dmg, 100); // 1600 >> 4 only, not + 1600 >> 5
        let evs = t.drain_events();
        assert!(!evs.iter().any(|e| matches!(
            e,
            StatusEvent::TickDamage {
                kind: StatusKind::Venom,
                ..
            }
        )));
    }

    #[test]
    fn dead_actor_does_not_tick() {
        // Retail guards the whole DoT arm on `+0x14C != 0`.
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Toxic);
        assert_eq!(t.tick_actor(0, 0, 1600), 0);
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
    fn numb_does_not_deal_damage_on_tick() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Numb);
        let dmg = t.tick_actor(0, 100, 160);
        assert_eq!(dmg, 0);
    }

    #[test]
    fn numb_blocks_actions_and_clears_on_being_hit() {
        // Numb is a full paralysis (not a chance roll): it blocks the turn and,
        // like Sleep, wears off when the unit is attacked.
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Numb);
        assert!(!t.check_can_act(0), "Numb blocks the turn");
        assert!(t.drain_events().iter().any(|e| matches!(
            e,
            StatusEvent::Blocked {
                kind: StatusKind::Numb,
                ..
            }
        )));
        t.on_damaged(0);
        assert!(!t.has(0, StatusKind::Numb), "being hit clears Numb");
    }

    #[test]
    fn petrified_actor_takes_no_poison_tick() {
        // Stone makes the unit invulnerable, so its poison DoT doesn't tick.
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Stone);
        t.apply(0, StatusKind::Venom);
        assert_eq!(t.tick_actor(0, 80, 160), 0, "Stone absorbs poison ticks");
    }

    #[test]
    fn stone_lasts_the_whole_battle() {
        // Stone has no in-battle cure - its default duration is effectively
        // "until battle end".
        assert_eq!(StatusKind::Stone.default_duration(), 255);
    }

    #[test]
    fn toxic_drains_exactly_twice_venom() {
        // Both arms key on max HP: max/16 vs max/32 (below the caps and the
        // never-kill clamp).
        assert_eq!(toxic_tick_damage(1000, 1600), 100);
        assert_eq!(venom_tick_damage(1000, 1600), 50);
        assert_eq!(
            toxic_tick_damage(1000, 1600),
            2 * venom_tick_damage(1000, 1600)
        );
    }

    #[test]
    fn cure_stone_on_escape_restores_only_stone() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Stone);
        t.apply(0, StatusKind::Venom);
        t.apply(2, StatusKind::Stone);
        t.apply(1, StatusKind::Toxic);
        t.drain_events();
        assert_eq!(t.cure_stone_on_escape(), 2);
        assert!(!t.has(0, StatusKind::Stone));
        assert!(!t.has(2, StatusKind::Stone));
        assert!(t.has(0, StatusKind::Venom), "escape cures only Stone");
        assert!(t.has(1, StatusKind::Toxic), "escape cures only Stone");
        let evs = t.drain_events();
        assert_eq!(
            evs.iter()
                .filter(|e| matches!(
                    e,
                    StatusEvent::Cleared {
                        kind: StatusKind::Stone,
                        ..
                    }
                ))
                .count(),
            2
        );
    }

    #[test]
    fn cure_stone_on_escape_is_noop_without_stone() {
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Venom);
        assert_eq!(t.cure_stone_on_escape(), 0);
        assert!(t.has(0, StatusKind::Venom));
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
    fn check_can_act_passes_when_only_toxic() {
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
    fn stone_persists_across_turns() {
        // Stone has no in-battle expiry (whole-battle duration), so a single
        // turn tick does not clear it.
        let mut t = StatusEffectTracker::new();
        t.apply(0, StatusKind::Stone);
        assert!(t.has(0, StatusKind::Stone));
        t.tick_actor(0, 100, 100);
        assert!(t.has(0, StatusKind::Stone), "Stone lasts the whole battle");
    }
}

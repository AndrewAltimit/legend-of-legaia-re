//! Player-driven battle Magic submenu.
//!
//! Sibling of [`crate::battle_input::BattleCommandSession`] for the **Magic**
//! command. Where the command session resolves to a physical strike, the spell
//! session lists the active caster's learned spells (MP-gated), opens a
//! [`crate::target_picker::TargetPickerSession`] matched to the spell's
//! [`crate::spells::SpellTarget`] shape, and resolves to a `(spell_id, target)`
//! the live loop casts via [`crate::spells::cast_spell`] and folds into world
//! state, then cycles the turn at `EndOfAction` (a cast is the actor's whole
//! turn - no strike fires).
//!
//! The World owns the session (not the command session) because building the
//! spell list needs the caster's learned-spell list + live MP, and resolving
//! the cast needs the live actor table. The session itself is renderer- and
//! world-agnostic: hosts drive it a frame at a time with edge-triggered input
//! and rebuild the `party` / `monsters` slot state from the live actor table.

use crate::spells::{SpellCatalog, SpellTarget};
use crate::target_picker::{
    CursorRow, PickerInput, PickerOutcome, SlotState, TargetKind, TargetPickerSession,
};

/// Map a spell's [`SpellTarget`] shape onto the target-picker kind.
pub fn target_kind_for(target: SpellTarget) -> TargetKind {
    match target {
        SpellTarget::OneAlly => TargetKind::SingleAllyOrSelf,
        SpellTarget::AllAllies => TargetKind::AllAllies,
        SpellTarget::OneEnemy => TargetKind::SingleEnemy,
        SpellTarget::AllEnemies => TargetKind::AllEnemies,
        SpellTarget::SelfOnly => TargetKind::Self_,
    }
}

/// One selectable spell row in the battle Magic menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellRow {
    pub id: u8,
    pub name: String,
    pub mp_cost: u8,
    /// `true` when the caster's live MP covers `mp_cost`.
    pub affordable: bool,
}

/// Sub-phase of the battle Magic submenu.
#[derive(Debug, Clone)]
pub enum SpellPhase {
    /// Browsing the learned-spell list. `cursor` indexes
    /// [`BattleSpellSession::spells`].
    Select { cursor: u8 },
    /// A spell is chosen; picking its target.
    Targeting {
        spell_id: u8,
        picker: TargetPickerSession,
    },
    /// Resolved: the live loop should cast `spell_id` against the target.
    Confirmed {
        spell_id: u8,
        target_row: CursorRow,
        target_slot: u8,
    },
    /// Backed out of the spell list (Circle from the top level, or no spell
    /// could resolve a target) - the live loop reopens the command menu.
    Aborted,
}

/// Per-frame, edge-triggered pad bundle for the spell session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleSpellInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Confirm (Cross).
    pub cross: bool,
    /// Cancel / back (Circle).
    pub circle: bool,
}

/// One caster's spell-selection session, driven a frame at a time.
#[derive(Debug, Clone)]
pub struct BattleSpellSession {
    /// Actor-table index of the casting party member.
    pub actor: u8,
    /// Party-row index (0..=2) of the caster - the target picker uses it to
    /// skip-self on ally-targeting spells.
    pub party_slot: u8,
    /// The caster's learned spells resolved against the catalog, in list order.
    pub spells: Vec<SpellRow>,
    pub phase: SpellPhase,
}

/// Outcome of a resolved [`BattleSpellSession`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellResolution {
    /// The player confirmed a cast.
    Confirmed {
        spell_id: u8,
        target_row: CursorRow,
        target_slot: u8,
    },
    /// The player backed out; the live loop reopens the command menu.
    Aborted,
}

impl BattleSpellSession {
    /// Build from the caster's learned spells. `learned` is the spell-id list;
    /// `catalog` resolves names + MP cost; `caster_mp` greys out unaffordable
    /// rows. `ability_bits` is the caster's character-record ability bitmask
    /// (the MP-saver accessory bits Half `0x20` / Quarter `0x10`): each row's
    /// displayed cost and affordability are computed against the **effective**
    /// cost the cast actually charges
    /// ([`legaia_engine_vm::battle_formulas::mp_cost_after_ability_bits`]),
    /// so an MP-saver lets the caster select a spell whose raw cost exceeds
    /// their MP - matching `World::cast_spell_on_slots`. The cursor starts on
    /// the first affordable spell (or row 0 when none are affordable). Spell ids
    /// missing from the catalog are dropped.
    pub fn new(
        actor: u8,
        party_slot: u8,
        learned: &[u8],
        catalog: &SpellCatalog,
        caster_mp: u16,
        ability_bits: u32,
    ) -> Self {
        use legaia_engine_vm::battle_formulas::{MpCostModifier, mp_cost_after_ability_bits};
        let modifier = MpCostModifier::from_ability_flags(ability_bits);
        let spells: Vec<SpellRow> = learned
            .iter()
            .filter_map(|id| {
                let def = catalog.get(*id)?;
                let cost = mp_cost_after_ability_bits(def.mp_cost as u16, modifier);
                Some(SpellRow {
                    id: *id,
                    name: def.name.clone(),
                    mp_cost: cost.min(u8::MAX as u16) as u8,
                    affordable: caster_mp >= cost,
                })
            })
            .collect();
        let cursor = spells.iter().position(|s| s.affordable).unwrap_or(0) as u8;
        Self {
            actor,
            party_slot,
            spells,
            phase: SpellPhase::Select { cursor },
        }
    }

    /// The spell row currently under the select cursor, or `None` once the
    /// session has left the spell list.
    pub fn menu_spell(&self) -> Option<&SpellRow> {
        match self.phase {
            SpellPhase::Select { cursor } => self.spells.get(cursor as usize),
            _ => None,
        }
    }

    /// The active target picker, while one is open.
    pub fn picker(&self) -> Option<&TargetPickerSession> {
        match &self.phase {
            SpellPhase::Targeting { picker, .. } => Some(picker),
            _ => None,
        }
    }

    /// The resolved cast / abort, or `None` while still selecting.
    pub fn resolved(&self) -> Option<SpellResolution> {
        match &self.phase {
            SpellPhase::Confirmed {
                spell_id,
                target_row,
                target_slot,
            } => Some(SpellResolution::Confirmed {
                spell_id: *spell_id,
                target_row: *target_row,
                target_slot: *target_slot,
            }),
            SpellPhase::Aborted => Some(SpellResolution::Aborted),
            _ => None,
        }
    }

    /// Advance one frame. `party` / `monsters` describe slot occupancy + alive
    /// state for the target picker (rebuilt by the host from the live actor
    /// table each frame). A no-op once the session has resolved.
    pub fn input(
        &mut self,
        ev: BattleSpellInput,
        catalog: &SpellCatalog,
        party: [SlotState; 3],
        monsters: [SlotState; 5],
    ) {
        // Take ownership of the phase so the borrow checker lets us read
        // `self.spells` / `self.party_slot` while computing the next phase.
        match std::mem::replace(&mut self.phase, SpellPhase::Aborted) {
            SpellPhase::Select { cursor } => {
                self.phase = step_select(
                    cursor,
                    ev,
                    self.party_slot,
                    &self.spells,
                    catalog,
                    party,
                    monsters,
                );
            }
            SpellPhase::Targeting {
                spell_id,
                mut picker,
            } => {
                picker.input(PickerInput {
                    up: ev.up,
                    down: ev.down,
                    left: ev.left,
                    right: ev.right,
                    cross: ev.cross,
                    circle: ev.circle,
                });
                self.phase = match picker.outcome() {
                    Some(PickerOutcome::Single { slot, row }) => SpellPhase::Confirmed {
                        spell_id,
                        target_row: row,
                        target_slot: slot,
                    },
                    Some(PickerOutcome::Sweep { row }) => SpellPhase::Confirmed {
                        spell_id,
                        target_row: row,
                        target_slot: 0,
                    },
                    // Backing out of targeting returns to the spell list.
                    Some(PickerOutcome::Cancelled) => SpellPhase::Select {
                        cursor: self
                            .spells
                            .iter()
                            .position(|s| s.id == spell_id)
                            .unwrap_or(0) as u8,
                    },
                    Some(PickerOutcome::NoCandidates) => SpellPhase::Aborted,
                    None => SpellPhase::Targeting { spell_id, picker },
                };
            }
            other => self.phase = other,
        }
    }
}

/// One frame of the spell-select list. Up/Down move the cursor (wrapping);
/// Cross on an affordable spell opens its target picker (immediate kinds fold
/// in-line); Circle backs out (`Aborted`). Unaffordable / unknown spells bounce
/// with no state change.
#[allow(clippy::too_many_arguments)]
fn step_select(
    cursor: u8,
    ev: BattleSpellInput,
    party_slot: u8,
    spells: &[SpellRow],
    catalog: &SpellCatalog,
    party: [SlotState; 3],
    monsters: [SlotState; 5],
) -> SpellPhase {
    let n = spells.len();
    if n == 0 {
        // Empty spell list: Circle (or any confirm) backs out so the player
        // isn't trapped; otherwise hold the (empty) list.
        if ev.circle || ev.cross {
            return SpellPhase::Aborted;
        }
        return SpellPhase::Select { cursor: 0 };
    }
    let mut cursor = (cursor as usize).min(n - 1);
    if ev.up {
        cursor = (cursor + n - 1) % n;
    } else if ev.down {
        cursor = (cursor + 1) % n;
    }
    if ev.circle {
        return SpellPhase::Aborted;
    }
    if ev.cross {
        let row = &spells[cursor];
        if row.affordable
            && let Some(def) = catalog.get(row.id)
        {
            let picker =
                TargetPickerSession::new(target_kind_for(def.target), party_slot, party, monsters);
            // Immediate kinds (sweep / self / no-candidates) resolve in the
            // constructor; fold that here so we don't stall a frame.
            if let Some(outcome) = picker.outcome() {
                return match outcome {
                    PickerOutcome::Single { slot, row: r } => SpellPhase::Confirmed {
                        spell_id: row.id,
                        target_row: r,
                        target_slot: slot,
                    },
                    PickerOutcome::Sweep { row: r } => SpellPhase::Confirmed {
                        spell_id: row.id,
                        target_row: r,
                        target_slot: 0,
                    },
                    PickerOutcome::NoCandidates => SpellPhase::Aborted,
                    PickerOutcome::Cancelled => SpellPhase::Select {
                        cursor: cursor as u8,
                    },
                };
            }
            return SpellPhase::Targeting {
                spell_id: row.id,
                picker,
            };
        }
    }
    SpellPhase::Select {
        cursor: cursor as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spells::SpellCatalog;

    fn alive(present: bool) -> SlotState {
        SlotState::alive(present, true)
    }
    fn party3() -> [SlotState; 3] {
        [alive(true), alive(true), alive(true)]
    }
    fn one_monster() -> [SlotState; 5] {
        [
            alive(true),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
        ]
    }
    fn press(b: &str) -> BattleSpellInput {
        BattleSpellInput {
            up: b == "U",
            down: b == "D",
            cross: b == "c",
            circle: b == "o",
            ..Default::default()
        }
    }

    #[test]
    fn cursor_starts_on_first_affordable_spell() {
        let cat = SpellCatalog::vanilla();
        // Heal (0x10, cost 4) and Flame (0x20, cost 5). With 4 MP only Heal is
        // affordable -> cursor on row 0.
        let s = BattleSpellSession::new(0, 0, &[0x10, 0x20], &cat, 4, 0);
        assert_eq!(s.menu_spell().map(|r| r.id), Some(0x10));
        assert!(s.spells[0].affordable);
        assert!(!s.spells[1].affordable);
    }

    #[test]
    fn confirm_offensive_spell_then_target_resolves() {
        let cat = SpellCatalog::vanilla();
        // Flame (0x20) is single-enemy; with enough MP it opens a cursor on the
        // lone monster, then Cross confirms.
        let mut s = BattleSpellSession::new(0, 0, &[0x20], &cat, 50, 0);
        s.input(press("c"), &cat, party3(), one_monster());
        assert!(matches!(s.phase, SpellPhase::Targeting { .. }));
        s.input(press("c"), &cat, party3(), one_monster());
        assert_eq!(
            s.resolved(),
            Some(SpellResolution::Confirmed {
                spell_id: 0x20,
                target_row: CursorRow::Enemy,
                target_slot: 0,
            })
        );
    }

    #[test]
    fn heal_all_resolves_immediately_as_sweep() {
        let cat = SpellCatalog::vanilla();
        // Heal All (0x11) is AllAllies -> sweep, no cursor; Cross confirms in
        // one step.
        let mut s = BattleSpellSession::new(0, 0, &[0x11], &cat, 50, 0);
        s.input(press("c"), &cat, party3(), one_monster());
        assert!(matches!(
            s.resolved(),
            Some(SpellResolution::Confirmed { spell_id: 0x11, .. })
        ));
    }

    #[test]
    fn circle_from_spell_list_aborts() {
        let cat = SpellCatalog::vanilla();
        let mut s = BattleSpellSession::new(0, 0, &[0x10], &cat, 50, 0);
        s.input(press("o"), &cat, party3(), one_monster());
        assert_eq!(s.resolved(), Some(SpellResolution::Aborted));
    }

    #[test]
    fn unaffordable_spell_does_not_open_target() {
        let cat = SpellCatalog::vanilla();
        // Only Mega Heal (0x12, cost 12); caster has 3 MP -> not affordable.
        let mut s = BattleSpellSession::new(0, 0, &[0x12], &cat, 3, 0);
        assert!(!s.spells[0].affordable);
        s.input(press("c"), &cat, party3(), one_monster());
        // Stayed in the list (no target picker, no resolution).
        assert!(matches!(s.phase, SpellPhase::Select { .. }));
        assert!(s.resolved().is_none());
    }

    /// An MP-saver accessory (Half bit `0x20`) halves the displayed cost and
    /// makes a spell whose RAW cost exceeds the caster's MP affordable - the
    /// same reduced cost `World::cast_spell_on_slots` charges. Without the bit
    /// the spell is unaffordable; with it, both the displayed cost and the
    /// affordability flip.
    #[test]
    fn mp_saver_ability_bit_makes_pricey_spell_affordable() {
        let cat = SpellCatalog::vanilla();
        // Mega Heal (0x12, raw cost 12); caster has 7 MP.
        let raw = BattleSpellSession::new(0, 0, &[0x12], &cat, 7, 0);
        assert_eq!(raw.spells[0].mp_cost, 12);
        assert!(!raw.spells[0].affordable, "raw 12 MP > 7 MP -> blocked");

        // Half bit 0x20: effective cost 12 - (12 >> 1) = 6, now within 7 MP.
        let half = BattleSpellSession::new(0, 0, &[0x12], &cat, 7, 0x20);
        assert_eq!(
            half.spells[0].mp_cost, 6,
            "displayed cost reflects the charge"
        );
        assert!(
            half.spells[0].affordable,
            "an MP-saver makes the spell selectable"
        );
        // The cursor lands on the now-affordable row (not the row-0 fallback).
        assert_eq!(half.menu_spell().map(|r| r.id), Some(0x12));
    }
}

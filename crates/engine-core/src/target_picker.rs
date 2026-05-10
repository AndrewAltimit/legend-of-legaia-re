//! Battle target picker session.
//!
//! Drives the "after the player has picked an art / item / spell, who does
//! it apply to?" sub-flow. Mirrors the retail target cursor that pops up
//! during the CommandInput phase of the battle session.
//!
//! The picker is parameterised on a [`TargetKind`] that constrains valid
//! targets:
//!
//! - [`TargetKind::SingleEnemy`] - one alive monster slot
//! - [`TargetKind::SingleAlly`] - one alive party slot (excluding self)
//! - [`TargetKind::SingleAllyOrSelf`] - any alive party slot
//! - [`TargetKind::DeadAlly`] - one fallen party slot (Resurrection)
//! - [`TargetKind::AnyAlly`] - any party slot, alive or dead
//! - [`TargetKind::AllEnemies`] - sweep target, no cursor; immediate confirm
//! - [`TargetKind::AllAllies`] - sweep target, no cursor; immediate confirm
//! - [`TargetKind::Self_` - the actor itself; immediate confirm
//!
//! The cursor moves left/right between valid candidates; up/down (where
//! the kind allows) flips between the party row and the monster row.
//! Cross confirms; Circle aborts. The session emits typed events the
//! engine can fold into HUD blips and BattleSession.
//!
//! ## Integration
//!
//! Engines run the target picker between the BattleRunner's
//! `push_command` (which records the action constant) and the
//! `commit_turn` step. The picker's [`TargetPickerSession::outcome`]
//! provides the resolved target slot or "abort" outcome; on abort,
//! the engine pops the just-pushed command via `BattleRunner::pop_command`.

/// The kind of target the action expects. Drives validation + cursor
/// motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetKind {
    /// One live enemy slot.
    SingleEnemy,
    /// One live party slot, excluding the actor.
    SingleAlly,
    /// One live party slot, including the actor.
    SingleAllyOrSelf,
    /// One dead party slot (Revive / Resurrection items).
    DeadAlly,
    /// One party slot, alive or dead.
    AnyAlly,
    /// All enemies - auto-confirm.
    AllEnemies,
    /// All allies - auto-confirm.
    AllAllies,
    /// The actor itself - auto-confirm.
    Self_,
}

impl TargetKind {
    /// `true` when the picker has no real cursor and resolves immediately.
    pub fn is_immediate(self) -> bool {
        matches!(
            self,
            TargetKind::AllEnemies | TargetKind::AllAllies | TargetKind::Self_
        )
    }

    /// `true` when the picker walks party slots.
    pub fn picks_ally(self) -> bool {
        matches!(
            self,
            TargetKind::SingleAlly
                | TargetKind::SingleAllyOrSelf
                | TargetKind::DeadAlly
                | TargetKind::AnyAlly
                | TargetKind::AllAllies
        )
    }

    /// `true` when the picker walks monster slots.
    pub fn picks_enemy(self) -> bool {
        matches!(self, TargetKind::SingleEnemy | TargetKind::AllEnemies)
    }
}

/// Where the cursor currently sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorRow {
    Ally,
    Enemy,
}

/// State of the picker SM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerState {
    /// Cursor is live. `slot` is the cursor's currently-targeted slot
    /// (resolved against ally / enemy frame).
    Cursor { row: CursorRow, slot: u8 },
    /// Picker resolved.
    Done(PickerOutcome),
}

/// Final result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerOutcome {
    /// One slot was confirmed.
    Single { slot: u8, row: CursorRow },
    /// All enemies / allies / self - sweep target.
    Sweep { row: CursorRow },
    /// Player cancelled.
    Cancelled,
    /// No valid target existed when the picker opened - auto-cancel.
    NoCandidates,
}

/// Per-tick input bundle. Mirrors `equip_session::EquipInput` shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PickerInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub cross: bool,
    pub circle: bool,
}

/// Events emitted per `input()` call. Engines fold these into HUD blips
/// + BattleHud cursor highlight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerEvent {
    CursorMoved { row: CursorRow, slot: u8 },
    RowSwitched { row: CursorRow, slot: u8 },
    Confirmed { row: CursorRow, slot: u8 },
    SweepConfirmed { row: CursorRow },
    Cancelled,
    InvalidConfirm,
}

/// One row of slot info the picker queries.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotState {
    pub present: bool,
    pub alive: bool,
}

impl SlotState {
    pub const fn alive(present: bool, alive: bool) -> Self {
        Self { present, alive }
    }
    pub const fn dead(present: bool) -> Self {
        Self {
            present,
            alive: false,
        }
    }
}

impl SlotState {
    /// Build from a [`crate::battle_session::SessionSlotInfo`]. A slot is
    /// "present" when its `record` is populated; alive state is supplied
    /// by the caller from the live `BattleActor::hp` field.
    pub fn from_session_slot(info: &crate::battle_session::SessionSlotInfo, hp: u16) -> Self {
        Self {
            present: info.record.is_some(),
            alive: hp > 0,
        }
    }
}

/// Target picker session.
#[derive(Debug, Clone)]
pub struct TargetPickerSession {
    kind: TargetKind,
    actor_slot: u8,
    party: [SlotState; 3],
    monsters: [SlotState; 5],
    state: PickerState,
    events: Vec<PickerEvent>,
}

impl TargetPickerSession {
    /// Construct a new picker for `kind`. `actor_slot` is the party-row
    /// index (0..=2) of the action's owner - used to skip-self for
    /// [`TargetKind::SingleAlly`]. `party` and `monsters` describe slot
    /// occupancy + alive state.
    pub fn new(
        kind: TargetKind,
        actor_slot: u8,
        party: [SlotState; 3],
        monsters: [SlotState; 5],
    ) -> Self {
        let mut s = Self {
            kind,
            actor_slot,
            party,
            monsters,
            state: PickerState::Done(PickerOutcome::NoCandidates),
            events: Vec::new(),
        };
        s.init_cursor();
        s
    }

    fn init_cursor(&mut self) {
        if self.kind.is_immediate() {
            let row = if self.kind == TargetKind::AllEnemies {
                CursorRow::Enemy
            } else {
                // AllAllies / Self_ - both party-row.
                CursorRow::Ally
            };
            // Sanity: ensure at least one valid target exists. For Self_,
            // the actor is alive (caller guarantees), so always emit.
            self.state = PickerState::Done(PickerOutcome::Sweep { row });
            self.events.push(PickerEvent::SweepConfirmed { row });
            return;
        }

        // Try ally row first if the kind picks allies; otherwise enemy.
        let initial_row = if self.kind.picks_ally() && !self.kind.picks_enemy() {
            CursorRow::Ally
        } else if self.kind.picks_enemy() {
            CursorRow::Enemy
        } else {
            CursorRow::Ally
        };

        if let Some(slot) = self.first_valid_in(initial_row) {
            self.state = PickerState::Cursor {
                row: initial_row,
                slot,
            };
            return;
        }
        // Fall back to the other row.
        let alt = match initial_row {
            CursorRow::Ally => CursorRow::Enemy,
            CursorRow::Enemy => CursorRow::Ally,
        };
        if let Some(slot) = self.first_valid_in(alt) {
            self.state = PickerState::Cursor { row: alt, slot };
            return;
        }
        self.state = PickerState::Done(PickerOutcome::NoCandidates);
    }

    pub fn state(&self) -> PickerState {
        self.state
    }

    pub fn kind(&self) -> TargetKind {
        self.kind
    }

    pub fn is_done(&self) -> bool {
        matches!(self.state, PickerState::Done(_))
    }

    pub fn outcome(&self) -> Option<PickerOutcome> {
        match self.state {
            PickerState::Done(o) => Some(o),
            _ => None,
        }
    }

    pub fn drain_events(&mut self) -> Vec<PickerEvent> {
        std::mem::take(&mut self.events)
    }

    fn slot_state(&self, row: CursorRow, slot: u8) -> Option<SlotState> {
        match row {
            CursorRow::Ally => self.party.get(slot as usize).copied(),
            CursorRow::Enemy => self.monsters.get(slot as usize).copied(),
        }
    }

    fn row_len(&self, row: CursorRow) -> u8 {
        match row {
            CursorRow::Ally => self.party.len() as u8,
            CursorRow::Enemy => self.monsters.len() as u8,
        }
    }

    fn is_valid(&self, row: CursorRow, slot: u8) -> bool {
        let state = match self.slot_state(row, slot) {
            Some(s) => s,
            None => return false,
        };
        if !state.present {
            return false;
        }
        // Apply per-kind constraints.
        match self.kind {
            TargetKind::SingleEnemy => row == CursorRow::Enemy && state.alive,
            TargetKind::SingleAlly => {
                row == CursorRow::Ally && state.alive && slot != self.actor_slot
            }
            TargetKind::SingleAllyOrSelf => row == CursorRow::Ally && state.alive,
            TargetKind::DeadAlly => row == CursorRow::Ally && !state.alive,
            TargetKind::AnyAlly => row == CursorRow::Ally,
            // Immediate kinds resolve in init_cursor; not used here.
            TargetKind::AllEnemies | TargetKind::AllAllies | TargetKind::Self_ => true,
        }
    }

    fn first_valid_in(&self, row: CursorRow) -> Option<u8> {
        let len = self.row_len(row);
        (0..len).find(|&s| self.is_valid(row, s))
    }

    fn step_within_row(&self, row: CursorRow, from: u8, dir: i8) -> Option<u8> {
        let len = self.row_len(row);
        if len == 0 {
            return None;
        }
        let mut cursor = from as i16;
        for _ in 0..len {
            cursor += dir as i16;
            if cursor < 0 {
                cursor = (len as i16) - 1;
            }
            if cursor >= len as i16 {
                cursor = 0;
            }
            let s = cursor as u8;
            if self.is_valid(row, s) {
                return Some(s);
            }
        }
        None
    }

    fn other_row(row: CursorRow) -> CursorRow {
        match row {
            CursorRow::Ally => CursorRow::Enemy,
            CursorRow::Enemy => CursorRow::Ally,
        }
    }

    fn can_switch_row(&self, from: CursorRow) -> bool {
        // SingleEnemy never lets you switch to ally row.
        // SingleAlly / DeadAlly / AnyAlly / SingleAllyOrSelf never switch to enemy.
        match self.kind {
            TargetKind::SingleEnemy => false,
            TargetKind::SingleAlly
            | TargetKind::DeadAlly
            | TargetKind::AnyAlly
            | TargetKind::SingleAllyOrSelf => false,
            _ => false, // sweep kinds resolve in init_cursor; we never reach here with them.
        }
        .then_some(())
        .map(|_| true)
        .unwrap_or_else(|| {
            // The dummy match above always returns false - but we want to
            // future-proof: allow row switching if the kind picks both.
            // None of the current variants do, so this is effectively false.
            let _ = from;
            false
        })
    }

    /// Drive the cursor for one frame.
    pub fn input(&mut self, input: PickerInput) {
        let (row, slot) = match self.state {
            PickerState::Cursor { row, slot } => (row, slot),
            PickerState::Done(_) => return,
        };

        if input.circle {
            self.state = PickerState::Done(PickerOutcome::Cancelled);
            self.events.push(PickerEvent::Cancelled);
            return;
        }

        if input.cross {
            if self.is_valid(row, slot) {
                self.state = PickerState::Done(PickerOutcome::Single { slot, row });
                self.events.push(PickerEvent::Confirmed { row, slot });
            } else {
                self.events.push(PickerEvent::InvalidConfirm);
            }
            return;
        }

        // Cursor motion. Left/Right step within the row.
        if input.left {
            if let Some(s) = self.step_within_row(row, slot, -1)
                && s != slot
            {
                self.state = PickerState::Cursor { row, slot: s };
                self.events.push(PickerEvent::CursorMoved { row, slot: s });
            }
            return;
        }
        if input.right {
            if let Some(s) = self.step_within_row(row, slot, 1)
                && s != slot
            {
                self.state = PickerState::Cursor { row, slot: s };
                self.events.push(PickerEvent::CursorMoved { row, slot: s });
            }
            return;
        }

        // Up/Down switch row when the kind allows it.
        if (input.up || input.down) && self.can_switch_row(row) {
            let new_row = Self::other_row(row);
            if let Some(s) = self.first_valid_in(new_row) {
                self.state = PickerState::Cursor {
                    row: new_row,
                    slot: s,
                };
                self.events.push(PickerEvent::RowSwitched {
                    row: new_row,
                    slot: s,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_party() -> [SlotState; 3] {
        [SlotState::alive(true, true); 3]
    }

    fn full_monsters() -> [SlotState; 5] {
        [SlotState::alive(true, true); 5]
    }

    #[test]
    fn single_enemy_initial_cursor_at_first_alive() {
        let mut monsters = full_monsters();
        monsters[0].alive = false;
        let p = TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), monsters);
        match p.state() {
            PickerState::Cursor {
                row: CursorRow::Enemy,
                slot,
            } => assert_eq!(slot, 1),
            _ => panic!("expected enemy cursor"),
        }
    }

    #[test]
    fn single_enemy_skips_dead_when_stepping() {
        let mut monsters = full_monsters();
        monsters[1].alive = false;
        monsters[2].alive = false;
        let mut p = TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), monsters);
        // Cursor starts at 0; step right → should jump to 3.
        p.input(PickerInput {
            right: true,
            ..Default::default()
        });
        match p.state() {
            PickerState::Cursor { slot: 3, .. } => {}
            other => panic!("expected slot 3, got {other:?}"),
        }
    }

    #[test]
    fn single_ally_excludes_self() {
        let mut p =
            TargetPickerSession::new(TargetKind::SingleAlly, 1, full_party(), full_monsters());
        // Cursor starts at first valid slot != 1.
        match p.state() {
            PickerState::Cursor {
                row: CursorRow::Ally,
                slot,
            } => assert_eq!(slot, 0),
            _ => panic!("expected ally cursor at 0"),
        }
        // Step right → 2 (skipping self at 1).
        p.input(PickerInput {
            right: true,
            ..Default::default()
        });
        match p.state() {
            PickerState::Cursor { slot, .. } => assert_eq!(slot, 2),
            _ => panic!(),
        }
    }

    #[test]
    fn single_ally_or_self_includes_actor() {
        let mut p = TargetPickerSession::new(
            TargetKind::SingleAllyOrSelf,
            1,
            full_party(),
            full_monsters(),
        );
        // Step from 0 → 1, includes self.
        p.input(PickerInput {
            right: true,
            ..Default::default()
        });
        match p.state() {
            PickerState::Cursor { slot, .. } => assert_eq!(slot, 1),
            _ => panic!(),
        }
    }

    #[test]
    fn dead_ally_only_picks_dead_slots() {
        let mut party = full_party();
        party[0].alive = false;
        party[2].alive = false;
        let mut p = TargetPickerSession::new(TargetKind::DeadAlly, 1, party, full_monsters());
        // Cursor starts at first dead slot.
        match p.state() {
            PickerState::Cursor { slot, .. } => assert_eq!(slot, 0),
            _ => panic!(),
        }
        // Step right → 2 (skipping the live slot at 1).
        p.input(PickerInput {
            right: true,
            ..Default::default()
        });
        match p.state() {
            PickerState::Cursor { slot, .. } => assert_eq!(slot, 2),
            _ => panic!(),
        }
    }

    #[test]
    fn no_candidates_when_all_dead() {
        let p = TargetPickerSession::new(
            TargetKind::SingleEnemy,
            0,
            full_party(),
            [SlotState::dead(true); 5],
        );
        assert_eq!(p.outcome(), Some(PickerOutcome::NoCandidates));
    }

    #[test]
    fn confirm_emits_single_outcome() {
        let mut p =
            TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), full_monsters());
        p.input(PickerInput {
            cross: true,
            ..Default::default()
        });
        match p.outcome().unwrap() {
            PickerOutcome::Single { slot, row } => {
                assert_eq!(slot, 0);
                assert_eq!(row, CursorRow::Enemy);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn cancel_emits_cancelled_outcome() {
        let mut p =
            TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), full_monsters());
        p.input(PickerInput {
            circle: true,
            ..Default::default()
        });
        assert_eq!(p.outcome(), Some(PickerOutcome::Cancelled));
    }

    #[test]
    fn all_enemies_immediate_sweep() {
        let p = TargetPickerSession::new(TargetKind::AllEnemies, 0, full_party(), full_monsters());
        match p.outcome().unwrap() {
            PickerOutcome::Sweep {
                row: CursorRow::Enemy,
            } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn all_allies_immediate_sweep() {
        let p = TargetPickerSession::new(TargetKind::AllAllies, 0, full_party(), full_monsters());
        match p.outcome().unwrap() {
            PickerOutcome::Sweep {
                row: CursorRow::Ally,
            } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn self_target_immediate() {
        let p = TargetPickerSession::new(TargetKind::Self_, 0, full_party(), full_monsters());
        match p.outcome().unwrap() {
            PickerOutcome::Sweep {
                row: CursorRow::Ally,
            } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn cursor_emits_event_on_move() {
        let mut p =
            TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), full_monsters());
        let _ = p.drain_events();
        p.input(PickerInput {
            right: true,
            ..Default::default()
        });
        let evs = p.drain_events();
        assert_eq!(evs.len(), 1);
        match evs[0] {
            PickerEvent::CursorMoved { slot: 1, .. } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn cursor_wraps_around_within_row() {
        let mut p =
            TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), full_monsters());
        // 0 → left → wraps to 4.
        p.input(PickerInput {
            left: true,
            ..Default::default()
        });
        match p.state() {
            PickerState::Cursor { slot, .. } => assert_eq!(slot, 4),
            _ => panic!(),
        }
    }

    #[test]
    fn invalid_confirm_when_initial_state_already_done() {
        let p = TargetPickerSession::new(
            TargetKind::SingleEnemy,
            0,
            full_party(),
            [SlotState::dead(true); 5],
        );
        // Already NoCandidates.
        assert!(p.is_done());
        assert_eq!(p.outcome(), Some(PickerOutcome::NoCandidates));
    }

    #[test]
    fn input_after_done_is_noop() {
        let mut p = TargetPickerSession::new(TargetKind::Self_, 0, full_party(), full_monsters());
        let evs_before = p.drain_events();
        assert!(!evs_before.is_empty());
        p.input(PickerInput {
            cross: true,
            ..Default::default()
        });
        // No new events.
        assert!(p.drain_events().is_empty());
    }

    #[test]
    fn target_kind_immediacy() {
        assert!(TargetKind::AllEnemies.is_immediate());
        assert!(TargetKind::AllAllies.is_immediate());
        assert!(TargetKind::Self_.is_immediate());
        assert!(!TargetKind::SingleEnemy.is_immediate());
    }
}

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

// ---------------------------------------------------------------------------
// Enemy target-menu rows
// ---------------------------------------------------------------------------

/// The formation table the menu builder walks - four monster-id bytes.
pub const FORMATION_SLOTS: usize = 4;

/// Menu row stride in the retail context block (`row * 0x20 + 0x29` is the row's
/// name field).
pub const MENU_ROW_STRIDE: usize = 0x20;

/// Screen-space centre the row X positions are built around.
pub const MENU_CENTRE_X: i16 = 0xA0;

/// Left clamp for a row's X (`0x801da144`: `slti v0,v0,0x6`).
pub const MENU_MIN_X: i16 = 6;

/// Right edge a row's `x + text_width` is clamped against
/// (`0x801da12c`: `li s1,0x13a`).
pub const MENU_MAX_RIGHT: i16 = 0x13A;

/// Minimum gap the overlap relaxation opens between two rows, on top of the
/// left row's own text width (`0x801da050`: `addiu v0,v0,0x14`).
pub const MENU_ROW_GAP: i16 = 0x14;

/// Y the rows are drawn at (`0x801da1fc`: the fifth argument `0x30`).
pub const MENU_ROW_Y: i16 = 0x30;

/// Dedup-glyph stand-in for callers with no font mapping to hand.
///
/// Retail's glyph is a one-character string literal in the battle overlay's
/// rodata at `0x801CECA8`, in the game's own text encoding; it is disc data, so
/// the port takes it as a parameter and only defaults to a Latin `'A'` (which
/// increments the same way its successors do) when the caller has nothing
/// better.
pub const DEDUP_GLYPH_FALLBACK: u8 = b'A';

/// Row height (`0x801da208`: the seventh argument `0xC`).
pub const MENU_ROW_HEIGHT: i16 = 0xC;

/// One row of the enemy target-selection menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyMenuRow {
    /// The monster id the row was opened for.
    pub monster_id: u8,
    /// Formation slot the row's **first** member occupies - the slot the cursor
    /// resolves to.
    pub first_slot: u8,
    /// Members collapsed into this row (consecutive identical ids).
    pub members: u8,
    /// The row's label. The first member contributes the plain name; each
    /// further member replaces the label's last character with a **dedup
    /// glyph** and then increments it, so a run of three reads
    /// `name`, `nam<g>`, `nam<g+1>`.
    pub label: String,
    /// Screen X, once [`layout_enemy_menu_rows`] has run. Before that it is the
    /// running sum of the members' projected positions.
    pub x: i16,
}

/// Build the deduplicated row set for the enemy target menu.
///
/// PORT: FUN_801D9D3C (`0x801d9d84..0x801d9f0c`).
///
/// `formation` is the four-byte monster-id table `_DAT_8007BD0C`; a zero id is
/// an empty slot and is skipped without ending the walk. `name_of` supplies the
/// per-slot display name (retail copies it from the battle actor's `+0x1BC`),
/// and `dedup_glyph` is the one-character suffix literal the overlay keeps in
/// its rodata at `0x801CECA8`.
///
/// Consecutive identical ids collapse into one row. The **run counter resets on
/// any id change**, so a formation `A A B A` produces three rows (`A`x2, `B`,
/// `A`), not two - the dedup is positional, not a set operation.
///
/// The suffix arithmetic is the part worth stating precisely, because it is
/// destructive: the second member of a run does not *append* a marker, it
/// overwrites the label's final character with `dedup_glyph`
/// (`0x801d9e54` stores a `0` over the last byte before the concat), and the
/// third and later members **increment that character in place**
/// (`0x801d9ea0`). So the labels stay the same byte length as the plain name.
///
/// `projected_x` is each slot's projected screen position (the battle actor's
/// `+0x34` word); the builder accumulates it per row so
/// [`layout_enemy_menu_rows`] can average it.
pub fn enemy_menu_rows(
    formation: [u8; FORMATION_SLOTS],
    dedup_glyph: u8,
    mut name_of: impl FnMut(u8) -> String,
    mut projected_x: impl FnMut(u8) -> i16,
) -> Vec<EnemyMenuRow> {
    let mut rows: Vec<EnemyMenuRow> = Vec::new();
    let mut run = 0u8;
    for slot in 0..FORMATION_SLOTS {
        let id = formation[slot];
        if id == 0 {
            continue;
        }
        let same_as_prev = slot > 0 && formation[slot - 1] == id;
        run = if same_as_prev { run + 1 } else { 0 };
        let px = projected_x(slot as u8);
        if run == 0 {
            rows.push(EnemyMenuRow {
                monster_id: id,
                first_slot: slot as u8,
                members: 1,
                label: name_of(slot as u8),
                x: px,
            });
            continue;
        }
        let Some(row) = rows.last_mut() else {
            continue; // retail would index row -1; a leading run cannot happen
        };
        if run == 1 {
            // Overwrite the final character with the dedup glyph.
            let mut bytes = row.label.clone().into_bytes();
            if let Some(last) = bytes.last_mut() {
                *last = dedup_glyph;
            }
            row.label = String::from_utf8_lossy(&bytes).into_owned();
            // Retail bumps the run counter a second time here, which is what
            // makes the *next* member take the increment arm.
            run += 1;
        } else {
            let mut bytes = row.label.clone().into_bytes();
            if let Some(last) = bytes.last_mut() {
                *last = last.wrapping_add(1);
            }
            row.label = String::from_utf8_lossy(&bytes).into_owned();
        }
        row.members += 1;
        row.x = row.x.wrapping_add(px);
    }
    rows
}

/// Place the enemy menu rows across the screen.
///
/// PORT: FUN_801D9D3C (`0x801d9f1c..0x801da1ac`).
///
/// Three passes, in order:
///
/// 1. **Average + centre.** Each row's accumulated projected X is divided by its
///    member count, scaled down by `>> 3`, then converted to a left edge:
///    `x = (avg >> 3) - text_width / 2 + 0xA0`.
/// 2. **Overlap relaxation.** Every unordered pair is compared; when the left
///    row's `x + text_width + 0x14` reaches into the right row's `x`, the
///    overlap is **split evenly** - each row moves by half of it, in opposite
///    directions. Which row counts as "left" is decided per pair by their
///    current X, so the pass is order-independent.
/// 3. **Clamp.** Rows are pushed inside `[0x06, 0x13A - text_width]`.
///
/// Passes 2 and 3 **repeat as a unit** until pass 3 changes nothing
/// (`0x801da1ac`), so a clamp that re-introduces an overlap is re-relaxed. With
/// fewer than two rows retail skips straight past both passes, so a single row
/// keeps its raw centred position and is never clamped.
///
/// `text_width_of` measures a row's label in pixels (retail's `FUN_80035F04`).
pub fn layout_enemy_menu_rows(
    rows: &mut [EnemyMenuRow],
    mut text_width_of: impl FnMut(&str) -> i16,
) {
    for row in rows.iter_mut() {
        let avg = if row.members == 0 {
            0
        } else {
            row.x / i16::from(row.members)
        };
        row.x = (avg >> 3) - text_width_of(&row.label) / 2 + MENU_CENTRE_X;
    }
    if rows.len() < 2 {
        return;
    }
    // Retail's outer loop is unbounded; it terminates because each iteration
    // either clamps (and so is followed by another) or does not (and stops).
    // The bound here is a safety net, not a behavioural change.
    for _ in 0..rows.len() * 8 + 8 {
        for i in 0..rows.len() - 1 {
            for j in i + 1..rows.len() {
                let (left, right) = if rows[j].x < rows[i].x {
                    (j, i)
                } else {
                    (i, j)
                };
                let reach = rows[left]
                    .x
                    .wrapping_add(text_width_of(&rows[left].label.clone()))
                    .wrapping_add(MENU_ROW_GAP);
                if reach < rows[right].x {
                    continue;
                }
                let half = (reach - rows[right].x) >> 1;
                rows[left].x = rows[left].x.wrapping_sub(half);
                rows[right].x = rows[right].x.wrapping_add(half);
            }
        }
        let mut clamped = false;
        for row in rows.iter_mut() {
            if row.x < MENU_MIN_X {
                row.x = MENU_MIN_X;
                clamped = true;
            }
            let right_limit = MENU_MAX_RIGHT - (text_width_of(&row.label) & 0xFF);
            if right_limit < row.x {
                row.x = right_limit;
                clamped = true;
            }
        }
        if !clamped {
            return;
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

    /// Fixed-pitch stand-in for the retail proportional measurer.
    fn width(s: &str) -> i16 {
        s.chars().count() as i16 * 8
    }

    fn rows_for(formation: [u8; 4], names: &[&str; 4], px: [i16; 4]) -> Vec<EnemyMenuRow> {
        let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        enemy_menu_rows(
            formation,
            b'A',
            |slot| names[slot as usize].clone(),
            |slot| px[slot as usize],
        )
    }

    #[test]
    fn distinct_monsters_each_get_their_own_row() {
        let r = rows_for(
            [1, 2, 3, 4],
            &["Bee", "Wasp", "Slug", "Bat"],
            [100, 200, 300, 400],
        );
        assert_eq!(r.len(), 4);
        assert_eq!(
            r.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["Bee", "Wasp", "Slug", "Bat"]
        );
        assert!(r.iter().all(|x| x.members == 1));
        assert_eq!(
            r.iter().map(|x| x.first_slot).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn a_run_collapses_and_the_suffix_overwrites_then_increments() {
        let r = rows_for([7, 7, 7, 0], &["Bee", "Bee", "Bee", ""], [80, 96, 112, 0]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].members, 3);
        // Member 2 replaces the final character; member 3 increments it. The
        // label never grows.
        assert_eq!(r[0].label, "BeB");
        assert_eq!(r[0].label.len(), "Bee".len());
        // The accumulator sums the members' projected positions.
        assert_eq!(r[0].x, 80 + 96 + 112);
        assert_eq!(r[0].first_slot, 0);
    }

    #[test]
    fn the_dedup_is_positional_so_a_b_a_makes_three_rows() {
        let r = rows_for([5, 5, 9, 5], &["Bee", "Bee", "Bat", "Bee"], [0; 4]);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].members, 2);
        assert_eq!(r[1].members, 1);
        assert_eq!(r[2].members, 1);
        // The trailing repeat starts a fresh row with the plain name.
        assert_eq!(r[2].label, "Bee");
    }

    #[test]
    fn empty_formation_slots_are_skipped_without_ending_the_walk() {
        let r = rows_for([1, 0, 2, 0], &["Bee", "", "Bat", ""], [0; 4]);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].first_slot, 2);
        assert!(rows_for([0; 4], &["", "", "", ""], [0; 4]).is_empty());
    }

    #[test]
    fn layout_centres_a_single_row_and_never_clamps_it() {
        let mut r = rows_for([1, 0, 0, 0], &["Bee", "", "", ""], [8 * 160, 0, 0, 0]);
        layout_enemy_menu_rows(&mut r, width);
        // (8*160 / 1) >> 3 == 160, minus half the 24px label, plus 0xA0.
        assert_eq!(r[0].x, 160 - 12 + 0xA0);
    }

    #[test]
    fn layout_averages_a_runs_members() {
        let mut r = rows_for([1, 1, 0, 0], &["Bee", "Bee", "", ""], [800, 1600, 0, 0]);
        layout_enemy_menu_rows(&mut r, width);
        // avg 1200 >> 3 = 150, label "BeB" is 24px wide.
        assert_eq!(r[0].x, 150 - 12 + 0xA0);
    }

    #[test]
    fn layout_pushes_overlapping_rows_apart_symmetrically() {
        // Two rows whose raw centres coincide.
        let mut r = rows_for([1, 2, 0, 0], &["Bee", "Bat", "", ""], [800, 800, 0, 0]);
        layout_enemy_menu_rows(&mut r, width);
        let gap = (r[1].x - r[0].x).abs();
        assert!(
            gap >= width("Bee") + MENU_ROW_GAP - 1,
            "rows still overlap: {:?}",
            r.iter().map(|x| x.x).collect::<Vec<_>>()
        );
        // The split is even, so the pair stays centred on where it started.
        let raw = 100 - 12 + 0xA0;
        assert_eq!((r[0].x + r[1].x) / 2, raw);
    }

    #[test]
    fn layout_clamps_rows_inside_the_screen() {
        // Four rows all crowded to the far right; relaxation plus clamping has
        // to fit them inside [6, 0x13A - width].
        let mut r = rows_for(
            [1, 2, 3, 4],
            &["Aaaaaa", "Bbbbbb", "Cccccc", "Dddddd"],
            [8 * 300; 4],
        );
        layout_enemy_menu_rows(&mut r, width);
        for row in &r {
            assert!(row.x >= MENU_MIN_X, "{row:?}");
            assert!(
                row.x + width(&row.label) <= MENU_MAX_RIGHT,
                "{row:?} runs past the right edge"
            );
        }
    }
}

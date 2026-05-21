//! Player-driven battle Arts submenu.
//!
//! Sibling of [`crate::battle_magic::BattleSpellSession`] for the **Arts**
//! command. In Legaia an Art is a chain of directional inputs; this engine
//! stores them as named [`legaia_save::SavedChainRecord`]s in the per-character
//! chain library. The Arts submenu lists the caster's saved chains, opens a
//! single-enemy [`crate::target_picker::TargetPickerSession`], and resolves to
//! a `(art_index, target)` the live loop executes as a **multi-hit strike** -
//! one generic hit per command in the chain (longer chain = more hits, the
//! Legaia shape) - then cycles the turn at `EndOfAction`.
//!
//! The full art-data resolution (per-art power bytes, hit cues, enemy effects
//! from the `legaia-art` records) is a heavier path that the richer
//! [`crate::battle_session`] runner drives; the live loop uses the
//! chain-length hit-count stand-in so a saved chain is playable without the
//! art-record tables wired in.
//!
//! The World owns the session because building the chain list needs the
//! caster's saved-chain library. The session is renderer- and world-agnostic.

use crate::target_picker::{
    CursorRow, PickerInput, PickerOutcome, SlotState, TargetKind, TargetPickerSession,
};

/// Maximum hits one art chain resolves to in the live loop, so a pathological
/// saved chain can't deal unbounded damage in a single turn.
pub const MAX_ART_HITS: u8 = 16;

/// One selectable art (saved chain) row in the battle Arts menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtRow {
    /// Display name of the saved chain.
    pub name: String,
    /// Number of strikes the chain resolves to (its non-terminator command
    /// count, clamped to `1..=MAX_ART_HITS`).
    pub hits: u8,
}

/// Sub-phase of the battle Arts submenu.
#[derive(Debug, Clone)]
pub enum ArtsPhase {
    /// Browsing the saved-chain list. `cursor` indexes
    /// [`BattleArtsSession::arts`].
    Select { cursor: u8 },
    /// An art is chosen; picking its target.
    Targeting {
        art_index: u8,
        picker: TargetPickerSession,
    },
    /// Resolved: the live loop should execute art `art_index` against the
    /// target.
    Confirmed {
        art_index: u8,
        target_row: CursorRow,
        target_slot: u8,
    },
    /// Backed out of the list (Circle, or no saved chain to run) - the live
    /// loop reopens the command menu.
    Aborted,
}

/// Per-frame, edge-triggered pad bundle for the arts session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleArtsInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Confirm (Cross).
    pub cross: bool,
    /// Cancel / back (Circle).
    pub circle: bool,
}

/// One caster's art-selection session, driven a frame at a time.
#[derive(Debug, Clone)]
pub struct BattleArtsSession {
    /// Actor-table index of the casting party member.
    pub actor: u8,
    /// Party-row index (0..=2) of the caster.
    pub party_slot: u8,
    /// The caster's saved chains (filtered to this character), in library order.
    pub arts: Vec<ArtRow>,
    pub phase: ArtsPhase,
}

/// Outcome of a resolved [`BattleArtsSession`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsResolution {
    /// The player confirmed an art execution.
    Confirmed {
        art_index: u8,
        target_row: CursorRow,
        target_slot: u8,
    },
    /// The player backed out; the live loop reopens the command menu.
    Aborted,
}

/// Count a packed chain's non-terminator commands, clamped to a usable hit
/// count. `0` bytes are terminators / empty slots.
pub fn chain_hit_count(sequence: &[u8]) -> u8 {
    let n = sequence.iter().filter(|&&b| b != 0).count();
    (n as u8).clamp(1, MAX_ART_HITS)
}

impl BattleArtsSession {
    /// Build from the caster's saved chains. Only chains whose `char_slot`
    /// matches `actor` are listed; each row's hit count is the chain's
    /// non-terminator command count. The cursor starts at row 0.
    pub fn new(actor: u8, party_slot: u8, chains: &[legaia_save::SavedChainRecord]) -> Self {
        let arts: Vec<ArtRow> = chains
            .iter()
            .filter(|c| c.char_slot == actor)
            .map(|c| ArtRow {
                name: c.name.clone(),
                hits: chain_hit_count(&c.sequence),
            })
            .collect();
        Self {
            actor,
            party_slot,
            arts,
            phase: ArtsPhase::Select { cursor: 0 },
        }
    }

    /// The art row currently under the select cursor, or `None` once the
    /// session has left the list.
    pub fn menu_art(&self) -> Option<&ArtRow> {
        match self.phase {
            ArtsPhase::Select { cursor } => self.arts.get(cursor as usize),
            _ => None,
        }
    }

    /// The active target picker, while one is open.
    pub fn picker(&self) -> Option<&TargetPickerSession> {
        match &self.phase {
            ArtsPhase::Targeting { picker, .. } => Some(picker),
            _ => None,
        }
    }

    /// The resolved execution / abort, or `None` while still selecting.
    pub fn resolved(&self) -> Option<ArtsResolution> {
        match &self.phase {
            ArtsPhase::Confirmed {
                art_index,
                target_row,
                target_slot,
            } => Some(ArtsResolution::Confirmed {
                art_index: *art_index,
                target_row: *target_row,
                target_slot: *target_slot,
            }),
            ArtsPhase::Aborted => Some(ArtsResolution::Aborted),
            _ => None,
        }
    }

    /// Advance one frame. `party` / `monsters` describe slot occupancy + alive
    /// state for the target picker. A no-op once the session has resolved.
    pub fn input(&mut self, ev: BattleArtsInput, party: [SlotState; 3], monsters: [SlotState; 5]) {
        match std::mem::replace(&mut self.phase, ArtsPhase::Aborted) {
            ArtsPhase::Select { cursor } => {
                self.phase = step_select(cursor, ev, self.party_slot, &self.arts, party, monsters);
            }
            ArtsPhase::Targeting {
                art_index,
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
                    Some(PickerOutcome::Single { slot, row }) => ArtsPhase::Confirmed {
                        art_index,
                        target_row: row,
                        target_slot: slot,
                    },
                    Some(PickerOutcome::Sweep { row }) => ArtsPhase::Confirmed {
                        art_index,
                        target_row: row,
                        target_slot: 0,
                    },
                    Some(PickerOutcome::Cancelled) => ArtsPhase::Select { cursor: art_index },
                    Some(PickerOutcome::NoCandidates) => ArtsPhase::Aborted,
                    None => ArtsPhase::Targeting { art_index, picker },
                };
            }
            other => self.phase = other,
        }
    }
}

/// One frame of the art-select list. Up/Down move the cursor (wrapping); Cross
/// opens a single-enemy target picker for the highlighted art; Circle backs
/// out. An empty list backs out on any confirm so the player isn't trapped.
fn step_select(
    cursor: u8,
    ev: BattleArtsInput,
    party_slot: u8,
    arts: &[ArtRow],
    party: [SlotState; 3],
    monsters: [SlotState; 5],
) -> ArtsPhase {
    let n = arts.len();
    if n == 0 {
        if ev.circle || ev.cross {
            return ArtsPhase::Aborted;
        }
        return ArtsPhase::Select { cursor: 0 };
    }
    let mut cursor = (cursor as usize).min(n - 1);
    if ev.up {
        cursor = (cursor + n - 1) % n;
    } else if ev.down {
        cursor = (cursor + 1) % n;
    }
    if ev.circle {
        return ArtsPhase::Aborted;
    }
    if ev.cross {
        let picker = TargetPickerSession::new(TargetKind::SingleEnemy, party_slot, party, monsters);
        if let Some(outcome) = picker.outcome() {
            return match outcome {
                PickerOutcome::Single { slot, row } => ArtsPhase::Confirmed {
                    art_index: cursor as u8,
                    target_row: row,
                    target_slot: slot,
                },
                PickerOutcome::Sweep { row } => ArtsPhase::Confirmed {
                    art_index: cursor as u8,
                    target_row: row,
                    target_slot: 0,
                },
                PickerOutcome::NoCandidates => ArtsPhase::Aborted,
                PickerOutcome::Cancelled => ArtsPhase::Select {
                    cursor: cursor as u8,
                },
            };
        }
        return ArtsPhase::Targeting {
            art_index: cursor as u8,
            picker,
        };
    }
    ArtsPhase::Select {
        cursor: cursor as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_save::SavedChainRecord;

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
    fn press(b: &str) -> BattleArtsInput {
        BattleArtsInput {
            up: b == "U",
            down: b == "D",
            cross: b == "c",
            circle: b == "o",
            ..Default::default()
        }
    }
    fn chain(char_slot: u8, name: &str, seq: &[u8]) -> SavedChainRecord {
        SavedChainRecord {
            char_slot,
            name: name.into(),
            sequence: seq.to_vec(),
        }
    }

    #[test]
    fn hit_count_counts_nonterminator_commands() {
        assert_eq!(chain_hit_count(&[1, 2, 3]), 3);
        assert_eq!(chain_hit_count(&[1, 2, 0, 0]), 2);
        assert_eq!(chain_hit_count(&[]), 1, "floors at one hit");
        assert_eq!(chain_hit_count(&[0; 64]), 1);
        assert_eq!(chain_hit_count(&[7; 64]), MAX_ART_HITS, "clamped");
    }

    #[test]
    fn lists_only_the_casters_chains() {
        let chains = [
            chain(0, "Vahn-A", &[1, 2]),
            chain(1, "Noa-A", &[3, 4, 1]),
            chain(0, "Vahn-B", &[1, 1, 1, 1]),
        ];
        let s = BattleArtsSession::new(0, 0, &chains);
        assert_eq!(s.arts.len(), 2);
        assert_eq!(s.arts[0].name, "Vahn-A");
        assert_eq!(s.arts[1].hits, 4);
    }

    #[test]
    fn confirm_art_then_target_resolves() {
        let chains = [chain(0, "Vahn-A", &[1, 2, 3])];
        let mut s = BattleArtsSession::new(0, 0, &chains);
        s.input(press("c"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsPhase::Targeting { .. }));
        s.input(press("c"), party3(), one_monster());
        assert_eq!(
            s.resolved(),
            Some(ArtsResolution::Confirmed {
                art_index: 0,
                target_row: CursorRow::Enemy,
                target_slot: 0,
            })
        );
    }

    #[test]
    fn circle_aborts_and_empty_list_aborts_on_confirm() {
        let chains = [chain(0, "Vahn-A", &[1])];
        let mut s = BattleArtsSession::new(0, 0, &chains);
        s.input(press("o"), party3(), one_monster());
        assert_eq!(s.resolved(), Some(ArtsResolution::Aborted));

        // A caster with no chains backs out on Cross.
        let mut empty = BattleArtsSession::new(2, 2, &chains);
        assert!(empty.arts.is_empty());
        empty.input(press("c"), party3(), one_monster());
        assert_eq!(empty.resolved(), Some(ArtsResolution::Aborted));
    }
}

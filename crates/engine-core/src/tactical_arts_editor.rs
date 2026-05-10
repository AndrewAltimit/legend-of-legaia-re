//! Tactical Arts chain editor + per-character chain library.
//!
//! Drives the menu screen where the player composes a Tactical Arts
//! command chain (3..=7 directional inputs), saves it to a per-character
//! library, and recalls it in battle. The retail engine stores up to
//! eight saved chains per character; we mirror that.
//!
//! ## Components
//!
//! - [`SavedChain`] - one chain (name + sequence of [`Command`] inputs).
//! - [`ChainLibrary`] - per-character ring buffer of saved chains, with
//!   `MAX_SLOTS` capacity.
//! - [`ChainEditor`] - the editor state machine: `Browsing → Editing →
//!   Naming → Done`. Engines feed it [`EditInput`] per frame; events
//!   surface as [`EditEvent`].
//!
//! On battle start, engines pull the active character's
//! [`ChainLibrary::saved`] and render the recall list. Selecting a saved
//! chain pushes the sequence through `BattleRunner::push_command` /
//! `push_chained_art`.

use legaia_art::queue::Command;
use std::collections::HashMap;

/// A single saved Tactical Arts chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedChain {
    pub name: String,
    pub sequence: Vec<Command>,
}

impl SavedChain {
    pub fn new(name: impl Into<String>, sequence: Vec<Command>) -> Self {
        Self {
            name: name.into(),
            sequence,
        }
    }

    pub fn len(&self) -> usize {
        self.sequence.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sequence.is_empty()
    }

    /// Format the chain as a one-line string (e.g. `"L R D U R"`).
    pub fn pretty_sequence(&self) -> String {
        self.sequence
            .iter()
            .map(|c| match c {
                Command::Left => "L",
                Command::Right => "R",
                Command::Up => "U",
                Command::Down => "D",
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Per-character chain library.
///
/// Indexed by `char_slot` (0..=2 for the main party). Each character
/// has up to [`Self::MAX_SLOTS`] saved chains.
#[derive(Debug, Default, Clone)]
pub struct ChainLibrary {
    slots: HashMap<u8, Vec<SavedChain>>,
}

impl ChainLibrary {
    /// Maximum saved chains per character (matches retail).
    pub const MAX_SLOTS: usize = 8;

    /// Minimum / maximum chain length the editor enforces.
    pub const MIN_LEN: usize = 3;
    pub const MAX_LEN: usize = 7;

    pub fn new() -> Self {
        Self::default()
    }

    /// Saved chains for a character. Empty slice if none.
    pub fn saved(&self, char_slot: u8) -> &[SavedChain] {
        self.slots
            .get(&char_slot)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Total saved chains across all characters.
    pub fn total_count(&self) -> usize {
        self.slots.values().map(Vec::len).sum()
    }

    /// `true` if `char_slot`'s library is full.
    pub fn is_full(&self, char_slot: u8) -> bool {
        self.saved(char_slot).len() >= Self::MAX_SLOTS
    }

    /// Append a chain to a character's library. Returns `Err(())` if the
    /// library is full or the chain length is out of range.
    pub fn save(&mut self, char_slot: u8, chain: SavedChain) -> Result<(), SaveError> {
        if chain.len() < Self::MIN_LEN || chain.len() > Self::MAX_LEN {
            return Err(SaveError::InvalidLength(chain.len()));
        }
        let list = self.slots.entry(char_slot).or_default();
        if list.len() >= Self::MAX_SLOTS {
            return Err(SaveError::LibraryFull);
        }
        list.push(chain);
        Ok(())
    }

    /// Replace an existing chain. Out-of-bounds index returns an error.
    pub fn replace(
        &mut self,
        char_slot: u8,
        index: usize,
        chain: SavedChain,
    ) -> Result<(), SaveError> {
        if chain.len() < Self::MIN_LEN || chain.len() > Self::MAX_LEN {
            return Err(SaveError::InvalidLength(chain.len()));
        }
        let list = self.slots.entry(char_slot).or_default();
        if index >= list.len() {
            return Err(SaveError::IndexOutOfBounds(index));
        }
        list[index] = chain;
        Ok(())
    }

    /// Remove a chain. Returns the removed entry or `None` if oob.
    pub fn remove(&mut self, char_slot: u8, index: usize) -> Option<SavedChain> {
        let list = self.slots.get_mut(&char_slot)?;
        if index >= list.len() {
            return None;
        }
        Some(list.remove(index))
    }
}

/// Reasons a save can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveError {
    InvalidLength(usize),
    LibraryFull,
    IndexOutOfBounds(usize),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::InvalidLength(n) => write!(f, "chain length {n} out of range"),
            SaveError::LibraryFull => write!(f, "library full"),
            SaveError::IndexOutOfBounds(i) => write!(f, "index {i} out of bounds"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Phase of the editor state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorPhase {
    /// Browsing the saved-chain list. Up/Down move the cursor; Cross
    /// edits the highlighted chain (or "+ New" at the bottom);
    /// Triangle deletes; Circle exits.
    Browsing {
        cursor: u8,
    },
    /// Editing the byte sequence. Direction buttons append; Triangle
    /// pops; Cross commits to the naming step (when length is valid).
    Editing {
        working: Vec<Command>,
    },
    /// Picking a name. The retail UI cycles through canned names; we
    /// expose `name` for engines to render. Cross confirms; Circle
    /// returns to Editing.
    Naming {
        working: Vec<Command>,
        name: String,
    },
    Done(EditOutcome),
}

/// Final outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    Saved { slot: usize, chain: SavedChain },
    Replaced { slot: usize, chain: SavedChain },
    Deleted { slot: usize },
    Cancelled,
}

/// Per-frame input bundle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub cross: bool,
    pub circle: bool,
    pub triangle: bool,
    pub square: bool,
    /// True when the player typed a name char (engines feed canned
    /// suggestions in `EditEvent::NameAdvanced` chains).
    pub name_next: bool,
}

/// Events emitted per `tick`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditEvent {
    /// Cursor moved in the browse list.
    BrowseCursorMoved {
        row: u8,
    },
    /// Player started editing a chain (existing or new).
    EnteredEdit {
        editing_slot: Option<usize>,
    },
    /// Player pushed a direction onto the working chain.
    SequenceAppended {
        command: Command,
        len: usize,
    },
    /// Player popped the last direction.
    SequencePopped {
        len: usize,
    },
    /// Player tried to commit too short a chain - feedback for HUD blip.
    InvalidCommit {
        len: usize,
    },
    /// Player advanced past the editing step into naming.
    EnteredNaming,
    /// Player committed a save.
    Saved {
        slot: usize,
    },
    /// Player replaced an existing slot.
    Replaced {
        slot: usize,
    },
    /// Player deleted an existing slot from the browse phase.
    Deleted {
        slot: usize,
    },
    Cancelled,
}

/// Tactical Arts chain editor session.
#[derive(Debug, Clone)]
pub struct ChainEditor {
    char_slot: u8,
    library_view: Vec<SavedChain>,
    /// `Some(idx)` when we're replacing slot `idx`; `None` when we're
    /// authoring a brand-new chain.
    editing_slot: Option<usize>,
    phase: EditorPhase,
}

impl ChainEditor {
    /// Start an editor for `char_slot`, snapshotting the existing
    /// library. The library is updated only on commit by callers using
    /// [`ChainEditor::outcome`].
    pub fn new(char_slot: u8, library: &ChainLibrary) -> Self {
        Self {
            char_slot,
            library_view: library.saved(char_slot).to_vec(),
            editing_slot: None,
            phase: EditorPhase::Browsing { cursor: 0 },
        }
    }

    pub fn char_slot(&self) -> u8 {
        self.char_slot
    }

    pub fn phase(&self) -> &EditorPhase {
        &self.phase
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, EditorPhase::Done(_))
    }

    pub fn outcome(&self) -> Option<&EditOutcome> {
        match &self.phase {
            EditorPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    /// Total rows in the browse list - saved chains plus a "+ New"
    /// trailer when not at capacity.
    pub fn browse_rows(&self) -> u8 {
        let mut n = self.library_view.len();
        if n < ChainLibrary::MAX_SLOTS {
            n += 1; // "+ New" row
        }
        n as u8
    }

    pub fn library_view(&self) -> &[SavedChain] {
        &self.library_view
    }

    /// Apply [`Self::outcome`] to a mutable library. Idempotent on
    /// failed branches.
    pub fn apply_outcome(self, library: &mut ChainLibrary) -> Result<(), SaveError> {
        let char_slot = self.char_slot;
        match self.phase {
            EditorPhase::Done(EditOutcome::Saved { chain, .. }) => library.save(char_slot, chain),
            EditorPhase::Done(EditOutcome::Replaced { slot, chain }) => {
                library.replace(char_slot, slot, chain)
            }
            EditorPhase::Done(EditOutcome::Deleted { slot }) => {
                library.remove(char_slot, slot);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn tick(&mut self, input: EditInput) -> Vec<EditEvent> {
        let mut events = Vec::new();
        // Snapshot the phase by value to avoid mutable-borrow conflicts
        // when calling helper methods.
        let phase = std::mem::replace(&mut self.phase, EditorPhase::Done(EditOutcome::Cancelled));
        self.phase = phase;
        match self.phase.clone() {
            EditorPhase::Browsing { cursor } => self.tick_browsing(cursor, input, &mut events),
            EditorPhase::Editing { working } => self.tick_editing(working, input, &mut events),
            EditorPhase::Naming { working, name } => {
                self.tick_naming(working, name, input, &mut events)
            }
            EditorPhase::Done(_) => {}
        }
        events
    }

    fn tick_browsing(&mut self, cursor: u8, input: EditInput, events: &mut Vec<EditEvent>) {
        if input.circle {
            self.phase = EditorPhase::Done(EditOutcome::Cancelled);
            events.push(EditEvent::Cancelled);
            return;
        }
        let rows = self.browse_rows();
        if rows == 0 {
            return;
        }
        if input.up {
            let new = step_wrap(cursor, -1, rows);
            if new != cursor {
                self.phase = EditorPhase::Browsing { cursor: new };
                events.push(EditEvent::BrowseCursorMoved { row: new });
            }
            return;
        }
        if input.down {
            let new = step_wrap(cursor, 1, rows);
            if new != cursor {
                self.phase = EditorPhase::Browsing { cursor: new };
                events.push(EditEvent::BrowseCursorMoved { row: new });
            }
            return;
        }
        if input.triangle {
            if (cursor as usize) < self.library_view.len() {
                let slot = cursor as usize;
                self.library_view.remove(slot);
                self.phase = EditorPhase::Done(EditOutcome::Deleted { slot });
                events.push(EditEvent::Deleted { slot });
            }
            return;
        }
        if input.cross {
            let row = cursor as usize;
            if row < self.library_view.len() {
                self.editing_slot = Some(row);
                let working = self.library_view[row].sequence.clone();
                self.phase = EditorPhase::Editing { working };
                events.push(EditEvent::EnteredEdit {
                    editing_slot: Some(row),
                });
            } else if row == self.library_view.len()
                && self.library_view.len() < ChainLibrary::MAX_SLOTS
            {
                self.editing_slot = None;
                self.phase = EditorPhase::Editing {
                    working: Vec::new(),
                };
                events.push(EditEvent::EnteredEdit { editing_slot: None });
            }
        }
    }

    fn tick_editing(
        &mut self,
        mut working: Vec<Command>,
        input: EditInput,
        events: &mut Vec<EditEvent>,
    ) {
        if input.circle {
            self.phase = EditorPhase::Browsing { cursor: 0 };
            return;
        }
        if input.triangle {
            if working.pop().is_some() {
                events.push(EditEvent::SequencePopped { len: working.len() });
            }
            self.phase = EditorPhase::Editing { working };
            return;
        }
        let cmd = if input.left {
            Some(Command::Left)
        } else if input.right {
            Some(Command::Right)
        } else if input.up {
            Some(Command::Up)
        } else if input.down {
            Some(Command::Down)
        } else {
            None
        };
        if let Some(c) = cmd {
            if working.len() < ChainLibrary::MAX_LEN {
                working.push(c);
                events.push(EditEvent::SequenceAppended {
                    command: c,
                    len: working.len(),
                });
            }
            self.phase = EditorPhase::Editing { working };
            return;
        }
        if input.cross {
            if working.len() < ChainLibrary::MIN_LEN {
                events.push(EditEvent::InvalidCommit { len: working.len() });
                self.phase = EditorPhase::Editing { working };
                return;
            }
            let name = default_name(self.editing_slot);
            self.phase = EditorPhase::Naming { working, name };
            events.push(EditEvent::EnteredNaming);
            return;
        }
        // No relevant input - restore.
        self.phase = EditorPhase::Editing { working };
    }

    fn tick_naming(
        &mut self,
        working: Vec<Command>,
        mut name: String,
        input: EditInput,
        events: &mut Vec<EditEvent>,
    ) {
        if input.circle {
            self.phase = EditorPhase::Editing { working };
            return;
        }
        if input.name_next {
            name = next_name(&name);
            self.phase = EditorPhase::Naming { working, name };
            return;
        }
        if input.cross {
            let chain = SavedChain {
                name: name.clone(),
                sequence: working,
            };
            if let Some(slot) = self.editing_slot {
                self.library_view[slot] = chain.clone();
                self.phase = EditorPhase::Done(EditOutcome::Replaced { slot, chain });
                events.push(EditEvent::Replaced { slot });
            } else {
                let slot = self.library_view.len();
                self.library_view.push(chain.clone());
                self.phase = EditorPhase::Done(EditOutcome::Saved { slot, chain });
                events.push(EditEvent::Saved { slot });
            }
            return;
        }
        // No relevant input - restore.
        self.phase = EditorPhase::Naming { working, name };
    }
}

fn step_wrap(from: u8, dir: i8, rows: u8) -> u8 {
    let n = rows as i16;
    if n <= 0 {
        return from;
    }
    let mut cur = from as i16;
    cur = (cur + dir as i16).rem_euclid(n);
    cur as u8
}

fn default_name(slot: Option<usize>) -> String {
    match slot {
        Some(s) => format!("Chain {s}"),
        None => "New Chain".to_string(),
    }
}

const NAME_PRESETS: [&str; 8] = [
    "Combo A", "Combo B", "Combo C", "Combo D", "Striker", "Finisher", "Heavy", "Quick",
];

fn next_name(current: &str) -> String {
    if let Some(idx) = NAME_PRESETS.iter().position(|n| *n == current) {
        NAME_PRESETS[(idx + 1) % NAME_PRESETS.len()].to_string()
    } else {
        NAME_PRESETS[0].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> ChainLibrary {
        ChainLibrary::new()
    }

    #[test]
    fn library_starts_empty() {
        let lib = lib();
        assert_eq!(lib.total_count(), 0);
        assert!(lib.saved(0).is_empty());
    }

    #[test]
    fn save_chain_validates_length() {
        let mut lib = lib();
        let too_short = SavedChain::new("ab", vec![Command::Left, Command::Right]);
        assert_eq!(lib.save(0, too_short), Err(SaveError::InvalidLength(2)));
        let ok = SavedChain::new("ok", vec![Command::Left, Command::Right, Command::Up]);
        assert!(lib.save(0, ok).is_ok());
        // Too long
        let too_long = SavedChain::new(
            "long",
            vec![
                Command::Left,
                Command::Right,
                Command::Up,
                Command::Down,
                Command::Left,
                Command::Right,
                Command::Up,
                Command::Down,
            ],
        );
        assert_eq!(lib.save(0, too_long), Err(SaveError::InvalidLength(8)));
    }

    #[test]
    fn library_caps_at_max_slots() {
        let mut lib = lib();
        for i in 0..ChainLibrary::MAX_SLOTS {
            lib.save(0, SavedChain::new(format!("c{i}"), vec![Command::Left; 4]))
                .unwrap();
        }
        assert!(lib.is_full(0));
        let overflow = lib.save(0, SavedChain::new("over", vec![Command::Left; 4]));
        assert_eq!(overflow, Err(SaveError::LibraryFull));
    }

    #[test]
    fn replace_chain_in_place() {
        let mut lib = lib();
        lib.save(0, SavedChain::new("a", vec![Command::Left; 4]))
            .unwrap();
        lib.replace(0, 0, SavedChain::new("b", vec![Command::Right; 5]))
            .unwrap();
        assert_eq!(lib.saved(0)[0].name, "b");
    }

    #[test]
    fn remove_returns_entry() {
        let mut lib = lib();
        lib.save(0, SavedChain::new("a", vec![Command::Left; 4]))
            .unwrap();
        let removed = lib.remove(0, 0).unwrap();
        assert_eq!(removed.name, "a");
        assert!(lib.saved(0).is_empty());
    }

    #[test]
    fn pretty_sequence_roundtrip() {
        let c = SavedChain::new(
            "x",
            vec![Command::Left, Command::Right, Command::Up, Command::Down],
        );
        assert_eq!(c.pretty_sequence(), "L R U D");
    }

    #[test]
    fn editor_starts_in_browsing() {
        let lib = lib();
        let ed = ChainEditor::new(0, &lib);
        match ed.phase() {
            EditorPhase::Browsing { cursor: 0 } => {}
            _ => panic!(),
        }
        // browse_rows = 0 saved + 1 "new" row.
        assert_eq!(ed.browse_rows(), 1);
    }

    #[test]
    fn editor_cancel_emits_cancelled() {
        let lib = lib();
        let mut ed = ChainEditor::new(0, &lib);
        let events = ed.tick(EditInput {
            circle: true,
            ..Default::default()
        });
        assert!(events.contains(&EditEvent::Cancelled));
        assert!(matches!(ed.outcome(), Some(EditOutcome::Cancelled)));
    }

    #[test]
    fn editor_creates_new_chain_via_naming() {
        let lib = lib();
        let mut ed = ChainEditor::new(0, &lib);
        // Open new-chain editor.
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        match ed.phase() {
            EditorPhase::Editing { .. } => {}
            _ => panic!(),
        }
        // Push 4 directions.
        ed.tick(EditInput {
            left: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            right: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            up: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            down: true,
            ..Default::default()
        });
        // Confirm → naming.
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        match ed.phase() {
            EditorPhase::Naming { .. } => {}
            _ => panic!(),
        }
        // Confirm naming → done.
        let events = ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        assert!(events.iter().any(|e| matches!(e, EditEvent::Saved { .. })));
        match ed.outcome().unwrap() {
            EditOutcome::Saved { slot: 0, chain } => {
                assert_eq!(chain.len(), 4);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn editor_reject_too_short_commit() {
        let lib = lib();
        let mut ed = ChainEditor::new(0, &lib);
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            left: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            left: true,
            ..Default::default()
        });
        let events = ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EditEvent::InvalidCommit { len: 2 }))
        );
        match ed.phase() {
            EditorPhase::Editing { .. } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn editor_pop_with_triangle() {
        let lib = lib();
        let mut ed = ChainEditor::new(0, &lib);
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            left: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            right: true,
            ..Default::default()
        });
        let events = ed.tick(EditInput {
            triangle: true,
            ..Default::default()
        });
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EditEvent::SequencePopped { len: 1 }))
        );
    }

    #[test]
    fn editor_replace_existing() {
        let mut lib = lib();
        lib.save(0, SavedChain::new("a", vec![Command::Left; 4]))
            .unwrap();
        let mut ed = ChainEditor::new(0, &lib);
        // Cursor is at row 0 - Cross opens edit.
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        // Pop the seed sequence and push new one.
        for _ in 0..4 {
            ed.tick(EditInput {
                triangle: true,
                ..Default::default()
            });
        }
        for _ in 0..3 {
            ed.tick(EditInput {
                up: true,
                ..Default::default()
            });
        }
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        match ed.outcome().unwrap() {
            EditOutcome::Replaced { slot: 0, chain } => {
                assert_eq!(chain.len(), 3);
                assert!(chain.sequence.iter().all(|c| matches!(c, Command::Up)));
            }
            other => panic!("expected Replaced, got {other:?}"),
        }
    }

    #[test]
    fn editor_delete_existing_via_triangle_in_browse() {
        let mut lib = lib();
        lib.save(0, SavedChain::new("a", vec![Command::Left; 4]))
            .unwrap();
        lib.save(0, SavedChain::new("b", vec![Command::Right; 4]))
            .unwrap();
        let mut ed = ChainEditor::new(0, &lib);
        // Move cursor to slot 1.
        ed.tick(EditInput {
            down: true,
            ..Default::default()
        });
        let events = ed.tick(EditInput {
            triangle: true,
            ..Default::default()
        });
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EditEvent::Deleted { slot: 1 }))
        );
        match ed.outcome().unwrap() {
            EditOutcome::Deleted { slot: 1 } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn apply_outcome_updates_library() {
        let mut lib = lib();
        lib.save(0, SavedChain::new("seed", vec![Command::Left; 4]))
            .unwrap();
        let mut ed = ChainEditor::new(0, &lib);
        ed.tick(EditInput {
            down: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        for _ in 0..3 {
            ed.tick(EditInput {
                up: true,
                ..Default::default()
            });
        }
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        ed.apply_outcome(&mut lib).unwrap();
        assert_eq!(lib.saved(0).len(), 2);
    }

    #[test]
    fn next_name_cycles_presets() {
        let n0 = next_name("");
        assert_eq!(n0, NAME_PRESETS[0]);
        let n1 = next_name(NAME_PRESETS[0]);
        assert_eq!(n1, NAME_PRESETS[1]);
        let last = next_name(NAME_PRESETS[7]);
        assert_eq!(last, NAME_PRESETS[0]);
    }

    #[test]
    fn editor_max_length_enforced() {
        let lib = lib();
        let mut ed = ChainEditor::new(0, &lib);
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        for _ in 0..(ChainLibrary::MAX_LEN + 3) {
            ed.tick(EditInput {
                left: true,
                ..Default::default()
            });
        }
        match ed.phase() {
            EditorPhase::Editing { working } => assert_eq!(working.len(), ChainLibrary::MAX_LEN),
            _ => panic!(),
        }
    }
}

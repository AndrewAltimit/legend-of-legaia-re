//! Per-character Tactical Arts tracker - the host side of retail's
//! **learn-on-use** check.
//!
//! The decision itself is retail's, not the engine's: every accepted art runs
//! through [`legaia_engine_vm::battle_action::check_and_learn_art`]
//! (`FUN_801EFBFC`), which does a membership scan over the character record's
//! learned-art list (count `+0x74D`, ascending ids `+0x74E..`) and, on a miss,
//! inserts the id in sorted order. This tracker holds that list per character
//! and emits a [`TacticalArtLearned`] event on the frame the insert happens.
//!
//! Two retail inputs the engine supplies rather than reads:
//!
//! * the **learn gate** `ctx[+0x266 + slot]`. Retail opens the leg when that
//!   per-slot context byte is clear, or on a 1/512 `rand()` draw, or under the
//!   `'O'` debug byte. The engine carries no `+0x266` marker, so the byte is
//!   permanently clear and the gate is permanently open - which is retail's
//!   own behaviour for a clear marker, and the reason an art is learned the
//!   first time it is successfully performed.
//! * the **innate cap** `0x801F686C + char_id - 1`: art ids at or below it are
//!   innate and never enter the list. That table is battle-overlay disc data
//!   (PROT 0898) with no parser yet, so it defaults to `0` and hosts that have
//!   it call [`TacticalArtsTracker::set_innate_cap`].
//!
//! Art names come from the game's MES dialog containers - the tracker stores
//! them as a caller-supplied `HashMap<u8, String>`. Without disc data the
//! fallback is `"Art #N"`.

use legaia_engine_vm::battle_action::{ArtUseCheck, check_and_learn_art};
use std::collections::HashMap;

/// Capacity of the retail learned-art id list (`record[+0x74E..]`, the bound
/// [`check_and_learn_art`] refuses to insert past).
pub const LEARNED_ART_SLOTS: usize = 16;

/// A "Tactical Art learned" notification produced by
/// [`TacticalArtsTracker::notify_art_used`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TacticalArtLearned {
    /// Roster index of the character who learned the art.
    pub char_id: u8,
    /// Move-table art index.
    pub art_id: u8,
    /// Display name. Overridden from disc MES data when loaded; falls back
    /// to `"Art #N"` when the name table has no entry for this id.
    pub name: String,
}

/// HUD banner shown after an art is learned.
///
/// Engines draw this via the dialog font overlay. `frames_remaining` counts
/// down each [`crate::world::World::tick`]; when it reaches zero the banner
/// is cleared by the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtLearnedBanner {
    /// One-line text drawn by the engine.
    pub text: String,
    /// Remaining display frames. Decremented by the world tick.
    pub frames_remaining: u16,
}

impl ArtLearnedBanner {
    /// Default display duration: 120 frames (2 s at 60 Hz).
    pub const DEFAULT_FRAMES: u16 = 120;
}

/// One character's retail learned-art list: the `+0x74D` count and the
/// ascending `+0x74E..` id array [`check_and_learn_art`] scans and inserts
/// into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LearnedArtList {
    count: u8,
    ids: [u8; LEARNED_ART_SLOTS],
}

impl Default for LearnedArtList {
    fn default() -> Self {
        Self {
            count: 0,
            ids: [0; LEARNED_ART_SLOTS],
        }
    }
}

/// Holds each character's learned-art list and runs retail's learn-on-use
/// check over it, emitting [`TacticalArtLearned`] on the insert.
///
/// Engines call [`notify_art_used`] from whatever path executes an art
/// (the battle side-effects handler, once a Tactical Arts strike lands).
/// The world's [`notify_art_used`] wrapper pushes the event onto the
/// pending battle events queue and sets the HUD banner.
///
/// [`notify_art_used`]: TacticalArtsTracker::notify_art_used
/// [`notify_art_used`]: crate::world::World::notify_art_used
#[derive(Debug, Clone, Default)]
pub struct TacticalArtsTracker {
    lists: HashMap<u8, LearnedArtList>,
    innate_cap: HashMap<u8, u8>,
    name_table: HashMap<u8, String>,
}

impl TacticalArtsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the per-character innate-art cap (`0x801F686C + char_id - 1`).
    /// Ids at or below `cap` are innate: [`check_and_learn_art`] refuses to
    /// insert them, so they never fire a learn event. Defaults to `0`, which
    /// makes every non-zero id learnable.
    pub fn set_innate_cap(&mut self, char_id: u8, cap: u8) {
        self.innate_cap.insert(char_id, cap);
    }

    /// Supply art display names from disc MES data.
    /// Keys are art IDs; values are display strings. Overrides the default
    /// `"Art #N"` fallback for any id present in the table.
    pub fn set_art_name_table(&mut self, table: HashMap<u8, String>) {
        self.name_table = table;
    }

    /// Mark `art_id` as already known for `char_id` (e.g. from a loaded
    /// save record) so the tracker does not re-fire a learn event for arts
    /// the character already has.
    ///
    /// The insert goes through the same retail kernel the live path uses, so
    /// a restored list comes back ascending-sorted exactly as retail keeps it.
    pub fn mark_known(&mut self, char_id: u8, art_id: u8) {
        let list = self.lists.entry(char_id).or_default();
        // `innate_cap = 0` + open gate: the kernel's only refusals here are
        // "already present" and "list full", both of which are correct for a
        // restore.
        let _ = check_and_learn_art(&mut list.count, &mut list.ids, art_id, true, 0);
    }

    /// Record one use of `art_id` by `char_id`.
    ///
    /// Runs `FUN_801EFBFC` over the character's learned-art list and returns
    /// `Some(TacticalArtLearned)` on the frame the id is inserted
    /// ([`ArtUseCheck::Learned`]); `None` when the art is already known, when
    /// it is at or below the character's innate cap, or when the list is full.
    pub fn notify_art_used(&mut self, char_id: u8, art_id: u8) -> Option<TacticalArtLearned> {
        let cap = self.innate_cap.get(&char_id).copied().unwrap_or(0);
        let list = self.lists.entry(char_id).or_default();
        // The learn gate: retail's `ctx[+0x266 + slot]` marker has no engine
        // analogue, so it reads as permanently clear (gate open) - see the
        // module note.
        let verdict = check_and_learn_art(&mut list.count, &mut list.ids, art_id, true, cap);
        if verdict != ArtUseCheck::Learned {
            return None;
        }
        let name = self
            .name_table
            .get(&art_id)
            .cloned()
            .unwrap_or_else(|| format!("Art #{art_id}"));
        Some(TacticalArtLearned {
            char_id,
            art_id,
            name,
        })
    }

    /// The character's learned-art ids, ascending - retail's `+0x74E..` list
    /// truncated to its `+0x74D` count.
    pub fn learned_ids(&self, char_id: u8) -> Vec<u8> {
        self.lists
            .get(&char_id)
            .map(|l| l.ids[..(l.count as usize).min(LEARNED_ART_SLOTS)].to_vec())
            .unwrap_or_default()
    }

    /// Returns `true` if `char_id` has already learned `art_id`.
    pub fn is_learned(&self, char_id: u8, art_id: u8) -> bool {
        self.lists
            .get(&char_id)
            .is_some_and(|l| l.ids[..(l.count as usize).min(LEARNED_ART_SLOTS)].contains(&art_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_art_is_learned_the_first_time_it_is_performed() {
        // The retail contract: `FUN_801EFBFC` inserts on the first miss, so
        // there is no use count to accumulate.
        let mut t = TacticalArtsTracker::new();
        let ev = t.notify_art_used(0, 1).expect("learned on first use");
        assert_eq!(ev.char_id, 0);
        assert_eq!(ev.art_id, 1);
    }

    #[test]
    fn art_not_re_fired_after_learn() {
        let mut t = TacticalArtsTracker::new();
        assert!(t.notify_art_used(0, 5).is_some());
        assert!(
            t.notify_art_used(0, 5).is_none(),
            "should be None after learn"
        );
    }

    #[test]
    fn mark_known_suppresses_learn() {
        let mut t = TacticalArtsTracker::new();
        t.mark_known(0, 7);
        assert!(t.notify_art_used(0, 7).is_none());
    }

    #[test]
    fn ids_at_or_below_the_innate_cap_never_fire() {
        // Retail inserts only when `art_id > innate_cap` - the character's
        // innate arts are already in the list from the record, so re-using
        // them is a plain membership hit.
        let mut t = TacticalArtsTracker::new();
        t.set_innate_cap(0, 0x20);
        for id in [0x01u8, 0x1F, 0x20] {
            assert!(t.notify_art_used(0, id).is_none(), "id {id:#x}");
        }
        assert!(t.notify_art_used(0, 0x21).is_some(), "one past the cap");
    }

    #[test]
    fn the_learned_list_stays_ascending() {
        let mut t = TacticalArtsTracker::new();
        for id in [0x30u8, 0x22, 0x2B, 0x24] {
            t.notify_art_used(1, id);
        }
        assert_eq!(t.learned_ids(1), vec![0x22, 0x24, 0x2B, 0x30]);
    }

    #[test]
    fn different_chars_tracked_independently() {
        let mut t = TacticalArtsTracker::new();
        assert!(t.notify_art_used(0, 4).is_some());
        assert!(t.notify_art_used(1, 4).is_some(), "char 1 has its own list");
        assert!(t.is_learned(0, 4) && t.is_learned(1, 4));
        assert!(!t.is_learned(2, 4));
    }

    #[test]
    fn custom_name_table() {
        let mut t = TacticalArtsTracker::new();
        t.set_art_name_table([(3u8, "Power Punch".to_string())].into());
        let ev = t.notify_art_used(0, 3).unwrap();
        assert_eq!(ev.name, "Power Punch");
    }

    #[test]
    fn fallback_name_when_missing_from_table() {
        let mut t = TacticalArtsTracker::new();
        let ev = t.notify_art_used(0, 42).unwrap();
        assert_eq!(ev.name, "Art #42");
    }

    #[test]
    fn a_full_list_refuses_further_inserts() {
        // Retail would spill past `+0x75E`; the port refuses instead, and the
        // tracker must not fire a learn event for the refusal.
        let mut t = TacticalArtsTracker::new();
        for i in 0..LEARNED_ART_SLOTS as u8 {
            assert!(t.notify_art_used(0, 0x20 + i).is_some());
        }
        assert!(t.notify_art_used(0, 0x60).is_none());
        assert_eq!(t.learned_ids(0).len(), LEARNED_ART_SLOTS);
    }

    #[test]
    fn banner_default_frames() {
        assert_eq!(ArtLearnedBanner::DEFAULT_FRAMES, 120);
    }
}

//! Per-character Tactical Arts tracker.
//!
//! Monitors per-art usage counts; emits a `TacticalArtLearned` event the
//! first time a character's use count for an art crosses the configured
//! threshold. The threshold approximates the retail per-art learning
//! condition; once the real formula is traced from the level-up / battle overlay
//! it can be replaced.
//!
//! Art names come from the game's MES dialog containers - the tracker stores
//! them as a caller-supplied `HashMap<u8, String>`. Without disc data the
//! fallback is `"Art #N"`.

use std::collections::{HashMap, HashSet};

/// Number of uses before an art is considered "learned". The retail formula
/// is per-art and tracks counters stored in the save record; this constant
/// is a clean-room approximation pending the save-screen overlay trace.
pub const DEFAULT_LEARN_THRESHOLD: u32 = 10;

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

/// Tracks per-character per-art use counts and emits [`TacticalArtLearned`]
/// events the first time a threshold is crossed.
///
/// Engines call [`notify_art_used`] from whatever path updates art usage
/// (typically the battle side-effects handler after a Tactical Arts strike
/// lands). The world's [`notify_art_used`] wrapper pushes the event onto the
/// pending battle events queue and sets the HUD banner.
///
/// [`notify_art_used`]: TacticalArtsTracker::notify_art_used
/// [`notify_art_used`]: crate::world::World::notify_art_used
#[derive(Debug, Clone, Default)]
pub struct TacticalArtsTracker {
    counters: HashMap<u8, HashMap<u8, u32>>,
    learned: HashMap<u8, HashSet<u8>>,
    threshold: u32,
    name_table: HashMap<u8, String>,
}

impl TacticalArtsTracker {
    pub fn new() -> Self {
        Self {
            threshold: DEFAULT_LEARN_THRESHOLD,
            ..Default::default()
        }
    }

    /// Override the use-count threshold (default: `DEFAULT_LEARN_THRESHOLD`).
    pub fn set_threshold(&mut self, threshold: u32) {
        self.threshold = threshold;
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
    pub fn mark_known(&mut self, char_id: u8, art_id: u8) {
        self.learned.entry(char_id).or_default().insert(art_id);
    }

    /// Record one use of `art_id` by `char_id`.
    ///
    /// Returns `Some(TacticalArtLearned)` the first time the use count
    /// crosses [`Self::threshold`]; `None` on every subsequent call (already
    /// learned) or when the threshold has not yet been reached.
    pub fn notify_art_used(&mut self, char_id: u8, art_id: u8) -> Option<TacticalArtLearned> {
        if self
            .learned
            .get(&char_id)
            .is_some_and(|s| s.contains(&art_id))
        {
            return None;
        }

        let count = self
            .counters
            .entry(char_id)
            .or_default()
            .entry(art_id)
            .or_insert(0);
        *count += 1;

        if *count >= self.threshold {
            self.learned.entry(char_id).or_default().insert(art_id);
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
        } else {
            None
        }
    }

    /// Current use count for `(char_id, art_id)`. Returns `0` when no uses
    /// have been recorded yet.
    pub fn use_count(&self, char_id: u8, art_id: u8) -> u32 {
        self.counters
            .get(&char_id)
            .and_then(|m| m.get(&art_id))
            .copied()
            .unwrap_or(0)
    }

    /// Returns `true` if `char_id` has already learned `art_id`.
    pub fn is_learned(&self, char_id: u8, art_id: u8) -> bool {
        self.learned
            .get(&char_id)
            .is_some_and(|s| s.contains(&art_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_learned_after_threshold() {
        let mut t = TacticalArtsTracker::new();
        t.set_threshold(3);
        assert!(t.notify_art_used(0, 1).is_none());
        assert!(t.notify_art_used(0, 1).is_none());
        let ev = t.notify_art_used(0, 1).expect("should learn on 3rd use");
        assert_eq!(ev.char_id, 0);
        assert_eq!(ev.art_id, 1);
    }

    #[test]
    fn art_not_re_fired_after_learn() {
        let mut t = TacticalArtsTracker::new();
        t.set_threshold(1);
        assert!(t.notify_art_used(0, 5).is_some());
        assert!(
            t.notify_art_used(0, 5).is_none(),
            "should be None after learn"
        );
    }

    #[test]
    fn mark_known_suppresses_learn() {
        let mut t = TacticalArtsTracker::new();
        t.set_threshold(1);
        t.mark_known(0, 7);
        assert!(t.notify_art_used(0, 7).is_none());
    }

    #[test]
    fn different_chars_tracked_independently() {
        let mut t = TacticalArtsTracker::new();
        t.set_threshold(2);
        t.notify_art_used(0, 0);
        assert!(t.notify_art_used(0, 0).is_some());
        assert!(
            t.notify_art_used(1, 0).is_none(),
            "char 1 not at threshold yet"
        );
    }

    #[test]
    fn custom_name_table() {
        let mut t = TacticalArtsTracker::new();
        t.set_threshold(1);
        t.set_art_name_table([(3u8, "Power Punch".to_string())].into());
        let ev = t.notify_art_used(0, 3).unwrap();
        assert_eq!(ev.name, "Power Punch");
    }

    #[test]
    fn fallback_name_when_missing_from_table() {
        let mut t = TacticalArtsTracker::new();
        t.set_threshold(1);
        let ev = t.notify_art_used(0, 42).unwrap();
        assert_eq!(ev.name, "Art #42");
    }

    #[test]
    fn use_count_and_is_learned_accessors() {
        let mut t = TacticalArtsTracker::new();
        t.set_threshold(3);
        assert_eq!(t.use_count(0, 2), 0);
        assert!(!t.is_learned(0, 2));
        t.notify_art_used(0, 2);
        t.notify_art_used(0, 2);
        assert_eq!(t.use_count(0, 2), 2);
        assert!(!t.is_learned(0, 2));
        t.notify_art_used(0, 2);
        assert_eq!(t.use_count(0, 2), 3);
        assert!(t.is_learned(0, 2));
    }

    #[test]
    fn banner_default_frames() {
        assert_eq!(ArtLearnedBanner::DEFAULT_FRAMES, 120);
    }
}

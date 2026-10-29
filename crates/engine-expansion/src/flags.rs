//! The pack's own flags and counters.
//!
//! A pack never writes a retail story flag: its quests, dialogue choices and
//! rewards raise named flags and bump named counters in this store, which is
//! scoped to the pack (the save records it under the pack id). A quest can
//! still *read* a retail system flag as a prerequisite; that read goes
//! through [`crate::quest::QuestWorld::story_flag`], never through here.

use std::collections::{BTreeMap, BTreeSet};

/// Named flags and signed counters, keyed by pack-chosen ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackFlags {
    /// Raised flags; an id absent here is clear.
    pub flags: BTreeSet<String>,
    /// Counters; an id absent here reads `0`.
    pub counters: BTreeMap<String, i64>,
}

impl PackFlags {
    /// Whether `flag` is raised.
    pub fn is_set(&self, flag: &str) -> bool {
        self.flags.contains(flag)
    }

    /// Raise `flag`. Returns `true` when it was clear.
    pub fn set(&mut self, flag: &str) -> bool {
        self.flags.insert(flag.to_string())
    }

    /// Clear `flag`. Returns `true` when it was raised.
    pub fn clear(&mut self, flag: &str) -> bool {
        self.flags.remove(flag)
    }

    /// The value of `counter`.
    pub fn counter(&self, counter: &str) -> i64 {
        self.counters.get(counter).copied().unwrap_or(0)
    }

    /// Add `add` to `counter` (saturating) and return the new value. A
    /// counter that returns to `0` is dropped, so the store holds only the
    /// counters that differ from their default.
    pub fn add(&mut self, counter: &str, add: i64) -> i64 {
        let v = self.counter(counter).saturating_add(add);
        if v == 0 {
            self.counters.remove(counter);
        } else {
            self.counters.insert(counter.to_string(), v);
        }
        v
    }

    /// Whether nothing is set.
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty() && self.counters.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_set_and_clear() {
        let mut f = PackFlags::default();
        assert!(!f.is_set("a"));
        assert!(f.set("a"));
        assert!(!f.set("a"), "a second raise reports no change");
        assert!(f.is_set("a"));
        assert!(f.clear("a"));
        assert!(!f.clear("a"));
        assert!(f.is_empty());
    }

    #[test]
    fn counters_add_and_drop_at_zero() {
        let mut f = PackFlags::default();
        assert_eq!(f.counter("k"), 0);
        assert_eq!(f.add("k", 3), 3);
        assert_eq!(f.add("k", -1), 2);
        assert_eq!(f.add("k", -2), 0);
        assert!(f.is_empty(), "a counter back at zero leaves the store");
        assert_eq!(f.add("k", i64::MAX), i64::MAX);
        assert_eq!(f.add("k", 1), i64::MAX, "saturates");
    }
}

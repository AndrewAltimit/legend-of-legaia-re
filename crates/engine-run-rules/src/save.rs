//! The run's save record: its rules, a Boss Rush in progress, the Boss Rush
//! leaderboard and the ironman run id, as one tagged block of an engine save.
//!
//! The body is versioned JSON, so a later rule reads back from an older save
//! at its default. A run with retail rules, no Boss Rush and an empty record
//! writes no block at all ([`RunSave::encode`] returns `None`), so its saves
//! are byte-identical to a build without run rules.

use serde::{Deserialize, Serialize};

use crate::boss_rush::{BossRushRecord, BossRushRun};
use crate::rules::RunRules;

/// The block's body version.
pub const RUN_SAVE_VERSION: u32 = 1;

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// What a save keeps of the run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSave {
    #[serde(default)]
    pub v: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub rules: RunRules,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boss_rush: Option<BossRushRun>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub record: BossRushRecord,
    /// The ironman run's id: the one save the run may keep is the one
    /// carrying it.
    #[serde(default, skip_serializing_if = "is_default")]
    pub run_id: u64,
}

impl RunSave {
    /// Whether there is nothing to keep.
    pub fn is_empty(&self) -> bool {
        self.rules.is_retail()
            && self.boss_rush.is_none()
            && self.record == BossRushRecord::default()
            && self.run_id == 0
    }

    /// The block body, `None` when there is nothing to keep.
    pub fn encode(&self) -> Option<Vec<u8>> {
        if self.is_empty() {
            return None;
        }
        let mut s = self.clone();
        s.v = RUN_SAVE_VERSION;
        serde_json::to_vec(&s).ok()
    }

    /// Read a block body back; an unreadable or newer body reads as nothing
    /// (retail rules), never as an error a load could fail on.
    pub fn decode(body: &[u8]) -> Option<Self> {
        let s: RunSave = serde_json::from_slice(body).ok()?;
        (s.v <= RUN_SAVE_VERSION).then_some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Difficulty, Ironman};

    #[test]
    fn retail_writes_nothing_and_rules_round_trip() {
        assert_eq!(RunSave::default().encode(), None);
        let s = RunSave {
            rules: RunRules {
                difficulty: Difficulty::Nightmare,
                ironman: Some(Ironman { hardcore: false }),
                ..Default::default()
            },
            run_id: 77,
            boss_rush: Some(BossRushRun::new(&[])),
            ..Default::default()
        };
        let body = s.encode().unwrap();
        let back = RunSave::decode(&body).unwrap();
        assert_eq!(back.rules, s.rules);
        assert_eq!(back.run_id, 77);
        assert_eq!(back.boss_rush, s.boss_rush);
        assert_eq!(RunSave::decode(b"{\"v\":99}"), None);
        assert_eq!(RunSave::decode(b"junk"), None);
        // A record alone keeps a block.
        let r = RunSave {
            record: BossRushRecord {
                runs: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(r.encode().is_some());
    }
}

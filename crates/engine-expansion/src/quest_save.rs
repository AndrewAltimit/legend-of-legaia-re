//! The expansion block a save carries: one pack's quest progress, pack flags,
//! counters and challenge records, as a self-describing byte string.
//!
//! The save layer stores the block opaquely (`legaia_save::SaveExtV2::expansion`,
//! the `LGXD` block of an `LGSF` file); this module owns its layout. A save
//! made with no pack carries no block at all, so its bytes do not change.
//!
//! ```text
//! 4      magic b"LXP1"
//! str    pack id
//! str    pack version (the version the progress was last written under)
//! u16    quest count, then per quest:
//!          str id, u8 status (0 active, 1 completed), u32 completions,
//!          u32 last_completed_at, u16 objective count, u32 progress each
//! u16    flag count, then str each
//! u16    counter count, then str id + i64 value each
//! u16    challenge count, then str id + u32 best + u32 clears each
//! ```
//!
//! `str` is a `u16` byte length then UTF-8. Integers are little-endian.
//! Maps are written in key order, so equal progress encodes to equal bytes.

use std::collections::BTreeMap;

use legaia_expansion_format::ExpansionPack;

use crate::flags::PackFlags;
use crate::quest::{QuestState, QuestStatus};
use crate::quest_challenge::ChallengeRecord;

/// Magic at the start of every expansion block.
pub const EXPANSION_BLOCK_MAGIC: [u8; 4] = *b"LXP1";

/// One pack's saved progress.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExpansionProgress {
    /// The pack the progress belongs to (`PackManifest::id`).
    pub pack_id: String,
    /// The pack version it was last written under.
    pub pack_version: String,
    /// Every quest that has started.
    pub quests: BTreeMap<String, QuestState>,
    /// Pack flags and counters.
    pub flags: PackFlags,
    /// Challenge records.
    pub challenges: BTreeMap<String, ChallengeRecord>,
}

impl ExpansionProgress {
    /// Fresh progress for `pack`.
    pub fn new(pack: &ExpansionPack) -> Self {
        Self {
            pack_id: pack.manifest.id.clone(),
            pack_version: pack.manifest.version.clone(),
            ..Self::default()
        }
    }

    /// The block bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Vec::new();
        w.extend_from_slice(&EXPANSION_BLOCK_MAGIC);
        put_str(&mut w, &self.pack_id);
        put_str(&mut w, &self.pack_version);
        put_len(&mut w, self.quests.len());
        for (id, q) in self.quests.iter().take(MAX_LEN) {
            put_str(&mut w, id);
            w.push(match q.status {
                QuestStatus::Active => 0,
                QuestStatus::Completed => 1,
            });
            w.extend_from_slice(&q.completions.to_le_bytes());
            w.extend_from_slice(&q.last_completed_at.to_le_bytes());
            put_len(&mut w, q.objectives.len());
            for p in q.objectives.iter().take(MAX_LEN) {
                w.extend_from_slice(&p.to_le_bytes());
            }
        }
        put_len(&mut w, self.flags.flags.len());
        for f in self.flags.flags.iter().take(MAX_LEN) {
            put_str(&mut w, f);
        }
        put_len(&mut w, self.flags.counters.len());
        for (k, v) in self.flags.counters.iter().take(MAX_LEN) {
            put_str(&mut w, k);
            w.extend_from_slice(&v.to_le_bytes());
        }
        put_len(&mut w, self.challenges.len());
        for (id, c) in self.challenges.iter().take(MAX_LEN) {
            put_str(&mut w, id);
            w.extend_from_slice(&c.best.to_le_bytes());
            w.extend_from_slice(&c.clears.to_le_bytes());
        }
        w
    }

    /// Decode a block; `None` for anything that is not a whole, well-formed
    /// block.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader { b: bytes, p: 0 };
        if r.take(4)? != EXPANSION_BLOCK_MAGIC {
            return None;
        }
        let mut out = Self {
            pack_id: r.str()?,
            pack_version: r.str()?,
            ..Self::default()
        };
        for _ in 0..r.u16()? {
            let id = r.str()?;
            let status = match r.u8()? {
                0 => QuestStatus::Active,
                1 => QuestStatus::Completed,
                _ => return None,
            };
            let completions = r.u32()?;
            let last_completed_at = r.u32()?;
            let n = r.u16()?;
            let objectives = (0..n).map(|_| r.u32()).collect::<Option<Vec<_>>>()?;
            out.quests.insert(
                id,
                QuestState {
                    status,
                    objectives,
                    completions,
                    last_completed_at,
                },
            );
        }
        for _ in 0..r.u16()? {
            out.flags.flags.insert(r.str()?);
        }
        for _ in 0..r.u16()? {
            let k = r.str()?;
            let v = i64::from_le_bytes(r.take(8)?.try_into().ok()?);
            out.flags.counters.insert(k, v);
        }
        for _ in 0..r.u16()? {
            let id = r.str()?;
            let best = r.u32()?;
            let clears = r.u32()?;
            out.challenges.insert(id, ChallengeRecord { best, clears });
        }
        (r.p == bytes.len()).then_some(out)
    }
}

/// Longest list or string the block stores; longer ones are cut, the
/// count and the items alike.
const MAX_LEN: usize = u16::MAX as usize;

fn put_len(w: &mut Vec<u8>, n: usize) {
    w.extend_from_slice(&(n.min(MAX_LEN) as u16).to_le_bytes());
}

fn put_str(w: &mut Vec<u8>, s: &str) {
    // Cut on a char boundary so the stored bytes stay UTF-8.
    let mut end = s.len().min(MAX_LEN);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    put_len(w, end);
    w.extend_from_slice(&s.as_bytes()[..end]);
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn str(&mut self) -> Option<String> {
        let n = usize::from(self.u16()?);
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ExpansionProgress {
        let mut p = ExpansionProgress {
            pack_id: "rebirth".into(),
            pack_version: "0.1.0".into(),
            ..Default::default()
        };
        p.quests.insert(
            "a".into(),
            QuestState {
                status: QuestStatus::Active,
                objectives: vec![1, 0, 3],
                completions: 0,
                last_completed_at: 0,
            },
        );
        p.quests.insert(
            "b".into(),
            QuestState {
                status: QuestStatus::Completed,
                objectives: vec![],
                completions: 2,
                last_completed_at: 3600,
            },
        );
        p.flags.set("door");
        p.flags.add("kills", -4);
        p.challenges.insert(
            "c".into(),
            ChallengeRecord {
                best: 1300,
                clears: 2,
            },
        );
        p
    }

    #[test]
    fn progress_round_trips() {
        let p = sample();
        let bytes = p.encode();
        assert_eq!(&bytes[..4], b"LXP1");
        assert_eq!(ExpansionProgress::decode(&bytes), Some(p));
    }

    #[test]
    fn equal_progress_encodes_equal_bytes() {
        assert_eq!(sample().encode(), sample().encode());
    }

    #[test]
    fn a_damaged_block_does_not_decode() {
        let bytes = sample().encode();
        assert!(ExpansionProgress::decode(&bytes[..bytes.len() - 1]).is_none());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(ExpansionProgress::decode(&extra).is_none());
        let mut bad = bytes;
        bad[0] = b'X';
        assert!(ExpansionProgress::decode(&bad).is_none());
        assert!(ExpansionProgress::decode(&[]).is_none());
    }
}

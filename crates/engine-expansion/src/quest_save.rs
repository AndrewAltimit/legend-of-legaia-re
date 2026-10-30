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
//! [tail]  zero or more tagged optional records, each `u8 tag` + body:
//!          1 = cleared bosses: u16 count, then str each (only when one is)
//!          2 = map: str map id, i16 x, i16 z, u16 facing, only on a save
//!              made on a pack map (`crate::map::MapLocation`)
//!          3 = cards (only once the player has any card state):
//!              u8 flags (bit 0: starter cards handed over), u64 card-stream
//!              state, u16 count then str id + u32 copies each (binder
//!              order), u16 count then str booster id + u32 unopened each,
//!              u16 deck count then str each, u16 seen count then str each
//!          4 = pack items (only once the player holds or wears any):
//!              u16 count then str key + u8 count each (the bag), u16 count
//!              then u8 roster slot + u8 equipment byte + str key each (the
//!              worn gear)
//!          5 = pack arts and spells (only once any is known): u16 count
//!              then str art key each, u16 count then u8 roster slot + str
//!              spell key + u8 magic level + u32 spell XP each
//! ```
//!
//! `str` is a `u16` byte length then UTF-8. Integers are little-endian.
//! Maps are written in key order, so equal progress encodes to equal bytes.

use std::collections::{BTreeMap, BTreeSet};

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
    /// Once-only `battle.toml` bosses the party has beaten, by local key
    /// (the battle runtime's `cleared_bosses`, mirrored here for the save).
    pub cleared_bosses: BTreeSet<String>,
    /// The pack map the save was made on. Written only when set, so a save
    /// made off every map keeps the block it had.
    pub map: Option<crate::map::MapLocation>,
    /// The trading-card collection, boosters and duel deck. Written only
    /// when it holds anything.
    pub cards: legaia_engine_modes::cards::economy::CardProgress,
    /// The pack items held and worn. Written only when it holds anything.
    pub items: crate::items::ItemProgress,
    /// The pack arts known. Written only when it holds anything.
    pub arts: crate::arts::ArtsProgress,
    /// The pack spells known and their levels. Written only when it holds
    /// anything.
    pub spells: crate::spells::SpellProgress,
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
        // The optional tail records: each is written only when it carries
        // something, so a block without them is the block an older writer
        // made.
        if !self.cleared_bosses.is_empty() {
            w.push(TAIL_CLEARED_BOSSES);
            put_len(&mut w, self.cleared_bosses.len());
            for b in self.cleared_bosses.iter().take(MAX_LEN) {
                put_str(&mut w, b);
            }
        }
        if let Some(m) = &self.map {
            w.push(TAIL_MAP);
            put_str(&mut w, &m.map_id);
            w.extend_from_slice(&m.x.to_le_bytes());
            w.extend_from_slice(&m.z.to_le_bytes());
            w.extend_from_slice(&m.facing.to_le_bytes());
        }
        let c = &self.cards;
        if *c != Default::default() {
            w.push(TAIL_CARDS);
            w.push(u8::from(c.starter_granted));
            w.extend_from_slice(&c.rng.to_le_bytes());
            let binder = c.collection.binder();
            put_len(&mut w, binder.len());
            for id in binder.iter().take(MAX_LEN) {
                put_str(&mut w, id);
                w.extend_from_slice(&c.collection.count(id).to_le_bytes());
            }
            put_len(&mut w, c.boosters.len());
            for (id, n) in c.boosters.iter().take(MAX_LEN) {
                put_str(&mut w, id);
                w.extend_from_slice(&n.to_le_bytes());
            }
            put_len(&mut w, c.deck.len());
            for id in c.deck.iter().take(MAX_LEN) {
                put_str(&mut w, id);
            }
            put_len(&mut w, c.seen.len());
            for id in c.seen.iter().take(MAX_LEN) {
                put_str(&mut w, id);
            }
        }
        let it = &self.items;
        if !it.is_empty() {
            w.push(TAIL_ITEMS);
            put_len(&mut w, it.bag.len());
            for (k, n) in it.bag.iter().take(MAX_LEN) {
                put_str(&mut w, k);
                w.push(*n);
            }
            put_len(&mut w, it.worn.len());
            for (&(r, i), k) in it.worn.iter().take(MAX_LEN) {
                w.push(r);
                w.push(i);
                put_str(&mut w, k);
            }
        }
        if !self.arts.learned.is_empty() || !self.spells.known.is_empty() {
            w.push(TAIL_ARTS_SPELLS);
            put_len(&mut w, self.arts.learned.len());
            for k in self.arts.learned.iter().take(MAX_LEN) {
                put_str(&mut w, k);
            }
            put_len(&mut w, self.spells.known.len());
            for ((roster, k), lv) in self.spells.known.iter().take(MAX_LEN) {
                w.push(*roster);
                put_str(&mut w, k);
                w.push(lv.level);
                w.extend_from_slice(&lv.xp.to_le_bytes());
            }
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
        while r.p < bytes.len() {
            match r.u8()? {
                TAIL_CLEARED_BOSSES => {
                    for _ in 0..r.u16()? {
                        out.cleared_bosses.insert(r.str()?);
                    }
                }
                TAIL_MAP => {
                    out.map = Some(crate::map::MapLocation {
                        map_id: r.str()?,
                        x: r.u16()? as i16,
                        z: r.u16()? as i16,
                        facing: r.u16()?,
                    });
                }
                TAIL_CARDS => {
                    let c = &mut out.cards;
                    c.starter_granted = match r.u8()? {
                        0 => false,
                        1 => true,
                        _ => return None,
                    };
                    c.rng = u64::from_le_bytes(r.take(8)?.try_into().ok()?);
                    for _ in 0..r.u16()? {
                        let id = r.str()?;
                        let n = r.u32()?;
                        c.collection.add(&id, n);
                    }
                    for _ in 0..r.u16()? {
                        let id = r.str()?;
                        let n = r.u32()?;
                        if n > 0 {
                            c.boosters.insert(id, n);
                        }
                    }
                    for _ in 0..r.u16()? {
                        c.deck.push(r.str()?);
                    }
                    for _ in 0..r.u16()? {
                        c.seen.insert(r.str()?);
                    }
                }
                TAIL_ITEMS => {
                    let it = &mut out.items;
                    for _ in 0..r.u16()? {
                        let k = r.str()?;
                        let n = r.u8()?;
                        if n > 0 {
                            it.bag.insert(k, n.min(crate::items::STACK_CAP));
                        }
                    }
                    for _ in 0..r.u16()? {
                        let roster = r.u8()?;
                        let index = r.u8()?;
                        if index > 7 {
                            return None;
                        }
                        it.worn.insert((roster, index), r.str()?);
                    }
                }
                TAIL_ARTS_SPELLS => {
                    for _ in 0..r.u16()? {
                        out.arts.learned.insert(r.str()?);
                    }
                    for _ in 0..r.u16()? {
                        let roster = r.u8()?;
                        let key = r.str()?;
                        let level = r.u8()?;
                        let xp = r.u32()?;
                        if level == 0 || level > legaia_expansion_format::spells::MAX_LEVEL {
                            return None;
                        }
                        out.spells
                            .known
                            .insert((roster, key), crate::spells::SpellLevel { level, xp });
                    }
                }
                _ => return None,
            }
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

/// Tail record tag: the cleared-boss list.
const TAIL_CLEARED_BOSSES: u8 = 1;
/// Tail record tag: the pack map the save was made on.
const TAIL_MAP: u8 = 2;
/// Tail record tag: the trading-card state.
const TAIL_CARDS: u8 = 3;
/// Tail record tag: the pack items held and worn.
const TAIL_ITEMS: u8 = 4;
/// Tail record tag: the pack arts and spells known.
const TAIL_ARTS_SPELLS: u8 = 5;

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
        p.cleared_bosses.insert("hollow_sovereign".into());
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
    fn a_map_location_rides_after_the_challenges_and_only_when_set() {
        let plain = sample().encode();
        let mut p = sample();
        p.map = Some(crate::map::MapLocation {
            map_id: "rebirth_tower_1f".into(),
            x: 2880,
            z: -64,
            facing: 0x800,
        });
        let bytes = p.encode();
        assert_eq!(
            &bytes[..plain.len()],
            &plain[..],
            "the block it extends is unchanged"
        );
        assert_eq!(ExpansionProgress::decode(&bytes), Some(p));
        assert!(ExpansionProgress::decode(&bytes[..bytes.len() - 1]).is_none());
    }

    #[test]
    fn the_card_record_round_trips_and_is_absent_without_cards() {
        let plain = sample().encode();
        let mut p = sample();
        assert_eq!(p.cards, Default::default());
        assert_eq!(p.encode(), plain, "no card state, no card record");
        let c = &mut p.cards;
        c.collection.add("mist-wisp", 2);
        c.collection.add("ash-hound", 1);
        c.collection.move_in_binder(1, 0);
        c.boosters.insert("rebirth-starter".into(), 3);
        c.deck = vec!["ash-hound".into(), "mist-wisp".into()];
        c.seen.insert("mist-wisp".into());
        c.starter_granted = true;
        c.rng = 0xDEAD_BEEF_0123_4567;
        let bytes = p.encode();
        assert_eq!(&bytes[..plain.len()], &plain[..], "appended after the rest");
        let back = ExpansionProgress::decode(&bytes).unwrap();
        assert_eq!(back.cards.collection.binder(), ["ash-hound", "mist-wisp"]);
        assert_eq!(back, p);
        assert!(ExpansionProgress::decode(&bytes[..bytes.len() - 1]).is_none());
        // The card record follows the map record when both are present.
        p.map = Some(crate::map::MapLocation {
            map_id: "m".into(),
            x: 1,
            z: 2,
            facing: 3,
        });
        assert_eq!(ExpansionProgress::decode(&p.encode()), Some(p));
    }

    #[test]
    fn the_item_record_round_trips_and_is_absent_without_items() {
        let plain = sample().encode();
        let mut p = sample();
        assert_eq!(p.encode(), plain, "no pack items, no item record");
        p.items.add("rebirth_blade", 1);
        p.items.add("phoenix_plume", 7);
        p.items.add("rebirth_mail", 1);
        assert!(p.items.equip("rebirth_mail", 0, 0));
        let bytes = p.encode();
        assert_eq!(&bytes[..plain.len()], &plain[..], "appended after the rest");
        assert_eq!(bytes[plain.len()], 4, "tag 4");
        let back = ExpansionProgress::decode(&bytes).unwrap();
        assert_eq!(back.items.count("phoenix_plume"), 7);
        assert_eq!(back.items.worn_at(0, 0), Some("rebirth_mail"));
        assert_eq!(back, p);
        assert!(ExpansionProgress::decode(&bytes[..bytes.len() - 1]).is_none());
    }

    #[test]
    fn the_arts_and_spells_record_round_trips_and_is_absent_without_any() {
        let plain = sample().encode();
        let mut p = sample();
        assert_eq!(p.encode(), plain, "nothing known, no record");
        p.arts.learned.insert("ember_cascade".into());
        assert!(p.spells.learn(1, "cinder_wyrm"));
        p.spells
            .known
            .get_mut(&(1, "cinder_wyrm".into()))
            .unwrap()
            .level = 3;
        p.spells
            .known
            .get_mut(&(1, "cinder_wyrm".into()))
            .unwrap()
            .xp = 99;
        let bytes = p.encode();
        assert_eq!(&bytes[..plain.len()], &plain[..], "appended after the rest");
        assert_eq!(bytes[plain.len()], 5, "tag 5");
        let back = ExpansionProgress::decode(&bytes).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.spells.level(1, "cinder_wyrm").unwrap().level, 3);
        assert!(ExpansionProgress::decode(&bytes[..bytes.len() - 1]).is_none());
        // A level outside 1..=9 is a damaged block.
        let mut bad = bytes.clone();
        let n = bad.len();
        bad[n - 5] = 0;
        assert!(ExpansionProgress::decode(&bad).is_none());
        // Items and arts together keep their tag order.
        p.items.add("phoenix_plume", 2);
        let both = p.encode();
        assert_eq!(ExpansionProgress::decode(&both).unwrap(), p);
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

    #[test]
    fn a_block_without_cleared_bosses_is_the_older_layout() {
        let mut p = sample();
        p.cleared_bosses.clear();
        let bytes = p.encode();
        // The challenge list is the last field written.
        let tail = b"c";
        let at = bytes.windows(1).rposition(|w| w == tail).unwrap();
        assert_eq!(
            bytes.len(),
            at + 1 + 8,
            "nothing follows the challenge record"
        );
        assert_eq!(ExpansionProgress::decode(&bytes), Some(p));
    }
}

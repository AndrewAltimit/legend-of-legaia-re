//! A save made with one version of a pack, loaded with another: drop what the
//! pack no longer defines, keep everything else, and say what was dropped.
//!
//! The expansion block ([`crate::quest_save`]) names pack content by id - a
//! quest, a challenge, a once-only boss, a map, a card, a booster set, a pack
//! item, an art, a spell. A newer (or older) version of the pack may have
//! renamed or removed any of them. Left in place, a vanished id would sit in
//! the Journal with no definition, hold a binder slot no card fills, or seat
//! the player on a map that does not exist. [`reconcile`] runs whenever
//! saved progress is adopted for the active pack, and:
//!
//! - removes every entry whose id the pack no longer defines (a card's
//!   copies, a booster's count, a worn item's slot all go with it; the duel
//!   deck keeps the cards that remain);
//! - resizes a kept quest's objective progress to the quest's current
//!   objective count (new objectives start at zero, removed ones are
//!   dropped), so the tracker never indexes past its definition;
//! - forgets a saved pack-map location whose map is gone, so the resume
//!   lands in the map's base scene instead;
//! - keeps pack flags and counters as they are: they are free-form names a
//!   pack reads, not definitions, and a flag nothing reads is harmless.
//!
//! It never fails and never invents progress. The [`CompatReport`] it
//! returns is what the hosts show (one toast) and log (every entry).

use std::collections::BTreeSet;

use legaia_expansion_format::ExpansionPack;

use crate::quest_save::ExpansionProgress;

/// One saved entry the pack no longer defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// What it was: `quest`, `challenge`, `boss`, `map`, `card`, `booster`,
    /// `item`, `art`, `spell`.
    pub kind: &'static str,
    pub id: String,
}

/// What reconciling a save with the active pack changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompatReport {
    pub pack_id: String,
    /// The pack version the progress was written under.
    pub saved_version: String,
    /// The version now active.
    pub pack_version: String,
    /// Entries removed, in the order above.
    pub dropped: Vec<Dropped>,
    /// Quests whose objective list changed length, so their progress was
    /// resized.
    pub resized: Vec<String>,
}

impl CompatReport {
    /// Did reconciling change nothing.
    pub fn is_clean(&self) -> bool {
        self.dropped.is_empty() && self.resized.is_empty()
    }

    /// Did the save come from another version of the pack.
    pub fn version_changed(&self) -> bool {
        !self.saved_version.is_empty() && self.saved_version != self.pack_version
    }

    /// The one-line notice a host shows, `None` when there is nothing to
    /// say: `Rebirth 0.2.0: 2 saved entries are gone from the pack and were
    /// dropped`.
    pub fn notice(&self) -> Option<String> {
        if self.dropped.is_empty() {
            return None;
        }
        let n = self.dropped.len();
        let what = if n == 1 {
            format!(
                "{} {} is gone from the pack and was dropped",
                self.dropped[0].kind, self.dropped[0].id
            )
        } else {
            format!("{n} saved entries are gone from the pack and were dropped")
        };
        Some(if self.version_changed() {
            format!("Saved with {} {}: {what}", self.pack_id, self.saved_version)
        } else {
            format!("{}: {what}", self.pack_id)
        })
    }

    /// Every change, one line each, for the log.
    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .dropped
            .iter()
            .map(|d| format!("dropped {} '{}'", d.kind, d.id))
            .collect();
        out.extend(
            self.resized
                .iter()
                .map(|q| format!("quest '{q}': objective progress resized")),
        );
        out
    }
}

/// Bring `progress` (saved for `pack`'s id, maybe under another version) in
/// line with what `pack` defines. See the module docs.
pub fn reconcile(progress: &mut ExpansionProgress, pack: &ExpansionPack) -> CompatReport {
    let mut r = CompatReport {
        pack_id: pack.manifest.id.clone(),
        saved_version: progress.pack_version.clone(),
        pack_version: pack.manifest.version.clone(),
        ..Default::default()
    };
    let mut drop = |kind: &'static str, id: &str| {
        r.dropped.push(Dropped {
            kind,
            id: id.to_string(),
        });
    };

    // Quests and their objective vectors.
    let mut resized = Vec::new();
    progress.quests.retain(|id, state| {
        let Some(def) = pack.quests.quest(id) else {
            drop("quest", id);
            return false;
        };
        if state.objectives.len() != def.objectives.len() {
            state.objectives.resize(def.objectives.len(), 0);
            resized.push(id.clone());
        }
        true
    });
    progress.challenges.retain(|id, _| {
        let keep = pack.quests.challenge(id).is_some();
        if !keep {
            drop("challenge", id);
        }
        keep
    });
    progress.cleared_bosses.retain(|id| {
        let keep = pack.battle.bosses.contains_key(id);
        if !keep {
            drop("boss", id);
        }
        keep
    });
    if let Some(m) = &progress.map
        && !pack.maps.contains_key(&m.map_id)
    {
        drop("map", &m.map_id);
        progress.map = None;
    }

    // Cards: the collection, boosters, deck and seen marks.
    let cards: BTreeSet<&str> = pack.cards.cards.iter().map(|c| c.id.as_str()).collect();
    let c = &mut progress.cards;
    let gone: Vec<String> = c
        .collection
        .binder()
        .iter()
        .filter(|id| !cards.contains(id.as_str()))
        .cloned()
        .collect();
    for id in gone {
        let n = c.collection.count(&id);
        // `remove` of exactly the count held cannot fail.
        let _ = c.collection.remove(&id, n);
        drop("card", &id);
    }
    let sets: BTreeSet<&str> = pack.cards.boosters.iter().map(|b| b.id.as_str()).collect();
    c.boosters.retain(|id, _| {
        let keep = sets.contains(id.as_str());
        if !keep {
            drop("booster", id);
        }
        keep
    });
    c.deck.retain(|id| cards.contains(id.as_str()));
    c.seen.retain(|id| cards.contains(id.as_str()));

    // Pack items held and worn.
    let it = &mut progress.items;
    it.bag.retain(|k, _| {
        let keep = pack.items.items.contains_key(k);
        if !keep {
            drop("item", k);
        }
        keep
    });
    it.worn.retain(|_, k| {
        let keep = pack.items.items.contains_key(k);
        if !keep {
            drop("item", k);
        }
        keep
    });

    // Arts and spells known.
    progress.arts.learned.retain(|k| {
        let keep = pack.arts.arts.contains_key(k);
        if !keep {
            drop("art", k);
        }
        keep
    });
    progress.spells.known.retain(|(_, k), _| {
        let keep = pack.spells.spells.contains_key(k);
        if !keep {
            drop("spell", k);
        }
        keep
    });

    // An item worn by two roster slots, or a spell known by two, is one
    // definition gone: name it once.
    let mut seen = BTreeSet::new();
    r.dropped.retain(|d| seen.insert((d.kind, d.id.clone())));
    r.resized = resized;
    progress.pack_version = pack.manifest.version.clone();
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::MapLocation;
    use crate::quest::{QuestState, QuestStatus};
    use crate::quest_challenge::ChallengeRecord;

    fn pack(version: &str, extra: &[(&str, &str)]) -> ExpansionPack {
        let mut files = legaia_expansion_format::PackFiles::new();
        files.insert(
            "pack.toml".into(),
            format!("id = \"p\"\nname = \"P\"\nversion = \"{version}\"\n").into_bytes(),
        );
        for (path, text) in extra {
            files.insert((*path).into(), text.as_bytes().to_vec());
        }
        ExpansionPack::from_files(&files).unwrap()
    }

    const QUESTS: &str = r#"
[[quests]]
id = "kept"
title = "Kept"
objectives = [
    { kind = "win_battles", count = 1 },
    { kind = "win_battles", count = 2 },
]
"#;
    const CARDS: &str = r#"
[[cards]]
id = "ember"
name = "Ember"
"#;

    fn saved() -> ExpansionProgress {
        let mut p = ExpansionProgress {
            pack_id: "p".into(),
            pack_version: "1.0.0".into(),
            ..Default::default()
        };
        for id in ["kept", "gone"] {
            p.quests.insert(
                id.into(),
                QuestState {
                    status: QuestStatus::Active,
                    objectives: vec![1],
                    completions: 0,
                    last_completed_at: 0,
                },
            );
        }
        p.challenges
            .insert("old_trial".into(), ChallengeRecord::default());
        p.cleared_bosses.insert("old_boss".into());
        p.map = Some(MapLocation {
            map_id: "old_map".into(),
            x: 1,
            z: 2,
            facing: 0,
        });
        p.cards.collection.add("ember", 2);
        p.cards.collection.add("frost", 3);
        p.cards.deck = vec!["ember".into(), "frost".into()];
        p.cards.seen.insert("frost".into());
        p.cards.boosters.insert("old_set".into(), 1);
        p.items.bag.insert("old_potion".into(), 4);
        p.items.worn.insert((0, 3), "old_ring".into());
        p.items.worn.insert((1, 3), "old_ring".into());
        p.arts.learned.insert("old_art".into());
        p
    }

    #[test]
    fn vanished_ids_are_dropped_and_named() {
        let pk = pack("2.0.0", &[("quests.toml", QUESTS), ("cards.toml", CARDS)]);
        let mut p = saved();
        let r = reconcile(&mut p, &pk);
        let kinds: Vec<(&str, &str)> = r.dropped.iter().map(|d| (d.kind, d.id.as_str())).collect();
        assert_eq!(
            kinds,
            [
                ("quest", "gone"),
                ("challenge", "old_trial"),
                ("boss", "old_boss"),
                ("map", "old_map"),
                ("card", "frost"),
                ("booster", "old_set"),
                ("item", "old_potion"),
                ("item", "old_ring"),
                ("art", "old_art"),
            ]
        );
        assert_eq!(r.resized, ["kept"]);
        assert_eq!(
            p.quests["kept"].objectives,
            [1, 0],
            "progress kept, new objective at 0"
        );
        assert_eq!(p.cards.collection.count("ember"), 2);
        assert_eq!(p.cards.collection.binder(), ["ember"]);
        assert_eq!(p.cards.deck, ["ember"]);
        assert!(p.cards.seen.is_empty());
        assert!(p.items.is_empty());
        assert!(p.map.is_none());
        assert_eq!(p.pack_version, "2.0.0");
        let n = r.notice().unwrap();
        assert!(n.contains("1.0.0") && n.contains("9 saved entries"), "{n}");

        // The reconciled progress encodes and decodes, and reconciling it
        // again changes nothing.
        let back = ExpansionProgress::decode(&p.encode()).unwrap();
        assert_eq!(back, p);
        let mut again = back;
        assert!(reconcile(&mut again, &pk).is_clean());
    }

    #[test]
    fn a_matching_pack_changes_nothing() {
        let pk = pack("1.0.0", &[("quests.toml", QUESTS)]);
        let mut p = ExpansionProgress::new(&pk);
        p.quests.insert(
            "kept".into(),
            QuestState {
                status: QuestStatus::Completed,
                objectives: vec![1, 1],
                completions: 1,
                last_completed_at: 9,
            },
        );
        let before = p.clone();
        let r = reconcile(&mut p, &pk);
        assert!(r.is_clean());
        assert!(r.notice().is_none());
        assert_eq!(p, before);
    }
}

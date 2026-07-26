//! Split the monster roster into **random encounters** and **bosses**, from the
//! disc's own encounter tables.
//!
//! The difficulty scale ([`crate::monster_stats::ScaleProfile`]) wants to move
//! trash mobs and set-piece fights by different amounts, which needs a per-id
//! answer to "is this a boss?". Nothing in a monster's `battle_data` record says
//! so - the record is stats, rewards and AI, with no encounter context - so the
//! classification is read off the thing that *does* know: each scene's formation
//! table, and which of its formations a random-encounter roll can actually
//! produce.
//!
//! [`crate::encounter`] already draws that line, because the encounter
//! randomizer has to: a formation is a **random encounter** iff some region with
//! `rate_increment > 0` reaches it, and everything else is a scripted fight the
//! field VM engages by explicit index (see
//! [`SceneEncounters::is_random_formation`]). This module reuses that mask
//! verbatim rather than re-deriving it, so the two features cannot disagree
//! about what a boss is - and the explicit
//! [`crate::encounter::PROTECTED_FORMATION_IDS`] guard that fixes the one
//! formation the region heuristic misreads (the early Gimard fight) is inherited
//! for free.
//!
//! ## The rule
//!
//! Walk every scene bundle and record, per monster id, whether it was seen in a
//! random formation, a scripted one, or both. Then:
//!
//! | seen random | seen scripted | class |
//! |---|---|---|
//! | yes | no | regular |
//! | yes | yes | regular |
//! | no | yes | **boss** |
//! | no | no | regular |
//!
//! The two rows worth justifying are the ones that aren't obvious.
//!
//! **Seen in both is regular.** A monster the player can meet on a random step
//! is a random encounter, whatever else it also does in a scripted fight. Some
//! ordinary enemies get a scripted appearance (an ambush, a story-gated fight)
//! without being bosses, and grouping those with Songi would let the boss slider
//! silently retune half the trash roster.
//!
//! **Seen nowhere is regular.** These are the ids no formation references - the
//! unused/cut enemies [`crate::unused::UNUSED_ENEMY_IDS`] curates. They are not
//! bosses, and the unused-content option can place them into ordinary random
//! encounters, where being scaled by the boss slider would be actively wrong.
//!
//! ## The curated floor
//!
//! That last row is also where the scan is blind: a boss **form** the game
//! swaps in mid-battle (Cort's later phases, a transformed Songi) is never named
//! by a formation record at all, so no amount of scanning can find it. So
//! [`ClassScan::finish`] takes an `always_boss` list - in practice
//! [`crate::monster_stats::STORY_BOSS_MONSTER_IDS`], the hand-curated set the
//! stat *shuffle* already guards - and unions it in. The scan supplies breadth
//! (every scripted-only enemy on the disc, including ones no hand list would
//! think to name); the curated list supplies the forms the encounter tables
//! cannot see.
//!
//! The two provenances stay checkable against each other rather than collapsing
//! into one claim: `monster_class_agrees_with_curated_bosses` in
//! `tests/monster_stats_real.rs` asserts the *scan alone* never sees a curated
//! boss in a random formation. That is the property the union can't fake - a
//! disagreement there would mean the region-rate heuristic had started calling a
//! boss fight a random encounter, and the test fails rather than the boss
//! quietly reverting to the trash multiplier. On the retail disc the two lists
//! overlap heavily and each contributes something the other misses: the curated
//! list supplies Caruban and Cort's later forms, which no formation names, and
//! the scan supplies several scripted-only fights the hand list never listed.
//!
//! Finally [`MonsterClasses::force_regular`] takes the opening fights back out -
//! see its own docs for why the disc classifies two of the first three Piura as
//! scripted, and why the difficulty knob must not honour that.
//!
//! The scan is only run when the two halves of the profile actually differ; a
//! uniform scale needs no classification and pays nothing for this (see
//! [`crate::apply::scale_monster_stats_profile`]).

use crate::encounter::SceneEncounters;

/// Which half of a [`crate::monster_stats::ScaleProfile`] a monster is scaled by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonsterClass {
    /// An enemy the player can meet on a random encounter roll - and every id
    /// no formation references at all (see the module docs).
    Regular,
    /// A scripted-only fight: reachable by the field VM's explicit formation
    /// index, never by a random roll.
    Boss,
}

/// The finished per-id classification, over the whole byte id space a formation
/// slot can hold.
///
/// Indexed by the 1-based `battle_data` monster id exactly as a formation slot
/// stores it, so no offset math at the lookup. Ids `> 255` cannot appear in a
/// formation and read as [`MonsterClass::Regular`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterClasses {
    boss: [bool; 256],
}

impl MonsterClasses {
    /// Every monster is a regular enemy. The classification a **uniform** scale
    /// uses, where the two halves of the profile are equal and the split cannot
    /// change a single byte - so the disc scan is skipped entirely.
    pub fn all_regular() -> Self {
        Self { boss: [false; 256] }
    }

    /// Build from an explicit boss-id list. Ids outside `0..=255` are ignored
    /// (no retail monster id exceeds a byte). For tests and for a caller that
    /// already knows its answer; the disc-derived path is [`ClassScan`].
    pub fn from_boss_ids(ids: impl IntoIterator<Item = u16>) -> Self {
        let mut boss = [false; 256];
        for id in ids {
            if let Some(slot) = boss.get_mut(id as usize) {
                *slot = true;
            }
        }
        Self { boss }
    }

    /// Whether `id` is a scripted-only fight.
    pub fn is_boss(&self, id: u16) -> bool {
        self.boss.get(id as usize).copied().unwrap_or(false)
    }

    /// Which half of the profile scales `id`.
    pub fn class_of(&self, id: u16) -> MonsterClass {
        if self.is_boss(id) {
            MonsterClass::Boss
        } else {
            MonsterClass::Regular
        }
    }

    /// Force `ids` back to [`MonsterClass::Regular`], whatever the scan and the
    /// curated floor said.
    ///
    /// One case needs it, and the disc is the reason. Two of the three first
    /// wild Piura are only ever *scripted* - the encounter tables show id 21
    /// rolling on a random step but 19 and 20 appearing solely at fixed
    /// formation indices - so the rule in the module docs classifies the opening
    /// fights of a fresh save as bosses. That is technically what they are and
    /// emphatically not what the knob means: "make the bosses 5x" must not make
    /// the tutorial lethal. [`crate::monster_stats::TUTORIAL_MONSTER_IDS`] is
    /// already the curated set for "must stay beatable at level 1", so it is the
    /// list that belongs here.
    pub fn force_regular(&mut self, ids: &[u16]) {
        for &id in ids {
            if let Some(slot) = self.boss.get_mut(id as usize) {
                *slot = false;
            }
        }
    }

    /// How many ids classify as bosses. The number a run manifest reports, and
    /// the one a test asserts is neither `0` (the scan found nothing) nor most
    /// of the roster (the mask inverted).
    pub fn boss_count(&self) -> usize {
        self.boss.iter().filter(|&&b| b).count()
    }

    /// The boss ids, ascending.
    pub fn boss_ids(&self) -> Vec<u16> {
        (0..256u16).filter(|&id| self.boss[id as usize]).collect()
    }
}

/// Accumulates the random / scripted observation across a corpus of scenes.
///
/// Kept separate from [`MonsterClasses`] because the answer is only correct once
/// **every** scene has been seen: a monster that is scripted in the scene the
/// caller happens to visit first can still be a random encounter three scenes
/// later, and a partial scan would call it a boss. [`Self::finish`] is the point
/// where the two observations become a verdict.
#[derive(Debug, Clone)]
pub struct ClassScan {
    random: [bool; 256],
    scripted: [bool; 256],
}

impl Default for ClassScan {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassScan {
    /// An empty scan - nothing observed yet.
    pub fn new() -> Self {
        Self {
            random: [false; 256],
            scripted: [false; 256],
        }
    }

    /// Record every monster id one scene's formation table holds, tagged by
    /// whether its formation is a random encounter.
    ///
    /// Slot id `0` is the empty-slot sentinel (a formation declaring fewer than
    /// its stride's worth of monsters) and is skipped, so it never lands in
    /// either set.
    pub fn observe(&mut self, scene: &SceneEncounters) {
        for i in 0..scene.formation_count() {
            let seen = if scene.is_random_formation(i) {
                &mut self.random
            } else {
                &mut self.scripted
            };
            for id in scene.formation_ids(i) {
                if id == 0 {
                    continue;
                }
                seen[id as usize] = true;
            }
        }
    }

    /// Apply the rule in the module docs: a boss is an id seen in a scripted
    /// formation and never in a random one, **or** named in `always_boss`.
    ///
    /// `always_boss` is the curated floor the scan cannot derive - the boss
    /// forms swapped in mid-battle, which no formation record names. It is a
    /// union, not an override: an id there is a boss even if the scan never saw
    /// it, and the scan's own findings are kept whether or not the list mentions
    /// them. Pass an empty slice for the raw scan.
    pub fn finish(&self, always_boss: &[u16]) -> MonsterClasses {
        let mut boss = [false; 256];
        for (id, slot) in boss.iter_mut().enumerate() {
            *slot = self.scripted[id] && !self.random[id];
        }
        for &id in always_boss {
            if let Some(slot) = boss.get_mut(id as usize) {
                *slot = true;
            }
        }
        MonsterClasses { boss }
    }

    /// Whether `id` was seen in at least one random formation. Exposed so a
    /// caller can tell "regular because it rolls" from "regular because nothing
    /// references it" - the two rows the module docs justify separately.
    pub fn seen_random(&self, id: u16) -> bool {
        self.random.get(id as usize).copied().unwrap_or(false)
    }

    /// Whether `id` was seen in at least one scripted formation.
    pub fn seen_scripted(&self, id: u16) -> bool {
        self.scripted.get(id as usize).copied().unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_regular_has_no_bosses() {
        let c = MonsterClasses::all_regular();
        assert_eq!(c.boss_count(), 0);
        assert_eq!(c.class_of(138), MonsterClass::Regular);
        assert!(c.boss_ids().is_empty());
    }

    #[test]
    fn explicit_boss_ids_round_trip() {
        let c = MonsterClasses::from_boss_ids([138, 10, 76]);
        assert_eq!(c.boss_ids(), vec![10, 76, 138], "ascending");
        assert_eq!(c.boss_count(), 3);
        assert_eq!(c.class_of(76), MonsterClass::Boss);
        assert_eq!(c.class_of(77), MonsterClass::Regular);
        // Out-of-byte ids can't appear in a formation and never classify boss.
        assert!(!MonsterClasses::from_boss_ids([300]).is_boss(300));
    }

    /// The four rows of the classification table, driven straight through the
    /// scan's own observation setters rather than a real scene.
    #[test]
    fn scan_applies_the_classification_rule() {
        let mut scan = ClassScan::new();
        // 11: random only. 12: scripted only. 13: both. 14: neither.
        scan.random[11] = true;
        scan.scripted[12] = true;
        scan.random[13] = true;
        scan.scripted[13] = true;

        let c = scan.finish(&[]);
        assert_eq!(c.class_of(11), MonsterClass::Regular, "random only");
        assert_eq!(c.class_of(12), MonsterClass::Boss, "scripted only");
        assert_eq!(
            c.class_of(13),
            MonsterClass::Regular,
            "seen in both is regular - a monster the player can meet on a step"
        );
        assert_eq!(
            c.class_of(14),
            MonsterClass::Regular,
            "never referenced (an unused enemy) is regular, not boss"
        );
        assert_eq!(c.boss_ids(), vec![12]);

        assert!(scan.seen_random(11) && !scan.seen_scripted(11));
        assert!(!scan.seen_random(12) && scan.seen_scripted(12));
        assert!(scan.seen_random(13) && scan.seen_scripted(13));
        assert!(!scan.seen_random(14) && !scan.seen_scripted(14));
    }

    /// A scan is only a verdict once every scene is in: a monster scripted in
    /// one scene and random in another must not classify boss on the strength of
    /// the first scene alone.
    #[test]
    fn later_scenes_can_demote_a_boss_to_regular() {
        let mut scan = ClassScan::new();
        scan.scripted[40] = true;
        assert_eq!(scan.finish(&[]).class_of(40), MonsterClass::Boss);
        scan.random[40] = true;
        assert_eq!(scan.finish(&[]).class_of(40), MonsterClass::Regular);
    }

    /// The curated floor is a union: it adds ids the scan never saw without
    /// dropping the ones it did, and it outranks the random-formation demotion
    /// (a mid-battle boss form named in the list stays a boss).
    #[test]
    fn curated_floor_unions_with_the_scan() {
        let mut scan = ClassScan::new();
        scan.scripted[12] = true; // scan-derived boss
        scan.random[13] = true; // scan-derived regular

        let c = scan.finish(&[13, 90]);
        assert_eq!(c.class_of(12), MonsterClass::Boss, "scan finding is kept");
        assert_eq!(
            c.class_of(90),
            MonsterClass::Boss,
            "a curated form the scan never saw is still a boss"
        );
        assert_eq!(
            c.class_of(13),
            MonsterClass::Boss,
            "the curated list outranks the random-formation demotion"
        );
        assert_eq!(c.boss_ids(), vec![12, 13, 90]);
    }
}

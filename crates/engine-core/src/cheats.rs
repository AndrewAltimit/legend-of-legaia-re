//! Player-facing **cheats** - the mutations behind the browser play page's
//! Cheats panel and the native `play-window`'s `--cheat-*` flags.
//!
//! Every cheat goes through the engine's own rules rather than poking raw
//! numbers into a record, so the state it leaves is one a real playthrough
//! could reach:
//!
//! * **Level** grants exactly the XP that reaches the target level through
//!   the party's [`crate::levelup::LevelUpTracker`], so the gains are the
//!   retail per-character growth (`FUN_801E9504`'s tables, installed at boot
//!   by [`World::install_retail_progression_tables`]) and the record's XP,
//!   next-level threshold and displayed level all agree. A level can only be
//!   raised - growth is not reversible.
//! * **Items** go through the bag's retail add helper ([`crate::world::ItemBag::add`]):
//!   they stack to 99 and land inside the installed active window, so a full
//!   window refuses rather than writing past it.
//! * **Gold** and **coins** clamp to the purses' retail ceilings
//!   ([`crate::shop::GOLD_CAP`], [`crate::casino_coin_bank::COIN_BANK_CEILING`]).
//!
//! Hosts own only the controls; this module owns every write.

use crate::levelup::{LevelUpTracker, MAX_LEVEL, MAX_PARTY};
use crate::world::World;

/// The result of one item grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemGrant {
    /// The item id granted.
    pub id: u8,
    /// How many of `qty` actually landed (stacks cap at 99; a full window
    /// lands none).
    pub granted: u8,
    /// The bag's count of `id` afterwards.
    pub held: u8,
}

/// Resolve an item query to an id: a decimal or `0x`-hex id, or a
/// case-insensitive item name (exact match first, then a unique prefix /
/// substring match). Punctuation and spacing are ignored, so `"healing leaf"`
/// and `"HealingLeaf"` both match. `names` yields `(id, name)` pairs.
pub fn resolve_item<'a>(query: &str, names: impl IntoIterator<Item = (u8, &'a str)>) -> Option<u8> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let parsed = if let Some(hex) = q.strip_prefix("0x").or_else(|| q.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16).ok()
    } else {
        q.parse::<u8>().ok()
    };
    if let Some(id) = parsed {
        return (id != 0).then_some(id);
    }
    let norm = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let want = norm(q);
    if want.is_empty() {
        return None;
    }
    let all: Vec<(u8, String)> = names
        .into_iter()
        .filter(|(id, _)| *id != 0)
        .map(|(id, n)| (id, norm(n)))
        .filter(|(_, n)| !n.is_empty())
        .collect();
    if let Some((id, _)) = all.iter().find(|(_, n)| *n == want) {
        return Some(*id);
    }
    let mut hits = all.iter().filter(|(_, n)| n.starts_with(&want));
    if let (Some((id, _)), None) = (hits.next(), hits.next()) {
        return Some(*id);
    }
    let mut hits = all.iter().filter(|(_, n)| n.contains(&want));
    match (hits.next(), hits.next()) {
        (Some((id, _)), None) => Some(*id),
        _ => None,
    }
}

impl World {
    /// The `(id, name)` pairs of the installed item-name table (empty when no
    /// executable was readable) - what [`resolve_item`] matches against.
    pub fn item_name_pairs(&self) -> Vec<(u8, String)> {
        let Some(t) = self.menu.text.as_ref().and_then(|t| t.item_names.as_ref()) else {
            return Vec::new();
        };
        (1u8..=255)
            .filter_map(|id| t.name(id).map(|n| (id, n.to_string())))
            .collect()
    }

    /// Raise roster slot `roster_slot`'s character to `level` (clamped to
    /// `1..=99`) by granting the XP that reaches it, with the retail stat
    /// growth applied. Returns the level the character is at afterwards, or
    /// `None` when the slot holds no character (no record, or a zeroed one).
    /// A target at or below the current level changes nothing.
    pub fn cheat_set_level(&mut self, roster_slot: usize, level: u8) -> Option<u8> {
        if roster_slot >= MAX_PARTY {
            return None;
        }
        if self
            .party
            .roster
            .members
            .get(roster_slot)
            .is_none_or(|r| r.hp_mp_sp().hp_max == 0)
        {
            return None;
        }
        let target = level.clamp(1, MAX_LEVEL);
        let tracker = &mut self.party.level_up_tracker;
        if target > tracker.level[roster_slot] {
            // XP that reaches `target`: the threshold out of `target - 1`.
            let need = tracker.threshold_for(roster_slot, target - 1);
            let have = tracker.xp[roster_slot];
            if let Some(need) = need
                && need > have
                && let Some(res) = tracker.grant_xp(roster_slot as u8, need - have)
                && let Some(rec) = self.party.roster.members.get_mut(roster_slot)
            {
                LevelUpTracker::apply_to_record(&res, rec);
            }
        }
        // Mirror the tracker onto the record the way a battle grant does
        // (`World::apply_battle_xp`): cumulative XP, the next threshold and
        // the displayed level the Status screen draws.
        let tracker = &self.party.level_up_tracker;
        let cur = tracker.level[roster_slot];
        let xp = tracker.xp[roster_slot];
        let next = tracker.threshold_for(roster_slot, cur).unwrap_or(0);
        if let Some(rec) = self.party.roster.members.get_mut(roster_slot) {
            rec.set_cumulative_xp(xp);
            rec.set_next_level_xp(next);
            rec.set_magic_rank(cur);
            rec.set_level(cur);
        }
        self.resync_party_after_cheat();
        Some(cur)
    }

    /// [`Self::cheat_set_level`] over every present party member. Returns
    /// `(roster slot, level after)` per member that holds a character.
    pub fn cheat_set_party_level(&mut self, level: u8) -> Vec<(u8, u8)> {
        let present: Vec<usize> = (0..usize::from(self.party.party_count.min(3)))
            .map(|m| self.party_roster_slot(m))
            .collect();
        present
            .into_iter()
            .filter_map(|r| self.cheat_set_level(r, level).map(|l| (r as u8, l)))
            .collect()
    }

    /// Set the gold purse, clamped to `0..=`[`crate::shop::GOLD_CAP`].
    /// Returns the purse afterwards.
    pub fn cheat_set_gold(&mut self, gold: i64) -> i32 {
        self.party.money = gold.clamp(0, i64::from(crate::shop::GOLD_CAP)) as i32;
        self.party.money
    }

    /// Set the casino coin bank, clamped to
    /// [`crate::casino_coin_bank::COIN_BANK_CEILING`]. Returns the bank
    /// afterwards.
    pub fn cheat_set_coins(&mut self, coins: u64) -> u32 {
        self.minigames.casino_coins =
            coins.min(u64::from(crate::casino_coin_bank::COIN_BANK_CEILING)) as u32;
        self.minigames.casino_coins
    }

    /// Add `qty` of item `id` through the bag's retail add helper. `None` for
    /// id `0`.
    pub fn cheat_give_item(&mut self, id: u8, qty: u8) -> Option<ItemGrant> {
        if id == 0 {
            return None;
        }
        let before = self.party.inventory.get(&id).copied().unwrap_or(0);
        if qty > 0 {
            self.party.inventory.add(id, qty);
        }
        let held = self.party.inventory.get(&id).copied().unwrap_or(0);
        Some(ItemGrant {
            id,
            granted: held.saturating_sub(before),
            held,
        })
    }

    /// Restore every roster member's HP and MP to their maxima (reviving the
    /// fallen) and refresh the live battle mirrors, so it works mid-fight too.
    pub fn cheat_restore_party(&mut self) {
        for rec in self.party.roster.members.iter_mut() {
            let mut hms = rec.hp_mp_sp();
            if hms.hp_max == 0 {
                continue;
            }
            hms.hp_cur = hms.hp_max;
            hms.mp_cur = hms.mp_max;
            rec.set_hp_mp_sp(hms);
        }
        for member in 0..usize::from(self.party.party_count.min(3)) {
            let rslot = self.party_roster_slot(member);
            let Some(hms) = self.party.roster.members.get(rslot).map(|r| r.hp_mp_sp()) else {
                continue;
            };
            if let Some(a) = self.actors.get_mut(member) {
                a.battle.hp = hms.hp_cur;
                a.battle.max_hp = hms.hp_max;
                a.battle.mp = hms.mp_cur;
                a.battle.liveness = u16::from(hms.hp_cur > 0);
            }
        }
    }

    /// After a record edit: re-fold the stats into the battle mirrors and
    /// carry each present member's new maxima onto its actor. Current HP / MP
    /// are left alone (a level-up is not a heal).
    fn resync_party_after_cheat(&mut self) {
        for member in 0..usize::from(self.party.party_count.min(3)) {
            let rslot = self.party_roster_slot(member);
            let Some(hms) = self.party.roster.members.get(rslot).map(|r| r.hp_mp_sp()) else {
                continue;
            };
            if let Some(a) = self.actors.get_mut(member) {
                a.battle.max_hp = hms.hp_max;
            }
        }
        self.seed_party_battle_stats();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::new_game::{StartingChar, StartingParty};

    fn tpl(name: &str, hp: u16) -> StartingChar {
        StartingChar {
            name: name.into(),
            hp_max: hp,
            mp_max: 20,
            agl: 100,
            atk: 24,
            udf: 16,
            ldf: 12,
            spd: 19,
            intel: 9,
        }
    }

    fn world_with_trio() -> World {
        let mut w = World::new();
        let party =
            StartingParty::from_members(vec![tpl("Vahn", 180), tpl("Noa", 150), tpl("Gala", 220)]);
        assert_eq!(w.seed_picker_party(&party, None), 3);
        w
    }

    #[test]
    fn set_level_grants_xp_and_growth() {
        let mut w = world_with_trio();
        // A per-level gain with an ATK term (the disc-free default grows
        // HP / MP only; boot installs the retail tables over it).
        w.party.level_up_tracker.stat_gains = [crate::levelup::StatGain {
            atk: 2,
            ..crate::levelup::StatGain::hp_mp(10, 5)
        }; MAX_PARTY];
        let hp_before = w.party.roster.members[0].hp_mp_sp().hp_max;
        let atk_before = w.party.roster.members[0].live_stats().atk;
        assert_eq!(w.cheat_set_level(0, 10), Some(10));
        let rec = &w.party.roster.members[0];
        assert_eq!(rec.level(), 10);
        assert_eq!(rec.magic_rank(), 10);
        let t = &w.party.level_up_tracker;
        assert_eq!(rec.cumulative_xp(), t.threshold_for(0, 9).unwrap());
        assert_eq!(rec.next_level_xp(), t.threshold_for(0, 10).unwrap());
        assert!(rec.hp_mp_sp().hp_max > hp_before, "growth raised max HP");
        assert!(rec.live_stats().atk > atk_before, "growth raised ATK");
        // A level-up is not a heal.
        assert_eq!(rec.hp_mp_sp().hp_cur, 180);
        assert_eq!(w.actors[0].battle.max_hp, rec.hp_mp_sp().hp_max);
        // Lowering is a no-op.
        assert_eq!(w.cheat_set_level(0, 3), Some(10));
        // An empty slot is not a character.
        assert_eq!(w.cheat_set_level(3, 10), None);
    }

    #[test]
    fn party_level_covers_every_present_member() {
        let mut w = world_with_trio();
        let got = w.cheat_set_party_level(5);
        assert_eq!(got, vec![(0, 5), (1, 5), (2, 5)]);
        // Noa (slot 1) and Gala (slot 2) take their own corrected thresholds.
        for slot in 0..3 {
            let rec = &w.party.roster.members[slot];
            assert_eq!(
                rec.cumulative_xp(),
                w.party.level_up_tracker.threshold_for(slot, 4).unwrap()
            );
        }
    }

    #[test]
    fn purses_clamp_to_their_ceilings() {
        let mut w = World::new();
        assert_eq!(w.cheat_set_gold(-5), 0);
        assert_eq!(w.cheat_set_gold(1234), 1234);
        assert_eq!(w.cheat_set_gold(i64::MAX), crate::shop::GOLD_CAP);
        assert_eq!(w.cheat_set_coins(500), 500);
        assert_eq!(
            w.cheat_set_coins(u64::MAX),
            crate::casino_coin_bank::COIN_BANK_CEILING
        );
    }

    #[test]
    fn items_stack_to_99_through_the_bag() {
        let mut w = World::new();
        let g = w.cheat_give_item(0x77, 5).unwrap();
        assert_eq!((g.granted, g.held), (5, 5));
        let g = w.cheat_give_item(0x77, 200).unwrap();
        assert_eq!(g.held, 99, "stack cap");
        assert_eq!(g.granted, 94);
        assert!(w.cheat_give_item(0, 1).is_none());
    }

    #[test]
    fn restore_heals_and_revives() {
        let mut w = world_with_trio();
        let mut hms = w.party.roster.members[1].hp_mp_sp();
        hms.hp_cur = 0;
        hms.mp_cur = 0;
        w.party.roster.members[1].set_hp_mp_sp(hms);
        w.actors[1].battle.hp = 0;
        w.actors[1].battle.liveness = 0;
        w.cheat_restore_party();
        assert_eq!(w.party.roster.members[1].hp_mp_sp().hp_cur, 150);
        assert_eq!(w.party.roster.members[1].hp_mp_sp().mp_cur, 20);
        assert_eq!(w.actors[1].battle.hp, 150);
        assert_eq!(w.actors[1].battle.liveness, 1);
    }

    #[test]
    fn item_queries_resolve_by_id_or_name() {
        let names = [
            (0x77u8, "Healing Leaf"),
            (0x78, "Healing Flower"),
            (0x80, "Door of Wind"),
        ];
        let it = || names.iter().map(|&(i, n)| (i, n));
        assert_eq!(resolve_item("119", it()), Some(0x77));
        assert_eq!(resolve_item("0x80", it()), Some(0x80));
        assert_eq!(resolve_item("healing leaf", it()), Some(0x77));
        assert_eq!(resolve_item("HealingFlower", it()), Some(0x78));
        assert_eq!(resolve_item("door", it()), Some(0x80));
        // Ambiguous prefix.
        assert_eq!(resolve_item("healing", it()), None);
        assert_eq!(resolve_item("0", it()), None);
        assert_eq!(resolve_item("", it()), None);
    }
}

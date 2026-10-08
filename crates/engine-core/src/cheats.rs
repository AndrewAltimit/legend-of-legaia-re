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
//!   next-level threshold and displayed level all agree. **Lowering** a level
//!   re-derives the character from its New Game template
//!   ([`crate::new_game::starting_record`]) plus the growth for the levels it
//!   keeps ([`World::cheat_rebuild_level`]) - growth is not reversible, so the
//!   only honest way down is to replay it from level 1.
//! * **Seru magic** goes through retail's learn edge
//!   ([`crate::magic_xp::learn_spell_prepend`], `FUN_801E92DC`) and the
//!   capture log the menus list from; levels are clamped to retail's cap of 9.
//! * **Arts** go through retail's ordered learned-art insert
//!   (`check_and_learn_art`, `FUN_801EFBFC`) into the record's `+0x185` list
//!   and the [`crate::tactical_arts::TacticalArtsTracker`] beside it.
//! * **AP** is the Spirit gauge (record `+0x10E`, battle actor `spirit_gauge`)
//!   filled to its ceiling of 100.
//! * **Items** go through the bag's retail add helper ([`crate::world::ItemBag::add`]):
//!   they stack to 99 and land inside the installed active window, so a full
//!   window refuses rather than writing past it.
//! * **Gold** and **coins** clamp to the purses' retail ceilings
//!   ([`crate::shop::GOLD_CAP`], [`crate::casino_coin_bank::COIN_BANK_CEILING`]).
//!
//! Hosts own only the controls; this module owns every write.

use crate::levelup::{LevelUpTracker, MAX_LEVEL, MAX_PARTY};
use crate::world::World;
use legaia_asset::new_game::{StartingChar, StartingParty};

/// The Spirit (AP) gauge ceiling - the value
/// [`World::spirit_gauge_full`] tests and the battle clamps to.
pub const AP_GAUGE_MAX: u16 = 100;

/// Retail's Seru-magic level cap (`FUN_801E70BC` guards `level < 9`).
pub const SERU_LEVEL_MAX: u8 = 9;

/// The player Seru-magic block every character can learn (`0x81..=0x95`,
/// the 21 named Seru - `docs/formats/spell-table.md`).
pub const SERU_SPELL_IDS: std::ops::RangeInclusive<u8> = 0x81..=0x95;

/// The Ra-Seru summon of roster slot `slot`: Meta (Vahn), Terra (Noa), Ozma
/// (Gala) at `0x9E..=0xA0`; `None` for any other slot.
pub fn ra_seru_spell_for(slot: usize) -> Option<u8> {
    (slot < 3).then(|| 0x9E + slot as u8)
}

/// The high-block summons roster slot `slot` carries in retail: its own
/// Ra-Seru, and for Vahn alone also the Evil Seru Magic (`0x99`) and the
/// Sim-Seru Palma / Mule / Horn / Jedo (`0x9A..=0x9D`). Capture-grounded:
/// across the save-state library those five ids only ever appear in slot 0's
/// spell list, while `0x9E` / `0x9F` / `0xA0` sit in slots 0 / 1 / 2.
pub fn summon_spells_for(slot: usize) -> Vec<u8> {
    let mut out = Vec::new();
    if slot == 0 {
        out.extend(0x99..=0x9D);
    }
    out.extend(ra_seru_spell_for(slot));
    out
}

/// The result of a Seru-magic grant for one character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeruGrant {
    /// Roster slot granted.
    pub slot: u8,
    /// Spells newly learned.
    pub learned: u8,
    /// Spells in the list afterwards.
    pub known: u8,
    /// The level every listed spell sits at afterwards.
    pub level: u8,
}

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
            rec.set_level(cur);
        }
        self.resync_party_after_cheat();
        Some(cur)
    }

    /// [`Self::cheat_set_level`] over every present party member. Returns
    /// `(roster slot, level after)` per member that holds a character.
    pub fn cheat_set_party_level(&mut self, level: u8) -> Vec<(u8, u8)> {
        self.cheat_set_party_level_with(level, None)
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
        self.mirror_party_records_to_actors();
    }

    /// [`Self::cheat_set_level`] for a level that may go **down**: when
    /// `level` is below the current one and `template` (this character's New
    /// Game template row) is given, rebuild through
    /// [`Self::cheat_rebuild_level`]; otherwise raise as usual.
    pub fn cheat_change_level(
        &mut self,
        roster_slot: usize,
        level: u8,
        template: Option<&StartingChar>,
    ) -> Option<u8> {
        let cur = *self.party.level_up_tracker.level.get(roster_slot)?;
        match template {
            Some(t) if level.clamp(1, MAX_LEVEL) < cur => {
                self.cheat_rebuild_level(roster_slot, level, t)
            }
            _ => self.cheat_set_level(roster_slot, level),
        }
    }

    /// Re-derive roster slot `roster_slot` at `level` from its New Game
    /// template: the record's stat block is reset to `template`'s seed
    /// ([`crate::new_game::starting_record`]), the tracker drops to level 1
    /// with 0 XP, and the XP that reaches `level` is granted back through the
    /// tracker - so the stats are the template plus the retail growth for
    /// exactly the levels kept. Permanent stat-up items and other off-curve
    /// stat edits are lost. Current HP / MP / AP are kept, clamped to the new
    /// maxima. Equipment, spells, arts and the name are untouched.
    pub fn cheat_rebuild_level(
        &mut self,
        roster_slot: usize,
        level: u8,
        template: &StartingChar,
    ) -> Option<u8> {
        if roster_slot >= MAX_PARTY {
            return None;
        }
        let rec = self.party.roster.members.get_mut(roster_slot)?;
        let old = rec.hp_mp_sp();
        if old.hp_max == 0 {
            return None;
        }
        let fresh = crate::new_game::starting_record(template);
        let mut hms = fresh.hp_mp_sp();
        hms.hp_cur = old.hp_cur;
        hms.mp_cur = old.mp_cur;
        hms.sp_cur = old.sp_cur;
        rec.set_hp_mp_sp(hms);
        rec.set_record_stats(fresh.record_stats());
        rec.set_live_stats(fresh.live_stats());
        rec.set_level(1);
        rec.set_cumulative_xp(0);
        let tracker = &mut self.party.level_up_tracker;
        tracker.level[roster_slot] = 1;
        tracker.xp[roster_slot] = 0;
        let got = self.cheat_set_level(roster_slot, level)?;
        if let Some(rec) = self.party.roster.members.get_mut(roster_slot) {
            let mut hms = rec.hp_mp_sp();
            hms.hp_cur = hms.hp_cur.min(hms.hp_max);
            hms.mp_cur = hms.mp_cur.min(hms.mp_max);
            hms.sp_cur = hms.sp_cur.min(AP_GAUGE_MAX);
            rec.set_hp_mp_sp(hms);
        }
        self.mirror_party_records_to_actors();
        Some(got)
    }

    /// [`Self::cheat_change_level`] over every present party member, each
    /// rebuilt from its own row of `templates` (indexed by roster slot) when
    /// lowering. Without templates a level only goes up. Returns `(roster
    /// slot, level after)` per member that holds a character.
    pub fn cheat_set_party_level_with(
        &mut self,
        level: u8,
        templates: Option<&StartingParty>,
    ) -> Vec<(u8, u8)> {
        self.present_roster_slots()
            .into_iter()
            .filter_map(|r| {
                let t = templates.and_then(|p| p.member(r));
                self.cheat_change_level(r, level, t).map(|l| (r as u8, l))
            })
            .collect()
    }

    /// Fill every present member's AP (Spirit) gauge to [`AP_GAUGE_MAX`]: the
    /// record's `+0x10E` (what the field Status page shows and the next
    /// battle seeds from) and, in battle, the actor's live gauge - so the
    /// Spirit / Super-Art command opens at once. Returns how many members
    /// were filled.
    pub fn cheat_max_ap(&mut self) -> usize {
        let mut n = 0;
        for (member, rslot) in self.present_roster_slots().into_iter().enumerate() {
            let Some(rec) = self.party.roster.members.get_mut(rslot) else {
                continue;
            };
            let mut hms = rec.hp_mp_sp();
            if hms.hp_max == 0 {
                continue;
            }
            hms.sp_cur = AP_GAUGE_MAX;
            rec.set_hp_mp_sp(hms);
            if let Some(a) = self.actors.get_mut(member) {
                a.battle.spirit_gauge = AP_GAUGE_MAX;
            }
            n += 1;
        }
        n
    }

    /// The Seru-magic ids [`Self::cheat_grant_seru`] teaches roster slot
    /// `slot`: the 21 player Seru plus [`summon_spells_for`]. With the executable's spell-name table installed, ids it
    /// does not name are left out.
    pub fn cheat_seru_spells_for(&self, slot: usize) -> Vec<u8> {
        let names = self.menu.text.as_ref().and_then(|t| t.spell_names.as_ref());
        SERU_SPELL_IDS
            .chain(summon_spells_for(slot))
            .filter(|&id| names.is_none_or(|n| n.name(id).is_some_and(|s| !s.is_empty())))
            .collect()
    }

    /// Teach every present member all of its Seru magic
    /// ([`Self::cheat_seru_spells_for`]) and set **every** listed spell's
    /// level to `level` (clamped `1..=`[`SERU_LEVEL_MAX`]). Learning goes
    /// through retail's prepend (`FUN_801E92DC`) plus the capture log, so
    /// the field and battle Magic lists pick the spells up at once; each
    /// spell's XP is set just past the threshold its level crossed, so the
    /// next level-up check stays coherent.
    pub fn cheat_grant_seru(&mut self, level: u8) -> Vec<SeruGrant> {
        let level = level.clamp(1, SERU_LEVEL_MAX);
        let thresholds = self.tables.magic_xp_thresholds;
        let mut out = Vec::new();
        for rslot in self.present_roster_slots() {
            if self
                .party
                .roster
                .members
                .get(rslot)
                .is_none_or(|r| r.hp_mp_sp().hp_max == 0)
            {
                continue;
            }
            let ids = self.cheat_seru_spells_for(rslot);
            let mut learned = 0u8;
            // Descending, so retail's prepend leaves the new block ascending.
            for &id in ids.iter().rev() {
                let rec = &mut self.party.roster.members[rslot];
                if crate::magic_xp::spell_slot(rec, id).is_some() {
                    continue;
                }
                if usize::from(rec.spell_list().count) >= legaia_save::MAX_SPELLS {
                    break;
                }
                crate::magic_xp::learn_spell_prepend(rec, id);
                learned += 1;
                let seru = self
                    .seru
                    .registry
                    .seru_for_spell(id)
                    .map_or(u16::from(id), |d| d.id);
                self.seru.log.mark_learned(rslot as u8, seru, id);
            }
            let rec = &mut self.party.roster.members[rslot];
            let mut list = rec.spell_list();
            let known = (list.count as usize).min(list.ids.len());
            for l in &mut list.levels[..known] {
                *l = level;
            }
            rec.set_spell_list(list);
            let xp = match (level, thresholds) {
                (2.., Some(t)) => u32::from(t[usize::from(level) - 2]) + 1,
                _ => 0,
            };
            for slot in 0..known.min(crate::magic_xp::SPELL_SEARCH_BOUND) {
                let off = crate::magic_xp::SPELL_XP_OFFSET + slot * 4;
                rec.raw[off..off + 4].copy_from_slice(&xp.to_le_bytes());
            }
            out.push(SeruGrant {
                slot: rslot as u8,
                learned,
                known: known as u8,
                level,
            });
        }
        out
    }

    /// Teach every present member every art the executable's arts table
    /// lists for it (Miracle Art included), through retail's ordered
    /// learned-art insert into the record's `+0x185` list and the
    /// [`crate::tactical_arts::TacticalArtsTracker`] beside it. Returns
    /// `(roster slot, arts newly learned, arts known)`; empty without the
    /// table (a disc-free build).
    pub fn cheat_learn_all_arts(&mut self) -> Vec<(u8, u8, u8)> {
        let Some(table) = self.menu.text.as_ref().and_then(|t| t.arts.clone()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for rslot in self.present_roster_slots() {
            let Some(rec) = self.party.roster.members.get_mut(rslot) else {
                continue;
            };
            if rec.hp_mp_sp().hp_max == 0 {
                continue;
            }
            let mut list = rec.displayed_skills();
            let mut learned = 0u8;
            for e in table.iter().filter(|e| e.character as usize == rslot) {
                let verdict = legaia_engine_vm::battle_action::check_and_learn_art(
                    &mut list.count,
                    &mut list.ids,
                    e.index,
                    true,
                    0,
                );
                if verdict == legaia_engine_vm::battle_action::ArtUseCheck::Learned {
                    learned += 1;
                }
                self.party.tactical_arts.mark_known(rslot as u8, e.index);
            }
            rec.set_displayed_skills(list);
            out.push((rslot as u8, learned, list.count));
        }
        out
    }

    /// Raise every held stack to 99 and add 99 of every usable consumable
    /// (an item-effect descriptor with the field or battle usability bit)
    /// through the bag's retail add helper, so nothing lands outside the
    /// active window. Key items and equipment are not added - a key item a
    /// story gate tests could open it early. Returns the stacks touched.
    pub fn cheat_max_items(&mut self) -> usize {
        let mut ids: Vec<u8> = self.party.inventory.iter().map(|(&id, _)| id).collect();
        if let Some(t) = self.tables.item_effects.as_ref() {
            ids.extend(
                (1u8..=255).filter(|&id| t.effect(id).is_some_and(|e| e.is_usable_consumable())),
            );
        }
        ids.sort_unstable();
        ids.dedup();
        let mut touched = 0;
        for id in ids.into_iter().filter(|&id| id != 0) {
            let held = self.party.inventory.get(&id).copied().unwrap_or(0);
            if held < 99
                && self
                    .cheat_give_item(id, 99 - held)
                    .is_some_and(|g| g.granted > 0)
            {
                touched += 1;
            }
        }
        touched
    }

    /// Random encounters on / off - the field roll the live loop runs
    /// ([`crate::world::WorldToggles::live_gameplay_loop`]). Scripted and
    /// boss fights are not affected: the battle side never reads the flag.
    pub fn cheat_set_random_encounters(&mut self, on: bool) {
        self.toggles.live_gameplay_loop = on;
    }

    /// Roster slots of the present party, in battle order.
    fn present_roster_slots(&self) -> Vec<usize> {
        (0..usize::from(self.party.party_count.min(3)))
            .map(|m| self.party_roster_slot(m))
            .collect()
    }

    /// Copy every present member's record HP / MP (current and max) onto its
    /// battle actor, so a record edit shows mid-fight.
    fn mirror_party_records_to_actors(&mut self) {
        for (member, rslot) in self.present_roster_slots().into_iter().enumerate() {
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

/// One player cheat - the **one list** both play hosts expose. The native
/// window maps its `--cheat-*` flags (and `F6` / `F7`) onto these, the play
/// page's Cheats panel maps its buttons onto the same values, and both run
/// them through [`World::apply_cheat`], so neither host owns a cheat the
/// other lacks or words its outcome differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerCheat {
    /// Every present member to this level (retail growth; lowering rebuilds
    /// from the New Game template).
    PartyLevel(u8),
    /// HP / MP refilled and the fallen revived, mid-battle included.
    Restore,
    /// Every present member's AP (Spirit) gauge to [`AP_GAUGE_MAX`].
    MaxAp,
    /// All Seru magic at this level (1..=9).
    GrantSeru(u8),
    /// Every art in each member's list.
    LearnAllArts,
    /// Every held stack and usable consumable to 99.
    MaxItems,
    /// The gold purse (clamped).
    Gold(i64),
    /// The casino coin bank (clamped).
    Coins(u64),
    /// Add `qty` of the item `query` names (id, `0x`-hex id or name).
    GiveItem { query: String, qty: u8 },
    /// Random encounters on (`true`) / off.
    RandomEncounters(bool),
}

/// The cheat list as `(key, label)` rows, in panel order: the native flag is
/// `--cheat-<key>`, the page's control carries the label. A host test pins
/// that each row has a control on both hosts.
pub const PLAYER_CHEAT_LIST: [(&str, &str); 10] = [
    ("level", "Level"),
    ("restore", "Restore HP/MP"),
    ("max-ap", "Max AP"),
    ("seru", "Grant Seru"),
    ("arts", "Learn all arts"),
    ("max-items", "Max items"),
    ("gold", "Gold"),
    ("coins", "Coins"),
    ("item", "Item"),
    ("encounters", "No encounters"),
];

impl PlayerCheat {
    /// The [`PLAYER_CHEAT_LIST`] key of this cheat.
    pub fn key(&self) -> &'static str {
        match self {
            PlayerCheat::PartyLevel(_) => "level",
            PlayerCheat::Restore => "restore",
            PlayerCheat::MaxAp => "max-ap",
            PlayerCheat::GrantSeru(_) => "seru",
            PlayerCheat::LearnAllArts => "arts",
            PlayerCheat::MaxItems => "max-items",
            PlayerCheat::Gold(_) => "gold",
            PlayerCheat::Coins(_) => "coins",
            PlayerCheat::GiveItem { .. } => "item",
            PlayerCheat::RandomEncounters(_) => "encounters",
        }
    }

    /// Parse an `ITEM[:QTY]` spec (QTY defaults to 1; `-` / `_` read as
    /// spaces, so `healing-leaf:5` names "Healing Leaf").
    pub fn give_item_spec(spec: &str) -> PlayerCheat {
        let (query, qty) = match spec.rsplit_once(':') {
            Some((q, n)) => match n.trim().parse::<u8>() {
                Ok(qty) => (q, qty),
                Err(_) => (spec, 1),
            },
            None => (spec, 1),
        };
        PlayerCheat::GiveItem {
            query: query.replace(['-', '_'], " "),
            qty,
        }
    }
}

impl World {
    /// Apply one [`PlayerCheat`] and return the one-line outcome both hosts
    /// show (the page's status line, the native log). `templates` is the
    /// executable's New Game party, which a lowered level is rebuilt from.
    pub fn apply_cheat(
        &mut self,
        cheat: &PlayerCheat,
        templates: Option<&StartingParty>,
    ) -> String {
        match cheat {
            PlayerCheat::PartyLevel(level) => {
                let got = self.cheat_set_party_level_with(*level, templates);
                if got.is_empty() {
                    return "no party members to level".to_string();
                }
                let parts: Vec<String> = got
                    .iter()
                    .map(|&(slot, lv)| format!("{} Lv {lv}", self.party_name(slot as usize)))
                    .collect();
                format!("Level: {}", parts.join(", "))
            }
            PlayerCheat::Restore => {
                self.cheat_restore_party();
                "Party restored.".to_string()
            }
            PlayerCheat::MaxAp => match self.cheat_max_ap() {
                0 => "no party members".to_string(),
                n => format!("AP full for {n} member(s)."),
            },
            PlayerCheat::GrantSeru(level) => {
                let got = self.cheat_grant_seru(*level);
                if got.is_empty() {
                    return "no party members".to_string();
                }
                let parts: Vec<String> = got
                    .iter()
                    .map(|g| {
                        format!(
                            "{} {} spells Lv {} (+{})",
                            self.party_name(g.slot as usize),
                            g.known,
                            g.level,
                            g.learned
                        )
                    })
                    .collect();
                format!("Seru: {}", parts.join(", "))
            }
            PlayerCheat::LearnAllArts => {
                let got = self.cheat_learn_all_arts();
                if got.is_empty() {
                    return "no arts table on this disc load".to_string();
                }
                let parts: Vec<String> = got
                    .iter()
                    .map(|&(slot, new, known)| {
                        format!("{} {known} arts (+{new})", self.party_name(slot as usize))
                    })
                    .collect();
                format!("Arts: {}", parts.join(", "))
            }
            PlayerCheat::MaxItems => {
                format!("{} item stack(s) raised to 99.", self.cheat_max_items())
            }
            PlayerCheat::Gold(g) => format!("Gold set to {}.", self.cheat_set_gold(*g)),
            PlayerCheat::Coins(c) => format!("Coins set to {}.", self.cheat_set_coins(*c)),
            PlayerCheat::GiveItem { query, qty } => {
                let pairs = self.item_name_pairs();
                let Some(id) = resolve_item(query, pairs.iter().map(|(i, n)| (*i, n.as_str())))
                else {
                    return format!("no single item matches '{query}'");
                };
                let name = pairs
                    .iter()
                    .find(|(i, _)| *i == id)
                    .map_or_else(|| format!("item {id:#04x}"), |(_, n)| n.clone());
                match self.cheat_give_item(id, *qty) {
                    Some(g) if g.granted == 0 && *qty > 0 => {
                        format!("{name}: bag full or stack at 99 (holding {})", g.held)
                    }
                    Some(g) => format!("{name} +{} (holding {})", g.granted, g.held),
                    None => format!("no single item matches '{query}'"),
                }
            }
            PlayerCheat::RandomEncounters(on) => {
                self.cheat_set_random_encounters(*on);
                if *on {
                    "Random encounters on.".to_string()
                } else {
                    "Random encounters off.".to_string()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn every_listed_cheat_key_names_one_variant() {
        let cheats = [
            PlayerCheat::PartyLevel(5),
            PlayerCheat::Restore,
            PlayerCheat::MaxAp,
            PlayerCheat::GrantSeru(9),
            PlayerCheat::LearnAllArts,
            PlayerCheat::MaxItems,
            PlayerCheat::Gold(1),
            PlayerCheat::Coins(1),
            PlayerCheat::give_item_spec("healing-leaf:5"),
            PlayerCheat::RandomEncounters(false),
        ];
        let keys: Vec<&str> = cheats.iter().map(PlayerCheat::key).collect();
        let listed: Vec<&str> = PLAYER_CHEAT_LIST.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, listed);
        assert_eq!(
            PlayerCheat::give_item_spec("healing-leaf:5"),
            PlayerCheat::GiveItem {
                query: "healing leaf".into(),
                qty: 5
            }
        );
    }

    #[test]
    fn apply_cheat_reports_the_shared_outcome_line() {
        let mut w = world_with_trio();
        assert_eq!(
            w.apply_cheat(&PlayerCheat::Gold(-5), None),
            "Gold set to 0."
        );
        assert_eq!(
            w.apply_cheat(&PlayerCheat::MaxAp, None),
            "AP full for 3 member(s)."
        );
        let mut hurt = w.party.roster.members[1].hp_mp_sp();
        hurt.hp_cur = 1;
        w.party.roster.members[1].set_hp_mp_sp(hurt);
        assert_eq!(
            w.apply_cheat(&PlayerCheat::Restore, None),
            "Party restored."
        );
        assert_eq!(w.party.roster.members[1].hp_mp_sp().hp_cur, 150);
        w.apply_cheat(&PlayerCheat::RandomEncounters(false), None);
        assert!(!w.toggles.live_gameplay_loop);
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
    fn level_survives_the_ability_bitfield_rebuild() {
        // The stat refresh zeroes and rebuilds `+0xF4..+0x103` (retail
        // `FUN_80042558`'s `sw zero,0x100(s0)`); the level and the AP base
        // derived from it must survive that pass.
        let mut w = world_with_trio();
        assert_eq!(w.cheat_set_level(0, 40), Some(40));
        w.party.roster.members[0].set_ability_bits([0; legaia_save::ABILITY_BITS_LEN]);
        w.seed_party_battle_stats();
        assert_eq!(w.party.roster.members[0].level(), 40);
        assert_eq!(
            w.battle.ap_gauges[0].base_ap,
            crate::ap_gauge::ap_base_for_level(40)
        );
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

    fn trio_templates() -> StartingParty {
        StartingParty::from_members(vec![tpl("Vahn", 180), tpl("Noa", 150), tpl("Gala", 220)])
    }

    #[test]
    fn lowering_rebuilds_from_the_template_plus_growth() {
        let mut w = world_with_trio();
        w.party.level_up_tracker.stat_gains = [crate::levelup::StatGain {
            atk: 2,
            ..crate::levelup::StatGain::hp_mp(10, 5)
        }; MAX_PARTY];
        let t = trio_templates();
        w.cheat_set_party_level(30);
        // A twin raised straight to 10 is the reference a rebuild must match.
        let mut twin = world_with_trio();
        twin.party.level_up_tracker.stat_gains = w.party.level_up_tracker.stat_gains;
        twin.cheat_set_party_level(10);
        // Without templates a level never goes down.
        assert_eq!(w.cheat_set_party_level(10), vec![(0, 30), (1, 30), (2, 30)]);
        assert_eq!(
            w.cheat_set_party_level_with(10, Some(&t)),
            vec![(0, 10), (1, 10), (2, 10)]
        );
        for slot in 0..3 {
            let (a, b) = (
                &w.party.roster.members[slot],
                &twin.party.roster.members[slot],
            );
            assert_eq!(a.level(), 10);
            assert_eq!(a.cumulative_xp(), b.cumulative_xp());
            assert_eq!(a.next_level_xp(), b.next_level_xp());
            assert_eq!(a.record_stats(), b.record_stats());
            assert_eq!(a.live_stats(), b.live_stats());
            assert_eq!(a.hp_mp_sp().hp_max, b.hp_mp_sp().hp_max);
            assert!(a.hp_mp_sp().hp_cur <= a.hp_mp_sp().hp_max);
            assert_eq!(w.party.level_up_tracker.level[slot], 10);
        }
        assert_eq!(
            w.actors[0].battle.max_hp,
            w.party.roster.members[0].hp_mp_sp().hp_max
        );
    }

    #[test]
    fn max_ap_fills_record_and_battle_gauge() {
        let mut w = world_with_trio();
        assert_eq!(w.cheat_max_ap(), 3);
        for slot in 0..3u8 {
            assert_eq!(
                w.party.roster.members[slot as usize].hp_mp_sp().sp_cur,
                AP_GAUGE_MAX
            );
            assert!(w.spirit_gauge_full(slot));
        }
    }

    #[test]
    fn grant_seru_learns_every_spell_at_the_level() {
        let mut w = world_with_trio();
        let got = w.cheat_grant_seru(12);
        assert_eq!(got.len(), 3);
        for g in &got {
            // 21 Seru + the high-block summons (Vahn: Evil Seru, four
            // Sim-Seru and Meta; Noa / Gala: their own Ra-Seru).
            let n = if g.slot == 0 { 27 } else { 22 };
            assert_eq!((g.learned, g.known, g.level), (n, n, SERU_LEVEL_MAX));
            let rec = &w.party.roster.members[g.slot as usize];
            let list = rec.spell_list();
            assert_eq!(list.ids[0], 0x81, "the block reads ascending");
            let n = usize::from(n);
            assert!(list.ids[..n].contains(&ra_seru_spell_for(g.slot as usize).unwrap()));
            assert!(list.levels[..n].iter().all(|&l| l == SERU_LEVEL_MAX));
            assert!(w.seru.log.learned_spells(g.slot).contains(&0x95));
        }
        // Noa never gets Meta or the Sim-Seru.
        let noa = w.party.roster.members[1].spell_list().ids;
        assert!(!noa.contains(&0x9E) && !noa.contains(&0x9A));
        // Idempotent: a second grant learns nothing new and can lower levels.
        let again = w.cheat_grant_seru(3);
        assert!(again.iter().all(|g| g.learned == 0 && g.level == 3));
    }

    #[test]
    fn learn_all_arts_fills_the_ordered_list() {
        use legaia_art::arts_table::ArtTableEntry;
        let mut w = world_with_trio();
        let row = |c, i: u8| ArtTableEntry {
            character: c,
            index: i,
            name: format!("art {i}"),
            ap: 0,
            commands: Vec::new(),
            is_miracle: i == 0,
        };
        let text = crate::pause_screens::MenuTextTables {
            arts: Some(vec![
                row(legaia_art::Character::Vahn, 5),
                row(legaia_art::Character::Vahn, 0),
                row(legaia_art::Character::Vahn, 2),
                row(legaia_art::Character::Noa, 1),
            ]),
            ..Default::default()
        };
        w.menu.text = Some(text);
        let got = w.cheat_learn_all_arts();
        assert_eq!(got, vec![(0, 3, 3), (1, 1, 1), (2, 0, 0)]);
        let list = w.party.roster.members[0].displayed_skills();
        assert_eq!(&list.ids[..3], &[0, 2, 5]);
        assert!(w.party.tactical_arts.is_learned(0, 5));
        assert_eq!(
            w.cheat_learn_all_arts(),
            vec![(0, 0, 3), (1, 0, 1), (2, 0, 0)]
        );
    }

    #[test]
    fn max_items_tops_up_held_stacks() {
        let mut w = world_with_trio();
        w.cheat_give_item(0x77, 3);
        w.cheat_give_item(0x78, 99);
        assert_eq!(w.cheat_max_items(), 1);
        assert_eq!(w.party.inventory.get(&0x77), Some(&99));
        assert_eq!(w.party.inventory.get(&0x78), Some(&99));
    }

    #[test]
    fn random_encounters_toggle_the_field_roll() {
        let mut w = world_with_trio();
        w.cheat_set_random_encounters(true);
        assert!(w.toggles.live_gameplay_loop);
        w.cheat_set_random_encounters(false);
        assert!(!w.toggles.live_gameplay_loop);
    }
}

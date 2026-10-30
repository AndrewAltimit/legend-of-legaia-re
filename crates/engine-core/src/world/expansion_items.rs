//! Pack items on the `World`: grants and counts by [`ItemRef`], the stat,
//! passive and look hooks worn pack gear reaches the game through, the
//! per-fight aliases that let a pack item be a drop, a steal or an Item
//! command row, and the item screen (pack shops and the pack bag).
//!
//! Every entry point is the identity with no pack active (or a pack with no
//! `items.toml`): nothing is compiled, no alias is lent, the stat hooks add
//! nothing, and a retail item id passes straight through to the retail bag.
//! The kernels are `World`-free in `legaia_engine_expansion::{items,
//! item_screen}`.

use std::sync::Arc;

use legaia_engine_expansion::item_screen::{
    ItemScreen, ItemScreenHost, ItemSeat, Member, RetailInfo, ScreenModel, ScreenStep, StatLine,
};
use legaia_engine_expansion::items::{
    self as xi, CompiledItems, ItemKey, ItemProgress, PackItem, WornBonus,
};
use legaia_engine_menus::items::{ItemEffect, ItemEntry, ItemOutcome};
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::items::{ItemEffectDef, ItemRef, ShopDef, Status};
use legaia_expansion_format::quest::Prerequisites;

use super::World;
use super::expansion_modes::GOLD_CEILING;

/// The engine handler a pack consumable runs, and whether it covers the
/// party. `AsItem` resolves to the retail item's own catalog entry.
fn engine_effect(world: &World, item: &PackItem) -> Option<(ItemEffect, bool)> {
    let effect = match xi::effect_of(&item.def)? {
        ItemEffectDef::HealHp { full: true, .. } => ItemEffect::HealAll,
        ItemEffectDef::HealHp { amount, .. } => ItemEffect::Heal { amount: *amount },
        ItemEffectDef::HealMp { full: true, .. } => ItemEffect::HealMpAll,
        ItemEffectDef::HealMp { amount, .. } => ItemEffect::HealMp { amount: *amount },
        ItemEffectDef::Revive { percent } => ItemEffect::Revive {
            factor: (u32::from(*percent) * 256 / 100).min(255) as u8,
        },
        ItemEffectDef::Cure { status } => ItemEffect::Cure {
            kind: status_kind(*status),
        },
        ItemEffectDef::CureAll => ItemEffect::CureAll,
        ItemEffectDef::Damage { amount } => ItemEffect::Damage { amount: *amount },
        ItemEffectDef::AsItem { item: id } => {
            let e = world.tables.item_catalog.get(*id)?;
            return Some((e.effect, world.tables.item_catalog.is_all_party(*id)));
        }
    };
    Some((effect, item.def.all_party))
}

fn status_kind(s: Status) -> legaia_engine_battle_vm::status_effects::StatusKind {
    use legaia_engine_battle_vm::status_effects::StatusKind as K;
    match s {
        Status::Venom => K::Venom,
        Status::Toxic => K::Toxic,
        Status::Numb => K::Numb,
        Status::Sleep => K::Sleep,
        Status::Confuse => K::Confuse,
        Status::Rot => K::Rot,
        Status::Curse => K::Curse,
        Status::Stone => K::Stone,
    }
}

/// The percent stat boosts (`0x00..=0x0C`) a pack's worn passives add on
/// top of what the retail equipment already grants: the same base-stat
/// divisors the aggregator applies (`FUN_80042558`), once per index - an
/// index the retail gear already sets adds nothing more.
pub(in crate::world) fn fold_pack_percent_boosts(
    stats: &mut legaia_engine_battle::battle_stats::BattleStats,
    record: &legaia_engine_battle::battle_stats::StatRecord,
    pack: &[u32; 4],
    retail: &[u32; 4],
) {
    use legaia_game_tables::accessory_passive::{
        BoostedStat, NO_PASSIVE, bit_location, stat_boosts,
    };
    for idx in 0..NO_PASSIVE {
        let (w, mask) = bit_location(idx);
        if pack[w] & mask == 0 || retail[w] & mask != 0 {
            continue;
        }
        for &(stat, div) in stat_boosts(idx) {
            let div = div as u16;
            let add = |v: &mut u16, base: u16| *v = v.saturating_add(base / div).min(999);
            match stat {
                BoostedStat::Attack => add(&mut stats.atk, record.base_attack),
                BoostedStat::DefenseUp => add(&mut stats.udf, record.base_udf),
                BoostedStat::DefenseDown => add(&mut stats.ldf, record.base_ldf),
                BoostedStat::Speed => add(&mut stats.spd, record.base_spd),
                BoostedStat::Intelligence => add(&mut stats.int, record.base_int),
                BoostedStat::Agility => {
                    add(&mut stats.acc, record.base_accuracy);
                    add(&mut stats.eva, record.base_evasion);
                }
                BoostedStat::MaxHp | BoostedStat::MaxMp => {}
            }
        }
    }
}

/// The catalog entry an alias carries: a consumable's handler, or a name
/// only (`KeyItem`, offered by no menu) for a dropped or stolen non-
/// consumable.
fn alias_entry(world: &World, id: u8, item: &PackItem) -> ItemEntry {
    let name = xi::intern(&item.def.name);
    match engine_effect(world, item) {
        Some((effect, _)) => ItemEntry {
            id,
            name,
            effect,
            usable_in_battle: item.def.battle_usable(),
            usable_in_field: item.def.field_usable(),
        },
        None => ItemEntry {
            id,
            name,
            effect: ItemEffect::KeyItem,
            usable_in_battle: false,
            usable_in_field: false,
        },
    }
}

impl World {
    /// The active pack's compiled items; `None` with no pack or no items.
    pub fn pack_items(&self) -> Option<Arc<CompiledItems>> {
        self.expansion.items.compiled.clone()
    }

    /// Compile the active pack's `items.toml` (activation), and fold any
    /// pack gear a retail item has since covered back into the bag.
    pub(in crate::world) fn compile_expansion_items(&mut self) {
        self.expansion.items.compiled = self
            .expansion
            .pack
            .as_deref()
            .filter(|p| !p.items.items.is_empty())
            .map(|p| Arc::new(CompiledItems::compile(p)));
        self.reconcile_pack_gear();
    }

    /// The pack items the player holds and wears.
    pub fn pack_item_progress(&self) -> &ItemProgress {
        &self.expansion.quests.progress.items
    }

    /// Resolve `r` against the active pack: a retail id, or a pack item the
    /// pack defines. `None` for an unknown pack key or with no pack.
    pub fn resolve_item_ref(&self, r: &ItemRef) -> Option<ItemKey> {
        let key = match self.expansion.pack.as_deref() {
            Some(p) => ItemKey::resolve(&p.manifest.id, r)?,
            None => ItemKey::Retail(r.retail_id().filter(|&id| id != 0)?),
        };
        if let ItemKey::Pack(k) = &key
            && self.pack_items().is_none_or(|c| c.get(k).is_none())
        {
            return None;
        }
        Some(key)
    }

    /// How many of `r` the bag holds (the retail bag for a retail id, the
    /// pack bag for a pack item).
    pub fn held_item_count(&self, r: &ItemRef) -> u32 {
        match self.resolve_item_ref(r) {
            Some(ItemKey::Retail(id)) => {
                u32::from(self.party.inventory.get(&id).copied().unwrap_or(0))
            }
            Some(ItemKey::Pack(k)) => u32::from(self.pack_item_progress().count(&k)),
            None => 0,
        }
    }

    /// Add `count` of `r`: a retail id to the retail bag, a pack item to the
    /// pack bag (with a "Got X" toast). Returns whether anything went in.
    pub fn grant_item_ref(&mut self, r: &ItemRef, count: u8) -> bool {
        match self.resolve_item_ref(r) {
            Some(ItemKey::Retail(id)) => self.party.inventory.add(id, count),
            Some(ItemKey::Pack(k)) => self.grant_pack_item(&k, count),
            None => {
                log::warn!("expansion: item {r} names nothing this pack defines");
                false
            }
        }
    }

    /// Take up to `count` of `r` from its bag.
    pub fn take_item_ref(&mut self, r: &ItemRef, count: u32) {
        let n = count.min(u32::from(u8::MAX)) as u8;
        match self.resolve_item_ref(r) {
            Some(ItemKey::Retail(id)) => {
                let held = self.party.inventory.get(&id).copied().unwrap_or(0);
                let left = held.saturating_sub(n);
                if left == 0 {
                    self.party.inventory.remove(&id);
                } else {
                    self.party.inventory.insert(id, left);
                }
            }
            Some(ItemKey::Pack(k)) => {
                self.expansion.quests.progress.items.take(&k, n);
            }
            None => {}
        }
    }

    fn grant_pack_item(&mut self, key: &str, count: u8) -> bool {
        let Some(name) = self
            .pack_items()
            .and_then(|c| c.get(key).map(|i| i.def.name.clone()))
        else {
            return false;
        };
        let put = self.expansion.quests.progress.items.add(key, count);
        if put > 0 {
            let line = if put == 1 {
                format!("Got {name}")
            } else {
                format!("Got {name} x{put}")
            };
            self.expansion.toasts.push(line.clone());
            self.expansion.items.notices.push(line);
        }
        put > 0
    }

    /// The bonus roster slot `roster`'s worn pack gear adds; `None` with no
    /// pack items or nothing worn.
    pub fn pack_worn_bonus(&self, roster: u8) -> Option<WornBonus> {
        self.pack_worn_bonus_with(roster, None)
    }

    fn pack_worn_bonus_with(
        &self,
        roster: u8,
        swap: Option<(u8, Option<&str>)>,
    ) -> Option<WornBonus> {
        let c = self.expansion.items.compiled.as_deref()?;
        let p = self.pack_item_progress();
        if swap.is_none() && p.worn_by(roster).next().is_none() {
            return None;
        }
        Some(c.worn_bonus(p, roster, swap))
    }

    /// The name of the pack item roster slot `roster` wears in equipment
    /// byte `index` (the record's own byte stays `0` under it), for the
    /// retail screens' slot rows to show instead of an empty slot. `None`
    /// with no pack gear there.
    pub fn pack_worn_item_name(&self, roster: u8, index: u8) -> Option<String> {
        let key = self.pack_item_progress().worn.get(&(roster, index))?;
        let name = self
            .pack_items()
            .and_then(|c| c.get(key).map(|i| i.name().to_string()))
            .unwrap_or_else(|| key.clone());
        Some(name)
    }

    /// Return to the pack bag any worn pack item whose retail slot a retail
    /// item now fills (the retail equip screen, Best Equipment, a script),
    /// so a slot never counts twice.
    pub fn reconcile_pack_gear(&mut self) {
        if self.expansion.quests.progress.items.worn.is_empty() {
            return;
        }
        let roster = &self.party.roster.members;
        let slots = |r: u8| roster.get(usize::from(r)).map(|m| m.equipment().slots);
        let back = self.expansion.quests.progress.items.reconcile(slots);
        if !back.is_empty() {
            log::info!("expansion: pack gear {back:?} returned to the bag under retail gear");
        }
    }

    /// Roster slot `roster`'s eight equipment bytes with each empty slot a
    /// pack item fills showing that item's borrowed look ([`PackItem::icon`]):
    /// what the battle assembler, the swing costs and the command chips read
    /// so a pack weapon draws and swings as its look. The stat fold reads the
    /// record itself. The record's bytes with no pack gear worn.
    pub fn member_look_equipment(&self, roster: usize) -> [u8; 8] {
        let mut slots = self
            .party
            .roster
            .members
            .get(roster)
            .map(|m| m.equipment().slots)
            .unwrap_or_default();
        let Some(c) = self.expansion.items.compiled.as_deref() else {
            return slots;
        };
        let Ok(r) = u8::try_from(roster) else {
            return slots;
        };
        for (i, key) in self.pack_item_progress().worn_by(r) {
            if let Some(s) = slots.get_mut(usize::from(i))
                && *s == 0
                && let Some(look) = c.get(key).and_then(PackItem::icon)
            {
                *s = look;
            }
        }
        slots
    }

    /// Wear one pack `key` from the pack bag on roster slot `roster` in
    /// equipment byte `index`. A retail item in that byte goes to the retail
    /// bag and a pack item to the pack bag; the record's byte is left `0`.
    pub fn equip_pack_item(&mut self, key: &str, roster: u8, index: u8) -> bool {
        let Some(item) = self.pack_items().and_then(|c| c.get(key).cloned()) else {
            return false;
        };
        if !item.wearable_by(roster)
            || !xi::equip_indices(item.def.kind, roster).contains(&index)
            || self.pack_item_progress().count(key) == 0
        {
            return false;
        }
        let Some(member) = self.party.roster.members.get_mut(usize::from(roster)) else {
            return false;
        };
        let mut eq = member.equipment();
        let old = eq.slots[usize::from(index)];
        if old != 0 {
            if !self.party.inventory.add(old, 1) {
                return false;
            }
            eq.slots[usize::from(index)] = 0;
            if let Some(m) = self.party.roster.members.get_mut(usize::from(roster)) {
                m.set_equipment(eq);
            }
        }
        if !self
            .expansion
            .quests
            .progress
            .items
            .equip(key, roster, index)
        {
            return false;
        }
        self.refresh_party_ability_bits();
        true
    }

    /// Take the pack item off roster slot `roster`'s byte `index`.
    pub fn unequip_pack_item(&mut self, roster: u8, index: u8) -> bool {
        let done = self
            .expansion
            .quests
            .progress
            .items
            .unequip(roster, index)
            .is_some();
        if done {
            self.refresh_party_ability_bits();
        }
        done
    }

    /// The equipment-fed stats of roster slot `roster` the pack screen's
    /// compare shows: the same base and retail fold as
    /// [`Self::seed_party_battle_stats`], plus the worn pack gear, with
    /// `preview` (byte, pack key or none) in place of what that byte holds.
    pub fn pack_stat_line(&self, roster: u8, preview: Option<(u8, Option<&str>)>) -> StatLine {
        let Some(rec) = self.party.roster.members.get(usize::from(roster)) else {
            return StatLine::default();
        };
        let live = rec.live_stats();
        let recs = rec.record_stats();
        let (atk, udf, ldf, spd, int) =
            if !self.tables.accessory_passives.is_empty() && recs.atk != 0 {
                (recs.atk, recs.udf, recs.ldf, recs.spd, recs.int)
            } else {
                (live.atk, live.udf, live.ldf, live.spd, live.int)
            };
        let mut equip = rec.equipment().slots;
        if let Some((i, _)) = preview
            && let Some(s) = equip.get_mut(usize::from(i))
        {
            *s = 0;
        }
        let record = legaia_engine_battle::battle_stats::StatRecord {
            base_attack: atk,
            base_udf: udf,
            base_ldf: ldf,
            base_accuracy: 0,
            base_evasion: 0,
            base_spd: spd,
            base_int: int,
            equip,
        };
        let mut s = legaia_engine_battle::battle_stats::compute_battle_stats_with_passives(
            &record,
            &self.tables.equipment_table,
            &self.tables.accessory_passives,
            &[],
            &legaia_engine_battle::battle_stats::StatusModifiers::default(),
        );
        let b = self
            .pack_worn_bonus_with(roster, preview)
            .unwrap_or_default();
        if !self.tables.accessory_passives.is_empty() {
            let retail = self
                .tables
                .accessory_passives
                .bits_for_equipment(&record.equip);
            fold_pack_percent_boosts(&mut s, &record, &b.ability_words, &retail);
        }
        let add = |v: u16, d: i16| (i32::from(v) + i32::from(d)).clamp(0, 999) as u16;
        StatLine {
            atk: add(s.atk, b.total.atk),
            udf: add(s.udf, b.total.udf),
            ldf: add(s.ldf, b.total.ldf),
            spd: add(s.spd, b.total.spd),
            int: add(s.int, b.total.int),
        }
    }

    /// Use one pack consumable `key` on roster slot `roster` from the field
    /// (every member for an all-party item), through the retail item
    /// handlers. Spends one copy when anything happened.
    pub fn use_pack_item(&mut self, key: &str, roster: u8) -> Option<ItemOutcome> {
        let item = self.pack_items()?.get(key).cloned()?;
        if !item.def.field_usable() || self.pack_item_progress().count(key) == 0 {
            return None;
        }
        let (_, all) = engine_effect(self, &item)?;
        let targets: Vec<u8> = if all {
            (0..self.party.roster.members.len().min(3) as u8).collect()
        } else {
            vec![roster]
        };
        let mut first = None;
        for t in targets {
            let o = self.with_pack_alias(&item, |w, id| w.use_item(id, t));
            if o != ItemOutcome::NoEffect {
                first.get_or_insert(o);
            }
        }
        if first.is_some() {
            self.expansion.quests.progress.items.take(key, 1);
        }
        first
    }

    /// Run `f` with `item` reachable under a `u8`: a retail `as_item` id
    /// directly, else an alias this fight already lent it, else a scratch
    /// alias installed for the call.
    fn with_pack_alias<T>(&mut self, item: &PackItem, f: impl FnOnce(&mut Self, u8) -> T) -> T {
        if let Some(ItemEffectDef::AsItem { item: id }) = xi::effect_of(&item.def) {
            return f(self, *id);
        }
        if let Some(id) = self.expansion.items.aliases.id_of(&item.key) {
            return f(self, id);
        }
        let catalog = &self.tables.item_catalog;
        let id = xi::ALIAS_POOL
            .into_iter()
            .find(|&id| catalog.get(id).is_none())
            .unwrap_or(xi::ALIAS_POOL[0]);
        let saved = self.tables.item_catalog.get(id).copied();
        let was_all = self.tables.item_catalog.is_all_party(id);
        let entry = alias_entry(self, id, item);
        self.tables.item_catalog.insert(entry);
        let out = f(self, id);
        match saved {
            Some(e) => self.tables.item_catalog.insert(e),
            None => {
                self.tables.item_catalog.remove(id);
            }
        }
        self.tables.item_catalog.set_all_party(id, was_all);
        out
    }

    /// Lend this fight's aliases: each pack item a seated monster drops or
    /// steals, then each battle-usable pack consumable held, until the pool
    /// runs out. Installs the alias catalog entries the Item command, the
    /// use path and the spoils names read. Runs from the battle prepare,
    /// after the catalog swap.
    pub(in crate::world) fn expansion_lend_item_aliases(
        &mut self,
        drops: &[(u16, u8, String)],
        steals: &[(u16, u8, String)],
    ) {
        self.expansion_release_item_aliases();
        let Some(c) = self.pack_items() else {
            return;
        };
        let held: Vec<String> = c
            .battle_consumables()
            .filter(|i| self.pack_item_progress().count(&i.key) > 0)
            .map(|i| i.key.clone())
            .collect();
        let wanted = drops
            .iter()
            .map(|(_, _, k)| k)
            .chain(steals.iter().map(|(_, _, k)| k))
            .chain(held.iter());
        for key in wanted {
            if c.get(key).is_none() {
                continue;
            }
            let catalog = &self.tables.item_catalog;
            let inv = &self.party.inventory;
            let taken = |id: u8| catalog.get(id).is_some() || inv.get(&id).is_some();
            if self.expansion.items.aliases.lend(key, taken).is_none() {
                log::info!("expansion: no item alias left for `{key}` this fight");
                break;
            }
            let id = self.expansion.items.aliases.id_of(key).unwrap_or(0);
            let entry = alias_entry(self, id, &c.items[key]);
            self.tables.item_catalog.insert(entry);
            let all = engine_effect(self, &c.items[key]).is_some_and(|(_, a)| a);
            self.tables.item_catalog.set_all_party(id, all);
        }
        for (monster, chance, key) in drops {
            if let Some(id) = self.expansion.items.aliases.id_of(key)
                && let Some(def) = self.tables.monster_catalog.by_id.get_mut(monster)
            {
                def.drop_item = Some(id);
                def.drop_chance_pct = *chance;
            }
        }
        for (monster, chance, key) in steals {
            if let Some(id) = self.expansion.items.aliases.id_of(key) {
                self.expansion.battle.steals.insert(*monster, (*chance, id));
            }
        }
    }

    /// Return every alias and drop its catalog entry. Runs at the next
    /// fight's prepare and at deactivation, not at the battle's end: the
    /// results window names a drop after the fight closes.
    pub(in crate::world) fn expansion_release_item_aliases(&mut self) {
        let ids: Vec<u8> = self
            .expansion
            .items
            .aliases
            .iter()
            .map(|(id, _)| id)
            .collect();
        for id in ids {
            self.tables.item_catalog.remove(id);
            self.tables.item_catalog.set_all_party(id, false);
        }
        self.expansion.items.aliases.clear();
    }

    /// The pack item an id stands for this fight.
    pub fn pack_item_alias(&self, id: u8) -> Option<&str> {
        self.expansion.items.aliases.key(id)
    }

    /// The retail item a pack `as_item` consumable's alias runs.
    pub(in crate::world) fn pack_alias_as_item(&self, id: u8) -> Option<u8> {
        let key = self.pack_item_alias(id)?;
        match xi::effect_of(&self.expansion.items.compiled.as_deref()?.get(key)?.def)? {
            ItemEffectDef::AsItem { item } => Some(*item),
            _ => None,
        }
    }

    /// How many of item id `id` the party holds: the pack bag for an alias,
    /// the retail bag otherwise.
    pub fn held_count_by_id(&self, id: u8) -> u8 {
        match self.pack_item_alias(id) {
            Some(k) => self.pack_item_progress().count(k),
            None => self.party.inventory.get(&id).copied().unwrap_or(0),
        }
    }

    /// Add `n` of item id `id`: an alias lands in the pack bag, anything
    /// else in the retail bag.
    pub fn grant_item_id(&mut self, id: u8, n: u8) -> bool {
        match self.pack_item_alias(id).map(str::to_string) {
            Some(k) => self.grant_pack_item(&k, n),
            None => self.party.inventory.add(id, n),
        }
    }

    /// Spend one copy of an alias from the pack bag. `false` for an id that
    /// is not an alias (the caller spends from the retail bag).
    pub(in crate::world) fn consume_pack_alias(&mut self, id: u8) -> bool {
        let Some(k) = self.pack_item_alias(id).map(str::to_string) else {
            return false;
        };
        self.expansion.quests.progress.items.take(&k, 1);
        true
    }

    /// The battle-usable pack consumables held, as their aliases, for the
    /// Item command's list.
    pub(in crate::world) fn pack_battle_item_ids(&self) -> Vec<u8> {
        let Some(c) = self.expansion.items.compiled.as_deref() else {
            return Vec::new();
        };
        self.expansion
            .items
            .aliases
            .iter()
            .filter(|(_, k)| {
                c.get(k).is_some_and(|i| i.def.battle_usable())
                    && self.pack_item_progress().count(k) > 0
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// The pack item's description for an alias id (the battle item
    /// window's info line).
    pub fn pack_alias_description(&self, id: u8) -> Option<String> {
        let key = self.pack_item_alias(id)?;
        let c = self.expansion.items.compiled.as_deref()?;
        Some(c.get(key)?.def.description.replace('|', " "))
    }

    // ----- the item screen -----

    fn open_item_screen(&mut self, screen: ItemScreen) -> bool {
        if self.expansion.items.compiled.is_none()
            || self.mode == SceneMode::Expansion
            || self.mode == SceneMode::Battle
        {
            return false;
        }
        self.reconcile_pack_gear();
        self.expansion.items.seat = Some(Box::new(ItemSeat {
            return_mode: self.mode,
            screen,
        }));
        self.mode = SceneMode::Expansion;
        true
    }

    /// Open the pack shop `shop` (`""` = the first): a map's `shop` trigger
    /// or a shop NPC. `false` for an unknown shop or with no pack items.
    pub fn open_item_shop(&mut self, shop: &str) -> bool {
        let Some(id) = self
            .expansion
            .pack
            .as_deref()
            .and_then(|p| p.items.shop(shop))
            .map(|s| s.id.clone())
        else {
            return false;
        };
        self.open_item_screen(ItemScreen::shop(&id))
    }

    /// Whether the active pack has a shop `id`.
    pub fn has_item_shop(&self, id: &str) -> bool {
        self.expansion
            .pack
            .as_deref()
            .is_some_and(|p| p.items.shops.iter().any(|s| s.id == id))
    }

    /// Open the pack bag (the Journal's Items page), on the Gear tab when
    /// `gear`.
    pub fn open_item_bag(&mut self, gear: bool) -> bool {
        self.open_item_screen(ItemScreen::bag(gear))
    }

    /// Whether the item screen owns the frame.
    pub fn item_screen_open(&self) -> bool {
        self.mode == SceneMode::Expansion && self.expansion.items.seat.is_some()
    }

    /// Close the item screen, restoring the mode it opened over.
    pub fn close_item_screen(&mut self) {
        if let Some(seat) = self.expansion.items.seat.take()
            && self.mode == SceneMode::Expansion
        {
            self.mode = seat.return_mode;
        }
    }

    /// One frame of the item screen, off this frame's pad.
    pub(in crate::world) fn tick_item_seat(&mut self) {
        let Some(mut seat) = self.expansion.items.seat.take() else {
            return;
        };
        let (pad, prev) = (self.input.pad(), self.input.pad_prev());
        let step = seat.screen.step(pad, prev, &mut WorldItems(self));
        match step {
            ScreenStep::Stay => self.expansion.items.seat = Some(seat),
            ScreenStep::Leave => self.mode = seat.return_mode,
        }
    }

    /// What the item screen draws this frame; `None` unless it owns the
    /// frame.
    pub fn item_screen_model(&mut self) -> Option<ScreenModel> {
        if !self.item_screen_open() {
            return None;
        }
        let seat = self.expansion.items.seat.take()?;
        let model = seat.screen.model(&WorldItems(self));
        self.expansion.items.seat = Some(seat);
        Some(model)
    }

    /// The item screen's model without a mutable borrow, for a draw that
    /// holds the world shared.
    pub fn item_screen_view(&self) -> Option<ScreenModel> {
        if !self.item_screen_open() {
            return None;
        }
        let seat = self.expansion.items.seat.as_deref()?;
        Some(seat.screen.model(&WorldItemsRef(self)))
    }
}

/// The `World` as the item screen reads and writes it.
struct WorldItems<'a>(&'a mut World);
/// The `World` as the item screen's draw reads it.
struct WorldItemsRef<'a>(&'a World);

fn members(w: &World) -> Vec<Member> {
    w.present_party_list()
        .into_iter()
        .filter_map(|slot| {
            let m = w.party.roster.members.get(usize::from(slot))?;
            let h = m.hp_mp_sp();
            Some(Member {
                roster: slot,
                name: m.name().trim().to_string(),
                hp: h.hp_cur,
                hp_max: h.hp_max,
                mp: h.mp_cur,
                mp_max: h.mp_max,
            })
        })
        .collect()
}

fn retail_info(w: &World, id: u8) -> RetailInfo {
    let text = w.menu.text.as_ref();
    RetailInfo {
        name: text
            .and_then(|t| t.item_name(id))
            .filter(|n| !n.trim().is_empty())
            .map(str::to_string)
            .or_else(|| w.tables.item_catalog.get(id).map(|e| e.name.to_string()))
            .unwrap_or_else(|| format!("Item {id:02X}")),
        description: text
            .and_then(|t| t.item_desc(id))
            .unwrap_or_default()
            .to_string(),
        price: w
            .shops
            .item_shop_data
            .as_ref()
            .map_or(0, |p| u32::from(p.price(id))),
        held: w.party.inventory.get(&id).copied().unwrap_or(0),
    }
}

fn worn_name(w: &World, roster: u8, index: u8) -> String {
    if let Some(name) = w
        .pack_item_progress()
        .worn_at(roster, index)
        .and_then(|k| w.pack_items()?.get(k).map(|i| i.def.name.clone()))
    {
        return name;
    }
    let id = w
        .party
        .roster
        .members
        .get(usize::from(roster))
        .map_or(0, |m| m.equipment().slots[usize::from(index.min(7))]);
    if id == 0 {
        String::new()
    } else {
        retail_info(w, id).name
    }
}

fn shop(w: &World, id: &str) -> Option<ShopDef> {
    w.expansion.pack.as_deref()?.items.shop(id).cloned()
}

impl WorldItems<'_> {
    fn w(&self) -> &World {
        self.0
    }
}

impl WorldItemsRef<'_> {
    fn w(&self) -> &World {
        self.0
    }
}

macro_rules! read_half {
    () => {
        fn gold(&self) -> u32 {
            self.w().party.money.max(0) as u32
        }
        fn members(&self) -> Vec<Member> {
            members(self.w())
        }
        fn retail_info(&self, id: u8) -> RetailInfo {
            retail_info(self.w(), id)
        }
        fn prerequisites_hold(&self, p: &Prerequisites) -> bool {
            self.w().expansion_prerequisites_hold(p)
        }
        fn compiled(&self) -> Option<Arc<CompiledItems>> {
            self.w().pack_items()
        }
        fn progress(&self) -> &ItemProgress {
            self.w().pack_item_progress()
        }
        fn shop(&self, id: &str) -> Option<ShopDef> {
            shop(self.w(), id)
        }
        fn stats(&self, roster: u8, preview: Option<(u8, Option<&str>)>) -> StatLine {
            self.w().pack_stat_line(roster, preview)
        }
        fn worn_name(&self, roster: u8, index: u8) -> String {
            worn_name(self.w(), roster, index)
        }
    };
}

impl ItemScreenHost for WorldItems<'_> {
    read_half!();

    fn buy(&mut self, item: &ItemKey, qty: u8, cost: u32) -> bool {
        let w = &mut *self.0;
        if i64::from(w.party.money) < i64::from(cost) {
            return false;
        }
        let ok = match item {
            ItemKey::Retail(id) => w.party.inventory.add(*id, qty),
            ItemKey::Pack(k) => w.expansion.quests.progress.items.add(k, qty) > 0,
        };
        if ok {
            w.party.money =
                (i64::from(w.party.money) - i64::from(cost)).clamp(0, GOLD_CEILING) as i32;
        }
        ok
    }

    fn sell(&mut self, key: &str, qty: u8, gain: u32) -> bool {
        let w = &mut *self.0;
        if w.expansion.quests.progress.items.take(key, qty) == 0 {
            return false;
        }
        w.party.money = (i64::from(w.party.money) + i64::from(gain)).clamp(0, GOLD_CEILING) as i32;
        true
    }

    fn use_item(&mut self, key: &str, roster: u8) -> Option<String> {
        let w = &mut *self.0;
        let name = w
            .party
            .roster
            .members
            .get(usize::from(roster))
            .map(|m| m.name().trim().to_string())
            .unwrap_or_default();
        Some(match w.use_pack_item(key, roster)? {
            ItemOutcome::HealedHp { amount } => format!("{name} recovered {amount} HP."),
            ItemOutcome::HealedMp { amount } => format!("{name} recovered {amount} MP."),
            ItemOutcome::Revived { .. } => format!("{name} was revived."),
            ItemOutcome::Cured { .. } | ItemOutcome::CuredAll => format!("{name} was cured."),
            ItemOutcome::StatRaised { .. } | ItemOutcome::StatsRaised { .. } => {
                format!("{name} grew stronger.")
            }
            _ => "Used.".to_string(),
        })
    }

    fn equip(&mut self, key: &str, roster: u8, index: u8) -> bool {
        self.0.equip_pack_item(key, roster, index)
    }

    fn unequip(&mut self, roster: u8, index: u8) -> bool {
        self.0.unequip_pack_item(roster, index)
    }
}

impl ItemScreenHost for WorldItemsRef<'_> {
    read_half!();

    fn buy(&mut self, _: &ItemKey, _: u8, _: u32) -> bool {
        false
    }
    fn sell(&mut self, _: &str, _: u8, _: u32) -> bool {
        false
    }
    fn use_item(&mut self, _: &str, _: u8) -> Option<String> {
        None
    }
    fn equip(&mut self, _: &str, _: u8, _: u8) -> bool {
        false
    }
    fn unequip(&mut self, _: u8, _: u8) -> bool {
        false
    }
}

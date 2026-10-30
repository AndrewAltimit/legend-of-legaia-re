//! Pack items at runtime: the compiled catalog, the player's pack bag and
//! worn pack gear, and the stat, passive and set arithmetic the battle and
//! menu hooks read.
//!
//! Retail's item id is a `u8` and every value but five is a named retail
//! item, so a pack item cannot take a retail id without displacing one.
//! Pack items therefore live in their own store, keyed by their local
//! `items.toml` key: [`ItemProgress`] is the bag and the worn gear, and it
//! rides the save's expansion block (tail record 4) beside the quests, so
//! it follows the pack through activation, saves and the dormant block. The
//! retail bag and the character records never hold a pack item: worn pack
//! gear leaves its retail slot empty (`0`), and the stat fold adds the
//! pack item's bonuses on top.
//!
//! A battle needs a `u8` for the few pack items that take part in it (a
//! drop, a steal, a row in the Item command): [`BattleAliases`] lends each
//! one of the five unnamed retail ids ([`ALIAS_POOL`]) for that fight only.

use std::collections::BTreeMap;

use legaia_engine_battle::battle_stats::ItemModifier;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::items::{ItemDef, ItemEffectDef, ItemKind, ItemRef, SetDef};

/// The most of one item the bag holds (retail's stack cap).
pub const STACK_CAP: u8 = 99;

/// An item named at runtime: a retail id or a pack item's local key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemKey {
    Retail(u8),
    Pack(String),
}

impl ItemKey {
    /// Resolve a pack's [`ItemRef`]: a retail id, or a pack key local to
    /// `pack_id` (`pack_id:x` or a bare `x`). A key qualified with another
    /// pack's id resolves to nothing - only one pack is active at a time.
    pub fn resolve(pack_id: &str, r: &ItemRef) -> Option<Self> {
        if let Some(id) = r.retail_id() {
            return (id != 0).then_some(Self::Retail(id));
        }
        let key = r.pack_key()?;
        match key.split_once(':') {
            None => Some(Self::Pack(key.to_string())),
            Some((p, k)) if p == pack_id => Some(Self::Pack(k.to_string())),
            Some(_) => None,
        }
    }

    pub fn retail(&self) -> Option<u8> {
        match self {
            Self::Retail(id) => Some(*id),
            Self::Pack(_) => None,
        }
    }

    pub fn pack(&self) -> Option<&str> {
        match self {
            Self::Pack(k) => Some(k),
            Self::Retail(_) => None,
        }
    }
}

/// The retail equipment-byte index (`+0x196 + i`) a worn pack item of
/// `kind` occupies on roster slot `roster`'s record: body armour `0`, head
/// `1`, the weapon `2` (Noa's `3` - her Ra-Seru sits at `2`), footwear `4`,
/// the three Goods slots `5..=7`. `None` for an item that is not worn or a
/// roster slot past the three heroes.
pub fn equip_indices(kind: ItemKind, roster: u8) -> &'static [u8] {
    if roster > 2 {
        return &[];
    }
    match kind {
        ItemKind::Armor => &[0],
        ItemKind::Helmet => &[1],
        ItemKind::Weapon if roster == 1 => &[3],
        ItemKind::Weapon => &[2],
        ItemKind::Boots => &[4],
        ItemKind::Accessory => &[5, 6, 7],
        ItemKind::Consumable | ItemKind::Key => &[],
    }
}

/// The slot label of equipment byte `index` for roster slot `roster`.
pub fn slot_label(index: u8, roster: u8) -> &'static str {
    match (index, roster) {
        (0, _) => "Armor",
        (1, _) => "Helmet",
        (2, 1) | (3, 0) | (3, 2) => "Ra-Seru",
        (2, _) | (3, _) => "Weapon",
        (4, _) => "Boots",
        _ => "Goods",
    }
}

/// One compiled pack item.
#[derive(Debug, Clone, PartialEq)]
pub struct PackItem {
    /// The local key (`items.toml` `[items.<key>]`).
    pub key: String,
    pub def: ItemDef,
    /// The flat bonuses while worn (no set bonus).
    pub modifier: ItemModifier,
    /// Passive indices granted while worn, element guard included.
    pub passives: Vec<u8>,
}

impl PackItem {
    pub fn name(&self) -> &str {
        &self.def.name
    }

    /// Whether roster slot `roster` may wear it.
    pub fn wearable_by(&self, roster: u8) -> bool {
        self.def.kind.is_equipment() && roster < 3 && self.def.equip_mask() & (1 << roster) != 0
    }

    /// The retail item whose look the item borrows.
    pub fn icon(&self) -> Option<u8> {
        match self.def.icon {
            Some(legaia_expansion_format::refs::DiscRef::Item { id }) if id != 0 => Some(id),
            _ => None,
        }
    }
}

fn modifier(atk: i16, defence: i16, udf: i16, ldf: i16, spd: i16, int: i16) -> ItemModifier {
    ItemModifier {
        atk,
        udf: udf.saturating_add(defence),
        ldf: ldf.saturating_add(defence),
        spd,
        int,
        ability_bits: [0; 32],
    }
}

fn add_modifier(into: &mut ItemModifier, m: &ItemModifier) {
    into.atk = into.atk.saturating_add(m.atk);
    into.udf = into.udf.saturating_add(m.udf);
    into.ldf = into.ldf.saturating_add(m.ldf);
    into.spd = into.spd.saturating_add(m.spd);
    into.int = into.int.saturating_add(m.int);
}

/// The active pack's items, compiled once at activation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompiledItems {
    pub items: BTreeMap<String, PackItem>,
    pub sets: BTreeMap<String, SetDef>,
}

impl CompiledItems {
    pub fn compile(pack: &ExpansionPack) -> Self {
        let items = pack
            .items
            .items
            .iter()
            .map(|(key, d)| {
                (
                    key.clone(),
                    PackItem {
                        key: key.clone(),
                        def: d.clone(),
                        modifier: modifier(
                            d.attack,
                            d.defence,
                            d.udf,
                            d.ldf,
                            d.speed,
                            d.intelligence,
                        ),
                        passives: d.all_passives(),
                    },
                )
            })
            .collect();
        Self {
            items,
            sets: pack.items.sets.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&PackItem> {
        self.items.get(key)
    }

    /// The consumables usable in battle, in key order.
    pub fn battle_consumables(&self) -> impl Iterator<Item = &PackItem> {
        self.items.values().filter(|i| i.def.battle_usable())
    }
}

/// The player's pack items: the bag and the gear each hero wears.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemProgress {
    /// Held count by local key; an entry is removed when it reaches zero.
    pub bag: BTreeMap<String, u8>,
    /// Worn pack gear: `(roster slot, equipment byte index)` -> local key.
    pub worn: BTreeMap<(u8, u8), String>,
}

impl ItemProgress {
    pub fn is_empty(&self) -> bool {
        self.bag.is_empty() && self.worn.is_empty()
    }

    pub fn count(&self, key: &str) -> u8 {
        self.bag.get(key).copied().unwrap_or(0)
    }

    /// Add `n`, capped at [`STACK_CAP`]; returns how many went in.
    pub fn add(&mut self, key: &str, n: u8) -> u8 {
        let have = self.count(key);
        let room = STACK_CAP.saturating_sub(have);
        let put = n.min(room);
        if put > 0 {
            self.bag.insert(key.to_string(), have + put);
        }
        put
    }

    /// Take up to `n`; returns how many came out.
    pub fn take(&mut self, key: &str, n: u8) -> u8 {
        let have = self.count(key);
        let out = n.min(have);
        if have == out {
            self.bag.remove(key);
        } else {
            self.bag.insert(key.to_string(), have - out);
        }
        out
    }

    /// How many of `key` the heroes wear.
    pub fn worn_count(&self, key: &str) -> usize {
        self.worn.values().filter(|k| *k == key).count()
    }

    /// The pack item roster slot `roster` wears in byte `index`.
    pub fn worn_at(&self, roster: u8, index: u8) -> Option<&str> {
        self.worn.get(&(roster, index)).map(String::as_str)
    }

    /// Every pack item roster slot `roster` wears, by equipment byte.
    pub fn worn_by(&self, roster: u8) -> impl Iterator<Item = (u8, &str)> {
        self.worn
            .range((roster, 0)..=(roster, u8::MAX))
            .map(|(&(_, i), k)| (i, k.as_str()))
    }

    /// Put the worn item at `(roster, index)` back in the bag. Returns its
    /// key; `None` when the slot held no pack item. A full stack keeps the
    /// item worn.
    pub fn unequip(&mut self, roster: u8, index: u8) -> Option<String> {
        let key = self.worn.get(&(roster, index))?.clone();
        if self.add(&key, 1) == 0 {
            return None;
        }
        self.worn.remove(&(roster, index));
        Some(key)
    }

    /// Wear one `key` from the bag at `(roster, index)`, returning the pack
    /// item it displaced to the bag. `false` when none is held.
    pub fn equip(&mut self, key: &str, roster: u8, index: u8) -> bool {
        if self.count(key) == 0 {
            return false;
        }
        if self.worn.contains_key(&(roster, index)) && self.unequip(roster, index).is_none() {
            return false;
        }
        self.take(key, 1);
        self.worn.insert((roster, index), key.to_string());
        true
    }

    /// Drop every worn entry whose retail slot the record now fills itself
    /// (a retail item equipped over the pack one while the pack was away, or
    /// through the retail equip screen): the pack item goes back to the bag.
    /// `retail_slots(roster)` reads the record's eight equipment bytes.
    pub fn reconcile(&mut self, retail_slots: impl Fn(u8) -> Option<[u8; 8]>) -> Vec<String> {
        let clash: Vec<(u8, u8)> = self
            .worn
            .keys()
            .copied()
            .filter(|&(r, i)| retail_slots(r).is_none_or(|s| s[usize::from(i)] != 0))
            .collect();
        let mut back = Vec::new();
        for (r, i) in clash {
            if let Some(k) = self.worn.remove(&(r, i)) {
                self.add(&k, 1);
                back.push(k);
            }
        }
        back
    }
}

/// What roster slot `roster`'s worn pack gear adds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WornBonus {
    /// Per equipment byte, the attack each worn item adds (the battle's
    /// per-slot fold).
    pub slot_atk: [i16; 8],
    /// The sum of every worn item and every reached set tier.
    pub total: ItemModifier,
    /// The four ability words the worn gear and set tiers grant.
    pub ability_words: [u32; 4],
    /// `(set name, pieces worn)` for every set with two or more pieces on.
    pub sets: Vec<(String, u8)>,
}

fn set_passive(words: &mut [u32; 4], index: u8) {
    if index < 0x40 {
        words[usize::from(index >> 5)] |= 1 << (index & 0x1F);
    }
}

impl CompiledItems {
    /// The bonus roster slot `roster` gets from the pack gear it wears.
    /// `swap` replaces one byte's pack item for a preview (`Some(key)`
    /// wears it, `None` empties the slot).
    pub fn worn_bonus(
        &self,
        progress: &ItemProgress,
        roster: u8,
        swap: Option<(u8, Option<&str>)>,
    ) -> WornBonus {
        let mut worn: BTreeMap<u8, &str> = progress.worn_by(roster).collect();
        if let Some((index, key)) = swap {
            match key {
                Some(k) => worn.insert(index, k),
                None => worn.remove(&index),
            };
        }
        let mut out = WornBonus::default();
        let mut set_pieces: BTreeMap<&str, u8> = BTreeMap::new();
        for (index, key) in worn {
            let Some(item) = self.get(key) else {
                continue;
            };
            out.slot_atk[usize::from(index.min(7))] = item.modifier.atk;
            add_modifier(&mut out.total, &item.modifier);
            for &p in &item.passives {
                set_passive(&mut out.ability_words, p);
            }
            if let Some(s) = &item.def.set {
                *set_pieces.entry(s.as_str()).or_default() += 1;
            }
        }
        for (key, pieces) in set_pieces {
            let Some(set) = self.sets.get(key) else {
                continue;
            };
            if pieces >= 2 {
                out.sets.push((
                    if set.name.is_empty() {
                        key.to_string()
                    } else {
                        set.name.clone()
                    },
                    pieces,
                ));
            }
            for b in set
                .bonuses
                .iter()
                .filter(|b| b.pieces >= 2 && pieces >= b.pieces)
            {
                add_modifier(
                    &mut out.total,
                    &modifier(b.attack, b.defence, b.udf, b.ldf, b.speed, b.intelligence),
                );
                for &p in &b.passives {
                    set_passive(&mut out.ability_words, p);
                }
            }
        }
        out
    }
}

/// The five retail item ids no retail item uses (`0x12`, `0x1A`, `0x52`,
/// `0xB9`, `0xFD`: empty names in the static item table), lent to pack items
/// for one fight at a time.
pub const ALIAS_POOL: [u8; 5] = [0x12, 0x1A, 0x52, 0xB9, 0xFD];

/// The pack items one fight gave a `u8` to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BattleAliases {
    by_id: BTreeMap<u8, String>,
}

impl BattleAliases {
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The alias `key` holds, lending it the next free one. `None` once the
    /// pool is spent, or when `in_use(id)` says a candidate id is held by
    /// the retail side (a patched disc that named it).
    pub fn lend(&mut self, key: &str, in_use: impl Fn(u8) -> bool) -> Option<u8> {
        if let Some(id) = self.id_of(key) {
            return Some(id);
        }
        let id = ALIAS_POOL
            .into_iter()
            .find(|id| !self.by_id.contains_key(id) && !in_use(*id))?;
        self.by_id.insert(id, key.to_string());
        Some(id)
    }

    /// The pack item alias `id` stands for this fight.
    pub fn key(&self, id: u8) -> Option<&str> {
        self.by_id.get(&id).map(String::as_str)
    }

    pub fn id_of(&self, key: &str) -> Option<u8> {
        self.by_id
            .iter()
            .find(|(_, k)| k.as_str() == key)
            .map(|(&id, _)| id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (u8, &str)> {
        self.by_id.iter().map(|(&id, k)| (id, k.as_str()))
    }

    pub fn clear(&mut self) {
        self.by_id.clear();
    }
}

/// The items runtime: the compiled catalog and this fight's aliases. The
/// player's items themselves are pack progress
/// ([`crate::quest_save::ExpansionProgress::items`]).
#[derive(Debug, Clone, Default)]
pub struct ItemRuntime {
    pub compiled: Option<std::sync::Arc<CompiledItems>>,
    pub aliases: BattleAliases,
    /// The item screen while it owns the frame.
    pub seat: Option<Box<crate::item_screen::ItemSeat>>,
    /// Notices ("Got Rebirth Blade") for the host's toast.
    pub notices: Vec<String>,
}

/// What one consumable effect is, as the engine's item handlers see it.
/// `engine-core` maps it onto `legaia_engine_menus::items::ItemEffect`.
pub fn effect_of(def: &ItemDef) -> Option<&ItemEffectDef> {
    (def.kind == ItemKind::Consumable)
        .then_some(def.effect.as_ref())
        .flatten()
}

/// A long-lived copy of `s` for the engine's `&'static str` item names.
/// Each distinct name is leaked once per process, so re-activating a pack
/// costs nothing new.
pub fn intern(s: &str) -> &'static str {
    use std::collections::BTreeSet;
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<BTreeSet<&'static str>>> = OnceLock::new();
    let mut set = NAMES
        .get_or_init(|| Mutex::new(BTreeSet::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(&n) = set.get(s) {
        return n;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> ExpansionPack {
        ExpansionPack {
            manifest: legaia_expansion_format::manifest::PackManifest::new("t", "T", "1.0.0"),
            items: toml::from_str(
                r#"
[items.blade]
name = "Blade"
kind = "weapon"
attack = 100
set = "s"
[items.mail]
name = "Mail"
kind = "armor"
defence = 40
set = "s"
passives = [6]
[items.ring]
name = "Ring"
kind = "accessory"
element = 1
[sets.s]
name = "Set"
bonuses = [{ pieces = 2, attack = 10, passives = [0x24] }]
"#,
            )
            .unwrap(),
            ..Default::default()
        }
    }

    #[test]
    fn keys_resolve_local_and_qualified() {
        assert_eq!(
            ItemKey::resolve("t", &ItemRef::Retail(0x79)),
            Some(ItemKey::Retail(0x79))
        );
        assert_eq!(ItemKey::resolve("t", &ItemRef::Retail(0)), None);
        assert_eq!(
            ItemKey::resolve("t", &ItemRef::Pack("t:blade".into())),
            Some(ItemKey::Pack("blade".into()))
        );
        assert_eq!(
            ItemKey::resolve("t", &ItemRef::Pack("o:blade".into())),
            None
        );
    }

    #[test]
    fn noa_wears_her_weapon_in_byte_three() {
        assert_eq!(equip_indices(ItemKind::Weapon, 0), [2]);
        assert_eq!(equip_indices(ItemKind::Weapon, 1), [3]);
        assert_eq!(equip_indices(ItemKind::Accessory, 2), [5, 6, 7]);
        assert!(equip_indices(ItemKind::Weapon, 3).is_empty());
        assert!(equip_indices(ItemKind::Consumable, 0).is_empty());
    }

    #[test]
    fn bag_caps_and_equip_moves_one() {
        let mut p = ItemProgress::default();
        assert_eq!(p.add("blade", 98), 98);
        assert_eq!(p.add("blade", 5), 1);
        assert!(p.equip("blade", 0, 2));
        assert_eq!((p.count("blade"), p.worn_count("blade")), (98, 1));
        assert_eq!(p.unequip(0, 2).as_deref(), Some("blade"));
        assert_eq!(p.count("blade"), 99);
        assert!(!p.equip("mail", 0, 0), "none held");
        assert_eq!(p.take("blade", 200), 99);
        assert!(p.is_empty());
    }

    #[test]
    fn worn_bonus_sums_items_sets_and_passives() {
        let c = CompiledItems::compile(&pack());
        let mut p = ItemProgress::default();
        p.add("blade", 1);
        p.add("mail", 1);
        p.add("ring", 1);
        p.equip("blade", 0, 2);
        let one = c.worn_bonus(&p, 0, None);
        assert_eq!(one.slot_atk[2], 100);
        assert_eq!(one.total.atk, 100);
        assert!(one.sets.is_empty());
        p.equip("mail", 0, 0);
        p.equip("ring", 0, 5);
        let all = c.worn_bonus(&p, 0, None);
        assert_eq!(all.total.atk, 110, "the two-piece tier");
        assert_eq!((all.total.udf, all.total.ldf), (40, 40));
        assert_eq!(all.sets, [("Set".to_string(), 2)]);
        assert_ne!(all.ability_words[0] & (1 << 6), 0, "ATK Boost");
        assert_ne!(all.ability_words[0] & (1 << 0x1E), 0, "element 1 guard");
        assert_ne!(all.ability_words[1] & (1 << 4), 0, "the tier's 0x24");
        // Previewing the blade off drops the set tier too.
        let preview = c.worn_bonus(&p, 0, Some((2, None)));
        assert_eq!(preview.total.atk, 0);
    }

    #[test]
    fn reconcile_returns_pack_gear_under_retail_gear() {
        let mut p = ItemProgress::default();
        p.add("blade", 1);
        p.equip("blade", 0, 2);
        let mut slots = [0u8; 8];
        assert!(p.reconcile(|_| Some(slots)).is_empty());
        slots[2] = 0x27;
        assert_eq!(p.reconcile(|_| Some(slots)), ["blade"]);
        assert_eq!(p.count("blade"), 1);
        assert!(p.worn.is_empty());
    }

    #[test]
    fn aliases_lend_from_the_pool_once_per_key() {
        let mut a = BattleAliases::default();
        assert_eq!(a.lend("x", |_| false), Some(0x12));
        assert_eq!(a.lend("x", |_| false), Some(0x12));
        assert_eq!(
            a.lend("y", |id| id == 0x1A),
            Some(0x52),
            "a held id is skipped"
        );
        for k in ["c", "d", "e"] {
            a.lend(k, |_| false);
        }
        assert_eq!(a.lend("f", |_| false), None, "the pool is spent");
        assert_eq!(a.key(0x52), Some("y"));
    }

    #[test]
    fn interned_names_are_shared() {
        let a = intern("Rebirth Blade");
        let b = intern(&String::from("Rebirth Blade"));
        assert!(std::ptr::eq(a, b));
    }
}

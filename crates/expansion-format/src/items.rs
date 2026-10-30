//! `items.toml`: new consumables and equipment, equipment sets and the
//! pack's shops.
//!
//! ```toml
//! [items.rebirth_blade]
//! name = "Rebirth Blade"
//! description = "Forged in the tower's heart."
//! kind = "weapon"
//! icon = { kind = "item", id = 0x27 }   # borrows the Chaos Breaker's look
//! price = 64000
//! characters = ["vahn"]
//! attack = 108
//! set = "rebirth"
//!
//! [items.phoenix_plume]
//! name = "Phoenix Plume"
//! description = "Revive every fallen ally."
//! kind = "consumable"
//! price = 9000
//! effect = { kind = "revive", percent = 50 }
//! all_party = true
//!
//! [sets.rebirth]
//! name = "Rebirth Regalia"
//! bonuses = [{ pieces = 2, attack = 8 }, { pieces = 3, defence = 12, passives = [0x24] }]
//!
//! [[shops]]
//! id = "tower_armory"
//! name = "Tower Armory"
//! stock = ["rebirth_blade", 0x79, { item = "phoenix_plume", requires = { flags = ["floor3"] } }]
//! ```
//!
//! A pack item is not a retail item id. Retail's id space is a `u8` with
//! every value but a handful already named, so pack items live beside it:
//! the engine keeps them in their own store, keyed by `pack:item`, and the
//! save carries them by that key (see `docs/formats/expansion-pack-items.md`).
//! Everywhere a pack names an item - a drop, a steal, a reward, a quest's
//! collect objective, a shop row - it writes an [`ItemRef`]: a retail id or a
//! pack item key.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::quest::Prerequisites;
use crate::refs::DiscRef;

/// A reference to an item: a retail item id or a pack item.
///
/// In TOML: an integer (`0x79`, a retail id), an item coordinate
/// (`{ kind = "item", id = 0x79 }`, the same retail id) or a string
/// (`"rebirth_blade"`, a pack item; `other:item` names one of a required
/// pack's).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ItemRef {
    /// A retail item id (the `PTR_DAT_8007436C` id space).
    Retail(u8),
    /// A retail item as a disc coordinate; only [`DiscRef::Item`] is an item.
    Disc(DiscRef),
    /// A pack item key (`items.toml` `[items.<key>]`).
    Pack(String),
}

impl Default for ItemRef {
    fn default() -> Self {
        Self::Retail(0)
    }
}

impl From<u8> for ItemRef {
    fn from(id: u8) -> Self {
        Self::Retail(id)
    }
}

impl ItemRef {
    /// The retail item id, when the reference names one.
    pub fn retail_id(&self) -> Option<u8> {
        match self {
            Self::Retail(id) | Self::Disc(DiscRef::Item { id }) => Some(*id),
            _ => None,
        }
    }

    /// The pack item key, as written (local or `pack:item`).
    pub fn pack_key(&self) -> Option<&str> {
        match self {
            Self::Pack(k) => Some(k),
            _ => None,
        }
    }

    /// Whether the reference names an item at all (a disc coordinate of
    /// another kind does not).
    pub fn is_item(&self) -> bool {
        !matches!(self, Self::Disc(d) if !matches!(d, DiscRef::Item { .. }))
    }

    /// Record the reference at `field`: a retail id is a disc coordinate, a
    /// pack key a content reference to an `items.toml` item.
    pub fn check(&self, cx: &mut crate::validate::Checker<'_>, field: &str) {
        match self {
            Self::Retail(0) | Self::Disc(DiscRef::Item { id: 0 }) => {
                cx.error(field, "item id 0 is \"no item\"");
            }
            Self::Retail(id) => cx.disc_ref(field, &DiscRef::Item { id: *id }),
            Self::Disc(d @ DiscRef::Item { .. }) => cx.disc_ref(field, d),
            Self::Disc(d) => cx.error(field, format!("{d} is not an item")),
            Self::Pack(k) => cx.reference("item", k, field),
        }
    }
}

impl std::fmt::Display for ItemRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retail(id) => write!(f, "item 0x{id:02X}"),
            Self::Disc(d) => write!(f, "{d}"),
            Self::Pack(k) => f.write_str(k),
        }
    }
}

/// The `items.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ItemSection {
    /// Pack items by key.
    pub items: BTreeMap<String, ItemDef>,
    /// Equipment sets by key.
    pub sets: BTreeMap<String, SetDef>,
    /// The pack's shops, opened by a map's `shop` trigger or a `shop` NPC.
    pub shops: Vec<ShopDef>,
}

/// What a pack item is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Used from the bag (field and / or battle).
    #[default]
    Consumable,
    /// The character's weapon slot.
    Weapon,
    /// Body armour.
    Armor,
    /// Head gear.
    Helmet,
    /// Footwear.
    Boots,
    /// A Goods (accessory) slot; three per character.
    Accessory,
    /// Carried only: quest items, keys.
    Key,
}

impl ItemKind {
    /// Whether the item is worn.
    pub fn is_equipment(self) -> bool {
        !matches!(self, Self::Consumable | Self::Key)
    }

    /// The label the item screens print.
    pub fn label(self) -> &'static str {
        match self {
            Self::Consumable => "Item",
            Self::Weapon => "Weapon",
            Self::Armor => "Armor",
            Self::Helmet => "Helmet",
            Self::Boots => "Boots",
            Self::Accessory => "Accessory",
            Self::Key => "Key item",
        }
    }
}

/// A character a pack item may be worn by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Character {
    Vahn,
    Noa,
    Gala,
}

impl Character {
    /// The roster slot (`0` Vahn, `1` Noa, `2` Gala).
    pub fn slot(self) -> u8 {
        match self {
            Self::Vahn => 0,
            Self::Noa => 1,
            Self::Gala => 2,
        }
    }
}

/// A status ailment a cure removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Venom,
    Toxic,
    Numb,
    Sleep,
    Confuse,
    Rot,
    Curse,
    Stone,
}

/// What using a consumable does: each maps onto one of the engine's retail
/// item handlers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemEffectDef {
    /// Restore `amount` HP, or all of it with `full`.
    HealHp {
        #[serde(default)]
        amount: u16,
        #[serde(default)]
        full: bool,
    },
    /// Restore `amount` MP, or all of it with `full`.
    HealMp {
        #[serde(default)]
        amount: u16,
        #[serde(default)]
        full: bool,
    },
    /// Revive a fallen ally at `percent` of max HP (`1..=100`).
    Revive { percent: u8 },
    /// Cure one ailment.
    Cure { status: Status },
    /// Cure every ailment.
    CureAll,
    /// Deal `amount` damage to one enemy (battle only).
    Damage { amount: u16 },
    /// Run retail item `item`'s own effect: its stat raise, battle buff,
    /// gauge extension or any other handler, exactly as using that item
    /// would.
    AsItem { item: u8 },
}

/// One pack item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ItemDef {
    /// The name the menus show (at most [`MAX_NAME_LEN`] characters).
    pub name: String,
    /// The info line; `|` breaks it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub kind: ItemKind,
    /// A retail item whose look the item borrows: its menu icon, and for
    /// equipment the mesh a battle draws (a [`DiscRef::Item`]). Equipment
    /// with none borrows nothing and the slot draws as empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<DiscRef>,
    /// Buy price in gold; `0` is never sold.
    pub price: u32,
    /// Sell price; half the buy price when left out. `0` cannot be sold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sell: Option<u32>,
    /// Who may wear it; every character when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub characters: Vec<Character>,
    /// Flat stat bonuses while worn.
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub attack: i16,
    /// Both defence facets at once (added to `udf` / `ldf`).
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub defence: i16,
    /// Upper-body defence.
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub udf: i16,
    /// Lower-body defence.
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub ldf: i16,
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub speed: i16,
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub intelligence: i16,
    /// An element (`0..=6`) the wearer guards against: the retail
    /// elemental-guard passive of that element (index `0x1D + element`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<u8>,
    /// Accessory passive indices (`0x00..=0x3F`) the item grants while worn.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub passives: Vec<u8>,
    /// The set (`[sets.<key>]`) the item belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
    /// What using it does (consumables).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<ItemEffectDef>,
    /// A consumable applies to the whole party at once.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub all_party: bool,
    /// Usable from the field menu (consumables); `true` when left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<bool>,
    /// Usable in battle (consumables); `true` when left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battle: Option<bool>,
}

impl ItemDef {
    /// The sell price.
    pub fn sell_price(&self) -> u32 {
        self.sell.unwrap_or(self.price / 2)
    }

    /// The equip mask: bit `n` for roster slot `n`; `0b111` when empty.
    pub fn equip_mask(&self) -> u8 {
        if self.characters.is_empty() {
            0b111
        } else {
            self.characters.iter().fold(0, |m, c| m | (1 << c.slot()))
        }
    }

    /// Every passive index the item grants (its `passives` and its element
    /// guard).
    pub fn all_passives(&self) -> Vec<u8> {
        let mut out = self.passives.clone();
        if let Some(e) = self.element.filter(|&e| e <= MAX_GUARD_ELEMENT) {
            out.push(ELEMENT_GUARD_BASE + e);
        }
        out
    }

    /// Whether the consumable is usable from the field menu.
    pub fn field_usable(&self) -> bool {
        self.kind == ItemKind::Consumable
            && self.field.unwrap_or(true)
            && !matches!(self.effect, Some(ItemEffectDef::Damage { .. }))
    }

    /// Whether the consumable is usable in battle.
    pub fn battle_usable(&self) -> bool {
        self.kind == ItemKind::Consumable && self.battle.unwrap_or(true)
    }
}

/// An equipment set: bonuses for wearing several of its pieces at once.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SetDef {
    pub name: String,
    /// Each tier applies when at least `pieces` of the set are worn; tiers
    /// stack.
    pub bonuses: Vec<SetBonus>,
}

/// One set tier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SetBonus {
    pub pieces: u8,
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub attack: i16,
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub defence: i16,
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub udf: i16,
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub ldf: i16,
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub speed: i16,
    #[serde(skip_serializing_if = "is_zero_i16")]
    pub intelligence: i16,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub passives: Vec<u8>,
}

/// One pack shop.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShopDef {
    pub id: String,
    /// The counter's heading.
    pub name: String,
    /// What it sells, in row order.
    pub stock: Vec<StockEntry>,
}

/// One shop row: an item, optionally gated and priced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StockEntry {
    /// A bare item reference.
    Item(ItemRef),
    /// An item with a gate or a price of its own.
    Gated {
        item: ItemRef,
        /// The row is listed only while this holds.
        #[serde(default, skip_serializing_if = "Prerequisites::is_empty")]
        requires: Prerequisites,
        /// The row's price; the item's own when left out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        price: Option<u32>,
    },
}

impl StockEntry {
    pub fn item(&self) -> &ItemRef {
        match self {
            Self::Item(i) | Self::Gated { item: i, .. } => i,
        }
    }

    /// The row's gate (always open for a bare reference).
    pub fn requires(&self) -> Option<&Prerequisites> {
        match self {
            Self::Item(_) => None,
            Self::Gated { requires, .. } => Some(requires),
        }
    }

    /// The row's own price, if it sets one.
    pub fn price(&self) -> Option<u32> {
        match self {
            Self::Item(_) => None,
            Self::Gated { price, .. } => *price,
        }
    }
}

fn is_zero_i16(v: &i16) -> bool {
    *v == 0
}

/// The longest item name accepted: the menus' name column.
pub const MAX_NAME_LEN: usize = 20;
/// The first elemental-guard passive index; element `e` guards at
/// `ELEMENT_GUARD_BASE + e`.
pub const ELEMENT_GUARD_BASE: u8 = 0x1D;
/// The highest element an equipment guard names (`0..=6`; `7` is neutral).
pub const MAX_GUARD_ELEMENT: u8 = 6;
/// The passive index space (`0x00..=0x3F`).
pub const PASSIVE_COUNT: u8 = 0x40;
/// The largest buy price (the purse's own ceiling).
pub const MAX_PRICE: u32 = 9_999_999;
/// The largest flat stat bonus an item or set tier grants.
pub const MAX_STAT_BONUS: i16 = 255;

impl ItemSection {
    /// The shop `id`, or the first one for `""`.
    pub fn shop(&self, id: &str) -> Option<&ShopDef> {
        if id.is_empty() {
            self.shops.first()
        } else {
            self.shops.iter().find(|s| s.id == id)
        }
    }
}

fn check_stats(cx: &mut crate::validate::Checker<'_>, at: &str, stats: [(&str, i16); 6]) {
    for (name, v) in stats {
        if !(-MAX_STAT_BONUS..=MAX_STAT_BONUS).contains(&v) {
            cx.error(
                &format!("{at}.{name}"),
                format!("{v} is outside -{MAX_STAT_BONUS}..={MAX_STAT_BONUS}"),
            );
        }
    }
}

fn check_passives(cx: &mut crate::validate::Checker<'_>, at: &str, passives: &[u8]) {
    for (i, &p) in passives.iter().enumerate() {
        if p >= PASSIVE_COUNT {
            cx.error(
                &format!("{at}.passives[{i}]"),
                format!("passive 0x{p:02X} is outside 0x00..=0x3F"),
            );
        }
    }
}

impl crate::validate::SectionRules for ItemSection {
    fn declare(&self, cx: &mut crate::validate::Checker<'_>) {
        for key in self.items.keys() {
            cx.declare("item", key, &format!("items.{key}"));
        }
        for key in self.sets.keys() {
            cx.declare("item_set", key, &format!("sets.{key}"));
        }
        for (i, s) in self.shops.iter().enumerate() {
            cx.declare("shop", &s.id, &format!("shops[{i}].id"));
        }
    }

    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        for (key, it) in &self.items {
            let at = format!("items.{key}");
            let name = it.name.trim();
            if name.is_empty() {
                cx.error(&format!("{at}.name"), "the item has no name");
            } else if name.chars().count() > MAX_NAME_LEN {
                cx.error(
                    &format!("{at}.name"),
                    format!("the name is longer than {MAX_NAME_LEN} characters"),
                );
            }
            if !it.name.is_ascii() || !it.description.is_ascii() {
                cx.warning(&at, "the dialog font draws ASCII only");
            }
            match &it.icon {
                None if it.kind.is_equipment() => cx.warning(
                    &format!("{at}.icon"),
                    "equipment with no icon draws an empty slot in battle",
                ),
                None => {}
                Some(d @ DiscRef::Item { .. }) => cx.disc_ref(&format!("{at}.icon"), d),
                Some(d) => cx.error(&format!("{at}.icon"), format!("{d} is not an item")),
            }
            if it.price > MAX_PRICE || it.sell.is_some_and(|s| s > MAX_PRICE) {
                cx.error(&format!("{at}.price"), "price is over 9,999,999");
            }
            if it.sell_price() > it.price && it.price > 0 {
                cx.warning(&format!("{at}.sell"), "sells for more than it costs");
            }
            check_stats(
                cx,
                &at,
                [
                    ("attack", it.attack),
                    ("defence", it.defence),
                    ("udf", it.udf),
                    ("ldf", it.ldf),
                    ("speed", it.speed),
                    ("intelligence", it.intelligence),
                ],
            );
            check_passives(cx, &at, &it.passives);
            if it.element.is_some_and(|e| e > MAX_GUARD_ELEMENT) {
                cx.error(&format!("{at}.element"), "element must be 0..=6");
            }
            if let Some(s) = &it.set {
                cx.reference("item_set", s, &format!("{at}.set"));
            }
            let worn_fields = it.attack != 0
                || it.defence != 0
                || it.udf != 0
                || it.ldf != 0
                || it.speed != 0
                || it.intelligence != 0
                || it.element.is_some()
                || !it.passives.is_empty()
                || it.set.is_some()
                || !it.characters.is_empty();
            match it.kind {
                ItemKind::Consumable => {
                    match &it.effect {
                        None => cx.error(&format!("{at}.effect"), "a consumable needs an effect"),
                        Some(e) => check_effect(cx, &format!("{at}.effect"), e),
                    }
                    if worn_fields {
                        cx.warning(&at, "stat, passive and set fields only apply to equipment");
                    }
                    if !it.field_usable() && !it.battle_usable() {
                        cx.error(
                            &at,
                            "the consumable is usable neither in the field nor in battle",
                        );
                    }
                }
                ItemKind::Key => {
                    if it.effect.is_some() || worn_fields {
                        cx.warning(&at, "a key item has no effect and is not worn");
                    }
                }
                _ => {
                    if it.effect.is_some() || it.all_party {
                        cx.warning(&at, "effect and all_party only apply to consumables");
                    }
                }
            }
        }
        for (key, set) in &self.sets {
            let at = format!("sets.{key}");
            if set.name.trim().is_empty() {
                cx.warning(&format!("{at}.name"), "the set has no name");
            }
            let pieces = self
                .items
                .values()
                .filter(|i| i.set.as_deref() == Some(key.as_str()))
                .count();
            if pieces < 2 {
                cx.warning(&at, format!("only {pieces} item(s) belong to the set"));
            }
            for (i, b) in set.bonuses.iter().enumerate() {
                let bat = format!("{at}.bonuses[{i}]");
                if b.pieces < 2 {
                    cx.error(&format!("{bat}.pieces"), "a tier needs at least 2 pieces");
                } else if usize::from(b.pieces) > pieces {
                    cx.warning(
                        &format!("{bat}.pieces"),
                        format!("the set has only {pieces} pieces"),
                    );
                }
                check_stats(
                    cx,
                    &bat,
                    [
                        ("attack", b.attack),
                        ("defence", b.defence),
                        ("udf", b.udf),
                        ("ldf", b.ldf),
                        ("speed", b.speed),
                        ("intelligence", b.intelligence),
                    ],
                );
                check_passives(cx, &bat, &b.passives);
            }
        }
        for (i, s) in self.shops.iter().enumerate() {
            let at = format!("shops[{i}]");
            if s.stock.is_empty() {
                cx.warning(&format!("{at}.stock"), "the shop sells nothing");
            }
            for (k, row) in s.stock.iter().enumerate() {
                let field = format!("{at}.stock[{k}]");
                row.item().check(cx, &field);
                if let Some(r) = row.requires() {
                    r.check(cx, &format!("{field}.requires"));
                }
                let priced = row.price().is_some_and(|p| p > 0)
                    || row
                        .item()
                        .pack_key()
                        .and_then(|k| self.items.get(local(cx, k)))
                        .is_some_and(|d| d.price > 0);
                if row.item().pack_key().is_some() && !priced {
                    cx.warning(&field, "the item has no price; the row cannot be bought");
                }
            }
        }
    }
}

/// The local part of a pack key (`rebirth:x` -> `x` in pack `rebirth`).
fn local<'k>(cx: &crate::validate::Checker<'_>, key: &'k str) -> &'k str {
    match key.split_once(':') {
        Some((p, k)) if p == cx.pack().manifest.id => k,
        _ => key,
    }
}

fn check_effect(cx: &mut crate::validate::Checker<'_>, at: &str, e: &ItemEffectDef) {
    match e {
        ItemEffectDef::HealHp {
            amount: 0,
            full: false,
        }
        | ItemEffectDef::HealMp {
            amount: 0,
            full: false,
        } => cx.error(at, "a heal needs an amount or `full = true`"),
        ItemEffectDef::Revive { percent } if !(1..=100).contains(percent) => {
            cx.error(at, "revive percent must be 1..=100")
        }
        ItemEffectDef::Damage { amount: 0 } => cx.error(at, "damage needs an amount"),
        ItemEffectDef::AsItem { item } => {
            if *item == 0 {
                cx.error(at, "item id 0 is \"no item\"");
            } else {
                cx.disc_ref(at, &DiscRef::Item { id: *item });
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[items.rebirth_blade]
name = "Rebirth Blade"
kind = "weapon"
icon = { kind = "item", id = 0x27 }
price = 64000
characters = ["vahn"]
attack = 108
set = "rebirth"

[items.rebirth_mail]
name = "Rebirth Mail"
kind = "armor"
icon = { kind = "item", id = 0x4A }
defence = 90
characters = ["vahn"]
set = "rebirth"

[items.plume]
name = "Phoenix Plume"
kind = "consumable"
price = 9000
effect = { kind = "revive", percent = 50 }
all_party = true

[sets.rebirth]
name = "Rebirth Regalia"
bonuses = [{ pieces = 2, attack = 8, passives = [0x24] }]

[[shops]]
id = "armory"
name = "Tower Armory"
stock = ["rebirth_blade", 0x79, { item = "plume", requires = { flags = ["f"] }, price = 8000 }]
"#;

    #[test]
    fn parses_items_sets_and_shops() {
        let s: ItemSection = toml::from_str(SAMPLE).unwrap();
        let blade = &s.items["rebirth_blade"];
        assert_eq!(blade.kind, ItemKind::Weapon);
        assert_eq!(blade.equip_mask(), 0b001);
        assert_eq!(blade.sell_price(), 32000);
        assert_eq!(s.items["plume"].equip_mask(), 0b111);
        let shop = s.shop("").unwrap();
        assert_eq!(shop.stock[0].item(), &ItemRef::Pack("rebirth_blade".into()));
        assert_eq!(shop.stock[1].item().retail_id(), Some(0x79));
        assert_eq!(shop.stock[2].price(), Some(8000));
        assert!(!shop.stock[2].requires().unwrap().is_empty());
        let back: ItemSection = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s, "round trip");
    }

    #[test]
    fn item_ref_forms() {
        #[derive(Deserialize)]
        struct W {
            a: ItemRef,
            b: ItemRef,
            c: ItemRef,
        }
        let w: W =
            toml::from_str("a = 121\nb = { kind = \"item\", id = 5 }\nc = \"rebirth:x\"").unwrap();
        assert_eq!(w.a.retail_id(), Some(121));
        assert_eq!(w.b.retail_id(), Some(5));
        assert_eq!(w.c.pack_key(), Some("rebirth:x"));
        assert!(!ItemRef::Disc(DiscRef::Monster { id: 1 }).is_item());
    }

    #[test]
    fn the_validator_checks_items_refs_and_shops() {
        let mut files = crate::PackFiles::new();
        files.insert(
            "pack.toml".into(),
            b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\n".to_vec(),
        );
        files.insert(
            "items.toml".into(),
            br#"
[items.blade]
name = "A Name Far Too Long For The Menu"
kind = "weapon"
attack = 300
passives = [0x40]
set = "nope"
[items.tonic]
name = "Tonic"
[[shops]]
id = "s"
stock = ["ghost", 0x77, 0]
"#
            .to_vec(),
        );
        files.insert(
            "quests.toml".into(),
            br#"
[[quests]]
id = "q"
title = "Q"
auto_start = true
rewards = { items = [{ item = "tonic" }, { item = "missing" }] }
"#
            .to_vec(),
        );
        let issues = crate::validate::validate_files(&files, None);
        let errs: Vec<String> = issues
            .iter()
            .filter(|i| i.severity == crate::validate::Severity::Error)
            .map(|i| format!("{}:{}", i.path, i.field))
            .collect();
        for want in [
            "items.toml:items.blade.name",
            "items.toml:items.blade.attack",
            "items.toml:items.blade.passives[0]",
            "items.toml:items.blade.set",
            "items.toml:items.tonic.effect",
            "items.toml:shops[0].stock[0]",
            "items.toml:shops[0].stock[2]",
            "quests.toml:quests[0].rewards.items[1]",
        ] {
            assert!(errs.iter().any(|e| e == want), "{want} in {errs:?}");
        }
        assert!(
            !errs
                .iter()
                .any(|e| e == "quests.toml:quests[0].rewards.items[0]"),
            "a defined pack item resolves: {errs:?}"
        );
    }

    #[test]
    fn element_guard_is_a_passive() {
        let d = ItemDef {
            element: Some(2),
            passives: vec![6],
            ..Default::default()
        };
        assert_eq!(d.all_passives(), [6, 0x1F]);
    }
}

//! `cards.toml`: trading cards - the card definitions, the booster sets
//! they come in, and the reward rules that hand boosters or cards out.
//!
//! A card's art and theme are disc coordinates ([`DiscRef`]): a monster
//! card names the retail monster whose model the host renders, an item
//! card the item. The card's name and flavour text are the modder's own
//! words.
//!
//! ```toml
//! [[cards]]
//! id = "gobu-scout"
//! name = "Gobu Scout"
//! rarity = "common"
//! element = "earth"
//! sides = [3, 2, 1, 4]        # top, right, bottom, left
//! art = { kind = "monster", id = 1 }
//!
//! [[boosters]]
//! id = "starter"
//! cards_per_pack = 5
//! weights = { common = 70, uncommon = 22, rare = 7, legendary = 1 }
//! guaranteed = "uncommon"
//!
//! [[rewards]]
//! source = { kind = "monster", id = 1 }
//! booster = "starter"
//! chance_percent = 10
//!
//! [[dealers]]
//! id = "tower"
//! name = "Card Dealer"
//! stock = ["starter"]
//! ```
//!
//! A reward rule's `source` is a retail monster id (any fight with a pack
//! active, retail fights included), a pack formation, a pack boss or a pack
//! mode instance. A dealer sells the boosters in its `stock` that carry a
//! `price` and buys single cards at its `sell_percent` of the card's sell
//! value; a map trigger (`event = { kind = "card_dealer", dealer = "tower" }`)
//! opens it.

use serde::{Deserialize, Serialize};

use crate::refs::DiscRef;

/// The `cards.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CardSection {
    pub cards: Vec<CardDef>,
    pub boosters: Vec<BoosterSet>,
    pub rewards: Vec<CardRewardRule>,
    /// Gold paid per card sold, by rarity; a card's own `sell` overrides.
    pub sell_prices: RarityTable,
    /// Cards handed over once, the first time the pack's collection is
    /// opened - a starter deck for the Grid Duel.
    pub starter: Vec<String>,
    /// The card shops a map can open.
    pub dealers: Vec<CardDealer>,
}

impl CardSection {
    /// The card with `id`.
    pub fn card(&self, id: &str) -> Option<&CardDef> {
        self.cards.iter().find(|c| c.id == id)
    }

    /// The booster set with `id`.
    pub fn booster(&self, id: &str) -> Option<&BoosterSet> {
        self.boosters.iter().find(|b| b.id == id)
    }

    /// The dealer with `id`; an empty id names the first dealer, or a
    /// default one selling every priced booster when the pack names none.
    pub fn dealer(&self, id: &str) -> Option<CardDealer> {
        if id.is_empty() {
            return Some(self.dealers.first().cloned().unwrap_or_default());
        }
        self.dealers.iter().find(|d| d.id == id).cloned()
    }

    /// The boosters `dealer` sells: its stock that carries a price, in stock
    /// order (every priced booster, in file order, for an empty stock).
    pub fn dealer_stock<'a>(&'a self, dealer: &CardDealer) -> Vec<&'a BoosterSet> {
        if dealer.stock.is_empty() {
            return self.boosters.iter().filter(|b| b.price.is_some()).collect();
        }
        dealer
            .stock
            .iter()
            .filter_map(|id| self.booster(id))
            .filter(|b| b.price.is_some())
            .collect()
    }
}

/// A card shop: what it sells and how it buys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CardDealer {
    pub id: String,
    pub name: String,
    /// Booster ids on sale; empty = every booster that has a `price`.
    pub stock: Vec<String>,
    /// Whether the dealer buys single cards.
    pub buys_cards: bool,
    /// What the dealer pays for a card, as a percentage of its sell value.
    pub sell_percent: u8,
}

impl Default for CardDealer {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Card Dealer".into(),
            stock: Vec::new(),
            buys_cards: true,
            sell_percent: 100,
        }
    }
}

/// Card rarity, lowest first.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Rarity {
    #[default]
    Common,
    Uncommon,
    Rare,
    Legendary,
}

impl Rarity {
    pub const ALL: [Rarity; 4] = [
        Rarity::Common,
        Rarity::Uncommon,
        Rarity::Rare,
        Rarity::Legendary,
    ];
}

/// The elements a card can carry - Legaia's battle elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardElement {
    Fire,
    Water,
    Wind,
    Earth,
    Thunder,
    Light,
    Dark,
}

/// One trading card.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CardDef {
    /// Stable id the collection and save files key on.
    pub id: String,
    pub name: String,
    pub flavor: String,
    pub rarity: Rarity,
    pub element: Option<CardElement>,
    /// Battle strength per side, `[top, right, bottom, left]`, `1..=10`.
    pub sides: [u8; 4],
    /// What the card face shows (a monster, an item, a character mesh).
    pub art: Option<DiscRef>,
    /// Gold paid when sold; `None` takes the section's rarity price.
    pub sell: Option<u32>,
}

/// One `u32` per rarity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RarityTable {
    pub common: u32,
    pub uncommon: u32,
    pub rare: u32,
    pub legendary: u32,
}

impl RarityTable {
    pub fn get(&self, r: Rarity) -> u32 {
        match r {
            Rarity::Common => self.common,
            Rarity::Uncommon => self.uncommon,
            Rarity::Rare => self.rare,
            Rarity::Legendary => self.legendary,
        }
    }
}

/// A booster set: how many cards a pack holds and how its rarity is
/// rolled.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoosterSet {
    pub id: String,
    pub name: String,
    pub cards_per_pack: u8,
    /// Card ids the set draws from; empty = every card in the section.
    pub pool: Vec<String>,
    /// Relative rarity weights per slot.
    pub weights: RarityTable,
    /// The last slot is at least this rare.
    pub guaranteed: Option<Rarity>,
    /// Shop price in gold, when the set is sold.
    pub price: Option<u32>,
}

/// What hands a card reward out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RewardSource {
    /// Defeating a retail monster (1-based PROT 867 id) in any fight.
    Monster { id: u16 },
    /// Winning a pack-defined formation (`battle.toml` formation id).
    Formation { id: String },
    /// Beating a pack boss (`battle.toml` boss id).
    Boss { id: String },
    /// Finishing a pack-defined mode instance (`modes.toml` id).
    Mode { id: String },
}

/// One reward rule: `source` grants `booster` or `card` with
/// `chance_percent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardRewardRule {
    pub source: RewardSource,
    #[serde(default)]
    pub booster: Option<String>,
    #[serde(default)]
    pub card: Option<String>,
    #[serde(default = "full_chance")]
    pub chance_percent: u8,
    #[serde(default = "one")]
    pub count: u8,
}

/// `cards.toml`'s rules. Card, booster and dealer ids are the section's
/// own (cards and boosters keep the hyphenated ids save files key on), so
/// they are checked here rather than declared as pack-wide content ids.
impl crate::validate::SectionRules for CardSection {
    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        let mut seen = std::collections::BTreeSet::new();
        for (i, c) in self.cards.iter().enumerate() {
            let at = format!("cards[{i}]");
            if c.id.is_empty() || !seen.insert(c.id.as_str()) {
                cx.error(&at, format!("card id {:?} is empty or repeated", c.id));
            }
            if c.sides.iter().any(|&s| !(1..=10).contains(&s)) {
                cx.error(&format!("{at}.sides"), "each side is 1..=10");
            }
            if c.name.trim().is_empty() {
                cx.warning(&at, format!("card {:?} has no name", c.id));
            }
            if let Some(art) = &c.art {
                cx.disc_ref(&format!("{at}.art"), art);
            }
        }
        let mut boosters = std::collections::BTreeSet::new();
        for (i, b) in self.boosters.iter().enumerate() {
            let at = format!("boosters[{i}]");
            if b.id.is_empty() || !boosters.insert(b.id.as_str()) {
                cx.error(&at, format!("booster id {:?} is empty or repeated", b.id));
            }
            if b.cards_per_pack == 0 {
                cx.error(
                    &format!("{at}.cards_per_pack"),
                    "a booster holds at least one card",
                );
            }
            for (j, id) in b.pool.iter().enumerate() {
                if self.card(id).is_none() {
                    cx.error(&format!("{at}.pool[{j}]"), format!("no card {id:?}"));
                }
            }
            if b.weights == RarityTable::default() {
                cx.error(&format!("{at}.weights"), "every rarity weighs zero");
            }
        }
        for (i, id) in self.starter.iter().enumerate() {
            if self.card(id).is_none() {
                cx.error(&format!("starter[{i}]"), format!("no card {id:?}"));
            }
        }
        for (i, r) in self.rewards.iter().enumerate() {
            let at = format!("rewards[{i}]");
            if r.booster.is_none() && r.card.is_none() {
                cx.error(&at, "a reward names a booster or a card");
            }
            if let Some(b) = &r.booster
                && self.booster(b).is_none()
            {
                cx.error(&format!("{at}.booster"), format!("no booster {b:?}"));
            }
            if let Some(c) = &r.card
                && self.card(c).is_none()
            {
                cx.error(&format!("{at}.card"), format!("no card {c:?}"));
            }
            if r.chance_percent > 100 {
                cx.error(&format!("{at}.chance_percent"), "a chance is 0..=100");
            }
            match &r.source {
                RewardSource::Monster { id } => {
                    cx.disc_ref(&format!("{at}.source"), &DiscRef::Monster { id: *id })
                }
                RewardSource::Formation { id } => {
                    cx.reference("formation", id, &format!("{at}.source"))
                }
                RewardSource::Boss { id } => cx.reference("boss", id, &format!("{at}.source")),
                RewardSource::Mode { id } => {
                    let m = &cx.pack().modes;
                    let known = m.poker.iter().any(|x| x.id == *id)
                        || m.card_duels.iter().any(|x| x.id == *id)
                        || m.golf.iter().any(|x| x.id == *id)
                        || m.races.iter().any(|x| x.id == *id)
                        || m.prop_hunt.iter().any(|x| x.id == *id);
                    if !known {
                        cx.error(&format!("{at}.source"), format!("no mode instance {id:?}"));
                    }
                }
            }
        }
        let mut dealers = std::collections::BTreeSet::new();
        for (i, d) in self.dealers.iter().enumerate() {
            let at = format!("dealers[{i}]");
            if d.id.is_empty() || !dealers.insert(d.id.as_str()) {
                cx.error(&at, format!("dealer id {:?} is empty or repeated", d.id));
            }
            for (j, b) in d.stock.iter().enumerate() {
                match self.booster(b) {
                    None => cx.error(&format!("{at}.stock[{j}]"), format!("no booster {b:?}")),
                    Some(set) if set.price.is_none() => cx.warning(
                        &format!("{at}.stock[{j}]"),
                        format!("booster {b:?} has no price, so it is not sold"),
                    ),
                    Some(_) => {}
                }
            }
            if d.sell_percent > 100 {
                cx.warning(
                    &format!("{at}.sell_percent"),
                    "a dealer paying over the sell value is a gold loop",
                );
            }
        }
    }
}

fn full_chance() -> u8 {
    100
}

fn one() -> u8 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_example() {
        let text = r#"
[[cards]]
id = "gobu-scout"
name = "Gobu Scout"
rarity = "common"
element = "earth"
sides = [3, 2, 1, 4]
art = { kind = "monster", id = 1 }

[[boosters]]
id = "starter"
cards_per_pack = 5
weights = { common = 70, uncommon = 22, rare = 7, legendary = 1 }
guaranteed = "uncommon"

[[rewards]]
source = { kind = "monster", id = 1 }
booster = "starter"
chance_percent = 10
"#;
        let s: CardSection = toml::from_str(text).unwrap();
        assert_eq!(s.cards[0].sides, [3, 2, 1, 4]);
        assert_eq!(s.cards[0].art, Some(DiscRef::Monster { id: 1 }));
        assert_eq!(s.boosters[0].guaranteed, Some(Rarity::Uncommon));
        assert_eq!(s.rewards[0].count, 1);
        assert_eq!(s.rewards[0].source, RewardSource::Monster { id: 1 });
        assert!(s.card("gobu-scout").is_some());
    }
}

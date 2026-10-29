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
//! ```

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

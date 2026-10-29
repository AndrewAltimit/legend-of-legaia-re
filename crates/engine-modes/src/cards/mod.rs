//! Trading cards: the card book a pack defines, a player's
//! [`Collection`], booster and reward rolls, sell values, trades, and the
//! [`duel`] card game.
//!
//! Cards are named by their pack id everywhere (collections, trades, save
//! files); the book resolves an id to its definition. Everything that
//! rolls takes the caller's [`ModeRng`], so a booster opened inside a
//! lockstep session or a replay opens the same cards everywhere.

pub mod duel;

use std::collections::BTreeMap;

use legaia_expansion_format::cards::{
    BoosterSet, CardDef, CardSection, Rarity, RarityTable, RewardSource,
};
use serde::{Deserialize, Serialize};

use crate::session::ModeRng;

/// Why a card operation was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardError {
    UnknownCard(String),
    UnknownBooster(String),
    /// The booster's pool holds no card it could roll.
    EmptyPool(String),
    /// A collection holds fewer copies than the operation needs.
    NotEnough {
        card: String,
        have: u32,
        need: u32,
    },
    /// A trade with nothing on either side.
    EmptyTrade,
    /// A binder order that is not a permutation of the owned cards.
    BadBinderOrder,
}

/// Default gold paid per card sold, by rarity, when a pack names none.
pub const DEFAULT_SELL: RarityTable = RarityTable {
    common: 10,
    uncommon: 40,
    rare: 150,
    legendary: 600,
};

/// A pack's card definitions, boosters and reward rules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardBook {
    section: CardSection,
}

/// One grant a reward roll produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardGrant {
    Booster(String),
    Card(String),
}

impl CardBook {
    pub fn new(section: CardSection) -> Self {
        Self { section }
    }

    pub fn section(&self) -> &CardSection {
        &self.section
    }

    pub fn card(&self, id: &str) -> Option<&CardDef> {
        self.section.card(id)
    }

    pub fn booster(&self, id: &str) -> Option<&BoosterSet> {
        self.section.booster(id)
    }

    /// Gold a sold copy of `id` fetches: the card's own price, else the
    /// section's rarity price, else [`DEFAULT_SELL`].
    pub fn sell_value(&self, id: &str) -> Result<u32, CardError> {
        let card = self
            .card(id)
            .ok_or_else(|| CardError::UnknownCard(id.to_string()))?;
        if let Some(p) = card.sell {
            return Ok(p);
        }
        let table = if self.section.sell_prices == RarityTable::default() {
            DEFAULT_SELL
        } else {
            self.section.sell_prices
        };
        Ok(table.get(card.rarity))
    }

    /// Sell `count` copies of `id` out of `collection`; the gold earned.
    pub fn sell(
        &self,
        collection: &mut Collection,
        id: &str,
        count: u32,
    ) -> Result<u32, CardError> {
        let each = self.sell_value(id)?;
        collection.remove(id, count)?;
        Ok(each.saturating_mul(count))
    }

    /// Open one pack of booster `id`.
    ///
    /// Each slot rolls a rarity on the set's weights - a rarity the pool
    /// has no card of weighs zero - then a card uniformly among the pool's
    /// cards of that rarity. The last slot, when the set has a
    /// `guaranteed` rarity, rolls only rarities at or above it (falling
    /// back to the ordinary roll if the pool has none).
    pub fn open_booster(&self, id: &str, rng: &mut ModeRng) -> Result<Vec<String>, CardError> {
        let set = self
            .booster(id)
            .ok_or_else(|| CardError::UnknownBooster(id.to_string()))?;
        let pool: Vec<&CardDef> = if set.pool.is_empty() {
            self.section.cards.iter().collect()
        } else {
            set.pool
                .iter()
                .map(|c| {
                    self.card(c)
                        .ok_or_else(|| CardError::UnknownCard(c.clone()))
                })
                .collect::<Result<_, _>>()?
        };
        let by_rarity: Vec<Vec<&CardDef>> = Rarity::ALL
            .iter()
            .map(|r| pool.iter().copied().filter(|c| c.rarity == *r).collect())
            .collect();
        let base: Vec<u32> = Rarity::ALL
            .iter()
            .enumerate()
            .map(|(i, r)| {
                if by_rarity[i].is_empty() {
                    0
                } else {
                    set.weights.get(*r)
                }
            })
            .collect();
        let slots = set.cards_per_pack.max(1) as usize;
        let mut out = Vec::with_capacity(slots);
        for slot in 0..slots {
            let mut w = base.clone();
            if slot + 1 == slots
                && let Some(min) = set.guaranteed
            {
                let mut g = w.clone();
                for (i, r) in Rarity::ALL.iter().enumerate() {
                    if *r < min {
                        g[i] = 0;
                    } else if g[i] == 0 && !by_rarity[i].is_empty() {
                        // A guaranteed tier with no weight still counts.
                        g[i] = 1;
                    }
                }
                if g.iter().any(|&x| x > 0) {
                    w = g;
                }
            }
            let tier = rng
                .weighted(&w)
                .or_else(|| by_rarity.iter().position(|v| !v.is_empty()))
                .ok_or_else(|| CardError::EmptyPool(id.to_string()))?;
            let cards = &by_rarity[tier];
            let pick = cards[rng.below(cards.len() as u32) as usize];
            out.push(pick.id.clone());
        }
        Ok(out)
    }

    /// Roll every reward rule matching `source`.
    pub fn roll_rewards(&self, source: &RewardSource, rng: &mut ModeRng) -> Vec<CardGrant> {
        let mut out = Vec::new();
        for rule in self.section.rewards.iter().filter(|r| &r.source == source) {
            for _ in 0..rule.count.max(1) {
                if !rng.chance(rule.chance_percent as u32) {
                    continue;
                }
                if let Some(b) = &rule.booster {
                    out.push(CardGrant::Booster(b.clone()));
                }
                if let Some(c) = &rule.card {
                    out.push(CardGrant::Card(c.clone()));
                }
            }
        }
        out
    }
}

/// A player's cards: copies per id, and the binder order they browse in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Collection {
    counts: BTreeMap<String, u32>,
    /// Every owned id once, in display order.
    binder: Vec<String>,
}

impl Collection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self, id: &str) -> u32 {
        self.counts.get(id).copied().unwrap_or(0)
    }

    /// Copies across every id.
    pub fn total(&self) -> u32 {
        self.counts.values().sum()
    }

    /// Owned ids in binder order.
    pub fn binder(&self) -> &[String] {
        &self.binder
    }

    /// Add copies; a new id goes to the end of the binder.
    pub fn add(&mut self, id: &str, n: u32) {
        if n == 0 {
            return;
        }
        let c = self.counts.entry(id.to_string()).or_insert(0);
        if *c == 0 {
            self.binder.push(id.to_string());
        }
        *c = c.saturating_add(n);
    }

    /// Remove copies; the last copy leaves the binder.
    pub fn remove(&mut self, id: &str, n: u32) -> Result<(), CardError> {
        let have = self.count(id);
        if have < n {
            return Err(CardError::NotEnough {
                card: id.to_string(),
                have,
                need: n,
            });
        }
        if have == n {
            self.counts.remove(id);
            self.binder.retain(|b| b != id);
        } else {
            self.counts.insert(id.to_string(), have - n);
        }
        Ok(())
    }

    /// Replace the binder order; `order` must list every owned id once.
    pub fn set_binder_order(&mut self, order: Vec<String>) -> Result<(), CardError> {
        let mut sorted = order.clone();
        sorted.sort();
        sorted.dedup();
        let owned: Vec<&String> = self.counts.keys().collect();
        if sorted.len() != order.len() || sorted.iter().collect::<Vec<_>>() != owned {
            return Err(CardError::BadBinderOrder);
        }
        self.binder = order;
        Ok(())
    }

    /// Move the binder entry at `from` to `to`.
    pub fn move_in_binder(&mut self, from: usize, to: usize) {
        if from < self.binder.len() && to < self.binder.len() {
            let id = self.binder.remove(from);
            self.binder.insert(to, id);
        }
    }

    /// Repair a collection read from an old save: drop zero counts, add
    /// missing binder entries, drop stale ones.
    pub fn normalize(&mut self) {
        self.counts.retain(|_, n| *n > 0);
        let counts = &self.counts;
        self.binder.retain(|id| counts.contains_key(id));
        let mut seen = std::collections::BTreeSet::new();
        self.binder.retain(|id| seen.insert(id.clone()));
        for id in self.counts.keys() {
            if !seen.contains(id) {
                self.binder.push(id.clone());
            }
        }
    }
}

/// A two-sided card trade. `give` leaves the proposer's collection for the
/// partner's, `take` the other way.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeOffer {
    pub give: Vec<(String, u32)>,
    pub take: Vec<(String, u32)>,
}

fn merged(lines: &[(String, u32)]) -> BTreeMap<&str, u32> {
    let mut m = BTreeMap::new();
    for (id, n) in lines {
        if *n > 0 {
            *m.entry(id.as_str()).or_insert(0) += n;
        }
    }
    m
}

impl TradeOffer {
    /// Check the offer against both collections (and, with a book, that
    /// every card exists).
    pub fn validate(
        &self,
        proposer: &Collection,
        partner: &Collection,
        book: Option<&CardBook>,
    ) -> Result<(), CardError> {
        let give = merged(&self.give);
        let take = merged(&self.take);
        if give.is_empty() && take.is_empty() {
            return Err(CardError::EmptyTrade);
        }
        for (side, lines) in [(proposer, &give), (partner, &take)] {
            for (&id, &n) in lines.iter() {
                if let Some(b) = book
                    && b.card(id).is_none()
                {
                    return Err(CardError::UnknownCard(id.to_string()));
                }
                let have = side.count(id);
                if have < n {
                    return Err(CardError::NotEnough {
                        card: id.to_string(),
                        have,
                        need: n,
                    });
                }
            }
        }
        Ok(())
    }

    /// Validate, then swap. Either every line moves or nothing does.
    pub fn execute(
        &self,
        proposer: &mut Collection,
        partner: &mut Collection,
        book: Option<&CardBook>,
    ) -> Result<(), CardError> {
        self.validate(proposer, partner, book)?;
        for (id, n) in merged(&self.give) {
            proposer.remove(id, n).expect("validated");
            partner.add(id, n);
        }
        for (id, n) in merged(&self.take) {
            partner.remove(id, n).expect("validated");
            proposer.add(id, n);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::cards::CardRewardRule;

    fn card(id: &str, rarity: Rarity) -> CardDef {
        CardDef {
            id: id.into(),
            name: id.into(),
            rarity,
            sides: [1, 1, 1, 1],
            ..Default::default()
        }
    }

    fn book() -> CardBook {
        CardBook::new(CardSection {
            cards: vec![
                card("c1", Rarity::Common),
                card("c2", Rarity::Common),
                card("u1", Rarity::Uncommon),
                card("r1", Rarity::Rare),
                CardDef {
                    sell: Some(999),
                    ..card("l1", Rarity::Legendary)
                },
            ],
            boosters: vec![
                BoosterSet {
                    id: "std".into(),
                    cards_per_pack: 5,
                    weights: RarityTable {
                        common: 70,
                        uncommon: 20,
                        rare: 9,
                        legendary: 1,
                    },
                    guaranteed: Some(Rarity::Rare),
                    ..Default::default()
                },
                BoosterSet {
                    id: "commons".into(),
                    cards_per_pack: 3,
                    pool: vec!["c1".into(), "c2".into()],
                    weights: RarityTable {
                        common: 1,
                        rare: 100,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
            rewards: vec![CardRewardRule {
                source: RewardSource::Monster { id: 4 },
                booster: Some("std".into()),
                card: None,
                chance_percent: 50,
                count: 2,
            }],
            sell_prices: RarityTable::default(),
        })
    }

    #[test]
    fn boosters_honour_size_pool_and_guarantee() {
        let b = book();
        let mut rng = ModeRng::new(5);
        let mut tally = BTreeMap::new();
        for _ in 0..2000 {
            let pack = b.open_booster("std", &mut rng).unwrap();
            assert_eq!(pack.len(), 5);
            let last = b.card(&pack[4]).unwrap();
            assert!(last.rarity >= Rarity::Rare, "guaranteed slot {last:?}");
            for id in &pack[..4] {
                *tally.entry(b.card(id).unwrap().rarity).or_insert(0u32) += 1;
            }
            // A rare in a commons-only pool never appears.
            for id in b.open_booster("commons", &mut rng).unwrap() {
                assert!(id.starts_with('c'));
            }
        }
        // Weighted roll: commons dominate the ordinary slots.
        assert!(tally[&Rarity::Common] > tally[&Rarity::Uncommon] * 2);
        assert!(tally[&Rarity::Uncommon] > tally[&Rarity::Rare]);
        assert_eq!(
            b.open_booster("nope", &mut rng),
            Err(CardError::UnknownBooster("nope".into()))
        );
    }

    #[test]
    fn reward_rules_roll_their_chance_per_count() {
        let b = book();
        let mut rng = ModeRng::new(1);
        let mut granted = 0;
        for _ in 0..1000 {
            granted += b
                .roll_rewards(&RewardSource::Monster { id: 4 }, &mut rng)
                .len();
            assert!(
                b.roll_rewards(&RewardSource::Monster { id: 5 }, &mut rng)
                    .is_empty()
            );
        }
        // 2 rolls at 50% per fight.
        assert!((900..1100).contains(&granted), "{granted}");
    }

    #[test]
    fn sell_values_and_selling() {
        let b = book();
        assert_eq!(b.sell_value("c1"), Ok(10));
        assert_eq!(b.sell_value("l1"), Ok(999));
        let mut col = Collection::new();
        col.add("r1", 3);
        assert_eq!(b.sell(&mut col, "r1", 2), Ok(300));
        assert_eq!(col.count("r1"), 1);
        assert!(matches!(
            b.sell(&mut col, "r1", 2),
            Err(CardError::NotEnough {
                have: 1,
                need: 2,
                ..
            })
        ));
    }

    #[test]
    fn collection_tracks_binder_order() {
        let mut col = Collection::new();
        col.add("b", 1);
        col.add("a", 2);
        col.add("b", 1);
        assert_eq!(col.binder(), ["b", "a"]);
        assert_eq!(col.total(), 4);
        col.move_in_binder(1, 0);
        assert_eq!(col.binder(), ["a", "b"]);
        assert_eq!(
            col.set_binder_order(vec!["a".into()]),
            Err(CardError::BadBinderOrder)
        );
        col.set_binder_order(vec!["b".into(), "a".into()]).unwrap();
        col.remove("b", 2).unwrap();
        assert_eq!(col.binder(), ["a"]);
        // Round-trips through serde for saves.
        let json = serde_json::to_string(&col).unwrap();
        let back: Collection = serde_json::from_str(&json).unwrap();
        assert_eq!(back, col);
        let mut broken: Collection =
            serde_json::from_str(r#"{"counts":{"x":1,"y":0},"binder":["y","y"]}"#).unwrap();
        broken.normalize();
        assert_eq!(broken.binder(), ["x"]);
        assert_eq!(broken.count("y"), 0);
    }

    #[test]
    fn trades_are_atomic() {
        let b = book();
        let mut me = Collection::new();
        let mut you = Collection::new();
        me.add("c1", 2);
        you.add("r1", 1);
        let offer = TradeOffer {
            give: vec![("c1".into(), 1), ("c1".into(), 1)],
            take: vec![("r1".into(), 1)],
        };
        offer.execute(&mut me, &mut you, Some(&b)).unwrap();
        assert_eq!(me.count("c1"), 0);
        assert_eq!(me.count("r1"), 1);
        assert_eq!(you.count("c1"), 2);
        // A short side leaves both collections untouched.
        let before = (me.clone(), you.clone());
        let bad = TradeOffer {
            give: vec![("r1".into(), 1)],
            take: vec![("c1".into(), 3)],
        };
        assert!(matches!(
            bad.execute(&mut me, &mut you, Some(&b)),
            Err(CardError::NotEnough { .. })
        ));
        assert_eq!((me.clone(), you.clone()), before);
        assert_eq!(
            TradeOffer::default().validate(&me, &you, None),
            Err(CardError::EmptyTrade)
        );
        let unknown = TradeOffer {
            give: vec![("zz".into(), 1)],
            take: vec![],
        };
        assert_eq!(
            unknown.validate(&me, &you, Some(&b)),
            Err(CardError::UnknownCard("zz".into()))
        );
    }
}

//! The card economy: what a player owns ([`CardProgress`]) and the shop
//! operations over it - buying boosters with gold, opening them, selling
//! single cards, and keeping a Grid Duel deck.
//!
//! Every roll draws from the progress's own stream ([`CardProgress::rng`]),
//! which is saved with it and never shared with the game's RNG: a booster
//! opened or a drop rolled consumes nothing a retail system reads, and a
//! reloaded save opens the same packs.

use std::collections::{BTreeMap, BTreeSet};

use legaia_expansion_format::cards::{CardDealer, RewardSource};

use super::{CardBook, CardError, CardGrant, Collection};
use crate::session::ModeRng;

/// The Grid Duel deck size.
pub const DECK_SIZE: usize = 5;

/// One player's cards, boosters and duel deck - what a save carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardProgress {
    pub collection: Collection,
    /// Unopened boosters by set id.
    pub boosters: BTreeMap<String, u32>,
    /// The chosen duel deck; used when every card in it is still owned.
    pub deck: Vec<String>,
    /// Card ids the binder or a reveal has shown; a card outside it reads
    /// as new.
    pub seen: BTreeSet<String>,
    /// The pack's `starter` cards were handed over.
    pub starter_granted: bool,
    /// The card stream's state; `0` until first seeded.
    pub rng: u64,
}

/// Why a shop operation was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShopError {
    Card(CardError),
    /// The dealer does not sell this booster.
    NotSold(String),
    /// The dealer does not buy cards.
    NotBuying,
    /// Short of gold: the price.
    Gold(u32),
    /// No unopened booster of this set.
    NoBooster(String),
}

impl From<CardError> for ShopError {
    fn from(e: CardError) -> Self {
        Self::Card(e)
    }
}

impl std::fmt::Display for ShopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Card(CardError::NotEnough { .. }) => f.write_str("You have none to sell."),
            Self::Card(e) => write!(f, "{e:?}"),
            Self::NotSold(_) => f.write_str("Not sold here."),
            Self::NotBuying => f.write_str("This dealer does not buy cards."),
            Self::Gold(p) => write!(f, "Not enough gold ({p} G)."),
            Self::NoBooster(_) => f.write_str("No booster to open."),
        }
    }
}

impl CardProgress {
    /// Seed the card stream from `seed` if it never was (a zero seed is
    /// nudged, since `0` means unseeded).
    pub fn seed_once(&mut self, seed: u64) {
        if self.rng == 0 {
            self.rng = seed | 1;
        }
    }

    /// Run `f` on the card stream and keep where it ended.
    pub fn with_rng<T>(&mut self, f: impl FnOnce(&mut ModeRng) -> T) -> T {
        let mut rng = ModeRng::new(self.rng);
        let out = f(&mut rng);
        // One further draw becomes the next state, so consecutive calls
        // never replay a stream.
        self.rng = rng.next_u64() | 1;
        out
    }

    pub fn booster_count(&self, id: &str) -> u32 {
        self.boosters.get(id).copied().unwrap_or(0)
    }

    pub fn total_boosters(&self) -> u32 {
        self.boosters.values().sum()
    }

    /// Ids owned but not yet shown.
    pub fn unseen(&self) -> usize {
        self.collection
            .binder()
            .iter()
            .filter(|id| !self.seen.contains(*id))
            .count()
    }

    /// Mark every owned card shown.
    pub fn mark_all_seen(&mut self) {
        for id in self.collection.binder() {
            self.seen.insert(id.clone());
        }
    }

    /// Credit one grant. An id the book does not know is dropped, so a
    /// stale save or a typo cannot plant a card no screen can draw.
    /// Returns whether anything was credited.
    pub fn grant(&mut self, book: &CardBook, g: &CardGrant) -> bool {
        match g {
            CardGrant::Card(id) if book.card(id).is_some() => {
                self.collection.add(id, 1);
                true
            }
            CardGrant::Booster(id) if book.booster(id).is_some() => {
                *self.boosters.entry(id.clone()).or_insert(0) += 1;
                true
            }
            _ => false,
        }
    }

    /// Hand over the book's starter cards, once.
    pub fn grant_starter(&mut self, book: &CardBook) -> Vec<String> {
        if self.starter_granted {
            return Vec::new();
        }
        self.starter_granted = true;
        let mut out = Vec::new();
        for id in &book.section().starter {
            if self.grant(book, &CardGrant::Card(id.clone())) {
                out.push(id.clone());
            }
        }
        out
    }

    /// Roll every reward rule matching each of `sources` on the card
    /// stream; returns what was credited.
    pub fn roll_and_grant(&mut self, book: &CardBook, sources: &[RewardSource]) -> Vec<CardGrant> {
        if sources.is_empty() || book.section().rewards.is_empty() {
            return Vec::new();
        }
        let grants = self.with_rng(|rng| {
            sources
                .iter()
                .flat_map(|s| book.roll_rewards(s, rng))
                .collect::<Vec<_>>()
        });
        grants.into_iter().filter(|g| self.grant(book, g)).collect()
    }

    /// Buy one `booster` from `dealer`, paying out of `gold`. Returns the
    /// price.
    pub fn buy_booster(
        &mut self,
        book: &CardBook,
        dealer: &CardDealer,
        booster: &str,
        gold: &mut i32,
    ) -> Result<u32, ShopError> {
        let set = book
            .section()
            .dealer_stock(dealer)
            .into_iter()
            .find(|b| b.id == booster)
            .ok_or_else(|| ShopError::NotSold(booster.to_string()))?;
        let price = set.price.unwrap_or(0);
        if i64::from(*gold) < i64::from(price) {
            return Err(ShopError::Gold(price));
        }
        *gold -= price as i32;
        *self.boosters.entry(booster.to_string()).or_insert(0) += 1;
        Ok(price)
    }

    /// What `dealer` pays for one copy of `card`.
    pub fn sell_price(book: &CardBook, dealer: &CardDealer, card: &str) -> Result<u32, CardError> {
        let v = book.sell_value(card)?;
        Ok((u64::from(v) * u64::from(dealer.sell_percent) / 100) as u32)
    }

    /// Sell one copy of `card` to `dealer`; the gold goes to `gold`
    /// (clamped at `gold_cap`). Returns the gold paid.
    pub fn sell_card(
        &mut self,
        book: &CardBook,
        dealer: &CardDealer,
        card: &str,
        gold: &mut i32,
        gold_cap: i32,
    ) -> Result<u32, ShopError> {
        if !dealer.buys_cards {
            return Err(ShopError::NotBuying);
        }
        let price = Self::sell_price(book, dealer, card)?;
        self.collection.remove(card, 1)?;
        *gold = (i64::from(*gold) + i64::from(price)).clamp(0, i64::from(gold_cap)) as i32;
        Ok(price)
    }

    /// Open one unopened `booster`: its cards join the collection, in pack
    /// order. Returns them.
    pub fn open_booster(
        &mut self,
        book: &CardBook,
        booster: &str,
    ) -> Result<Vec<String>, ShopError> {
        if self.booster_count(booster) == 0 {
            return Err(ShopError::NoBooster(booster.to_string()));
        }
        let cards = self.with_rng(|rng| book.open_booster(booster, rng))?;
        let left = self.booster_count(booster) - 1;
        if left == 0 {
            self.boosters.remove(booster);
        } else {
            self.boosters.insert(booster.to_string(), left);
        }
        for c in &cards {
            self.collection.add(c, 1);
        }
        Ok(cards)
    }

    /// The deck the Grid Duel deals: the chosen deck while it is whole and
    /// owned, else the strongest five owned cards. `None` when the
    /// collection holds fewer than five cards (the duel then lends its
    /// starter deck).
    pub fn duel_deck(&self, book: &CardBook) -> Option<Vec<String>> {
        if self.deck.len() == DECK_SIZE && book.deck_is_owned(&self.collection, &self.deck) {
            return Some(self.deck.clone());
        }
        let deck = book.strongest_deck(&self.collection, DECK_SIZE);
        (deck.len() == DECK_SIZE).then_some(deck)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::cards::{
        BoosterSet, CardDef, CardRewardRule, CardSection, Rarity, RarityTable,
    };

    fn book() -> CardBook {
        let card = |id: &str, r, s: u8| CardDef {
            id: id.into(),
            name: id.into(),
            rarity: r,
            sides: [s; 4],
            ..Default::default()
        };
        CardBook::new(CardSection {
            cards: vec![
                card("a", Rarity::Common, 1),
                card("b", Rarity::Common, 2),
                card("c", Rarity::Uncommon, 3),
                card("d", Rarity::Rare, 4),
                card("e", Rarity::Legendary, 5),
                card("f", Rarity::Common, 1),
            ],
            boosters: vec![
                BoosterSet {
                    id: "pk".into(),
                    cards_per_pack: 3,
                    weights: RarityTable {
                        common: 1,
                        uncommon: 1,
                        rare: 1,
                        legendary: 1,
                    },
                    price: Some(100),
                    ..Default::default()
                },
                BoosterSet {
                    id: "free".into(),
                    cards_per_pack: 1,
                    weights: RarityTable {
                        common: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
            rewards: vec![CardRewardRule {
                source: RewardSource::Monster { id: 9 },
                booster: None,
                card: Some("a".into()),
                chance_percent: 100,
                count: 1,
            }],
            starter: vec!["a".into(), "b".into(), "zz".into()],
            ..Default::default()
        })
    }

    #[test]
    fn buying_charges_gold_and_only_sells_priced_stock() {
        let b = book();
        let dealer = CardDealer::default();
        let mut p = CardProgress::default();
        let mut gold = 150;
        assert_eq!(p.buy_booster(&b, &dealer, "pk", &mut gold), Ok(100));
        assert_eq!((gold, p.booster_count("pk")), (50, 1));
        assert_eq!(
            p.buy_booster(&b, &dealer, "pk", &mut gold),
            Err(ShopError::Gold(100))
        );
        assert_eq!(
            p.buy_booster(&b, &dealer, "free", &mut gold),
            Err(ShopError::NotSold("free".into()))
        );
        assert_eq!(gold, 50);
    }

    #[test]
    fn opening_consumes_the_booster_and_reproduces_from_the_saved_stream() {
        let b = book();
        let mut p = CardProgress::default();
        p.seed_once(42);
        p.boosters.insert("pk".into(), 2);
        let saved = p.clone();
        let first = p.open_booster(&b, "pk").unwrap();
        assert_eq!(first.len(), 3);
        assert_eq!(p.collection.total(), 3);
        assert_eq!(p.booster_count("pk"), 1);
        // The same saved state opens the same pack.
        let mut again = saved;
        assert_eq!(again.open_booster(&b, "pk").unwrap(), first);
        // The stream moves on: the second pack is rolled from a new state.
        let rng_after_one = p.rng;
        p.open_booster(&b, "pk").unwrap();
        assert_ne!(p.rng, rng_after_one);
        assert!(!p.boosters.contains_key("pk"));
        assert_eq!(
            p.open_booster(&b, "pk"),
            Err(ShopError::NoBooster("pk".into()))
        );
    }

    #[test]
    fn selling_pays_the_dealer_rate_and_respects_the_cap() {
        let b = book();
        let mut dealer = CardDealer {
            sell_percent: 50,
            ..Default::default()
        };
        let mut p = CardProgress::default();
        p.collection.add("d", 2);
        let mut gold = 0;
        // Rare default value 150, at half.
        assert_eq!(p.sell_card(&b, &dealer, "d", &mut gold, 9_999_999), Ok(75));
        assert_eq!(p.sell_card(&b, &dealer, "d", &mut gold, 100), Ok(75));
        assert_eq!(gold, 100, "clamped at the cap");
        assert!(p.sell_card(&b, &dealer, "d", &mut gold, 100).is_err());
        dealer.buys_cards = false;
        p.collection.add("d", 1);
        assert_eq!(
            p.sell_card(&b, &dealer, "d", &mut gold, 100),
            Err(ShopError::NotBuying)
        );
    }

    #[test]
    fn grants_drop_unknown_ids_and_the_starter_comes_once() {
        let b = book();
        let mut p = CardProgress::default();
        assert_eq!(p.grant_starter(&b), vec!["a", "b"]);
        assert!(p.grant_starter(&b).is_empty());
        assert!(!p.grant(&b, &CardGrant::Card("zz".into())));
        assert!(p.grant(&b, &CardGrant::Booster("free".into())));
        assert_eq!(p.unseen(), 2);
        p.mark_all_seen();
        assert_eq!(p.unseen(), 0);
        let got = p.roll_and_grant(&b, &[RewardSource::Monster { id: 9 }]);
        assert_eq!(got, vec![CardGrant::Card("a".into())]);
        assert_eq!(p.collection.count("a"), 2);
    }

    #[test]
    fn the_duel_deck_is_the_chosen_one_while_owned_else_the_strongest() {
        let b = book();
        let mut p = CardProgress::default();
        for id in ["a", "b", "c", "d"] {
            p.collection.add(id, 1);
        }
        assert_eq!(p.duel_deck(&b), None, "four cards are not a deck");
        p.collection.add("a", 2);
        assert_eq!(
            p.duel_deck(&b).unwrap(),
            vec!["d", "c", "b", "a", "a"],
            "strongest first, copies fill slots"
        );
        p.deck = vec!["a".into(), "a".into(), "a".into(), "b".into(), "c".into()];
        assert_eq!(p.duel_deck(&b).unwrap(), p.deck);
        p.collection.remove("a", 1).unwrap();
        assert_eq!(p.duel_deck(&b).unwrap()[0], "d", "no longer owned");
    }
}

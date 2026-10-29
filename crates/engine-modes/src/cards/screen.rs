//! The card screens: the binder, booster opening and its reveal, the card
//! dealer (buy boosters, sell singles) and the duel deck builder, as one
//! pad-driven state machine over a [`CardProgress`].
//!
//! The screen holds only where the player is; the cards, boosters and gold
//! are the caller's, handed in each frame, so the operations a screen runs
//! are the [`CardProgress`] ones a test can call directly.
//!
//! Controls: Up / Down / Left / Right move, Cross picks, Circle goes back a
//! screen (and leaves from the first), Start leaves from anywhere. In the
//! binder L1 / R1 turn the page, Square cycles the sort, Triangle the
//! filter, and Cross opens the card's detail. In the deck builder Cross adds
//! the card, Square takes the last one out, Triangle fills the deck with the
//! strongest cards owned, and Circle keeps a full deck. In a reveal Cross
//! turns the next card and Triangle turns them all.

use std::collections::BTreeSet;

use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::cards::{CardDealer, Rarity};

use super::CardBook;
use super::economy::{CardProgress, DECK_SIZE};

/// Cards on one binder page.
pub const BINDER_COLUMNS: usize = 4;
pub const BINDER_ROWS: usize = 2;
pub const BINDER_PAGE: usize = BINDER_COLUMNS * BINDER_ROWS;

/// The binder's sort.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BinderSort {
    /// The pack's card order.
    #[default]
    Book,
    /// The collection's own order (cards as they were gained).
    Gained,
    /// Rarest first.
    Rarity,
    /// Strongest (side sum) first.
    Strength,
    /// Most copies first.
    Copies,
}

impl BinderSort {
    const ALL: [Self; 5] = [
        Self::Book,
        Self::Gained,
        Self::Rarity,
        Self::Strength,
        Self::Copies,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Book => "Set order",
            Self::Gained => "Newest last",
            Self::Rarity => "Rarity",
            Self::Strength => "Strength",
            Self::Copies => "Copies",
        }
    }

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// Which cards the binder lists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BinderFilter {
    /// Every card of the set; missing ones as blank slots.
    #[default]
    All,
    Owned,
    Missing,
    Rarity(Rarity),
}

impl BinderFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All cards",
            Self::Owned => "Owned",
            Self::Missing => "Missing",
            Self::Rarity(Rarity::Common) => "Common",
            Self::Rarity(Rarity::Uncommon) => "Uncommon",
            Self::Rarity(Rarity::Rare) => "Rare",
            Self::Rarity(Rarity::Legendary) => "Legendary",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::All => Self::Owned,
            Self::Owned => Self::Missing,
            Self::Missing => Self::Rarity(Rarity::Common),
            Self::Rarity(Rarity::Common) => Self::Rarity(Rarity::Uncommon),
            Self::Rarity(Rarity::Uncommon) => Self::Rarity(Rarity::Rare),
            Self::Rarity(Rarity::Rare) => Self::Rarity(Rarity::Legendary),
            Self::Rarity(Rarity::Legendary) => Self::All,
        }
    }
}

/// One binder slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinderEntry {
    pub id: String,
    pub count: u32,
    /// Owned and never shown before.
    pub fresh: bool,
}

/// The binder's slots under `sort` and `filter`.
pub fn binder_entries(
    book: &CardBook,
    progress: &CardProgress,
    sort: BinderSort,
    filter: BinderFilter,
) -> Vec<BinderEntry> {
    let col = &progress.collection;
    let cards = &book.section().cards;
    let mut order: Vec<usize> = (0..cards.len()).collect();
    match sort {
        BinderSort::Book => {}
        BinderSort::Gained => {
            let pos = |id: &str| {
                col.binder()
                    .iter()
                    .position(|b| b == id)
                    .unwrap_or(usize::MAX)
            };
            order.sort_by_key(|&i| pos(&cards[i].id));
        }
        BinderSort::Rarity => order.sort_by_key(|&i| std::cmp::Reverse(cards[i].rarity)),
        BinderSort::Strength => order.sort_by_key(|&i| {
            std::cmp::Reverse(cards[i].sides.iter().map(|&s| u32::from(s)).sum::<u32>())
        }),
        BinderSort::Copies => order.sort_by_key(|&i| std::cmp::Reverse(col.count(&cards[i].id))),
    }
    order
        .into_iter()
        .map(|i| &cards[i])
        .filter(|c| {
            let n = col.count(&c.id);
            match filter {
                BinderFilter::All => true,
                BinderFilter::Owned => n > 0,
                BinderFilter::Missing => n == 0,
                BinderFilter::Rarity(r) => c.rarity == r,
            }
        })
        .map(|c| {
            let count = col.count(&c.id);
            BinderEntry {
                id: c.id.clone(),
                count,
                fresh: count > 0 && !progress.seen.contains(&c.id),
            }
        })
        .collect()
}

/// A row of the card menu or the dealer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuRow {
    Binder,
    Boosters,
    Sell,
    Deck,
    /// Buy one of this booster set.
    Buy(String),
}

/// Where the card screen is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardStage {
    /// The card menu (opened from the pause menu) or the dealer's counter.
    Menu { cursor: usize },
    Binder {
        cursor: usize,
        sort: BinderSort,
        filter: BinderFilter,
        detail: bool,
    },
    /// Unopened boosters.
    Boosters { cursor: usize },
    /// A booster just opened: `shown` of its cards are face up.
    Reveal {
        booster: String,
        cards: Vec<String>,
        /// Per card: never owned before this pack.
        fresh: Vec<bool>,
        shown: usize,
    },
    /// Owned cards with what the dealer pays.
    Sell { cursor: usize },
    /// The duel deck builder: `deck` is the deck being built.
    Deck { cursor: usize, deck: Vec<String> },
}

/// What one [`CardScreen::step`] asks of the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenStep {
    Stay,
    /// Close the screen.
    Leave,
}

/// The card screen's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardScreen {
    pub stage: CardStage,
    /// The screens Circle returns to, innermost last.
    back: Vec<CardStage>,
    /// The dealer whose counter this is; `None` for the pause-menu card
    /// menu, which sells at the pack's first dealer's rate.
    pub dealer: Option<CardDealer>,
    /// The last operation's outcome line.
    pub message: Option<String>,
    /// Buttons held since the screen gained the pad, withheld until
    /// released.
    pub held_over: u16,
}

impl CardScreen {
    fn new(stage: CardStage, dealer: Option<CardDealer>) -> Self {
        Self {
            stage,
            back: Vec::new(),
            dealer,
            message: None,
            held_over: u16::MAX,
        }
    }

    /// The card menu.
    pub fn menu() -> Self {
        Self::new(CardStage::Menu { cursor: 0 }, None)
    }

    /// A dealer's counter.
    pub fn at_dealer(dealer: CardDealer) -> Self {
        Self::new(CardStage::Menu { cursor: 0 }, Some(dealer))
    }

    /// Straight into the binder.
    pub fn binder() -> Self {
        Self::new(
            CardStage::Binder {
                cursor: 0,
                sort: BinderSort::default(),
                filter: BinderFilter::default(),
                detail: false,
            },
            None,
        )
    }

    /// The dealer selling and buying here: the counter's own, else the
    /// pack's first.
    pub fn trading_dealer(&self, book: &CardBook) -> CardDealer {
        self.dealer
            .clone()
            .or_else(|| book.section().dealer(""))
            .unwrap_or_default()
    }

    /// The menu's rows: at a dealer its stock first, then the card rows.
    pub fn menu_rows(&self, book: &CardBook) -> Vec<MenuRow> {
        let mut rows = Vec::new();
        if let Some(d) = &self.dealer {
            for b in book.section().dealer_stock(d) {
                rows.push(MenuRow::Buy(b.id.clone()));
            }
            if d.buys_cards {
                rows.push(MenuRow::Sell);
            }
            rows.push(MenuRow::Boosters);
            rows.push(MenuRow::Binder);
        } else {
            rows.extend([
                MenuRow::Binder,
                MenuRow::Boosters,
                MenuRow::Sell,
                MenuRow::Deck,
            ]);
        }
        rows
    }

    /// Owned booster ids, in book order.
    pub fn booster_rows(book: &CardBook, progress: &CardProgress) -> Vec<String> {
        book.section()
            .boosters
            .iter()
            .filter(|b| progress.booster_count(&b.id) > 0)
            .map(|b| b.id.clone())
            .collect()
    }

    /// Owned card ids in the collection's order - the sell and deck lists.
    pub fn owned_rows(progress: &CardProgress) -> Vec<String> {
        progress.collection.binder().to_vec()
    }

    fn push(&mut self, next: CardStage) {
        let prev = std::mem::replace(&mut self.stage, next);
        self.back.push(prev);
    }

    /// Go back a screen; `Leave` from the first.
    fn pop(&mut self, progress: &mut CardProgress) -> ScreenStep {
        if matches!(
            self.stage,
            CardStage::Binder { .. } | CardStage::Reveal { .. }
        ) {
            progress.mark_all_seen();
        }
        match self.back.pop() {
            Some(s) => {
                self.stage = s;
                ScreenStep::Stay
            }
            None => ScreenStep::Leave,
        }
    }

    /// One frame. `gold` is the party purse; `gold_cap` its ceiling.
    pub fn step(
        &mut self,
        pad: u16,
        prev: u16,
        book: &CardBook,
        progress: &mut CardProgress,
        gold: &mut i32,
        gold_cap: i32,
    ) -> ScreenStep {
        self.held_over &= pad;
        let live = pad & !self.held_over;
        let pressed = |b: PadButton| live & !prev & b.mask() != 0;
        if pressed(PadButton::Start) {
            progress.mark_all_seen();
            return ScreenStep::Leave;
        }
        if pressed(PadButton::Circle) && !self.circle_is_local() {
            self.message = None;
            return self.pop(progress);
        }
        let dir = |n: usize, cursor: &mut usize, up: PadButton, down: PadButton| {
            if n == 0 {
                *cursor = 0;
                return;
            }
            if pressed(up) {
                *cursor = (*cursor + n - 1) % n;
            }
            if pressed(down) {
                *cursor = (*cursor + 1) % n;
            }
            *cursor = (*cursor).min(n - 1);
        };
        let menu_rows = self.menu_rows(book);
        match &mut self.stage {
            CardStage::Menu { cursor } => {
                let rows = menu_rows;
                dir(rows.len(), cursor, PadButton::Up, PadButton::Down);
                let pick = if pressed(PadButton::Cross) {
                    rows.get(*cursor).cloned()
                } else {
                    None
                };
                if let Some(row) = pick {
                    self.menu_pick(row, book, progress, gold);
                }
            }
            CardStage::Binder {
                cursor,
                sort,
                filter,
                detail,
            } => {
                let n = binder_entries(book, progress, *sort, *filter).len();
                if *detail {
                    dir(n, cursor, PadButton::Left, PadButton::Right);
                    if pressed(PadButton::Circle) || pressed(PadButton::Cross) {
                        *detail = false;
                    }
                } else {
                    if n > 0 {
                        if pressed(PadButton::Left) {
                            *cursor = (*cursor + n - 1) % n;
                        }
                        if pressed(PadButton::Right) {
                            *cursor = (*cursor + 1) % n;
                        }
                        if pressed(PadButton::Up) {
                            *cursor = cursor.saturating_sub(BINDER_COLUMNS);
                        }
                        if pressed(PadButton::Down) {
                            *cursor = (*cursor + BINDER_COLUMNS).min(n - 1);
                        }
                        if pressed(PadButton::L1) {
                            *cursor = cursor.saturating_sub(BINDER_PAGE);
                        }
                        if pressed(PadButton::R1) {
                            *cursor = (*cursor + BINDER_PAGE).min(n - 1);
                        }
                        if pressed(PadButton::Cross) {
                            *detail = true;
                        }
                    }
                    if pressed(PadButton::Square) {
                        *sort = sort.next();
                        *cursor = 0;
                    }
                    if pressed(PadButton::Triangle) {
                        *filter = filter.next();
                        *cursor = 0;
                    }
                    if pressed(PadButton::Circle) {
                        return self.pop(progress);
                    }
                }
            }
            CardStage::Boosters { cursor } => {
                let rows = Self::booster_rows(book, progress);
                dir(rows.len(), cursor, PadButton::Up, PadButton::Down);
                if pressed(PadButton::Cross)
                    && let Some(id) = rows.get(*cursor).cloned()
                {
                    let before: BTreeSet<String> = progress
                        .collection
                        .binder()
                        .iter()
                        .chain(progress.seen.iter())
                        .cloned()
                        .collect();
                    match progress.open_booster(book, &id) {
                        Ok(cards) => {
                            let mut known = before;
                            let fresh = cards.iter().map(|c| known.insert(c.clone())).collect();
                            self.message = None;
                            self.push(CardStage::Reveal {
                                booster: id,
                                cards,
                                fresh,
                                shown: 0,
                            });
                        }
                        Err(e) => self.message = Some(e.to_string()),
                    }
                }
            }
            CardStage::Reveal { cards, shown, .. } => {
                if pressed(PadButton::Triangle) {
                    *shown = cards.len();
                } else if pressed(PadButton::Cross) {
                    if *shown < cards.len() {
                        *shown += 1;
                    } else {
                        return self.pop(progress);
                    }
                }
            }
            CardStage::Sell { cursor } => {
                let rows = Self::owned_rows(progress);
                dir(rows.len(), cursor, PadButton::Up, PadButton::Down);
                if pressed(PadButton::Cross)
                    && let Some(id) = rows.get(*cursor).cloned()
                {
                    let dealer = self.trading_dealer(book);
                    let name = card_name(book, &id);
                    self.message = Some(
                        match progress.sell_card(book, &dealer, &id, gold, gold_cap) {
                            Ok(p) => format!("Sold {name} for {p} G."),
                            Err(e) => e.to_string(),
                        },
                    );
                }
            }
            CardStage::Deck { cursor, deck } => {
                let rows = Self::owned_rows(progress);
                dir(rows.len(), cursor, PadButton::Up, PadButton::Down);
                if pressed(PadButton::Cross)
                    && let Some(id) = rows.get(*cursor)
                {
                    let used = deck.iter().filter(|d| *d == id).count() as u32;
                    if deck.len() >= DECK_SIZE {
                        self.message = Some("The deck is full.".into());
                    } else if used >= progress.collection.count(id) {
                        self.message = Some("Every copy is in the deck.".into());
                    } else {
                        deck.push(id.clone());
                        self.message = None;
                    }
                }
                if pressed(PadButton::Square) {
                    deck.pop();
                }
                if pressed(PadButton::Triangle) {
                    *deck = book.strongest_deck(&progress.collection, DECK_SIZE);
                }
                if pressed(PadButton::Circle) {
                    if deck.len() == DECK_SIZE {
                        progress.deck = deck.clone();
                        self.message = Some("Deck saved.".into());
                    } else {
                        self.message = Some("Deck not saved: it needs five cards.".into());
                    }
                    return self.pop(progress);
                }
            }
        }
        ScreenStep::Stay
    }

    fn menu_pick(
        &mut self,
        row: MenuRow,
        book: &CardBook,
        progress: &mut CardProgress,
        gold: &mut i32,
    ) {
        self.message = None;
        match row {
            MenuRow::Binder => self.push(CardStage::Binder {
                cursor: 0,
                sort: BinderSort::default(),
                filter: BinderFilter::default(),
                detail: false,
            }),
            MenuRow::Boosters => self.push(CardStage::Boosters { cursor: 0 }),
            MenuRow::Sell => self.push(CardStage::Sell { cursor: 0 }),
            MenuRow::Deck => {
                let deck = progress.duel_deck(book).unwrap_or_default();
                self.push(CardStage::Deck { cursor: 0, deck });
            }
            MenuRow::Buy(id) => {
                let dealer = self.trading_dealer(book);
                let name = booster_name(book, &id);
                self.message = Some(match progress.buy_booster(book, &dealer, &id, gold) {
                    Ok(p) => format!("Bought {name} for {p} G."),
                    Err(e) => e.to_string(),
                });
            }
        }
    }

    /// Whether this stage handles Circle itself.
    fn circle_is_local(&self) -> bool {
        matches!(
            self.stage,
            CardStage::Binder { .. } | CardStage::Deck { .. }
        )
    }
}

/// A card's display name (its id when the book has none).
pub fn card_name(book: &CardBook, id: &str) -> String {
    book.card(id)
        .map(|c| c.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| id.to_string())
}

/// A booster set's display name (its id when it has none).
pub fn booster_name(book: &CardBook, id: &str) -> String {
    book.booster(id)
        .map(|b| b.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::cards::{BoosterSet, CardDef, CardSection, RarityTable};

    fn book() -> CardBook {
        let card = |id: &str, r, s: u8| CardDef {
            id: id.into(),
            name: id.to_uppercase(),
            rarity: r,
            sides: [s; 4],
            ..Default::default()
        };
        CardBook::new(CardSection {
            cards: (0..10)
                .map(|i| {
                    card(
                        &format!("c{i}"),
                        if i < 6 { Rarity::Common } else { Rarity::Rare },
                        (i % 9 + 1) as u8,
                    )
                })
                .collect(),
            boosters: vec![BoosterSet {
                id: "pk".into(),
                name: "Pack".into(),
                cards_per_pack: 3,
                weights: RarityTable {
                    common: 1,
                    rare: 1,
                    ..Default::default()
                },
                price: Some(50),
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    struct Pad {
        prev: u16,
    }

    impl Pad {
        fn press(
            &mut self,
            s: &mut CardScreen,
            b: PadButton,
            book: &CardBook,
            p: &mut CardProgress,
            gold: &mut i32,
        ) -> ScreenStep {
            s.step(0, self.prev, book, p, gold, 9_999_999);
            let r = s.step(b.mask(), 0, book, p, gold, 9_999_999);
            self.prev = b.mask();
            r
        }
    }

    #[test]
    fn the_binder_sorts_filters_and_marks_fresh_cards() {
        let b = book();
        let mut p = CardProgress::default();
        p.collection.add("c9", 1);
        p.collection.add("c1", 3);
        let all = binder_entries(&b, &p, BinderSort::Book, BinderFilter::All);
        assert_eq!(all.len(), 10);
        assert_eq!((all[1].count, all[1].fresh), (3, true));
        let owned = binder_entries(&b, &p, BinderSort::Gained, BinderFilter::Owned);
        assert_eq!(
            owned.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["c9", "c1"]
        );
        let copies = binder_entries(&b, &p, BinderSort::Copies, BinderFilter::Owned);
        assert_eq!(copies[0].id, "c1");
        let rare = binder_entries(&b, &p, BinderSort::Book, BinderFilter::Rarity(Rarity::Rare));
        assert_eq!(rare.len(), 4);
        assert_eq!(
            binder_entries(&b, &p, BinderSort::Book, BinderFilter::Missing).len(),
            8
        );
        p.mark_all_seen();
        assert!(
            binder_entries(&b, &p, BinderSort::Book, BinderFilter::Owned)
                .iter()
                .all(|e| !e.fresh)
        );
    }

    #[test]
    fn a_dealer_sells_then_the_booster_opens_into_a_reveal() {
        let b = book();
        let mut p = CardProgress::default();
        p.seed_once(3);
        let mut gold = 120;
        let mut pad = Pad { prev: 0 };
        let mut s = CardScreen::at_dealer(CardDealer::default());
        assert_eq!(
            s.menu_rows(&b),
            [
                MenuRow::Buy("pk".into()),
                MenuRow::Sell,
                MenuRow::Boosters,
                MenuRow::Binder
            ]
        );
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        assert_eq!(gold, 70);
        assert_eq!(s.message.as_deref(), Some("Bought Pack for 50 G."));
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        assert_eq!(gold, 20);
        assert_eq!(s.message.as_deref(), Some("Not enough gold (50 G)."));
        // Down twice to "Open boosters", then open the first.
        pad.press(&mut s, PadButton::Down, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Down, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        assert!(matches!(s.stage, CardStage::Boosters { .. }));
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        let CardStage::Reveal { cards, fresh, .. } = s.stage.clone() else {
            panic!("{:?}", s.stage)
        };
        assert_eq!(cards.len(), 3);
        assert!(fresh[0], "the first card of an empty collection is new");
        assert_eq!(p.collection.total(), 3);
        assert_eq!(p.booster_count("pk"), 1);
        for _ in 0..3 {
            pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        }
        assert!(matches!(s.stage, CardStage::Reveal { shown: 3, .. }));
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        assert!(matches!(s.stage, CardStage::Boosters { .. }));
        assert_eq!(p.unseen(), 0, "a finished reveal marks its cards seen");
        pad.press(&mut s, PadButton::Circle, &b, &mut p, &mut gold);
        assert_eq!(
            pad.press(&mut s, PadButton::Circle, &b, &mut p, &mut gold),
            ScreenStep::Leave
        );
    }

    #[test]
    fn selling_from_the_card_menu_pays_gold() {
        let b = book();
        let mut p = CardProgress::default();
        p.collection.add("c7", 2);
        let mut gold = 0;
        let mut pad = Pad { prev: 0 };
        let mut s = CardScreen::menu();
        pad.press(&mut s, PadButton::Down, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Down, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        assert!(matches!(s.stage, CardStage::Sell { .. }));
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        assert_eq!((gold, p.collection.count("c7")), (150, 1));
    }

    #[test]
    fn the_deck_builder_keeps_only_a_full_owned_deck() {
        let b = book();
        let mut p = CardProgress::default();
        p.collection.add("c1", 1);
        p.collection.add("c2", 5);
        let mut gold = 0;
        let mut pad = Pad { prev: 0 };
        let mut s = CardScreen::menu();
        for _ in 0..3 {
            pad.press(&mut s, PadButton::Down, &b, &mut p, &mut gold);
        }
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        let CardStage::Deck { deck, .. } = &s.stage else {
            panic!()
        };
        assert_eq!(deck.len(), 5, "starts from the strongest owned deck");
        // Five copies of the stronger c2; swap the last for c1 (first row).
        pad.press(&mut s, PadButton::Square, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        assert_eq!(s.message.as_deref(), Some("The deck is full."));
        pad.press(&mut s, PadButton::Square, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Square, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        assert_eq!(s.message.as_deref(), Some("Every copy is in the deck."));
        pad.press(&mut s, PadButton::Down, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        pad.press(&mut s, PadButton::Circle, &b, &mut p, &mut gold);
        assert_eq!(p.deck.len(), 5);
        assert_eq!(p.deck.iter().filter(|d| *d == "c2").count(), 4);
        assert!(matches!(s.stage, CardStage::Menu { .. }));
    }

    #[test]
    fn the_binder_pages_and_opens_details() {
        let b = book();
        let mut p = CardProgress::default();
        let mut gold = 0;
        let mut pad = Pad { prev: 0 };
        let mut s = CardScreen::binder();
        pad.press(&mut s, PadButton::R1, &b, &mut p, &mut gold);
        assert!(matches!(s.stage, CardStage::Binder { cursor: 8, .. }));
        pad.press(&mut s, PadButton::Cross, &b, &mut p, &mut gold);
        assert!(matches!(s.stage, CardStage::Binder { detail: true, .. }));
        pad.press(&mut s, PadButton::Right, &b, &mut p, &mut gold);
        assert!(matches!(s.stage, CardStage::Binder { cursor: 9, .. }));
        pad.press(&mut s, PadButton::Circle, &b, &mut p, &mut gold);
        assert!(matches!(s.stage, CardStage::Binder { detail: false, .. }));
        pad.press(&mut s, PadButton::Square, &b, &mut p, &mut gold);
        assert!(matches!(
            s.stage,
            CardStage::Binder {
                sort: BinderSort::Gained,
                cursor: 0,
                ..
            }
        ));
        assert_eq!(
            pad.press(&mut s, PadButton::Circle, &b, &mut p, &mut gold),
            ScreenStep::Leave
        );
    }
}

//! Texas Hold'em for 2-6 seats.
//!
//! No-limit, with blinds, four betting streets, side pots and a showdown.
//! The table is a [`ModeSession`]: a human seat steers an action menu with
//! the pad (Left / Right picks Fold / Check-Call / Raise / All-in, Up /
//! Down sizes the raise in big-blind steps, Cross commits), and AI seats
//! act after a short think delay. [`PokerSession::act`] is the same entry
//! point without the menu, for tests and for a host that collects actions
//! some other way.
//!
//! Betting rules the kernel enforces:
//!
//! - the seat after the button posts the small blind and the next the big
//!   blind; heads-up, the button posts the small blind and acts first
//!   preflop, last afterwards;
//! - preflop the current bet is the big blind (even when the big blind is
//!   all-in for less), and the big blind keeps its option to raise;
//! - a raise must add at least the last full raise (the big blind at the
//!   start of a street);
//! - an all-in that raises by less than that is a short raise: it makes
//!   every other seat act again, but a seat that had already acted on the
//!   street may only call or fold;
//! - when at most one seat can still bet, the remaining board is dealt
//!   straight to showdown;
//! - each pot ([`pots::build_pots`]) goes to the best hand among its
//!   eligible seats; a split pot's odd chips go one at a time to the tied
//!   winners nearest the button's left.
//!
//! The session ends when one seat holds every chip, every human seat has
//! busted, or `max_hands` is reached.

pub mod ai;
pub mod hand;
pub mod pots;

use legaia_engine_system::pad::PadButton;
use serde::{Deserialize, Serialize};

use crate::session::{
    Currency, ModeResults, ModeRng, ModeSession, PadEdges, PadWord, Reward, SeatKind, StateHasher,
};
use hand::{Card, HandValue, best_hand};

/// Fewest seats at a table.
pub const MIN_SEATS: usize = 2;
/// Most seats at a table.
pub const MAX_TABLE_SEATS: usize = 6;

/// A table's setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PokerConfig {
    pub seats: Vec<SeatKind>,
    pub buy_in: u32,
    pub small_blind: u32,
    pub big_blind: u32,
    pub currency: Currency,
    pub chip_value: u32,
    pub max_hands: Option<u32>,
    pub blind_double_every: Option<u32>,
    /// Frames an AI seat waits before acting.
    pub ai_think_frames: u32,
    /// Frames the showdown stays up before the next hand.
    pub showdown_frames: u32,
    pub seed: u64,
}

impl Default for PokerConfig {
    fn default() -> Self {
        Self {
            seats: vec![SeatKind::Human, SeatKind::Ai(Default::default())],
            buy_in: 1000,
            small_blind: 10,
            big_blind: 20,
            currency: Currency::Gold,
            chip_value: 1,
            max_hands: None,
            blind_double_every: None,
            ai_think_frames: 30,
            showdown_frames: 120,
            seed: 0,
        }
    }
}

impl PokerConfig {
    /// Build from a pack's table: seat 0 is the player, the rest AI at the
    /// table's tier.
    pub fn from_table(t: &legaia_expansion_format::modes::PokerTable, seed: u64) -> Self {
        let n = (t.seats as usize).clamp(MIN_SEATS, MAX_TABLE_SEATS);
        let mut seats = vec![SeatKind::Human];
        seats.extend(std::iter::repeat_n(SeatKind::Ai(t.ai_tier.into()), n - 1));
        Self {
            seats,
            buy_in: t.buy_in.max(1),
            small_blind: t.small_blind.max(1),
            big_blind: t.big_blind.max(t.small_blind.max(1)),
            currency: t.currency.into(),
            chip_value: t.chip_value.max(1),
            max_hands: t.max_hands,
            blind_double_every: t.blind_double_every,
            seed,
            ..Self::default()
        }
    }
}

/// A betting street.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
}

/// A betting action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Fold,
    Check,
    Call,
    /// Raise (or open-bet) so the seat's street total becomes this.
    RaiseTo(u32),
    AllIn,
}

/// Why [`PokerSession::act`] refused an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionError {
    NotYourTurn,
    NotBetting,
    CannotCheck,
    NothingToCall,
    RaiseTooSmall,
    RaiseTooLarge,
    RaiseClosed,
}

/// Where the table is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    Betting(Street),
    /// The hand is over; `timer` frames until the next deal.
    Showdown {
        timer: u32,
    },
    Finished,
}

/// What a tick reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PokerEvent {
    HandStarted {
        hand: u32,
        button: usize,
    },
    BlindPosted {
        seat: usize,
        amount: u32,
    },
    Dealt {
        street: Street,
    },
    Acted {
        seat: usize,
        action: Action,
    },
    PotAwarded {
        pot: usize,
        winners: Vec<usize>,
        amount: u32,
        hand: Option<HandValue>,
    },
    SeatBusted {
        seat: usize,
    },
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Seat {
    kind: SeatKind,
    chips: u32,
    hole: [Card; 2],
    in_hand: bool,
    folded: bool,
    all_in: bool,
    street_bet: u32,
    contributed: u32,
    needs_to_act: bool,
    can_raise: bool,
}

impl Seat {
    fn live(&self) -> bool {
        self.in_hand && !self.folded
    }

    fn can_bet(&self) -> bool {
        self.live() && !self.all_in
    }
}

/// A human seat's action menu.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionMenu {
    /// `0` Fold, `1` Check / Call, `2` Raise, `3` All-in.
    pub cursor: u8,
    pub raise_to: u32,
}

/// One seat as a host draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatView {
    pub kind: SeatKind,
    pub chips: u32,
    pub street_bet: u32,
    pub in_hand: bool,
    pub folded: bool,
    pub all_in: bool,
    /// Every seat's hole cards. A host shows only its own seat's until
    /// showdown; in lockstep every peer holds the whole deal.
    pub hole: Option<[Card; 2]>,
}

/// The table snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PokerView {
    pub phase: Phase,
    pub hand: u32,
    pub button: usize,
    pub to_act: Option<usize>,
    pub board: Vec<Card>,
    pub pot: u32,
    pub current_bet: u32,
    pub min_raise_to: u32,
    pub small_blind: u32,
    pub big_blind: u32,
    pub seats: Vec<SeatView>,
    pub menus: Vec<ActionMenu>,
    /// The last hand's awards, for the showdown banner.
    pub last_awards: Vec<(Vec<usize>, u32, Option<HandValue>)>,
}

/// A running table.
#[derive(Debug, Clone)]
pub struct PokerSession {
    cfg: PokerConfig,
    seats: Vec<Seat>,
    deck: Vec<Card>,
    stacked: Vec<Vec<Card>>,
    deck_pos: usize,
    board: Vec<Card>,
    button: usize,
    to_act: usize,
    current_bet: u32,
    min_raise: u32,
    small_blind: u32,
    big_blind: u32,
    hand_no: u32,
    phase: Phase,
    rng: ModeRng,
    pads: PadEdges,
    menus: Vec<ActionMenu>,
    ai_wait: u32,
    bust_order: Vec<usize>,
    last_awards: Vec<(Vec<usize>, u32, Option<HandValue>)>,
}

impl PokerSession {
    /// Hand `seat` to `kind` from the next tick on: a networked table
    /// seats its players as humans and gives a dropped player's seat to the
    /// AI. Out-of-range seats are ignored.
    pub fn set_seat_kind(&mut self, seat: usize, kind: SeatKind) {
        if let Some(s) = self.seats.get_mut(seat) {
            s.kind = kind;
            self.cfg.seats[seat] = kind;
        }
    }

    /// Seat the table and deal the first hand.
    pub fn new(cfg: PokerConfig) -> Self {
        Self::build(cfg, Vec::new())
    }

    fn build(cfg: PokerConfig, stacked: Vec<Vec<Card>>) -> Self {
        assert!(
            (MIN_SEATS..=MAX_TABLE_SEATS).contains(&cfg.seats.len()),
            "poker takes 2..=6 seats"
        );
        let seats = cfg
            .seats
            .iter()
            .map(|&kind| Seat {
                kind,
                chips: cfg.buy_in,
                hole: [Card(0); 2],
                in_hand: false,
                folded: false,
                all_in: false,
                street_bet: 0,
                contributed: 0,
                needs_to_act: false,
                can_raise: false,
            })
            .collect();
        let n = cfg.seats.len();
        let mut s = Self {
            rng: ModeRng::new(cfg.seed),
            small_blind: cfg.small_blind,
            big_blind: cfg.big_blind,
            seats,
            deck: Card::deck(),
            stacked,
            deck_pos: 0,
            board: Vec::new(),
            // The first deal moves the button onto seat 0.
            button: n - 1,
            to_act: 0,
            current_bet: 0,
            min_raise: cfg.big_blind,
            hand_no: 0,
            phase: Phase::Showdown { timer: 0 },
            pads: PadEdges::default(),
            menus: vec![ActionMenu::default(); n],
            ai_wait: 0,
            bust_order: Vec::new(),
            last_awards: Vec::new(),
            cfg,
        };
        s.start_hand(&mut Vec::new());
        s
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The seat whose turn it is, while betting.
    pub fn to_act(&self) -> Option<usize> {
        matches!(self.phase, Phase::Betting(_)).then_some(self.to_act)
    }

    pub fn chips(&self, seat: usize) -> u32 {
        self.seats[seat].chips
    }

    pub fn board(&self) -> &[Card] {
        &self.board
    }

    /// Chips in all pots this hand.
    pub fn pot(&self) -> u32 {
        self.seats.iter().map(|s| s.contributed).sum()
    }

    /// The smallest legal raise-to for the seat to act.
    pub fn min_raise_to(&self) -> u32 {
        self.current_bet + self.min_raise
    }

    /// Seat the table with the first hands' decks fixed (tests and
    /// scripted tutorials). Each deck is dealt in order: hole cards one at
    /// a time round the table from the small blind, then flop, turn and
    /// river with no burn cards. Later hands shuffle.
    pub fn with_decks(cfg: PokerConfig, decks: Vec<Vec<Card>>) -> Self {
        assert!(decks.iter().all(|d| d.len() == 52));
        Self::build(cfg, decks)
    }

    /// Overwrite seat stacks (tests and scenario setup).
    pub fn set_chips(&mut self, seat: usize, chips: u32) {
        self.seats[seat].chips = chips;
    }

    fn next_in_hand(&self, from: usize) -> usize {
        let n = self.seats.len();
        let mut i = from;
        for _ in 0..n {
            i = (i + 1) % n;
            if self.seats[i].in_hand {
                return i;
            }
        }
        from
    }

    fn next_with_chips(&self, from: usize) -> usize {
        let n = self.seats.len();
        let mut i = from;
        for _ in 0..n {
            i = (i + 1) % n;
            if self.seats[i].chips > 0 {
                return i;
            }
        }
        from
    }

    fn draw(&mut self) -> Card {
        let c = self.deck[self.deck_pos];
        self.deck_pos += 1;
        c
    }

    fn post(&mut self, seat: usize, amount: u32) -> u32 {
        let s = &mut self.seats[seat];
        let paid = amount.min(s.chips);
        s.chips -= paid;
        s.street_bet += paid;
        s.contributed += paid;
        if s.chips == 0 {
            s.all_in = true;
        }
        paid
    }

    /// Start the next hand: move the button, post blinds, deal.
    pub(crate) fn start_hand(&mut self, ev: &mut Vec<PokerEvent>) {
        self.hand_no += 1;
        if let Some(every) = self.cfg.blind_double_every
            && every > 0
            && self.hand_no > 1
            && (self.hand_no - 1).is_multiple_of(every)
        {
            self.small_blind = self.small_blind.saturating_mul(2);
            self.big_blind = self.big_blind.saturating_mul(2);
        }
        for s in &mut self.seats {
            s.in_hand = s.chips > 0;
            s.folded = false;
            s.all_in = false;
            s.street_bet = 0;
            s.contributed = 0;
            s.needs_to_act = s.in_hand;
            s.can_raise = s.in_hand;
        }
        self.button = self.next_with_chips(self.button);
        if self.stacked.is_empty() {
            self.deck = Card::deck();
            self.rng.shuffle(&mut self.deck);
        } else {
            self.deck = self.stacked.remove(0);
        }
        self.deck_pos = 0;
        self.board.clear();
        self.last_awards.clear();
        ev.push(PokerEvent::HandStarted {
            hand: self.hand_no,
            button: self.button,
        });
        let in_hand = self.seats.iter().filter(|s| s.in_hand).count();
        let (sb, bb) = if in_hand == 2 {
            (self.button, self.next_in_hand(self.button))
        } else {
            let sb = self.next_in_hand(self.button);
            (sb, self.next_in_hand(sb))
        };
        let paid = self.post(sb, self.small_blind);
        ev.push(PokerEvent::BlindPosted {
            seat: sb,
            amount: paid,
        });
        let paid = self.post(bb, self.big_blind);
        ev.push(PokerEvent::BlindPosted {
            seat: bb,
            amount: paid,
        });
        // Two cards each, one at a time from the small blind round.
        for round in 0..2 {
            let mut i = sb;
            for _ in 0..in_hand {
                let c = self.draw();
                self.seats[i].hole[round] = c;
                i = self.next_in_hand(i);
            }
        }
        self.current_bet = self.big_blind;
        self.min_raise = self.big_blind;
        self.phase = Phase::Betting(Street::Preflop);
        ev.push(PokerEvent::Dealt {
            street: Street::Preflop,
        });
        self.to_act = bb;
        self.advance_turn(ev);
    }

    fn seat_menu_reset(&mut self) {
        let min = self.min_raise_to();
        let seat = self.to_act;
        self.menus[seat].raise_to = min;
        self.menus[seat].cursor = 1;
        self.ai_wait = self.cfg.ai_think_frames;
    }

    /// Find the next seat that owes an action, or close the street.
    fn advance_turn(&mut self, ev: &mut Vec<PokerEvent>) {
        if self.seats.iter().filter(|s| s.live()).count() == 1 {
            self.award_uncontested(ev);
            return;
        }
        let n = self.seats.len();
        let mut i = self.to_act;
        for _ in 0..n {
            i = (i + 1) % n;
            let s = &self.seats[i];
            if s.can_bet() && s.needs_to_act {
                // A lone seat that can still bet, already matched, has no
                // decision left.
                let others = self.seats.iter().filter(|o| o.can_bet()).count();
                if others == 1 && s.street_bet >= self.current_bet {
                    break;
                }
                self.to_act = i;
                self.seat_menu_reset();
                return;
            }
        }
        self.end_street(ev);
    }

    fn end_street(&mut self, ev: &mut Vec<PokerEvent>) {
        let Phase::Betting(street) = self.phase else {
            return;
        };
        for s in &mut self.seats {
            s.street_bet = 0;
            s.needs_to_act = s.can_bet();
            s.can_raise = true;
        }
        self.current_bet = 0;
        self.min_raise = self.big_blind;
        let next = match street {
            Street::Preflop => {
                for _ in 0..3 {
                    let c = self.draw();
                    self.board.push(c);
                }
                Street::Flop
            }
            Street::Flop => {
                let c = self.draw();
                self.board.push(c);
                Street::Turn
            }
            Street::Turn => {
                let c = self.draw();
                self.board.push(c);
                Street::River
            }
            Street::River => {
                self.showdown(ev);
                return;
            }
        };
        self.phase = Phase::Betting(next);
        ev.push(PokerEvent::Dealt { street: next });
        if self.seats.iter().filter(|s| s.can_bet()).count() < 2 {
            // Nobody left to bet against: run the board out.
            self.end_street(ev);
            return;
        }
        // Postflop the first live seat left of the button acts first.
        self.to_act = self.button;
        self.advance_turn(ev);
    }

    fn award_uncontested(&mut self, ev: &mut Vec<PokerEvent>) {
        let winner = self.seats.iter().position(|s| s.live()).unwrap();
        let amount = self.pot();
        self.seats[winner].chips += amount;
        self.last_awards.push((vec![winner], amount, None));
        ev.push(PokerEvent::PotAwarded {
            pot: 0,
            winners: vec![winner],
            amount,
            hand: None,
        });
        self.finish_hand(ev);
    }

    fn showdown(&mut self, ev: &mut Vec<PokerEvent>) {
        let contributed: Vec<u32> = self.seats.iter().map(|s| s.contributed).collect();
        let live: Vec<bool> = self.seats.iter().map(|s| s.live()).collect();
        let values: Vec<HandValue> = self
            .seats
            .iter()
            .map(|s| {
                if s.live() {
                    let mut seven = self.board.clone();
                    seven.extend_from_slice(&s.hole);
                    best_hand(&seven)
                } else {
                    HandValue(0)
                }
            })
            .collect();
        let n = self.seats.len();
        for (pi, pot) in pots::build_pots(&contributed, &live).iter().enumerate() {
            if pot.eligible.is_empty() {
                continue;
            }
            let best = pot.eligible.iter().map(|&i| values[i]).max().unwrap();
            // Winners ordered from the button's left, for odd chips.
            let mut winners: Vec<usize> = (1..=n)
                .map(|k| (self.button + k) % n)
                .filter(|i| pot.eligible.contains(i) && values[*i] == best)
                .collect();
            let share = pot.amount / winners.len() as u32;
            let mut odd = pot.amount % winners.len() as u32;
            for &w in &winners {
                let extra = if odd > 0 {
                    odd -= 1;
                    1
                } else {
                    0
                };
                self.seats[w].chips += share + extra;
            }
            winners.sort_unstable();
            let hand = (pot.eligible.len() > 1).then_some(best);
            self.last_awards.push((winners.clone(), pot.amount, hand));
            ev.push(PokerEvent::PotAwarded {
                pot: pi,
                winners,
                amount: pot.amount,
                hand,
            });
        }
        self.finish_hand(ev);
    }

    fn finish_hand(&mut self, ev: &mut Vec<PokerEvent>) {
        for s in &mut self.seats {
            s.contributed = 0;
            s.street_bet = 0;
        }
        for i in 0..self.seats.len() {
            if self.seats[i].in_hand && self.seats[i].chips == 0 {
                self.seats[i].in_hand = false;
                self.bust_order.push(i);
                ev.push(PokerEvent::SeatBusted { seat: i });
            }
        }
        let with_chips = self.seats.iter().filter(|s| s.chips > 0).count();
        let humans = self.seats.iter().filter(|s| !s.kind.is_ai()).count();
        let humans_alive = self
            .seats
            .iter()
            .filter(|s| !s.kind.is_ai() && s.chips > 0)
            .count();
        let out_of_hands = self.cfg.max_hands.is_some_and(|m| self.hand_no >= m);
        if with_chips <= 1 || (humans > 0 && humans_alive == 0) || out_of_hands {
            self.phase = Phase::Finished;
            ev.push(PokerEvent::Finished);
        } else {
            self.phase = Phase::Showdown {
                timer: self.cfg.showdown_frames,
            };
        }
    }

    /// Apply `action` for `seat`.
    pub fn act(&mut self, seat: usize, action: Action) -> Result<Vec<PokerEvent>, ActionError> {
        if !matches!(self.phase, Phase::Betting(_)) {
            return Err(ActionError::NotBetting);
        }
        if seat != self.to_act {
            return Err(ActionError::NotYourTurn);
        }
        let s = &self.seats[seat];
        let to_call = self.current_bet.saturating_sub(s.street_bet);
        let max_to = s.street_bet + s.chips;
        // Resolve All-in into the call or raise it is.
        let action = match action {
            Action::AllIn if s.chips <= to_call || !s.can_raise => Action::Call,
            Action::AllIn => Action::RaiseTo(max_to),
            a => a,
        };
        match action {
            Action::Check if to_call > 0 => return Err(ActionError::CannotCheck),
            Action::Call if to_call == 0 => return Err(ActionError::NothingToCall),
            Action::RaiseTo(_) if !s.can_raise => return Err(ActionError::RaiseClosed),
            Action::RaiseTo(x) if x > max_to => return Err(ActionError::RaiseTooLarge),
            Action::RaiseTo(x) if x <= self.current_bet => return Err(ActionError::RaiseTooSmall),
            Action::RaiseTo(x) if x < self.min_raise_to() && x != max_to => {
                return Err(ActionError::RaiseTooSmall);
            }
            _ => {}
        }
        let mut ev = vec![PokerEvent::Acted { seat, action }];
        self.seats[seat].needs_to_act = false;
        match action {
            Action::Fold => self.seats[seat].folded = true,
            Action::Check => {}
            Action::Call => {
                self.post(seat, to_call);
            }
            Action::RaiseTo(x) => {
                let add = x - self.seats[seat].street_bet;
                self.post(seat, add);
                let raise = x - self.current_bet;
                let full = raise >= self.min_raise;
                if full {
                    self.min_raise = raise;
                }
                for (i, o) in self.seats.iter_mut().enumerate() {
                    if i == seat || !o.can_bet() {
                        continue;
                    }
                    if full {
                        o.can_raise = true;
                    } else if !o.needs_to_act {
                        o.can_raise = false;
                    }
                    o.needs_to_act = true;
                }
                self.current_bet = x;
            }
            Action::AllIn => unreachable!(),
        }
        self.advance_turn(&mut ev);
        Ok(ev)
    }

    fn ai_spot(&self, seat: usize) -> ai::AiSpot<'_> {
        let s = &self.seats[seat];
        ai::AiSpot {
            hole: s.hole,
            board: &self.board,
            to_call: self.current_bet.saturating_sub(s.street_bet),
            pot: self.pot(),
            chips: s.chips,
            current_bet: self.current_bet,
            street_bet: s.street_bet,
            min_raise_to: self.min_raise_to(),
            can_raise: s.can_raise,
            opponents: self.seats.iter().filter(|o| o.live()).count() - 1,
            big_blind: self.big_blind,
        }
    }

    /// The legal fallback for an action the seat cannot take.
    fn coerce(&self, seat: usize, a: Action) -> Action {
        let s = &self.seats[seat];
        let to_call = self.current_bet.saturating_sub(s.street_bet);
        match a {
            Action::Check if to_call > 0 => Action::Fold,
            Action::Call if to_call == 0 => Action::Check,
            Action::RaiseTo(x) => {
                let max_to = s.street_bet + s.chips;
                if !s.can_raise {
                    if to_call == 0 {
                        Action::Check
                    } else {
                        Action::Call
                    }
                } else if x >= max_to {
                    Action::AllIn
                } else {
                    Action::RaiseTo(x.max(self.min_raise_to()))
                }
            }
            a => a,
        }
    }

    fn human_input(&mut self, seat: usize) -> Option<Action> {
        let s = &self.seats[seat];
        let max_to = s.street_bet + s.chips;
        let min_to = self.min_raise_to().min(max_to);
        let step = self.big_blind.max(1);
        let m = &mut self.menus[seat];
        if self.pads.pressed(seat, PadButton::Left) {
            m.cursor = m.cursor.saturating_sub(1);
        }
        if self.pads.pressed(seat, PadButton::Right) {
            m.cursor = (m.cursor + 1).min(3);
        }
        if self.pads.pressed(seat, PadButton::Up) {
            m.raise_to = m.raise_to.saturating_add(step).min(max_to);
        }
        if self.pads.pressed(seat, PadButton::Down) {
            m.raise_to = m.raise_to.saturating_sub(step).max(min_to);
        }
        m.raise_to = m.raise_to.clamp(min_to, max_to);
        if !self.pads.pressed(seat, PadButton::Cross) {
            return None;
        }
        let to_call = self.current_bet.saturating_sub(s.street_bet);
        Some(match m.cursor {
            0 => Action::Fold,
            1 if to_call == 0 => Action::Check,
            1 => Action::Call,
            2 => Action::RaiseTo(m.raise_to),
            _ => Action::AllIn,
        })
    }

    fn results_now(&self) -> ModeResults {
        let n = self.seats.len();
        // Seats still holding chips rank by stack; busted seats by how late
        // they went out.
        let mut score = vec![0i64; n];
        for (i, s) in self.seats.iter().enumerate() {
            score[i] = s.chips as i64;
        }
        let mut order: Vec<usize> = (0..n).filter(|&i| self.seats[i].chips > 0).collect();
        order.sort_by(|&a, &b| score[b].cmp(&score[a]).then(a.cmp(&b)));
        order.extend(self.bust_order.iter().rev());
        let mut placements = Vec::with_capacity(n);
        for (k, &seat) in order.iter().enumerate() {
            let place = match placements.last() {
                Some(crate::session::Placement {
                    score: ps, place, ..
                }) if *ps == score[seat] && score[seat] > 0 => *place,
                _ => (k + 1) as u8,
            };
            placements.push(crate::session::Placement {
                seat,
                place,
                score: score[seat],
            });
        }
        let rewards = (0..n)
            .map(|seat| Reward::Currency {
                seat,
                currency: self.cfg.currency,
                amount: (self.seats[seat].chips as i64 - self.cfg.buy_in as i64)
                    * self.cfg.chip_value as i64,
            })
            .collect();
        ModeResults {
            placements,
            rewards,
        }
    }
}

impl ModeSession for PokerSession {
    type Event = PokerEvent;
    type View = PokerView;

    fn seats(&self) -> usize {
        self.seats.len()
    }

    fn seat_kind(&self, seat: usize) -> SeatKind {
        self.seats[seat].kind
    }

    fn tick(&mut self, pads: &[PadWord]) -> Vec<PokerEvent> {
        self.pads.update(pads);
        let mut ev = Vec::new();
        match self.phase {
            Phase::Finished => {}
            Phase::Showdown { timer } => {
                let skip = (0..self.seats.len())
                    .any(|i| !self.seats[i].kind.is_ai() && self.pads.pressed(i, PadButton::Cross));
                if timer == 0 || skip {
                    self.start_hand(&mut ev);
                } else {
                    self.phase = Phase::Showdown { timer: timer - 1 };
                }
            }
            Phase::Betting(_) => {
                let seat = self.to_act;
                let choice = match self.seats[seat].kind {
                    SeatKind::Ai(tier) => {
                        if self.ai_wait > 0 {
                            self.ai_wait -= 1;
                            None
                        } else {
                            let spot = self.ai_spot(seat);
                            let mut rng = self.rng.clone();
                            let a = ai::decide(&spot, tier, &mut rng);
                            self.rng = rng;
                            Some(a)
                        }
                    }
                    SeatKind::Human => self.human_input(seat),
                };
                if let Some(a) = choice {
                    let a = self.coerce(seat, a);
                    match self.act(seat, a) {
                        Ok(mut e) => ev.append(&mut e),
                        // A menu raise below the legal size folds to a call.
                        Err(_) => {
                            let fallback = self.coerce(seat, Action::Call);
                            if let Ok(mut e) = self.act(seat, fallback) {
                                ev.append(&mut e);
                            }
                        }
                    }
                }
            }
        }
        ev
    }

    fn view(&self) -> PokerView {
        PokerView {
            phase: self.phase,
            hand: self.hand_no,
            button: self.button,
            to_act: self.to_act(),
            board: self.board.clone(),
            pot: self.pot(),
            current_bet: self.current_bet,
            min_raise_to: self.min_raise_to(),
            small_blind: self.small_blind,
            big_blind: self.big_blind,
            seats: self
                .seats
                .iter()
                .map(|s| SeatView {
                    kind: s.kind,
                    chips: s.chips,
                    street_bet: s.street_bet,
                    in_hand: s.in_hand,
                    folded: s.folded,
                    all_in: s.all_in,
                    hole: s.in_hand.then_some(s.hole),
                })
                .collect(),
            menus: self.menus.clone(),
            last_awards: self.last_awards.clone(),
        }
    }

    fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.rng.hash_into(&mut h);
        self.pads.hash_into(&mut h);
        for s in &self.seats {
            h.seat(s.kind);
            h.u32(s.chips);
            h.u8(s.hole[0].0);
            h.u8(s.hole[1].0);
            h.bool(s.in_hand);
            h.bool(s.folded);
            h.bool(s.all_in);
            h.u32(s.street_bet);
            h.u32(s.contributed);
            h.bool(s.needs_to_act);
            h.bool(s.can_raise);
        }
        for c in &self.deck {
            h.u8(c.0);
        }
        h.usize(self.deck_pos);
        h.usize(self.stacked.len());
        for c in &self.board {
            h.u8(c.0);
        }
        h.usize(self.button);
        h.usize(self.to_act);
        h.u32(self.current_bet);
        h.u32(self.min_raise);
        h.u32(self.small_blind);
        h.u32(self.big_blind);
        h.u32(self.hand_no);
        match self.phase {
            Phase::Betting(s) => h.u8(s as u8),
            Phase::Showdown { timer } => {
                h.u8(10);
                h.u32(timer)
            }
            Phase::Finished => h.u8(11),
        }
        for m in &self.menus {
            h.u8(m.cursor);
            h.u32(m.raise_to);
        }
        h.u32(self.ai_wait);
        for &b in &self.bust_order {
            h.usize(b);
        }
        h.finish()
    }

    fn is_finished(&self) -> bool {
        self.phase == Phase::Finished
    }

    fn results(&self) -> Option<ModeResults> {
        self.is_finished().then(|| self.results_now())
    }
}

#[cfg(test)]
mod tests;

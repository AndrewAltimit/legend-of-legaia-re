//! Grid Duel: the trading-card battle game.
//!
//! Two players, five cards each, both hands face up, a 3x3 grid. Players
//! alternate placing one card on an empty cell; the first player places
//! five, the second four, so one card stays in the second player's hand.
//!
//! - **Capture.** A placed card compares each side with the facing side of
//!   every orthogonally adjacent opponent card and takes it when its side
//!   is strictly higher.
//! - **Elemental cells** (optional). Up to three cells carry an element. A
//!   card of the same element there plays every side at `+1`; a card of
//!   another element at `-1` (never below 1); an elementless card is
//!   unaffected.
//! - **Chain** (optional). Each card a placement captured then captures
//!   its own weaker opponent neighbours, one step and no further.
//! - **Score.** When the grid is full each player counts the cards they own
//!   on the grid plus the cards left in their hand; more wins, equal draws.
//! - **Stakes** (optional). The winner then takes one card of the loser's
//!   deck (picked with the pad, or the strongest for an AI winner).
//!
//! Pad: Left / Right pick a hand card, Cross lifts it; the D-pad moves the
//! grid cursor, Cross places, Circle puts the card back.

use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::cards::{CardElement, Rarity};
use serde::{Deserialize, Serialize};

use super::{CardBook, CardError};
use crate::session::{
    AiTier, ModeResults, ModeRng, ModeSession, PadEdges, PadWord, Placement, Reward, SeatKind,
    StateHasher,
};

/// Cards per hand.
pub const HAND: usize = 5;
/// Grid cells.
pub const CELLS: usize = 9;

/// A card as the duel plays it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuelCard {
    pub id: String,
    /// `[top, right, bottom, left]`.
    pub sides: [u8; 4],
    pub element: Option<CardElement>,
    pub rarity: Rarity,
}

impl DuelCard {
    fn strength(&self) -> u32 {
        self.sides.iter().map(|&s| s as u32).sum::<u32>() + self.rarity as u32 * 4
    }
}

/// A duel's setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuelConfig {
    pub seats: [SeatKind; 2],
    pub decks: [Vec<DuelCard>; 2],
    pub elemental_cells: bool,
    pub chain: bool,
    pub stakes: bool,
    /// Booster granted to seat 0 when it wins.
    pub win_booster: Option<String>,
    pub ai_think_frames: u32,
    pub seed: u64,
}

impl DuelConfig {
    /// Build from a pack's duel: seat 0 is the player with `player_deck`,
    /// seat 1 the duel's opponent.
    pub fn from_duel(
        duel: &legaia_expansion_format::modes::CardDuel,
        book: &CardBook,
        player_deck: &[String],
        seed: u64,
    ) -> Result<Self, CardError> {
        let resolve = |ids: &[String]| -> Result<Vec<DuelCard>, CardError> {
            ids.iter()
                .take(HAND)
                .map(|id| {
                    let c = book
                        .card(id)
                        .ok_or_else(|| CardError::UnknownCard(id.clone()))?;
                    Ok(DuelCard {
                        id: c.id.clone(),
                        sides: c.sides.map(|s| s.clamp(1, 10)),
                        element: c.element,
                        rarity: c.rarity,
                    })
                })
                .collect()
        };
        let mine = resolve(player_deck)?;
        let theirs = resolve(&duel.deck)?;
        for deck in [&mine, &theirs] {
            if deck.len() != HAND {
                return Err(CardError::NotEnough {
                    card: "deck".into(),
                    have: deck.len() as u32,
                    need: HAND as u32,
                });
            }
        }
        Ok(Self {
            seats: [SeatKind::Human, SeatKind::Ai(duel.ai_tier.into())],
            decks: [mine, theirs],
            elemental_cells: duel.elemental_cells,
            chain: duel.chain,
            stakes: duel.stakes,
            win_booster: duel.win_booster.clone(),
            ai_think_frames: 20,
            seed,
        })
    }
}

/// Why [`DuelSession::play`] refused a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuelError {
    NotPlaying,
    NotYourTurn,
    OutOfRange,
    CardPlayed,
    CellTaken,
}

/// A card on the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    /// The original owner's seat and hand index.
    pub deck: usize,
    pub card: usize,
    /// Who owns it now.
    pub owner: usize,
}

/// The duel's phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuelPhase {
    /// `seat` is choosing a hand card.
    PickCard {
        seat: usize,
    },
    /// `seat` holds hand card `card` and is choosing a cell.
    PickCell {
        seat: usize,
        card: usize,
    },
    /// The winner chooses a card from the loser's deck.
    Spoils {
        winner: usize,
    },
    Finished,
}

/// What a tick reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuelEvent {
    Placed {
        seat: usize,
        card: usize,
        cell: usize,
    },
    Captured {
        cell: usize,
        by: usize,
        chained: bool,
    },
    GameOver {
        scores: [u32; 2],
    },
    SpoilTaken {
        seat: usize,
        card: String,
    },
}

/// The duel snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuelView {
    pub phase: DuelPhase,
    pub grid: [Option<Placed>; CELLS],
    pub cell_elements: [Option<CardElement>; CELLS],
    pub hands: [Vec<DuelCard>; 2],
    /// `used[seat][i]`: hand card `i` is on the grid.
    pub used: [[bool; HAND]; 2],
    pub hand_cursor: [usize; 2],
    pub cell_cursor: [usize; 2],
    pub spoil_cursor: usize,
    pub scores: [u32; 2],
}

/// A running duel.
#[derive(Debug, Clone)]
pub struct DuelSession {
    cfg: DuelConfig,
    grid: [Option<Placed>; CELLS],
    elements: [Option<CardElement>; CELLS],
    used: [[bool; HAND]; 2],
    phase: DuelPhase,
    first: usize,
    hand_cursor: [usize; 2],
    cell_cursor: [usize; 2],
    spoil_cursor: usize,
    spoil: Option<(usize, String)>,
    ai_wait: u32,
    rng: ModeRng,
    pads: PadEdges,
}

/// Neighbour cell and the facing side indices: `(cell, my_side, their_side)`.
fn neighbours(cell: usize) -> impl Iterator<Item = (usize, usize, usize)> {
    let (r, c) = (cell / 3, cell % 3);
    let mut out = Vec::with_capacity(4);
    if r > 0 {
        out.push((cell - 3, 0, 2));
    }
    if c < 2 {
        out.push((cell + 1, 1, 3));
    }
    if r < 2 {
        out.push((cell + 3, 2, 0));
    }
    if c > 0 {
        out.push((cell - 1, 3, 1));
    }
    out.into_iter()
}

/// The pure board the rules and the AI search share.
#[derive(Debug, Clone)]
struct Board<'a> {
    cfg: &'a DuelConfig,
    grid: [Option<Placed>; CELLS],
    elements: [Option<CardElement>; CELLS],
}

impl Board<'_> {
    fn side(&self, cell: usize, side: usize) -> i32 {
        let p = self.grid[cell].unwrap();
        let card = &self.cfg.decks[p.deck][p.card];
        let base = card.sides[side] as i32;
        let adj = match (self.elements[cell], card.element) {
            (Some(e), Some(ce)) if e == ce => 1,
            (Some(_), Some(_)) => -1,
            _ => 0,
        };
        (base + adj).max(1)
    }

    /// Place and resolve captures; the captured cells (and whether each was
    /// a chain capture).
    fn place(&mut self, seat: usize, card: usize, cell: usize) -> Vec<(usize, bool)> {
        self.grid[cell] = Some(Placed {
            deck: seat,
            card,
            owner: seat,
        });
        let mut taken = Vec::new();
        for (n, mine, theirs) in neighbours(cell) {
            if let Some(p) = self.grid[n]
                && p.owner != seat
                && self.side(cell, mine) > self.side(n, theirs)
            {
                self.grid[n].as_mut().unwrap().owner = seat;
                taken.push((n, false));
            }
        }
        if self.cfg.chain {
            let first: Vec<usize> = taken.iter().map(|t| t.0).collect();
            for from in first {
                for (n, mine, theirs) in neighbours(from) {
                    if let Some(p) = self.grid[n]
                        && p.owner != seat
                        && self.side(from, mine) > self.side(n, theirs)
                    {
                        self.grid[n].as_mut().unwrap().owner = seat;
                        taken.push((n, true));
                    }
                }
            }
        }
        taken
    }

    fn owned(&self, seat: usize) -> u32 {
        self.grid
            .iter()
            .flatten()
            .filter(|p| p.owner == seat)
            .count() as u32
    }
}

impl DuelSession {
    /// Hand `seat` to `kind` from the next tick on: a networked table
    /// seats its players as humans and gives a dropped player's seat to the
    /// AI. Out-of-range seats are ignored.
    pub fn set_seat_kind(&mut self, seat: usize, kind: SeatKind) {
        if let Some(k) = self.cfg.seats.get_mut(seat) {
            *k = kind;
        }
    }

    pub fn new(cfg: DuelConfig) -> Self {
        assert!(
            cfg.decks.iter().all(|d| d.len() == HAND),
            "decks hold 5 cards"
        );
        let mut rng = ModeRng::new(cfg.seed);
        let mut elements = [None; CELLS];
        if cfg.elemental_cells {
            const ALL: [CardElement; 7] = [
                CardElement::Fire,
                CardElement::Water,
                CardElement::Wind,
                CardElement::Earth,
                CardElement::Thunder,
                CardElement::Light,
                CardElement::Dark,
            ];
            for _ in 0..rng.range(1, 3) {
                let cell = rng.below(CELLS as u32) as usize;
                elements[cell] = Some(ALL[rng.below(7) as usize]);
            }
        }
        let first = rng.below(2) as usize;
        Self {
            grid: [None; CELLS],
            elements,
            used: [[false; HAND]; 2],
            phase: DuelPhase::PickCard { seat: first },
            first,
            hand_cursor: [0; 2],
            cell_cursor: [4; 2],
            spoil_cursor: 0,
            spoil: None,
            ai_wait: cfg.ai_think_frames,
            rng,
            pads: PadEdges::default(),
            cfg,
        }
    }

    pub fn phase(&self) -> DuelPhase {
        self.phase
    }

    /// The seat that moves first.
    pub fn first_player(&self) -> usize {
        self.first
    }

    fn board(&self) -> Board<'_> {
        Board {
            cfg: &self.cfg,
            grid: self.grid,
            elements: self.elements,
        }
    }

    /// Final or running score: owned grid cards plus unplayed hand cards.
    pub fn scores(&self) -> [u32; 2] {
        let b = self.board();
        [0, 1].map(|s| b.owned(s) + self.used[s].iter().filter(|u| !**u).count() as u32)
    }

    /// Place `card` from `seat`'s hand on `cell` (tests and alternate
    /// hosts; the pad path ends here too).
    pub fn play(
        &mut self,
        seat: usize,
        card: usize,
        cell: usize,
    ) -> Result<Vec<DuelEvent>, DuelError> {
        let turn = match self.phase {
            DuelPhase::PickCard { seat } | DuelPhase::PickCell { seat, .. } => seat,
            _ => return Err(DuelError::NotPlaying),
        };
        if turn != seat {
            return Err(DuelError::NotYourTurn);
        }
        if card >= HAND || cell >= CELLS {
            return Err(DuelError::OutOfRange);
        }
        if self.used[seat][card] {
            return Err(DuelError::CardPlayed);
        }
        if self.grid[cell].is_some() {
            return Err(DuelError::CellTaken);
        }
        let mut b = self.board();
        let taken = b.place(seat, card, cell);
        self.grid = b.grid;
        self.used[seat][card] = true;
        let mut ev = vec![DuelEvent::Placed { seat, card, cell }];
        ev.extend(taken.into_iter().map(|(c, chained)| DuelEvent::Captured {
            cell: c,
            by: seat,
            chained,
        }));
        if self.grid.iter().all(|c| c.is_some()) {
            let scores = self.scores();
            ev.push(DuelEvent::GameOver { scores });
            let winner = match scores[0].cmp(&scores[1]) {
                std::cmp::Ordering::Greater => Some(0),
                std::cmp::Ordering::Less => Some(1),
                std::cmp::Ordering::Equal => None,
            };
            self.phase = match winner {
                Some(w) if self.cfg.stakes => DuelPhase::Spoils { winner: w },
                _ => DuelPhase::Finished,
            };
        } else {
            let next = 1 - seat;
            self.phase = DuelPhase::PickCard { seat: next };
            self.hand_cursor[next] = (0..HAND).find(|&i| !self.used[next][i]).unwrap_or(0);
        }
        self.ai_wait = self.cfg.ai_think_frames;
        Ok(ev)
    }

    fn legal_moves(&self, seat: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for card in (0..HAND).filter(|&c| !self.used[seat][c]) {
            for cell in (0..CELLS).filter(|&c| self.grid[c].is_none()) {
                out.push((card, cell));
            }
        }
        out
    }

    /// Choose a move for an AI seat.
    fn ai_move(&mut self, seat: usize, tier: AiTier) -> (usize, usize) {
        let moves = self.legal_moves(seat);
        if tier == AiTier::Novice || moves.len() == 1 {
            return moves[self.rng.below(moves.len() as u32) as usize];
        }
        let foe = 1 - seat;
        let mut best = i32::MIN;
        let mut picks = Vec::new();
        for &(card, cell) in &moves {
            let mut b = self.board();
            let gained = b.place(seat, card, cell).len() as i32;
            let mut score = gained * 10;
            // Penalise exposing weak sides to empty cells.
            for (n, mine, _) in neighbours(cell) {
                if b.grid[n].is_none() {
                    score -= (10 - b.side(cell, mine)).max(0) / 3;
                }
            }
            if tier == AiTier::Expert {
                // Subtract the opponent's best immediate reply.
                let mut worst = 0;
                for card2 in (0..HAND).filter(|&c| !self.used[foe][c]) {
                    for cell2 in (0..CELLS).filter(|&c| b.grid[c].is_none()) {
                        let mut b2 = b.clone();
                        worst = worst.max(b2.place(foe, card2, cell2).len() as i32);
                    }
                }
                score -= worst * 10;
            }
            if score > best {
                best = score;
                picks.clear();
            }
            if score == best {
                picks.push((card, cell));
            }
        }
        picks[self.rng.below(picks.len() as u32) as usize]
    }

    fn move_cursor(&mut self, seat: usize) {
        let c = &mut self.cell_cursor[seat];
        if self.pads.pressed(seat, PadButton::Up) && *c >= 3 {
            *c -= 3;
        }
        if self.pads.pressed(seat, PadButton::Down) && *c < 6 {
            *c += 3;
        }
        if self.pads.pressed(seat, PadButton::Left) && !c.is_multiple_of(3) {
            *c -= 1;
        }
        if self.pads.pressed(seat, PadButton::Right) && *c % 3 < 2 {
            *c += 1;
        }
    }

    fn step_hand_cursor(&mut self, seat: usize, dir: i32) {
        let mut i = self.hand_cursor[seat] as i32;
        for _ in 0..HAND {
            i = (i + dir).rem_euclid(HAND as i32);
            if !self.used[seat][i as usize] {
                self.hand_cursor[seat] = i as usize;
                return;
            }
        }
    }

    fn take_spoil(&mut self, winner: usize, index: usize) -> DuelEvent {
        let loser = 1 - winner;
        let id = self.cfg.decks[loser][index].id.clone();
        self.spoil = Some((winner, id.clone()));
        self.phase = DuelPhase::Finished;
        DuelEvent::SpoilTaken {
            seat: winner,
            card: id,
        }
    }
}

impl ModeSession for DuelSession {
    type Event = DuelEvent;
    type View = DuelView;

    fn seats(&self) -> usize {
        2
    }

    fn seat_kind(&self, seat: usize) -> SeatKind {
        self.cfg.seats[seat]
    }

    fn tick(&mut self, pads: &[PadWord]) -> Vec<DuelEvent> {
        self.pads.update(pads);
        let seat = match self.phase {
            DuelPhase::PickCard { seat } | DuelPhase::PickCell { seat, .. } => seat,
            DuelPhase::Spoils { winner } => winner,
            DuelPhase::Finished => return Vec::new(),
        };
        if let SeatKind::Ai(tier) = self.cfg.seats[seat] {
            if self.ai_wait > 0 {
                self.ai_wait -= 1;
                return Vec::new();
            }
            if let DuelPhase::Spoils { winner } = self.phase {
                let loser = &self.cfg.decks[1 - winner];
                let best = (0..HAND)
                    .max_by_key(|&i| (loser[i].strength(), HAND - i))
                    .unwrap();
                return vec![self.take_spoil(winner, best)];
            }
            let (card, cell) = self.ai_move(seat, tier);
            return self.play(seat, card, cell).unwrap_or_default();
        }
        match self.phase {
            DuelPhase::PickCard { seat } => {
                if self.used[seat][self.hand_cursor[seat]] {
                    self.step_hand_cursor(seat, 1);
                }
                if self.pads.pressed(seat, PadButton::Left) {
                    self.step_hand_cursor(seat, -1);
                }
                if self.pads.pressed(seat, PadButton::Right) {
                    self.step_hand_cursor(seat, 1);
                }
                if self.pads.pressed(seat, PadButton::Cross) {
                    self.phase = DuelPhase::PickCell {
                        seat,
                        card: self.hand_cursor[seat],
                    };
                }
                Vec::new()
            }
            DuelPhase::PickCell { seat, card } => {
                if self.pads.pressed(seat, PadButton::Circle) {
                    self.phase = DuelPhase::PickCard { seat };
                    return Vec::new();
                }
                self.move_cursor(seat);
                if self.pads.pressed(seat, PadButton::Cross) {
                    return self
                        .play(seat, card, self.cell_cursor[seat])
                        .unwrap_or_default();
                }
                Vec::new()
            }
            DuelPhase::Spoils { winner } => {
                if self.pads.pressed(winner, PadButton::Left) {
                    self.spoil_cursor = (self.spoil_cursor + HAND - 1) % HAND;
                }
                if self.pads.pressed(winner, PadButton::Right) {
                    self.spoil_cursor = (self.spoil_cursor + 1) % HAND;
                }
                if self.pads.pressed(winner, PadButton::Cross) {
                    return vec![self.take_spoil(winner, self.spoil_cursor)];
                }
                Vec::new()
            }
            DuelPhase::Finished => Vec::new(),
        }
    }

    fn view(&self) -> DuelView {
        DuelView {
            phase: self.phase,
            grid: self.grid,
            cell_elements: self.elements,
            hands: self.cfg.decks.clone(),
            used: self.used,
            hand_cursor: self.hand_cursor,
            cell_cursor: self.cell_cursor,
            spoil_cursor: self.spoil_cursor,
            scores: self.scores(),
        }
    }

    fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.rng.hash_into(&mut h);
        self.pads.hash_into(&mut h);
        for cell in &self.grid {
            match cell {
                None => h.u8(0xFF),
                Some(p) => {
                    h.usize(p.deck);
                    h.usize(p.card);
                    h.usize(p.owner);
                }
            }
        }
        for e in &self.elements {
            h.u8(e.map_or(0xFF, |e| e as u8));
        }
        for side in &self.used {
            for &u in side {
                h.bool(u);
            }
        }
        match self.phase {
            DuelPhase::PickCard { seat } => {
                h.u8(0);
                h.usize(seat)
            }
            DuelPhase::PickCell { seat, card } => {
                h.u8(1);
                h.usize(seat);
                h.usize(card)
            }
            DuelPhase::Spoils { winner } => {
                h.u8(2);
                h.usize(winner)
            }
            DuelPhase::Finished => h.u8(3),
        }
        for s in 0..2 {
            h.usize(self.hand_cursor[s]);
            h.usize(self.cell_cursor[s]);
        }
        h.usize(self.spoil_cursor);
        h.u32(self.ai_wait);
        h.finish()
    }

    fn is_finished(&self) -> bool {
        self.phase == DuelPhase::Finished
    }

    fn results(&self) -> Option<ModeResults> {
        if !self.is_finished() {
            return None;
        }
        let s = self.scores();
        let placements: Vec<Placement> = ModeResults::rank(&[s[0] as i64, s[1] as i64], false);
        let mut rewards = Vec::new();
        if s[0] > s[1]
            && let Some(b) = &self.cfg.win_booster
        {
            rewards.push(Reward::Booster {
                seat: 0,
                booster: b.clone(),
            });
        }
        if let Some((seat, card)) = &self.spoil {
            rewards.push(Reward::Card {
                seat: *seat,
                card: card.clone(),
            });
        }
        Some(ModeResults {
            placements,
            rewards,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dc(id: &str, sides: [u8; 4]) -> DuelCard {
        DuelCard {
            id: id.into(),
            sides,
            element: None,
            rarity: Rarity::Common,
        }
    }

    fn deck(prefix: &str, sides: [u8; 4]) -> Vec<DuelCard> {
        (0..HAND)
            .map(|i| dc(&format!("{prefix}{i}"), sides))
            .collect()
    }

    fn cfg(seats: [SeatKind; 2]) -> DuelConfig {
        DuelConfig {
            seats,
            decks: [deck("a", [5, 5, 5, 5]), deck("b", [5, 5, 5, 5])],
            elemental_cells: false,
            chain: false,
            stakes: false,
            win_booster: Some("starter".into()),
            ai_think_frames: 0,
            seed: 0,
        }
    }

    fn humans() -> [SeatKind; 2] {
        [SeatKind::Human, SeatKind::Human]
    }

    #[test]
    fn stronger_side_captures_and_equal_does_not() {
        let mut c = cfg(humans());
        c.decks[0][0] = dc("hi", [9, 9, 9, 9]);
        let mut s = DuelSession::new(c);
        s.phase = DuelPhase::PickCard { seat: 1 };
        s.play(1, 1, 1).unwrap();
        let ev = s.play(0, 0, 2).unwrap();
        // Seat 0's left side 9 beats seat 1's right side 5.
        assert!(ev.contains(&DuelEvent::Captured {
            cell: 1,
            by: 0,
            chained: false
        }));
        // Equal sides (5 under 5) take nothing.
        let ev = s.play(1, 2, 4).unwrap();
        assert!(!ev.iter().any(|e| matches!(e, DuelEvent::Captured { .. })));
        assert_eq!(s.grid[1].unwrap().owner, 0);
        // Out of turn and occupied cells are refused.
        assert!(s.play(1, 3, 5).is_err());
        assert!(s.play(0, 1, 4).is_err());
    }

    #[test]
    fn chain_captures_one_step_onward() {
        for (captured_sides, chains) in [([8, 8, 8, 8], true), ([2, 2, 2, 2], false)] {
            let mut c = cfg(humans());
            c.chain = true;
            c.decks[0][0] = dc("hi", [9, 9, 9, 9]);
            c.decks[1][1] = dc("victim", captured_sides);
            let mut s = DuelSession::new(c);
            // Seat 1 holds cell 1 (the victim) and cell 0 (a plain 5).
            s.grid[1] = Some(Placed {
                deck: 1,
                card: 1,
                owner: 1,
            });
            s.grid[0] = Some(Placed {
                deck: 1,
                card: 2,
                owner: 1,
            });
            s.used[1][1] = true;
            s.used[1][2] = true;
            s.phase = DuelPhase::PickCard { seat: 0 };
            // 9 at the centre takes the victim above it; the victim's left
            // side then meets the 5 at cell 0.
            let ev = s.play(0, 0, 4).unwrap();
            assert!(ev.contains(&DuelEvent::Captured {
                cell: 1,
                by: 0,
                chained: false
            }));
            assert_eq!(
                ev.contains(&DuelEvent::Captured {
                    cell: 0,
                    by: 0,
                    chained: true
                }),
                chains
            );
        }
    }

    #[test]
    fn elemental_cells_shift_sides() {
        let mut c = cfg(humans());
        c.decks[0][0] = DuelCard {
            element: Some(CardElement::Fire),
            ..dc("f", [5, 5, 5, 5])
        };
        let mut s = DuelSession::new(c);
        s.elements = [None; CELLS];
        s.elements[4] = Some(CardElement::Fire);
        s.grid[1] = Some(Placed {
            deck: 1,
            card: 0,
            owner: 1,
        });
        s.used[1][0] = true;
        s.phase = DuelPhase::PickCard { seat: 0 };
        // 5 + 1 on a matching cell beats 5.
        let ev = s.play(0, 0, 4).unwrap();
        assert!(
            ev.iter()
                .any(|e| matches!(e, DuelEvent::Captured { cell: 1, .. }))
        );
        assert_eq!(s.board().side(4, 0), 6);
        s.elements[4] = Some(CardElement::Water);
        assert_eq!(s.board().side(4, 0), 4);
    }

    #[test]
    fn full_grid_scores_board_plus_hand_and_grants() {
        let mut c = cfg([SeatKind::Ai(AiTier::Expert), SeatKind::Ai(AiTier::Novice)]);
        c.decks[0] = deck("a", [8, 8, 8, 8]);
        c.decks[1] = deck("b", [2, 2, 2, 2]);
        c.stakes = true;
        let mut s = DuelSession::new(c);
        let mut frames = 0;
        while !s.is_finished() {
            s.tick(&[]);
            frames += 1;
            assert!(frames < 100);
        }
        let sc = s.scores();
        assert_eq!(sc[0] + sc[1], 10);
        assert!(sc[0] > sc[1]);
        let r = s.results().unwrap();
        assert_eq!(r.placements[0].seat, 0);
        assert!(r.rewards.contains(&Reward::Booster {
            seat: 0,
            booster: "starter".into()
        }));
        assert!(
            r.rewards
                .iter()
                .any(|r| matches!(r, Reward::Card { seat: 0, .. }))
        );
    }

    #[test]
    fn pad_places_a_card() {
        let mut s = DuelSession::new(cfg(humans()));
        let seat = s.first_player();
        let mut pads = [0u16; 2];
        let press = |s: &mut DuelSession, pads: &mut [u16; 2], b: PadButton| {
            pads[seat] = b.mask();
            let ev = s.tick(pads.as_slice());
            pads[seat] = 0;
            s.tick(pads.as_slice());
            ev
        };
        press(&mut s, &mut pads, PadButton::Right);
        press(&mut s, &mut pads, PadButton::Cross);
        assert_eq!(s.phase(), DuelPhase::PickCell { seat, card: 1 });
        press(&mut s, &mut pads, PadButton::Up);
        press(&mut s, &mut pads, PadButton::Left);
        let ev = press(&mut s, &mut pads, PadButton::Cross);
        assert_eq!(
            ev[0],
            DuelEvent::Placed {
                seat,
                card: 1,
                cell: 0
            }
        );
        // Illegal: the same cell twice.
        assert!(s.play(1 - seat, 0, 0).is_err());
    }

    #[test]
    fn expert_beats_novice_more_often_than_not() {
        let mut wins = 0;
        for seed in 0..40 {
            let mut c = cfg([SeatKind::Ai(AiTier::Expert), SeatKind::Ai(AiTier::Novice)]);
            c.seed = seed;
            let mut r = ModeRng::new(seed ^ 0xABCD);
            for d in &mut c.decks {
                for card in d.iter_mut() {
                    card.sides = [0; 4].map(|_| r.range(1, 9) as u8);
                }
            }
            let mut s = DuelSession::new(c);
            while !s.is_finished() {
                s.tick(&[]);
            }
            let sc = s.scores();
            if sc[0] > sc[1] {
                wins += 1;
            }
        }
        assert!(wins >= 24, "expert won {wins}/40");
    }
}

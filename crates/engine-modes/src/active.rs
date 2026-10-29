//! One running mode of any kind, for a host that seats one session at a
//! time.
//!
//! The play hosts do not care which rules engine is running: they enter a
//! mode instance by id, tick it with the pad, draw its view and settle its
//! results. [`ActiveMode`] is that seam, an enum over the five sessions
//! with the same [`ModeSession`] surface, plus what a screen needs beyond
//! the session's own view (the golf hole's tiles, the race track, the prop
//! hunt arena, the duel's card names).
//!
//! Instances come from a pack's `modes.toml` / `cards.toml`, or from the
//! built-in set ([`builtin_sections`]) a host offers when no pack is
//! active, so every mode can be played and tested without one.

use std::collections::BTreeMap;

use legaia_expansion_format::cards::{CardSection, RewardSource};
use legaia_expansion_format::modes::ModeSection;

use crate::cards::CardBook;
use crate::cards::duel::{DuelConfig, DuelEvent, DuelSession, DuelView};
use crate::golf::course::Course;
use crate::golf::{GolfConfig, GolfEvent, GolfSession, GolfView};
use crate::poker::{Action, PokerConfig, PokerEvent, PokerSession, PokerView};
use crate::prop_hunt::{Arena, HuntEvent, HuntView, PropHuntConfig, PropHuntSession};
use crate::racing::{RaceConfig, RaceEvent, RaceSession, RaceView, Track};
use crate::session::{AiTier, ModeResults, ModeRng, ModeSession, PadWord, Reward};

/// Which rules engine an instance runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModeKind {
    Poker,
    CardDuel,
    Golf,
    Race,
    PropHunt,
}

impl ModeKind {
    /// The kind as a player reads it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Poker => "Poker",
            Self::CardDuel => "Grid Duel",
            Self::Golf => "Golf",
            Self::Race => "Race",
            Self::PropHunt => "Prop Hunt",
        }
    }
}

/// One playable instance of a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeEntry {
    pub id: String,
    pub name: String,
    pub kind: ModeKind,
}

/// Every instance in `section`, in file order, poker first.
pub fn entries(section: &ModeSection) -> Vec<ModeEntry> {
    let mut out = Vec::new();
    let mut push = |id: &str, name: &str, kind| {
        out.push(ModeEntry {
            id: id.to_string(),
            name: if name.is_empty() { id } else { name }.to_string(),
            kind,
        })
    };
    for t in &section.poker {
        push(&t.id, &t.name, ModeKind::Poker);
    }
    for d in &section.card_duels {
        push(&d.id, &d.name, ModeKind::CardDuel);
    }
    for c in &section.golf {
        push(&c.id, &c.name, ModeKind::Golf);
    }
    for r in &section.races {
        push(&r.id, &r.name, ModeKind::Race);
    }
    for a in &section.prop_hunt {
        push(&a.id, &a.name, ModeKind::PropHunt);
    }
    out
}

const BUILTIN_MODES: &str = include_str!("../builtin/modes.toml");
const BUILTIN_CARDS: &str = include_str!("../builtin/cards.toml");

/// The built-in mode and card sections: one practice instance of every
/// kind, offered while no pack is active. Parsed once.
pub fn builtin_sections() -> &'static (ModeSection, CardSection) {
    static BUILTIN: std::sync::OnceLock<(ModeSection, CardSection)> = std::sync::OnceLock::new();
    BUILTIN.get_or_init(parse_builtin)
}

fn parse_builtin() -> (ModeSection, CardSection) {
    let mut files = legaia_expansion_format::PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"builtin\"\nname = \"Built-in modes\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert("modes.toml".into(), BUILTIN_MODES.as_bytes().to_vec());
    files.insert("cards.toml".into(), BUILTIN_CARDS.as_bytes().to_vec());
    let pack = legaia_expansion_format::ExpansionPack::from_files(&files)
        .expect("the built-in mode files parse");
    (pack.modes, pack.cards)
}

/// Why an instance could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// No instance has this id.
    UnknownInstance(String),
    /// The instance's data is unusable; the message names the problem.
    Invalid(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownInstance(id) => write!(f, "no mode instance `{id}`"),
            Self::Invalid(m) => f.write_str(m),
        }
    }
}

/// Host choices an instance does not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOptions {
    /// The player's duel deck. `None` takes the first five cards of the
    /// card section (the starter deck until a collection supplies one).
    pub player_deck: Option<Vec<String>>,
    /// AI opponents on a golf round (a course names none).
    pub golf_ai: Vec<AiTier>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            player_deck: None,
            golf_ai: vec![AiTier::Regular],
        }
    }
}

/// The session behind an [`ActiveMode`].
#[derive(Debug, Clone)]
pub enum ModeRunner {
    Poker(Box<PokerSession>),
    Duel(Box<DuelSession>),
    Golf(Box<GolfSession>),
    Race(Box<RaceSession>),
    PropHunt(Box<PropHuntSession>),
}

/// A running instance: the session plus the labels its screen shows.
#[derive(Debug, Clone)]
pub struct ActiveMode {
    pub id: String,
    pub name: String,
    pub runner: ModeRunner,
    /// Card id -> card name, for the duel's hands.
    pub card_names: BTreeMap<String, String>,
    book: CardBook,
}

/// A view of whichever session is running. A per-frame snapshot, built
/// and dropped by the draw, so the duel's larger variant is not boxed.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeView {
    Poker(PokerView),
    Duel(DuelView),
    Golf(GolfView),
    Race(RaceView),
    PropHunt(HuntView),
}

/// What a finished instance hands the host: the session's results plus the
/// pack's mode-source card rewards for a win.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settlement {
    pub results: ModeResults,
    /// Seat 0's place (1 = won).
    pub place: u8,
}

impl ActiveMode {
    /// Build instance `id` of `modes`, with `cards` supplying the duel's
    /// card book and the mode rewards.
    pub fn build(
        modes: &ModeSection,
        cards: &CardSection,
        id: &str,
        seed: u64,
        opts: &BuildOptions,
    ) -> Result<Self, BuildError> {
        let book = CardBook::new(cards.clone());
        let invalid = |e: &dyn std::fmt::Debug| BuildError::Invalid(format!("{id}: {e:?}"));
        let (name, runner) = if let Some(t) = modes.poker.iter().find(|t| t.id == id) {
            let cfg = PokerConfig::from_table(t, seed);
            (&t.name, ModeRunner::Poker(Box::new(PokerSession::new(cfg))))
        } else if let Some(d) = modes.card_duels.iter().find(|d| d.id == id) {
            let deck = match &opts.player_deck {
                Some(deck) => deck.clone(),
                None => cards.cards.iter().take(5).map(|c| c.id.clone()).collect(),
            };
            let cfg = DuelConfig::from_duel(d, &book, &deck, seed).map_err(|e| invalid(&e))?;
            (&d.name, ModeRunner::Duel(Box::new(DuelSession::new(cfg))))
        } else if let Some(c) = modes.golf.iter().find(|c| c.id == id) {
            let cfg = GolfConfig::from_course(c, &opts.golf_ai, seed).map_err(|e| invalid(&e))?;
            (&c.name, ModeRunner::Golf(Box::new(GolfSession::new(cfg))))
        } else if let Some(r) = modes.races.iter().find(|r| r.id == id) {
            let cfg = RaceConfig::from_def(r, seed).map_err(|e| invalid(&e))?;
            (&r.name, ModeRunner::Race(Box::new(RaceSession::new(cfg))))
        } else if let Some(a) = modes.prop_hunt.iter().find(|a| a.id == id) {
            let cfg = PropHuntConfig::from_arena(a, seed).map_err(|e| invalid(&e))?;
            (
                &a.name,
                ModeRunner::PropHunt(Box::new(PropHuntSession::new(cfg))),
            )
        } else {
            return Err(BuildError::UnknownInstance(id.to_string()));
        };
        let card_names = cards
            .cards
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        Ok(Self {
            id: id.to_string(),
            name: if name.is_empty() { id } else { name }.to_string(),
            runner,
            card_names,
            book,
        })
    }

    pub fn kind(&self) -> ModeKind {
        match &self.runner {
            ModeRunner::Poker(_) => ModeKind::Poker,
            ModeRunner::Duel(_) => ModeKind::CardDuel,
            ModeRunner::Golf(_) => ModeKind::Golf,
            ModeRunner::Race(_) => ModeKind::Race,
            ModeRunner::PropHunt(_) => ModeKind::PropHunt,
        }
    }

    /// Advance one frame; returns this tick's events as log lines.
    pub fn tick(&mut self, pads: &[PadWord]) -> Vec<String> {
        match &mut self.runner {
            ModeRunner::Poker(s) => s.tick(pads).iter().filter_map(poker_line).collect(),
            ModeRunner::Duel(s) => s.tick(pads).iter().filter_map(duel_line).collect(),
            ModeRunner::Golf(s) => s.tick(pads).iter().filter_map(golf_line).collect(),
            ModeRunner::Race(s) => s.tick(pads).iter().filter_map(race_line).collect(),
            ModeRunner::PropHunt(s) => s.tick(pads).iter().filter_map(hunt_line).collect(),
        }
    }

    pub fn view(&self) -> ModeView {
        match &self.runner {
            ModeRunner::Poker(s) => ModeView::Poker(s.view()),
            ModeRunner::Duel(s) => ModeView::Duel(s.view()),
            ModeRunner::Golf(s) => ModeView::Golf(s.view()),
            ModeRunner::Race(s) => ModeView::Race(s.view()),
            ModeRunner::PropHunt(s) => ModeView::PropHunt(s.view()),
        }
    }

    pub fn is_finished(&self) -> bool {
        match &self.runner {
            ModeRunner::Poker(s) => s.is_finished(),
            ModeRunner::Duel(s) => s.is_finished(),
            ModeRunner::Golf(s) => s.is_finished(),
            ModeRunner::Race(s) => s.is_finished(),
            ModeRunner::PropHunt(s) => s.is_finished(),
        }
    }

    pub fn results(&self) -> Option<ModeResults> {
        match &self.runner {
            ModeRunner::Poker(s) => s.results(),
            ModeRunner::Duel(s) => s.results(),
            ModeRunner::Golf(s) => s.results(),
            ModeRunner::Race(s) => s.results(),
            ModeRunner::PropHunt(s) => s.results(),
        }
    }

    pub fn state_hash(&self) -> u64 {
        match &self.runner {
            ModeRunner::Poker(s) => s.state_hash(),
            ModeRunner::Duel(s) => s.state_hash(),
            ModeRunner::Golf(s) => s.state_hash(),
            ModeRunner::Race(s) => s.state_hash(),
            ModeRunner::PropHunt(s) => s.state_hash(),
        }
    }

    /// The finished session's settlement: its results, plus a win's
    /// mode-source reward rules from the card section rolled on `rng`
    /// (granted to seat 0). `None` before the session ends.
    pub fn settle(&self, rng: &mut ModeRng) -> Option<Settlement> {
        let mut results = self.results()?;
        let place = results
            .placements
            .iter()
            .find(|p| p.seat == 0)
            .map_or(u8::MAX, |p| p.place);
        if place == 1 {
            let source = RewardSource::Mode {
                id: self.id.clone(),
            };
            for g in self.book.roll_rewards(&source, rng) {
                results.rewards.push(match g {
                    crate::cards::CardGrant::Booster(booster) => {
                        Reward::Booster { seat: 0, booster }
                    }
                    crate::cards::CardGrant::Card(card) => Reward::Card { seat: 0, card },
                });
            }
        }
        Some(Settlement { results, place })
    }

    /// The golf hole in play.
    pub fn golf_course(&self) -> Option<&Course> {
        match &self.runner {
            ModeRunner::Golf(s) => Some(s.course()),
            _ => None,
        }
    }

    /// The race track.
    pub fn race_track(&self) -> Option<&Track> {
        match &self.runner {
            ModeRunner::Race(s) => Some(s.track()),
            _ => None,
        }
    }

    /// The prop hunt arena.
    pub fn hunt_arena(&self) -> Option<&Arena> {
        match &self.runner {
            ModeRunner::PropHunt(s) => Some(s.arena()),
            _ => None,
        }
    }

    /// Each prop kind's radius in world units, by prop index.
    pub fn hunt_prop_radii(&self) -> Vec<i32> {
        match &self.runner {
            ModeRunner::PropHunt(s) => s.prop_radii(),
            _ => Vec::new(),
        }
    }

    /// A card's display name (its id when the book has none).
    pub fn card_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.card_names.get(id).map_or(id, String::as_str)
    }

    /// The poker table directly, for a host that collects actions without
    /// the pad menu.
    pub fn poker_mut(&mut self) -> Option<&mut PokerSession> {
        match &mut self.runner {
            ModeRunner::Poker(s) => Some(s),
            _ => None,
        }
    }
}

/// A poker hand category as a player reads it.
pub fn category_label(c: crate::poker::hand::Category) -> &'static str {
    use crate::poker::hand::Category as C;
    match c {
        C::HighCard => "High card",
        C::Pair => "Pair",
        C::TwoPair => "Two pair",
        C::Trips => "Three of a kind",
        C::Straight => "Straight",
        C::Flush => "Flush",
        C::FullHouse => "Full house",
        C::Quads => "Four of a kind",
        C::StraightFlush => "Straight flush",
    }
}

/// How a mode's placement score reads ("chips", "strokes").
pub fn score_unit(kind: ModeKind) -> &'static str {
    match kind {
        ModeKind::Poker => "chips",
        ModeKind::CardDuel => "cards",
        ModeKind::Golf => "strokes",
        ModeKind::Race => "frames",
        ModeKind::PropHunt => "points",
    }
}

/// Seat `seat` as a player reads it: "You" for seat 0.
pub fn seat_name(seat: usize) -> String {
    if seat == 0 {
        "You".to_string()
    } else {
        format!("Seat {}", seat + 1)
    }
}

fn poker_line(e: &PokerEvent) -> Option<String> {
    Some(match e {
        PokerEvent::HandStarted { hand, .. } => format!("Hand {hand}"),
        PokerEvent::Acted { seat, action } => {
            let who = seat_name(*seat);
            match action {
                Action::Fold => format!("{who}: fold"),
                Action::Check => format!("{who}: check"),
                Action::Call => format!("{who}: call"),
                Action::RaiseTo(n) => format!("{who}: raise to {n}"),
                Action::AllIn => format!("{who}: all in"),
            }
        }
        PokerEvent::PotAwarded {
            winners,
            amount,
            hand,
            ..
        } => {
            let who: Vec<String> = winners.iter().map(|&s| seat_name(s)).collect();
            match hand {
                Some(h) => format!(
                    "{} win {amount} ({})",
                    who.join(", "),
                    category_label(h.category())
                ),
                None => format!("{} win {amount}", who.join(", ")),
            }
        }
        PokerEvent::SeatBusted { seat } => format!("{} busted", seat_name(*seat)),
        PokerEvent::Finished => "Table closed".to_string(),
        PokerEvent::BlindPosted { .. } | PokerEvent::Dealt { .. } => return None,
    })
}

fn duel_line(e: &DuelEvent) -> Option<String> {
    Some(match e {
        DuelEvent::Captured { by, chained, .. } => {
            let what = if *chained { "chain capture" } else { "capture" };
            format!("{}: {what}", seat_name(*by))
        }
        DuelEvent::GameOver { scores } => format!("Final {} - {}", scores[0], scores[1]),
        DuelEvent::SpoilTaken { seat, card } => format!("{} takes {card}", seat_name(*seat)),
        DuelEvent::Placed { .. } => return None,
    })
}

fn golf_line(e: &GolfEvent) -> Option<String> {
    Some(match e {
        GolfEvent::HoleStarted { hole } => format!("Hole {}", hole + 1),
        GolfEvent::Hazard { seat } => format!("{}: hazard, +1 stroke", seat_name(*seat)),
        GolfEvent::Holed { seat, strokes } => {
            format!("{}: holed in {strokes}", seat_name(*seat))
        }
        GolfEvent::PickedUp { seat } => format!("{}: picked up", seat_name(*seat)),
        GolfEvent::Finished => "Round over".to_string(),
        GolfEvent::Struck { .. } | GolfEvent::WallHit { .. } | GolfEvent::HoleFinished { .. } => {
            return None;
        }
    })
}

fn race_line(e: &RaceEvent) -> Option<String> {
    Some(match e {
        RaceEvent::Go => "Go!".to_string(),
        RaceEvent::Lap { seat, lap } => format!("{}: lap {}", seat_name(*seat), lap + 1),
        RaceEvent::Finished { seat, place, .. } => {
            format!("{} finished #{place}", seat_name(*seat))
        }
        RaceEvent::RaceOver => "Race over".to_string(),
        _ => return None,
    })
}

fn hunt_line(e: &HuntEvent) -> Option<String> {
    Some(match e {
        HuntEvent::RoundStarted { round, .. } => format!("Round {}", round + 1),
        HuntEvent::HuntersReleased => "Hunters released".to_string(),
        HuntEvent::Found { hunter, hider } => {
            format!("{} found {}", seat_name(*hunter), seat_name(*hider))
        }
        HuntEvent::Missed { hunter, .. } => format!("{}: decoy!", seat_name(*hunter)),
        HuntEvent::HunterOut { seat } => format!("{} is out", seat_name(*seat)),
        HuntEvent::Taunt { seat, .. } => format!("{} taunts", seat_name(*seat)),
        HuntEvent::RoundOver { hunters_won } => if *hunters_won {
            "Hunters win the round"
        } else {
            "Hiders win the round"
        }
        .to_string(),
        HuntEvent::Finished => "Match over".to_string(),
        HuntEvent::PropChosen { .. } => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_offers_every_kind_and_builds() {
        let (modes, cards) = builtin_sections();
        let list = entries(modes);
        let kinds: Vec<ModeKind> = list.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ModeKind::Poker,
                ModeKind::CardDuel,
                ModeKind::Golf,
                ModeKind::Race,
                ModeKind::PropHunt
            ]
        );
        for e in &list {
            let m = ActiveMode::build(modes, cards, &e.id, 7, &BuildOptions::default())
                .unwrap_or_else(|err| panic!("{err}"));
            assert_eq!(m.kind(), e.kind);
            assert!(!m.is_finished());
        }
        assert_eq!(
            ActiveMode::build(modes, cards, "nope", 0, &BuildOptions::default()).unwrap_err(),
            BuildError::UnknownInstance("nope".into())
        );
    }

    #[test]
    fn a_won_mode_rolls_its_reward_rules() {
        let (modes, cards) = builtin_sections();
        let mut m =
            ActiveMode::build(modes, cards, "builtin-golf", 3, &BuildOptions::default()).unwrap();
        assert!(m.settle(&mut ModeRng::new(0)).is_none());
        // Strike every shot straight at full power from the player's pad;
        // the AI seat plays itself.
        let mut frames = 0;
        while !m.is_finished() && frames < 60 * 60 * 20 {
            let press = if frames % 8 == 0 {
                legaia_engine_system::pad::PadButton::Cross.mask()
            } else {
                0
            };
            m.tick(&[press]);
            frames += 1;
        }
        let s = m.settle(&mut ModeRng::new(0)).expect("the round ends");
        assert_eq!(s.results.placements.len(), 2);
        let booster = Reward::Booster {
            seat: 0,
            booster: "builtin-booster".into(),
        };
        assert_eq!(s.place == 1, s.results.rewards.contains(&booster));
    }
}

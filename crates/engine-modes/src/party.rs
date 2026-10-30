//! Party night: a points-based playlist of short rounds of the other
//! modes, with a scoreboard between rounds, catch-up rules and prizes for
//! the top places.
//!
//! [`PartyNight`] is the night's whole state and its director. It plays no
//! round itself: a host ticks it once per frame ([`PartyNight::tick`]), and
//! when it cues [`PartyCue::Start`] the host builds the round's session from
//! [`round_section`] (one instance of the base `modes.toml`, cut short by the
//! round's knobs), plays it, and reports the placements back
//! ([`PartyNight::record`]). A Grid Duel round is a king-of-the-hill ladder
//! of 1v1 matches ([`Hill`]); the host reports each match's winner
//! ([`PartyNight::record_duel`]).
//!
//! The night is deterministic and serialisable: two machines that build it
//! from the same definition, players and seed and report the same results
//! hold the same scoreboard, which [`PartyNight::scoreboard_hash`] digests
//! (timers excluded - they are presentation). A networked night keeps one
//! copy per peer; the room's host directs and the others verify the hash.
//!
//! The rules:
//!
//! - **Points by placement** from the night's table; tied seats share a
//!   place and its points; a place past the table scores nothing, and a
//!   player who has left scores nothing (their total stands).
//! - **Double points** on a round marked `double`, and on the final round
//!   when `double_final` is set.
//! - **Wager round**: every player still in stakes `wager` percent of their
//!   points (rounded down, never below zero) into a pot; the round's winners
//!   split it, the remainder to the earliest-seated winner.
//! - **Last place picks**: with `last_place_picks`, before every round
//!   after the first, the player in last place picks which remaining round
//!   comes next (an AI rival, or a player who has left, picks at once; a
//!   pick not made in time takes the highlighted round).
//! - **Voiding**: a round that cannot finish (a desync, a player ending
//!   it) is recorded void - no points, no stakes - and the night goes on.

use legaia_expansion_format::modes::{
    ModeSection, PartyNight as NightDef, PartyOrder, PartyPrize, PartyRound,
};
use serde::{Deserialize, Serialize};

use crate::active::ModeKind;
use crate::session::{ModeRng, StateHasher};

/// Frames the round-intro card stands before a round starts.
pub const INTRO_FRAMES: u32 = 4 * 60;
/// Frames the card stands between two matches of a Grid Duel ladder.
pub const MATCH_INTRO_FRAMES: u32 = 2 * 60;
/// Frames the scoreboard stands between rounds.
pub const BOARD_FRAMES: u32 = 5 * 60;
/// Frames the last-place player has to pick the next round.
pub const PICK_FRAMES: u32 = 10 * 60;
/// Frames the podium stands before the night is over.
pub const PODIUM_FRAMES: u32 = 15 * 60;
/// The most players a night seats.
pub const MAX_PARTY_PLAYERS: usize = 8;

/// A player's key: the netplay player id, or the seat index offline.
pub type PlayerKey = u32;

/// One player on the scoreboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyPlayer {
    pub key: PlayerKey,
    pub name: String,
    pub score: i64,
    /// A person (offline: the player; online: every room member). An AI
    /// rival picks its round at once.
    pub human: bool,
    /// Left the night: no more points, the total stands, no prize.
    pub left: bool,
}

/// One round as the screens title it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundInfo {
    /// The instance id the round plays.
    pub mode: String,
    pub kind: ModeKind,
    /// "Tower Green".
    pub title: String,
    /// "1 hole", "3 hands, 300 chips", "King of the hill".
    pub detail: String,
    pub double: bool,
    /// Wager percentage (0 = none).
    pub wager: u8,
}

/// One player's line of a played round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundRow {
    pub key: PlayerKey,
    /// `None`: did not place (had left, or held no seat).
    pub place: Option<u8>,
    /// Points gained (stake lost and pot share included).
    pub points: i64,
}

/// A played (or voided) round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundRecord {
    /// Index into the night's rounds.
    pub slot: usize,
    pub voided: bool,
    /// Best first; empty when voided.
    pub rows: Vec<RoundRow>,
    /// The wager pot (0 = none).
    pub pot: i64,
    /// 1, or 2 for double points.
    pub multiplier: u8,
}

/// A Grid Duel round's king-of-the-hill ladder: the two players lowest on
/// the scoreboard duel first, the winner holds the hill against the next,
/// and the leader defends last. A draw keeps the king.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hill {
    pub king: PlayerKey,
    /// Challengers still to come, next first.
    pub challengers: Vec<PlayerKey>,
    /// Knocked off the hill, first out first.
    pub out: Vec<PlayerKey>,
    /// Matches in the ladder, and played so far.
    pub matches: u8,
    pub played: u8,
}

/// Where the night is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartyPhase {
    /// The round-intro card for round `slot`.
    Intro { slot: usize, timer: u32 },
    /// Round `slot` (one match of it, for a Grid Duel) is in play.
    Playing { slot: usize },
    /// The scoreboard after a round.
    Board { timer: u32 },
    /// The last-place player picks the next round from the queue.
    Pick {
        picker: PlayerKey,
        cursor: usize,
        timer: u32,
    },
    /// The final podium.
    Podium { timer: u32 },
    /// Done; `early` when it ended before its last round.
    Over { early: bool },
}

/// What [`PartyNight::tick`] asks of the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartyCue {
    None,
    /// Build round `slot` and play it (a Grid Duel: the match
    /// [`PartyNight::current_match`] names).
    Start {
        slot: usize,
    },
    /// The night reached its podium: grant the prizes.
    Podium,
    /// The podium has stood its time.
    Over,
}

/// Why a night could not be set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartyError {
    NoRounds,
    TooFewPlayers,
    /// A round names an instance the section does not have.
    UnknownInstance(String),
    /// A golf round's hole is past the course's last.
    BadHole {
        mode: String,
        hole: u8,
    },
}

impl std::fmt::Display for PartyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRounds => f.write_str("the night has no rounds"),
            Self::TooFewPlayers => f.write_str("a party night needs two players"),
            Self::UnknownInstance(id) => write!(f, "no mode instance `{id}`"),
            Self::BadHole { mode, hole } => write!(f, "`{mode}` has no hole {hole}"),
        }
    }
}

/// One final standing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    pub key: PlayerKey,
    pub name: String,
    pub score: i64,
    /// 1-based; tied scores share a place.
    pub place: u8,
    pub left: bool,
}

/// The night.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyNight {
    /// The definition's id.
    pub id: String,
    pub name: String,
    /// The definition's rounds (the knobs [`round_section`] applies).
    pub rounds: Vec<PartyRound>,
    /// Each round as the screens title it, by slot.
    pub infos: Vec<RoundInfo>,
    pub points: Vec<u32>,
    pub double_final: bool,
    pub last_place_picks: bool,
    pub prizes: Vec<PartyPrize>,
    /// How hard a night played alone's rivals (and every AI seat) play.
    pub ai_tier: legaia_expansion_format::modes::AiTier,
    pub players: Vec<PartyPlayer>,
    /// Slots still to play, the next (or the one in play) first.
    pub queue: Vec<usize>,
    pub history: Vec<RoundRecord>,
    pub phase: PartyPhase,
    /// The Grid Duel ladder of the round in play.
    pub hill: Option<Hill>,
    rng: ModeRng,
}

/// `def`'s rounds as they read on screen, checked against `base`.
fn round_infos(def: &NightDef, base: &ModeSection) -> Result<Vec<RoundInfo>, PartyError> {
    def.rounds.iter().map(|r| round_info(r, base)).collect()
}

fn plural(n: u32, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn pick_name(round: &str, instance: &str, id: &str) -> String {
    [round, instance, id]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn round_info(r: &PartyRound, base: &ModeSection) -> Result<RoundInfo, PartyError> {
    let id = r.mode.as_str();
    let (kind, title, detail) = if let Some(t) = base.poker.iter().find(|t| t.id == id) {
        let stack = r.stack.unwrap_or(t.buy_in);
        let hands = r.hands.or(t.max_hands);
        let detail = match hands {
            Some(h) => format!("{}, {stack} chips", plural(h, "hand", "hands")),
            None => format!("{stack} chips, last stack wins"),
        };
        (ModeKind::Poker, pick_name(&r.name, &t.name, id), detail)
    } else if let Some(d) = base.card_duels.iter().find(|d| d.id == id) {
        (
            ModeKind::CardDuel,
            pick_name(&r.name, &d.name, id),
            "King of the hill".to_string(),
        )
    } else if let Some(c) = base.golf.iter().find(|c| c.id == id) {
        if let Some(&hole) = r
            .holes
            .iter()
            .find(|&&h| h == 0 || usize::from(h) > c.holes.len())
        {
            return Err(PartyError::BadHole {
                mode: id.to_string(),
                hole,
            });
        }
        let n = if r.holes.is_empty() {
            c.holes.len()
        } else {
            r.holes.len()
        };
        (
            ModeKind::Golf,
            pick_name(&r.name, &c.name, id),
            plural(n as u32, "hole", "holes"),
        )
    } else if let Some(t) = base.races.iter().find(|t| t.id == id) {
        let laps = r.laps.unwrap_or(t.laps).max(1);
        (
            ModeKind::Race,
            pick_name(&r.name, &t.name, id),
            plural(u32::from(laps), "lap", "laps"),
        )
    } else if let Some(a) = base.prop_hunt.iter().find(|a| a.id == id) {
        let rounds = r.rounds.unwrap_or(a.rounds).max(1);
        (
            ModeKind::PropHunt,
            pick_name(&r.name, &a.name, id),
            plural(u32::from(rounds), "round", "rounds"),
        )
    } else {
        return Err(PartyError::UnknownInstance(id.to_string()));
    };
    Ok(RoundInfo {
        mode: id.to_string(),
        kind,
        title,
        detail,
        double: r.double,
        wager: r.wager.min(50),
    })
}

/// The one-instance mode section round `r` plays: its instance from `base`
/// cut short by the round's knobs, with room for `seats` players (a poker
/// table, a race grid and a prop hunt arena widen to seat everyone). The
/// instance keeps its id; its name becomes the round's title.
pub fn round_section(
    base: &ModeSection,
    r: &PartyRound,
    seats: usize,
) -> Result<ModeSection, PartyError> {
    let info = round_info(r, base)?;
    let id = r.mode.as_str();
    let seats = seats.clamp(2, MAX_PARTY_PLAYERS);
    let mut out = ModeSection::default();
    if let Some(t) = base.poker.iter().find(|t| t.id == id) {
        let mut t = t.clone();
        t.name.clone_from(&info.title);
        t.seats = t.seats.max(seats as u8).clamp(2, 6);
        if let Some(s) = r.stack {
            t.buy_in = s;
        }
        if r.hands.is_some() {
            t.max_hands = r.hands;
        }
        out.poker.push(t);
    } else if let Some(d) = base.card_duels.iter().find(|d| d.id == id) {
        let mut d = d.clone();
        d.name.clone_from(&info.title);
        // A party duel bets nothing: no card changes hands.
        d.stakes = false;
        d.win_booster = None;
        out.card_duels.push(d);
    } else if let Some(c) = base.golf.iter().find(|c| c.id == id) {
        let mut c = c.clone();
        c.name.clone_from(&info.title);
        if !r.holes.is_empty() {
            c.holes = r
                .holes
                .iter()
                .map(|&h| c.holes[usize::from(h) - 1].clone())
                .collect();
        }
        out.golf.push(c);
    } else if let Some(t) = base.races.iter().find(|t| t.id == id) {
        let mut t = t.clone();
        t.name.clone_from(&info.title);
        if let Some(l) = r.laps {
            t.laps = l.max(1);
        }
        t.racers = t.racers.max(seats as u8);
        out.races.push(t);
    } else if let Some(a) = base.prop_hunt.iter().find(|a| a.id == id) {
        let mut a = a.clone();
        a.name.clone_from(&info.title);
        if let Some(n) = r.rounds {
            a.rounds = n.max(1);
        }
        a.players = a.players.max(seats as u8).clamp(2, MAX_PARTY_PLAYERS as u8);
        a.hunters = a.hunters.clamp(1, a.players - 1);
        out.prop_hunt.push(a);
    }
    Ok(out)
}

impl PartyNight {
    /// A night of `def` over `base`'s instances for `players` (key, name,
    /// human), in seat order, its shuffle and AI picks drawn from `seed`.
    pub fn new(
        def: &NightDef,
        base: &ModeSection,
        players: &[(PlayerKey, String, bool)],
        seed: u64,
    ) -> Result<Self, PartyError> {
        if def.rounds.is_empty() {
            return Err(PartyError::NoRounds);
        }
        if players.len() < 2 {
            return Err(PartyError::TooFewPlayers);
        }
        let infos = round_infos(def, base)?;
        let mut rng = ModeRng::new(seed ^ 0x50_4152_5459);
        let mut queue: Vec<usize> = (0..def.rounds.len()).collect();
        if def.order == PartyOrder::Shuffle {
            rng.shuffle(&mut queue);
        }
        let mut night = Self {
            id: def.id.clone(),
            name: if def.name.is_empty() {
                def.id.clone()
            } else {
                def.name.clone()
            },
            rounds: def.rounds.clone(),
            infos,
            points: def.points.clone(),
            double_final: def.double_final,
            last_place_picks: def.last_place_picks,
            prizes: def.prizes.clone(),
            ai_tier: def.ai_tier,
            players: players
                .iter()
                .take(MAX_PARTY_PLAYERS)
                .map(|(key, name, human)| PartyPlayer {
                    key: *key,
                    name: name.clone(),
                    score: 0,
                    human: *human,
                    left: false,
                })
                .collect(),
            queue,
            history: Vec::new(),
            phase: PartyPhase::Over { early: false },
            hill: None,
            rng,
        };
        night.intro_next();
        Ok(night)
    }

    /// Rounds in the night.
    pub fn total_rounds(&self) -> usize {
        self.infos.len()
    }

    /// The 1-based number of the round being introduced or played (or the
    /// next one, on the scoreboard).
    pub fn round_number(&self) -> usize {
        match self.phase {
            PartyPhase::Intro { .. } | PartyPhase::Playing { .. } => self.history.len() + 1,
            _ => (self.history.len() + 1).min(self.total_rounds()),
        }
    }

    /// The round being introduced or played.
    pub fn current_slot(&self) -> Option<usize> {
        match self.phase {
            PartyPhase::Intro { slot, .. } | PartyPhase::Playing { slot } => Some(slot),
            _ => None,
        }
    }

    /// The round on deck after this phase (the scoreboard's "Next").
    pub fn next_slot(&self) -> Option<usize> {
        match self.phase {
            PartyPhase::Pick { cursor, .. } => self.queue.get(cursor).copied(),
            _ => self.queue.first().copied(),
        }
    }

    /// Whether the round in `slot` scores double.
    pub fn multiplier(&self, slot: usize) -> u8 {
        let last = self.queue.len() <= 1;
        if self.infos.get(slot).is_some_and(|i| i.double) || (self.double_final && last) {
            2
        } else {
            1
        }
    }

    /// A player by key.
    pub fn player(&self, key: PlayerKey) -> Option<&PartyPlayer> {
        self.players.iter().find(|p| p.key == key)
    }

    /// Whether `key` is in the night and has not left.
    pub fn is_active(&self, key: PlayerKey) -> bool {
        self.player(key).is_some_and(|p| !p.left)
    }

    fn active_keys(&self) -> Vec<PlayerKey> {
        self.players
            .iter()
            .filter(|p| !p.left)
            .map(|p| p.key)
            .collect()
    }

    /// The scoreboard, best first: score, then seat order; tied scores
    /// share a place.
    pub fn standings(&self) -> Vec<Standing> {
        let mut order: Vec<usize> = (0..self.players.len()).collect();
        order.sort_by(|&a, &b| {
            self.players[b]
                .score
                .cmp(&self.players[a].score)
                .then(a.cmp(&b))
        });
        let mut out: Vec<Standing> = Vec::with_capacity(order.len());
        for (i, &p) in order.iter().enumerate() {
            let pl = &self.players[p];
            let place = match out.last() {
                Some(prev) if prev.score == pl.score => prev.place,
                _ => (i + 1) as u8,
            };
            out.push(Standing {
                key: pl.key,
                name: pl.name.clone(),
                score: pl.score,
                place,
                left: pl.left,
            });
        }
        out
    }

    /// The night ran to its podium.
    pub fn completed(&self) -> bool {
        matches!(
            self.phase,
            PartyPhase::Podium { .. } | PartyPhase::Over { early: false }
        )
    }

    /// The night is over (its podium has stood, or it ended early).
    pub fn is_over(&self) -> bool {
        matches!(self.phase, PartyPhase::Over { .. })
    }

    /// What `key` won: the prize for their final place, once the night has
    /// completed; nothing for a player who left, a night ended early, or a
    /// night whose every round was void.
    pub fn prize_for(&self, key: PlayerKey) -> Option<&PartyPrize> {
        if !self.completed() || self.history.iter().all(|r| r.voided) {
            return None;
        }
        let s = self.standings().into_iter().find(|s| s.key == key)?;
        if s.left {
            return None;
        }
        self.prizes.iter().find(|p| p.place == s.place)
    }

    /// The Grid Duel match in play or on deck: (king, challenger).
    pub fn current_match(&self) -> Option<(PlayerKey, PlayerKey)> {
        let h = self.hill.as_ref()?;
        Some((h.king, *h.challengers.first()?))
    }

    /// One frame of the night's own clock.
    pub fn tick(&mut self) -> PartyCue {
        match &mut self.phase {
            PartyPhase::Intro { slot, timer } => {
                *timer = timer.saturating_sub(1);
                if *timer == 0 {
                    let slot = *slot;
                    self.phase = PartyPhase::Playing { slot };
                    return PartyCue::Start { slot };
                }
                PartyCue::None
            }
            PartyPhase::Board { timer } => {
                *timer = timer.saturating_sub(1);
                if *timer == 0 {
                    return self.advance();
                }
                PartyCue::None
            }
            PartyPhase::Pick { cursor, timer, .. } => {
                *timer = timer.saturating_sub(1);
                if *timer == 0 {
                    let c = *cursor;
                    self.choose(c);
                }
                PartyCue::None
            }
            PartyPhase::Podium { timer } => {
                *timer = timer.saturating_sub(1);
                if *timer == 0 {
                    self.phase = PartyPhase::Over { early: false };
                    return PartyCue::Over;
                }
                PartyCue::None
            }
            PartyPhase::Playing { .. } | PartyPhase::Over { .. } => PartyCue::None,
        }
    }

    /// Leave the scoreboard: the podium, a pick, or the next intro.
    fn advance(&mut self) -> PartyCue {
        if self.queue.is_empty() {
            self.phase = PartyPhase::Podium {
                timer: PODIUM_FRAMES,
            };
            return PartyCue::Podium;
        }
        if self.active_keys().len() < 2 {
            self.end_early();
            return PartyCue::None;
        }
        if self.last_place_picks && !self.history.is_empty() && self.queue.len() > 1 {
            let picker = self
                .standings()
                .into_iter()
                .rev()
                .find(|s| !s.left)
                .map(|s| s.key);
            if let Some(picker) = picker {
                let human = self.player(picker).is_some_and(|p| p.human);
                if human {
                    self.phase = PartyPhase::Pick {
                        picker,
                        cursor: 0,
                        timer: PICK_FRAMES,
                    };
                } else {
                    let c = self.rng.below(self.queue.len() as u32) as usize;
                    self.choose(c);
                }
                return PartyCue::None;
            }
        }
        self.intro_next();
        PartyCue::None
    }

    /// Move queue entry `i` to the front and introduce it.
    fn choose(&mut self, i: usize) {
        if i < self.queue.len() {
            let slot = self.queue.remove(i);
            self.queue.insert(0, slot);
        }
        self.intro_next();
    }

    fn intro_next(&mut self) {
        let Some(&slot) = self.queue.first() else {
            self.phase = PartyPhase::Podium {
                timer: PODIUM_FRAMES,
            };
            return;
        };
        self.hill = None;
        if self.infos[slot].kind == ModeKind::CardDuel {
            // The two lowest duel first; the leader defends last.
            let mut ladder: Vec<PlayerKey> = self
                .standings()
                .into_iter()
                .rev()
                .filter(|s| !s.left)
                .map(|s| s.key)
                .collect();
            if ladder.len() >= 2 {
                let king = ladder.remove(0);
                let matches = ladder.len() as u8;
                self.hill = Some(Hill {
                    king,
                    challengers: ladder,
                    out: Vec::new(),
                    matches,
                    played: 0,
                });
            }
        }
        self.phase = PartyPhase::Intro {
            slot,
            timer: INTRO_FRAMES,
        };
    }

    /// The last-place player's pick input: `step` moves the highlight
    /// through the remaining rounds, `confirm` takes it. Anyone else's input
    /// is ignored. Returns whether it changed anything.
    pub fn pick(&mut self, key: PlayerKey, step: i32, confirm: bool) -> bool {
        let n = self.queue.len();
        let PartyPhase::Pick { picker, cursor, .. } = &mut self.phase else {
            return false;
        };
        if *picker != key || n == 0 {
            return false;
        }
        let before = *cursor;
        *cursor = (*cursor as i32 + step).rem_euclid(n as i32) as usize;
        if confirm {
            let c = *cursor;
            self.choose(c);
            return true;
        }
        *cursor != before
    }

    /// The round (or Grid Duel ladder) in play finished with `placements`
    /// (player key, 1-based place; tied keys share a place). Players not
    /// listed, and players who have left, score nothing. No-op outside a
    /// round in play.
    pub fn record(&mut self, placements: &[(PlayerKey, u8)]) -> Option<&RoundRecord> {
        let PartyPhase::Playing { slot } = self.phase else {
            return None;
        };
        let mult = self.multiplier(slot);
        let wager = self.infos[slot].wager;
        let place_of = |key: PlayerKey| {
            placements
                .iter()
                .filter(|(k, _)| *k == key)
                .map(|(_, p)| *p)
                .min()
        };
        let mut rows: Vec<RoundRow> = Vec::with_capacity(self.players.len());
        let mut pot = 0i64;
        for p in &self.players {
            let place = if p.left { None } else { place_of(p.key) };
            let stake = if wager > 0 && !p.left {
                p.score.max(0) * i64::from(wager) / 100
            } else {
                0
            };
            pot += stake;
            let base = place
                .and_then(|pl| self.points.get(usize::from(pl.max(1)) - 1))
                .map_or(0, |&pts| i64::from(pts) * i64::from(mult));
            rows.push(RoundRow {
                key: p.key,
                place,
                points: base - stake,
            });
        }
        if pot > 0 {
            let winners: Vec<usize> = rows
                .iter()
                .enumerate()
                .filter(|(_, r)| r.place == Some(1))
                .map(|(i, _)| i)
                .collect();
            if let Some(&first) = winners.first() {
                let share = pot / winners.len() as i64;
                for &w in &winners {
                    rows[w].points += share;
                }
                rows[first].points += pot - share * winners.len() as i64;
            } else {
                // Nobody won: the stakes go back.
                for (r, p) in rows.iter_mut().zip(&self.players) {
                    if !p.left {
                        r.points += p.score.max(0) * i64::from(wager) / 100;
                    }
                }
                pot = 0;
            }
        }
        for (p, r) in self.players.iter_mut().zip(&rows) {
            p.score += r.points;
        }
        rows.sort_by_key(|r| (r.place.unwrap_or(u8::MAX), std::cmp::Reverse(r.points)));
        self.finish_round(RoundRecord {
            slot,
            voided: false,
            rows,
            pot,
            multiplier: mult,
        });
        self.history.last()
    }

    /// The Grid Duel match in play ended: `winner` (`None` = a draw, which
    /// keeps the king). The ladder moves to its next match, or, after the
    /// last, the round is recorded: the last king first, then the others in
    /// reverse order of falling.
    pub fn record_duel(&mut self, winner: Option<PlayerKey>) -> Option<&RoundRecord> {
        let PartyPhase::Playing { slot } = self.phase else {
            return None;
        };
        let h = self.hill.as_mut()?;
        let challenger = *h.challengers.first()?;
        h.challengers.remove(0);
        h.played += 1;
        if winner == Some(challenger) {
            h.out.push(h.king);
            h.king = challenger;
        } else {
            h.out.push(challenger);
        }
        if !h.challengers.is_empty() {
            self.phase = PartyPhase::Intro {
                slot,
                timer: MATCH_INTRO_FRAMES,
            };
            return None;
        }
        let mut placements = vec![(h.king, 1u8)];
        placements.extend(
            h.out
                .iter()
                .rev()
                .enumerate()
                .map(|(i, &k)| (k, i as u8 + 2)),
        );
        self.record(&placements)
    }

    /// The round in play (or about to be) could not finish: it is recorded
    /// void - no points, no stakes - and the scoreboard follows.
    pub fn void_round(&mut self) -> Option<&RoundRecord> {
        let (PartyPhase::Playing { slot } | PartyPhase::Intro { slot, .. }) = self.phase else {
            return None;
        };
        let multiplier = self.multiplier(slot);
        self.finish_round(RoundRecord {
            slot,
            voided: true,
            rows: Vec::new(),
            pot: 0,
            multiplier,
        });
        self.history.last()
    }

    fn finish_round(&mut self, rec: RoundRecord) {
        if self.queue.first() == Some(&rec.slot) {
            self.queue.remove(0);
        } else {
            self.queue.retain(|&s| s != rec.slot);
        }
        self.hill = None;
        self.history.push(rec);
        self.phase = PartyPhase::Board {
            timer: BOARD_FRAMES,
        };
    }

    /// `key` left the night: their total stands, they score no more. A
    /// pick they owed is made for them; a ladder they stood in drops them
    /// (a match in play is the host's to settle).
    pub fn mark_left(&mut self, key: PlayerKey) {
        let Some(p) = self.players.iter_mut().find(|p| p.key == key) else {
            return;
        };
        p.left = true;
        if let Some(h) = self.hill.as_mut() {
            h.challengers.retain(|&k| k != key);
        }
        if let PartyPhase::Pick { picker, cursor, .. } = self.phase
            && picker == key
        {
            self.choose(cursor);
        }
    }

    /// End the night now, before its podium: no prizes.
    pub fn end_early(&mut self) {
        if !self.completed() {
            self.phase = PartyPhase::Over { early: true };
        }
    }

    /// The scoreboard's digest: players, totals, history, the queue, the
    /// phase (without its timer) and the ladder. Two copies of one night
    /// that took the same results are equal here.
    pub fn scoreboard_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.str(&self.id);
        h.usize(self.players.len());
        for p in &self.players {
            h.u32(p.key);
            h.i64(p.score);
            h.bool(p.left);
        }
        h.usize(self.history.len());
        for r in &self.history {
            h.usize(r.slot);
            h.bool(r.voided);
            h.i64(r.pot);
            h.u8(r.multiplier);
            for row in &r.rows {
                h.u32(row.key);
                h.u8(row.place.unwrap_or(0));
                h.i64(row.points);
            }
        }
        h.usize(self.queue.len());
        for &s in &self.queue {
            h.usize(s);
        }
        match &self.phase {
            PartyPhase::Intro { slot, .. } => {
                h.u8(0);
                h.usize(*slot);
            }
            PartyPhase::Playing { slot } => {
                h.u8(1);
                h.usize(*slot);
            }
            PartyPhase::Board { .. } => h.u8(2),
            PartyPhase::Pick { picker, .. } => {
                h.u8(3);
                h.u32(*picker);
            }
            PartyPhase::Podium { .. } => h.u8(4),
            PartyPhase::Over { early } => {
                h.u8(5);
                h.bool(*early);
            }
        }
        if let Some(hill) = &self.hill {
            h.u32(hill.king);
            for &k in hill.challengers.iter().chain(&hill.out) {
                h.u32(k);
            }
        }
        h.finish()
    }

    /// Run the phase timer down for the screens only, with no transition:
    /// a networked guest's copy, which the room's host moves on.
    pub fn tick_display(&mut self) {
        match &mut self.phase {
            PartyPhase::Intro { timer, .. }
            | PartyPhase::Board { timer }
            | PartyPhase::Pick { timer, .. }
            | PartyPhase::Podium { timer } => *timer = timer.saturating_sub(1),
            _ => {}
        }
    }

    /// The phase without its timer, to tell a transition from a tick.
    pub fn phase_key(&self) -> (u8, usize) {
        match &self.phase {
            PartyPhase::Intro { slot, .. } => (
                0,
                *slot * 64 + self.hill.as_ref().map_or(0, |h| usize::from(h.played)),
            ),
            PartyPhase::Playing { slot } => (1, *slot),
            PartyPhase::Board { .. } => (2, self.history.len()),
            PartyPhase::Pick { cursor, .. } => (3, *cursor),
            PartyPhase::Podium { .. } => (4, 0),
            PartyPhase::Over { early } => (5, usize::from(*early)),
        }
    }

    /// Frames left in the phase on screen (0 where none runs).
    pub fn timer(&self) -> u32 {
        match self.phase {
            PartyPhase::Intro { timer, .. }
            | PartyPhase::Board { timer }
            | PartyPhase::Pick { timer, .. }
            | PartyPhase::Podium { timer } => timer,
            _ => 0,
        }
    }
}

/// The players of `def` played alone: the player (key 0, named `you`) and
/// the rivals that fill its seats (keys 1.., named from `def.rivals`, else
/// "Rival 1", ...).
pub fn solo_players(def: &NightDef, you: &str) -> Vec<(PlayerKey, String, bool)> {
    let seats = usize::from(def.players).clamp(2, 6);
    let mut out = vec![(0, you.to_string(), true)];
    for i in 1..seats {
        let name = def
            .rivals
            .get(i - 1)
            .filter(|n| !n.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Rival {i}"));
        out.push((i as PlayerKey, name, false));
    }
    out
}

#[cfg(test)]
#[path = "party_tests.rs"]
mod tests;

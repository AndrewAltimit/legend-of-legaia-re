//! The in-world minigame **status caption**: the engine's own readout of the
//! rules state plus the key hints, for the slot machine, the Baka Fighter
//! duel, the Muscle Dome leg, the dance floor and the fishing pond.
//!
//! None of it is retail - each machine's own HUD is drawn by the retail-shaped
//! builders beside it (the dance frame rows, the Baka digit strips and chrome
//! labels, the slot paylines). These rows are the port's readout while those
//! pages are partial, and one builder per game here authors them for every
//! host, so the wording cannot drift between hosts.
//!
//! The rows are **data, not a draw list**, and they carry no stage position.
//! A status row printed inside the 320x240 frame is covered by whatever the
//! game draws there - both play hosts draw a game's screen primitives over
//! their text layer, so the Baka Fighter title logo sat on the prompt line -
//! and a long row ran off the frame's right edge. [`caption`] is the one
//! entry point, and each host shows what it returns **outside the game
//! picture**: the browser play page in the strip under its canvas (the
//! `play_mg_caption_json` export), the native window in its title bar
//! ([`MinigameCaption::title_line`]). The hosts reach it through
//! `WorldMinigameCaption::minigame_caption` in engine-world-views, which supplies the two Muscle
//! Dome answers that read the world ([`MuscleWorld`]).

use crate::minigame_state::MinigameState;
use legaia_engine_battle_menus::muscle_dome_session::MusclePhase;
use legaia_engine_fishing::fishing::PondSession;
use legaia_engine_minigames::baka_fighter::{BakaFight, MatchPhase};
use legaia_engine_minigames::dance::{DanceGame, Judge, dance_number_digits};
use legaia_engine_minigames::muscle_dome::{ContestFlags, TIME_METER_MAX};
use legaia_engine_minigames::slot_machine::{SlotMachine, SlotPhase};
use legaia_engine_mode::mode::SceneMode;

/// What a caption row says. A host may weight the two differently (the page
/// brightens the readout); the order of the rows is the reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// The rules state: scores, HP, the turn reached, the phase.
    Readout,
    /// What the player can press, and what the last press did.
    Hint,
}

impl RowKind {
    /// The page-facing name (`play_mg_caption_json`'s `kind`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Readout => "readout",
            Self::Hint => "hint",
        }
    }
}

/// One caption row: its role and its clauses in reading order. A clause is
/// one statement ("coins 50", "Start = quit"); the host picks the separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    pub kind: RowKind,
    pub clauses: Vec<String>,
}

impl StatusRow {
    /// A row of `clauses`. Empty clauses are dropped, so a builder can pass a
    /// phase that has nothing to say without leaving a dangling separator.
    fn new<S: Into<String>>(kind: RowKind, clauses: impl IntoIterator<Item = S>) -> Self {
        Self {
            kind,
            clauses: clauses
                .into_iter()
                .map(Into::into)
                .filter(|c: &String| !c.trim().is_empty())
                .collect(),
        }
    }

    fn readout<S: Into<String>>(clauses: impl IntoIterator<Item = S>) -> Self {
        Self::new(RowKind::Readout, clauses)
    }

    fn hint<S: Into<String>>(clauses: impl IntoIterator<Item = S>) -> Self {
        Self::new(RowKind::Hint, clauses)
    }

    /// The row as one line of text.
    pub fn line(&self) -> String {
        self.clauses.join("  ")
    }
}

/// Which minigame a caption belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusGame {
    Slot,
    Baka,
    Muscle,
    Dance,
    Fishing,
}

impl StatusGame {
    /// The game whose session owns `mode`.
    pub fn of_mode(mode: SceneMode) -> Option<Self> {
        match mode {
            SceneMode::SlotMachine => Some(Self::Slot),
            SceneMode::BakaFighter => Some(Self::Baka),
            SceneMode::MuscleDome => Some(Self::Muscle),
            SceneMode::Dance => Some(Self::Dance),
            SceneMode::Fishing => Some(Self::Fishing),
            _ => None,
        }
    }

    /// The page-facing label (`play_mg_caption_json`'s `game`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Slot => "slot",
            Self::Baka => "baka",
            Self::Muscle => "muscle",
            Self::Dance => "dance",
            Self::Fishing => "fishing",
        }
    }
}

/// What only the host can answer about a caption: which of its own surfaces
/// already carries a row's content, and what its keys are called.
#[derive(Debug, Clone, Copy)]
pub struct StatusHost<'a> {
    /// The slot machine itself is on screen (its art decoded and the host
    /// draws the cabinet). Its marquee, lamps and coin readout then carry
    /// everything the slot rows say, so the caption has no rows.
    pub slot_machine_drawn: bool,
    /// The host's names for the fishing keys.
    pub fishing_keys: FishingKeys<'a>,
}

/// A host's names for the keys the fishing rows mention. The pond is the one
/// game whose rows name host keys rather than pad buttons, because the page's
/// bindings are the visitor's to change.
#[derive(Debug, Clone, Copy)]
pub struct FishingKeys<'a> {
    pub cast: &'a str,
    pub reel_a: &'a str,
    pub reel_b: &'a str,
    /// Opens the venue hub (the pad's Triangle).
    pub menu: &'a str,
    /// The engine's minigame escape (the pad's Start).
    pub quit: &'a str,
    /// Opens the point exchange: a key on the native window, a button beside
    /// the frame on the page.
    pub prizes: &'a str,
}

/// A live minigame's caption: the game and its rows this frame. `rows` is
/// empty on a frame the game has nothing to say (the Muscle Dome's command
/// ring, a decided leg, the dance count-in, a slot machine that draws
/// itself). The game is still live then, so a host keeps the caption's seat
/// rather than collapsing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinigameCaption {
    pub game: StatusGame,
    pub rows: Vec<StatusRow>,
}

impl MinigameCaption {
    /// The caption as one line - a window title, a log line. Rows are
    /// separated by `" | "`, clauses by two spaces. Empty when no row is up.
    pub fn title_line(&self) -> String {
        self.rows
            .iter()
            .map(StatusRow::line)
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// What the Muscle Dome rows read off the world rather than the minigame
/// seat: the contest's story-flag gates (`World::muscle_contest_flags`) and
/// the running turn play-out tally (`World::muscle_playback_tally`).
#[derive(Debug, Clone, Copy, Default)]
pub struct MuscleWorld {
    pub contest_flags: ContestFlags,
    pub playback_tally: Option<(usize, i32)>,
}

/// The status caption for the minigame that owns `mode` this frame, or
/// `None` outside one. The one entry point every play host reads (through
/// `WorldMinigameCaption::minigame_caption`): which game is live is the world's mode, and
/// each game's rows come from its builder below.
pub fn caption(
    mode: SceneMode,
    mg: &MinigameState,
    muscle: &MuscleWorld,
    host: &StatusHost<'_>,
) -> Option<MinigameCaption> {
    let game = StatusGame::of_mode(mode)?;
    let rows = match game {
        StatusGame::Slot if host.slot_machine_drawn => Vec::new(),
        StatusGame::Slot => mg
            .slot_machine
            .as_ref()
            .map(slot_status_rows)
            .unwrap_or_default(),
        StatusGame::Baka => mg
            .baka_fighter
            .as_ref()
            .map(baka_status_rows)
            .unwrap_or_default(),
        StatusGame::Muscle => muscle_status_rows(mg, muscle),
        // Withheld while the pre-song count-in banner is up: the rule lives
        // with the phase (`dance_status_visible`), not with a host.
        StatusGame::Dance if !mg.dance_status_visible() => Vec::new(),
        StatusGame::Dance => mg
            .dance
            .as_ref()
            .map(|g| dance_status_rows(g, mg.dance_last_judge.as_ref()))
            .unwrap_or_default(),
        StatusGame::Fishing => mg
            .fishing
            .as_ref()
            .map(|s| fishing_status_rows(s, &host.fishing_keys))
            .unwrap_or_default(),
    };
    Some(MinigameCaption { game, rows })
}

/// The fishing pond's three rows: the phase line and the reel hint (the
/// session's own derivation, [`PondSession::status_rows`]), then the session
/// affordances - venue hub, quit, point exchange.
pub fn fishing_status_rows(s: &PondSession, keys: &FishingKeys<'_>) -> Vec<StatusRow> {
    let (line, hint) = s.status_rows(keys.cast, keys.reel_a, keys.reel_b);
    vec![
        StatusRow::readout([line]),
        StatusRow::hint([hint]),
        StatusRow::hint([
            format!("{} = menu", keys.menu),
            format!("{} = quit", keys.quit),
            format!("{} = prizes", keys.prizes),
        ]),
    ]
}

/// The slot machine's two rows: reels / balance / feature, then the phase
/// prompt.
pub fn slot_status_rows(m: &SlotMachine) -> Vec<StatusRow> {
    let reels = format!(
        "[{}] [{}] [{}]",
        m.payline_symbol(0),
        m.payline_symbol(1),
        m.payline_symbol(2)
    );
    let feature = match m.feature_mode() {
        6 => format!("BONUS x{}", m.bonus_spins()),
        0 => String::new(),
        mode => format!("feature {mode}"),
    };
    let prompt = match m.phase() {
        SlotPhase::Idle if !m.can_spin() => "not enough coins".to_string(),
        // `spin_cost` is 1 in the feature modes 4..=6 and 3 otherwise.
        SlotPhase::Idle => format!("Cross = spin ({} coins)  Triangle = menu", m.spin_cost()),
        SlotPhase::Spinning => "spinning...".to_string(),
        SlotPhase::Stopping => "Square/Cross/Circle = stop reels 1/2/3".to_string(),
        SlotPhase::Payout => match m.last_result() {
            Some(r) if r.payout > 0 => format!("WIN +{} coins!  (Cross = collect)", r.payout),
            _ => "no win  (Cross = continue)".to_string(),
        },
        SlotPhase::Menu => "Up/Down = row  Cross = take  Circle = back".to_string(),
        SlotPhase::NoCoins => "out of coins - any button leaves".to_string(),
        SlotPhase::Leaving | SlotPhase::CashedOut => "cashed out".to_string(),
    };
    vec![
        StatusRow::readout([
            "SLOTS".to_string(),
            reels,
            format!("coins {}", m.balance()),
            feature,
        ]),
        StatusRow::hint([prompt, "Start = cash out + quit".to_string()]),
    ]
}

/// The Baka Fighter duel's two rows: both fighters' HP / round wins, then the
/// last exchange or the match outcome.
pub fn baka_status_rows(f: &BakaFight) -> Vec<StatusRow> {
    if f.cabinet().front_end() {
        // The cabinet's attract card and player select: no duel yet, so the
        // row names the screen and its keys instead of the fight's numbers.
        let row = match f.select_lineup() {
            Some((cursor, _)) => StatusRow::hint([
                format!(
                    "PLAYER SELECT  {}",
                    ["Vahn", "Noa", "Gala"].get(cursor).copied().unwrap_or("?")
                ),
                "Left/Right pick, Cross confirms".to_string(),
            ]),
            None if f.cabinet().state() == legaia_engine_minigames::baka_cabinet::ST_ATTRACT => {
                StatusRow::hint(["BAKA FIGHTER", "Start or Cross to begin"])
            }
            None => StatusRow::hint(["BAKA FIGHTER"]),
        };
        return vec![row];
    }
    let status = match f.phase() {
        MatchPhase::MatchOver(0) if f.cabinet().choice_sheet().is_some() => {
            "NEXT GAME / PAY OUT: Left/Right, Cross confirms".to_string()
        }
        MatchPhase::MatchOver(0) => format!("YOU WIN the match! +{} coins", f.gold_reward()),
        MatchPhase::MatchOver(_) => "you lose the match - GAME OVER".to_string(),
        MatchPhase::RoundOver(0) => "round won!".to_string(),
        MatchPhase::RoundOver(_) => "round lost".to_string(),
        MatchPhase::Fighting => match f.last_exchange() {
            Some(r) => {
                let who = if r.draw {
                    "trade"
                } else if r.winner == 0 {
                    "you hit"
                } else {
                    "foe hits"
                };
                let crit = if r.critical { " CRIT" } else { "" };
                let sp = if r.special_round_win { " SPECIAL" } else { "" };
                format!("{who} {}{crit}{sp}", r.damage)
            }
            None => "choose your attack".to_string(),
        },
    };
    vec![
        StatusRow::readout([
            "BAKA".to_string(),
            format!("you {}hp (wins {})", f.hp(0), f.round_wins(0)),
            format!("foe {}hp (wins {})", f.hp(1), f.round_wins(1)),
            format!("round {}", f.round() + 1),
        ]),
        StatusRow::hint([
            status,
            "Square/Circle/Cross attack".to_string(),
            "Start = quit".to_string(),
        ]),
    ]
}

/// The Muscle Dome leg's rows: the contest line (course, round, banked
/// coins) when a contest is open, the turn / HP line, then the phase prompt.
///
/// The retail "Turns Left / HP Left" strip is deliberately not here: its draw
/// sites gate on formation slot 0 == `0xB6` (Koru), and the dome ladder tops
/// out at `0xAA`, so no dome round raises it. A dome leg is an unbounded
/// battle, so the turn line reports the turn reached.
///
/// During the selection the battle chrome owns the screen - the command
/// cluster, the direction entry, the status and AP plates, all drawn by the
/// shared battle HUD builders off the session's command flow - so the only
/// text left is the Ra-Seru list, whose retail window (`ctx+6 = 0x46`) is not
/// piece-pinned and stays a text stand-in.
pub fn muscle_status_rows(mg: &MinigameState, muscle: &MuscleWorld) -> Vec<StatusRow> {
    let Some(s) = mg.muscle_dome.as_ref() else {
        return Vec::new();
    };
    if s.phase() == MusclePhase::Select && !s.magic_open() {
        return Vec::new();
    }
    // A decided leg is the battle's KO: retail puts no text up over it, and
    // what follows is the arena hub's INTERVAL tally (a survived, unfinished
    // course) or the hand-back to the field. The confirm that closes the leg
    // is the pad's alone.
    if s.decided() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(c) = mg.muscle_contest.as_ref() {
        out.push(StatusRow::readout([
            format!("Course {}", c.course() + 1),
            format!(
                "Round {}/{}",
                c.round() + 1,
                c.staged_course_length(&muscle.contest_flags)
            ),
            format!("Coins banked: {}", c.tally()),
        ]));
    }
    out.push(StatusRow::readout([
        format!("Turn: {}", s.turn()),
        format!("HP Left: {}", s.hp_left()),
    ]));
    let status = match s.phase() {
        // The Ra-Seru list, opened from the ring's Right chip.
        MusclePhase::Select if s.magic_open() => {
            let rows = s.spell_rows(0);
            let cursor = s.magic_cursor() as usize;
            let line = rows
                .get(cursor)
                .map(|r| {
                    format!(
                        "{} ({} MP){}",
                        r.name,
                        r.mp_cost,
                        if r.affordable {
                            ""
                        } else {
                            "  - not enough MP"
                        }
                    )
                })
                .unwrap_or_else(|| "(no Seru learned)".to_string());
            vec![
                format!("Ra-Seru {}/{}: {}", cursor + 1, rows.len().max(1), line),
                format!("MP {}", s.mp(0)),
                "Up/Down, Cross = cast, Circle = back".to_string(),
            ]
        }
        // The ring, the entry and the confirm draw as battle chrome.
        MusclePhase::Select => Vec::new(),
        MusclePhase::Resolve => vec!["resolving...".to_string()],
        // Retail's turn top is automatic (`World::tick` calls `next_turn`
        // with no press), so this row names no button.
        // The turn's play-out: the acting side and its running damage tally,
        // held for as long as the surface replays the plays.
        MusclePhase::TurnOver => vec![match muscle.playback_tally {
            Some((attacker, total)) => format!(
                "{} attack  TOTAL {total}",
                if attacker == 0 { "Your" } else { "Foe's" }
            ),
            None => {
                let [taken, dealt] = s.last_turn_damage();
                format!("turn: dealt {dealt}, took {taken}")
            }
        }],
        MusclePhase::Won | MusclePhase::Lost => Vec::new(),
    };
    out.push(StatusRow::hint(status.into_iter().chain([
        format!("you {}hp", s.hp(0)),
        format!("foe {}hp", s.hp(1)),
        format!("time {}/{}", s.time_meter(), TIME_METER_MAX),
        "Start = quit".to_string(),
    ])));
    out
}

/// The dance floor's two rows: score / gauge / lane, then the arrow the beat
/// calls for with the last judgement.
///
/// The beat track is not a caption row. It is the game's own beat bar, which
/// the HUD frame draws at retail's anchor - as the retail quads when the
/// hall's HUD page is resident, as `DanceGame::hud_frame_rows`' text cells
/// when it is not - so a second copy spelt in ASCII would say nothing new.
pub fn dance_status_rows(g: &DanceGame, last_judge: Option<&Judge>) -> Vec<StatusRow> {
    let arrow = match g.required_symbol() {
        Some(1) => "< (Square)",
        Some(2) => "> (Circle)",
        Some(3) => "^ (Triangle)",
        _ => "- (rest)",
    };
    let judge = match last_judge {
        Some(Judge::Sequence { .. }) => "SEQUENCE!",
        Some(Judge::Hit { .. }) => "HIT",
        Some(Judge::Miss) => "miss",
        None => "",
    };
    // The score goes through the retail number renderer's decimal split, so
    // leading zeros are blank slots and a score of zero draws nothing - the
    // overlay's `-1` sentinel.
    let score_digits: String = dance_number_digits(g.score())
        .iter()
        .map(|d| match d {
            Some(v) => char::from(b'0' + v),
            None => ' ',
        })
        .collect();
    vec![
        StatusRow::readout([
            "DANCE".to_string(),
            format!("score {}", score_digits.trim_start()),
            format!("gauge {}", g.gauge()),
            format!("lane {}", g.lane()),
        ]),
        StatusRow::hint([
            format!("press {arrow}"),
            judge.to_string(),
            "Start = quit".to_string(),
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: FishingKeys<'static> = FishingKeys {
        cast: "Circle",
        reel_a: "Cross",
        reel_b: "Square",
        menu: "Triangle",
        quit: "Start",
        prizes: "P",
    };

    fn host(slot_machine_drawn: bool) -> StatusHost<'static> {
        StatusHost {
            slot_machine_drawn,
            fishing_keys: KEYS,
        }
    }

    fn slot_machine() -> SlotMachine {
        use legaia_engine_minigames::slot_machine::SYMBOL_COUNT;
        use legaia_minigame_formats::slot_payout::SlotPayoutTable;
        let payouts = SlotPayoutTable {
            payouts: [2u8; SYMBOL_COUNT],
        };
        SlotMachine::new(payouts, 1, 50)
    }

    #[test]
    fn no_dome_session_has_no_rows() {
        assert!(muscle_status_rows(&MinigameState::default(), &MuscleWorld::default()).is_empty());
    }

    #[test]
    fn no_minigame_mode_has_no_caption() {
        let mg = MinigameState::default();
        assert_eq!(
            caption(SceneMode::Field, &mg, &MuscleWorld::default(), &host(false)),
            None
        );
    }

    #[test]
    fn slot_rows_are_a_readout_then_the_keys() {
        let rows = slot_status_rows(&slot_machine());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, RowKind::Readout);
        assert_eq!(rows[0].clauses[0], "SLOTS");
        assert_eq!(rows[0].clauses.last().unwrap(), "coins 50");
        assert_eq!(rows[1].kind, RowKind::Hint);
        assert_eq!(rows[1].clauses.last().unwrap(), "Start = cash out + quit");
    }

    #[test]
    fn a_slot_machine_that_draws_itself_keeps_the_caption_live_and_empty() {
        let mg = MinigameState {
            slot_machine: Some(slot_machine()),
            ..MinigameState::default()
        };
        let muscle = MuscleWorld::default();
        let fallback = caption(SceneMode::SlotMachine, &mg, &muscle, &host(false))
            .expect("slot mode is a minigame");
        assert_eq!(fallback.game, StatusGame::Slot);
        assert_eq!(fallback.rows.len(), 2);
        assert!(fallback.title_line().contains(" | "));
        let drawn = caption(SceneMode::SlotMachine, &mg, &muscle, &host(true)).expect("still live");
        assert!(drawn.rows.is_empty());
        assert_eq!(drawn.title_line(), "");
    }

    #[test]
    fn an_empty_clause_leaves_no_separator() {
        let row = StatusRow::hint(["press - (rest)", "", "Start = quit"]);
        assert_eq!(row.clauses.len(), 2);
        assert_eq!(row.line(), "press - (rest)  Start = quit");
    }
}

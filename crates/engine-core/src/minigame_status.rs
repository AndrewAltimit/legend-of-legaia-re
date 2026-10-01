//! The in-world minigame **status rows**: the engine's own affordance text
//! both play hosts print over the slot machine, the Baka Fighter duel, the
//! Muscle Dome leg and the dance floor, in 320x240 stage space.
//!
//! None of it is retail - each machine's own HUD is drawn by the retail-shaped
//! builders beside it (the dance frame rows, the Baka digit strips and chrome
//! labels, the slot paylines). These rows are the port's readout of the rules
//! state while those pages are partial, and they used to be written out twice,
//! once per host. The copies had drifted in wording (a Muscle Dome turn
//! boundary that told the player to press Cross when retail's turn top is
//! automatic, a slot exit that read "quit" on one host and "leave" on the
//! other) and in space: the native window drew the Muscle Dome rows and the
//! dance beat track in raw surface pixels, top-left at a fraction of the
//! page's size, while the page stage-scaled them. One builder per game here,
//! one draw kernel in `engine-ui` (`ui_text_lines::status_row_draws_for`), and
//! each host applies its one stage transform.

use crate::baka_fighter::{BakaFight, MatchPhase};
use crate::dance::{
    DanceGame, GAUGE_STEP, Judge, dance_beat_track_note_x, dance_combo_window_bright,
    dance_number_digits,
};
use crate::muscle_dome::{DomeRingChip, MusclePhase, TIME_METER_MAX};
use crate::slot_machine::{SlotMachine, SlotPhase};
use crate::world::World;

/// The contest line's pen (Muscle Dome ladder position).
pub const PEN_CONTEST: (i32, i32) = (8, 44);
/// The status line's pen.
pub const PEN_STATUS: (i32, i32) = (8, 62);
/// The prompt line's pen.
pub const PEN_PROMPT: (i32, i32) = (8, 80);
/// The extra row's pen (the dance beat track).
pub const PEN_EXTRA: (i32, i32) = (8, 98);

/// One positioned row of status text in stage space. `bright` picks the
/// highlight ink over the dim one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    pub text: String,
    pub pen: (i32, i32),
    pub bright: bool,
}

impl StatusRow {
    fn new(text: impl Into<String>, pen: (i32, i32), bright: bool) -> Self {
        Self {
            text: text.into(),
            pen,
            bright,
        }
    }
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
        6 => format!("  BONUS x{}", m.bonus_spins()),
        0 => String::new(),
        mode => format!("  feature {mode}"),
    };
    let l1 = format!("SLOTS  {reels}  coins {}{feature}", m.balance());
    let prompt = match m.phase() {
        SlotPhase::Idle if !m.can_spin() => "not enough coins".to_string(),
        // `spin_cost` is 1 in the feature modes 4..=6 and 3 otherwise.
        SlotPhase::Idle => format!("Cross = spin ({} coins)", m.spin_cost()),
        SlotPhase::Spinning => "spinning...".to_string(),
        SlotPhase::Stopping => "Square/Cross/Circle = stop reels 1/2/3".to_string(),
        SlotPhase::Payout => match m.last_result() {
            Some(r) if r.payout > 0 => format!("WIN +{} coins!  (Cross = collect)", r.payout),
            _ => "no win  (Cross = continue)".to_string(),
        },
        SlotPhase::CashedOut => "cashed out".to_string(),
    };
    vec![
        StatusRow::new(l1, PEN_STATUS, true),
        StatusRow::new(
            format!("{prompt}   (Start = cash out + quit)"),
            PEN_PROMPT,
            false,
        ),
    ]
}

/// The Baka Fighter duel's two rows: both fighters' HP / round wins, then the
/// last exchange or the match outcome.
pub fn baka_status_rows(f: &BakaFight) -> Vec<StatusRow> {
    let l1 = format!(
        "BAKA  you {}hp (wins {})  vs  foe {}hp (wins {})  round {}",
        f.hp(0),
        f.round_wins(0),
        f.hp(1),
        f.round_wins(1),
        f.round() + 1
    );
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
        StatusRow::new(l1, PEN_STATUS, true),
        StatusRow::new(
            format!("{status}   Square/Circle/Cross attack, Triangle special (Start = quit)"),
            PEN_PROMPT,
            false,
        ),
    ]
}

/// The Muscle Dome leg's rows: the contest line (course, round, banked
/// coins) when a contest is open, the turn / HP line, then the phase prompt.
///
/// The retail "Turns Left / HP Left" strip is deliberately not here: its draw
/// sites gate on formation slot 0 == `0xB6` (Koru), and the dome ladder tops
/// out at `0xAA`, so no dome round raises it. A dome leg is an unbounded
/// battle, so the turn line reports the turn reached.
pub fn muscle_status_rows(world: &World) -> Vec<StatusRow> {
    let Some(s) = world.minigames.muscle_dome.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(c) = world.minigames.muscle_contest.as_ref() {
        let flags = world.muscle_contest_flags();
        out.push(StatusRow::new(
            format!(
                "Course {}  Round {}/{}   Coins banked: {}",
                c.course() + 1,
                c.round() + 1,
                c.staged_course_length(&flags),
                c.tally(),
            ),
            PEN_CONTEST,
            true,
        ));
    }
    out.push(StatusRow::new(
        format!("      Turn: {}         HP Left: {}", s.turn(), s.hp_left()),
        PEN_STATUS,
        true,
    ));
    let status = match s.phase() {
        // The Ra-Seru list is the ring's Right chip in retail; the hosts have
        // no ring screen, so Triangle opens it.
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
            format!(
                "Ra-Seru {}/{}: {}   MP {}   (Up/Down, Cross = cast, Circle = back)",
                cursor + 1,
                rows.len().max(1),
                line,
                s.mp(0),
            )
        }
        MusclePhase::Select => {
            let h = s.hand(0);
            let chip = if s.chip_enabled(0, DomeRingChip::RaSeru) {
                "  Triangle = Ra-Seru"
            } else {
                ""
            };
            format!(
                "AP L:{} R:{} U:{} D:{}  budget {}  entered {}  (Cross = fight){chip}",
                h[0].cost,
                h[1].cost,
                h[2].cost,
                h[3].cost,
                s.budget(0),
                s.queue(0).len()
            )
        }
        MusclePhase::Resolve => "resolving...".to_string(),
        // Retail's turn top is automatic (`World::tick` calls `next_turn`
        // with no press), so this row names no button.
        MusclePhase::TurnOver => {
            let [taken, dealt] = s.last_turn_damage();
            format!("turn: dealt {dealt}, took {taken}")
        }
        // The caption names a spell; it awards nothing. The contest's payout
        // lands when the ladder settles.
        MusclePhase::Won => format!(
            "LEG WON! caption spell {:#x}  (Cross = next leg)",
            s.reward_spell_id()
        ),
        MusclePhase::Lost => "you lose the leg  (Cross = leave)".to_string(),
    };
    out.push(StatusRow::new(
        format!(
            "{status}   you {}hp  foe {}hp  time {}/{}   (Start = quit)",
            s.hp(0),
            s.hp(1),
            s.time_meter(),
            TIME_METER_MAX,
        ),
        PEN_PROMPT,
        false,
    ));
    out
}

/// The dance floor's rows: score / gauge / lane, the arrow the beat calls for
/// with the last judgement, and the beat track - the COMBO / beat label and
/// the upcoming eight cells of the dancer's own chart row at the ported
/// scroll positions (`FUN_801d2524`).
///
/// The displayed combo slot uses the track renderer's own level-widened beat
/// mask and narrow flash window, which is NOT the judge's combo slot
/// (`DanceGame::on_combo_slot`) - the judged cell is not the displayed cell.
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
    let mut out = vec![
        StatusRow::new(
            format!(
                "DANCE  score {}  gauge {}  lane {}",
                score_digits.trim_start(),
                g.gauge(),
                g.lane()
            ),
            PEN_STATUS,
            true,
        ),
        StatusRow::new(
            format!("press {arrow}   {judge}   (Start = quit)"),
            PEN_PROMPT,
            false,
        ),
    ];
    let beat = g.beat_index();
    let frac = g.intra_beat_phase();
    let level = g.gauge() / GAUGE_STEP;
    let bright = dance_combo_window_bright(beat, level, frac);
    out.push(StatusRow::new(
        if bright { "COMBO" } else { "beat " },
        PEN_EXTRA,
        bright,
    ));
    // The x base is this row's pen, not the overlay's screen constant; the
    // per-note offset is retail's.
    const TRACK_BASE_X: i32 = 60;
    if let Some(row) = g.chart_row(g.lane()) {
        for i in 0..8u32 {
            let cell = row[((beat + i) % row.len() as u32) as usize];
            let glyph = match cell {
                1 => "<",
                2 => ">",
                3 => "^",
                _ => ".",
            };
            out.push(StatusRow::new(
                glyph,
                (dance_beat_track_note_x(TRACK_BASE_X, i, frac), PEN_EXTRA.1),
                i == 0 && !g.in_dead_zone(),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_dome_session_draws_no_rows() {
        assert!(muscle_status_rows(&World::new()).is_empty());
    }

    #[test]
    fn every_row_sits_on_the_stage_pens() {
        let pens = [PEN_CONTEST, PEN_STATUS, PEN_PROMPT, PEN_EXTRA];
        use crate::slot_machine::SYMBOL_COUNT;
        use legaia_asset::slot_payout::SlotPayoutTable;
        let payouts = SlotPayoutTable {
            payouts: [2u8; SYMBOL_COUNT],
        };
        let rows = slot_status_rows(&SlotMachine::new(payouts, 1, 50));
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| pens.contains(&r.pen)));
        assert!(rows[1].text.ends_with("(Start = cash out + quit)"));
    }
}

//! The party night's cards: the round intro, the scoreboard between rounds
//! (with the last-place pick), and the final podium with its confetti.
//!
//! [`party_card`] projects a [`PartyNight`] into a [`PartyCard`] for the
//! player with key `me`; the three draw entry points turn the card into
//! the frame's flat prims and labels, the same composition on both play
//! hosts. The netplay room draws it over the field between the rounds of
//! a networked night; the world's mode screen draws it for a night played
//! alone.
//!
//! Animation runs off the phase timer and a frame counter, never a clock:
//! the intro card slides in, the scoreboard's rows step in one by one, the
//! podium blocks rise and confetti falls on a fixed per-piece path.

use legaia_engine_modes::party::{
    BOARD_FRAMES, INTRO_FRAMES, MATCH_INTRO_FRAMES, PODIUM_FRAMES, PartyNight, PartyPhase,
    PlayerKey, RoundRecord,
};
use legaia_render_kernels::screen_prim::ScreenPrim;

use super::{Canvas, LINE, STAGE_H, STAGE_W, footer, ink, labels_to_draws, pen, seat_color};
use crate::TextDraw;

/// One player's scoreboard row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyRow {
    pub name: String,
    pub score: i64,
    /// Points from the last round (`None`: no round yet, or voided).
    pub delta: Option<i64>,
    pub place: u8,
    pub you: bool,
    pub left: bool,
    /// Seat index, for the marker colour.
    pub seat: usize,
}

/// The last-place player's pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyPick {
    pub picker: String,
    /// The remaining rounds, "Golf - Tower Green".
    pub options: Vec<String>,
    pub cursor: usize,
    /// This player picks.
    pub yours: bool,
    pub seconds: u32,
}

/// What the party overlay shows this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartyCard {
    /// "Round 3/6: Golf - Tower Green, 1 hole".
    Intro {
        night: String,
        round: usize,
        total: usize,
        kind: String,
        title: String,
        detail: String,
        /// "Double points!", "Wager round: 25% of your points".
        tags: Vec<String>,
        /// A Grid Duel match: "Match 2/3: Noa vs Gala".
        matchup: Option<String>,
        seconds: u32,
        /// Frames since the card came up (its slide-in).
        shown: u32,
        rows: Vec<PartyRow>,
    },
    /// The scoreboard between rounds, or a round in play elsewhere (a
    /// player who watches).
    Board {
        night: String,
        heading: String,
        /// "Round 2: Quick Hold'em - Noa wins, pot 12".
        last: Option<String>,
        rows: Vec<PartyRow>,
        /// "Next: Race - Sol Ring, 1 lap".
        next: Option<String>,
        pick: Option<PartyPick>,
        seconds: u32,
        /// Frames since the board came up (its rows step in).
        shown: u32,
    },
    /// The final standings.
    Podium {
        night: String,
        rows: Vec<PartyRow>,
        /// This player's prize, one line each.
        prize: Vec<String>,
        frame: u32,
        /// The podium has stood its time (any button closes it).
        over: bool,
        /// The night ended before its last round (no prizes).
        early: bool,
        /// The podium stands in the world (`modes.toml` `podium`): the card
        /// is a caption over the field, not a screen of its own.
        in_world: bool,
    },
}

/// Who won the round `rec`, as the scoreboard words it.
pub fn round_line(night: &PartyNight, rec: &RoundRecord, number: usize) -> String {
    let title = night.infos.get(rec.slot).map_or("", |i| i.title.as_str());
    if rec.voided {
        return format!("Round {number}: {title} - void, no points");
    }
    let winners: Vec<&str> = rec
        .rows
        .iter()
        .filter(|r| r.place == Some(1))
        .filter_map(|r| night.player(r.key).map(|p| p.name.as_str()))
        .collect();
    let who = match winners.len() {
        0 => "nobody wins".to_string(),
        1 => format!("{} wins", winners[0]),
        _ => format!("{} tie", winners.join(" & ")),
    };
    let mut line = format!("Round {number}: {title} - {who}");
    if rec.pot > 0 {
        line.push_str(&format!(", pot {}", rec.pot));
    }
    if rec.multiplier > 1 {
        line.push_str(" (x2)");
    }
    line
}

fn rows(night: &PartyNight, me: Option<PlayerKey>) -> Vec<PartyRow> {
    let last = night.history.last().filter(|r| !r.voided);
    night
        .standings()
        .into_iter()
        .map(|s| PartyRow {
            delta: last.and_then(|r| {
                r.rows
                    .iter()
                    .find(|row| row.key == s.key)
                    .map(|row| row.points)
            }),
            seat: night
                .players
                .iter()
                .position(|p| p.key == s.key)
                .unwrap_or(0),
            you: Some(s.key) == me,
            name: s.name,
            score: s.score,
            place: s.place,
            left: s.left,
        })
        .collect()
}

fn slot_label(night: &PartyNight, slot: usize) -> String {
    night.infos.get(slot).map_or_else(String::new, |i| {
        format!("{} - {}, {}", i.kind.label(), i.title, i.detail)
    })
}

/// The overlay card for the player with key `me` (`None`: watching), with
/// the prize lines this machine granted, at `frame` (the confetti's clock).
pub fn party_card(
    night: &PartyNight,
    me: Option<PlayerKey>,
    granted: Option<&[String]>,
    frame: u32,
) -> PartyCard {
    let name = night.name.clone();
    let round = night.round_number();
    let total = night.total_rounds();
    match &night.phase {
        PartyPhase::Intro { slot, timer } => {
            let info = &night.infos[*slot];
            let mut tags = Vec::new();
            if night.multiplier(*slot) > 1 {
                tags.push(if night.queue.len() <= 1 && night.double_final {
                    "Final round: double points!".to_string()
                } else {
                    "Double points!".to_string()
                });
            }
            if info.wager > 0 {
                tags.push(format!(
                    "Wager round: {}% of your points on the line",
                    info.wager
                ));
            }
            let span = if night.hill.as_ref().is_some_and(|h| h.played > 0) {
                MATCH_INTRO_FRAMES
            } else {
                INTRO_FRAMES
            };
            let matchup = night.current_match().map(|(a, b)| {
                let n = |k| night.player(k).map_or("?", |p| p.name.as_str());
                let h = night.hill.as_ref().expect("a match has a ladder");
                format!("Match {}/{}: {} vs {}", h.played + 1, h.matches, n(a), n(b))
            });
            PartyCard::Intro {
                night: name,
                round,
                total,
                kind: info.kind.label().to_string(),
                title: info.title.clone(),
                detail: info.detail.clone(),
                tags,
                matchup,
                seconds: timer.div_ceil(60),
                shown: span.saturating_sub(*timer),
                rows: rows(night, me),
            }
        }
        PartyPhase::Playing { slot } => PartyCard::Board {
            night: name,
            heading: format!("Round {round}/{total} in play"),
            last: Some(slot_label(night, *slot)),
            rows: rows(night, me),
            next: None,
            pick: None,
            seconds: 0,
            shown: BOARD_FRAMES,
        },
        PartyPhase::Board { timer } | PartyPhase::Pick { timer, .. } => {
            let played = night.history.len();
            let pick = match night.phase {
                PartyPhase::Pick { picker, cursor, .. } => Some(PartyPick {
                    picker: night
                        .player(picker)
                        .map_or_else(String::new, |p| p.name.clone()),
                    options: night.queue.iter().map(|&s| slot_label(night, s)).collect(),
                    cursor,
                    yours: Some(picker) == me,
                    seconds: timer.div_ceil(60),
                }),
                _ => None,
            };
            let shown = match night.phase {
                PartyPhase::Board { .. } => BOARD_FRAMES.saturating_sub(*timer),
                _ => BOARD_FRAMES,
            };
            PartyCard::Board {
                night: name,
                heading: format!("After round {played} of {total}"),
                last: night.history.last().map(|r| round_line(night, r, played)),
                rows: rows(night, me),
                next: if pick.is_some() {
                    None
                } else {
                    night
                        .next_slot()
                        .map(|s| format!("Next: {}", slot_label(night, s)))
                        .or_else(|| Some("Next: the podium".to_string()))
                },
                pick,
                seconds: timer.div_ceil(60),
                shown,
            }
        }
        PartyPhase::Podium { timer } => PartyCard::Podium {
            night: name,
            rows: rows(night, me),
            prize: granted.map(<[String]>::to_vec).unwrap_or_default(),
            frame: frame.max(PODIUM_FRAMES.saturating_sub(*timer)),
            over: false,
            early: false,
            in_world: false,
        },
        PartyPhase::Over { early } => PartyCard::Podium {
            night: name,
            rows: rows(night, me),
            prize: granted.map(<[String]>::to_vec).unwrap_or_default(),
            frame: frame.max(PODIUM_FRAMES),
            over: true,
            early: *early,
            in_world: false,
        },
    }
}

/// The card's flat prims, in stage pixels.
pub fn party_prims(card: &PartyCard) -> Vec<ScreenPrim> {
    compose(card).prims
}

/// The card's labels, in stage pixels.
pub fn party_texts(font: &legaia_font::Font, card: &PartyCard) -> Vec<TextDraw> {
    labels_to_draws(font, &compose(card).labels)
}

/// The card's labels as strings, in draw order.
pub fn party_strings(card: &PartyCard) -> Vec<String> {
    compose(card).labels.into_iter().map(|l| l.text).collect()
}

/// A deep night-sky backdrop with a band of colour, so a party card reads
/// apart from the room screen.
fn backdrop(c: &mut Canvas, accent: [u8; 4]) {
    c.rect(0, 0, STAGE_W, STAGE_H, [12, 10, 28, 255]);
    c.rect(0, 0, STAGE_W, 18, [30, 18, 56, 255]);
    c.rect(0, 18, STAGE_W, 2, accent);
}

const MAGENTA: [u8; 4] = [228, 76, 160, 255];
const VIOLET: [u8; 4] = [120, 88, 220, 255];

fn ease_in(shown: u32, span: u32) -> i32 {
    // 0 -> 1 over `span` frames, cubic out; in 1/256ths.
    let t = shown.min(span) as i64 * 256 / span.max(1) as i64;
    let inv = 256 - t;
    (256 - inv * inv * inv / (256 * 256)) as i32
}

fn score_text(v: i64) -> String {
    format!("{v}")
}

fn delta_text(d: i64) -> String {
    if d >= 0 {
        format!("+{d}")
    } else {
        format!("{d}")
    }
}

fn place_text(p: u8) -> String {
    match p {
        1 => "1st".into(),
        2 => "2nd".into(),
        3 => "3rd".into(),
        n => format!("{n}th"),
    }
}

/// The scoreboard table: rows step in over the first frames.
fn table(c: &mut Canvas, rows: &[PartyRow], x: i32, y: i32, w: i32, shown: u32) {
    let pitch = 16;
    c.panel(
        x,
        y,
        w,
        rows.len().max(1) as i32 * pitch + 6,
        ink::PANEL,
        ink::PANEL_EDGE,
    );
    for (i, r) in rows.iter().enumerate() {
        // Row i slides in from the left starting at frame 6*i.
        let t = ease_in(shown.saturating_sub(6 * i as u32), 18);
        let dx = (256 - t) * 40 / 256;
        let ry = y + 3 + i as i32 * pitch;
        if r.you {
            c.rect(x + 2, ry, w - 4, pitch, ink::SELECT);
        }
        if r.place == 1 && !r.left {
            c.rect(x + 2, ry, 3, pitch, ink::HIGHLIGHT);
        }
        let ink_name = if r.left {
            pen::DIM
        } else if r.you {
            pen::WHITE
        } else {
            [0.80, 0.82, 0.88, 1.0]
        };
        c.text(place_text(r.place), x + 8 - dx, ry + 1, pen::GOLD);
        c.dot(x + 40 - dx, ry + 8, 3, seat_color(r.seat));
        let mut name = r.name.clone();
        if r.you && r.name != "You" {
            name.push_str(" (you)");
        }
        if r.left {
            name.push_str(" - left");
        }
        c.text(name, x + 48 - dx, ry + 1, ink_name);
        c.text_right(score_text(r.score), x + w - 44, ry + 1, pen::WHITE);
        if let Some(d) = r.delta {
            c.text_right(
                delta_text(d),
                x + w - 6,
                ry + 1,
                if d > 0 {
                    pen::GREEN
                } else if d < 0 {
                    pen::RED
                } else {
                    pen::DIM
                },
            );
        }
    }
}

fn compose(card: &PartyCard) -> Canvas {
    let mut c = Canvas::default();
    match card {
        PartyCard::Intro {
            night,
            round,
            total,
            kind,
            title,
            detail,
            tags,
            matchup,
            seconds,
            shown,
            rows,
        } => {
            backdrop(&mut c, MAGENTA);
            c.text(night.clone(), 6, 2, pen::GOLD);
            c.text_right("Party night", STAGE_W - 6, 2, pen::DIM);
            // The card slides in from the right.
            let t = ease_in(*shown, 24);
            let dx = (256 - t) * STAGE_W / 256;
            let (x, y, w, h) = (30 + dx, 34, 260, 112);
            c.rect(x + 4, y + 4, w, h, [0, 0, 0, 150]);
            c.panel(x, y, w, h, [40, 26, 74, 255], ink::HIGHLIGHT);
            c.rect(x, y, w, 22, MAGENTA);
            c.text_centre(
                format!("ROUND {round}/{total}"),
                x + w / 2,
                y + 3,
                pen::WHITE,
            );
            c.text_centre(kind.clone(), x + w / 2, y + 28, pen::GOLD);
            c.text_centre(title.clone(), x + w / 2, y + 28 + LINE, pen::WHITE);
            c.text_centre(detail.clone(), x + w / 2, y + 28 + 2 * LINE, pen::DIM);
            let mut ty = y + 28 + 3 * LINE + 2;
            if let Some(m) = matchup {
                c.text_centre(m.clone(), x + w / 2, ty, pen::GREEN);
                ty += LINE;
            }
            for tag in tags {
                c.text_centre(tag.clone(), x + w / 2, ty, pen::GOLD);
                ty += LINE;
            }
            // Countdown pips.
            let pips = (*seconds).min(5) as i32;
            for i in 0..pips {
                c.dot(
                    x + w / 2 - (pips - 1) * 6 + i * 12,
                    y + h - 8,
                    2,
                    ink::HIGHLIGHT,
                );
            }
            // The standings strip along the bottom.
            let strip_y = 156;
            c.text("Standings", 8, strip_y, pen::DIM);
            for (i, r) in rows.iter().take(6).enumerate() {
                let col = i as i32 % 2;
                let row = i as i32 / 2;
                let rx = 8 + col * 156;
                let ry = strip_y + LINE + row * LINE;
                c.dot(rx + 3, ry + 7, 2, seat_color(r.seat));
                let name = if r.you && r.name != "You" {
                    format!("{} (you)", r.name)
                } else {
                    r.name.clone()
                };
                c.text(
                    format!("{} {name}", place_text(r.place)),
                    rx + 10,
                    ry,
                    if r.left { pen::DIM } else { pen::WHITE },
                );
                c.text_right(score_text(r.score), rx + 148, ry, pen::GOLD);
            }
            footer(&mut c, &format!("Get ready... {seconds}"));
        }
        PartyCard::Board {
            night,
            heading,
            last,
            rows,
            next,
            pick,
            seconds,
            shown,
        } => {
            backdrop(&mut c, VIOLET);
            c.text(night.clone(), 6, 2, pen::GOLD);
            c.text_right(heading.clone(), STAGE_W - 6, 2, pen::DIM);
            let mut y = 24;
            if let Some(l) = last {
                c.text_centre(l.clone(), STAGE_W / 2, y, pen::WHITE);
                y += LINE + 2;
            }
            let (tx, tw) = (24, 272);
            table(&mut c, rows, tx, y, tw, *shown);
            y += rows.len().max(1) as i32 * 16 + 12;
            if let Some(p) = pick {
                let h = 20 + p.options.len() as i32 * LINE;
                c.panel(40, y, 240, h, [40, 26, 74, 255], ink::HIGHLIGHT);
                let who = if p.yours {
                    "You're last: pick the next round!".to_string()
                } else {
                    format!("{} is last and picks the next round", p.picker)
                };
                c.text_centre(who, STAGE_W / 2, y + 2, pen::GOLD);
                for (i, o) in p.options.iter().enumerate() {
                    let oy = y + 18 + i as i32 * LINE;
                    let on = i == p.cursor;
                    if on {
                        c.rect(44, oy, 232, LINE, ink::SELECT);
                    }
                    c.text(o.clone(), 50, oy, if on { pen::WHITE } else { pen::DIM });
                }
                footer(
                    &mut c,
                    &if p.yours {
                        format!("Up/Down: round   Cross: pick   {}s", p.seconds)
                    } else {
                        format!("Waiting for {}... {}s", p.picker, p.seconds)
                    },
                );
            } else {
                if let Some(n) = next {
                    c.text_centre(n.clone(), STAGE_W / 2, y, pen::GOLD);
                }
                footer(
                    &mut c,
                    &if *seconds > 0 {
                        format!("Next round in {seconds}   Tab: room")
                    } else {
                        "Tab: room".to_string()
                    },
                );
            }
        }
        PartyCard::Podium {
            night,
            rows,
            prize,
            over,
            in_world: true,
            ..
        } => caption(&mut c, night, rows, prize, *over),
        PartyCard::Podium {
            night,
            rows,
            prize,
            frame,
            over,
            early,
            ..
        } => {
            backdrop(&mut c, ink::HIGHLIGHT);
            c.text(night.clone(), 6, 2, pen::GOLD);
            c.text_right(
                if *early {
                    "Ended early"
                } else {
                    "Final standings"
                },
                STAGE_W - 6,
                2,
                pen::DIM,
            );
            podium(&mut c, rows, *frame, !*early);
            // Everyone past third, and the prize.
            let mut y = 168;
            for r in rows.iter().skip(3).take(2) {
                c.text(
                    format!("{} {}", place_text(r.place), r.name),
                    70,
                    y,
                    pen::DIM,
                );
                c.text_right(score_text(r.score), 250, y, pen::DIM);
                y += LINE - 2;
            }
            if *early {
                c.text_centre(
                    "The night ended early: no prizes",
                    STAGE_W / 2,
                    196,
                    pen::RED,
                );
            } else if !prize.is_empty() {
                c.text_centre(
                    format!("You won: {}", prize.join(", ")),
                    STAGE_W / 2,
                    196,
                    pen::GREEN,
                );
            }
            footer(
                &mut c,
                if *over {
                    "Cross: close"
                } else {
                    "What a night!"
                },
            );
        }
    }
    c
}

/// The podium standing in the world: a title strip over the field, and a
/// caption under it naming the top three left to right as the blocks stand
/// (2nd, 1st, 3rd), with the prize.
fn caption(c: &mut Canvas, night: &str, rows: &[PartyRow], prize: &[String], over: bool) {
    c.panel(0, 0, STAGE_W, 18, [20, 14, 40, 220], [228, 196, 90, 255]);
    c.text(night.to_string(), 6, 2, pen::GOLD);
    c.text_right("Final standings", STAGE_W - 6, 2, pen::DIM);
    // Under the title strip, clear of the toasts and the footer below.
    let top = 22;
    let prize_rows: Vec<String> = prize
        .chunks(2)
        .enumerate()
        .map(|(i, c)| {
            let items = c.join(", ");
            if i == 0 {
                format!("You won: {items}")
            } else {
                items
            }
        })
        .collect();
    let h = 4 + 2 * (LINE - 2) + prize_rows.len() as i32 * (LINE - 2) + 2;
    c.panel(
        30,
        top,
        STAGE_W - 60,
        h,
        [14, 10, 30, 210],
        [228, 196, 90, 255],
    );
    for (i, x) in [(1usize, 80), (0, STAGE_W / 2), (2, STAGE_W - 80)] {
        let Some(r) = rows.get(i) else { continue };
        let mut name = r.name.clone();
        if r.you && r.name != "You" {
            name.push_str(" (you)");
        }
        c.text_centre(
            format!("{} {}", place_text(r.place), name),
            x,
            top + 4,
            if r.you { pen::GOLD } else { pen::WHITE },
        );
        c.text_centre(score_text(r.score), x, top + 4 + LINE - 2, pen::DIM);
    }
    for (i, row) in prize_rows.into_iter().enumerate() {
        c.text_centre(
            row,
            STAGE_W / 2,
            top + 4 + (2 + i as i32) * (LINE - 2),
            pen::GREEN,
        );
    }
    footer(
        c,
        if over {
            "Cross: close"
        } else {
            "What a night!"
        },
    );
}

/// Three blocks, 2nd / 1st / 3rd, rising in; a figure in each player's
/// colour on top; confetti and sparkles over a completed night.
fn podium(c: &mut Canvas, rows: &[PartyRow], frame: u32, party: bool) {
    let base = 160;
    let t = ease_in(frame, 40);
    // (row index into the standings, x centre, block height)
    let blocks = [(1usize, 90, 40), (0, 160, 62), (2, 230, 26)];
    for (i, cx, h) in blocks {
        let Some(r) = rows.get(i) else { continue };
        let h = h * t / 256;
        let w = 62;
        let x = cx - w / 2;
        let top = base - h;
        let fill = match i {
            0 => [196, 152, 48, 255],
            1 => [150, 156, 170, 255],
            _ => [164, 104, 60, 255],
        };
        c.panel(x, top, w, h.max(1), fill, [250, 236, 190, 255]);
        if h > 18 {
            c.text_centre(place_text(r.place), cx, top + 3, pen::INK);
            c.text_centre(score_text(r.score), cx, top + 3 + LINE - 2, pen::INK);
        }
        // A figure standing on the block: head and body in the seat colour,
        // with a little bounce for the winner.
        let bounce = if i == 0 && party {
            ((frame / 8) % 2) as i32 * 2
        } else {
            0
        };
        let col = seat_color(r.seat);
        let fy = top - 2 - bounce;
        c.rect(cx - 5, fy - 16, 10, 14, col);
        c.dot(cx, fy - 21, 4, col);
        c.rect(cx - 8, fy - 13, 3, 8, col);
        c.rect(cx + 5, fy - 13, 3, 8, col);
        let mut name = r.name.clone();
        if r.you && r.name != "You" {
            name.push_str(" (you)");
        }
        c.text_centre(
            name,
            cx,
            fy - 40,
            if r.you { pen::GOLD } else { pen::WHITE },
        );
    }
    c.rect(40, base, 240, 2, [250, 236, 190, 255]);
    if !party {
        return;
    }
    // Confetti: each piece falls on its own fixed path, looping.
    const COLORS: [[u8; 4]; 5] = [
        [240, 86, 120, 255],
        [250, 210, 80, 255],
        [100, 200, 250, 255],
        [140, 240, 140, 255],
        [210, 130, 250, 255],
    ];
    for i in 0..48u32 {
        let seed = i.wrapping_mul(2_654_435_761);
        let speed = 1 + (seed >> 28) as i32 % 3;
        let x0 = (seed % STAGE_W as u32) as i32;
        let period = (STAGE_H + 20) as u32;
        let y = ((frame as i32 * speed + (seed >> 8) as i32 % period as i32) % period as i32) - 20;
        let sway = ((((frame + i * 7) / 6) % 8) as i32 - 4).abs() - 2;
        let (w, h) = if seed.is_multiple_of(2) {
            (3, 2)
        } else {
            (2, 3)
        };
        c.rect(x0 + sway, y, w, h, COLORS[i as usize % COLORS.len()]);
    }
    // Sparkles around the winner.
    for i in 0..6u32 {
        if (frame / 5 + i).is_multiple_of(3) {
            let a = (i * 60 + frame * 3) % 360;
            let (dx, dy) = match a / 60 {
                0 => (22, -6),
                1 => (14, -30),
                2 => (-14, -30),
                3 => (-22, -6),
                4 => (-10, 10),
                _ => (10, 10),
            };
            c.dot(160 + dx, base - 62 - 30 + dy, 1, [255, 250, 210, 255]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_modes::active::builtin_sections;
    use legaia_engine_modes::party::PartyCue;

    fn night() -> PartyNight {
        let def = builtin_sections().0.party_night("builtin-party").unwrap();
        let players = vec![
            (1, "Vahn".to_string(), true),
            (2, "Noa".to_string(), true),
            (3, "Gala".to_string(), true),
        ];
        PartyNight::new(def, &builtin_sections().0, &players, 3).unwrap()
    }

    fn has(strings: &[String], needle: &str) -> bool {
        strings.iter().any(|s| s.contains(needle))
    }

    #[test]
    fn the_intro_names_the_round_and_the_board_scores_it() {
        let mut n = night();
        let card = party_card(&n, Some(1), None, 0);
        let s = party_strings(&card);
        for needle in [
            "ROUND 1/4",
            "Golf",
            "Practice Green",
            "1 hole",
            "Vahn (you)",
        ] {
            assert!(has(&s, needle), "{needle}: {s:?}");
        }
        assert!(party_prims(&card).len() > 10);
        while !matches!(n.tick(), PartyCue::Start { .. }) {}
        n.record(&[(2, 1), (1, 2), (3, 3)]);
        let s = party_strings(&party_card(&n, Some(1), None, 0));
        for needle in ["After round 1 of 4", "Noa wins", "+10", "Next: Poker"] {
            assert!(has(&s, needle), "{needle}: {s:?}");
        }
        // The poker round is a wager.
        while !matches!(n.phase, PartyPhase::Intro { .. }) {
            n.tick();
        }
        let s = party_strings(&party_card(&n, Some(1), None, 0));
        assert!(has(&s, "Wager round: 25%"), "{s:?}");
    }

    #[test]
    fn the_podium_shows_the_top_three_and_the_prize() {
        let mut n = night();
        for _ in 0..4 {
            while !matches!(n.tick(), PartyCue::Start { .. }) {}
            if n.hill.is_some() {
                while n.current_match().is_some() {
                    let (k, _) = n.current_match().unwrap();
                    n.record_duel(Some(k));
                    if n.current_match().is_some() {
                        while !matches!(n.tick(), PartyCue::Start { .. }) {}
                    }
                }
            } else {
                n.record(&[(3, 1), (2, 2), (1, 3)]);
            }
        }
        while n.tick() != PartyCue::Podium {}
        let lines = vec!["+300 gold".to_string()];
        let card = party_card(&n, Some(3), Some(&lines), 100);
        let s = party_strings(&card);
        for needle in ["Final standings", "1st", "Gala (you)", "You won: +300 gold"] {
            assert!(has(&s, needle), "{needle}: {s:?}");
        }
        // Confetti falls.
        let a = party_prims(&card);
        let b = party_prims(&party_card(&n, Some(3), Some(&lines), 130));
        assert!(a.len() > 48);
        assert_ne!(a, b);
    }
}

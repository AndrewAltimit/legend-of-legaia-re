//! The poker table: opponents along the far rail, the board and pot in the
//! middle, the player's hand and action menu along the near edge.

use legaia_engine_modes::active::{ActiveMode, category_label};
use legaia_engine_modes::poker::{Phase, PokerView, SeatView, Street};

use super::cards::{CARD_H, CARD_W, card_back, card_slot, pip, playing_card, rank_label};
use super::{Align, Canvas, LINE, STAGE_H, STAGE_W, footer, ink, log_box, pen};

const TABLE: (i32, i32, i32, i32) = (8, 20, 304, 150);

/// The local player's seat ([`ActiveMode::local_seat`]) is the near
/// edge's hand and menu; a watcher sees seat names, no "You".
pub(super) fn draw(c: &mut Canvas, v: &PokerView, mode: &ActiveMode, log: &[String]) {
    let mine = mode.local_seat.min(v.seats.len().saturating_sub(1));
    let (tx, ty, tw, th) = TABLE;
    c.rect(tx, ty, tw, th, ink::FELT_EDGE);
    c.rect(tx + 4, ty + 4, tw - 8, th - 8, ink::FELT);
    let showdown = matches!(v.phase, Phase::Showdown { .. } | Phase::Finished);
    let winners: Vec<usize> = v
        .last_awards
        .iter()
        .flat_map(|(w, _, _)| w.iter().copied())
        .collect();

    // Opponents along the far rail.
    let others: Vec<usize> = (0..v.seats.len()).filter(|&s| s != mine).collect();
    let slot = (tw - 8) / others.len().max(1) as i32;
    let pw = (slot - 6).min(64);
    for (i, &seat) in others.iter().enumerate() {
        let cx = tx + 4 + slot * i as i32 + slot / 2;
        let s = &v.seats[seat];
        let x = cx - pw / 2;
        let lit = v.to_act == Some(seat) && !showdown;
        let won = showdown && winners.contains(&seat);
        let edge = if lit || won {
            ink::HIGHLIGHT
        } else {
            ink::PANEL_EDGE
        };
        c.panel(x, 26, pw, 31, ink::PANEL, edge);
        let name_ink = if s.folded || !s.in_hand {
            pen::DIM
        } else {
            pen::WHITE
        };
        c.text_in(
            mode.seat_name(seat),
            cx,
            27,
            pw - 6,
            name_ink,
            Align::Centre,
        );
        c.text_in(seat_status(s), cx, 41, pw - 6, status_ink(s), Align::Centre);
        if v.button == seat {
            button_marker(c, x + pw - 4, 29);
        }
        if s.in_hand && !s.folded {
            match (showdown, s.hole) {
                (true, Some(h)) => {
                    mini_card(c, h[0], cx - 15, 60);
                    mini_card(c, h[1], cx + 1, 60);
                }
                _ => {
                    card_back(c, cx - 13, 60, 12, 17);
                    card_back(c, cx + 1, 60, 12, 17);
                }
            }
        }
        if s.street_bet > 0 && !showdown {
            c.text_in(
                format!("bet {}", s.street_bet),
                cx,
                82,
                slot - 4,
                pen::GOLD,
                Align::Centre,
            );
        }
    }

    // The board.
    let bx = 160 - (5 * (CARD_W + 4) - 4) / 2;
    for i in 0..5 {
        let x = bx + i * (CARD_W + 4);
        match v.board.get(i as usize) {
            Some(&card) => playing_card(c, card, x, 100),
            None => card_slot(c, x, 100),
        }
    }
    // The pot line, or the showdown's awards over it.
    if showdown && !v.last_awards.is_empty() {
        let (w, amount, hand) = &v.last_awards[0];
        let names: Vec<String> = w.iter().map(|&s| mode.seat_name(s)).collect();
        let verb = if w.len() == 1 && mode.is_you(w[0]) {
            "win"
        } else {
            "wins"
        };
        let text = match hand {
            Some(h) => format!(
                "{} {verb} {amount} - {}",
                names.join(" & "),
                category_label(h.category())
            ),
            None => format!("{} {verb} {amount}", names.join(" & ")),
        };
        c.text_in(text, 160, 138, tw - 16, pen::GOLD, Align::Centre);
    } else {
        let street = match v.phase {
            Phase::Betting(Street::Preflop) => "Preflop",
            Phase::Betting(Street::Flop) => "Flop",
            Phase::Betting(Street::Turn) => "Turn",
            Phase::Betting(Street::River) => "River",
            _ => "",
        };
        c.text_in(
            format!("Pot {}", v.pot),
            160,
            138,
            100,
            pen::WHITE,
            Align::Centre,
        );
        c.text_in(street, tx + 10, 138, 60, pen::DIM, Align::Left);
        c.text_in(
            format!("Blinds {}/{}", v.small_blind, v.big_blind),
            tx + tw - 10,
            138,
            90,
            pen::DIM,
            Align::Right,
        );
    }
    let me = &v.seats[mine];
    if me.street_bet > 0 && !showdown {
        c.text_in(
            format!("your bet {}", me.street_bet),
            160,
            152,
            tw - 16,
            pen::GOLD,
            Align::Centre,
        );
    }

    // The near edge: log, hand, action menu.
    log_box(c, 4, 172, 112, 3, log);
    let hx = 122;
    match me.hole {
        Some(h) if me.in_hand => {
            let lift = if me.folded { 6 } else { 0 };
            playing_card(c, h[0], hx, 174 + lift);
            playing_card(c, h[1], hx + CARD_W + 3, 174 + lift);
            if me.folded {
                c.rect(hx, 180, 2 * CARD_W + 3, CARD_H, ink::SHADE);
            }
        }
        _ => {
            card_slot(c, hx, 174);
            card_slot(c, hx + CARD_W + 3, 174);
        }
    }
    if v.button == mine {
        button_marker(c, hx - 4, 176);
    }
    c.text_in(
        format!("{} {}", mode.seat_name(mine), me.chips),
        hx + CARD_W + 1,
        210,
        // Between the log box and the action menu.
        62,
        if showdown && winners.contains(&mine) {
            pen::GOLD
        } else {
            pen::WHITE
        },
        Align::Centre,
    );
    action_menu(c, v, mine, 186, 172);
    footer(
        c,
        if showdown {
            "Cross: next hand   Start: leave"
        } else {
            "Left/Right: action  Up/Down: raise  Cross: go"
        },
    );
}

fn seat_status(s: &SeatView) -> String {
    if !s.in_hand && s.chips == 0 {
        "Out".into()
    } else if s.folded {
        "Fold".into()
    } else if s.all_in {
        "All in".into()
    } else {
        s.chips.to_string()
    }
}

fn status_ink(s: &SeatView) -> [f32; 4] {
    if s.all_in {
        pen::GOLD
    } else if s.folded || !s.in_hand {
        pen::DIM
    } else {
        pen::WHITE
    }
}

/// The dealer button.
fn button_marker(c: &mut Canvas, x: i32, y: i32) {
    c.dot(x, y, 3, [245, 245, 245, 255]);
    c.dot(x, y, 1, [40, 40, 40, 255]);
}

/// A small face-up card for a revealed opponent hand.
fn mini_card(c: &mut Canvas, card: legaia_engine_modes::poker::hand::Card, x: i32, y: i32) {
    c.panel(x, y, 14, 24, [238, 236, 226, 255], [60, 60, 70, 255]);
    let red = matches!(card.suit(), 1 | 2);
    c.text_centre(
        rank_label(card.rank()),
        x + 7,
        y - 1,
        if red { pen::RED } else { pen::INK },
    );
    pip(c, card.suit(), x + 4, y + 15, 1);
}

fn action_menu(c: &mut Canvas, v: &PokerView, seat: usize, x: i32, y: i32) {
    let (w, h) = (130, 52);
    let mine = v.to_act == Some(seat) && matches!(v.phase, Phase::Betting(_));
    c.panel(
        x,
        y,
        w,
        h,
        ink::PANEL,
        if mine {
            ink::HIGHLIGHT
        } else {
            ink::PANEL_EDGE
        },
    );
    if !mine {
        let msg = match v.phase {
            Phase::Showdown { .. } => "Showdown",
            Phase::Finished => "Table closed",
            _ if !v.seats[seat].in_hand || v.seats[seat].folded => "Sitting out",
            _ => "Waiting...",
        };
        c.text_in(msg, x + w / 2, y + 18, w - 8, pen::DIM, Align::Centre);
        return;
    }
    let me = &v.seats[seat];
    let menu = v.menus.get(seat).copied().unwrap_or_default();
    let to_call = v.current_bet.saturating_sub(me.street_bet).min(me.chips);
    let labels = [
        "Fold".to_string(),
        if to_call == 0 {
            "Check".to_string()
        } else {
            format!("Call {to_call}")
        },
        "Raise".to_string(),
        "All in".to_string(),
    ];
    for (i, l) in labels.iter().enumerate() {
        let (col, row) = (i as i32 % 2, i as i32 / 2);
        let (cx, cy) = (x + 2 + col * 63, y + 3 + row * 16);
        let on = menu.cursor as usize == i;
        if on {
            c.rect(cx, cy, 63, 16, ink::SELECT);
        }
        c.text_in(
            l.clone(),
            cx + 4,
            cy + 1,
            63 - 6,
            if on { pen::GOLD } else { pen::WHITE },
            Align::Left,
        );
    }
    c.text_in(
        format!("to {}  (min {})", menu.raise_to, v.min_raise_to),
        x + w / 2,
        y + 35,
        w - 8,
        if menu.cursor == 2 {
            pen::GOLD
        } else {
            pen::DIM
        },
        Align::Centre,
    );
}

/// The table HUD over an in-world poker game: a plate per other seat along
/// the top (name, chips or state, the street bet on its own row, the seat to
/// act lit), the pot under them, the player's own plate and the action menu
/// along the bottom. The cards and chips themselves are the table's.
///
/// Every label sits in a box of its own ([`Canvas::text_in`]): a long table
/// name is cut to its plate, and a seat's stack and bet never share a row,
/// so "4900" and "+100" cannot run together into "4900+100".
///
/// Returns the bottom of the HUD's top band (the plates and the pot),
/// where a banner may start.
pub(super) fn hud(
    c: &mut Canvas,
    mode: &legaia_engine_modes::active::ActiveMode,
    v: &PokerView,
) -> i32 {
    let me = mode.local_seat;
    let name = |seat: usize| mode.seat_name(seat);
    let others: Vec<usize> = (0..v.seats.len()).filter(|&s| s != me).collect();
    let n = others.len().max(1) as i32;
    let gap = 4;
    let pw = ((STAGE_W - 8 - gap * (n - 1)) / n).min(104);
    let x0 = (STAGE_W - (pw * n + gap * (n - 1))) / 2;
    let plate = [10, 14, 24, 190];
    for (i, &seat) in others.iter().enumerate() {
        let s = &v.seats[seat];
        let x = x0 + i as i32 * (pw + gap);
        let acting = v.to_act == Some(seat) && matches!(v.phase, Phase::Betting(_));
        let edge = if acting {
            ink::HIGHLIGHT
        } else {
            ink::PANEL_EDGE
        };
        c.panel(x, SEAT_PLATE_Y, pw, SEAT_PLATE_H, plate, edge);
        // The dealer button takes the name row's right corner.
        let button = v.button == seat;
        if button {
            button_marker(c, x + pw - 6, SEAT_PLATE_Y + 5);
        }
        let inner = pw - 8;
        c.text_in(
            name(seat),
            x + 4,
            SEAT_PLATE_Y,
            inner - if button { 8 } else { 0 },
            if acting { pen::GOLD } else { pen::WHITE },
            Align::Left,
        );
        c.text_in(
            seat_status(s),
            x + 4,
            SEAT_PLATE_Y + LINE,
            inner,
            status_ink(s),
            Align::Left,
        );
        if s.street_bet > 0 {
            c.text_in(
                format!("Bet {}", s.street_bet),
                x + 4,
                SEAT_PLATE_Y + 2 * LINE,
                inner,
                pen::GOLD,
                Align::Left,
            );
        }
    }
    if v.pot > 0 {
        let (w, y) = (88, SEAT_PLATE_Y + SEAT_PLATE_H + 3);
        c.panel((STAGE_W - w) / 2, y, w, 16, plate, ink::PANEL_EDGE);
        c.text_in(
            format!("Pot {}", v.pot),
            STAGE_W / 2,
            y,
            w - 8,
            pen::GOLD,
            Align::Centre,
        );
    }
    // The player's plate, bottom-left: "You" and the best hand made, the
    // stack, then the bet (or the fold) - one row each.
    if let Some(s) = v.seats.get(me) {
        let (x, y, w, h) = (4, STAGE_H - 56, 120, 52);
        c.panel(x, y, w, h, plate, ink::PANEL_EDGE);
        if v.button == me {
            button_marker(c, x + w - 6, y + h - 7);
        }
        let you_w = 28;
        c.text_in("You", x + 4, y + 1, you_w, pen::GOLD, Align::Left);
        c.text_in(
            format!("{} chips", s.chips),
            x + 4,
            y + 1 + LINE,
            w - 8,
            status_ink(s),
            Align::Left,
        );
        let last_w = w - 8 - 10;
        if s.street_bet > 0 {
            c.text_in(
                format!("Bet {}", s.street_bet),
                x + 4,
                y + 1 + 2 * LINE,
                last_w,
                pen::WHITE,
                Align::Left,
            );
        } else if s.folded {
            c.text_in(
                "Folded",
                x + 4,
                y + 1 + 2 * LINE,
                last_w,
                pen::DIM,
                Align::Left,
            );
        }
        if let (Some(hole), false) = (s.hole, v.board.is_empty()) {
            let mut seven: Vec<_> = hole.to_vec();
            seven.extend_from_slice(&v.board);
            if seven.len() >= 5 && !s.folded {
                let best = legaia_engine_modes::poker::hand::best_hand(&seven);
                c.text_in(
                    category_label(best.category()),
                    x + w - 4,
                    y + 1,
                    w - 8 - you_w - 4,
                    pen::GREEN,
                    Align::Right,
                );
            }
        }
    }
    action_menu(c, v, me, STAGE_W - 4 - 130, STAGE_H - 56);
    SEAT_PLATE_Y + SEAT_PLATE_H + 3 + 16
}

/// The top of the other seats' plates, and their height: three rows (name,
/// stack, bet).
const SEAT_PLATE_Y: i32 = 4;
const SEAT_PLATE_H: i32 = 3 * LINE + 4;

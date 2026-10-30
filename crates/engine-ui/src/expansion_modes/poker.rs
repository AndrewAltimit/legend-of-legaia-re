//! The poker table: opponents along the far rail, the board and pot in the
//! middle, the player's hand and action menu along the near edge.

use legaia_engine_modes::active::{ActiveMode, category_label};
use legaia_engine_modes::poker::{Phase, PokerView, SeatView, Street};

use super::cards::{CARD_H, CARD_W, card_back, card_slot, pip, playing_card, rank_label};
use super::{Canvas, footer, ink, log_box, pen};

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
        c.text_centre(mode.seat_name(seat), cx, 27, name_ink);
        c.text_centre(seat_status(s), cx, 41, status_ink(s));
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
            c.text_centre(format!("bet {}", s.street_bet), cx, 82, pen::GOLD);
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
        c.text_centre(text, 160, 138, pen::GOLD);
    } else {
        let street = match v.phase {
            Phase::Betting(Street::Preflop) => "Preflop",
            Phase::Betting(Street::Flop) => "Flop",
            Phase::Betting(Street::Turn) => "Turn",
            Phase::Betting(Street::River) => "River",
            _ => "",
        };
        c.text_centre(format!("Pot {}", v.pot), 160, 138, pen::WHITE);
        c.text(street, tx + 10, 138, pen::DIM);
        c.text_right(
            format!("Blinds {}/{}", v.small_blind, v.big_blind),
            tx + tw - 10,
            138,
            pen::DIM,
        );
    }
    let me = &v.seats[mine];
    if me.street_bet > 0 && !showdown {
        c.text_centre(format!("your bet {}", me.street_bet), 160, 152, pen::GOLD);
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
    c.text_centre(
        format!("{} {}", mode.seat_name(mine), me.chips),
        hx + CARD_W + 1,
        210,
        if showdown && winners.contains(&mine) {
            pen::GOLD
        } else {
            pen::WHITE
        },
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
        c.text_centre(msg, x + w / 2, y + 18, pen::DIM);
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
        c.text(
            l.clone(),
            cx + 4,
            cy + 1,
            if on { pen::GOLD } else { pen::WHITE },
        );
    }
    c.text_centre(
        format!("to {}  (min {})", menu.raise_to, v.min_raise_to),
        x + w / 2,
        y + 35,
        if menu.cursor == 2 {
            pen::GOLD
        } else {
            pen::DIM
        },
    );
}

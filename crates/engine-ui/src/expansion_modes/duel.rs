//! The Grid Duel board: the 3x3 grid in the middle, the player's hand down
//! the left, the opponent's down the right, the score below.

use legaia_engine_modes::active::ActiveMode;
use legaia_engine_modes::cards::duel::{CELLS, DuelPhase, DuelView, HAND};

use super::cards::{DUEL_H, DUEL_W, ROW_H, ROW_W, duel_card, element_color, hand_row};
use super::{Canvas, STAGE_W, footer, ink, pen};

const CELL: i32 = DUEL_W + 2;
const GRID_X: i32 = (STAGE_W - 3 * CELL) / 2;
const GRID_Y: i32 = 20;

pub(super) fn draw(c: &mut Canvas, mode: &ActiveMode, v: &DuelView, log: &[String]) {
    // The grid.
    c.panel(
        GRID_X - 3,
        GRID_Y - 3,
        3 * CELL + 4,
        3 * CELL + 4,
        ink::PANEL,
        ink::PANEL_EDGE,
    );
    let picking_cell = match v.phase {
        DuelPhase::PickCell { seat: 0, .. } => Some(v.cell_cursor[0]),
        _ => None,
    };
    for cell in 0..CELLS {
        let (x, y) = cell_xy(cell);
        match v.grid[cell] {
            Some(p) => {
                let card = &v.hands[p.deck][p.card];
                duel_card(c, card, mode.card_name(&card.id), p.owner, x, y);
            }
            None => {
                c.rect(x, y, DUEL_W, DUEL_H, [18, 24, 36, 255]);
                if let Some(e) = v.cell_elements[cell] {
                    let col = element_color(Some(e));
                    c.outline(x + 6, y + 6, DUEL_W - 12, DUEL_H - 12, col);
                    c.dot(x + DUEL_W / 2, y + DUEL_H / 2, 3, col);
                }
            }
        }
        if picking_cell == Some(cell) {
            c.outline(x - 1, y - 1, DUEL_W + 2, DUEL_H + 2, ink::HIGHLIGHT);
            c.outline(x, y, DUEL_W, DUEL_H, ink::HIGHLIGHT);
        }
    }

    // The hands: the player's down the left, the opponent's down the right,
    // one row per card still in hand.
    for seat in 0..2 {
        let x = if seat == 0 { 2 } else { STAGE_W - 2 - ROW_W };
        let (turn, held) = match v.phase {
            DuelPhase::PickCard { seat: s } => (s == seat, None),
            DuelPhase::PickCell { seat: s, card } => (s == seat, Some(card)),
            _ => (false, None),
        };
        let selected = held.or(turn.then_some(v.hand_cursor[seat]));
        let mut row = 0;
        for i in 0..HAND {
            if v.used[seat][i] {
                continue;
            }
            let y = GRID_Y + row * (ROW_H + 3);
            row += 1;
            let card = &v.hands[seat][i];
            hand_row(
                c,
                card,
                mode.card_name(&card.id),
                seat,
                x,
                y,
                Some(i) == selected,
            );
        }
    }

    // The score and the turn line.
    let sy = GRID_Y + 3 * CELL + 4;
    c.text_centre(
        format!("You {}  -  {} Rival", v.scores[0], v.scores[1]),
        STAGE_W / 2,
        sy,
        pen::WHITE,
    );
    let turn = match v.phase {
        DuelPhase::PickCard { seat: 0 } => "Your turn: pick a card",
        DuelPhase::PickCell { seat: 0, .. } => "Your turn: pick a cell",
        DuelPhase::PickCard { .. } | DuelPhase::PickCell { .. } => "Rival is thinking...",
        DuelPhase::Spoils { winner: 0 } => "Take one of the rival's cards",
        DuelPhase::Spoils { .. } => "The rival takes a card",
        DuelPhase::Finished => "Duel over",
    };
    c.text_centre(turn, STAGE_W / 2, sy + 14, pen::GOLD);
    if let DuelPhase::Spoils { winner: 0 } = v.phase {
        spoils(c, mode, v);
    }
    // The newest event under the turn line.
    if let Some(last) = log.last() {
        c.text_centre(last.clone(), STAGE_W / 2, sy + 28, pen::DIM);
    }
    footer(
        c,
        match v.phase {
            DuelPhase::PickCell { .. } => "D-pad: cell   Cross: place   Circle: back",
            _ => "Left/Right: card   Cross: choose   Start: leave",
        },
    );
}

fn cell_xy(cell: usize) -> (i32, i32) {
    (
        GRID_X + (cell % 3) as i32 * CELL,
        GRID_Y + (cell / 3) as i32 * CELL,
    )
}

/// The stakes pick: the rival's deck as a list.
fn spoils(c: &mut Canvas, mode: &ActiveMode, v: &DuelView) {
    let (x, y, w) = (90, 60, 140);
    c.panel(x, y, w, HAND as i32 * 16 + 8, ink::PANEL, ink::HIGHLIGHT);
    for (i, card) in v.hands[1].iter().enumerate() {
        let ry = y + 4 + i as i32 * 16;
        let on = v.spoil_cursor == i;
        if on {
            c.rect(x + 2, ry, w - 4, 16, ink::SELECT);
        }
        c.text(
            mode.card_name(&card.id).to_string(),
            x + 8,
            ry + 1,
            if on { pen::GOLD } else { pen::WHITE },
        );
    }
}

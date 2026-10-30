//! The Grid Duel's screens.
//!
//! - **2D board** ([`draw`]), for a duel with no table in the world: the
//!   3x3 grid in the middle, the player's hand fanned down the left, the
//!   rival's down the right, the selected card's full name and strengths
//!   under the board.
//! - **Table HUD** ([`hud`]), over a duel played at a table in the world:
//!   only the score, the turn prompt and the selected card's details, so
//!   the table stays the picture.
//!
//! Both draw a card as its face on the duel's card page
//! (`legaia_engine_modes::cards::table_art`), which the world writes into
//! VRAM while a duel is seated: the same art the table's 3D cards show. A
//! face's body is transparent over a quad in the owner's colour, so a
//! captured card changes colour with no texture of its own.

use legaia_engine_modes::active::ActiveMode;
use legaia_engine_modes::cards::duel::{CELLS, DuelCard, DuelPhase, DuelView, HAND};
use legaia_engine_modes::cards::table_art;
use legaia_expansion_format::cards::CardElement;
use legaia_render_kernels::screen_prim::{ScreenPrim, ScreenQuad};

use super::cards::{element_color, owner_color, rarity_color, side_label};
use super::{Canvas, STAGE_H, STAGE_W, ink, pen};

/// A card on the 2D board.
const CARD_W: i32 = 38;
const CARD_H: i32 = 52;
const CELL_W: i32 = CARD_W + 4;
const CELL_H: i32 = CARD_H + 3;
const GRID_X: i32 = (STAGE_W - 3 * CELL_W) / 2;
const GRID_Y: i32 = 20;
/// The vertical step between two hand cards (they overlap).
const HAND_STEP: i32 = 27;

impl Canvas {
    /// A card face (or the back) from the duel's card page, stretched over
    /// `(x, y, w, h)`.
    pub(crate) fn card_tex(&mut self, x: i32, y: i32, w: i32, h: i32, slot: u16) {
        let ot = self.next_ot();
        let p = |x: i32, y: i32| (x.clamp(-1024, 1024) as i16, y.clamp(-1024, 1024) as i16);
        self.prims.push(ScreenPrim::Textured(ScreenQuad {
            xy: [p(x, y), p(x + w, y), p(x, y + h), p(x + w, y + h)],
            uv: table_art::slot_quad_uv(slot),
            clut: 0,
            tpage: table_art::tpage(),
            color: 0x80_80_80,
            gouraud: None,
            semi_transparent: false,
            ot_index: ot,
            depth: None,
        }));
    }
}

/// The local player's seat and the rival's.
fn seats(mode: &ActiveMode) -> (usize, usize) {
    let me = mode.local_seat.min(1);
    (me, 1 - me)
}

/// The owner's colour, from the viewer's side: the player blue, the rival
/// red, whichever seat the player holds.
fn side_color(owner: usize, me: usize) -> [u8; 4] {
    owner_color(usize::from(owner != me))
}

/// A face at `(x, y)` owned by `owner`, raised with a gold edge when `lit`.
#[allow(clippy::too_many_arguments)]
fn face(
    c: &mut Canvas,
    deck: usize,
    card: usize,
    owner: usize,
    me: usize,
    x: i32,
    y: i32,
    lit: bool,
) {
    if lit {
        c.rect(x - 2, y - 2, CARD_W + 4, CARD_H + 4, ink::HIGHLIGHT);
    }
    // Dark under the corners, the owner's colour under the body.
    c.rect(x + 1, y + 1, CARD_W - 2, CARD_H - 2, [12, 10, 14, 255]);
    c.rect(x + 3, y + 3, CARD_W - 6, CARD_H - 6, side_color(owner, me));
    c.card_tex(x, y, CARD_W, CARD_H, table_art::card_slot(deck, card));
}

pub(crate) fn element_name(e: Option<CardElement>) -> &'static str {
    match e {
        Some(CardElement::Fire) => "Fire",
        Some(CardElement::Water) => "Water",
        Some(CardElement::Wind) => "Wind",
        Some(CardElement::Earth) => "Earth",
        Some(CardElement::Thunder) => "Thunder",
        Some(CardElement::Light) => "Light",
        Some(CardElement::Dark) => "Dark",
        None => "No element",
    }
}

/// The card the local player is looking at: the one held or under the hand
/// cursor while it is their turn.
fn focus_card(v: &DuelView, me: usize) -> Option<(usize, usize)> {
    match v.phase {
        DuelPhase::PickCard { seat } if seat == me => Some((me, v.hand_cursor[me])),
        DuelPhase::PickCell { seat, card } if seat == me => Some((me, card)),
        _ => None,
    }
}

/// The turn line. A watcher reads whose turn it is by name.
fn prompt(mode: &ActiveMode, v: &DuelView, me: usize) -> String {
    if mode.watching {
        return match v.phase {
            DuelPhase::PickCard { seat } | DuelPhase::PickCell { seat, .. } => {
                format!("{}'s turn", mode.seat_label(seat, "Rival"))
            }
            DuelPhase::Spoils { winner } => {
                format!("{} takes a card", mode.seat_label(winner, "Rival"))
            }
            DuelPhase::Finished => "Duel over".to_string(),
        };
    }
    match v.phase {
        DuelPhase::PickCard { seat } if seat == me => "Your turn - choose a card",
        DuelPhase::PickCell { seat, .. } if seat == me => "Your turn - choose a cell",
        DuelPhase::PickCard { .. } | DuelPhase::PickCell { .. } => "Rival is thinking...",
        DuelPhase::Spoils { winner } if winner == me => "Take one of the rival's cards",
        DuelPhase::Spoils { .. } => "The rival takes a card",
        DuelPhase::Finished => "Duel over",
    }
    .to_string()
}

/// The selected card's details on one panel: its full name, rarity and
/// element, the four strengths, and what the cell under the cursor does to
/// it.
fn card_panel(c: &mut Canvas, mode: &ActiveMode, v: &DuelView, me: usize, x: i32, y: i32, w: i32) {
    let Some((seat, i)) = focus_card(v, me) else {
        return;
    };
    let Some(card) = v.hands[seat].get(i) else {
        return;
    };
    let h = 34;
    c.panel(x, y, w, h, [10, 14, 24, 210], ink::PANEL_EDGE);
    c.rect(x + 3, y + 3, 3, h - 6, element_color(card.element));
    c.dot(x + w - 8, y + 9, 2, rarity_color(card.rarity));
    c.text(
        mode.card_name(&card.id).to_string(),
        x + 10,
        y + 2,
        pen::WHITE,
    );
    let [t, r, b, l] = card.sides.map(side_label);
    c.text(
        format!("Up {t} Right {r} Down {b} Left {l}"),
        x + 10,
        y + 17,
        pen::GOLD,
    );
}

/// What the cursor's cell does to `card`: `+1` on its own element, `-1` on
/// another.
fn cell_hint(v: &DuelView, me: usize, card: &DuelCard) -> Option<String> {
    let DuelPhase::PickCell { seat, .. } = v.phase else {
        return None;
    };
    if seat != me {
        return None;
    }
    let e = v.cell_elements[v.cell_cursor[me]]?;
    Some(match card.element {
        Some(ce) if ce == e => format!("{} cell +1", element_name(Some(e))),
        Some(_) => format!("{} cell -1", element_name(Some(e))),
        None => format!("{} cell", element_name(Some(e))),
    })
}

/// The score plates: the player's on the left in blue, the rival's on the
/// right in red, the count large.
fn scores(c: &mut Canvas, mode: &ActiveMode, v: &DuelView, me: usize, rival: usize, y: i32) {
    let plate = [10, 14, 24, 200];
    let w = 70;
    c.panel(4, y, w, 18, plate, ink::PANEL_EDGE);
    c.rect(5, y + 1, 4, 16, owner_color(0));
    c.text(mode.seat_label(me, "You"), 13, y + 2, pen::WHITE);
    c.text_right(v.scores[me].to_string(), 4 + w - 6, y + 2, pen::GOLD);
    let x = STAGE_W - 4 - w;
    c.panel(x, y, w, 18, plate, ink::PANEL_EDGE);
    c.rect(x + w - 5, y + 1, 4, 16, owner_color(1));
    c.text(mode.seat_label(rival, "Rival"), x + 6, y + 2, pen::WHITE);
    c.text_right(v.scores[rival].to_string(), x + w - 10, y + 2, pen::GOLD);
}

/// The 2D board.
pub(super) fn draw(c: &mut Canvas, mode: &ActiveMode, v: &DuelView, log: &[String]) {
    let (me, rival) = seats(mode);
    // The board: a rim and a felt under the cells.
    c.panel(
        GRID_X - 5,
        GRID_Y - 4,
        3 * CELL_W + 8,
        3 * CELL_H + 6,
        [70, 48, 26, 255],
        [186, 146, 64, 255],
    );
    let picking_cell = match v.phase {
        DuelPhase::PickCell { seat, .. } if seat == me => Some(v.cell_cursor[me]),
        _ => None,
    };
    for cell in 0..CELLS {
        let (x, y) = cell_xy(cell);
        c.rect(x, y, CARD_W, CARD_H, [26, 92, 64, 255]);
        if let Some(e) = v.cell_elements[cell] {
            let col = element_color(Some(e));
            c.outline(x + 6, y + 10, CARD_W - 12, CARD_H - 20, col);
            c.dot(x + CARD_W / 2, y + CARD_H / 2, 3, col);
        }
        if let Some(p) = v.grid[cell] {
            face(c, p.deck, p.card, p.owner, me, x, y, false);
        }
        if picking_cell == Some(cell) {
            c.outline(x - 2, y - 2, CARD_W + 4, CARD_H + 4, ink::HIGHLIGHT);
            c.outline(x - 1, y - 1, CARD_W + 2, CARD_H + 2, ink::HIGHLIGHT);
        }
    }

    // The hands, overlapping down each side; the selected card slides out
    // toward the board.
    for seat in [rival, me] {
        let (turn, held) = match v.phase {
            DuelPhase::PickCard { seat: s } => (s == seat, None),
            DuelPhase::PickCell { seat: s, card } => (s == seat, Some(card)),
            _ => (false, None),
        };
        let selected = held.or(turn.then_some(v.hand_cursor[seat]));
        let left = seat == me;
        let x0 = if left { 10 } else { STAGE_W - 10 - CARD_W };
        for i in 0..HAND.min(v.hands[seat].len()) {
            if v.used[seat][i] {
                continue;
            }
            let lit = Some(i) == selected;
            let dx = if lit { if left { 8 } else { -8 } } else { 0 };
            let y = GRID_Y + i as i32 * HAND_STEP;
            face(c, seat, i, seat, me, x0 + dx, y, lit);
        }
    }

    // Under the board: the selected card, the turn line, the newest event.
    let sy = GRID_Y + 3 * CELL_H + 1;
    scores(c, mode, v, me, rival, sy);
    let held = focus_card(v, me).and_then(|(s, i)| v.hands[s].get(i));
    match held.and_then(|card| cell_hint(v, me, card)) {
        Some(hint) => c.text_centre(hint, STAGE_W / 2, sy + 2, pen::GREEN),
        None => c.text_centre(prompt(mode, v, me), STAGE_W / 2, sy + 2, pen::GOLD),
    }
    if focus_card(v, me).is_none()
        && let Some(last) = log.last()
    {
        c.text(last.clone(), 8, STAGE_H - 30, pen::DIM);
    }
    bottom_bar(c, mode, v, me);
    if matches!(v.phase, DuelPhase::Spoils { winner } if winner == me) {
        spoils(c, mode, v, rival);
    }
}

/// The selected card's panel bottom-left and the controls bottom-right.
fn bottom_bar(c: &mut Canvas, mode: &ActiveMode, v: &DuelView, me: usize) {
    card_panel(c, mode, v, me, 4, STAGE_H - 36, 168);
    let (a, b) = match v.phase {
        DuelPhase::PickCell { seat, .. } if seat == me => ("D-pad: cell", "Cross: place"),
        DuelPhase::Spoils { winner } if winner == me => ("Left/Right: card", "Cross: take"),
        _ => ("Left/Right: card", "Cross: choose"),
    };
    let (x, w) = (STAGE_W - 4 - 140, 140);
    c.panel(x, STAGE_H - 36, w, 34, [8, 12, 20, 170], ink::PANEL_EDGE);
    c.text_right(a, x + w - 5, STAGE_H - 35, pen::DIM);
    c.text_right(b, x + w - 5, STAGE_H - 20, pen::DIM);
}

/// The table HUD: the score plates and the turn prompt along the top, the
/// selected card's panel along the bottom. During the reaction shot only
/// the stage's banner shows.
pub(super) fn hud(c: &mut Canvas, mode: &ActiveMode, v: &DuelView, outro: bool) {
    if outro {
        return;
    }
    let (me, rival) = seats(mode);
    scores(c, mode, v, me, rival, 4);
    let plate = [10, 14, 24, 170];
    c.panel(84, 4, 152, 18, plate, ink::PANEL_EDGE);
    // Over an elemental cell the prompt says what the cell does to the
    // held card.
    let held = focus_card(v, me).and_then(|(s, i)| v.hands[s].get(i));
    match held.and_then(|card| cell_hint(v, me, card)) {
        Some(hint) => c.text_centre(hint, STAGE_W / 2, 6, pen::GREEN),
        None => c.text_centre(prompt(mode, v, me), STAGE_W / 2, 6, pen::GOLD),
    }
    // The selected card bottom-left, the controls bottom-right, the hand
    // between them.
    bottom_bar(c, mode, v, me);
    if matches!(v.phase, DuelPhase::Spoils { winner } if winner == me) {
        spoils(c, mode, v, rival);
    }
}

fn cell_xy(cell: usize) -> (i32, i32) {
    (
        GRID_X + (cell % 3) as i32 * CELL_W,
        GRID_Y + (cell / 3) as i32 * CELL_H,
    )
}

/// The stakes pick: the rival's deck, the cursor's card shown large.
fn spoils(c: &mut Canvas, mode: &ActiveMode, v: &DuelView, rival: usize) {
    let (x, y, w) = (70, 56, 180);
    let h = HAND as i32 * 16 + 8;
    c.panel(x, y, w, h, ink::PANEL, ink::HIGHLIGHT);
    for (i, card) in v.hands[rival].iter().enumerate() {
        let ry = y + 4 + i as i32 * 16;
        let on = v.spoil_cursor == i;
        if on {
            c.rect(x + 2, ry, w - 4 - CARD_W - 6, 16, ink::SELECT);
        }
        c.text(
            mode.card_name(&card.id).to_string(),
            x + 8,
            ry + 1,
            if on { pen::GOLD } else { pen::WHITE },
        );
    }
    let i = v.spoil_cursor.min(HAND - 1);
    let (fx, fy) = (x + w - CARD_W - 4, y + (h - CARD_H) / 2);
    face(c, rival, i, rival, mode.local_seat.min(1), fx, fy, true);
}

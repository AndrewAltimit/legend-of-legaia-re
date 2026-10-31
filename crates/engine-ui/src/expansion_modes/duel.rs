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
use super::{Align, Canvas, LINE, STAGE_H, STAGE_W, ink, pen};

/// The score plates' width over the table, and under the 2D board.
const HUD_SCORE_W: i32 = 84;
const BOARD_SCORE_W: i32 = 70;

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
        DuelPhase::PickCard { seat } if seat == me => "Choose a card",
        DuelPhase::PickCell { seat, .. } if seat == me => "Choose a cell",
        DuelPhase::PickCard { .. } | DuelPhase::PickCell { .. } => "Rival is thinking...",
        DuelPhase::Spoils { winner } if winner == me => "Take a rival's card",
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
    c.text_in(
        mode.card_name(&card.id).to_string(),
        x + 10,
        y + 2,
        w - 10 - 14,
        pen::WHITE,
        Align::Left,
    );
    let [t, r, b, l] = card.sides.map(side_label);
    c.text_in(
        format!("Up {t} Right {r} Down {b} Left {l}"),
        x + 10,
        y + 17,
        w - 14,
        pen::GOLD,
        Align::Left,
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
/// right in red. A one-row plate (`tall` false, under the 2D board) sets
/// the name and the count side by side; a two-row plate (the table HUD's
/// corners) gives the name the whole top row and the count the row under
/// it. Either way the name is held to its own box, clear of the count, so
/// a table NPC's long name ("The Warden's Second") is cut, not run into the
/// score.
#[allow(clippy::too_many_arguments)]
fn scores(
    c: &mut Canvas,
    mode: &ActiveMode,
    v: &DuelView,
    me: usize,
    rival: usize,
    y: i32,
    w: i32,
    tall: bool,
) {
    let plate = [10, 14, 24, 200];
    let h = if tall { 18 + LINE } else { 18 };
    // The count's box: two digits.
    const COUNT_W: i32 = 16;
    for (seat, left) in [(me, true), (rival, false)] {
        let x = if left { 4 } else { STAGE_W - 4 - w };
        c.panel(x, y, w, h, plate, ink::PANEL_EDGE);
        let bar = if left { x + 1 } else { x + w - 5 };
        c.rect(bar, y + 1, 4, h - 2, owner_color(usize::from(!left)));
        // The text between the colour bar and the far edge.
        let (l, r) = if left {
            (x + 9, x + w - 4)
        } else {
            (x + 4, x + w - 9)
        };
        let name = mode.seat_label(seat, if left { "You" } else { "Rival" });
        let count = v.scores[seat].to_string();
        if tall {
            c.text_in(name, l, y + 2, r - l, pen::WHITE, Align::Left);
            let (cx, al) = if left {
                (r, Align::Right)
            } else {
                (l, Align::Left)
            };
            c.text_in(count, cx, y + 2 + LINE, COUNT_W, pen::GOLD, al);
        } else {
            c.text_in(name, l, y + 2, r - l - COUNT_W - 2, pen::WHITE, Align::Left);
            c.text_in(count, r, y + 2, COUNT_W, pen::GOLD, Align::Right);
        }
    }
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
    scores(c, mode, v, me, rival, sy, BOARD_SCORE_W, false);
    let held = focus_card(v, me).and_then(|(s, i)| v.hands[s].get(i));
    let (line, pen_) = match held.and_then(|card| cell_hint(v, me, card)) {
        Some(hint) => (hint, pen::GREEN),
        None => (prompt(mode, v, me), pen::GOLD),
    };
    let pw = STAGE_W - 2 * (4 + BOARD_SCORE_W + 4);
    c.text_in(line, STAGE_W / 2, sy + 2, pw, pen_, Align::Centre);
    if focus_card(v, me).is_none()
        && let Some(last) = log.last()
    {
        c.text_in(
            last.clone(),
            8,
            STAGE_H - 30,
            STAGE_W - 16,
            pen::DIM,
            Align::Left,
        );
    }
    bottom_bar(c, mode, v, me);
    if matches!(v.phase, DuelPhase::Spoils { winner } if winner == me) {
        spoils(c, mode, v, rival);
    }
}

/// The selected card's panel bottom-left and the controls bottom-right.
fn bottom_bar(c: &mut Canvas, mode: &ActiveMode, v: &DuelView, me: usize) {
    // The card's line of four strengths is the widest label; the
    // controls take what is left.
    card_panel(c, mode, v, me, 4, STAGE_H - 36, 196);
    let (a, b) = match v.phase {
        DuelPhase::PickCell { seat, .. } if seat == me => ("D-pad: cell", "Cross: place"),
        DuelPhase::Spoils { winner } if winner == me => ("Left/Right: card", "Cross: take"),
        _ => ("Left/Right: card", "Cross: choose"),
    };
    let (x, w) = (STAGE_W - 4 - 112, 112);
    c.panel(x, STAGE_H - 36, w, 34, [8, 12, 20, 170], ink::PANEL_EDGE);
    c.text_in(a, x + w - 5, STAGE_H - 35, w - 10, pen::DIM, Align::Right);
    c.text_in(b, x + w - 5, STAGE_H - 20, w - 10, pen::DIM, Align::Right);
}

/// The table HUD: the score plates and the turn prompt along the top, the
/// selected card's panel along the bottom. During the reaction shot only
/// the stage's banner shows.
/// Returns the bottom of the HUD's top band (the score plates and the
/// prompt), where a banner may start.
pub(super) fn hud(c: &mut Canvas, mode: &ActiveMode, v: &DuelView, outro: bool) -> i32 {
    if outro {
        return 0;
    }
    let (me, rival) = seats(mode);
    scores(c, mode, v, me, rival, 4, HUD_SCORE_W, true);
    let plate = [10, 14, 24, 170];
    // The prompt between the two plates.
    let (px, pw) = (4 + HUD_SCORE_W + 4, STAGE_W - 2 * (4 + HUD_SCORE_W + 4));
    c.panel(px, 4, pw, 18, plate, ink::PANEL_EDGE);
    // Over an elemental cell the prompt says what the cell does to the
    // held card.
    let held = focus_card(v, me).and_then(|(s, i)| v.hands[s].get(i));
    let (line, pen_) = match held.and_then(|card| cell_hint(v, me, card)) {
        Some(hint) => (hint, pen::GREEN),
        None => (prompt(mode, v, me), pen::GOLD),
    };
    c.text_in(line, STAGE_W / 2, 6, pw - 8, pen_, Align::Centre);
    // The selected card bottom-left, the controls bottom-right, the hand
    // between them.
    bottom_bar(c, mode, v, me);
    if matches!(v.phase, DuelPhase::Spoils { winner } if winner == me) {
        spoils(c, mode, v, rival);
    }
    4 + 18 + LINE
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
        c.text_in(
            mode.card_name(&card.id).to_string(),
            x + 8,
            ry + 1,
            w - CARD_W - 6 - 12,
            if on { pen::GOLD } else { pen::WHITE },
            Align::Left,
        );
    }
    let i = v.spoil_cursor.min(HAND - 1);
    let (fx, fy) = (x + w - CARD_W - 4, y + (h - CARD_H) / 2);
    face(c, rival, i, rival, mode.local_seat.min(1), fx, fy, true);
}

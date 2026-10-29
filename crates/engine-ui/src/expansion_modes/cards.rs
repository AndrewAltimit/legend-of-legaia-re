//! Card faces: the poker deck's playing cards and the Grid Duel's monster
//! cards.

use legaia_engine_modes::cards::duel::DuelCard;
use legaia_engine_modes::poker::hand::Card;
use legaia_expansion_format::cards::{CardElement, Rarity};

use super::{Canvas, pen};

/// A playing card's size.
pub const CARD_W: i32 = 26;
pub const CARD_H: i32 = 36;

const FACE: [u8; 4] = [238, 236, 226, 255];
const FACE_EDGE: [u8; 4] = [60, 60, 70, 255];
const BACK: [u8; 4] = [40, 64, 140, 255];
const BACK_INNER: [u8; 4] = [70, 100, 180, 255];
const RED: [u8; 4] = [200, 36, 36, 255];
const BLACK: [u8; 4] = [24, 24, 30, 255];

/// 7x7 suit pips, clubs / diamonds / hearts / spades (the kernel's suit
/// order), one row per string.
const PIPS: [[&str; 7]; 4] = [
    [
        "..XXX..", "..XXX..", "XX.X.XX", "XXXXXXX", "XX.X.XX", "...X...", "..XXX..",
    ],
    [
        "...X...", "..XXX..", ".XXXXX.", "XXXXXXX", ".XXXXX.", "..XXX..", "...X...",
    ],
    [
        ".XX.XX.", "XXXXXXX", "XXXXXXX", ".XXXXX.", "..XXX..", "...X...", ".......",
    ],
    [
        "...X...", "..XXX..", ".XXXXX.", "XXXXXXX", "XXXXXXX", "...X...", "..XXX..",
    ],
];

fn is_red(suit: u8) -> bool {
    suit == 1 || suit == 2
}

/// A suit pip with its top-left at `(x, y)`, `scale` pixels per bit.
pub fn pip(c: &mut Canvas, suit: u8, x: i32, y: i32, scale: i32) {
    let col = if is_red(suit) { RED } else { BLACK };
    for (r, row) in PIPS[suit as usize % 4].iter().enumerate() {
        // Runs of set bits become one rect each.
        let bytes = row.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'X' {
                let start = i;
                while i < bytes.len() && bytes[i] == b'X' {
                    i += 1;
                }
                c.rect(
                    x + start as i32 * scale,
                    y + r as i32 * scale,
                    (i - start) as i32 * scale,
                    scale,
                    col,
                );
            } else {
                i += 1;
            }
        }
    }
}

/// A rank as printed on a card.
pub fn rank_label(rank: u8) -> &'static str {
    match rank {
        2 => "2",
        3 => "3",
        4 => "4",
        5 => "5",
        6 => "6",
        7 => "7",
        8 => "8",
        9 => "9",
        10 => "10",
        11 => "J",
        12 => "Q",
        13 => "K",
        _ => "A",
    }
}

/// A face-up playing card with its top-left at `(x, y)`.
pub fn playing_card(c: &mut Canvas, card: Card, x: i32, y: i32) {
    c.panel(x, y, CARD_W, CARD_H, FACE, FACE_EDGE);
    let ink = if is_red(card.suit()) {
        pen::RED
    } else {
        pen::INK
    };
    c.text(rank_label(card.rank()), x + 3, y, ink);
    pip(c, card.suit(), x + CARD_W - 16, y + CARD_H - 17, 2);
}

/// A face-down playing card.
pub fn card_back(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
    c.panel(x, y, w, h, BACK, FACE_EDGE);
    c.outline(x + 3, y + 3, w - 6, h - 6, BACK_INNER);
}

/// An empty card slot.
pub fn card_slot(c: &mut Canvas, x: i32, y: i32) {
    c.outline(x, y, CARD_W, CARD_H, [60, 130, 90, 255]);
}

/// A Grid Duel card's size.
pub const DUEL_W: i32 = 52;
pub const DUEL_H: i32 = 52;

/// The owner's frame colour.
pub fn owner_color(seat: usize) -> [u8; 4] {
    if seat == 0 {
        [56, 104, 196, 255]
    } else {
        [190, 60, 60, 255]
    }
}

pub fn element_color(e: Option<CardElement>) -> [u8; 4] {
    match e {
        Some(CardElement::Fire) => [230, 100, 40, 255],
        Some(CardElement::Water) => [60, 140, 230, 255],
        Some(CardElement::Wind) => [120, 210, 140, 255],
        Some(CardElement::Earth) => [170, 130, 70, 255],
        Some(CardElement::Thunder) => [230, 210, 60, 255],
        Some(CardElement::Light) => [240, 240, 200, 255],
        Some(CardElement::Dark) => [120, 70, 160, 255],
        None => [110, 110, 120, 255],
    }
}

pub(crate) fn rarity_color(r: Rarity) -> [u8; 4] {
    match r {
        Rarity::Common => [150, 150, 150, 255],
        Rarity::Uncommon => [90, 190, 110, 255],
        Rarity::Rare => [80, 150, 240, 255],
        Rarity::Legendary => [236, 184, 64, 255],
    }
}

/// A side strength as printed ("A" for 10, as the classic card game does).
pub(crate) fn side_label(v: u8) -> String {
    if v >= 10 { "A".into() } else { v.to_string() }
}

/// A duel card at `(x, y)` owned by `owner`: the four side strengths on a
/// cross, the art placeholder frame, the element strip and the name.
pub fn duel_card(c: &mut Canvas, card: &DuelCard, name: &str, owner: usize, x: i32, y: i32) {
    c.panel(x, y, DUEL_W, DUEL_H, owner_color(owner), [16, 16, 24, 255]);
    // The art placeholder: a frame where the monster portrait goes.
    c.panel(
        x + 4,
        y + 4,
        DUEL_W - 8,
        DUEL_H - 22,
        [20, 24, 36, 255],
        rarity_color(card.rarity),
    );
    c.rect(
        x + 4,
        y + DUEL_H - 17,
        DUEL_W - 8,
        3,
        element_color(card.element),
    );
    let [top, right, bottom, left] = card.sides;
    let cx = x + DUEL_W / 2;
    c.text_centre(side_label(top), cx, y + 2, pen::WHITE);
    c.text_centre(side_label(bottom), cx, y + 18, pen::WHITE);
    c.text(side_label(left), x + 7, y + 10, pen::WHITE);
    c.text_right(side_label(right), x + DUEL_W - 6, y + 10, pen::WHITE);
    // The name on the strip below the art; long names are cut to fit.
    let short: String = name.chars().take(7).collect();
    c.text_centre(short, cx, y + DUEL_H - 15, pen::WHITE);
}

/// A hand row's size: hands list as rows rather than stacked cards, since a
/// label in the text layer would show through any panel drawn over it.
pub const ROW_W: i32 = 74;
pub const ROW_H: i32 = 38;

/// One hand card as a row: name, the four sides as `top-right-bottom-left`,
/// the element strip and the rarity mark. `lit` frames the selected card.
pub fn hand_row(
    c: &mut Canvas,
    card: &DuelCard,
    name: &str,
    owner: usize,
    x: i32,
    y: i32,
    lit: bool,
) {
    let edge = if lit {
        super::ink::HIGHLIGHT
    } else {
        [16, 16, 24, 255]
    };
    c.panel(x, y, ROW_W, ROW_H, owner_color(owner), edge);
    if lit {
        c.outline(x + 1, y + 1, ROW_W - 2, ROW_H - 2, super::ink::HIGHLIGHT);
    }
    c.rect(x + 3, y + 3, 3, ROW_H - 6, element_color(card.element));
    c.dot(x + ROW_W - 6, y + 6, 2, rarity_color(card.rarity));
    let short: String = name.chars().take(9).collect();
    c.text(short, x + 9, y + 2, pen::WHITE);
    let [t, r, b, l] = card.sides.map(side_label);
    c.text(format!("{t}-{r}-{b}-{l}"), x + 9, y + 19, pen::GOLD);
}

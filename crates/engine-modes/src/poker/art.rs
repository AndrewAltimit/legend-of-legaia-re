//! The poker table's card art as a texture page: playing-card faces, the
//! card back and a strip of digit glyphs for the floating bet amounts,
//! composed as 15-bit PSX texels the world writes into VRAM while an
//! in-world poker table runs (the same page rectangle as the Grid Duel's,
//! [`crate::cards::table_art::PAGE_RECT`]).
//!
//! Hidden information stays hidden: only cards the viewer may see get a
//! face - the viewer's own hole cards, the board, and the hands shown at a
//! showdown. Every other card draws its back and has no face on the page.

use super::hand::Card;
use crate::cards::table_art::{PAGE_X, PAGE_Y, texel};

/// One card's texture, texels.
pub const CARD_W: u16 = 32;
pub const CARD_H: u16 = 44;
const PER_ROW: u16 = 6;
/// Hole-card slots: two per seat, six seats.
pub const HOLE_SLOTS: u16 = 12;
/// The board's first slot.
pub const BOARD_SLOT: u16 = 12;
/// The back's slot.
pub const BACK_SLOT: u16 = 17;
/// The digit strip's row and one glyph's cell.
pub const DIGITS_V: u8 = 132;
pub const GLYPH_W: u8 = 8;
pub const GLYPH_H: u8 = 12;
/// The glyph index of the dealer button's `D`.
pub const GLYPH_D: u8 = 10;

/// Seat `seat`'s hole card `i`.
pub fn hole_slot(seat: usize, i: usize) -> u16 {
    (seat.min(5) * 2 + i.min(1)) as u16
}

/// Board card `i`.
pub fn board_slot(i: usize) -> u16 {
    BOARD_SLOT + i.min(4) as u16
}

/// A slot's texture rectangle `[u, v, w, h]`, page-relative.
pub fn slot_rect(slot: u16) -> [u8; 4] {
    let s = slot.min(BACK_SLOT);
    [
        ((s % PER_ROW) * CARD_W) as u8,
        ((s / PER_ROW) * CARD_H) as u8,
        CARD_W as u8,
        CARD_H as u8,
    ]
}

/// A glyph's texture rectangle (`0..=9` digits, [`GLYPH_D`]).
pub fn glyph_rect(g: u8) -> [u8; 4] {
    [g.min(GLYPH_D) * GLYPH_W, DIGITS_V, GLYPH_W, GLYPH_H]
}

/// 3x5 glyphs: digits, then `D`.
const GLYPHS: [[&str; 5]; 11] = [
    ["XXX", "X.X", "X.X", "X.X", "XXX"],
    [".X.", "XX.", ".X.", ".X.", "XXX"],
    ["XX.", "..X", ".X.", "X..", "XXX"],
    ["XX.", "..X", ".X.", "..X", "XX."],
    ["X.X", "X.X", "XXX", "..X", "..X"],
    ["XXX", "X..", "XX.", "..X", "XX."],
    [".XX", "X..", "XXX", "X.X", "XXX"],
    ["XXX", "..X", ".X.", ".X.", ".X."],
    ["XXX", "X.X", "XXX", "X.X", "XXX"],
    ["XXX", "X.X", "XXX", "..X", "XX."],
    ["XX.", "X.X", "X.X", "X.X", "XX."],
];

/// Rank glyphs, 3x5 (`T` as a narrow 10 drawn 5 wide).
fn rank_glyph(rank: u8) -> &'static [&'static str; 5] {
    const A: [&str; 5] = [".X.", "X.X", "XXX", "X.X", "X.X"];
    const J: [&str; 5] = ["..X", "..X", "..X", "X.X", ".X."];
    const Q: [&str; 5] = [".X.", "X.X", "X.X", "XX.", ".XX"];
    const K: [&str; 5] = ["X.X", "XX.", "X..", "XX.", "X.X"];
    const T: [&str; 5] = ["X.XXX", "X.X.X", "X.X.X", "X.X.X", "X.XXX"];
    match rank {
        14 => &A,
        13 => &K,
        12 => &Q,
        11 => &J,
        10 => &T,
        n => &GLYPHS[usize::from(n.clamp(2, 9))],
    }
}

/// 7x7 suit pips: clubs, diamonds, hearts, spades.
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

struct Img {
    w: i32,
    h: i32,
    px: Vec<u16>,
}

impl Img {
    fn new(w: u16, h: u16) -> Self {
        Self {
            w: i32::from(w),
            h: i32::from(h),
            px: vec![0; usize::from(w) * usize::from(h)],
        }
    }

    fn set(&mut self, x: i32, y: i32, c: u16) {
        if (0..self.w).contains(&x) && (0..self.h).contains(&y) {
            self.px[(y * self.w + x) as usize] = c;
        }
    }

    /// Bitmap rows `rows` at `(x, y)`, each bit `scale` texels, mirrored
    /// through the glyph's centre when `flip`.
    fn bits(&mut self, rows: &[&str], x: i32, y: i32, scale: i32, c: u16, flip: bool) {
        let h = rows.len() as i32;
        for (r, row) in rows.iter().enumerate() {
            let w = row.len() as i32;
            for (k, b) in row.bytes().enumerate() {
                if b != b'X' {
                    continue;
                }
                let (cx, cy) = if flip {
                    (w - 1 - k as i32, h - 1 - r as i32)
                } else {
                    (k as i32, r as i32)
                };
                for dy in 0..scale {
                    for dx in 0..scale {
                        self.set(x + cx * scale + dx, y + cy * scale + dy, c);
                    }
                }
            }
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> u16 {
    texel(r, g, b)
}

/// A playing card's face: a cream card with the rank and suit in two
/// corners and a large pip in the middle.
pub fn compose_face(card: Card) -> Vec<u16> {
    let mut img = Img::new(CARD_W, CARD_H);
    let (w, h) = (i32::from(CARD_W), i32::from(CARD_H));
    for y in 0..h {
        for x in 0..w {
            let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            let k = 1.0 - y as f32 / h as f32 * 0.08;
            let c = if edge {
                rgb(40, 36, 44)
            } else {
                rgb((246.0 * k) as u8, (240.0 * k) as u8, (226.0 * k) as u8)
            };
            img.set(x, y, c);
        }
    }
    for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
        img.set(x, y, 0);
    }
    let red = matches!(card.suit(), 1 | 2);
    let ink = if red {
        rgb(200, 30, 36)
    } else {
        rgb(22, 22, 30)
    };
    let glyph = rank_glyph(card.rank());
    let pip = &PIPS[usize::from(card.suit() % 4)];
    img.bits(glyph, 3, 3, 2, ink, false);
    img.bits(pip, 3, 15, 1, ink, false);
    let gw = glyph[0].len() as i32 * 2;
    img.bits(glyph, w - 3 - gw, h - 13, 2, ink, true);
    img.bits(pip, w - 10, h - 22, 1, ink, true);
    img.bits(pip, w / 2 - 7, h / 2 - 7, 2, ink, false);
    img.px
}

/// The card back: a wine-red field under a gold diamond lattice in a gold
/// border.
pub fn compose_back() -> Vec<u16> {
    let mut img = Img::new(CARD_W, CARD_H);
    let (w, h) = (i32::from(CARD_W), i32::from(CARD_H));
    for y in 0..h {
        for x in 0..w {
            let d = x.min(y).min(w - 1 - x).min(h - 1 - y);
            let c = match d {
                0 => rgb(30, 20, 18),
                1 | 2 => rgb(214, 172, 80),
                3 => rgb(70, 20, 24),
                _ => {
                    let (u, v) = (x - w / 2, y - h / 2);
                    if (u + v).rem_euclid(6) == 0 || (u - v).rem_euclid(6) == 0 {
                        rgb(190, 140, 70)
                    } else {
                        rgb(120, 26, 34)
                    }
                }
            };
            img.set(x, y, c);
        }
    }
    for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
        img.set(x, y, 0);
    }
    img.px
}

/// The digit strip: `0..=9` and `D`, white on a dark outline, transparent
/// round them.
pub fn compose_glyphs() -> Vec<u16> {
    let (gw, gh) = (u16::from(GLYPH_W), u16::from(GLYPH_H));
    let mut img = Img::new(gw * (u16::from(GLYPH_D) + 1), gh);
    let (ink, edge) = (rgb(255, 246, 214), rgb(14, 10, 8));
    for (g, rows) in GLYPHS.iter().enumerate() {
        let x0 = g as i32 * i32::from(GLYPH_W) + 1;
        // The outline: the glyph stamped round its eight neighbours first.
        for (dx, dy) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            img.bits(rows, x0 + dx, 1 + dy, 2, edge, false);
        }
        img.bits(rows, x0, 1, 2, ink, false);
    }
    img.px
}

/// What the viewer may see this frame: each seat's hole cards when known
/// (`None` draws backs), and the board.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VisibleCards {
    pub holes: Vec<Option<[Card; 2]>>,
    pub board: Vec<Card>,
}

/// Every texture of the table, `(x, y, w, h, texels)` in VRAM.
pub fn compose_page(cards: &VisibleCards) -> Vec<(u16, u16, u16, u16, Vec<u16>)> {
    let mut out = Vec::new();
    let mut put = |r: [u8; 4], px: Vec<u16>| {
        out.push((
            PAGE_X + u16::from(r[0]),
            PAGE_Y + u16::from(r[1]),
            u16::from(r[2]),
            u16::from(r[3]),
            px,
        ));
    };
    for (seat, hole) in cards.holes.iter().enumerate().take(6) {
        if let Some(h) = hole {
            for (i, c) in h.iter().enumerate() {
                put(slot_rect(hole_slot(seat, i)), compose_face(*c));
            }
        }
    }
    for (i, c) in cards.board.iter().enumerate().take(5) {
        put(slot_rect(board_slot(i)), compose_face(*c));
    }
    put(slot_rect(BACK_SLOT), compose_back());
    let g = glyph_rect(0);
    let strip = compose_glyphs();
    let w = (strip.len() / usize::from(GLYPH_H)) as u8;
    put([g[0], g[1], w, GLYPH_H], strip);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::table_art::PAGE_RECT;

    #[test]
    fn the_page_fits_the_shared_rectangle() {
        let (_, _, pw, ph) = PAGE_RECT;
        let r = slot_rect(BACK_SLOT);
        assert!(u16::from(r[0]) + u16::from(r[2]) <= pw);
        assert!(u16::from(r[1]) + u16::from(r[3]) <= u16::from(DIGITS_V));
        let g = glyph_rect(GLYPH_D);
        assert!(u16::from(g[0]) + u16::from(g[2]) <= pw);
        assert!(u16::from(g[1]) + u16::from(g[3]) <= ph);
        assert_eq!(hole_slot(5, 1), 11);
        assert_eq!(board_slot(4), 16);
    }

    #[test]
    fn hidden_holes_get_no_face() {
        let shown = VisibleCards {
            holes: vec![Some([Card::new(14, 0), Card::new(13, 3)]), None, None],
            board: vec![Card::new(2, 1), Card::new(10, 2), Card::new(11, 0)],
        };
        let page = compose_page(&shown);
        // Two hole faces, three board faces, the back, the glyph strip.
        assert_eq!(page.len(), 7);
        let r = slot_rect(hole_slot(1, 0));
        let at = (PAGE_X + u16::from(r[0]), PAGE_Y + u16::from(r[1]));
        assert!(
            page.iter().all(|p| (p.0, p.1) != at),
            "seat 1's cards stay faceless"
        );
    }

    #[test]
    fn a_red_card_inks_red() {
        let px = compose_face(Card::new(14, 2));
        // The centre pip is red.
        let mid = px[22 * usize::from(CARD_W) + 16];
        assert!(mid & 0x1F > 20 && (mid >> 10) & 0x1F < 10, "{mid:#06x}");
    }
}

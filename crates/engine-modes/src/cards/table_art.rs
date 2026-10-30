//! The Grid Duel's card art as a texture page: each card's face (frame,
//! monster portrait, element gem, side strengths) and the shared card back,
//! composed as 15-bit PSX texels the world writes into VRAM while a duel
//! runs, so both hosts draw the cards as ordinary textured primitives - on
//! the 3D table and on the 2D screen alike.
//!
//! The page is the port's own art, built here from the session's cards and
//! the portraits the scene host rendered off the player's disc; it holds no
//! disc bytes of its own. A face's body is left transparent (texel `0`), so
//! the owner's colour - a quad the drawing puts under the face - shows
//! through it: a captured card changes colour without a second texture.
//!
//! The page sits in retail's display-buffer band, which the port never
//! draws a frame into ([`PAGE_X`], [`PAGE_Y`]); the pack-map VRAM allocator
//! keeps the same rectangle out of its free list
//! (`legaia_engine_scene::expansion_map::vram_alloc::RESERVED`).

use legaia_expansion_format::cards::{CardElement, Rarity};

use super::duel::{DuelCard, HAND};

/// The page's top-left halfword in VRAM.
pub const PAGE_X: u16 = 0;
pub const PAGE_Y: u16 = 256;
/// One card texture's size, texels.
pub const FACE_W: u16 = 52;
pub const FACE_H: u16 = 72;
/// Slots a row.
pub const SLOTS_PER_ROW: u16 = 4;
/// Slots on the page: both hands, the back, one spare.
pub const SLOTS: u16 = 12;
/// The card back's slot.
pub const BACK_SLOT: u16 = 2 * HAND as u16;
/// The rectangle the page covers, `(x, y, w, h)`.
pub const PAGE_RECT: (u16, u16, u16, u16) = (
    PAGE_X,
    PAGE_Y,
    SLOTS_PER_ROW * FACE_W,
    SLOTS.div_ceil(SLOTS_PER_ROW) * FACE_H,
);

/// The page's GP0 texpage word: a 15-bit direct page at
/// `(PAGE_X, PAGE_Y)`.
pub const fn tpage() -> u16 {
    (PAGE_X / 64) | ((PAGE_Y / 256) << 4) | (2 << 7)
}

/// The slot of hand card `card` of `deck` (the seat that dealt it).
pub fn card_slot(deck: usize, card: usize) -> u16 {
    (deck.min(1) * HAND + card.min(HAND - 1)) as u16
}

/// A slot's top-left texel, page-relative `(u, v)`.
pub fn slot_uv(slot: u16) -> (u8, u8) {
    let s = slot.min(SLOTS - 1);
    (
        ((s % SLOTS_PER_ROW) * FACE_W) as u8,
        ((s / SLOTS_PER_ROW) * FACE_H) as u8,
    )
}

/// A slot's four UV corners in `POLY_FT4` order (top-left, top-right,
/// bottom-left, bottom-right), inset half a texel so a magnified face
/// never samples its neighbour.
pub fn slot_quad_uv(slot: u16) -> [(u8, u8); 4] {
    let (u, v) = slot_uv(slot);
    let (r, b) = (u + FACE_W as u8 - 1, v + FACE_H as u8 - 1);
    [(u, v), (r, v), (u, b), (r, b)]
}

/// A 15-bit texel from 8-bit RGB, never `0` (which draws transparent).
pub fn texel(r: u8, g: u8, b: u8) -> u16 {
    let v = u16::from(r >> 3) | (u16::from(g >> 3) << 5) | (u16::from(b >> 3) << 10);
    if v == 0 { 0x8000 } else { v }
}

/// A portrait as the scene host rendered it: RGBA bytes, `w * h * 4`.
#[derive(Debug, Clone, Copy)]
pub struct PortraitPixels<'a> {
    pub w: u16,
    pub h: u16,
    pub rgba: &'a [u8],
}

/// The element colour (the 2D screen's palette).
pub fn element_rgb(e: Option<CardElement>) -> [u8; 3] {
    match e {
        Some(CardElement::Fire) => [230, 100, 40],
        Some(CardElement::Water) => [60, 140, 230],
        Some(CardElement::Wind) => [120, 210, 140],
        Some(CardElement::Earth) => [170, 130, 70],
        Some(CardElement::Thunder) => [230, 210, 60],
        Some(CardElement::Light) => [240, 240, 200],
        Some(CardElement::Dark) => [150, 90, 200],
        None => [110, 110, 120],
    }
}

/// The frame's metal per rarity: bronze, silver, a blue steel, gold.
pub fn rarity_rgb(r: Rarity) -> [u8; 3] {
    match r {
        Rarity::Common => [176, 120, 72],
        Rarity::Uncommon => [196, 204, 214],
        Rarity::Rare => [96, 164, 236],
        Rarity::Legendary => [240, 198, 76],
    }
}

/// 3x5 digits `1..=9` and `A` (10), one row per string.
const DIGITS: [[&str; 5]; 10] = [
    [".X.", "XX.", ".X.", ".X.", "XXX"],
    ["XX.", "..X", ".X.", "X..", "XXX"],
    ["XX.", "..X", ".X.", "..X", "XX."],
    ["X.X", "X.X", "XXX", "..X", "..X"],
    ["XXX", "X..", "XX.", "..X", "XX."],
    [".XX", "X..", "XXX", "X.X", "XXX"],
    ["XXX", "..X", ".X.", ".X.", ".X."],
    ["XXX", "X.X", "XXX", "X.X", "XXX"],
    ["XXX", "X.X", "XXX", "..X", "XX."],
    [".X.", "X.X", "XXX", "X.X", "X.X"],
];

/// One texture being painted.
struct Img {
    w: usize,
    h: usize,
    px: Vec<u16>,
}

impl Img {
    fn new(w: u16, h: u16) -> Self {
        Self {
            w: usize::from(w),
            h: usize::from(h),
            px: vec![0; usize::from(w) * usize::from(h)],
        }
    }

    fn set(&mut self, x: i32, y: i32, c: u16) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }

    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }

    /// A digit glyph with a one-texel dark outline.
    fn digit(&mut self, v: u8, x: i32, y: i32, ink: u16, outline: u16) {
        let g = &DIGITS[usize::from(v.clamp(1, 10) - 1)];
        let on = |cx: i32, cy: i32| {
            (0..3).contains(&cx)
                && (0..5).contains(&cy)
                && g[cy as usize].as_bytes()[cx as usize] == b'X'
        };
        for cy in -1..6 {
            for cx in -1..4 {
                if on(cx, cy) {
                    self.set(x + cx, y + cy, ink);
                } else if (-1..=1).any(|dy| (-1..=1).any(|dx| on(cx + dx, cy + dy))) {
                    self.set(x + cx, y + cy, outline);
                }
            }
        }
    }

    /// Clear the four corners to transparent for a rounded outline.
    fn round_corners(&mut self) {
        let (w, h) = (self.w as i32, self.h as i32);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (2, 0), (0, 2)] {
            for (cx, cy) in [
                (x, y),
                (w - 1 - x, y),
                (x, h - 1 - y),
                (w - 1 - x, h - 1 - y),
            ] {
                self.set(cx, cy, 0);
            }
        }
    }
}

fn shade(c: [u8; 3], k: f32) -> [u8; 3] {
    c.map(|v| (f32::from(v) * k).clamp(0.0, 255.0) as u8)
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (f32::from(a[i]) * (1.0 - t) + f32::from(b[i]) * t) as u8)
}

fn tex(c: [u8; 3]) -> u16 {
    texel(c[0], c[1], c[2])
}

/// The metal frame: a dark outer line, a three-texel bevel lit from the
/// top-left, a dark inner line.
fn frame(img: &mut Img, metal: [u8; 3]) {
    let (w, h) = (img.w as i32, img.h as i32);
    let dark = tex([18, 14, 12]);
    for y in 0..h {
        for x in 0..w {
            let d = x.min(y).min(w - 1 - x).min(h - 1 - y);
            let c = match d {
                0 => dark,
                1..=3 => {
                    // Light along the top and left edges, shadow along the
                    // bottom and right, a sheen down the vertical.
                    let lit = if x.min(y) == d { 1.25 } else { 0.7 };
                    let sheen = 1.0 + 0.18 * (1.0 - (y as f32 / h as f32 - 0.3).abs() * 2.0);
                    let inner = [1.0, 1.08, 0.92][(d - 1) as usize];
                    tex(shade(metal, lit * sheen * inner))
                }
                4 => dark,
                _ => continue,
            };
            img.set(x, y, c);
        }
    }
    img.round_corners();
}

/// The portrait window: the monster over a vignette in its element's
/// colour, box-filtered down from the scene host's render.
fn portrait_window(
    img: &mut Img,
    rect: (i32, i32, i32, i32),
    element: [u8; 3],
    portrait: Option<PortraitPixels<'_>>,
) {
    let (x0, y0, w, h) = rect;
    for y in 0..h {
        for x in 0..w {
            // The vignette: the element's colour, dark at the rim.
            let (u, v) = (
                (x as f32 + 0.5) / w as f32 - 0.5,
                (y as f32 + 0.5) / h as f32 - 0.45,
            );
            let r = (u * u + v * v).sqrt() * 1.6;
            let back = shade(mix(element, [24, 22, 34], 0.55), (1.0 - r * 0.8).max(0.25));
            let mut c = back;
            if let Some(p) = portrait {
                let (sw, sh) = (f32::from(p.w), f32::from(p.h));
                let sx0 = x as f32 * sw / w as f32;
                let sx1 = (x + 1) as f32 * sw / w as f32;
                let sy0 = y as f32 * sh / h as f32;
                let sy1 = (y + 1) as f32 * sh / h as f32;
                let mut acc = [0.0f32; 4];
                let mut n = 0.0f32;
                let mut sy = sy0.floor() as i32;
                while (sy as f32) < sy1 {
                    let mut sx = sx0.floor() as i32;
                    while (sx as f32) < sx1 {
                        let wx = (sx1.min(sx as f32 + 1.0) - sx0.max(sx as f32)).max(0.0);
                        let wy = (sy1.min(sy as f32 + 1.0) - sy0.max(sy as f32)).max(0.0);
                        let k = wx * wy;
                        let i = (sy as usize * usize::from(p.w) + sx as usize) * 4;
                        if let Some(px) = p.rgba.get(i..i + 4) {
                            let a = f32::from(px[3]) / 255.0;
                            for ch in 0..3 {
                                acc[ch] += f32::from(px[ch]) * a * k;
                            }
                            acc[3] += a * k;
                        }
                        n += k;
                        sx += 1;
                    }
                    sy += 1;
                }
                if n > 0.0 && acc[3] > 0.0 {
                    let cover = (acc[3] / n).clamp(0.0, 1.0);
                    let fg = [0, 1, 2].map(|ch| (acc[ch] / acc[3]).clamp(0.0, 255.0) as u8);
                    c = mix(back, fg, cover);
                }
            }
            img.set(x0 + x, y0 + y, tex(c));
        }
    }
    // A thin gold rule under the window.
    img.fill(x0, y0 + h, w, 1, tex([206, 168, 82]));
    img.fill(x0, y0 - 1, w, 1, tex([12, 10, 14]));
}

/// A faceted element gem centred on `(cx, cy)`.
fn gem(img: &mut Img, cx: i32, cy: i32, c: [u8; 3]) {
    for dy in -4i32..=4 {
        for dx in -4i32..=4 {
            let d = dx.abs() + dy.abs();
            if d > 4 {
                continue;
            }
            let k = if d == 4 {
                0.35
            } else if dx + dy < 0 {
                1.3
            } else {
                0.85
            };
            img.set(cx + dx, cy + dy, tex(shade(c, k)));
        }
    }
    img.set(cx - 1, cy - 2, tex([255, 255, 255]));
}

/// The face of `card`: frame in its rarity's metal, the portrait window,
/// the element gem, the four side strengths on a cross and the rarity pips.
/// The body under the strengths stays transparent for the owner's colour.
pub fn compose_face(card: &DuelCard, portrait: Option<PortraitPixels<'_>>) -> Vec<u16> {
    let mut img = Img::new(FACE_W, FACE_H);
    frame(&mut img, rarity_rgb(card.rarity));
    let el = element_rgb(card.element);
    portrait_window(&mut img, (5, 6, 42, 32), el, portrait);
    if card.element.is_some() {
        gem(&mut img, 41, 11, el);
    }
    // The strengths on a cross under the art, white on a dark outline.
    let [top, right, bottom, left] = card.sides;
    let (ink, edge) = (tex([250, 248, 236]), tex([16, 12, 10]));
    let cx = i32::from(FACE_W) / 2 - 1;
    img.digit(top, cx, 42, ink, edge);
    img.digit(left, cx - 7, 49, ink, edge);
    img.digit(right, cx + 7, 49, ink, edge);
    img.digit(bottom, cx, 56, ink, edge);
    // Rarity pips down the bottom-right corner.
    let pip = tex(shade(rarity_rgb(card.rarity), 1.15));
    for i in 0..=(card.rarity as i32) {
        img.fill(43 - i * 3, 63, 2, 2, pip);
    }
    img.px
}

/// The shared card back: a deep indigo field under a gold lattice and a
/// sun emblem, in a dark-gold frame.
pub fn compose_back() -> Vec<u16> {
    let mut img = Img::new(FACE_W, FACE_H);
    let (w, h) = (i32::from(FACE_W), i32::from(FACE_H));
    for y in 5..h - 5 {
        for x in 5..w - 5 {
            let (u, v) = (x - w / 2, y - h / 2);
            let lattice = (u + v).rem_euclid(8) == 0 || (u - v).rem_euclid(8) == 0;
            let r = ((u * u + v * v) as f32).sqrt();
            let base = shade([40, 48, 120], 1.15 - r / 60.0);
            let c = if lattice {
                mix(base, [120, 140, 220], 0.45)
            } else {
                base
            };
            img.set(x, y, tex(c));
        }
    }
    // The emblem: a gold ring with rays.
    let (cx, cy) = (w / 2, h / 2);
    for y in -12..=12 {
        for x in -12..=12 {
            let r = ((x * x + y * y) as f32).sqrt();
            let ang = (y as f32).atan2(x as f32);
            let ray = (ang * 4.0).cos().abs() > 0.92 && r < 12.0 && r > 7.0;
            if (5.5..7.5).contains(&r) || ray {
                let k = 1.2 - (y as f32 + 12.0) / 40.0;
                img.set(cx + x, cy + y, tex(shade([236, 190, 80], k)));
            } else if r < 5.5 {
                img.set(cx + x, cy + y, tex(shade([180, 60, 50], 1.1 - r / 12.0)));
            }
        }
    }
    frame(&mut img, [196, 150, 70]);
    img.px
}

/// Every texture of a duel, `(x, y, w, h, texels)` in VRAM: the ten hand
/// faces and the back. `portrait` answers a card id's portrait.
pub fn compose_page<'a>(
    hands: &[Vec<DuelCard>; 2],
    portrait: &dyn Fn(&str) -> Option<PortraitPixels<'a>>,
) -> Vec<(u16, u16, u16, u16, Vec<u16>)> {
    let mut out = Vec::new();
    let mut put = |slot: u16, px: Vec<u16>| {
        let (u, v) = slot_uv(slot);
        out.push((
            PAGE_X + u16::from(u),
            PAGE_Y + u16::from(v),
            FACE_W,
            FACE_H,
            px,
        ));
    };
    for (deck, hand) in hands.iter().enumerate() {
        for (i, card) in hand.iter().enumerate().take(HAND) {
            put(card_slot(deck, i), compose_face(card, portrait(&card.id)));
        }
    }
    put(BACK_SLOT, compose_back());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(sides: [u8; 4]) -> DuelCard {
        DuelCard {
            id: "c".into(),
            sides,
            element: Some(CardElement::Fire),
            rarity: Rarity::Rare,
        }
    }

    #[test]
    fn the_page_fits_one_texture_page_in_the_display_band() {
        let (x, y, w, h) = PAGE_RECT;
        assert!(x + w <= 256 && y >= 256 && y + h <= 479, "{PAGE_RECT:?}");
        assert_eq!(tpage(), 0x110);
        let (u, v) = slot_uv(SLOTS - 1);
        assert!(u16::from(u) + FACE_W <= 256 && u16::from(v) + FACE_H <= 256);
        assert_eq!(card_slot(1, 4), 9);
        const { assert!(BACK_SLOT < SLOTS) };
    }

    #[test]
    fn a_face_leaves_its_body_clear_and_its_corners_round() {
        let px = compose_face(&card([1, 5, 9, 10]), None);
        assert_eq!(px.len(), usize::from(FACE_W) * usize::from(FACE_H));
        assert_eq!(px[0], 0, "a rounded corner is transparent");
        // The body beside the strengths shows the owner's colour.
        let at = |x: usize, y: usize| px[y * usize::from(FACE_W) + x];
        assert_eq!(at(8, 60), 0);
        // The frame and the art window are opaque.
        assert_ne!(at(2, 30), 0);
        assert_ne!(at(20, 20), 0);
    }

    #[test]
    fn a_portrait_lands_in_the_window() {
        // A solid red portrait covers the window's middle.
        let rgba: Vec<u8> = (0..64 * 48).flat_map(|_| [255, 0, 0, 255]).collect();
        let p = PortraitPixels {
            w: 64,
            h: 48,
            rgba: &rgba,
        };
        let px = compose_face(&card([3, 3, 3, 3]), Some(p));
        let mid = px[20 * usize::from(FACE_W) + 20];
        assert_eq!(mid & 0x1F, 31, "red channel full: {mid:#06x}");
        assert_eq!((mid >> 10) & 0x1F, 0);
    }

    #[test]
    fn the_page_holds_both_hands_and_the_back() {
        let hands = [vec![card([1, 2, 3, 4]); 5], vec![card([5, 6, 7, 8]); 5]];
        let page = compose_page(&hands, &|_| None);
        assert_eq!(page.len(), 11);
        for (x, y, w, h, px) in &page {
            assert!(*x + *w <= PAGE_RECT.0 + PAGE_RECT.2);
            assert!(*y + *h <= PAGE_RECT.1 + PAGE_RECT.3);
            assert_eq!(px.len(), usize::from(*w) * usize::from(*h));
        }
        // Same input, same texels.
        assert_eq!(page, compose_page(&hands, &|_| None));
    }
}

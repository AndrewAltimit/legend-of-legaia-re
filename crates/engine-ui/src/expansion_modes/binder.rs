//! The trading-card screens: the card menu and the dealer's counter, the
//! binder and its card detail, the booster list and the booster reveal, the
//! sell list and the deck builder - one composition per
//! [`legaia_engine_modes::cards::screen::CardScreen`] stage, drawn in the
//! same flat style and the same two halves (prims, labels) as the mode
//! screens.
//!
//! A monster card's face shows its retail monster as a portrait: an RGBA
//! image the host rendered once from the player's own disc and hands in
//! through [`CardScreenView::portrait`]. The image is drawn as flat runs of
//! stage pixels, one rect per run of equal 15-bit colour along a row, so both
//! hosts draw it through the ordinary screen-prim pass with no texture of
//! its own. A card without a portrait (no disc yet, or art that is not a
//! monster) shows its element colour in the frame instead.

use legaia_engine_modes::cards::CardBook;
use legaia_engine_modes::cards::economy::{CardProgress, DECK_SIZE};
use legaia_engine_modes::cards::screen::{
    BINDER_COLUMNS, BINDER_PAGE, BinderEntry, CardScreen, CardStage, MenuRow, binder_entries,
    booster_name, card_name,
};
use legaia_expansion_format::cards::{CardDef, Rarity};
use legaia_expansion_format::refs::DiscRef;
use legaia_render_kernels::screen_prim::ScreenPrim;

use super::cards::{element_color, rarity_color, side_label};
use super::{Canvas, LINE, STAGE_H, STAGE_W, footer, header, ink, labels_to_draws, pen};
use crate::TextDraw;

/// One portrait image, RGBA8 row-major.
#[derive(Debug, Clone, Copy)]
pub struct PortraitRef<'a> {
    pub w: u16,
    pub h: u16,
    pub rgba: &'a [u8],
}

/// Everything the card screen draws from.
#[derive(Clone, Copy)]
pub struct CardScreenView<'a> {
    pub screen: &'a CardScreen,
    pub book: &'a CardBook,
    pub progress: &'a CardProgress,
    /// The party purse.
    pub gold: i32,
    /// The portrait of a retail monster id, when the host has one.
    pub portrait: &'a dyn Fn(u16) -> Option<PortraitRef<'a>>,
}

/// The screen's flat prims, in stage pixels, in draw order.
pub fn card_screen_prims(v: &CardScreenView<'_>) -> Vec<ScreenPrim> {
    compose(v).prims
}

/// The screen's labels as text draws, in stage pixels.
pub fn card_screen_texts(font: &legaia_font::Font, v: &CardScreenView<'_>) -> Vec<TextDraw> {
    labels_to_draws(font, &compose(v).labels)
}

/// The labels as plain strings, in draw order.
pub fn card_screen_strings(v: &CardScreenView<'_>) -> Vec<String> {
    compose(v).labels.into_iter().map(|l| l.text).collect()
}

/// The card toasts ("Got card: X") as text draws in stage pixels: centred
/// near the top of the stage over play (`top`), or stacked above the
/// footer while an expansion screen owns the frame.
pub fn card_toast_texts(font: &legaia_font::Font, lines: &[&str], top: bool) -> Vec<TextDraw> {
    labels_to_draws(font, &toast_canvas(lines, top).labels)
}

fn toast_canvas(lines: &[&str], top: bool) -> Canvas {
    let mut c = Canvas::default();
    for (i, l) in lines.iter().enumerate() {
        let y = if top {
            24 + i as i32 * LINE
        } else {
            STAGE_H - 46 - i as i32 * LINE
        };
        let pen = if l.starts_with("Lost") {
            pen::RED
        } else {
            pen::GOLD
        };
        c.text_centre(*l, STAGE_W / 2, y, pen);
    }
    c
}

/// Binder slot size and pitch.
const SLOT_W: i32 = 72;
const SLOT_H: i32 = 88;
const SLOT_PITCH_X: i32 = 76;
const SLOT_PITCH_Y: i32 = 92;
const GRID_X: i32 = 8;
const GRID_Y: i32 = 22;
/// The portrait box inside a binder slot.
const ART_W: i32 = 64;
const ART_H: i32 = 48;

const CARD_BACK: [u8; 4] = [40, 64, 140, 255];
const CARD_BACK_INNER: [u8; 4] = [70, 100, 180, 255];
const SLOT_EMPTY: [u8; 4] = [20, 24, 34, 255];
const ART_BG: [u8; 4] = [12, 14, 22, 255];

fn compose(v: &CardScreenView<'_>) -> Canvas {
    let mut c = Canvas::default();
    c.rect(0, 0, STAGE_W, STAGE_H, ink::BACKDROP);
    let title = match &v.screen.dealer {
        Some(d) if !d.name.is_empty() => d.name.clone(),
        Some(_) => "Card Dealer".to_string(),
        None => "Cards".to_string(),
    };
    let (owned, total) = v.book.completion(&v.progress.collection);
    let pct = (owned * 100).checked_div(total).unwrap_or(0);
    header(
        &mut c,
        &title,
        &format!("{} G   {owned}/{total} ({pct}%)", v.gold),
    );
    match &v.screen.stage {
        CardStage::Menu { cursor } => menu(&mut c, v, *cursor),
        CardStage::Binder {
            cursor,
            sort,
            filter,
            detail,
        } => {
            let entries = binder_entries(v.book, v.progress, *sort, *filter);
            if *detail && let Some(e) = entries.get(*cursor) {
                detail_view(&mut c, v, e, *cursor, entries.len());
            } else {
                binder(&mut c, v, &entries, *cursor, sort.label(), filter.label());
            }
        }
        CardStage::Boosters { cursor } => boosters(&mut c, v, *cursor),
        CardStage::Reveal {
            booster,
            cards,
            fresh,
            shown,
        } => reveal(&mut c, v, booster, cards, fresh, *shown),
        CardStage::Sell { cursor } => sell(&mut c, v, *cursor),
        CardStage::Deck { cursor, deck } => deck_builder(&mut c, v, *cursor, deck),
    }
    if let Some(m) = &v.screen.message {
        c.rect(0, STAGE_H - 30, STAGE_W, 15, ink::PANEL);
        c.text_centre(m.clone(), STAGE_W / 2, STAGE_H - 30, pen::GOLD);
    }
    c
}

/// A list of rows like the mode picker's; `rows` are `(left, right)`.
fn list(c: &mut Canvas, x: i32, w: i32, top: i32, rows: &[(String, String)], cursor: usize) {
    const VISIBLE: usize = 10;
    let first = cursor.saturating_sub(VISIBLE - 1);
    let shown = rows.len().min(VISIBLE);
    c.panel(
        x,
        top,
        w,
        shown.max(1) as i32 * 16 + 6,
        ink::PANEL,
        ink::PANEL_EDGE,
    );
    if rows.is_empty() {
        c.text_centre("Nothing here.", x + w / 2, top + 4, pen::DIM);
    }
    for (row, (i, (l, r))) in rows
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE)
        .enumerate()
    {
        let y = top + 3 + row as i32 * 16;
        let on = i == cursor;
        if on {
            c.rect(x + 2, y, w - 4, 16, ink::SELECT);
            c.rect(x + 2, y, 3, 16, ink::HIGHLIGHT);
        }
        c.text(
            l.clone(),
            x + 10,
            y + 1,
            if on { pen::WHITE } else { pen::DIM },
        );
        c.text_right(
            r.clone(),
            x + w - 8,
            y + 1,
            if on { pen::GOLD } else { pen::DIM },
        );
    }
}

fn menu(c: &mut Canvas, v: &CardScreenView<'_>, cursor: usize) {
    let p = v.progress;
    let rows: Vec<(String, String)> = v
        .screen
        .menu_rows(v.book)
        .into_iter()
        .map(|r| match r {
            MenuRow::Binder => (
                "Binder".to_string(),
                match p.unseen() {
                    0 => format!("{} cards", p.collection.total()),
                    n => format!("{n} new"),
                },
            ),
            MenuRow::Boosters => (
                "Open boosters".to_string(),
                format!("{} unopened", p.total_boosters()),
            ),
            MenuRow::Sell => ("Sell cards".to_string(), String::new()),
            MenuRow::Deck => (
                "Duel deck".to_string(),
                if p.duel_deck(v.book).is_some() {
                    format!("{DECK_SIZE} cards")
                } else {
                    "need 5 cards".to_string()
                },
            ),
            MenuRow::Buy(id) => (
                format!("Buy {}", booster_name(v.book, &id)),
                v.book
                    .booster(&id)
                    .and_then(|b| b.price)
                    .map_or(String::new(), |g| format!("{g} G")),
            ),
        })
        .collect();
    list(c, 40, 240, 30, &rows, cursor);
    footer(c, "Up/Down: choose   Cross: select   Circle: back");
}

/// Draw `p` into the box `(x, y, w, h)`, scaled nearest to fit with its
/// aspect kept and centred, as runs of equal 15-bit colour along each row;
/// transparent pixels are skipped.
pub(crate) fn portrait(c: &mut Canvas, p: PortraitRef<'_>, x: i32, y: i32, w: i32, h: i32) {
    let (pw, ph) = (i32::from(p.w), i32::from(p.h));
    if pw == 0 || ph == 0 || w <= 0 || h <= 0 || p.rgba.len() < (pw * ph * 4) as usize {
        return;
    }
    // The largest box of the image's aspect inside `w x h`.
    let (dw, dh) = if w * ph <= h * pw {
        (w, (w * ph / pw).max(1))
    } else {
        ((h * pw / ph).max(1), h)
    };
    let (ox, oy) = (x + (w - dw) / 2, y + (h - dh) / 2);
    let at = |dx: i32, dy: i32| {
        let (sx, sy) = (dx * pw / dw, dy * ph / dh);
        let i = ((sy * pw + sx) * 4) as usize;
        let px = &p.rgba[i..i + 4];
        (px[3] >= 128).then_some([px[0] & 0xF8, px[1] & 0xF8, px[2] & 0xF8, 255])
    };
    for dy in 0..dh {
        let mut dx = 0;
        while dx < dw {
            let Some(col) = at(dx, dy) else {
                dx += 1;
                continue;
            };
            let start = dx;
            while dx < dw && at(dx, dy) == Some(col) {
                dx += 1;
            }
            c.rect(ox + start, oy + dy, dx - start, 1, col);
        }
    }
}

/// The card's monster id, when its art is a monster.
fn art_monster(card: &CardDef) -> Option<u16> {
    match card.art {
        Some(DiscRef::Monster { id }) => Some(id),
        _ => None,
    }
}

/// The art box of `card`: its portrait, else its element colour.
fn art(c: &mut Canvas, v: &CardScreenView<'_>, card: &CardDef, x: i32, y: i32, w: i32, h: i32) {
    c.rect(x, y, w, h, ART_BG);
    match art_monster(card).and_then(|id| (v.portrait)(id)) {
        Some(p) => portrait(c, p, x, y, w, h),
        None => {
            let e = element_color(card.element);
            c.rect(
                x + w / 2 - 8,
                y + h / 2 - 8,
                16,
                16,
                [e[0] / 2, e[1] / 2, e[2] / 2, 255],
            );
        }
    }
    c.outline(x, y, w, h, rarity_color(card.rarity));
}

fn rarity_label(r: Rarity) -> &'static str {
    match r {
        Rarity::Common => "Common",
        Rarity::Uncommon => "Uncommon",
        Rarity::Rare => "Rare",
        Rarity::Legendary => "Legendary",
    }
}

fn short(name: &str, n: usize) -> String {
    name.chars().take(n).collect()
}

/// A binder slot: a card face for an owned card, a blank numbered slot for
/// a missing one.
#[allow(clippy::too_many_arguments)]
fn slot(
    c: &mut Canvas,
    v: &CardScreenView<'_>,
    e: &BinderEntry,
    number: usize,
    x: i32,
    y: i32,
    lit: bool,
) {
    let Some(card) = v.book.card(&e.id) else {
        return;
    };
    let edge = if lit {
        ink::HIGHLIGHT
    } else {
        [16, 16, 24, 255]
    };
    if e.count == 0 {
        c.panel(x, y, SLOT_W, SLOT_H, SLOT_EMPTY, edge);
        c.text_centre(format!("#{number:02}"), x + SLOT_W / 2, y + 30, pen::DIM);
        c.text_centre("?", x + SLOT_W / 2, y + 48, pen::DIM);
        return;
    }
    c.panel(x, y, SLOT_W, SLOT_H, ink::PANEL, edge);
    if lit {
        c.outline(x + 1, y + 1, SLOT_W - 2, SLOT_H - 2, ink::HIGHLIGHT);
    }
    art(c, v, card, x + 4, y + 4, ART_W, ART_H);
    c.rect(x + 4, y + 4 + ART_H, ART_W, 2, element_color(card.element));
    c.text(short(&card.name, 9), x + 4, y + 55, pen::WHITE);
    let [t, r, b, l] = card.sides.map(side_label);
    c.text(format!("{t}-{r}-{b}-{l}"), x + 4, y + 70, pen::GOLD);
    c.text_right(format!("x{}", e.count), x + SLOT_W - 4, y + 70, pen::WHITE);
    if e.fresh {
        c.rect(x + SLOT_W - 26, y + 5, 22, 12, [190, 40, 40, 255]);
        c.text_centre("NEW", x + SLOT_W - 15, y + 4, pen::WHITE);
    }
}

fn binder(
    c: &mut Canvas,
    v: &CardScreenView<'_>,
    entries: &[BinderEntry],
    cursor: usize,
    sort: &str,
    filter: &str,
) {
    let page = cursor / BINDER_PAGE;
    let pages = entries.len().div_ceil(BINDER_PAGE).max(1);
    if entries.is_empty() {
        c.text_centre("No cards match.", STAGE_W / 2, 100, pen::DIM);
    }
    for (i, e) in entries
        .iter()
        .enumerate()
        .skip(page * BINDER_PAGE)
        .take(BINDER_PAGE)
    {
        let k = (i % BINDER_PAGE) as i32;
        let (col, row) = (k % BINDER_COLUMNS as i32, k / BINDER_COLUMNS as i32);
        // The book number is the card's place in the set.
        let number = v
            .book
            .section()
            .cards
            .iter()
            .position(|d| d.id == e.id)
            .map_or(0, |n| n + 1);
        slot(
            c,
            v,
            e,
            number,
            GRID_X + col * SLOT_PITCH_X,
            GRID_Y + row * SLOT_PITCH_Y,
            i == cursor,
        );
    }
    c.text(
        format!("{filter} / {sort}"),
        8,
        GRID_Y + 2 * SLOT_PITCH_Y,
        pen::DIM,
    );
    c.text_right(
        format!("Page {}/{pages}", page + 1),
        STAGE_W - 8,
        GRID_Y + 2 * SLOT_PITCH_Y,
        pen::DIM,
    );
    footer(c, "Cross: detail  L1/R1: page  Square: sort  Tri: filter");
}

/// Split `text` into lines of at most `n` characters at spaces.
fn wrap(text: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > n {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

fn detail_view(c: &mut Canvas, v: &CardScreenView<'_>, e: &BinderEntry, at: usize, n: usize) {
    let Some(card) = v.book.card(&e.id) else {
        return;
    };
    let (x, y) = (10, 24);
    c.panel(x, y, 140, 184, ink::PANEL, rarity_color(card.rarity));
    if e.count == 0 {
        c.rect(x + 6, y + 6, 128, 96, ART_BG);
        c.text_centre("?", x + 70, y + 46, pen::DIM);
        c.text_centre("Not yet found", x + 70, y + 110, pen::DIM);
    } else {
        art(c, v, card, x + 6, y + 6, 128, 96);
        c.rect(x + 6, y + 102, 128, 3, element_color(card.element));
        // The four sides on a cross under the art.
        let [t, r, b, l] = card.sides.map(side_label);
        let cx = x + 70;
        c.text_centre(t, cx, y + 110, pen::WHITE);
        c.text(l, cx - 30, y + 126, pen::WHITE);
        c.text_right(r, cx + 30, y + 126, pen::WHITE);
        c.text_centre(b, cx, y + 142, pen::WHITE);
    }
    let rx = 160;
    c.panel(rx, y, 150, 184, ink::PANEL, ink::PANEL_EDGE);
    let name = if e.count == 0 {
        "???".to_string()
    } else {
        card.name.clone()
    };
    c.text(name, rx + 6, y + 4, pen::GOLD);
    c.text(rarity_label(card.rarity), rx + 6, y + 4 + LINE, pen::WHITE);
    let element = card
        .element
        .map_or("No element".to_string(), |e| format!("{e:?}"));
    c.text(element, rx + 6, y + 4 + 2 * LINE, pen::DIM);
    c.text(
        format!("Owned: {}", e.count),
        rx + 6,
        y + 4 + 3 * LINE,
        pen::WHITE,
    );
    if let Ok(g) = v.book.sell_value(&e.id) {
        c.text(
            format!("Sells for {g} G"),
            rx + 6,
            y + 4 + 4 * LINE,
            pen::DIM,
        );
    }
    if e.count > 0 {
        for (i, line) in wrap(&card.flavor, 24).iter().take(5).enumerate() {
            c.text(
                line.clone(),
                rx + 6,
                y + 8 + (6 + i as i32) * LINE,
                pen::WHITE,
            );
        }
    }
    if e.fresh {
        c.text_right("NEW", rx + 144, y + 4, [0.95, 0.35, 0.3, 1.0]);
    }
    c.text_right(format!("{}/{n}", at + 1), STAGE_W - 8, y + 188, pen::DIM);
    footer(c, "Left/Right: browse   Circle: back");
}

fn boosters(c: &mut Canvas, v: &CardScreenView<'_>, cursor: usize) {
    let ids = CardScreen::booster_rows(v.book, v.progress);
    let rows: Vec<(String, String)> = ids
        .iter()
        .map(|id| {
            (
                booster_name(v.book, id),
                format!("x{}", v.progress.booster_count(id)),
            )
        })
        .collect();
    list(c, 20, 180, 30, &rows, cursor);
    if let Some(set) = ids.get(cursor).and_then(|id| v.book.booster(id)) {
        let x = 208;
        c.panel(x, 30, 104, 70, ink::PANEL, ink::PANEL_EDGE);
        c.text(
            format!("{} cards", set.cards_per_pack),
            x + 6,
            34,
            pen::WHITE,
        );
        if let Some(g) = set.guaranteed {
            c.text(
                format!("1 {}+", rarity_label(g)),
                x + 6,
                34 + LINE,
                pen::GOLD,
            );
        }
        let pool = if set.pool.is_empty() {
            v.book.section().cards.len()
        } else {
            set.pool.len()
        };
        c.text(format!("from {pool}"), x + 6, 34 + 2 * LINE, pen::DIM);
    }
    footer(c, "Cross: open   Circle: back");
}

/// A face-down card.
fn card_back(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
    c.panel(x, y, w, h, CARD_BACK, [16, 16, 24, 255]);
    c.outline(x + 3, y + 3, w - 6, h - 6, CARD_BACK_INNER);
    c.outline(x + w / 2 - 6, y + h / 2 - 6, 12, 12, CARD_BACK_INNER);
}

fn reveal(
    c: &mut Canvas,
    v: &CardScreenView<'_>,
    booster: &str,
    cards: &[String],
    fresh: &[bool],
    shown: usize,
) {
    c.text_centre(booster_name(v.book, booster), STAGE_W / 2, 22, pen::GOLD);
    let per_row = cards.len().clamp(1, 5) as i32;
    let (w, h) = (58, 88);
    let gap = 4;
    let row_w = per_row * w + (per_row - 1) * gap;
    for (i, id) in cards.iter().enumerate() {
        let (col, row) = (i as i32 % per_row, i as i32 / per_row);
        let x = (STAGE_W - row_w) / 2 + col * (w + gap);
        let y = 44 + row * (h + 6);
        if i >= shown {
            card_back(c, x, y, w, h);
            continue;
        }
        let Some(card) = v.book.card(id) else {
            continue;
        };
        c.panel(x, y, w, h, ink::PANEL, rarity_color(card.rarity));
        art(c, v, card, x + 3, y + 3, w - 6, 44);
        c.text(short(&card.name, 7), x + 3, y + 50, pen::WHITE);
        c.text(rarity_label(card.rarity), x + 3, y + 64, pen::DIM);
        if fresh.get(i).copied().unwrap_or(false) {
            c.rect(x + w - 25, y + 4, 22, 12, [190, 40, 40, 255]);
            c.text_centre("NEW", x + w - 14, y + 3, pen::WHITE);
        }
    }
    let hint = if shown < cards.len() {
        "Cross: turn a card   Triangle: turn all"
    } else {
        "Cross: done"
    };
    footer(c, hint);
}

fn sell(c: &mut Canvas, v: &CardScreenView<'_>, cursor: usize) {
    let dealer = v.screen.trading_dealer(v.book);
    let ids = CardScreen::owned_rows(v.progress);
    let rows: Vec<(String, String)> = ids
        .iter()
        .map(|id| {
            let price = CardProgress::sell_price(v.book, &dealer, id).unwrap_or(0);
            (
                format!(
                    "{} x{}",
                    short(&card_name(v.book, id), 14),
                    v.progress.collection.count(id)
                ),
                format!("{price} G"),
            )
        })
        .collect();
    list(c, 10, 196, 30, &rows, cursor);
    if let Some(card) = ids.get(cursor).and_then(|id| v.book.card(id)) {
        let x = 214;
        c.panel(x, 30, 98, 96, ink::PANEL, rarity_color(card.rarity));
        art(c, v, card, x + 4, 34, 90, 64);
        c.text(rarity_label(card.rarity), x + 4, 102, pen::DIM);
    }
    footer(c, "Cross: sell one   Circle: back");
}

fn deck_builder(c: &mut Canvas, v: &CardScreenView<'_>, cursor: usize, deck: &[String]) {
    let ids = CardScreen::owned_rows(v.progress);
    let rows: Vec<(String, String)> = ids
        .iter()
        .map(|id| {
            let used = deck.iter().filter(|d| *d == id).count();
            (
                short(&card_name(v.book, id), 14),
                format!("{used}/{}", v.progress.collection.count(id)),
            )
        })
        .collect();
    list(c, 10, 176, 30, &rows, cursor);
    let x = 194;
    c.panel(
        x,
        30,
        118,
        16 * DECK_SIZE as i32 + 22,
        ink::PANEL,
        ink::PANEL_EDGE,
    );
    c.text("Deck", x + 6, 32, pen::GOLD);
    for i in 0..DECK_SIZE {
        let y = 48 + i as i32 * 16;
        match deck.get(i).and_then(|id| v.book.card(id)) {
            Some(card) => {
                c.rect(x + 4, y + 2, 3, 12, element_color(card.element));
                c.text(short(&card.name, 12), x + 10, y, pen::WHITE);
            }
            None => c.text("-", x + 10, y, pen::DIM),
        }
    }
    footer(c, "Cross: add  Square: remove  Tri: best  Circle: keep");
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_modes::cards::screen::{BinderFilter, BinderSort};
    use legaia_expansion_format::cards::{BoosterSet, CardDealer, CardSection, RarityTable};

    fn book() -> CardBook {
        let cards = (0..10)
            .map(|i| CardDef {
                id: format!("c{i}"),
                name: format!("Card {i}"),
                flavor: "A long line of flavour text that wraps over lines.".into(),
                rarity: if i == 9 {
                    Rarity::Legendary
                } else {
                    Rarity::Common
                },
                sides: [1, 2, 3, 10],
                art: Some(DiscRef::Monster { id: 5 }),
                ..Default::default()
            })
            .collect();
        CardBook::new(CardSection {
            cards,
            boosters: vec![BoosterSet {
                id: "pk".into(),
                name: "Pack".into(),
                cards_per_pack: 5,
                weights: RarityTable {
                    common: 1,
                    ..Default::default()
                },
                price: Some(300),
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    /// A 4x2 portrait: a red pair, a transparent pixel, a blue one; then a
    /// whole green row.
    fn image() -> Vec<u8> {
        let mut v = Vec::new();
        for px in [
            [255, 0, 0, 255],
            [255, 0, 0, 255],
            [0, 0, 0, 0],
            [0, 0, 255, 255],
        ] {
            v.extend(px);
        }
        for _ in 0..4 {
            v.extend([0, 255, 0, 255]);
        }
        v
    }

    fn strings(screen: &CardScreen, progress: &CardProgress, with_art: bool) -> Vec<String> {
        let b = book();
        let img = image();
        let portrait = |id: u16| {
            (with_art && id == 5).then_some(PortraitRef {
                w: 4,
                h: 2,
                rgba: &img,
            })
        };
        let v = CardScreenView {
            screen,
            book: &b,
            progress,
            gold: 1234,
            portrait: &portrait,
        };
        card_screen_strings(&v)
    }

    fn prims(screen: &CardScreen, progress: &CardProgress, with_art: bool) -> usize {
        let b = book();
        let img = image();
        let portrait = |id: u16| {
            (with_art && id == 5).then_some(PortraitRef {
                w: 4,
                h: 2,
                rgba: &img,
            })
        };
        card_screen_prims(&CardScreenView {
            screen,
            book: &b,
            progress,
            gold: 0,
            portrait: &portrait,
        })
        .len()
    }

    #[test]
    fn a_portrait_draws_as_colour_runs_skipping_transparency() {
        let img = image();
        let mut c = Canvas::default();
        let p = PortraitRef {
            w: 4,
            h: 2,
            rgba: &img,
        };
        portrait(&mut c, p, 0, 0, 4, 2);
        // Row 0: the red run and the blue pixel; row 1: one green run.
        assert_eq!(c.prims.len(), 3);
        let mut big = Canvas::default();
        portrait(&mut big, p, 0, 0, 8, 8);
        // Fit keeps the 2:1 aspect: 8x4, one rect per destination row run.
        assert_eq!(big.prims.len(), 2 * 2 + 2);
    }

    #[test]
    fn the_menu_shows_gold_completion_and_counts() {
        let mut p = CardProgress::default();
        p.collection.add("c1", 2);
        p.boosters.insert("pk".into(), 3);
        let s = strings(&CardScreen::menu(), &p, false);
        assert_eq!(s[0], "Cards");
        assert_eq!(s[1], "1234 G   1/10 (10%)");
        assert!(s.contains(&"1 new".to_string()));
        assert!(s.contains(&"3 unopened".to_string()));
        assert!(s.contains(&"need 5 cards".to_string()));
        let dealer = CardScreen::at_dealer(CardDealer {
            name: "Stall".into(),
            ..Default::default()
        });
        let s = strings(&dealer, &p, false);
        assert_eq!(s[0], "Stall");
        assert!(s.contains(&"Buy Pack".to_string()) && s.contains(&"300 G".to_string()));
    }

    #[test]
    fn the_binder_page_shows_owned_faces_and_numbered_blanks() {
        let mut p = CardProgress::default();
        p.collection.add("c0", 3);
        let s = strings(&CardScreen::binder(), &p, true);
        assert!(s.contains(&"Card 0".to_string()));
        assert!(s.contains(&"1-2-3-A".to_string()), "10 prints as A");
        assert!(s.contains(&"x3".to_string()) && s.contains(&"NEW".to_string()));
        assert!(s.contains(&"#02".to_string()) && s.contains(&"?".to_string()));
        assert!(
            !s.contains(&"Card 1".to_string()),
            "a missing card shows no name"
        );
        assert!(s.contains(&"Page 1/2".to_string()));
        assert!(s.contains(&"All cards / Set order".to_string()));
        // The portrait adds its runs to the owned slot.
        assert!(prims(&CardScreen::binder(), &p, true) > prims(&CardScreen::binder(), &p, false));
    }

    #[test]
    fn the_detail_view_wraps_the_flavour_and_hides_a_missing_name() {
        let mut p = CardProgress::default();
        p.collection.add("c0", 1);
        let mut screen = CardScreen::binder();
        screen.stage = CardStage::Binder {
            cursor: 0,
            sort: BinderSort::Book,
            filter: BinderFilter::All,
            detail: true,
        };
        let s = strings(&screen, &p, false);
        assert!(s.contains(&"Card 0".to_string()));
        assert!(s.contains(&"Owned: 1".to_string()));
        assert!(s.iter().any(|l| l.starts_with("A long line")));
        assert!(wrap(&"word ".repeat(30), 24).iter().all(|l| l.len() <= 24));
        screen.stage = CardStage::Binder {
            cursor: 1,
            sort: BinderSort::Book,
            filter: BinderFilter::All,
            detail: true,
        };
        let s = strings(&screen, &p, false);
        assert!(s.contains(&"???".to_string()) && !s.contains(&"Card 1".to_string()));
    }

    #[test]
    fn toasts_stack_from_the_top_over_play_and_above_the_footer_in_a_screen() {
        let c = toast_canvas(&["Got card: A", "Lost card: B"], true);
        assert_eq!((c.labels[0].y, c.labels[1].y), (24, 24 + LINE));
        assert_eq!(c.labels[1].color, pen::RED);
        let c = toast_canvas(&["Got card: A"], false);
        assert_eq!(c.labels[0].y, STAGE_H - 46);
    }

    #[test]
    fn a_reveal_turns_cards_one_at_a_time() {
        let p = CardProgress::default();
        let mut screen = CardScreen::menu();
        let reveal = |shown| CardStage::Reveal {
            booster: "pk".into(),
            cards: vec!["c9".into(), "c1".into()],
            fresh: vec![true, false],
            shown,
        };
        screen.stage = reveal(0);
        let s = strings(&screen, &p, false);
        assert!(s.contains(&"Pack".to_string()));
        assert!(!s.contains(&"Card 9".to_string()));
        screen.stage = reveal(1);
        let s = strings(&screen, &p, false);
        assert!(s.contains(&"Card 9".to_string()) && s.contains(&"Legendary".to_string()));
        assert!(s.contains(&"NEW".to_string()) && !s.contains(&"Card 1".to_string()));
        screen.stage = reveal(2);
        assert!(strings(&screen, &p, false).contains(&"Cross: done".to_string()));
    }
}

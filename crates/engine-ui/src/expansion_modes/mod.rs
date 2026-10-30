//! The expansion game modes' screens: the instance picker, each mode's
//! table / board / course view, the quit prompt and the results card.
//!
//! These are the port's own screens, not retail ones, drawn in a flat
//! style: solid panels as flat [`ScreenPrim`]s and every label in the
//! dialog font. Both play hosts draw them from one composition - the prims
//! ride the screen-prim pass under the text layer
//! (`legaia_engine_screens::screen_layers`), the labels the text layer -
//! so a screen cannot differ between the native window and the browser
//! page.
//!
//! Everything is laid out in the 320x240 stage. [`mode_screen_prims`] and
//! [`mode_screen_texts`] run the same composition and keep their half of
//! it; a panel's geometry never depends on a label's measured width, which
//! is what lets the prim half build without a font.
//!
//! The trading-card screens (binder, boosters, dealer, deck builder) are
//! [`binder`]'s, drawn the same way; a Grid Duel card's art box stays a
//! frame. The pack item screen (shops and the pack bag) is [`items`]'s.

mod binder;
mod cards;
mod duel;
mod golf;
mod hunt;
mod items;
pub mod netplay;
pub mod party;
mod poker;
mod race;

use legaia_engine_modes::active::{
    ActiveMode, ModeEntry, ModeKind, ModeView, score_unit, seat_name_for,
};
use legaia_engine_modes::session::Placement;
use legaia_render_kernels::screen_prim::{FlatQuad, ScreenPrim, line_quad};

use crate::TextDraw;

pub use binder::{
    CardScreenView, PortraitRef, card_screen_prims, card_screen_strings, card_screen_texts,
    card_toast_texts,
};
pub use items::{
    ItemCompareView, ItemMemberView, ItemRowView, ItemScreenView, item_screen_prims,
    item_screen_strings, item_screen_texts,
};

/// Stage width.
pub const STAGE_W: i32 = 320;
/// Stage height.
pub const STAGE_H: i32 = 240;
/// Where a duel's reaction-shot banner sits.
const REACTION_BANNER_Y: i32 = 196;
/// One text line's pitch.
pub const LINE: i32 = 14;

/// The ordering-table bucket the first prim links at; each later prim
/// links one bucket nearer, so the list draws in the order it is built.
const OT_BASE: u32 = 0x2000;

/// Colours, RGBA bytes.
pub(crate) mod ink {
    pub const BACKDROP: [u8; 4] = [14, 20, 30, 255];
    pub const BAR: [u8; 4] = [8, 12, 20, 255];
    pub const PANEL: [u8; 4] = [26, 34, 52, 255];
    pub const PANEL_EDGE: [u8; 4] = [104, 124, 158, 255];
    pub const HIGHLIGHT: [u8; 4] = [236, 184, 64, 255];
    pub const SELECT: [u8; 4] = [52, 72, 112, 255];
    pub const FELT: [u8; 4] = [22, 92, 58, 255];
    pub const FELT_EDGE: [u8; 4] = [104, 68, 34, 255];
    pub const SHADE: [u8; 4] = [0, 0, 0, 150];
}

/// Text colours.
pub(crate) mod pen {
    pub const WHITE: [f32; 4] = [0.92, 0.92, 0.92, 1.0];
    pub const DIM: [f32; 4] = [0.58, 0.62, 0.68, 1.0];
    pub const GOLD: [f32; 4] = [0.93, 0.74, 0.26, 1.0];
    pub const RED: [f32; 4] = [0.86, 0.18, 0.16, 1.0];
    pub const INK: [f32; 4] = [0.08, 0.08, 0.10, 1.0];
    pub const GREEN: [f32; 4] = [0.45, 0.85, 0.50, 1.0];
}

/// How a label sits on its pen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    Left,
    Centre,
    Right,
}

/// One label of the composition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Label {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub color: [f32; 4],
    pub align: Align,
}

/// The composition both halves come from.
#[derive(Debug, Default)]
pub(crate) struct Canvas {
    pub prims: Vec<ScreenPrim>,
    pub labels: Vec<Label>,
}

impl Canvas {
    fn next_ot(&self) -> u32 {
        OT_BASE.saturating_sub(self.prims.len() as u32).max(2)
    }

    /// A solid rect. A colour with alpha below 255 blends at half
    /// (`ABR 0`).
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        if w <= 0 || h <= 0 {
            return;
        }
        let p = |x: i32, y: i32| (x.clamp(-1024, 1024) as i16, y.clamp(-1024, 1024) as i16);
        let ot = self.next_ot();
        self.prims.push(ScreenPrim::Flat(FlatQuad {
            xy: [p(x, y), p(x + w, y), p(x, y + h), p(x + w, y + h)],
            color: c,
            gouraud: None,
            semi_transparent: c[3] < 255,
            abr_mode: 0,
            ot_index: ot,
            depth: None,
        }));
    }

    /// A one-pixel outline.
    pub fn outline(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        self.rect(x, y, w, 1, c);
        self.rect(x, y + h - 1, w, 1, c);
        self.rect(x, y + 1, 1, h - 2, c);
        self.rect(x + w - 1, y + 1, 1, h - 2, c);
    }

    /// A filled panel with an outline.
    pub fn panel(&mut self, x: i32, y: i32, w: i32, h: i32, fill: [u8; 4], edge: [u8; 4]) {
        self.rect(x, y, w, h, fill);
        self.outline(x, y, w, h, edge);
    }

    /// A solid quad on four corners in `POLY_F4` order (top-left,
    /// top-right, bottom-left, bottom-right of the unrotated shape).
    pub fn quad(&mut self, xy: [(f32, f32); 4], c: [u8; 4]) {
        let ot = self.next_ot();
        let p = |(x, y): (f32, f32)| (x.round() as i16, y.round() as i16);
        self.prims.push(ScreenPrim::Flat(FlatQuad {
            xy: xy.map(p),
            color: c,
            gouraud: None,
            semi_transparent: c[3] < 255,
            abr_mode: 0,
            ot_index: ot,
            depth: None,
        }));
    }

    /// A one-pixel line.
    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), c: [u8; 4]) {
        let ot = self.next_ot();
        if let Some(q) = line_quad(a, b, c, c[3] < 255, 0, ot) {
            self.prims.push(ScreenPrim::Flat(q));
        }
    }

    /// A small square marker centred on `(x, y)`.
    pub fn dot(&mut self, x: i32, y: i32, r: i32, c: [u8; 4]) {
        self.rect(x - r, y - r, 2 * r + 1, 2 * r + 1, c);
    }

    pub fn text(&mut self, text: impl Into<String>, x: i32, y: i32, color: [f32; 4]) {
        self.label(text, x, y, color, Align::Left);
    }

    pub fn text_centre(&mut self, text: impl Into<String>, x: i32, y: i32, color: [f32; 4]) {
        self.label(text, x, y, color, Align::Centre);
    }

    pub fn text_right(&mut self, text: impl Into<String>, x: i32, y: i32, color: [f32; 4]) {
        self.label(text, x, y, color, Align::Right);
    }

    fn label(&mut self, text: impl Into<String>, x: i32, y: i32, color: [f32; 4], align: Align) {
        self.labels.push(Label {
            text: text.into(),
            x,
            y,
            color,
            align,
        });
    }
}

/// What the mode seat shows this frame.
#[derive(Debug, Clone, Copy)]
pub enum ModeScreen<'a> {
    /// The instance list.
    Picker {
        /// "Rebirth modes" / "Practice modes".
        heading: &'a str,
        entries: &'a [ModeEntry],
        cursor: usize,
        error: Option<&'a str>,
    },
    /// A session in play.
    Playing {
        mode: &'a ActiveMode,
        /// Recent event lines, newest last.
        log: &'a [String],
        quit_prompt: bool,
    },
    /// A golf round or race played in the world on its pack map: the field
    /// draws under it, so this is only the HUD - the hole and scorecard or
    /// the lap and place, the power meter or boost gauge, the stage's banner
    /// - over a clear stage.
    InWorld {
        mode: &'a ActiveMode,
        log: &'a [String],
        quit_prompt: bool,
        /// The stage's banner ("Birdie!", "Lap 2", the countdown).
        banner: Option<&'a str>,
        /// A duel table's reaction shot is up: the HUD steps aside.
        outro: bool,
    },
    /// A finished (or quit) session.
    Results {
        name: &'a str,
        kind: ModeKind,
        rows: &'a [Placement],
        granted: &'a [String],
        completed: bool,
        /// The local player's seat (the highlighted "You" row).
        local_seat: usize,
        /// Over the world (a duel at a table): no backdrop, and while
        /// `holding` only the stage's banner - the reaction shot plays
        /// first.
        over_world: bool,
        holding: bool,
        banner: Option<&'a str>,
    },
}

/// The screen's flat prims, in stage pixels, in draw order.
pub fn mode_screen_prims(screen: &ModeScreen<'_>) -> Vec<ScreenPrim> {
    compose(screen).prims
}

/// The screen's labels as text draws, in stage pixels.
pub fn mode_screen_texts(font: &legaia_font::Font, screen: &ModeScreen<'_>) -> Vec<TextDraw> {
    labels_to_draws(font, &compose(screen).labels)
}

/// The labels of [`mode_screen_texts`] as plain strings, in draw order -
/// for tests and for a host reading the screen as text.
pub fn mode_screen_strings(screen: &ModeScreen<'_>) -> Vec<String> {
    compose(screen).labels.into_iter().map(|l| l.text).collect()
}

fn labels_to_draws(font: &legaia_font::Font, labels: &[Label]) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for l in labels {
        let layout = font.layout_ascii(&l.text);
        let w = layout.advance_x as i32;
        let x = match l.align {
            Align::Left => l.x,
            Align::Centre => l.x - w / 2,
            Align::Right => l.x - w,
        };
        out.extend(crate::text_draws_for(&layout, (x, l.y), l.color));
    }
    out
}

pub(crate) fn compose(screen: &ModeScreen<'_>) -> Canvas {
    let mut c = Canvas::default();
    let over_world = matches!(
        screen,
        ModeScreen::InWorld { .. }
            | ModeScreen::Results {
                over_world: true,
                ..
            }
    );
    if !over_world {
        c.rect(0, 0, STAGE_W, STAGE_H, ink::BACKDROP);
    }
    match *screen {
        ModeScreen::InWorld {
            mode,
            log,
            quit_prompt,
            banner,
            outro,
        } => {
            match mode.view() {
                ModeView::Golf(v) => golf::hud(&mut c, mode, &v, log),
                ModeView::Race(v) => race::hud(&mut c, mode, &v),
                ModeView::Duel(v) => duel::hud(&mut c, mode, &v, outro),
                _ => {}
            }
            if let Some(text) = banner {
                if outro {
                    stage_banner_at(&mut c, text, REACTION_BANNER_Y)
                } else {
                    stage_banner(&mut c, text)
                }
            }
            if quit_prompt {
                modal(
                    &mut c,
                    "Leave this game?",
                    &["Nothing is won by leaving early."],
                    "Cross: leave    Circle: keep playing",
                );
            }
        }
        ModeScreen::Picker {
            heading,
            entries,
            cursor,
            error,
        } => picker(&mut c, heading, entries, cursor, error),
        ModeScreen::Playing {
            mode,
            log,
            quit_prompt,
        } => {
            header(&mut c, &mode.name, mode.kind().label());
            match mode.view() {
                ModeView::Poker(v) => poker::draw(&mut c, &v, mode.local_seat, log),
                ModeView::Duel(v) => duel::draw(&mut c, mode, &v, log),
                ModeView::Golf(v) => golf::draw(&mut c, mode, &v, log),
                ModeView::Race(v) => race::draw(&mut c, mode, &v, log),
                ModeView::PropHunt(v) => hunt::draw(&mut c, mode, &v, log),
            }
            if quit_prompt {
                modal(
                    &mut c,
                    "Leave this game?",
                    &["Nothing is won by leaving early."],
                    "Cross: leave    Circle: keep playing",
                );
            }
        }
        ModeScreen::Results {
            over_world: true,
            holding: true,
            banner,
            ..
        } => {
            if let Some(text) = banner {
                stage_banner_at(&mut c, text, REACTION_BANNER_Y);
            }
        }
        ModeScreen::Results {
            name,
            kind,
            rows,
            granted,
            completed,
            local_seat,
            over_world,
            ..
        } => {
            if over_world {
                // A shade over the table, lighter than the backdrop.
                c.rect(0, 0, STAGE_W, STAGE_H, [6, 8, 14, 140]);
            }
            results(&mut c, name, kind, rows, granted, completed, local_seat)
        }
    }
    c
}

/// The title bar: the instance name and the mode kind.
pub(crate) fn header(c: &mut Canvas, name: &str, kind: &str) {
    c.rect(0, 0, STAGE_W, 16, ink::BAR);
    c.rect(0, 16, STAGE_W, 1, ink::PANEL_EDGE);
    c.text(name, 6, 1, pen::GOLD);
    c.text_right(kind, STAGE_W - 6, 1, pen::DIM);
}

/// A banner across the upper middle of the stage, over the world.
pub(crate) fn stage_banner(c: &mut Canvas, text: &str) {
    stage_banner_at(c, text, 52);
}

/// [`stage_banner`] at row `y`: a duel's reaction shot puts it low, clear
/// of the opponent's face.
pub(crate) fn stage_banner_at(c: &mut Canvas, text: &str, y: i32) {
    let (w, h) = (180, 22);
    let x = (STAGE_W - w) / 2;
    c.panel(x, y, w, h, [10, 14, 24, 200], ink::HIGHLIGHT);
    c.text_centre(text, STAGE_W / 2, y + 4, pen::GOLD);
}

/// The control hint bar along the bottom.
pub(crate) fn footer(c: &mut Canvas, hint: &str) {
    c.rect(0, STAGE_H - 15, STAGE_W, 15, ink::BAR);
    c.text(hint, 6, STAGE_H - 15, pen::DIM);
}

/// The recent-event box.
pub(crate) fn log_box(c: &mut Canvas, x: i32, y: i32, w: i32, lines: usize, log: &[String]) {
    let h = lines as i32 * LINE + 4;
    c.panel(x, y, w, h, ink::PANEL, ink::PANEL_EDGE);
    let start = log.len().saturating_sub(lines);
    for (i, l) in log[start..].iter().enumerate() {
        let newest = start + i + 1 == log.len();
        c.text(
            l.clone(),
            x + 4,
            y + 1 + i as i32 * LINE,
            if newest { pen::WHITE } else { pen::DIM },
        );
    }
}

/// A centred dialog over a shade.
pub(crate) fn modal(c: &mut Canvas, title: &str, body: &[&str], hint: &str) {
    c.rect(0, 0, STAGE_W, STAGE_H, ink::SHADE);
    let h = 34 + body.len() as i32 * LINE + LINE;
    let (w, x) = (260, (STAGE_W - 260) / 2);
    let y = (STAGE_H - h) / 2;
    c.panel(x, y, w, h, ink::PANEL, ink::HIGHLIGHT);
    c.text_centre(title, STAGE_W / 2, y + 4, pen::GOLD);
    for (i, line) in body.iter().enumerate() {
        c.text_centre(*line, STAGE_W / 2, y + 22 + i as i32 * LINE, pen::WHITE);
    }
    c.text_centre(hint, STAGE_W / 2, y + h - LINE - 4, pen::DIM);
}

fn picker(
    c: &mut Canvas,
    heading: &str,
    entries: &[ModeEntry],
    cursor: usize,
    error: Option<&str>,
) {
    header(c, "Game modes", heading);
    let (x, w) = (40, 240);
    let top = 30;
    // Eleven rows fit between the title bar and the error / hint lines.
    const ROWS: usize = 11;
    let first = cursor.saturating_sub(ROWS - 1);
    if entries.is_empty() {
        c.text_centre("No modes to play.", STAGE_W / 2, top + 20, pen::DIM);
    }
    let shown = entries.len().min(ROWS);
    c.panel(
        x,
        top,
        w,
        shown.max(1) as i32 * 16 + 6,
        ink::PANEL,
        ink::PANEL_EDGE,
    );
    for (row, (i, e)) in entries
        .iter()
        .enumerate()
        .skip(first)
        .take(ROWS)
        .enumerate()
    {
        let y = top + 3 + row as i32 * 16;
        let on = i == cursor;
        if on {
            c.rect(x + 2, y, w - 4, 16, ink::SELECT);
            c.rect(x + 2, y, 3, 16, ink::HIGHLIGHT);
        }
        c.text(
            e.name.clone(),
            x + 10,
            y + 1,
            if on { pen::WHITE } else { pen::DIM },
        );
        c.text_right(
            e.kind.label(),
            x + w - 8,
            y + 1,
            if on { pen::GOLD } else { pen::DIM },
        );
    }
    if let Some(err) = error {
        c.text_centre(err, STAGE_W / 2, STAGE_H - 34, pen::RED);
    }
    footer(c, "Up/Down: choose   Cross: play   Circle: back");
}

fn place_label(place: u8) -> String {
    match place {
        1 => "1st".into(),
        2 => "2nd".into(),
        3 => "3rd".into(),
        n => format!("{n}th"),
    }
}

fn results(
    c: &mut Canvas,
    name: &str,
    kind: ModeKind,
    rows: &[Placement],
    granted: &[String],
    completed: bool,
    me: usize,
) {
    header(c, name, kind.label());
    let (x, w) = (40, 240);
    let title = if !completed {
        "Left early"
    } else if rows.iter().any(|p| p.seat == me && p.place == 1) {
        "You win!"
    } else {
        "Game over"
    };
    c.text_centre(title, STAGE_W / 2, 26, pen::GOLD);
    let y0 = 46;
    let h = rows.len().max(1) as i32 * 16 + 6;
    c.panel(x, y0, w, h, ink::PANEL, ink::PANEL_EDGE);
    if rows.is_empty() {
        c.text_centre("No result", STAGE_W / 2, y0 + 4, pen::DIM);
    }
    for (i, p) in rows.iter().enumerate() {
        let y = y0 + 4 + i as i32 * 16;
        let you = p.seat == me;
        if you {
            c.rect(x + 2, y - 1, w - 4, 16, ink::SELECT);
        }
        let ink = if you { pen::WHITE } else { pen::DIM };
        c.text(place_label(p.place), x + 8, y, ink);
        c.text(seat_name_for(p.seat, me), x + 48, y, ink);
        let score = if kind == ModeKind::Race && p.score == i64::MAX {
            "DNF".to_string()
        } else if kind == ModeKind::Race {
            // The finish frame as a race time.
            let s = p.score / 60;
            format!("{}:{:02}.{:02}", s / 60, s % 60, (p.score % 60) * 100 / 60)
        } else {
            format!("{} {}", p.score, score_unit(kind))
        };
        c.text_right(score, x + w - 8, y, ink);
    }
    let gy = y0 + h + 10;
    if granted.is_empty() {
        c.text_centre("Nothing won", STAGE_W / 2, gy, pen::DIM);
    } else {
        c.text_centre("Won", STAGE_W / 2, gy, pen::GOLD);
        for (i, g) in granted.iter().enumerate() {
            let ink = if g.starts_with('-') {
                pen::RED
            } else {
                pen::GREEN
            };
            c.text_centre(g.clone(), STAGE_W / 2, gy + LINE + i as i32 * LINE, ink);
        }
    }
    footer(c, "Cross: continue");
}

/// Stage-space transform for a world-space box: fit `(min, max)` world
/// units into the `rect` with a uniform scale, centred.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fit {
    scale: f32,
    ox: f32,
    oy: f32,
    min: (f32, f32),
}

impl Fit {
    pub fn new(min: (f32, f32), max: (f32, f32), rect: (i32, i32, i32, i32)) -> Self {
        let (w, h) = ((max.0 - min.0).max(1.0), (max.1 - min.1).max(1.0));
        let scale = (rect.2 as f32 / w).min(rect.3 as f32 / h);
        let ox = rect.0 as f32 + (rect.2 as f32 - w * scale) / 2.0;
        let oy = rect.1 as f32 + (rect.3 as f32 - h * scale) / 2.0;
        Self { scale, ox, oy, min }
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// A world point to stage pixels.
    pub fn at(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.ox + (x - self.min.0) * self.scale,
            self.oy + (y - self.min.1) * self.scale,
        )
    }

    pub fn at_i(&self, x: f32, y: f32) -> (i32, i32) {
        let (sx, sy) = self.at(x, y);
        (sx.round() as i32, sy.round() as i32)
    }
}

/// A 20.12 fixed-point value in world units.
pub(crate) fn fx(v: i32) -> f32 {
    v as f32 / legaia_engine_modes::fixed::FX_ONE as f32
}

/// Distinct marker colours per seat.
pub(crate) fn seat_color(seat: usize) -> [u8; 4] {
    const C: [[u8; 4]; 8] = [
        [240, 220, 80, 255],
        [90, 170, 250, 255],
        [240, 110, 100, 255],
        [140, 230, 130, 255],
        [210, 130, 240, 255],
        [250, 170, 70, 255],
        [120, 230, 230, 255],
        [230, 230, 230, 255],
    ];
    C[seat % C.len()]
}

#[cfg(test)]
mod tests;

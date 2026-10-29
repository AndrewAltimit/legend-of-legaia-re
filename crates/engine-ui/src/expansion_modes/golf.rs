//! The golf screen: the hole top-down from its tiles, the balls, the cup,
//! the aim line and power meter, and the scorecard.

use legaia_engine_modes::active::ActiveMode;
use legaia_engine_modes::golf::course::{Surface, TILE};
use legaia_engine_modes::golf::{GolfPhase, GolfView};

use super::{Canvas, Fit, footer, fx, ink, pen, seat_color};

/// The course viewport.
const VIEW: (i32, i32, i32, i32) = (6, 20, 224, 176);
/// The scorecard column.
const CARD_X: i32 = 236;

fn surface_color(s: Surface, height: u8) -> [u8; 4] {
    let base = match s {
        Surface::Fairway => [70, 160, 70],
        Surface::Rough => [40, 110, 50],
        Surface::Sand => [210, 190, 120],
        Surface::Water => [50, 100, 200],
        Surface::Wall => [110, 90, 70],
        Surface::OutOfBounds => return ink::BACKDROP,
    };
    // Higher ground reads lighter.
    let lift = i32::from(height.min(9)) * 6;
    let ch = |v: i32| (v + lift).min(255) as u8;
    [ch(base[0]), ch(base[1]), ch(base[2]), 255]
}

pub(super) fn draw(c: &mut Canvas, mode: &ActiveMode, v: &GolfView, log: &[String]) {
    let Some(course) = mode.golf_course() else {
        return;
    };
    let (w, h) = (course.width as f32, course.height as f32);
    let t = TILE as f32;
    let fit = Fit::new((0.0, 0.0), (w * t, h * t), VIEW);
    c.rect(VIEW.0, VIEW.1, VIEW.2, VIEW.3, [10, 14, 22, 255]);
    for ty in 0..course.height {
        for tx in 0..course.width {
            let (x0, y0) = fit.at_i(tx as f32 * t, ty as f32 * t);
            let (x1, y1) = fit.at_i((tx + 1) as f32 * t, (ty + 1) as f32 * t);
            let centre = legaia_engine_modes::fixed::Vec2::new(
                (tx as i32 * TILE + TILE / 2) * legaia_engine_modes::fixed::FX_ONE,
                (ty as i32 * TILE + TILE / 2) * legaia_engine_modes::fixed::FX_ONE,
            );
            let tile = course.tile_at(centre);
            c.rect(
                x0,
                y0,
                x1 - x0,
                y1 - y0,
                surface_color(tile.surface, tile.height),
            );
            // A slope tile carries its downhill arrow.
            if tile.slope != (0, 0) {
                let (cx, cy) = ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0);
                let r = (x1 - x0) as f32 * 0.3;
                let (dx, dy) = (f32::from(tile.slope.0), f32::from(tile.slope.1));
                let tip = (cx + dx * r, cy + dy * r);
                c.line((cx - dx * r, cy - dy * r), tip, [220, 240, 200, 255]);
                c.dot(
                    tip.0.round() as i32,
                    tip.1.round() as i32,
                    1,
                    [220, 240, 200, 255],
                );
            }
        }
    }
    // The cup and its flag.
    let (cx, cy) = fit.at_i(fx(v.cup.x), fx(v.cup.y));
    let cup_r = ((6.0 * fit.scale()).round() as i32).max(2);
    c.dot(cx, cy, cup_r, [10, 10, 10, 255]);
    c.rect(cx, cy - 14, 1, 14, [230, 230, 230, 255]);
    c.rect(cx + 1, cy - 14, 7, 5, [220, 40, 40, 255]);

    // The aim line for the player to play.
    let (aim_seat, meter) = match v.phase {
        GolfPhase::Aim { seat } => (Some(seat), None),
        GolfPhase::Power { seat, meter, .. } => (Some(seat), Some(meter)),
        _ => (None, None),
    };
    if let Some(seat) = aim_seat
        && let Some(b) = v.balls.get(seat)
    {
        let a = f32::from(b.aim) / 4096.0 * std::f32::consts::TAU;
        let len = 3.0 * t * (0.35 + 0.65 * meter.unwrap_or(100) as f32 / 100.0);
        let from = fit.at(fx(b.pos.x), fx(b.pos.y));
        let to = fit.at(fx(b.pos.x) + a.cos() * len, fx(b.pos.y) + a.sin() * len);
        c.line(from, to, [255, 255, 255, 255]);
        c.dot(to.0.round() as i32, to.1.round() as i32, 1, ink::HIGHLIGHT);
    }

    // The balls: each seat's resting ball, or the one rolling.
    let rolling_seat = match v.phase {
        GolfPhase::Rolling { seat } => Some(seat),
        _ => None,
    };
    // The player's ball last, so a shared tee shows it on top.
    let order = (1..v.balls.len()).chain(std::iter::once(0));
    for (seat, b) in order.filter_map(|s| v.balls.get(s).map(|b| (s, b))) {
        if b.done {
            continue;
        }
        let pos = match (rolling_seat, v.rolling) {
            (Some(s), Some(r)) if s == seat => r.pos,
            _ => b.pos,
        };
        let (bx, by) = fit.at_i(fx(pos.x), fx(pos.y));
        c.dot(bx, by, 2, [10, 10, 10, 255]);
        c.dot(bx, by, 1, seat_color(seat));
    }

    // The power meter under the course.
    let (mx, my, mw) = (VIEW.0 + 46, 200, VIEW.2 - 46);
    c.text("Power", VIEW.0, 196, pen::DIM);
    c.panel(mx, my, mw, 8, [20, 24, 36, 255], ink::PANEL_EDGE);
    if let Some(m) = meter {
        let fill = (mw - 2) * m.min(100) as i32 / 100;
        let col = if m > 85 {
            [230, 80, 60, 255]
        } else {
            ink::HIGHLIGHT
        };
        c.rect(mx + 1, my + 1, fill, 6, col);
    }
    if let Some(last) = log.last() {
        c.text(last.clone(), VIEW.0, 209, pen::DIM);
    }

    scorecard(c, mode, v);
    if let GolfPhase::HoleOver { .. } = v.phase {
        c.panel(VIEW.0 + 40, 90, VIEW.2 - 80, 24, ink::PANEL, ink::HIGHLIGHT);
        c.text_centre(
            format!("Hole {} complete", v.hole + 1),
            VIEW.0 + VIEW.2 / 2,
            95,
            pen::GOLD,
        );
    }
    footer(
        c,
        match v.phase {
            GolfPhase::Power { .. } => "Cross: strike   Circle: re-aim",
            _ => "Left/Right: aim (Square: fine)   Cross: power",
        },
    );
}

fn scorecard(c: &mut Canvas, mode: &ActiveMode, v: &GolfView) {
    let (x, w) = (CARD_X, 320 - CARD_X - 4);
    let holes = v.scorecard.first().map_or(0, Vec::len);
    c.panel(x, 20, w, 176, ink::PANEL, ink::PANEL_EDGE);
    c.text(format!("Hole {}/{holes}", v.hole + 1), x + 4, 21, pen::GOLD);
    c.text(format!("Par {}", v.par), x + 4, 35, pen::DIM);
    c.text("Now", x + 4, 55, pen::DIM);
    let to_play = match v.phase {
        GolfPhase::Aim { seat } | GolfPhase::Power { seat, .. } | GolfPhase::Rolling { seat } => {
            Some(seat)
        }
        _ => None,
    };
    for (seat, b) in v.balls.iter().enumerate() {
        let y = 69 + seat as i32 * 14;
        let ink = if to_play == Some(seat) {
            pen::GOLD
        } else {
            pen::WHITE
        };
        c.dot(x + 6, y + 7, 2, seat_color(seat));
        c.text(mode.seat_name(seat), x + 12, y, ink);
        let s = if b.done {
            format!("{} in", b.strokes)
        } else {
            b.strokes.to_string()
        };
        c.text_right(s, x + w - 4, y, ink);
    }
    let ty = 69 + v.balls.len() as i32 * 14 + 8;
    c.text("Total", x + 4, ty, pen::DIM);
    for (seat, card) in v.scorecard.iter().enumerate() {
        let y = ty + 14 + seat as i32 * 14;
        let total: u32 = card.iter().flatten().map(|&s| u32::from(s)).sum();
        c.text(mode.seat_name(seat), x + 12, y, pen::WHITE);
        c.text_right(total.to_string(), x + w - 4, y, pen::WHITE);
    }
}

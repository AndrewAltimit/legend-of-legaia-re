//! The race screen: a top-down track map with every racer, and the lap /
//! place / boost panel; and the in-world HUD over a race run on its pack
//! map ([`hud`]).

use legaia_engine_modes::active::ActiveMode;
use legaia_engine_modes::racing::{BOOST_METER, RacePhase, RaceView};

use super::{Align, Canvas, Fit, STAGE_W, footer, fx, ink, pen, seat_color};

const VIEW: (i32, i32, i32, i32) = (6, 20, 224, 200);
const PANEL_X: i32 = 236;

pub(super) fn draw(c: &mut Canvas, mode: &ActiveMode, v: &RaceView, log: &[String]) {
    let Some(track) = mode.race_track() else {
        return;
    };
    let hw = fx(track.half_width);
    let pts: Vec<(f32, f32)> = track.points.iter().map(|p| (fx(p.x), fx(p.y))).collect();
    let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
    for &(x, y) in &pts {
        lo = (lo.0.min(x - hw), lo.1.min(y - hw));
        hi = (hi.0.max(x + hw), hi.1.max(y + hw));
    }
    let fit = Fit::new(lo, hi, VIEW);
    c.rect(VIEW.0, VIEW.1, VIEW.2, VIEW.3, [34, 70, 40, 255]);
    let n = pts.len();
    let road = [90, 90, 96, 255];
    let r = (hw * fit.scale()).max(2.0);
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let boost = track.boost_segments.contains(&i);
        let (sa, sb) = (fit.at(a.0, a.1), fit.at(b.0, b.1));
        let (dx, dy) = (sb.0 - sa.0, sb.1 - sa.1);
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let (nx, ny) = (-dy / len * r, dx / len * r);
        let col = if boost { [120, 100, 40, 255] } else { road };
        c.quad(
            [
                (sa.0 + nx, sa.1 + ny),
                (sb.0 + nx, sb.1 + ny),
                (sa.0 - nx, sa.1 - ny),
                (sb.0 - nx, sb.1 - ny),
            ],
            col,
        );
        // Fill the joint so corners stay closed.
        let ri = r.round() as i32;
        c.rect(
            sa.0.round() as i32 - ri,
            sa.1.round() as i32 - ri,
            2 * ri,
            2 * ri,
            road,
        );
    }
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        c.line(fit.at(a.0, a.1), fit.at(b.0, b.1), [150, 150, 150, 255]);
    }
    // The start / finish line across checkpoint 0.
    if n >= 2 {
        let (a, b) = (fit.at(pts[0].0, pts[0].1), fit.at(pts[1].0, pts[1].1));
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let (nx, ny) = (-dy / len * r, dx / len * r);
        c.line(
            (a.0 + nx, a.1 + ny),
            (a.0 - nx, a.1 - ny),
            [255, 255, 255, 255],
        );
    }
    // The racers, the player last so it draws on top.
    let order: Vec<usize> = (1..v.racers.len()).chain(std::iter::once(0)).collect();
    for seat in order {
        let Some(rc) = v.racers.get(seat) else {
            continue;
        };
        let (x, y) = fit.at(fx(rc.pos.x), fx(rc.pos.y));
        let a = f32::from(rc.heading) / 4096.0 * std::f32::consts::TAU;
        c.line(
            (x, y),
            (x + a.cos() * 6.0, y + a.sin() * 6.0),
            [255, 255, 255, 255],
        );
        let (xi, yi) = (x.round() as i32, y.round() as i32);
        c.dot(xi, yi, 3, [10, 10, 10, 255]);
        c.dot(xi, yi, 2, seat_color(seat));
    }

    // The panel.
    let (px, pw) = (PANEL_X, STAGE_W - PANEL_X - 4);
    c.panel(px, 20, pw, 200, ink::PANEL, ink::PANEL_EDGE);
    let mine = mode.local_seat;
    let me = v.racers.get(mine);
    let laps = track.laps.max(1);
    if let Some(me) = me {
        let lap = (me.lap + 1).min(laps);
        c.text(format!("Lap {lap}/{laps}"), px + 4, 21, pen::GOLD);
        let place = v
            .standings
            .iter()
            .position(|&s| s == mine)
            .map_or(0, |p| p + 1);
        c.text(
            format!("Place {place}/{}", v.racers.len()),
            px + 4,
            35,
            pen::WHITE,
        );
        c.text(
            format!("Speed {}", (fx(me.speed) * 10.0) as i32),
            px + 4,
            49,
            pen::DIM,
        );
        c.text("Boost", px + 4, 63, pen::DIM);
        c.panel(px + 4, 78, pw - 8, 6, [20, 24, 36, 255], ink::PANEL_EDGE);
        let fill = ((pw - 10) as u32 * me.boost_meter.min(BOOST_METER) / BOOST_METER) as i32;
        c.rect(px + 5, 79, fill, 4, [90, 200, 250, 255]);
    }
    c.text("Order", px + 4, 92, pen::DIM);
    for (i, &seat) in v.standings.iter().enumerate().take(6) {
        let y = 106 + i as i32 * 14;
        c.dot(px + 7, y + 7, 2, seat_color(seat));
        c.text(
            mode.seat_name(seat),
            px + 13,
            y,
            if seat == mine { pen::GOLD } else { pen::WHITE },
        );
    }
    if let Some(last) = log.last() {
        c.text_centre(last.clone(), VIEW.0 + VIEW.2 / 2, 22, pen::WHITE);
    }
    if let RacePhase::Countdown { frames } = v.phase {
        let n = frames / 60 + 1;
        c.panel(
            VIEW.0 + VIEW.2 / 2 - 20,
            100,
            40,
            22,
            ink::PANEL,
            ink::HIGHLIGHT,
        );
        c.text_centre(n.to_string(), VIEW.0 + VIEW.2 / 2, 104, pen::GOLD);
    }
    footer(c, "Cross: gas  Square: brake  R1: boost");
}

/// The in-world HUD: lap and place at the top left, the boost gauge and
/// speed at the bottom, and a small running order at the right. The track
/// itself is the field under it.
/// Returns the bottom of the HUD's top band (the place plate and the
/// standings), where a banner may start.
pub(super) fn hud(c: &mut Canvas, mode: &ActiveMode, v: &RaceView) -> i32 {
    let Some(track) = mode.race_track() else {
        return 0;
    };
    let plate = [10, 14, 24, 190];
    let laps = track.laps.max(1);
    if let Some(me) = v.racers.get(mode.local_seat) {
        let lap = (me.lap + 1).min(laps);
        let place = v
            .standings
            .iter()
            .position(|&s| s == mode.local_seat)
            .map_or(0, |p| p + 1);
        c.panel(4, 4, 110, 32, plate, ink::PANEL_EDGE);
        c.text_in(
            format!("Lap {lap}/{laps}"),
            8,
            5,
            102,
            pen::GOLD,
            Align::Left,
        );
        c.text_in(
            format!("Place {place}/{}", v.racers.len()),
            8,
            19,
            102,
            pen::WHITE,
            Align::Left,
        );
        let (bx, by, bw) = (60, 210, 200);
        c.panel(bx - 4, by - 14, bw + 8, 24, plate, ink::PANEL_EDGE);
        c.text_in("Boost", bx, by - 14, 60, pen::DIM, Align::Left);
        c.text_in(
            format!("Speed {}", (fx(me.speed) * 10.0) as i32),
            bx + bw,
            by - 14,
            bw - 64,
            pen::DIM,
            Align::Right,
        );
        c.panel(bx, by, bw, 8, [20, 24, 36, 255], ink::PANEL_EDGE);
        let fill = ((bw - 2) as u32 * me.boost_meter.min(BOOST_METER) / BOOST_METER) as i32;
        let col = if me.boost_meter >= BOOST_METER {
            [120, 230, 255, 255]
        } else {
            [60, 140, 200, 255]
        };
        c.rect(bx + 1, by + 1, fill, 6, col);
    }
    let (x, w) = (216, 100);
    let rows = v.standings.len().min(6);
    c.panel(x, 4, w, 6 + rows as i32 * 14, plate, ink::PANEL_EDGE);
    for (i, &seat) in v.standings.iter().enumerate().take(rows) {
        let y = 6 + i as i32 * 14;
        c.dot(x + 6, y + 7, 2, seat_color(seat));
        c.text_in(
            format!("{} {}", i + 1, mode.seat_name(seat)),
            x + 12,
            y,
            w - 16,
            if seat == mode.local_seat {
                pen::GOLD
            } else {
                pen::WHITE
            },
            Align::Left,
        );
    }
    footer(c, "Cross: gas  Square: brake  D-pad: steer  R1: boost");
    (4 + 32).max(4 + 6 + rows as i32 * 14)
}

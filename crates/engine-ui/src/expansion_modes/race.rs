//! The race screen: a top-down track map with every racer and power-up
//! spot, and the lap / place / boost / power-up panel; and the in-world HUD
//! over a race run on its pack map or a kingdom overworld ([`hud`]).

use legaia_engine_modes::active::ActiveMode;
use legaia_engine_modes::racing::{
    BOOST_METER, PowerKind, RacePhase, RaceView, Racer, SHIELD_FRAMES, Track,
};

use super::{Align, Canvas, Fit, STAGE_W, footer, fx, ink, pen, seat_color};

const VIEW: (i32, i32, i32, i32) = (6, 20, 224, 200);
const PANEL_X: i32 = 236;

/// The controls line both race screens show.
const CONTROLS: &str = "Cross: gas  Square: brake  R1: boost  Tri: power";

/// A power-up's colour on the HUD (the in-world gem's).
pub fn power_color(kind: PowerKind) -> [u8; 4] {
    match kind {
        PowerKind::Boost => [80, 200, 255, 255],
        PowerKind::Hop => [120, 236, 110, 255],
        PowerKind::Shield => [255, 210, 70, 255],
        PowerKind::Slow => [200, 110, 255, 255],
    }
}

fn power_pen(kind: PowerKind) -> [f32; 4] {
    let c = power_color(kind);
    [
        f32::from(c[0]) / 255.0,
        f32::from(c[1]) / 255.0,
        f32::from(c[2]) / 255.0,
        1.0,
    ]
}

/// The progress line: the lap on a circuit, the gate on a sprint.
fn progress_line(track: &Track, me: &Racer) -> String {
    if track.closed {
        let laps = track.laps.max(1);
        format!("Lap {}/{laps}", (me.lap + 1).min(laps))
    } else {
        let gates = track.points.len() - 1;
        let passed = if me.finished.is_some() {
            gates
        } else {
            me.next_cp.saturating_sub(1)
        };
        format!("Gate {passed}/{gates}")
    }
}

/// What runs on the racer right now, seconds left: shield, slow, hop.
fn effects_line(me: &Racer) -> Option<String> {
    let mut parts = Vec::new();
    if me.hop_frames > 0 {
        parts.push("Hop!".to_string());
    }
    if me.shield_frames > 0 {
        parts.push(format!("Shield {}s", me.shield_frames.div_ceil(60)));
    }
    if me.slow_frames > 0 {
        parts.push(format!("Slowed {}s", me.slow_frames.div_ceil(60)));
    }
    (!parts.is_empty()).then(|| parts.join("  "))
}

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
    let segs = track.segment_count();
    let road = [90, 90, 96, 255];
    let r = (hw * fit.scale()).max(2.0);
    for i in 0..segs {
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
    for i in 0..segs {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        c.line(fit.at(a.0, a.1), fit.at(b.0, b.1), [150, 150, 150, 255]);
    }
    // The power-up spots lying there.
    for p in v.pickups.iter().filter(|p| p.ready) {
        let (x, y) = fit.at(fx(p.pos.x), fx(p.pos.y));
        let (xi, yi) = (x.round() as i32, y.round() as i32);
        c.dot(xi, yi, 3, [10, 10, 10, 255]);
        c.dot(xi, yi, 2, power_color(p.kind));
    }
    // The start / finish line across checkpoint 0.
    // A sprint's finish line across its last point too.
    let lines: &[(usize, usize)] = if track.closed {
        &[(0, 1)]
    } else {
        &[(0, 1), (n - 1, n - 2)]
    };
    for &(i, j) in lines.iter().filter(|_| n >= 2) {
        let (a, b) = (fit.at(pts[i].0, pts[i].1), fit.at(pts[j].0, pts[j].1));
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
    if let Some(me) = me {
        c.text(progress_line(track, me), px + 4, 21, pen::GOLD);
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
        match me.held {
            Some(k) => c.text_in(k.label(), px + 4, 86, pw - 8, power_pen(k), Align::Left),
            None => c.text_in("No power", px + 4, 86, pw - 8, pen::DIM, Align::Left),
        }
    }
    c.text("Order", px + 4, 104, pen::DIM);
    for (i, &seat) in v.standings.iter().enumerate().take(5) {
        let y = 118 + i as i32 * 14;
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
    footer(c, CONTROLS);
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
    let mut top = 0;
    if let Some(me) = v.racers.get(mode.local_seat) {
        let place = v
            .standings
            .iter()
            .position(|&s| s == mode.local_seat)
            .map_or(0, |p| p + 1);
        let effects = effects_line(me);
        let plate_h = if effects.is_some() { 46 } else { 32 };
        top = 4 + plate_h;
        c.panel(4, 4, 110, plate_h, plate, ink::PANEL_EDGE);
        c.text_in(progress_line(track, me), 8, 5, 102, pen::GOLD, Align::Left);
        if let Some(e) = effects {
            let pen = if me.slow_frames > 0 {
                power_pen(PowerKind::Slow)
            } else if me.shield_frames > 0 {
                power_pen(PowerKind::Shield)
            } else {
                power_pen(PowerKind::Hop)
            };
            c.text_in(e, 8, 33, 102, pen, Align::Left);
        }
        c.text_in(
            format!("Place {place}/{}", v.racers.len()),
            8,
            19,
            102,
            pen::WHITE,
            Align::Left,
        );
        // The power-up slot, left of the boost gauge: the one held (its
        // gem's colour, a pip under it), or an empty slot.
        let (sx, sy, sw) = (4, 196, 48);
        c.panel(sx, sy, sw, 24, plate, ink::PANEL_EDGE);
        match me.held {
            Some(k) => {
                c.text_in(
                    k.label(),
                    sx + sw / 2,
                    sy,
                    sw - 8,
                    power_pen(k),
                    Align::Centre,
                );
                c.rect(sx + 6, sy + 17, sw - 12, 3, power_color(k));
            }
            None => c.text_in("-", sx + sw / 2, sy, sw - 8, pen::DIM, Align::Centre),
        }
        if me.shield_frames > 0 {
            let fill = ((sw - 12) as u32 * me.shield_frames / SHIELD_FRAMES) as i32;
            c.rect(sx + 6, sy + 21, fill, 2, power_color(PowerKind::Shield));
        }
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
    footer(c, CONTROLS);
    top.max(4 + 32).max(4 + 6 + rows as i32 * 14)
}

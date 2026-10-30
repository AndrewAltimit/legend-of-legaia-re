//! The prop hunt screen: a top-down arena with the props, the hunters and
//! (for a hider, or once found) the hiders, plus the round panel. A
//! functional view; prop hunt moves into real scenes later.
//!
//! A hidden hider draws exactly like a decoy of its prop while the player
//! hunts - telling them apart is the game.

use legaia_engine_modes::active::ActiveMode;
use legaia_engine_modes::prop_hunt::{HuntPhase, HuntView, Role};

use super::{Canvas, Fit, STAGE_W, footer, fx, ink, pen, seat_color};

const VIEW: (i32, i32, i32, i32) = (6, 20, 224, 200);
const PANEL_X: i32 = 236;

fn prop_color(prop: usize) -> [u8; 4] {
    const C: [[u8; 4]; 4] = [
        [150, 100, 50, 255],
        [200, 170, 110, 255],
        [210, 200, 160, 255],
        [120, 140, 150, 255],
    ];
    C[prop % C.len()]
}

pub(super) fn draw(c: &mut Canvas, mode: &ActiveMode, v: &HuntView, log: &[String]) {
    let Some(arena) = mode.hunt_arena() else {
        return;
    };
    let radii = mode.hunt_prop_radii();
    let b = arena.bounds;
    let fit = Fit::new((fx(b.x0), fx(b.y0)), (fx(b.x1), fx(b.y1)), VIEW);
    c.rect(VIEW.0, VIEW.1, VIEW.2, VIEW.3, ink::BACKDROP);
    let (x0, y0) = fit.at_i(fx(b.x0), fx(b.y0));
    let (x1, y1) = fit.at_i(fx(b.x1), fx(b.y1));
    c.rect(x0, y0, x1 - x0, y1 - y0, [64, 56, 44, 255]);
    for bl in &arena.blockers {
        let (a0, b0) = fit.at_i(fx(bl.x0), fx(bl.y0));
        let (a1, b1) = fit.at_i(fx(bl.x1), fx(bl.y1));
        c.panel(
            a0,
            b0,
            a1 - a0,
            b1 - b0,
            [40, 34, 28, 255],
            [96, 84, 66, 255],
        );
    }
    let prop_box = |c: &mut Canvas, prop: usize, x: f32, y: f32| {
        let r = radii.get(prop).copied().unwrap_or(12) as f32 * fit.scale();
        let ri = r.round().max(2.0) as i32;
        let (xi, yi) = fit.at_i(x, y);
        c.panel(
            xi - ri,
            yi - ri,
            2 * ri,
            2 * ri,
            prop_color(prop),
            [20, 16, 12, 255],
        );
    };
    for d in &v.decoys {
        prop_box(c, d.prop, fx(d.pos.x), fx(d.pos.y));
    }
    let mine = mode.local_seat;
    let me = v.avatars.get(mine);
    let i_hunt = me.is_some_and(|a| a.role == Role::Hunter);
    for (seat, a) in v.avatars.iter().enumerate() {
        let (x, y) = (fx(a.pos.x), fx(a.pos.y));
        let (xi, yi) = fit.at_i(x, y);
        match a.role {
            Role::Hunter => {
                if a.out {
                    c.dot(xi, yi, 2, [90, 90, 90, 255]);
                } else {
                    c.dot(xi, yi, 4, [10, 10, 10, 255]);
                    c.dot(xi, yi, 3, [220, 50, 50, 255]);
                }
            }
            Role::Hider => {
                let in_hunt = matches!(v.phase, HuntPhase::Hide { .. } | HuntPhase::Hunt { .. });
                if a.out {
                    c.dot(xi, yi, 2, [90, 90, 90, 255]);
                } else if in_hunt {
                    prop_box(c, a.prop, x, y);
                }
                // The player sees its own hider, and every hider while
                // the player is one too; a hunting player sees only props.
                if !a.out && (seat == mine || !i_hunt) {
                    c.dot(xi, yi, 1, seat_color(seat));
                }
            }
        }
        if seat == mine && !a.out {
            let r = 7;
            c.outline(xi - r, yi - r, 2 * r + 1, 2 * r + 1, ink::HIGHLIGHT);
        }
    }

    // The panel.
    let (px, pw) = (PANEL_X, STAGE_W - PANEL_X - 4);
    c.panel(px, 20, pw, 200, ink::PANEL, ink::PANEL_EDGE);
    let (label, timer) = match v.phase {
        HuntPhase::Choose { timer } => ("Choose", timer),
        HuntPhase::Hide { timer } => ("Hide", timer),
        HuntPhase::Hunt { timer } => ("Hunt", timer),
        HuntPhase::RoundOver { timer, .. } => ("Round over", timer),
        HuntPhase::Finished => ("Finished", 0),
    };
    c.text(format!("Round {}", v.round + 1), px + 4, 21, pen::GOLD);
    c.text(label, px + 4, 35, pen::WHITE);
    c.text(format!("{}s", timer / 60), px + pw - 4 - 24, 35, pen::DIM);
    if let Some(me) = me {
        let role = if me.role == Role::Hunter {
            format!("Hunter {}", me.health)
        } else {
            let prop = v.prop_ids.get(me.prop).map_or("?", String::as_str);
            format!("Hider: {prop}")
        };
        c.text(role, px + 4, 49, pen::GOLD);
    }
    c.text("Score", px + 4, 69, pen::DIM);
    for (seat, a) in v.avatars.iter().enumerate().take(8) {
        let y = 83 + seat as i32 * 14;
        let ink = if a.out { pen::DIM } else { pen::WHITE };
        c.dot(px + 7, y + 7, 2, seat_color(seat));
        c.text(mode.seat_name(seat), px + 13, y, ink);
        c.text_right(a.score.to_string(), px + pw - 4, y, ink);
    }
    if let Some(last) = log.last() {
        c.text_centre(last.clone(), VIEW.0 + VIEW.2 / 2, 22, pen::WHITE);
    }
    footer(
        c,
        match v.phase {
            HuntPhase::Choose { .. } => "Left/Right: prop   Cross: choose",
            _ if i_hunt => "D-pad: move   Cross: inspect",
            _ => "D-pad: move   Triangle: taunt",
        },
    );
}

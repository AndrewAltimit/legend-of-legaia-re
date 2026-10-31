//! A poker table as the hosts draw it
//! ([`legaia_engine_expansion::poker_stage`]): the oval table and felt,
//! the cards (faces and backs off the table's card page,
//! `legaia_engine_modes::poker::art`), the chip stacks as stacked discs,
//! the dealer button, the seat-to-act ring, the floating bet amounts in the
//! page's digit glyphs, and the bursts.
//!
//! Like the duel table ([`super::duel`]) every corner carries its scene
//! depth and the draw order is explicit ordering-table bands.

use legaia_engine_expansion::poker_stage::{CHIP_COLOURS, ChipStack, PokerStageFrame};
use legaia_engine_modes::cards::table_art;
use legaia_engine_modes::poker::art;
use legaia_engine_modes::poker::table::OVAL;
use legaia_render_kernels::screen_prim::{CornerDepth, ScreenPrim, ScreenQuad};

use super::duel::{
    BAND_BOARD, BAND_CARDS, BAND_MARKS, BAND_SPARKS, BAND_TABLE, FELT, Table, WOOD, WOOD_DARK,
    box_sides, burst, card, flat, front, rgba, shade,
};
use super::{Projector, Pt};

/// Segments of the oval.
const OVAL_SEGMENTS: usize = 32;
/// Segments of a chip.
const CHIP_SEGMENTS: usize = 10;
/// A chip's radius and thickness, world units.
const CHIP_R: f32 = 5.5;
const CHIP_T: f32 = 1.6;

/// The oval's point at angle `a` (radians, `0` toward the player) scaled by
/// `k`, table-local.
fn oval(t: &PokerStageFrame, a: f32, k: f32) -> (f32, f32) {
    let r = t.table.radius;
    (-a.sin() * r * OVAL * k, -a.cos() * r * k)
}

/// The table: legs, the wooden rail round the oval, the felt lit brightest
/// in the middle, and a gold betting line.
fn table(out: &mut Vec<ScreenPrim>, t: &Table<'_, '_>, s: &PokerStageFrame) {
    let top = s.table.top();
    let r = s.table.radius;
    if s.table.draw_table {
        for (lr, la) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let (cr, ca) = (lr * r * OVAL * 0.6, la * r * 0.55);
            box_sides(
                out,
                t,
                (cr - 10.0, ca - 10.0, cr + 10.0, ca + 10.0),
                top - 12.0,
                s.table.lift,
                WOOD_DARK,
                BAND_TABLE + 4,
            );
        }
    }
    let n = OVAL_SEGMENTS;
    let ang = |i: usize| i as f32 / n as f32 * std::f32::consts::TAU;
    for i in 0..n {
        let (a0, a1) = (ang(i), ang(i + 1));
        // The rail's outer face, where it turns to the camera.
        if s.table.draw_table {
            let (p0, p1) = (oval(s, a0, 1.16), oval(s, a1, 1.16));
            let q = [
                t.at(p1.0, p1.1, top + 3.0),
                t.at(p0.0, p0.1, top + 3.0),
                t.at(p1.0, p1.1, top - 12.0),
                t.at(p0.0, p0.1, top - 12.0),
            ];
            if let [Some(a), Some(b), Some(c), Some(d)] = q
                && front(&[a, b, c, d])
            {
                let hi = rgba(WOOD, 255);
                let lo = rgba(shade(WOOD, 0.5), 255);
                out.push(flat([a, b, c, d], [hi, hi, lo, lo], None, BAND_TABLE + 3));
            }
        }
        // The rail's top: a padded leather ring.
        let ring = [
            oval(s, a0, 1.16),
            oval(s, a1, 1.16),
            oval(s, a0, 1.0),
            oval(s, a1, 1.0),
        ];
        let pts = ring.map(|(x, z)| t.at(x, z, top + 3.0));
        if let [Some(a), Some(b), Some(c), Some(d)] = pts {
            let k = 0.75 + 0.25 * (a0 - 0.6).cos();
            let rail = rgba(shade([88, 40, 30], k), 255);
            let inner = rgba(shade([120, 60, 42], k), 255);
            out.push(flat(
                [a, b, c, d],
                [rail, rail, inner, inner],
                None,
                BAND_TABLE + 2,
            ));
        }
        // The felt: a fan from the middle, bright under the lamp.
        let mid = t.at(0.0, 0.0, top);
        let (f0, f1) = (oval(s, a0, 1.0), oval(s, a1, 1.0));
        if let (Some(m), Some(a), Some(b)) = (mid, t.at(f0.0, f0.1, top), t.at(f1.0, f1.1, top)) {
            let centre = rgba(shade(FELT, 1.35), 255);
            let rim = rgba(shade(FELT, 0.72), 255);
            out.push(flat(
                [m, a, b, b],
                [centre, rim, rim, rim],
                None,
                BAND_BOARD + 2,
            ));
        }
        // The gold betting line.
        let line = [
            oval(s, a0, 0.66),
            oval(s, a1, 0.66),
            oval(s, a0, 0.64),
            oval(s, a1, 0.64),
        ];
        let pts = line.map(|(x, z)| t.at(x, z, top + 0.3));
        if let [Some(a), Some(b), Some(c), Some(d)] = pts {
            let g = rgba([150, 120, 50], 255);
            out.push(flat([a, b, c, d], [g; 4], Some(1), BAND_BOARD + 1));
        }
    }
}

/// A flat disc on the felt (the dealer button, a ring's fill), table-world.
#[allow(clippy::too_many_arguments)]
fn disc(
    out: &mut Vec<ScreenPrim>,
    t: &Table<'_, '_>,
    at: [f32; 2],
    h: f32,
    r: f32,
    c: [u8; 4],
    blend: Option<u8>,
    ot: u32,
) {
    let p = |a: f32, k: f32| {
        t.pr.at([
            at[0] + a.cos() * r * k,
            t.floor - h,
            at[1] + a.sin() * r * k,
        ])
    };
    let Some(m) = p(0.0, 0.0) else {
        return;
    };
    for i in 0..CHIP_SEGMENTS {
        let (a0, a1) = (
            i as f32 / CHIP_SEGMENTS as f32 * std::f32::consts::TAU,
            (i + 1) as f32 / CHIP_SEGMENTS as f32 * std::f32::consts::TAU,
        );
        if let (Some(a), Some(b)) = (p(a0, 1.0), p(a1, 1.0)) {
            out.push(flat([m, a, b, b], [c; 4], blend, ot));
        }
    }
}

/// A chip stack: a side band per chip, alternating with a white stripe,
/// and the top chip's face.
fn stack(out: &mut Vec<ScreenPrim>, t: &Table<'_, '_>, s: &ChipStack) {
    let at = s.at;
    let base = s.height;
    let dist = t.dist([at[0], t.floor - base, at[1]]);
    let ot = BAND_CARDS + (dist * 4.0) as u32 * 4 + 2;
    let main = CHIP_COLOURS[usize::from(s.colour) % CHIP_COLOURS.len()];
    for k in 0..s.chips {
        let (h0, h1) = (
            base + f32::from(k) * CHIP_T,
            base + f32::from(k + 1) * CHIP_T,
        );
        let colour = if k % 3 == 2 { [232, 228, 214] } else { main };
        for i in 0..CHIP_SEGMENTS {
            let (a0, a1) = (
                i as f32 / CHIP_SEGMENTS as f32 * std::f32::consts::TAU,
                (i + 1) as f32 / CHIP_SEGMENTS as f32 * std::f32::consts::TAU,
            );
            let p = |a: f32, h: f32| {
                t.pr.at([
                    at[0] + a.cos() * CHIP_R,
                    t.floor - h,
                    at[1] + a.sin() * CHIP_R,
                ])
            };
            if let (Some(a), Some(b), Some(c), Some(d)) =
                (p(a1, h1), p(a0, h1), p(a1, h0), p(a0, h0))
                && front(&[a, b, c, d])
            {
                let k2 = 0.7 + 0.3 * (a0 + 0.8).cos().max(0.0);
                let col = rgba(shade(colour, k2), 255);
                let dark = rgba(shade(colour, k2 * 0.7), 255);
                out.push(flat([a, b, c, d], [col, col, dark, dark], None, ot));
            }
        }
    }
    let top = base + f32::from(s.chips) * CHIP_T;
    disc(
        out,
        t,
        at,
        top,
        CHIP_R,
        rgba(shade(main, 1.15), 255),
        None,
        ot - 1,
    );
    disc(
        out,
        t,
        at,
        top + 0.05,
        CHIP_R * 0.55,
        rgba([236, 230, 210], 255),
        None,
        ot - 2,
    );
    disc(
        out,
        t,
        at,
        top + 0.1,
        CHIP_R * 0.4,
        rgba(shade(main, 1.0), 255),
        None,
        ot - 3,
    );
}

/// A glyph strip quad (`table_art::tpage`) on four projected corners.
fn glyph(out: &mut Vec<ScreenPrim>, c: [Pt; 4], g: u8, ot: u32) {
    let [u, v, w, h] = art::glyph_rect(g);
    out.push(ScreenPrim::Textured(ScreenQuad {
        xy: c.map(|p| (p.x.round() as i16, p.y.round() as i16)),
        uv: [
            (u, v),
            (u + w - 1, v),
            (u, v + h - 1),
            (u + w - 1, v + h - 1),
        ],
        clut: 0,
        tpage: table_art::tpage(),
        color: 0x80_80_80,
        gouraud: None,
        semi_transparent: false,
        ot_index: ot,
        depth: Some(CornerDepth::new(c.map(|p| p.depth))),
    }));
}

/// A number floating at a world point, facing the screen.
fn amount(out: &mut Vec<ScreenPrim>, t: &Table<'_, '_>, at: [f32; 2], h: f32, value: u32) {
    let p = [at[0], t.floor - h, at[1]];
    let (Some(c), Some(up)) = (t.pr.at(p), t.pr.at([p[0], p[1] - 9.0, p[2]])) else {
        return;
    };
    let gh = ((up.x - c.x).powi(2) + (up.y - c.y).powi(2))
        .sqrt()
        .clamp(6.0, 24.0);
    let gw = gh * f32::from(art::GLYPH_W) / f32::from(art::GLYPH_H);
    let digits: Vec<u8> = value.to_string().bytes().map(|b| b - b'0').collect();
    let total = gw * digits.len() as f32;
    // A dark plate under the digits, so the amount reads as a tag over the
    // stack rather than print on the felt behind it.
    let pad = (gh * 0.25).max(2.0);
    let (l, r) = (c.x - total / 2.0 - pad, c.x + total / 2.0 + pad);
    let (top, bot) = (c.y - gh / 2.0 - pad * 0.6, c.y + gh / 2.0 + pad * 0.6);
    let corner = |x: f32, y: f32| Pt {
        x,
        y,
        depth: c.depth,
    };
    let shade_ = rgba([8, 10, 16], 255);
    out.push(flat(
        [
            corner(l, top),
            corner(r, top),
            corner(l, bot),
            corner(r, bot),
        ],
        [shade_; 4],
        Some(0),
        BAND_SPARKS + 101,
    ));
    for (i, d) in digits.iter().enumerate() {
        let x0 = c.x - total / 2.0 + i as f32 * gw;
        let y0 = c.y - gh / 2.0;
        let q = |x: f32, y: f32| Pt {
            x,
            y,
            depth: c.depth,
        };
        glyph(
            out,
            [
                q(x0, y0),
                q(x0 + gw, y0),
                q(x0, y0 + gh),
                q(x0 + gw, y0 + gh),
            ],
            *d,
            BAND_SPARKS + 100 - i as u32,
        );
    }
}

/// The poker table's pieces through `pr`.
pub(super) fn poker_pieces(pr: &Projector<'_>, s: &PokerStageFrame) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    let c = s.table.centre();
    let floor = pr.world.sample_field_floor_height(c[0] as i32, c[1] as i32) as f32;
    let (across, right) = s.table.axes();
    let t = Table {
        pr,
        floor,
        centre: c,
        right,
        across,
        eye: pr.eye,
    };
    let top = s.table.top();
    table(&mut out, &t, s);
    // The seat to act: a pulsing gold ring on the felt before it.
    if let Some(at) = s.to_act {
        let k = 0.55 + 0.45 * (s.clock as f32 / 9.0).sin().abs();
        disc(
            &mut out,
            &t,
            at,
            top + 0.4,
            16.0,
            rgba(shade([255, 200, 90], k * 0.6), 255),
            Some(1),
            BAND_MARKS,
        );
    }
    // The dealer button.
    if let Some(at) = s.button {
        disc(
            &mut out,
            &t,
            at,
            top + 1.2,
            7.5,
            rgba([244, 240, 228], 255),
            None,
            BAND_MARKS - 2,
        );
        let q = |dr: f32, da: f32| {
            t.pr.at([
                at[0] + right[0] * dr + across[0] * da,
                floor - top - 1.4,
                at[1] + right[1] * dr + across[1] * da,
            ])
        };
        if let (Some(a), Some(b), Some(cc), Some(d)) =
            (q(-3.5, 5.0), q(3.5, 5.0), q(-3.5, -5.0), q(3.5, -5.0))
        {
            glyph(&mut out, [a, b, cc, d], art::GLYPH_D, BAND_MARKS - 3);
        }
    }
    for card_ in &s.cards {
        card(&mut out, &t, (top, s.local, s.clock), card_);
    }
    for st in &s.stacks {
        stack(&mut out, &t, st);
    }
    for b in &s.bursts {
        burst(&mut out, &t, s.local, b);
    }
    for a in &s.amounts {
        amount(&mut out, &t, a.at, a.height, a.value);
    }
    out
}

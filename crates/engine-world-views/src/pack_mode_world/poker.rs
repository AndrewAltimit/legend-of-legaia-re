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
    box_sides, burst, card, chair, flat, front, rgba, shade,
};
use super::{Projector, Pt};

/// Segments of the oval.
const OVAL_SEGMENTS: usize = 32;
/// Segments of a chip.
const CHIP_SEGMENTS: usize = 10;
/// A chip's radius and thickness, world units.
const CHIP_R: f32 = 5.5;
const CHIP_T: f32 = 1.6;
/// A stack of this many chips or more lies as a pile of columns.
const PILE_MIN: u8 = 6;
/// The white of a chip's edge spots and face inlay.
const CHIP_SPOT: [u8; 3] = [236, 232, 218];

/// The oval's point at angle `a` (radians, `0` toward the player) scaled by
/// `k`, table-local.
fn oval(t: &PokerStageFrame, a: f32, k: f32) -> (f32, f32) {
    let r = t.table.radius;
    (-a.sin() * r * OVAL * k, -a.cos() * r * k)
}

/// The rail's outer edge, the wooden racetrack's outer edge, and the
/// apron's foot, as fractions of the oval.
const RAIL_OUT: f32 = 1.2;
const RAIL_MID: f32 = 1.135;
const TRACK_OUT: f32 = 1.07;
/// The rail's heights over the felt: its inner lip, its crown, its outer
/// lip; and how far the apron drops below the felt.
const RAIL_IN_H: f32 = 2.2;
const RAIL_CROWN_H: f32 = 6.0;
const RAIL_OUT_H: f32 = 3.6;
const APRON: f32 = 11.0;
/// The rail's leather and the pedestal's half-width.
const LEATHER: [u8; 3] = [78, 34, 26];
const PEDESTAL: f32 = 13.0;

/// The table: a pedestal on a cross foot, the wooden apron round the oval
/// and its shadowed underside (only with `draw_table`); and always the
/// padded rail with its rounded crown, the polished wooden racetrack inside
/// it, the felt lit brightest under the lamp, and a gold betting line.
fn table(out: &mut Vec<ScreenPrim>, t: &Table<'_, '_>, s: &PokerStageFrame) {
    let top = s.table.top();
    let r = s.table.radius;
    let lift = s.table.lift;
    if s.table.draw_table {
        // The foot: two crossed planks on the floor, then the column.
        let (fw, fd) = (r * OVAL * 0.52, r * 0.42);
        for (r0, a0, r1, a1) in [(-fw, -8.0, fw, 8.0), (-8.0, -fd, 8.0, fd)] {
            box_sides(
                out,
                t,
                (r0, a0, r1, a1),
                lift + 6.0,
                lift,
                WOOD_DARK,
                BAND_TABLE + 6,
            );
        }
        box_sides(
            out,
            t,
            (-PEDESTAL, -PEDESTAL, PEDESTAL, PEDESTAL),
            top - APRON - 2.0,
            lift + 6.0,
            shade(WOOD_DARK, 1.15),
            BAND_TABLE + 5,
        );
    }
    let n = OVAL_SEGMENTS;
    let ang = |i: usize| i as f32 / n as f32 * std::f32::consts::TAU;
    // How a band facing the lamp (behind the viewer's left shoulder)
    // brightens.
    let lamp = |a: f32| 0.72 + 0.28 * (a - 0.6).cos();
    for i in 0..n {
        let (a0, a1) = (ang(i), ang(i + 1));
        let k = lamp((a0 + a1) * 0.5);
        let ring = |k0: f32, h0: f32, k1: f32, h1: f32| {
            let (p0, p1, q0, q1) = (
                oval(s, a0, k0),
                oval(s, a1, k0),
                oval(s, a0, k1),
                oval(s, a1, k1),
            );
            match (
                t.at(p0.0, p0.1, h0),
                t.at(p1.0, p1.1, h0),
                t.at(q0.0, q0.1, h1),
                t.at(q1.0, q1.1, h1),
            ) {
                (Some(a), Some(b), Some(c), Some(d)) => Some([a, b, c, d]),
                _ => None,
            }
        };
        if s.table.draw_table {
            // The apron: the wooden outer face, where it turns to the
            // camera, and the shadowed underside below it.
            if let Some([p0, p1, q0, q1]) = ring(RAIL_OUT, top + RAIL_OUT_H, RAIL_OUT, top - APRON)
            {
                let q = [p1, p0, q1, q0];
                if front(&q) {
                    let hi = rgba(shade(WOOD, k * 1.1), 255);
                    let lo = rgba(shade(WOOD, k * 0.55), 255);
                    out.push(flat(q, [hi, hi, lo, lo], None, BAND_TABLE + 4));
                }
            }
            if let Some([p0, p1, q0, q1]) =
                ring(RAIL_OUT, top - APRON, RAIL_OUT - 0.1, top - APRON - 4.0)
            {
                let q = [p1, p0, q1, q0];
                if front(&q) {
                    let c = rgba(shade(WOOD_DARK, 0.6), 255);
                    out.push(flat(q, [c; 4], None, BAND_TABLE + 4));
                }
            }
        }
        // The rail: an outer slope up to the crown, an inner slope down to
        // the racetrack, so the leather reads rounded.
        if let Some(q) = ring(RAIL_OUT, top + RAIL_OUT_H, RAIL_MID, top + RAIL_CROWN_H) {
            let lo = rgba(shade(LEATHER, k * 0.7), 255);
            let hi = rgba(shade(LEATHER, k * 1.25), 255);
            out.push(flat(q, [lo, lo, hi, hi], None, BAND_TABLE + 2));
        }
        if let Some(q) = ring(RAIL_MID, top + RAIL_CROWN_H, TRACK_OUT, top + RAIL_IN_H) {
            let hi = rgba(shade(LEATHER, k * 1.35), 255);
            let lo = rgba(shade(LEATHER, k * 0.8), 255);
            out.push(flat(q, [hi, hi, lo, lo], None, BAND_TABLE + 2));
        }
        // The racetrack: polished wood between the rail and the felt, with
        // a dark seam at the felt's edge.
        if let Some(q) = ring(TRACK_OUT, top + RAIL_IN_H, 1.0, top + 0.6) {
            let hi = rgba(shade(WOOD, k * 1.3), 255);
            let lo = rgba(shade(WOOD, k * 0.95), 255);
            out.push(flat(q, [hi, hi, lo, lo], None, BAND_TABLE + 1));
        }
        if let Some(q) = ring(1.005, top + 0.65, 0.985, top + 0.1) {
            let c = rgba([20, 14, 10], 255);
            out.push(flat(q, [c; 4], None, BAND_TABLE));
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
        if let Some(q) = ring(0.665, top + 0.3, 0.645, top + 0.3) {
            let g = rgba([150, 120, 50], 255);
            out.push(flat(q, [g; 4], Some(1), BAND_BOARD + 1));
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

/// One column of chips: a side band per chip with the edge spots of a
/// casino chip (white inserts, turned a little per chip so the column does
/// not stripe), and the top chip's face with its inlay.
#[allow(clippy::too_many_arguments)]
fn column(
    out: &mut Vec<ScreenPrim>,
    t: &Table<'_, '_>,
    at: [f32; 2],
    base: f32,
    chips: u8,
    main: [u8; 3],
    ot: u32,
) {
    if chips == 0 {
        return;
    }
    let seg = |i: usize| i as f32 / CHIP_SEGMENTS as f32 * std::f32::consts::TAU;
    for k in 0..chips {
        let (h0, h1) = (
            base + f32::from(k) * CHIP_T,
            base + f32::from(k + 1) * CHIP_T,
        );
        for i in 0..CHIP_SEGMENTS {
            let (a0, a1) = (seg(i), seg(i + 1));
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
                let spot = (i + usize::from(k) * 3) % 5 == 0;
                let colour = if spot { CHIP_SPOT } else { main };
                let k2 = 0.7 + 0.3 * (a0 + 0.8).cos().max(0.0);
                let col = rgba(shade(colour, k2), 255);
                let dark = rgba(shade(colour, k2 * 0.62), 255);
                out.push(flat([a, b, c, d], [col, col, dark, dark], None, ot));
            }
        }
    }
    let top = base + f32::from(chips) * CHIP_T;
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
        CHIP_R * 0.62,
        rgba(CHIP_SPOT, 255),
        None,
        ot - 2,
    );
    disc(
        out,
        t,
        at,
        top + 0.1,
        CHIP_R * 0.48,
        rgba(shade(main, 1.0), 255),
        None,
        ot - 3,
    );
}

/// A chip stack. A small one (or one in flight) is a single column; a
/// larger one is a pile of up to three columns, its top denomination
/// tallest and the two below it beside it - so a seat's chips read as a
/// stack of money rather than a single tube. The split is a pure function
/// of the stack, so every frame and every peer draws the same pile.
fn stack(out: &mut Vec<ScreenPrim>, t: &Table<'_, '_>, s: &ChipStack, felt: f32) {
    let at = s.at;
    let dist = t.dist([at[0], t.floor - s.height, at[1]]);
    let ot = BAND_CARDS + (dist * 4.0) as u32 * 4 + 8;
    let colour = |step: u8| CHIP_COLOURS[usize::from(step) % CHIP_COLOURS.len()];
    let n = s.chips;
    if n < PILE_MIN || s.height > felt + 0.5 {
        column(out, t, at, s.height, n, colour(s.colour), ot);
        return;
    }
    // Heights: the top denomination about half, the rest shared.
    let cols: u8 = if n >= 9 { 3 } else { 2 };
    let first = n.div_ceil(2);
    let rest = n - first;
    let second = if cols == 3 { rest.div_ceil(2) } else { rest };
    let third = rest - second;
    // The steps below the top one, wrapping past the lowest so a white
    // stack still shows two colours.
    let below = |d: u8| (s.colour + CHIP_COLOURS.len() as u8 - d) % CHIP_COLOURS.len() as u8;
    let gap = CHIP_R * 2.08;
    let off = |r: f32, a: f32| {
        [
            at[0] + t.right[0] * r + t.across[0] * a,
            at[1] + t.right[1] * r + t.across[1] * a,
        ]
    };
    // Back to front from the table's centre, so the near column is drawn
    // last within the pile.
    let mut piles = vec![
        (off(0.0, 0.0), first, colour(s.colour)),
        (off(gap, 0.0), second, colour(below(1))),
    ];
    if cols == 3 {
        piles.push((off(gap * 0.5, -gap * 0.87), third, colour(below(2))));
    }
    piles.sort_by(|a, b| {
        let da = t.dist([a.0[0], t.floor - s.height, a.0[1]]);
        let db = t.dist([b.0[0], t.floor - s.height, b.0[1]]);
        db.total_cmp(&da)
    });
    for (i, (p, chips, c)) in piles.into_iter().enumerate() {
        column(out, t, p, s.height, chips, c, ot - 4 * i as u32);
    }
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
    for ch in &s.chairs {
        chair(&mut out, pr, ch);
    }
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
        stack(&mut out, &t, st, top);
    }
    for b in &s.bursts {
        burst(&mut out, &t, s.local, b);
    }
    for a in &s.amounts {
        amount(&mut out, &t, a.at, a.height, a.value);
    }
    out
}

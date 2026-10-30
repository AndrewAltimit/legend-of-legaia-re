//! A Grid Duel's table as the hosts draw it
//! ([`legaia_engine_expansion::duel_stage`]): the table, the board and its
//! elemental cells, the cursor, every card and the bursts, as screen
//! primitives projected through the stage's shot.
//!
//! The cards are textured quads sampling the duel's card page
//! (`legaia_engine_modes::cards::table_art`), which the world writes into
//! VRAM while the duel is seated: the face where the projected card turns
//! its front to the camera, the back where it turns away - so the
//! opponent's fanned hand shows its backs and a capture flip shows the back
//! at its half turn with no rule of its own. A face's body is transparent,
//! and the owner's colour is a quad drawn under it.
//!
//! The table and the board are built here (a wooden top on legs, a gold
//! rimmed felt board), or the board's cells sample a texture of the base
//! scene when the table names a `surface`. Every corner carries its scene
//! depth, so an NPC or a wall in front of the table hides it. Draw order is
//! explicit bands of the ordering table - legs, apron, top, board, marks,
//! shadows, cards by distance, sparks - all deeper than the HUD's buckets.

use legaia_engine_expansion::duel_stage::{BurstKind, DuelStageFrame, StageCard};
use legaia_engine_modes::cards::table_art::{self, BACK_SLOT};
use legaia_render_kernels::screen_prim::{CornerDepth, FlatQuad, ScreenPrim, ScreenQuad};

use super::{Projector, Pt};

/// Ordering-table bands, farthest first. Every band sits deeper than the
/// mode HUD's buckets (`engine-ui`'s `0x2000` down).
const BAND_TABLE: u32 = 0x9_0000;
const BAND_BOARD: u32 = 0x8_0000;
const BAND_MARKS: u32 = 0x7_0000;
const BAND_SHADOW: u32 = 0x6_0000;
const BAND_CARDS: u32 = 0x4_0000;
const BAND_SPARKS: u32 = 0x3_0000;

/// The seat colours under a face: the player blue, the rival red.
pub fn owner_rgb(seat: usize, local: usize) -> [u8; 3] {
    if seat == local {
        [52, 104, 214]
    } else {
        [206, 58, 60]
    }
}

/// Tilted space around the table: a table-local `(right, across)` and a
/// height above the floor, through the projector.
struct Table<'a, 'b> {
    pr: &'a Projector<'b>,
    floor: f32,
    centre: [f32; 2],
    right: [f32; 2],
    across: [f32; 2],
    eye: [f32; 3],
}

impl Table<'_, '_> {
    fn world(&self, r: f32, a: f32, h: f32) -> [f32; 3] {
        [
            self.centre[0] + self.right[0] * r + self.across[0] * a,
            self.floor - h,
            self.centre[1] + self.right[1] * r + self.across[1] * a,
        ]
    }

    fn at(&self, r: f32, a: f32, h: f32) -> Option<Pt> {
        self.pr.at(self.world(r, a, h))
    }

    /// Distance from the eye to a raw world point.
    fn dist(&self, p: [f32; 3]) -> f32 {
        let d = [p[0] - self.eye[0], p[1] - self.eye[1], p[2] - self.eye[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }
}

fn xy(p: Pt) -> (i16, i16) {
    (p.x.round() as i16, p.y.round() as i16)
}

fn rgba(c: [u8; 3], a: u8) -> [u8; 4] {
    [c[0], c[1], c[2], a]
}

fn shade(c: [u8; 3], k: f32) -> [u8; 3] {
    c.map(|v| (f32::from(v) * k).clamp(0.0, 255.0) as u8)
}

/// A flat quad on four projected corners (`POLY_F4` order), gouraud when
/// the colours differ.
fn flat(c: [Pt; 4], colors: [[u8; 4]; 4], blend: Option<u8>, ot: u32) -> ScreenPrim {
    ScreenPrim::Flat(FlatQuad {
        xy: c.map(xy),
        color: colors[0],
        gouraud: colors.iter().any(|&k| k != colors[0]).then_some(colors),
        semi_transparent: blend.is_some(),
        abr_mode: blend.unwrap_or(0),
        ot_index: ot,
        depth: Some(CornerDepth::new(c.map(|q| q.depth))),
    })
}

/// Whether projected corners `[tl, tr, bl, ...]` turn their front to the
/// screen (clockwise with Y down).
fn front(c: &[Pt; 4]) -> bool {
    let (ax, ay) = (c[1].x - c[0].x, c[1].y - c[0].y);
    let (bx, by) = (c[2].x - c[0].x, c[2].y - c[0].y);
    ax * by - ay * bx > 0.0
}

/// A lit surface subdivided `n x n` over the table-local rectangle
/// `(r0, a0)-(r1, a1)` at height `h`, brightest under the lamp at the board's
/// centre.
#[allow(clippy::too_many_arguments)]
fn lit_rect(
    out: &mut Vec<ScreenPrim>,
    t: &Table<'_, '_>,
    (r0, a0, r1, a1): (f32, f32, f32, f32),
    h: f32,
    n: usize,
    base: [u8; 3],
    lamp: f32,
    ot: u32,
) {
    let light = |r: f32, a: f32| {
        let d = (r * r + a * a).sqrt() / lamp;
        1.25 - 0.55 * d.min(1.4)
    };
    for i in 0..n {
        for j in 0..n {
            let (u0, u1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
            let (v0, v1) = (j as f32 / n as f32, (j + 1) as f32 / n as f32);
            let (ra, rb) = (r0 + (r1 - r0) * u0, r0 + (r1 - r0) * u1);
            let (aa, ab) = (a0 + (a1 - a0) * v1, a0 + (a1 - a0) * v0);
            let corners = [(ra, aa), (rb, aa), (ra, ab), (rb, ab)];
            let pts = corners.map(|(r, a)| t.at(r, a, h));
            let [Some(p0), Some(p1), Some(p2), Some(p3)] = pts else {
                continue;
            };
            let cols = corners.map(|(r, a)| rgba(shade(base, light(r, a)), 255));
            out.push(flat([p0, p1, p2, p3], cols, None, ot));
        }
    }
}

/// A box side: the quad between two top corners and the same corners
/// `depth` lower, drawn only when it faces the camera.
#[allow(clippy::too_many_arguments)]
fn side(
    out: &mut Vec<ScreenPrim>,
    t: &Table<'_, '_>,
    a: (f32, f32),
    b: (f32, f32),
    top: f32,
    bottom: f32,
    color: [u8; 3],
    ot: u32,
) {
    let c = [
        t.at(a.0, a.1, top),
        t.at(b.0, b.1, top),
        t.at(a.0, a.1, bottom),
        t.at(b.0, b.1, bottom),
    ];
    let [Some(p0), Some(p1), Some(p2), Some(p3)] = c else {
        return;
    };
    let q = [p0, p1, p2, p3];
    if !front(&q) {
        return;
    }
    let hi = rgba(color, 255);
    let lo = rgba(shade(color, 0.55), 255);
    out.push(flat(q, [hi, hi, lo, lo], None, ot));
}

/// A box from `(r0, a0)-(r1, a1)` between two heights: its visible sides.
#[allow(clippy::too_many_arguments)]
fn box_sides(
    out: &mut Vec<ScreenPrim>,
    t: &Table<'_, '_>,
    (r0, a0, r1, a1): (f32, f32, f32, f32),
    top: f32,
    bottom: f32,
    color: [u8; 3],
    ot: u32,
) {
    // Each side runs left to right as seen from outside it (the near side
    // from the player's left to right, and on round), so its outward face
    // is its front and an inward one is culled.
    let ring = [(r0, a0), (r1, a0), (r1, a1), (r0, a1)];
    for i in 0..4 {
        side(out, t, ring[i], ring[(i + 1) % 4], top, bottom, color, ot);
    }
}

const WOOD: [u8; 3] = [104, 62, 34];
const WOOD_DARK: [u8; 3] = [62, 36, 20];
const RIM: [u8; 3] = [186, 146, 64];
const FELT: [u8; 3] = [30, 98, 70];

/// The table: legs, apron and top.
fn table(out: &mut Vec<ScreenPrim>, t: &Table<'_, '_>, s: &DuelStageFrame) {
    let tb = &s.table;
    let (hw, hd) = (tb.board * 0.5 + 58.0, tb.board_depth() * 0.5 + 64.0);
    let top = tb.top();
    let lift = tb.lift;
    let leg = 9.0;
    for (r, a) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let (cr, ca) = (r * (hw - 16.0), a * (hd - 16.0));
        box_sides(
            out,
            t,
            (cr - leg, ca - leg, cr + leg, ca + leg),
            top - 10.0,
            lift,
            WOOD_DARK,
            BAND_TABLE + 3,
        );
    }
    box_sides(
        out,
        t,
        (-hw, -hd, hw, hd),
        top,
        top - 11.0,
        WOOD,
        BAND_TABLE + 2,
    );
    lit_rect(
        out,
        t,
        (-hw, -hd, hw, hd),
        top,
        6,
        WOOD,
        hw.max(hd),
        BAND_TABLE + 1,
    );
    // A darker inlay band around the top's edge.
    let e = 7.0;
    for (r0, a0, r1, a1) in [
        (-hw, hd - e, hw, hd),
        (-hw, -hd, hw, -hd + e),
        (-hw, -hd + e, -hw + e, hd - e),
        (hw - e, -hd + e, hw, hd - e),
    ] {
        lit_rect(
            out,
            t,
            (r0, a0, r1, a1),
            top + 0.2,
            1,
            WOOD_DARK,
            hw.max(hd),
            BAND_TABLE,
        );
    }
}

/// The board: a gold rim, the felt cells (or the scene's surface texture),
/// with gaps between them.
fn board(out: &mut Vec<ScreenPrim>, t: &Table<'_, '_>, s: &DuelStageFrame) {
    let tb = &s.table;
    let (w, d) = (tb.board * 0.5, tb.board_depth() * 0.5);
    let top = tb.top();
    let pad = 6.0;
    lit_rect(
        out,
        t,
        (-w - pad, -d - pad, w + pad, d + pad),
        top + 0.6,
        2,
        RIM,
        w * 2.0,
        BAND_BOARD + 2,
    );
    lit_rect(
        out,
        t,
        (-w - 1.5, -d - 1.5, w + 1.5, d + 1.5),
        top + 0.7,
        1,
        [26, 20, 14],
        w * 2.0,
        BAND_BOARD + 1,
    );
    let (cw, cd) = (tb.board / 3.0, tb.board_depth() / 3.0);
    let gap = 1.6;
    for cell in 0..9 {
        // The table's axes are the local seat's, as the stage laid the
        // cards out in.
        let (r, a) = tb.cell_local(cell);
        let rect = (
            r - cw * 0.5 + gap,
            a - cd * 0.5 + gap,
            r + cw * 0.5 - gap,
            a + cd * 0.5 - gap,
        );
        match tb.surface {
            Some(surf) => surface_cell(out, t, rect, top + 0.8, surf, BAND_BOARD),
            None => lit_rect(out, t, rect, top + 0.8, 2, FELT, w * 1.6, BAND_BOARD),
        }
    }
}

/// One cell textured from the scene's own VRAM.
fn surface_cell(
    out: &mut Vec<ScreenPrim>,
    t: &Table<'_, '_>,
    (r0, a0, r1, a1): (f32, f32, f32, f32),
    h: f32,
    surf: legaia_expansion_format::modes::DuelSurface,
    ot: u32,
) {
    let c = [
        t.at(r0, a1, h),
        t.at(r1, a1, h),
        t.at(r0, a0, h),
        t.at(r1, a0, h),
    ];
    let [Some(p0), Some(p1), Some(p2), Some(p3)] = c else {
        return;
    };
    let [u, v, w, hh] = surf.uv;
    let (u1, v1) = (
        u.saturating_add(w.saturating_sub(1)),
        v.saturating_add(hh.saturating_sub(1)),
    );
    let q = [p0, p1, p2, p3];
    out.push(ScreenPrim::Textured(ScreenQuad {
        xy: q.map(xy),
        uv: [(u, v), (u1, v), (u, v1), (u1, v1)],
        clut: surf.clut,
        tpage: surf.tpage,
        color: 0x80_80_80,
        gouraud: None,
        semi_transparent: false,
        ot_index: ot,
        depth: Some(CornerDepth::new(q.map(|p| p.depth))),
    }));
}

/// The elemental cells' runes and the cursor's frame.
fn marks(out: &mut Vec<ScreenPrim>, t: &Table<'_, '_>, s: &DuelStageFrame) {
    let tb = &s.table;
    let top = tb.top() + 1.0;
    let pulse = 0.75 + 0.25 * (s.clock as f32 / 40.0).sin();
    let to_local = |p: [f32; 2]| {
        let d = [p[0] - t.centre[0], p[1] - t.centre[1]];
        (
            d[0] * t.right[0] + d[1] * t.right[1],
            d[0] * t.across[0] + d[1] * t.across[1],
        )
    };
    let cw = tb.board / 3.0;
    for &(at, e) in &s.elements {
        let (r, a) = to_local(at);
        let c = legaia_engine_modes::cards::table_art::element_rgb(Some(e));
        let glow = rgba(shade(c, 0.9 * pulse), 255);
        ring(
            out,
            t,
            (r, a),
            cw * 0.3,
            cw * 0.36,
            top,
            glow,
            BAND_MARKS + 1,
        );
        ring(
            out,
            t,
            (r, a),
            cw * 0.12,
            cw * 0.16,
            top,
            glow,
            BAND_MARKS + 1,
        );
    }
    if let Some(at) = s.cursor {
        let (r, a) = to_local(at);
        let (hw, hd) = (cw * 0.5, tb.board_depth() / 6.0);
        let k = 0.6 + 0.4 * (s.clock as f32 / 8.0).sin().abs();
        let gold = rgba(shade([255, 200, 90], k), 255);
        let e = 2.5;
        for (r0, a0, r1, a1) in [
            (r - hw, a + hd - e, r + hw, a + hd),
            (r - hw, a - hd, r + hw, a - hd + e),
            (r - hw, a - hd + e, r - hw + e, a + hd - e),
            (r + hw - e, a - hd + e, r + hw, a + hd - e),
        ] {
            let c = [
                t.at(r0, a1, top + 0.3),
                t.at(r1, a1, top + 0.3),
                t.at(r0, a0, top + 0.3),
                t.at(r1, a0, top + 0.3),
            ];
            if let [Some(p0), Some(p1), Some(p2), Some(p3)] = c {
                out.push(flat([p0, p1, p2, p3], [gold; 4], Some(1), BAND_MARKS));
            }
        }
    }
}

/// An additive ring on the table.
#[allow(clippy::too_many_arguments)]
fn ring(
    out: &mut Vec<ScreenPrim>,
    t: &Table<'_, '_>,
    c: (f32, f32),
    r0: f32,
    r1: f32,
    h: f32,
    color: [u8; 4],
    ot: u32,
) {
    const N: usize = 16;
    for i in 0..N {
        let (a0, a1) = (
            i as f32 / N as f32 * std::f32::consts::TAU,
            (i + 1) as f32 / N as f32 * std::f32::consts::TAU,
        );
        let p = |a: f32, r: f32| t.at(c.0 + a.cos() * r, c.1 + a.sin() * r, h);
        if let (Some(p0), Some(p1), Some(p2), Some(p3)) =
            (p(a0, r1), p(a1, r1), p(a0, r0), p(a1, r0))
        {
            out.push(flat([p0, p1, p2, p3], [color; 4], Some(1), ot));
        }
    }
}

/// A card's world corners, `[tl, tr, bl, br]`, sub-divided `2 x 2`: rows of
/// points from the top edge down, each left to right.
fn card_grid(t: &Table<'_, '_>, c: &StageCard) -> [[[f32; 3]; 3]; 3] {
    std::array::from_fn(|row| {
        std::array::from_fn(|col| {
            let (u, v) = (col as f32 - 1.0, 1.0 - row as f32);
            [
                c.centre[0] + c.right[0] * u + c.up[0] * v,
                t.floor - (c.height + c.right[1] * u + c.up[1] * v),
                c.centre[1] + c.right[2] * u + c.up[2] * v,
            ]
        })
    })
}

/// One card: its shadow on the table, the owner's colour, the face or the
/// back.
fn card(out: &mut Vec<ScreenPrim>, t: &Table<'_, '_>, s: &DuelStageFrame, c: &StageCard) {
    let grid = card_grid(t, c);
    let pts: Option<Vec<Vec<Pt>>> = grid
        .iter()
        .map(|row| row.iter().map(|&p| t.pr.at(p)).collect())
        .collect();
    let Some(pts) = pts else {
        return;
    };
    let centre = grid[1][1];
    let dist = t.dist(centre);
    // A raised card draws over its neighbours in the hand.
    let near = if c.lit { 0.0 } else { dist * 4.0 + 64.0 };
    let ot = BAND_CARDS + near as u32 * 4;
    // The shadow: the card's outline pressed onto the table, spreading and
    // fading as it rises.
    let top = s.table.top() + 0.4;
    let spread = 1.0 + c.airborne * 0.35;
    let off = c.airborne * 10.0;
    let sh = |u: f32, v: f32| {
        let p = [
            c.centre[0] + (c.right[0] * u + c.up[0] * v) * spread + t.right[0] * off,
            t.floor - top,
            c.centre[1] + (c.right[2] * u + c.up[2] * v) * spread + t.right[1] * off,
        ];
        t.pr.at(p)
    };
    if let (Some(a), Some(b), Some(cc), Some(d)) =
        (sh(-1.0, 1.0), sh(1.0, 1.0), sh(-1.0, -1.0), sh(1.0, -1.0))
    {
        let k = (70.0 * (1.0 - 0.6 * c.airborne)) as u8;
        out.push(flat(
            [a, b, cc, d],
            [[k, k, k, 255]; 4],
            Some(2),
            BAND_SHADOW,
        ));
    }
    let corners = [pts[0][0], pts[0][2], pts[2][0], pts[2][2]];
    let facing = front(&corners);
    // The owner's colour under the face.
    if facing {
        let base = owner_rgb(c.owner, s.local);
        let hi = rgba(shade(base, 1.2), 255);
        let lo = rgba(shade(base, 0.7), 255);
        out.push(flat(corners, [hi, hi, lo, lo], None, ot + 2));
    }
    // The texture, `2 x 2` so its affine mapping bends with the card.
    let slot = if facing {
        table_art::card_slot(c.deck, c.card)
    } else {
        BACK_SLOT
    };
    let uv = table_art::slot_quad_uv(slot);
    let (u0, v0) = uv[0];
    let (u1, v1) = uv[3];
    let mid = |a: u8, b: u8| ((u16::from(a) + u16::from(b)) / 2) as u8;
    let us = [u0, mid(u0, u1), u1];
    let vs = [v0, mid(v0, v1), v1];
    let tint = if c.lit { 0x98_98_98 } else { 0x80_80_80 };
    for row in 0..2 {
        for col in 0..2 {
            let q = [
                pts[row][col],
                pts[row][col + 1],
                pts[row + 1][col],
                pts[row + 1][col + 1],
            ];
            // Seen from behind the columns run the other way, so the back
            // reads unmirrored.
            let (ca, cb) = if facing {
                (col, col + 1)
            } else {
                (2 - col, 1 - col)
            };
            let uvq = [
                (us[ca], vs[row]),
                (us[cb], vs[row]),
                (us[ca], vs[row + 1]),
                (us[cb], vs[row + 1]),
            ];
            out.push(ScreenPrim::Textured(ScreenQuad {
                xy: q.map(xy),
                uv: uvq,
                clut: 0,
                tpage: table_art::tpage(),
                color: tint,
                gouraud: None,
                semi_transparent: false,
                ot_index: ot + 1,
                depth: Some(CornerDepth::new(q.map(|p| p.depth))),
            }));
        }
    }
    // A raised card's rim light.
    if c.lit && facing {
        let k = 0.55 + 0.45 * (s.clock as f32 / 10.0).sin().abs();
        let g = rgba(shade([255, 210, 110], k), 255);
        let grow = |p: Pt, q: Pt| Pt {
            x: p.x + (p.x - q.x) * 0.08,
            y: p.y + (p.y - q.y) * 0.08,
            depth: p.depth,
        };
        let m = pts[1][1];
        let rim = corners.map(|p| grow(p, m));
        out.push(flat(rim, [g; 4], Some(1), ot + 3));
    }
}

/// The sparks of a capture (gold for the player, red for the rival; a
/// chain's twice as many and higher) and a landing's dust ring.
fn burst(
    out: &mut Vec<ScreenPrim>,
    t: &Table<'_, '_>,
    s: &DuelStageFrame,
    b: &legaia_engine_expansion::duel_stage::StageBurst,
) {
    let age = b.age as f32;
    let life = legaia_engine_expansion::duel_stage::BURST_FRAMES as f32;
    let fade = (1.0 - age / life).max(0.0);
    let (n, speed, up, size, color) = match b.kind {
        BurstKind::Dust => (12, 1.6, 0.4, 5.0, [120, 110, 100]),
        BurstKind::Capture => (
            16,
            2.0,
            2.6,
            3.2,
            if b.seat == s.local {
                [255, 214, 120]
            } else {
                [255, 120, 96]
            },
        ),
        BurstKind::Chain => (32, 2.8, 3.6, 4.0, [255, 244, 190]),
    };
    for i in 0..n {
        let ang = i as f32 * 2.399_963 + (b.at[0] * 0.013);
        let k = 0.6 + 0.4 * ((i * 7 % 5) as f32 / 4.0);
        let d = age * speed * k;
        let h = b.height + 2.0 + (up * k * age - 0.09 * age * age).max(0.0);
        let p = [
            b.at[0] + ang.cos() * d,
            t.floor - h,
            b.at[1] + ang.sin() * d,
        ];
        let Some(c) = t.pr.at(p) else {
            continue;
        };
        let Some(e) = t.pr.at([p[0], p[1] - size, p[2]]) else {
            continue;
        };
        let r = ((e.x - c.x).powi(2) + (e.y - c.y).powi(2)).sqrt().max(0.8);
        let r = if b.kind == BurstKind::Dust {
            r * (1.0 + age / 10.0)
        } else {
            r * (0.5 + fade)
        };
        let at = |dx: f32, dy: f32| Pt {
            x: c.x + dx * r,
            y: c.y + dy * r,
            depth: c.depth,
        };
        let q = [at(0.0, -1.0), at(1.0, 0.0), at(-1.0, 0.0), at(0.0, 1.0)];
        // Additive: a spark fades by dimming.
        let col = rgba(shade(color, fade), 255);
        out.push(flat(q, [col; 4], Some(1), BAND_SPARKS + i as u32));
    }
}

/// The duel table's pieces through `pr`.
pub(super) fn duel_pieces(pr: &Projector<'_>, s: &DuelStageFrame) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    let c = s.table.centre();
    let floor = pr.world.sample_field_floor_height(c[0] as i32, c[1] as i32) as f32;
    let (across, right) = s.table.axes(s.local);
    let t = Table {
        pr,
        floor,
        centre: c,
        right,
        across,
        eye: pr.eye,
    };
    if s.table.draw_table {
        table(&mut out, &t, s);
    }
    board(&mut out, &t, s);
    marks(&mut out, &t, s);
    for card_ in &s.cards {
        card(&mut out, &t, s, card_);
    }
    for b in &s.bursts {
        burst(&mut out, &t, s, b);
    }
    out
}

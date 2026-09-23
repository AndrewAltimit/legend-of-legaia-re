//! The move-VM extension's **scanline strip emitter** - `FUN_801D31B0` in the
//! field overlay (PROT 0897, slot-A base `0x801CE818`), the routine sub-op
//! `0x2C` of the extension dispatcher `FUN_801D362C` calls (`jal 0x801D31B0`
//! at `0x801D44C8`, the only reference to it on the disc).
//!
//! # What it draws
//!
//! A textured object built out of **one-pixel-tall rows**. The actor's
//! position is projected twice: once as a point (`FUN_8005BA38`, a
//! `RotTransPers` - the screen centre and the ordering-table depth), once as
//! a camera-facing box of half-extent `(slab+0x18, slab+0x1A)` through the
//! billboard projector `FUN_800195A8` (the box's top and bottom rows and its
//! two top corners' x). Then, bottom row first, every screen row between the
//! box's bottom and top edges gets:
//!
//! * a **width** - the box's top edge `x0..x1`, scaled about the centre by
//!   `cos(angle)`, where `angle = ((sy - y) << 10) / (sy - y0 + 2)`: full
//!   width at the centre row, narrowing toward the box's top edge (and, by
//!   the cosine's symmetry, toward the rows below the centre too);
//! * an optional horizontal **wobble** - both edges shift by
//!   `sin((y - sy) * freq -/+ phase) * amp >> 12` when `slab+0x1E` (`freq`)
//!   is non-zero, the left edge on `- phase`, the right on `+ phase`;
//! * a **texture row** `v`, one texel per screen row, scrolled by the
//!   actor's `+0x28` and wrapped into the slab's `v0..=v1` band;
//! * a run of `POLY_FT4` quads tiling the row with the slab's `u0..=u1`
//!   span, one texel per pixel, scrolled by the actor's `+0x24` and clipped
//!   to the row's edges and to the draw-area rectangle.
//!
//! Every quad is opaque (GP0 `0x2C`, colour `0x808080`), carries the slab's
//! tpage and CLUT, and links at the point projection's depth
//! `otz >> DAT_1F8003A4`; one `SetDrawMode(0, 0, tpage, NULL)` packet
//! (`FUN_80059010`) is linked last at the same slot, so it executes first.
//!
//! # Where its inputs live
//!
//! The slab is the actor record's `+0x9C` window, read at these offsets
//! (`s5 = actor + 0x9C` at `0x801D31FC`):
//!
//! | slab | actor | field |
//! |---|---|---|
//! | `+0x0C..+0x12` | `+0xA8..+0xAE` | texture rect `u0, v0, u1, v1` (`lhu`) |
//! | `+0x14` / `+0x16` | `+0xB0` / `+0xB2` | tpage / CLUT words |
//! | `+0x18` / `+0x1A` | `+0xB4` / `+0xB6` | box half-extent (`lh`) |
//! | `+0x1C` / `+0x1E` | `+0xB8` / `+0xBA` | wobble amplitude / frequency (`lh`) |
//!
//! `+0x18..+0x1E` are the four words sub-ops `0x2B` (set) and `0x2D` (add)
//! write - a box size and a wobble, not "UV bounds". The scroll phases are
//! the actor's render banks `+0x24` (u), `+0x26` (wobble phase) and `+0x28`
//! (v), which sub-op `0x31` / `0x33` and the outer VM's render-bank ops
//! drive. The instruction's own five operand words are **never read**: the
//! routine overwrites `a1` (the instruction pointer) before its first use
//! (`addiu a1,sp,0x30` at `0x801D31C4`).
//!
//! # What the port splits off
//!
//! The two projections are the host's (they need the live camera); the port
//! takes their results as [`StripProjection`]. The trig tables are the
//! in-image LUT pair (`_DAT_8007B7F8` cosine view, `_DAT_8007B81C` sine
//! view) and arrive through a lookup closure, the move-VM host's
//! `rotation_lut`. What is left - the row walk, the wrap loops, the tiling
//! and the packet fields - is this module, instruction for instruction.
//!
//! One deliberate difference: retail's wrap loops (`do x -= span while ...`)
//! never terminate for a slab whose `u` or `v` span is zero or negative; the
//! port returns `None` for such a slab instead of spinning.
//!
//! # Who issues it
//!
//! Nothing on the disc, as far as the per-scene carriers go: the disc-gated
//! census `crates/engine-core/tests/move_ext_strip_census_disc.rs` walks
//! every CDNAME scene's prescript stager records through the move-VM decoder
//! and scans every PROT entry's type-`0x05` MOVE slot, and finds no
//! `0x2F 0x2C` instruction in either, while sub-ops `0x2B` and `0x2D` do
//! occur. See `docs/subsystems/move-vm-overlay-ext.md`.

use crate::move_vm::ActorState;

/// Mask of the 12-bit angle / phase space the LUT lookups use.
const ANGLE_MASK: i32 = 0xFFF;

/// GP0 word 1 of every strip quad: `POLY_FT4` opaque, colour `0x808080`
/// (`lui v1,0x2c80; ori v1,v1,0x8080` at `0x801D34F8`).
pub const STRIP_QUAD_CODE: u32 = 0x2C80_8080;

/// The slab fields `FUN_801D31B0` reads, off the actor record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StripSlab {
    /// `slab+0x0C` - left texel column.
    pub u0: u16,
    /// `slab+0x0E` - top texel row.
    pub v0: u16,
    /// `slab+0x10` - right texel column (inclusive).
    pub u1: u16,
    /// `slab+0x12` - bottom texel row (inclusive).
    pub v1: u16,
    /// `slab+0x14` - GP0 tpage word.
    pub tpage: u16,
    /// `slab+0x16` - GP0 CLUT word.
    pub clut: u16,
    /// `slab+0x18` - box half-width, the projector's `hw`.
    pub half_w: i16,
    /// `slab+0x1A` - box half-height, the projector's `hh`.
    pub half_h: i16,
    /// `slab+0x1C` - wobble amplitude (q12 multiplier of the sine).
    pub wobble_amp: i16,
    /// `slab+0x1E` - wobble frequency; `0` turns the wobble off.
    pub wobble_freq: i16,
}

impl StripSlab {
    /// Read the slab off a move-VM actor. `+0xA8` / `+0xAA` are the low and
    /// high halves of [`ActorState::field_a8`]; `+0xAC..` is the
    /// [`ActorState::anim_block`] window.
    pub fn from_actor(s: &ActorState) -> Self {
        let a8 = s.field_a8 as u32;
        Self {
            u0: a8 as u16,
            v0: (a8 >> 16) as u16,
            u1: s.anim_block_u16(0),
            v1: s.anim_block_u16(2),
            tpage: s.anim_block_u16(4),
            clut: s.anim_block_u16(6),
            half_w: s.anim_block_u16(8) as i16,
            half_h: s.anim_block_u16(10) as i16,
            wobble_amp: s.anim_block_u16(12) as i16,
            wobble_freq: s.anim_block_u16(14) as i16,
        }
    }
}

/// The actor's scroll phases: render banks `+0x24` / `+0x26` / `+0x28`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StripScroll {
    /// `+0x24` - u scroll (`& 0xFFF`, a fraction of the u span).
    pub u_phase: u16,
    /// `+0x26` - wobble phase (`lh`, added before the `& 0xFFF`).
    pub wobble_phase: i16,
    /// `+0x28` - v scroll (`& 0xFFF`, a fraction of the v span).
    pub v_phase: u16,
}

impl StripScroll {
    /// Read the three phases off a move-VM actor.
    pub fn from_actor(s: &ActorState) -> Self {
        Self {
            u_phase: s.render_24 as u16,
            wobble_phase: s.render_26,
            v_phase: s.render_28 as u16,
        }
    }
}

/// The two projections the routine makes of the actor position, as the
/// host's camera computes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StripProjection {
    /// `FUN_8005BA38`'s screen point `(sx, sy)` (`sp+0x30` / `sp+0x32`).
    pub centre: (i16, i16),
    /// `FUN_8005BA38`'s return - the OT depth before the resolution shift.
    /// Zero skips the whole draw (`beqz s7` at `0x801D3204`).
    pub otz: u32,
    /// `FUN_800195A8`'s four corners, retail out-pointer order
    /// `(-hw,-hh)`, `(+hw,-hh)`, `(-hw,+hh)`, `(+hw,+hh)`. The routine reads
    /// corner 0's x and y, corner 1's x and corner 3's y.
    pub corners: [(i16, i16); 4],
}

/// The draw-area clip rectangle the rows test against: the render scratch
/// halfwords `0x1F800388` (left), `0x1F80038A` (top), `0x1F80038C` (right)
/// and `0x1F80038E` (bottom), all read with `lhu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripClip {
    /// `0x1F800388`.
    pub left: u16,
    /// `0x1F80038A`.
    pub top: u16,
    /// `0x1F80038C`.
    pub right: u16,
    /// `0x1F80038E`.
    pub bottom: u16,
}

impl Default for StripClip {
    /// The field's 320x240 display.
    fn default() -> Self {
        Self {
            left: 0,
            top: 0,
            right: 0x140,
            bottom: 0xF0,
        }
    }
}

/// One sub-op `0x2C` execution, captured at the move-VM step: everything the
/// routine reads off the actor, so the draw can run later on the host's
/// render pass with the camera it draws the frame through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StripRequest {
    /// The slab window.
    pub slab: StripSlab,
    /// The three render-bank phases.
    pub scroll: StripScroll,
    /// The actor position `+0x14 / +0x16 / +0x18` the two projections take.
    pub pos: [i16; 3],
}

impl StripRequest {
    /// Capture a request off the actor the extension dispatcher is stepping.
    pub fn from_actor(s: &ActorState) -> Self {
        Self {
            slab: StripSlab::from_actor(s),
            scroll: StripScroll::from_actor(s),
            pos: [s.world_x, s.world_y, s.world_z],
        }
    }
}

/// One emitted `POLY_FT4`: a one-pixel-tall textured span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripQuad {
    /// Left x (`xy0` / `xy2`).
    pub x0: i16,
    /// Right x (`xy1` / `xy3`).
    pub x1: i16,
    /// Top y (`xy0` / `xy1`); the bottom vertices sit on `y + 1`.
    pub y: i16,
    /// `u` at the left vertices.
    pub u0: u8,
    /// `u` at the right vertices.
    pub u1: u8,
    /// `v` at all four vertices - the quad samples one texel row.
    pub v: u8,
    /// GP0 tpage word (`+0x16`).
    pub tpage: u16,
    /// GP0 CLUT word (`+0x0E`).
    pub clut: u16,
}

impl StripQuad {
    /// The four vertices in retail `v0..v3` order.
    pub fn xy(&self) -> [(i16, i16); 4] {
        let y1 = self.y.wrapping_add(1);
        [
            (self.x0, self.y),
            (self.x1, self.y),
            (self.x0, y1),
            (self.x1, y1),
        ]
    }

    /// The four texture coordinates in retail `v0..v3` order.
    pub fn uv(&self) -> [(u8, u8); 4] {
        [
            (self.u0, self.v),
            (self.u1, self.v),
            (self.u0, self.v),
            (self.u1, self.v),
        ]
    }
}

/// One call's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripEmit {
    /// The OT slot every packet links at: `otz >> DAT_1F8003A4`.
    pub ot_index: u32,
    /// The spans, in emission order (bottom row first, left tile first).
    pub quads: Vec<StripQuad>,
    /// The tpage the trailing `SetDrawMode` packet installs (`slab+0x14`).
    pub mode_tpage: u16,
}

/// Run `FUN_801D31B0` over a slab.
///
/// `ot_shift` is the scratchpad OT-resolution byte `DAT_1F8003A4`; `lut`
/// maps a 12-bit angle to the retail `(sin, cos)` pair (q12). Returns `None`
/// when the point projection's depth is zero (retail's early exit) or when
/// the slab's `u` / `v` span is not positive (where retail's wrap loops
/// would not terminate).
///
/// PORT: FUN_801D31B0 (PROT 0897; the move-VM extension's sub-op `0x2C`
/// callee). Reached through `MoveHost::ext_func801d31b0`, which the
/// engine-core field host fills; the draw is `World::move_strip_render_step`.
pub fn emit_strip(
    slab: &StripSlab,
    scroll: &StripScroll,
    proj: &StripProjection,
    clip: &StripClip,
    ot_shift: u8,
    lut: impl Fn(u16) -> (i16, i16),
) -> Option<StripEmit> {
    if proj.otz == 0 {
        return None;
    }
    let ot_index = proj.otz >> (ot_shift & 0x1F);

    let v0 = i32::from(slab.v0);
    let v1 = i32::from(slab.v1);
    let u0 = i32::from(slab.u0);
    let u1 = i32::from(slab.u1);
    // `t2 = v1 - (v0 - 1)`, `s6 = u1 - (u0 - 1)`.
    let v_span = v1 - (v0 - 1);
    let u_span = u1 - (u0 - 1);
    if v_span <= 0 || u_span <= 0 {
        return None;
    }
    let v_scroll = (i32::from(scroll.v_phase) & ANGLE_MASK).wrapping_mul(v_span);
    let u_scroll = (i32::from(scroll.u_phase) & ANGLE_MASK).wrapping_mul(u_span) >> 12;

    let (sx, sy) = (i32::from(proj.centre.0), i32::from(proj.centre.1));
    let (x0, y0) = (i32::from(proj.corners[0].0), i32::from(proj.corners[0].1));
    let x1 = i32::from(proj.corners[1].0);
    let y_bottom = i32::from(proj.corners[3].1);

    // The texture row the bottom screen row samples.
    let mut v = ((v0 + (v1 + 1)) >> 1) + (y_bottom - sy + (v_scroll >> 12));
    while v1 < v {
        v -= v_span;
    }

    let mut quads = Vec::new();
    let mut y = y_bottom;
    while y >= y0 {
        if v < v0 {
            v += v_span;
        }
        let visible = y >= i32::from(clip.top) - 4 && y <= i32::from(clip.bottom);
        if visible {
            emit_row(
                &mut quads,
                slab,
                scroll,
                (sx, sy),
                (x0, x1, y0),
                y,
                v,
                u_span,
                u_scroll,
                clip,
                &lut,
            );
        }
        y -= 1;
        v -= 1;
    }
    Some(StripEmit {
        ot_index,
        quads,
        mode_tpage: slab.tpage,
    })
}

/// One screen row: `0x801D3344..0x801D359C`.
#[allow(clippy::too_many_arguments)]
fn emit_row(
    out: &mut Vec<StripQuad>,
    slab: &StripSlab,
    scroll: &StripScroll,
    (sx, sy): (i32, i32),
    (x0, x1, y0): (i32, i32, i32),
    y: i32,
    v: i32,
    u_span: i32,
    u_scroll: i32,
    clip: &StripClip,
    lut: &impl Fn(u16) -> (i16, i16),
) {
    // Row width: the top edge scaled about the centre by cos(angle).
    let denom = sy - y0 + 2;
    let angle = if denom != 0 {
        (((sy - y) << 10) / denom) & ANGLE_MASK
    } else {
        0
    };
    let cos = i32::from(lut(angle as u16).1);
    let mut left = ((x0 - sx).wrapping_mul(cos) >> 12) + sx;
    let mut right = ((x1 - sx).wrapping_mul(cos) >> 12) + sx;

    // Wobble: the two edges ride the sine on opposite phase offsets.
    if slab.wobble_freq != 0 {
        let a = (y - sy).wrapping_mul(i32::from(slab.wobble_freq));
        let phase = i32::from(scroll.wobble_phase);
        let amp = i32::from(slab.wobble_amp);
        let sin_l = i32::from(lut(((a - phase) & ANGLE_MASK) as u16).0);
        let sin_r = i32::from(lut(((a + phase) & ANGLE_MASK) as u16).0);
        left += sin_l.wrapping_mul(amp) >> 12;
        right += sin_r.wrapping_mul(amp) >> 12;
    }
    if right < left {
        core::mem::swap(&mut left, &mut right);
    }

    // First tile origin, walked left until it no longer passes the row start.
    let mut tile = sx - (u_span >> 1) + u_scroll;
    while left < tile {
        tile -= u_span;
    }
    while right >= tile {
        if i32::from(clip.right) < tile {
            break;
        }
        let skip = if tile < left { left - tile } else { 0 };
        let next = tile + u_span;
        let width = if right < next { right - tile } else { u_span };
        let end = tile + width;
        if i32::from(clip.left) < end {
            out.push(StripQuad {
                x0: (tile + skip) as i16,
                x1: end as i16,
                y: y as i16,
                u0: (i32::from(slab.u0) + skip) as u8,
                u1: (i32::from(slab.u0) + width) as u8,
                v: v as u8,
                tpage: slab.tpage,
                clut: slab.clut,
            });
        }
        tile = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat LUT: `cos = 4096` everywhere, `sin = 0` - rows keep the full
    /// box width and never wobble.
    fn flat(_: u16) -> (i16, i16) {
        (0, 4096)
    }

    fn slab() -> StripSlab {
        StripSlab {
            u0: 0,
            v0: 0,
            u1: 31,
            v1: 15,
            tpage: 0x0017,
            clut: 0x7F00,
            half_w: 16,
            half_h: 8,
            wobble_amp: 0,
            wobble_freq: 0,
        }
    }

    fn proj() -> StripProjection {
        StripProjection {
            centre: (160, 120),
            otz: 0x200,
            corners: [(150, 110), (170, 110), (150, 130), (170, 130)],
        }
    }

    #[test]
    fn a_zero_depth_skips_the_draw() {
        let mut p = proj();
        p.otz = 0;
        assert!(
            emit_strip(
                &slab(),
                &StripScroll::default(),
                &p,
                &StripClip::default(),
                0,
                flat
            )
            .is_none()
        );
    }

    #[test]
    fn a_non_positive_span_is_refused_rather_than_looping() {
        let mut s = slab();
        s.u1 = 0;
        s.u0 = 5;
        assert!(
            emit_strip(
                &s,
                &StripScroll::default(),
                &proj(),
                &StripClip::default(),
                0,
                flat
            )
            .is_none()
        );
    }

    #[test]
    fn every_row_between_the_box_edges_draws_one_texel_row() {
        let e = emit_strip(
            &slab(),
            &StripScroll::default(),
            &proj(),
            &StripClip::default(),
            2,
            flat,
        )
        .unwrap();
        assert_eq!(e.ot_index, 0x200 >> 2);
        assert_eq!(e.mode_tpage, 0x0017);
        // Rows 130 down to 110 inclusive, bottom first.
        let rows: Vec<i16> = e.quads.iter().map(|q| q.y).collect();
        assert_eq!(rows.first(), Some(&130));
        assert_eq!(rows.last(), Some(&110));
        for q in &e.quads {
            assert!(q.x0 >= 150 && q.x1 <= 170, "{q:?} leaves the box");
            assert!(q.u1 >= q.u0);
            assert_eq!(q.u1 - q.u0, (q.x1 - q.x0) as u8, "one texel per pixel");
            assert!(q.v <= 15, "v wraps inside the slab band");
        }
        // Consecutive rows step v by one texel (with the band wrap).
        let v: Vec<u8> = e.quads.iter().map(|q| q.v).collect();
        for w in v.windows(2) {
            assert!(w[1] + 1 == w[0] || (w[0] == 0 && w[1] == 15), "{w:?}");
        }
    }

    #[test]
    fn the_centre_row_samples_the_band_middle() {
        // Bottom row sits `y_bottom - sy = 10` below the centre, so the
        // bottom texel row is `(0 + 15 + 1) / 2 + 10 = 18`, wrapped by the
        // 16-row span to 2.
        let e = emit_strip(
            &slab(),
            &StripScroll::default(),
            &proj(),
            &StripClip::default(),
            0,
            flat,
        )
        .unwrap();
        assert_eq!(e.quads[0].v, 2);
        let centre = e.quads.iter().find(|q| q.y == 120).unwrap();
        assert_eq!(centre.v, 8);
    }

    #[test]
    fn a_row_wider_than_the_texture_tiles() {
        let mut p = proj();
        p.corners = [(100, 110), (220, 110), (100, 130), (220, 130)];
        let e = emit_strip(
            &slab(),
            &StripScroll::default(),
            &p,
            &StripClip::default(),
            0,
            flat,
        )
        .unwrap();
        let row: Vec<&StripQuad> = e.quads.iter().filter(|q| q.y == 120).collect();
        assert!(row.len() >= 4, "a 120 px row over a 32 texel span tiles");
        // Tiles abut: each starts where the previous ended.
        for w in row.windows(2) {
            assert_eq!(w[0].x1, w[1].x0);
        }
        assert_eq!(row.first().unwrap().x0, 100);
        assert_eq!(row.last().unwrap().x1, 220);
    }

    #[test]
    fn the_clip_rect_drops_rows_and_tiles_outside_it() {
        let clip = StripClip {
            left: 0,
            top: 125,
            right: 0x140,
            bottom: 0xF0,
        };
        let e = emit_strip(&slab(), &StripScroll::default(), &proj(), &clip, 0, flat).unwrap();
        // `y < top - 4` is dropped: rows 121..=130 survive.
        assert!(e.quads.iter().all(|q| q.y >= 121));
        assert!(e.quads.iter().any(|q| q.y == 121));
    }

    #[test]
    fn the_cosine_narrows_rows_toward_the_box_top() {
        // A real cosine: cos(angle) with angle 0..=1024 across the half box.
        let cosine = |a: u16| -> (i16, i16) {
            let r = f64::from(a) * std::f64::consts::TAU / 4096.0;
            ((r.sin() * 4096.0) as i16, (r.cos() * 4096.0) as i16)
        };
        let e = emit_strip(
            &slab(),
            &StripScroll::default(),
            &proj(),
            &StripClip::default(),
            0,
            cosine,
        )
        .unwrap();
        let width = |y: i16| {
            e.quads
                .iter()
                .filter(|q| q.y == y)
                .map(|q| i32::from(q.x1 - q.x0))
                .sum::<i32>()
        };
        assert!(
            width(120) > width(112),
            "centre row wider than a row near the top"
        );
    }

    #[test]
    fn the_slab_reads_off_the_actor_record() {
        let mut a = ActorState::new();
        a.field_a8 = (0x0010_0004u32) as i32; // u0 = 4, v0 = 0x10
        for (i, v) in [0x23u16, 0x1F, 0x17, 0x7F00, 16, 8, 3, 2]
            .iter()
            .enumerate()
        {
            a.anim_block_u16_set(i * 2, *v);
        }
        let s = StripSlab::from_actor(&a);
        assert_eq!((s.u0, s.v0, s.u1, s.v1), (4, 0x10, 0x23, 0x1F));
        assert_eq!((s.tpage, s.clut), (0x17, 0x7F00));
        assert_eq!(
            (s.half_w, s.half_h, s.wobble_amp, s.wobble_freq),
            (16, 8, 3, 2)
        );
    }
}

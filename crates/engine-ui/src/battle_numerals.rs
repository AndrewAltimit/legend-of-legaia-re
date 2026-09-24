//! The battle value readout as **screen-space PSX primitives** - retail's own
//! 24x24 numeral cells, sampled out of VRAM rather than restyled in a font.
//!
//! [`legaia_engine_vm::battle_value_readout`] pins the layout: where a landed
//! hit's numeral pops in and rises to, which texel cell each decimal digit is
//! (`digit_cell_u`, the strip runs `1234567890`), and the `N HIT` / `TOTAL` /
//! `DAMAGE` cluster's label seats and 16-px value row. What it does *not* say
//! is how a host draws them, and that is where the two hosts drifted: the
//! native window built a VRAM-textured mesh of its own and the browser play
//! page fell back to dialog-font glyphs, so the same fight showed retail's
//! gold numerals in the window and white text digits in the tab.
//!
//! This module is the missing shared half. It turns the kernel's output into
//! [`ScreenPrim`]s on the battle effect atlas's glyph page ([`GLYPH_TPAGE`] /
//! [`GLYPH_CLUT`]), which both hosts already have a pass for:
//! `engine-render`'s `SceneWithScreenPrims` tail natively, and the play page's
//! `ScreenPrimPass` over the same `build_geometry` output in the browser. The
//! only per-host step left is projecting the struck actor to a stage point,
//! which needs the camera the host is drawing with.
//!
//! # Two things the quads must get right
//!
//! **The texel rect is inclusive.** A cell's right/bottom texel is
//! `u + cell - 1`, not `u + cell`: retail's `POLY_FT4` names corner texels,
//! so an exclusive rect bleeds one column of the neighbouring digit into
//! every cell. [`digit_quad`] applies the `- 1`.
//!
//! **The colour word is neutral and the quads are opaque.** Retail's readout
//! quads carry `0x2C808080` - GP0 command `0x2C` (opaque textured quad) at
//! the passthrough modulation colour. The sheet is already the gold ramp, so
//! tinting here double-applies it, and marking them semi-transparent puts a
//! blend equation under art that has none.
//!
//! [`legaia_engine_vm::battle_value_readout`]: https://docs.rs/legaia-engine-vm

use legaia_engine_vm::battle_value_readout as vr;

use crate::screen_prim::{ScreenPrim, ScreenQuad};

pub use vr::{GLYPH_CLUT, GLYPH_TPAGE};

/// Ordering-table bucket the readout links at - **retail's own**, not a
/// chosen one.
///
/// Every readout packet in the captured display lists carries the OT tag
/// [`vr::OT_TAG`] = `0x09000000`, whose high byte is the bucket, so the
/// numerals sit at `9`: behind the cinematic bars
/// ([`crate::screen_prim::CINEMATIC_BAR_OT`] = `8`, which retail links one
/// bucket in front of everything) and ahead of the weapon trails (`0x20`) and
/// the move-FX streak (`0x18`). Picking `0` instead would put a damage figure
/// over a full-screen fade the summon band raises, which retail never does.
pub const VALUE_READOUT_OT: u32 = vr::OT_TAG >> 24;

/// The modulation colour word every readout quad carries (`0x808080`, the
/// passthrough level of the PSX texture blend `texel * colour / 128`).
pub const READOUT_COLOUR: u32 = 0x0080_8080;

/// Clamp a stage coordinate into the `i16` the primitive record holds. A
/// numeral seated off an actor that projected far off-screen is clamped
/// rather than wrapped.
fn stage_i16(v: i32) -> i16 {
    v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// One opaque textured quad on the glyph page: a stage rect `(x, y, w, h)`
/// and an **inclusive** texel rect `(u0, v0, u1, v1)`.
///
/// The stage rect is inclusive on the same terms as the texel rect, because
/// that is what every caller hands it: a label's `size()` is `u1 - u0 + 1`
/// and a digit cell's `w` tops out at the cell's own [`vr::DIGIT_CELL`]
/// texels. So the far corner is `x + w - 1`, not `x + w` - retail's own
/// readout quads name it that way (31/31, 47/47 and 55/55 blits across three
/// captured battle states), and the exclusive form drew every glyph a pixel
/// wider and a pixel taller than the art it sampled.
///
/// The digit **pitch** is the other half of the same fact: retail's pitch is
/// the inclusive drawn width, which is why [`vr::DIGIT_GAP`] is `0`. Changing
/// one without the other moves the run.
pub fn readout_quad(rect: (i32, i32, u32, u32), uv: (u8, u8, u8, u8), ot_index: u32) -> ScreenPrim {
    let (x0, y0) = (stage_i16(rect.0), stage_i16(rect.1));
    let x1 = stage_i16(rect.0 + rect.2.max(1) as i32 - 1);
    let y1 = stage_i16(rect.1 + rect.3.max(1) as i32 - 1);
    ScreenPrim::Textured(ScreenQuad {
        xy: [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
        uv: [(uv.0, uv.1), (uv.2, uv.1), (uv.0, uv.3), (uv.2, uv.3)],
        clut: GLYPH_CLUT,
        tpage: GLYPH_TPAGE,
        color: READOUT_COLOUR,
        gouraud: None,
        semi_transparent: false,
        ot_index,
        depth: None,
    })
}

/// One laid-out digit cell as a quad. The texel rect is the cell's own
/// square, made inclusive; the screen rect is whatever the pop ramp scaled it
/// to.
pub fn digit_quad(cell: &vr::ValueCell, ot_index: u32) -> ScreenPrim {
    let u1 = cell.u.saturating_add(cell.cell.saturating_sub(1));
    let v1 = cell.v.saturating_add(cell.cell.saturating_sub(1));
    readout_quad(
        (cell.x, cell.y, cell.w, cell.h),
        (cell.u, cell.v, u1, v1),
        ot_index,
    )
}

/// A run of digit cells (a floating numeral, or the cluster's value row) as
/// quads. Zero-extent cells are skipped - a host must not emit a degenerate
/// quad for a numeral that has not popped in yet.
pub fn digit_run_prims(cells: &[vr::ValueCell], ot_index: u32) -> Vec<ScreenPrim> {
    cells
        .iter()
        .filter(|c| c.w != 0 && c.h != 0)
        .map(|c| digit_quad(c, ot_index))
        .collect()
}

/// The `N HIT` / `TOTAL x` (or `DAMAGE x`) counter cluster as quads: the
/// sheet's own word cells followed by its digit cells.
///
/// The word cells are the labels' inclusive texel rects drawn 1:1, which is
/// what makes this the retail-art path rather than the font fallback in
/// [`crate::battle_combo_cluster_draws_for`].
pub fn combo_cluster_prims(cluster: &vr::ComboCluster, ot_index: u32) -> Vec<ScreenPrim> {
    let mut out = Vec::with_capacity(cluster.labels.len() + cluster.cells.len());
    for l in &cluster.labels {
        let (w, h) = l.size();
        out.push(readout_quad((l.x, l.y, w, h), l.uv, ot_index));
    }
    out.extend(digit_run_prims(&cluster.cells, ot_index));
    out
}

/// The **Arts announcement banner**'s quads as screen primitives - the one
/// builder both hosts emit the banner through.
///
/// `quads` is `World::battle_arts_banner_quads`, i.e. the pairs
/// [`legaia_engine_vm::battle_action::flash_quads`] emits for this frame's
/// layers. The emitter has already resolved every field a primitive needs -
/// the GP0 code carries the semi-transparency, the grey word is the packet
/// colour, and the CLUT / texture page are the readout's own - so this is a
/// re-shape, not a second layout: the banner's geometry lives in the VM
/// kernel and neither renderer re-derives it.
///
/// The banner shares the value readout's ordering bucket
/// ([`VALUE_READOUT_OT`]) because it shares its page.
pub fn arts_banner_prims(
    quads: &[legaia_engine_vm::battle_action::FlashQuad],
    ot_index: u32,
) -> Vec<ScreenPrim> {
    quads
        .iter()
        .map(|q| {
            let grey = u32::from(q.gray);
            ScreenPrim::Textured(ScreenQuad {
                xy: [
                    (q.x.0, q.y.0),
                    (q.x.1, q.y.0),
                    (q.x.0, q.y.1),
                    (q.x.1, q.y.1),
                ],
                uv: [
                    (q.u.0, q.v.0),
                    (q.u.1, q.v.0),
                    (q.u.0, q.v.1),
                    (q.u.1, q.v.1),
                ],
                clut: q.clut,
                tpage: q.tpage,
                color: (grey << 16) | (grey << 8) | grey,
                gouraud: None,
                // Retail's GP0 code, `0x2C | semi << 1`.
                semi_transparent: q.code & 0x02 != 0,
                ot_index,
                depth: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(p: &ScreenPrim) -> ScreenQuad {
        match p {
            ScreenPrim::Textured(q) => *q,
            ScreenPrim::Flat(_) => panic!("the readout emits textured quads only"),
        }
    }

    /// The readout's bucket is retail's own OT tag, not a chosen layer: the
    /// packets carry `0x09000000` and the bucket is its high byte. Pinned
    /// against the cinematic bars' own retail bucket so the two cannot be
    /// re-ordered by accident.
    #[test]
    fn the_ot_bucket_is_the_retail_tags_high_byte() {
        assert_eq!(VALUE_READOUT_OT, 9);
        assert_eq!(VALUE_READOUT_OT, vr::OT_TAG >> 24);
        // Larger bucket = farther = drawn earlier, so the bars sit in front.
        const {
            assert!(VALUE_READOUT_OT > crate::screen_prim::CINEMATIC_BAR_OT);
            assert!(VALUE_READOUT_OT < crate::battle_trail::WEAPON_TRAIL_OT);
            assert!(VALUE_READOUT_OT < crate::streak_pass::MOVE_FX_STREAK_OT);
        }
    }

    /// Every quad names the battle effect atlas's glyph page through its CLUT,
    /// carries the neutral modulation word, and is opaque. A tint here would
    /// double-apply the sheet's own gold ramp; a semi-transparency flag would
    /// put a blend equation under art that has none.
    #[test]
    fn quads_are_opaque_neutral_glyph_page_samples() {
        let cells = vr::value_cells(250, 160, 120, vr::POP_FRAMES);
        for p in digit_run_prims(&cells, VALUE_READOUT_OT) {
            let q = quad(&p);
            assert_eq!(q.tpage, GLYPH_TPAGE);
            assert_eq!(q.clut, GLYPH_CLUT);
            assert_eq!(q.color, READOUT_COLOUR);
            assert!(!q.semi_transparent);
            assert_eq!(q.ot_index, VALUE_READOUT_OT);
        }
    }

    /// The texel rect is inclusive: a 24-texel cell spans `u ..= u + 23`.
    /// An exclusive rect bleeds the next digit's first column into every cell.
    #[test]
    fn the_texel_rect_is_inclusive() {
        let cells = vr::value_cells(1, 160, 120, vr::POP_FRAMES);
        let q = quad(&digit_quad(&cells[0], 0));
        let u0 = vr::digit_cell_u(1);
        assert_eq!(q.uv[0], (u0, vr::DIGIT_ROW_V));
        assert_eq!(
            q.uv[3],
            (
                u0 + vr::DIGIT_CELL - 1,
                vr::DIGIT_ROW_V + vr::DIGIT_CELL - 1
            )
        );
    }

    /// The four corners are in the `v0..v3` order `build_geometry` expects:
    /// top-left, top-right, bottom-left, bottom-right, and the screen rect
    /// spans the cell's `(x, y, w, h)` **inclusively**, the way retail's own
    /// readout quads name their far corner.
    #[test]
    fn corner_order_matches_the_shared_quad_convention() {
        let p = readout_quad((10, 20, 24, 24), (0, 64, 23, 87), 0);
        let q = quad(&p);
        // Inclusive far corner: a 24-wide cell at x=10 ends at 33, not 34 -
        // the same convention the texel rect uses (0..=23).
        assert_eq!(q.xy, [(10, 20), (33, 20), (10, 43), (33, 43)]);
        assert_eq!(q.uv, [(0, 64), (23, 64), (0, 87), (23, 87)]);
    }

    /// A popping numeral draws smaller cells but samples the same 24-texel
    /// squares - the screen extent scales, the texel extent never does.
    #[test]
    fn the_pop_scales_the_screen_rect_and_not_the_texels() {
        let early = vr::value_cells(7, 160, 120, 0);
        let late = vr::value_cells(7, 160, 120, vr::POP_FRAMES);
        let qe = quad(&digit_quad(&early[0], 0));
        let ql = quad(&digit_quad(&late[0], 0));
        assert!(qe.xy[1].0 - qe.xy[0].0 < ql.xy[1].0 - ql.xy[0].0);
        assert_eq!(qe.uv, ql.uv);
    }

    /// The cluster emits one quad per label plus one per value cell, and the
    /// `HIT`/`TOTAL` style carries both words.
    #[test]
    fn the_cluster_emits_its_word_cells_and_its_digits() {
        let cluster = vr::combo_cluster(vr::ComboStyle::HitTotal, 5, 72, 0);
        let prims = combo_cluster_prims(&cluster, VALUE_READOUT_OT);
        assert_eq!(prims.len(), cluster.labels.len() + cluster.cells.len());
        // The two word cells sample the label rects the sheet carries.
        let uvs: Vec<_> = prims.iter().take(2).map(|p| quad(p).uv[0]).collect();
        assert!(uvs.contains(&(vr::LABEL_HIT.0, vr::LABEL_HIT.1)));
        assert!(uvs.contains(&(vr::LABEL_TOTAL.0, vr::LABEL_TOTAL.1)));
    }

    /// A zero-extent cell emits nothing rather than a degenerate quad.
    #[test]
    fn zero_extent_cells_are_dropped() {
        let mut cells = vr::value_cells(12, 160, 120, vr::POP_FRAMES);
        cells[0].w = 0;
        assert_eq!(digit_run_prims(&cells, 0).len(), cells.len() - 1);
    }
}

//! The battle value readout as **screen-space PSX primitives** - retail's own
//! 24x24 numeral cells, sampled out of VRAM rather than restyled in a font.
//!
//! [`legaia_engine_battle_vm::battle_value_readout`] pins the layout: where a landed
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
//! [`legaia_engine_battle_vm::battle_value_readout`]: https://docs.rs/legaia-engine-vm

use legaia_engine_battle_vm::battle_value_readout as vr;

use legaia_render_kernels::screen_prim::{ScreenPrim, ScreenQuad};

use vr::{GLYPH_CLUT, GLYPH_TPAGE};

/// Ordering-table bucket a struck actor's popup numeral links at: the damage
/// popup renderer `FUN_801DF6B8` calls `AddPrim(ot + 4, prim)` for every
/// digit (`lw a0,0xe0(s7)` with `s7 = 0x1F800314`, then `addiu a0,a0,4` in
/// the delay slot of `jal 0x8003D2C4` at `0x801DFC14` / `0x801DFD4C`), so the
/// popup shares bucket `1` with the HUD's glyphs and plates. It is linked
/// after them, so it draws first in the bucket and they cover it: in
/// `battle_menu_rugi_barra_retail`'s ordering table the three digit quads head
/// bucket 1, ahead of the `Begin` / `Run` plates, and the frame on screen shows
/// the figure behind the `Run` plate.
///
/// The `0x09` of the packets' tag word [`vr::OT_TAG`] is not a bucket: a
/// packet tag's high byte is its length in words, and a `POLY_FT4` is nine.
pub const POPUP_OT: u32 = 1;

/// Ordering-table bucket the combo / multi-row value window links at:
/// `FUN_801E805C` calls `AddPrim(ot, prim)` (`lw a0,0xe0(s5)` with
/// `s5 = 0x1F800314`, `0x801E82AC..0x801E887C`), bucket `0`, over the HUD
/// text.
pub const COMBO_OT: u32 = 0;

/// Ordering-table bucket of the Arts announcement banner: `FUN_801E2650`
/// links both quads of a layer at `ot` itself (`lw a0,0xe0(s0)` before each
/// `jal 0x8003D2C4` at `0x801E2934` / `0x801E2940`), bucket `0`.
pub const ARTS_BANNER_OT: u32 = 0;

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
/// `legaia_engine_ui::battle_combo_cluster_draws_for`.
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
/// [`legaia_engine_battle_vm::battle_action::flash_quads`] emits for this frame's
/// layers. The emitter has already resolved every field a primitive needs -
/// the GP0 code carries the semi-transparency, the grey word is the packet
/// colour, and the CLUT / texture page are the readout's own - so this is a
/// re-shape, not a second layout: the banner's geometry lives in the VM
/// kernel and neither renderer re-derives it.
///
/// The banner links at [`ARTS_BANNER_OT`].
pub fn arts_banner_prims(
    quads: &[legaia_engine_battle_vm::battle_action::FlashQuad],
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

/// Project `FUN_801DF6B8`'s view-space square through a host camera:
/// `vp` is the column-major view-projection matrix of the battle camera
/// alone (`legaia_engine_camera::projection::battle_vp`, with no model
/// scale composed), `world_scale` what the host multiplies a PSX world
/// coordinate by before it. The square's half-extent is laid along the
/// camera's own right axis (row 0 of `vp`, normalised), which is how a
/// view-space offset reads in world space; retail's square is square on
/// screen, so the vertical half-extent is the horizontal one. `None` behind
/// the eye.
///
/// The anchor rides the world scale and the half-extent does not: it is a
/// view-space length, added after the camera matrix has scaled the centre.
/// `battle_melee_hit_spark`'s two frame arenas pin it - the `15` over Gimard
/// is drawn as `18`-wide cells and then `22`-wide ones, the spans half the
/// ring timer gives at that depth with no scale, where a half-extent taken
/// through the 4x stage scale is at the 24-px cap from its fourth frame.
///
/// `anchor` is a raw PSX point, `+Y` down, and `vp` is a draw matrix: every
/// host view-projection ends in the `scale(1, -1, 1)` that cancels the
/// per-model Y flip its draws carry
/// (`legaia_engine_camera::projection::battle_vp`), so a raw point goes
/// through it with its Y negated. Fed unflipped, a numeral's rise
/// (`popup_anchor` lifts it `3/2` of the ring timer, toward `-Y`) read as a
/// fall: the run sank through the floor and off the bottom of the stage.
///
/// Beside the rect it returns the square's view-space depth, the clip `w` of
/// its centre: what `FUN_800195A8`'s ordering-table return is made of
/// ([`vr::popup_ot_depth`]).
pub fn popup_square(
    vp: &[f32; 16],
    world_scale: f32,
    anchor: [i32; 3],
    half: i32,
) -> Option<((i32, i32, i32, i32), i32)> {
    let depth = |p: [f32; 3]| vp[3] * p[0] + vp[7] * p[1] + vp[11] * p[2] + vp[15];
    let stage = |p: [f32; 3]| -> Option<(f32, f32)> {
        let mut clip = [0.0f32; 4];
        for (i, c) in clip.iter_mut().enumerate() {
            *c = vp[i] * p[0] + vp[4 + i] * p[1] + vp[8 + i] * p[2] + vp[12 + i];
        }
        (clip[3] > 0.01).then(|| {
            (
                (clip[0] / clip[3] * 0.5 + 0.5) * 320.0,
                (0.5 - clip[1] / clip[3] * 0.5) * 240.0,
            )
        })
    };
    let c = [
        anchor[0] as f32 * world_scale,
        -(anchor[1] as f32) * world_scale,
        anchor[2] as f32 * world_scale,
    ];
    let (cx, cy) = stage(c)?;
    let right = [vp[0], vp[4], vp[8]];
    let len = (right[0] * right[0] + right[1] * right[1] + right[2] * right[2]).sqrt();
    if len <= f32::EPSILON {
        return None;
    }
    let off = half as f32 / len;
    let (ex, ey) = stage([
        c[0] + right[0] * off,
        c[1] + right[1] * off,
        c[2] + right[2] * off,
    ])?;
    let hp = ((ex - cx).powi(2) + (ey - cy).powi(2)).sqrt();
    let (cx, cy) = (cx.round() as i32, cy.round() as i32);
    let hp = hp.round() as i32;
    let sz = depth(c).round() as i32;
    Some(((cx - hp, cy - hp, cx + hp, cy + hp), sz))
}

/// One landed hit's numeral, `age` frames after its push, as retail's
/// renderer `FUN_801DF6B8` lays it out over the struck actor's display trio
/// (`World::battle_display_trio`) - empty once the ring entry has expired
/// ([`vr::popup_live`]). Both hosts seat their popups through this.
pub fn popup_value_cells(
    vp: &[f32; 16],
    world_scale: f32,
    trio: [f32; 3],
    amount: u16,
    age: u16,
) -> Vec<vr::ValueCell> {
    popup_value_cells_at_step(vp, world_scale, trio, amount, age, 1)
}

/// [`popup_value_cells`] on a battle frame of `step` vsyncs: the square's
/// half-extent and the minimum-width gate read the timer after the frame's
/// own `0x10 * step` ([`vr::popup_anchor_at_step`]).
pub fn popup_value_cells_at_step(
    vp: &[f32; 16],
    world_scale: f32,
    trio: [f32; 3],
    amount: u16,
    age: u16,
    step: u8,
) -> Vec<vr::ValueCell> {
    if !vr::popup_live(age) {
        return Vec::new();
    }
    let (anchor, half) = vr::popup_anchor_at_step(trio.map(|v| v as i32), age, step);
    let Some((rect, sz)) = popup_square(vp, world_scale, anchor, half) else {
        return Vec::new();
    };
    let rect = vr::popup_near_square(rect, vr::popup_ot_depth(sz));
    vr::popup_cells(amount, rect, vr::popup_timer_at_step(age, step))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An identity-like camera: the square's projected half-extent follows
    /// the timer, and the numeral expires after 37 frames.
    #[test]
    fn popup_cells_grow_with_age_and_expire() {
        // x/z scaled to NDC over a fixed w: 1 world unit = 1/1000 NDC.
        // `w` stands at a far view depth, past the near arm.
        let mut vp = [0.0f32; 16];
        vp[0] = 4.096;
        vp[5] = 4.096;
        vp[10] = 4.096;
        vp[15] = 4096.0;
        let small = popup_value_cells(&vp, 1.0, [0.0, 0.0, 0.0], 7, 0);
        let big = popup_value_cells(&vp, 1.0, [0.0, 0.0, 0.0], 7, 20);
        assert_eq!(small.len(), 1);
        assert!(big[0].w > small[0].w);
        assert!(popup_value_cells(&vp, 1.0, [0.0, 0.0, 0.0], 7, 37).is_empty());
    }

    /// The numeral rises. Under a real battle draw matrix - the camera
    /// `battle_gimard_tail_fire_b` holds (pitch `64`, yaw `2620`, `TR (0,
    /// 1024, 3283)`, focus `(125, 0, -642)`) - a hit on the struck member's
    /// display trio `(144, -112, -1173)` projects above the floor under it
    /// from the first frame and has reached the resting row by the ring
    /// timer the capture holds (`352`, age `21`), which is where the
    /// capture's display list seats the run.
    #[test]
    fn a_popup_rises_under_a_battle_draw_matrix() {
        use legaia_engine_camera::projection::{BattleCamPose, battle_vp};
        let pose = BattleCamPose {
            pitch: 64.0,
            yaw: 2620.0,
            tr: [0.0, 1024.0, 3283.0],
            focus: [125.0, 0.0, -642.0],
        };
        // The retail battle world scale the actor stage is drawn at.
        const SCALE: f32 = 4.0;
        let vp = battle_vp(&pose, SCALE, 4.0 / 3.0);
        let trio = [144.0, -112.0, -1173.0];
        let row = |age: u16| popup_value_cells(&vp, SCALE, trio, 16, age)[0].y;
        let floor = popup_square(&vp, SCALE, [144, 0, -1173], 1).unwrap().0.1;
        assert!(row(0) < floor, "the push frame sits over the floor");
        assert!(row(8) < row(0), "and climbs: {} then {}", row(0), row(8));
        assert_eq!(row(21), vr::RESTING_TOP_Y);
        assert_eq!(popup_value_cells(&vp, SCALE, trio, 16, 21).len(), 2);
    }

    /// The square's depth is the anchor's view-space `Z`, the `MAC3` the
    /// projection's ordering-table return is made of.
    #[test]
    fn the_square_depth_is_the_anchors_view_z() {
        use legaia_engine_camera::projection::{BattleCamPose, battle_view_depth, battle_vp};
        let pose = BattleCamPose {
            pitch: 64.0,
            yaw: 2620.0,
            tr: [0.0, 1024.0, 3283.0],
            focus: [125.0, 0.0, -642.0],
        };
        let vp = battle_vp(&pose, 4.0, 4.0 / 3.0);
        let anchor = [144, -112, -1173];
        let (_, sz) = popup_square(&vp, 4.0, anchor, 16).unwrap();
        let want = battle_view_depth(&pose, 4.0, anchor.map(|v| v as f32));
        assert!((sz as f32 - want).abs() <= 1.0, "{sz} against {want}");
    }

    /// Under a close camera the square keeps its near corner and is laid out
    /// `0x20` pixels from it (`FUN_801DF6B8`, `0x801DF820..0x801DF854`).
    #[test]
    fn a_near_square_is_laid_out_from_its_near_corner() {
        let mut near = [0.0f32; 16];
        near[0] = 0.5;
        near[5] = 0.5;
        near[15] = 1000.0;
        let (rect, sz) = popup_square(&near, 1.0, [0, 0, 0], 400).unwrap();
        assert!(vr::popup_ot_depth(sz) < vr::POPUP_NEAR_OT_DEPTH);
        let seated = vr::popup_near_square(rect, vr::popup_ot_depth(sz));
        assert_eq!((seated.0, seated.1), (rect.0, rect.1));
        assert_eq!((seated.2 - seated.0, seated.3 - seated.1), (0x20, 0x20));
    }

    /// The numeral's size is the ring timer's, in view units.
    /// `battle_melee_hit_spark` holds two frames of one `15` in its packet
    /// arenas - cells `18` wide and then `22` wide (corner spans; the kernel's
    /// `w` is the span plus one), the later one on the resting row - over
    /// Gimard's display trio `(-2, -461, 492)` under pitch `0`, yaw `2925`,
    /// `TR (0, 1298, 3424)`, focus `(-2, 0, 389)`. Those are the spans of
    /// ring timers `224` and `288` with the half-extent unscaled.
    #[test]
    fn a_popup_grows_at_the_ring_timers_view_space_rate() {
        use legaia_engine_camera::projection::{BattleCamPose, battle_vp};
        let pose = BattleCamPose {
            pitch: 0.0,
            yaw: 2925.0,
            tr: [0.0, 1298.0, 3424.0],
            focus: [-2.0, 0.0, 389.0],
        };
        const SCALE: f32 = 4.0;
        let vp = battle_vp(&pose, SCALE, 4.0 / 3.0);
        let cells = |age: u16| popup_value_cells(&vp, SCALE, [-2.0, -461.0, 492.0], 15, age);
        assert_eq!(vr::popup_timer(13), 224);
        assert_eq!(vr::popup_timer(17), 288);
        let (early, late) = (cells(13), cells(17));
        assert_eq!((early[0].w, early[0].h), (19, 19));
        assert_eq!((late[0].w, late[0].h), (23, 23));
        assert_eq!(late[0].y, vr::RESTING_TOP_Y);
        // One pitch apart, the units digit to the right.
        assert_eq!(late[1].x - late[0].x, 23);
    }

    fn quad(p: &ScreenPrim) -> ScreenQuad {
        match p {
            ScreenPrim::Textured(q) => *q,
            ScreenPrim::Flat(_) => panic!("the readout emits textured quads only"),
        }
    }

    /// The popup shares the HUD text's bucket (so it stays under it), the
    /// value window and the Arts banner link one in front of it (so they
    /// cover it), and every one of them draws over the cinematic bars, the
    /// weapon trails and the move-FX streak.
    #[test]
    fn the_readout_buckets_are_the_add_prim_slots() {
        use legaia_render_kernels::screen_prim::{TEXT_OT_BUCKET, push_covers_text};
        assert_eq!(POPUP_OT, TEXT_OT_BUCKET as u32);
        assert!(!push_covers_text(POPUP_OT as i16));
        assert!(push_covers_text(COMBO_OT as i16));
        assert!(push_covers_text(ARTS_BANNER_OT as i16));
        const {
            assert!(POPUP_OT < legaia_render_kernels::screen_prim::CINEMATIC_BAR_OT);
            assert!(POPUP_OT < legaia_render_kernels::battle_trail::WEAPON_TRAIL_OT);
            assert!(POPUP_OT < legaia_render_kernels::streak_pass::MOVE_FX_STREAK_OT);
        }
        // The tag's `0x09` is the packet's length in words.
        assert_eq!(vr::OT_TAG >> 24, (vr::PRIM_BYTES as u32 - 4) / 4);
    }

    /// Every quad names the battle effect atlas's glyph page through its CLUT,
    /// carries the neutral modulation word, and is opaque. A tint here would
    /// double-apply the sheet's own gold ramp; a semi-transparency flag would
    /// put a blend equation under art that has none.
    #[test]
    fn quads_are_opaque_neutral_glyph_page_samples() {
        let cells = vr::value_cells(250, 160, 120, vr::POP_FRAMES);
        for p in digit_run_prims(&cells, POPUP_OT) {
            let q = quad(&p);
            assert_eq!(q.tpage, GLYPH_TPAGE);
            assert_eq!(q.clut, GLYPH_CLUT);
            assert_eq!(q.color, READOUT_COLOUR);
            assert!(!q.semi_transparent);
            assert_eq!(q.ot_index, POPUP_OT);
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
        let prims = combo_cluster_prims(&cluster, COMBO_OT);
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

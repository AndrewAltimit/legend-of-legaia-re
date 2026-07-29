//! The field-to-battle intro emitter, from a captured frame to drawn pixels.
//!
//! The claim under test is the one the whole lane rests on: that a battle can
//! now **open with its transition** rather than cutting straight in. That is
//! not a statement about a kernel composing - `battle_intro_chain.rs` in
//! `engine-vm` already pins that - it is a statement about pixels, so the
//! end-to-end case here drives the real overlay pipeline on a GPU and reads
//! the drawn frame back.
//!
//! The identity frame is what makes it checkable. The curtain's row warp is
//! `y = (row - 120) * (clock + 28) / 28 + 120`, which at `clock == 0` is the
//! identity - so the very first transition frame must reproduce the captured
//! field frame **pixel for pixel**, modulo the one thing the style does to
//! every strip unconditionally (see [`drawn_channel`]). Anything wrong in
//! the chain (the capture rect, the texture-page decode, the UV per strip, the
//! two-halves split at `0xC0`, the ordering, the geometry space) breaks that
//! equality. Later frames must then differ, or the warp is not advancing.
//!
//! Disc-free by construction: the "field frame" is a synthetic gradient and
//! the descriptor table is [`IntroQuadTable::neutral`]. The disc-gated
//! companion that parses the real table lives in
//! `crates/engine-render/tests/battle_intro_table_real.rs`.

use super::*;
use crate::battle_intro::{BattleIntro, IntroQuadTable};
use crate::screen_overlay::ScreenPrim;
use crate::tests::screen_overlay_gpu::{build_harness_vram, headless_device, render_frame_rgba};
use crate::vram_capture::{
    CaptureOpts, FIELD_CAPTURE_COLS, FIELD_CAPTURE_ROWS, PSX_SCREEN_HEIGHT, PSX_SCREEN_WIDTH,
    blit_rgba_into_vram,
};
use legaia_engine_vm::battle_intro_particles::ParticleEnv;
use legaia_engine_vm::battle_intro_styles::{CURTAIN_COLS, CURTAIN_ROWS, IntroStyle};
use legaia_engine_vm::battle_intro_swirl::SwirlTrig;
use legaia_tim::Vram;

/// Deterministic stand-in for the disc's trig tables, sqrt and PRNG - the same
/// shape `engine-vm`'s `battle_intro_chain.rs` uses, kept synthetic so this
/// file needs no disc.
struct Env {
    lcg: u32,
}

impl Env {
    fn new() -> Self {
        Self { lcg: 0x1234_5678 }
    }
    fn sin_q12(units: i32) -> i16 {
        let r = (units.rem_euclid(0x1000) as f64) * std::f64::consts::TAU / 4096.0;
        (r.sin() * 4096.0) as i16
    }
}

impl ParticleEnv for Env {
    fn heading(&mut self, x: i32, z: i32) -> i32 {
        let a = (z as f64).atan2(x as f64);
        ((a * 4096.0 / std::f64::consts::TAU) as i32).rem_euclid(0x1000)
    }
    fn sin(&mut self, h: i32) -> i16 {
        Self::sin_q12(h)
    }
    fn cos(&mut self, h: i32) -> i16 {
        Self::sin_q12(h + 0x400)
    }
    fn sqrt(&mut self, v: i32) -> i32 {
        if v <= 0 { 0 } else { (v as f64).sqrt() as i32 }
    }
    fn rand(&mut self) -> i32 {
        self.lcg = self.lcg.wrapping_mul(0x0001_9660).wrapping_add(0x3C6E_F35F);
        ((self.lcg >> 16) & 0x7FFF) as i32
    }
}

impl SwirlTrig for Env {
    fn table_x(&mut self, e: i32) -> i16 {
        Env::sin_q12(e + 0x400)
    }
    fn table_y(&mut self, e: i32) -> i16 {
        Env::sin_q12(e)
    }
}

fn intro(style: IntroStyle, total: i32) -> BattleIntro {
    let mut env = Env::new();
    let mut trig = Env::new();
    BattleIntro::new(
        style,
        0,
        total,
        IntroQuadTable::neutral(),
        &mut env,
        &mut trig,
        [0, 1, 0x11, 0x12],
    )
}

/// The 5-bit source channel of the synthetic field frame at `(x, y)`.
///
/// A high-frequency ramp on all three channels: red walks with x, green with
/// y, blue with their sum, each stepping one 5-bit level per pixel. A strip
/// drawn one row or one column off its source is therefore wrong in a whole
/// level, not in a rounding bit.
fn source_c5(x: usize, y: usize) -> [u8; 3] {
    [(x % 32) as u8, (y % 32) as u8, ((x + y) % 32) as u8]
}

/// The synthetic field frame, as RGBA8 with every channel exactly
/// representable in 5 bits - so the 24->15 quantisation on the way into VRAM
/// is lossless and nothing downstream can blame rounding.
fn field_frame() -> Vec<u8> {
    let (w, h) = (PSX_SCREEN_WIDTH as usize, PSX_SCREEN_HEIGHT as usize);
    let expand = |c5: u8| (c5 << 3) | (c5 >> 2);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let c = source_c5(x, y);
            rgba.extend_from_slice(&[expand(c[0]), expand(c[1]), expand(c[2]), 0xFF]);
        }
    }
    rgba
}

/// What the overlay pipeline must put on screen for a 5-bit source channel
/// carried by a curtain strip.
///
/// Two transforms compose, and both are the port reproducing hardware rather
/// than slack to be tolerated:
///
/// * **The style's modulation.** Every strip is drawn with `param_5 = 0x80`
///   ([`legaia_engine_vm::battle_intro_styles::CURTAIN_INTENSITY`]) and the
///   disc's descriptor records carry `0xFF` on both edges, so
///   `build_intro_quad`'s `c * intensity / 256` shades white to `0x7F` - and
///   PSX texture modulation is `texel * colour / 128`. A curtain strip is
///   therefore one 128th darker than the frame it was cut from, on hardware
///   as here.
/// * **The 5-bit expansion.** The shader widens a VRAM channel as `c5 / 31`,
///   the exact rescale, before the modulation and the UNORM8 write.
///
/// The comparison below allows a **one-step** difference per channel and no
/// more. That is not slack for the wiring: it covers only the UNORM8 write's
/// rounding, where one source level (`c5 = 28`) lands 0.02 above a `.5`
/// boundary and the GPU's `f32` chain settles just below it. The frame steps
/// one 5-bit level per pixel, so a strip drawn one row or one column off its
/// source is wrong by about **eight** 8-bit steps - far outside the bound.
fn drawn_channel(c5: u8) -> u8 {
    ((c5 as f32 / 31.0) * (127.0 / 128.0) * 255.0).round() as u8
}

/// [`field_frame`] as the curtain must redraw it.
fn expected_frame() -> Vec<u8> {
    let (w, h) = (PSX_SCREEN_WIDTH as usize, PSX_SCREEN_HEIGHT as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let c = source_c5(x, y);
            rgba.extend_from_slice(&[
                drawn_channel(c[0]),
                drawn_channel(c[1]),
                drawn_channel(c[2]),
                0xFF,
            ]);
        }
    }
    rgba
}

fn vram_with_capture() -> Vram {
    let mut vram = Vram::new();
    let frame = field_frame();
    for rect in [FIELD_CAPTURE_ROWS, FIELD_CAPTURE_COLS] {
        blit_rgba_into_vram(
            &frame,
            PSX_SCREEN_WIDTH as u32,
            PSX_SCREEN_HEIGHT as u32,
            &mut vram,
            rect,
            CaptureOpts { set_mask_bit: true },
        );
    }
    vram
}

// ---------------------------------------------------------------------------
// CPU-side shape
// ---------------------------------------------------------------------------

#[test]
fn the_curtain_emits_two_strips_per_scanline_plus_the_visible_columns() {
    let mut it = intro(IntroStyle::Curtain, 200);
    let frame = it.tick(0, 1);
    assert!(frame.style_drawn);
    // Every scanline is drawn in two halves, and at clock zero no column is
    // culled yet.
    assert_eq!(
        frame.prims.len(),
        (2 * CURTAIN_ROWS + CURTAIN_COLS) as usize,
        "480 row strips + 320 column strips"
    );
    // No fade this early: style 3's ramp only starts 0x40 frames from the end.
    assert!(frame.fade.is_none());
}

#[test]
fn the_row_strips_are_the_identity_on_the_first_frame() {
    let mut it = intro(IntroStyle::Curtain, 200);
    let frame = it.tick(0, 1);
    // The row pass emits first, two quads per scanline: left at x 0 width
    // 0xC0, right at x 0xC0 width 0x80.
    for row in 0..CURTAIN_ROWS as usize {
        let ScreenPrim::Textured(l) = frame.prims[row * 2] else {
            panic!("row {row} left half is not textured")
        };
        let ScreenPrim::Textured(r) = frame.prims[row * 2 + 1] else {
            panic!("row {row} right half is not textured")
        };
        assert_eq!(l.xy[0], (0, row as i16));
        assert_eq!(r.xy[0], (0xC0, row as i16));
        assert_eq!(l.xy[1].0 - l.xy[0].0, 0xC0, "left half width");
        assert_eq!(r.xy[1].0 - r.xy[0].0, 0x80, "right half width");
        // Each strip samples its own scanline of the capture.
        assert_eq!(l.uv[0], (0, row as u8));
    }
}

#[test]
fn the_rows_pull_apart_from_the_screen_centre_as_the_clock_runs() {
    let mut it = intro(IntroStyle::Curtain, 400);
    let first = it.tick(0, 1);
    let later = it.tick(28, 1);
    let y = |f: &crate::battle_intro::IntroFrame, row: usize| match f.prims[row * 2] {
        ScreenPrim::Textured(q) => q.xy[0].1,
        _ => panic!(),
    };
    assert_eq!(y(&first, 0), 0);
    // (0 - 120) * (28 + 28) / 28 + 120 == -120: the top scanline has left the
    // screen, which is the curtain opening.
    assert_eq!(y(&later, 0), -120);
    // The centre scanline is the pivot and never moves.
    assert_eq!(y(&first, 120), 120);
    assert_eq!(y(&later, 120), 120);
}

#[test]
fn the_column_pass_draws_outside_the_display_and_that_is_retails() {
    // `FUN_801D11D0` tests a column's visibility against `warped + 0xA0` in
    // `0..0x140` and then draws it at `warped + 0x1E0` (`0x801D1438` and the
    // delay slot at `0x801D1454`). The two biases differ by exactly one screen
    // width, so every column that passes the test is drawn 320 pixels to the
    // right of where it was tested - off a 320-wide display entirely.
    //
    // Both constants are read off the disassembly and both are reproduced, so
    // this is not a transcription slip to be "corrected" by inventing a -320.
    // Pinned so that a future change has to argue with the disassembly rather
    // than with a silent expectation. The row pass is the half that is on
    // screen, and it is the half the pixel test below checks.
    let mut it = intro(IntroStyle::Curtain, 200);
    let frame = it.tick(0, 1);
    let cols: Vec<_> = frame.prims[2 * CURTAIN_ROWS as usize..]
        .iter()
        .filter_map(|p| match p {
            ScreenPrim::Textured(q) => Some(q.xy[0].0),
            _ => None,
        })
        .collect();
    assert_eq!(cols.len(), CURTAIN_COLS as usize);
    assert!(
        cols.iter().all(|&x| x >= PSX_SCREEN_WIDTH as i16),
        "every column strip is at or past the right screen edge"
    );
}

#[test]
fn the_fade_rides_the_same_clock_and_arrives_as_a_full_screen_quad() {
    let total = 200;
    let mut it = intro(IntroStyle::Curtain, total);
    // Style 3's lead is 0x40.
    assert!(it.tick(total as i16 - 0x40, 1).fade.is_none());
    let f = it.tick(total as i16 - 0x40 + 1, 1);
    let fade = f.fade.expect("the ramp has started");
    assert_eq!(fade.level, 4);
    // The last primitive is the fade quad: full display rect, blended.
    let ScreenPrim::Flat(q) = f.prims.last().copied().unwrap() else {
        panic!("the fade is not a flat quad")
    };
    assert_eq!(q.xy[0], (0, 0));
    assert_eq!(q.xy[3], (PSX_SCREEN_WIDTH as i16, PSX_SCREEN_HEIGHT as i16));
    assert!(q.semi_transparent);
    assert_eq!(q.color[..3], [4, 4, 4]);
}

#[test]
fn the_three_unported_styles_tick_their_working_set_and_draw_nothing() {
    // Not a wire and not claimed as one: their retail packet builders are not
    // ported. What they must still do is advance, because the fade and the
    // handoff both ride the same clock - so a battle opened on any of them
    // still fades and still hands off on the retail frame.
    for style in [
        IntroStyle::ScatterParticles,
        IntroStyle::SpinUpParticles,
        IntroStyle::Swirl,
    ] {
        let total = 100;
        let mut it = intro(style, total);
        let early = it.tick(0, 1);
        assert!(!early.style_drawn, "{style:?} must not claim a draw");
        assert!(early.prims.is_empty(), "{style:?} emitted geometry");
        // The fade still arrives on schedule.
        let lead = match style {
            IntroStyle::Swirl => 0x20,
            _ => 0x18,
        };
        let late = it.tick(total as i16 - lead + 1, 1);
        assert!(
            late.fade.is_some(),
            "{style:?} lost its fade because the clock did not reach it"
        );
        assert_eq!(late.prims.len(), 1, "{style:?}: the fade quad only");
    }
}

// ---------------------------------------------------------------------------
// The tile shatter's emitter
// ---------------------------------------------------------------------------

#[test]
fn the_tile_shatter_skips_its_first_frame_and_draws_from_the_second() {
    // Retail's first shatter frame projects through the field camera's stale
    // view matrix and every tile lands behind the near plane (pinned live);
    // the emitter reproduces the outcome by gating on the same
    // `not_first_frame` signal the retail tick derives.
    let mut it = intro(IntroStyle::TileShatter, 200);
    let first = it.tick(0, 1);
    assert!(!first.style_drawn, "frame one draws no tiles in retail");
    assert!(first.prims.is_empty());

    let second = it.tick(1, 1);
    assert!(second.style_drawn);
    // 256 tiles, ten faces each, minus NCLIP rejects - at the seeded pose
    // every front face survives, so at minimum the full 16x16 sheet draws.
    assert!(
        second.prims.len() >= 256,
        "{} prims for 256 tiles",
        second.prims.len()
    );
}

#[test]
fn the_seeded_sheet_projects_to_the_retail_screen_rect() {
    // The projection constants under test are the pinned trio OFX=160,
    // OFY=114, H=0x80, *and* the seeder's deliberate stored-vs-pivot offset:
    // a tile's corners are made relative to a pivot 0xA0 *below* the stored
    // position, so the whole sheet projects 10 px lower than the raw lattice
    // would. Lattice x -0xA00..0xA00 at view z 0x800 maps to x 0..320;
    // lattice y -0x800..0x800 plus the 0xA0 lift maps to y -4..252 - the
    // OFY=114 six-up and the pivot ten-down nearly cancelling is what centres
    // the retail sheet on the display.
    let mut it = intro(IntroStyle::TileShatter, 200);
    it.tick(0, 1);
    let f = it.tick(1, 1);
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (i16::MAX, i16::MIN, i16::MAX, i16::MIN);
    for p in &f.prims {
        let ScreenPrim::Textured(q) = p else { continue };
        for &(x, y) in &q.xy {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    // The UNR reciprocal tracks the exact divide to within one step.
    assert!(min_x.abs() <= 1, "left edge at {min_x}");
    assert!((max_x - 320).abs() <= 1, "right edge at {max_x}");
    assert!((min_y - -4).abs() <= 1, "top edge at {min_y}");
    assert!((max_y - 252).abs() <= 1, "bottom edge at {max_y}");
}

#[test]
fn the_shade_faces_carry_the_three_literals_and_blend_additively() {
    use legaia_engine_vm::battle_intro_tiles::{SHADE_CLUT, SHADE_TPAGE, SHADE_UVS};
    let mut it = intro(IntroStyle::TileShatter, 200);
    it.tick(0, 1);
    let f = it.tick(1, 1);
    let shade: Vec<_> = f
        .prims
        .iter()
        .filter_map(|p| match p {
            ScreenPrim::Textured(q) if q.tpage == SHADE_TPAGE => Some(q),
            _ => None,
        })
        .collect();
    assert!(!shade.is_empty(), "no shade faces drawn");
    for q in &shade {
        assert_eq!(q.clut, SHADE_CLUT);
        assert_eq!(q.uv, SHADE_UVS);
        assert!(q.semi_transparent);
        // tpage 0x0027 bits 5..=6 = 01: ABR mode 1, additive - the glint adds
        // over the opaque side underneath it.
        assert_eq!(q.abr_mode(), 1);
    }
}

#[test]
fn a_shade_face_draws_on_top_of_its_opaque_sibling() {
    use crate::screen_overlay::order_primitives;
    use legaia_engine_vm::battle_intro_tiles::SHADE_TPAGE;
    let mut it = intro(IntroStyle::TileShatter, 200);
    it.tick(0, 1);
    let f = it.tick(1, 1);
    // Find a shade face and the opaque record face with identical corners
    // (packet rows 0 and 5 share the corner table row). Same corners = same
    // AVSZ4 = same OT bucket, so only the tie-break separates them.
    let order = order_primitives(&f.prims);
    let mut pos_of = vec![0usize; f.prims.len()];
    for (draw_pos, &idx) in order.iter().enumerate() {
        pos_of[idx] = draw_pos;
    }
    let mut checked = 0;
    for (i, p) in f.prims.iter().enumerate() {
        let ScreenPrim::Textured(q) = p else { continue };
        if q.tpage != SHADE_TPAGE {
            continue;
        }
        for (j, p2) in f.prims.iter().enumerate() {
            let ScreenPrim::Textured(q2) = p2 else {
                continue;
            };
            if q2.tpage == SHADE_TPAGE || q2.xy != q.xy || q2.ot_index != q.ot_index {
                continue;
            }
            assert!(
                pos_of[i] > pos_of[j],
                "shade prim {i} must draw after opaque sibling {j}"
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no shade/opaque sibling pair found");
}

#[test]
fn the_euler_kernel_matches_the_cardinal_rotations() {
    use crate::battle_intro::euler_rot_psx;
    // FUN_80026988 composes Rx * Ry * Rz. At the cardinals the q12
    // truncation is exact, so the axis matrices come out clean.
    let id = euler_rot_psx((0, 0, 0));
    assert_eq!(id.m, [[4096, 0, 0], [0, 4096, 0], [0, 0, 4096]]);
    // Pure Z quarter turn: [c -s 0; s c 0; 0 0 1].
    let rz = euler_rot_psx((0, 0, 0x400));
    assert_eq!(rz.m, [[0, -4096, 0], [4096, 0, 0], [0, 0, 4096]]);
    // Pure Y quarter turn: [c 0 s; 0 1 0; -s 0 c].
    let ry = euler_rot_psx((0, 0x400, 0));
    assert_eq!(ry.m, [[0, 0, 4096], [0, 4096, 0], [-4096, 0, 0]]);
    // Pure X quarter turn: [1 0 0; 0 c -s; 0 s c].
    let rx = euler_rot_psx((0x400, 0, 0));
    assert_eq!(rx.m, [[4096, 0, 0], [0, 0, -4096], [0, 4096, 0]]);
    // Angles fold at 12 bits, exactly the `& 0xFFF` masks in the kernel.
    assert_eq!(euler_rot_psx((0x1400, 0, 0)).m, rx.m);
    // A mixed rotation agrees with the generic q12 product to within the
    // kernel's own per-term truncation (each element one step at most).
    let m = euler_rot_psx((0x100, 0x200, 0x300));
    let composed = crate::billboard::rot_z_psx(0); // identity in q12
    let _ = composed;
    let rx = euler_rot_psx((0x100, 0, 0));
    let ry = euler_rot_psx((0, 0x200, 0));
    let rz = euler_rot_psx((0, 0, 0x300));
    let want = rx.mul(&ry).mul(&rz);
    for r in 0..3 {
        for c in 0..3 {
            assert!(
                (i32::from(m.m[r][c]) - i32::from(want.m[r][c])).abs() <= 2,
                "element ({r},{c}): {} vs {}",
                m.m[r][c],
                want.m[r][c]
            );
        }
    }
}

/// A tile with the seeded local shape at an arbitrary `pos.z`.
fn boxy_tile(pos_z: i16) -> legaia_engine_vm::battle_intro_tiles::TileRecord {
    use legaia_engine_vm::battle_intro_tiles::TileRecord;
    let mut rec = TileRecord::default();
    for k in 0..4 {
        let (x, y) = [(-0xA0, -0xA0), (0xA0, -0xA0), (-0xA0, 0xA0), (0xA0, 0xA0)][k];
        rec.front[k].x = x;
        rec.front[k].y = y;
        rec.front[k].z = -0x80;
        rec.back[k].x = x;
        rec.back[k].y = y;
        rec.back[k].z = 0x80;
    }
    rec.pos = (0, 0, pos_z);
    rec
}

#[test]
fn the_backface_cull_is_single_sided() {
    use crate::battle_intro::emit_tile;
    // At the resting pose the front face passes NCLIP and the back face -
    // whose packet row reverses the corner order - rejects. Front is the one
    // 0x80-grey opaque quad; back would be the 0x20-grey one.
    let mut prims = Vec::new();
    emit_tile(&boxy_tile(0x880), &mut prims);
    let greys: Vec<u32> = prims
        .iter()
        .filter_map(|p| match p {
            ScreenPrim::Textured(q) if !q.semi_transparent => Some(q.color & 0xFF),
            _ => None,
        })
        .collect();
    assert!(greys.contains(&0x80), "the front face draws");
    assert!(!greys.contains(&0x20), "the back face culls at rest");
}

#[test]
fn the_near_cutoff_drops_a_face_hugging_the_camera() {
    use crate::battle_intro::emit_tile;
    // Control: at the seeded depth the front face (OTZ 0x800) draws.
    let mut far = Vec::new();
    emit_tile(&boxy_tile(0x880), &mut far);
    let front = |prims: &[ScreenPrim]| {
        prims.iter().any(|p| match p {
            ScreenPrim::Textured(q) => !q.semi_transparent && q.color & 0xFF == 0x80,
            _ => false,
        })
    };
    assert!(front(&far));
    // Hugging the camera: pos.z = 0x8C puts the front corners at view z 0xC,
    // whose AVSZ4 average lands below the 0x10 cutoff - the face is dropped,
    // and nothing that survives may sit below the cutoff either.
    let mut near = Vec::new();
    emit_tile(&boxy_tile(0x8C), &mut near);
    assert!(!front(&near), "the front face must fall to the near cutoff");
    for p in &near {
        assert!(p.ot_index() >= 0x10);
    }
}

#[test]
fn the_capture_is_a_one_shot() {
    let mut it = intro(IntroStyle::Curtain, 200);
    assert!(it.needs_capture());
    // Ticking must not consume it - the capture happens once, when the host
    // still has a field frame to capture, and every later frame samples it.
    it.tick(0, 1);
    it.tick(1, 1);
    assert!(it.needs_capture());
}

#[test]
fn the_tile_sheet_covers_the_display_on_the_gpu() {
    // End-to-end for the shatter's draw path: real overlay pipeline, real
    // 4bpp CLUT sampling of the shade page, on a headless device. At clock 1
    // every tile still rests on the grid plane, so the opaque front faces
    // tile the whole display - clear to magenta and require that almost none
    // of it survives. (The curtain's identity-frame twin below pins exact
    // pixels; a resting tile sheet is not pixel-identity - the seeder's 0xA0
    // pivot offset shifts it - so this case pins coverage, not equality.)
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping the tile-shatter end-to-end frame");
        return;
    };
    let mut vram = vram_with_capture();
    // A synthetic shade page at (448,0): texel index = row nibble, and the
    // (16,473) CLUT strip as a grey ramp - enough for the additive side
    // faces to sample real data through the real decode.
    let page_rows: Vec<u8> = (0..64u16)
        .flat_map(|row| {
            let n = (row % 16) as u8;
            std::iter::repeat_n(n | n << 4, 32)
        })
        .collect();
    vram.write_block(448, 0, 16, 64, &page_rows);
    let clut: Vec<u8> = (0..16u16)
        .flat_map(|i| (0x8000u16 | i << 10 | i << 5 | i).to_le_bytes())
        .collect();
    vram.write_clut_row(16, 473, &clut);

    let h = build_harness_vram(
        device,
        queue,
        &vram,
        (PSX_SCREEN_WIDTH as u32, PSX_SCREEN_HEIGHT as u32),
    );
    let mut it = intro(IntroStyle::TileShatter, 200);
    it.tick(0, 1);
    let f = it.tick(1, 1);
    assert!(f.style_drawn);
    let drawn = render_frame_rgba(&h, &f.prims, [1.0, 0.0, 1.0, 1.0]);
    let (w, hpx) = (PSX_SCREEN_WIDTH as usize, PSX_SCREEN_HEIGHT as usize);
    let mut magenta = 0usize;
    let mut black = 0usize;
    for px in drawn.chunks_exact(4) {
        if px[0] > 200 && px[1] < 50 && px[2] > 200 {
            magenta += 1;
        }
        if px[0] == 0 && px[1] == 0 && px[2] == 0 {
            black += 1;
        }
    }
    let total = w * hpx;
    assert!(
        magenta * 20 < total,
        "{magenta} of {total} pixels undrawn - the sheet is not covering the display"
    );
    assert!(
        black * 2 < total,
        "{black} of {total} pixels black - the tiles drew but sampled nothing"
    );
}

// ---------------------------------------------------------------------------
// End to end on the GPU
// ---------------------------------------------------------------------------

/// Both end-to-end frames, driven through **one** GPU device.
///
/// Deliberately a single test rather than two: each case needs the whole
/// 1024x512 VRAM page and a 320x240 colour target, and two `headless_device()`
/// adapters holding that at once takes the driver down. One device, one
/// harness, both frames.
#[test]
fn the_curtain_redraws_the_captured_field_frame_and_then_opens_it() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping the battle-intro end-to-end frames");
        return;
    };
    let vram = vram_with_capture();
    let h = build_harness_vram(
        device,
        queue,
        &vram,
        (PSX_SCREEN_WIDTH as u32, PSX_SCREEN_HEIGHT as u32),
    );
    let (w, h_px) = (PSX_SCREEN_WIDTH as usize, PSX_SCREEN_HEIGHT as usize);
    let source = expected_frame();
    let mut it = intro(IntroStyle::Curtain, 400);

    // --- Frame 1: the warp is the identity, so the transition's first frame
    // must be the field frame it captured. Clear to magenta so an undrawn
    // pixel is unmistakable.
    let drawn = render_frame_rgba(&h, &it.tick(0, 1).prims, [1.0, 0.0, 1.0, 1.0]);
    assert_eq!(drawn.len(), w * h_px * 4);

    let mut worst = 0i32;
    let mut worst_at: Option<(usize, usize, [u8; 3], [u8; 3])> = None;
    let mut off_by_one = 0usize;
    for y in 0..h_px {
        for x in 0..w {
            let o = (y * w + x) * 4;
            let got = [drawn[o], drawn[o + 1], drawn[o + 2]];
            let want = [source[o], source[o + 1], source[o + 2]];
            let d = (0..3)
                .map(|i| (got[i] as i32 - want[i] as i32).abs())
                .max()
                .unwrap();
            if d > 0 {
                off_by_one += 1;
            }
            if d > worst {
                worst = d;
                worst_at = Some((x, y, got, want));
            }
        }
    }
    assert!(
        worst <= 1,
        "a pixel is off by {worst} steps at {worst_at:?} - that is a misplaced \
         strip, not the UNORM8 rounding"
    );
    assert!(
        off_by_one * 8 < w * h_px,
        "{off_by_one} of {} pixels differ; too many for the single boundary level",
        w * h_px
    );

    // --- Frame 2: at clock 28 the warp doubles the scanline spread about
    // y = 120, so most destination rows lose their strip and keep the clear
    // colour. If this frame still matched, the curtain would not be opening.
    let opened = render_frame_rgba(&h, &it.tick(28, 1).prims, [1.0, 0.0, 1.0, 1.0]);
    let differing = (0..h_px * w)
        .filter(|&i| {
            let o = i * 4;
            [opened[o], opened[o + 1], opened[o + 2]] != [source[o], source[o + 1], source[o + 2]]
        })
        .count();
    assert!(
        differing > w * h_px / 4,
        "only {differing} pixels moved; the curtain is not opening"
    );
    // The pivot scanline is the one place the warp stays the identity, so it
    // must still carry its own row of the capture.
    let o = (120 * w + 10) * 4;
    assert!(
        (0..3).all(|i| (opened[o + i] as i32 - source[o + i] as i32).abs() <= 1),
        "the pivot scanline moved: {:?} vs {:?}",
        &opened[o..o + 3],
        &source[o..o + 3]
    );
}

/// The capture must not overwrite a texture page the style itself samples.
///
/// `FIELD_CAPTURE_ROWS` spans VRAM `320..640` x `0..240`, which contains the
/// tile shatter's 4bpp side-face shade page at `(448, 0)`. Blitting the rows
/// rect for that style destroys an input it depends on and gains it nothing -
/// its own pages (`0x135` / `0x137`) are both inside the columns rect. The
/// curtain does sample the rows rect, so it must keep both.
mod capture_rects {
    use crate::battle_intro::{TILE_SHADE_PAGE, capture_rects_for};
    use crate::vram_capture::{FIELD_CAPTURE_COLS, FIELD_CAPTURE_ROWS, VramRect};
    use legaia_engine_vm::battle_intro_styles::IntroStyle;

    /// Halfword-rect overlap, the relation that makes the clobber a clobber.
    fn overlaps(a: VramRect, b: VramRect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    #[test]
    fn the_rows_rect_really_does_cover_the_shade_page() {
        // If this ever stops being true the whole guard below is vacuous.
        assert!(
            overlaps(FIELD_CAPTURE_ROWS, TILE_SHADE_PAGE),
            "the premise of the fix: rows {FIELD_CAPTURE_ROWS:?} covers the \
             shade page {TILE_SHADE_PAGE:?}"
        );
        // ...and the columns rect does not, which is why columns-only is safe.
        assert!(!overlaps(FIELD_CAPTURE_COLS, TILE_SHADE_PAGE));
    }

    #[test]
    fn tile_shatter_captures_columns_only() {
        let rects = capture_rects_for(IntroStyle::TileShatter);
        assert_eq!(rects, [FIELD_CAPTURE_COLS]);
        assert!(
            !rects.iter().any(|r| overlaps(*r, TILE_SHADE_PAGE)),
            "the tile style must not blit over its own shade page"
        );
    }

    #[test]
    fn the_curtain_keeps_both_rects() {
        // Its row pass samples pages at (320, 0) / (512, 0); dropping the rows
        // rect would leave that pass reading whatever the scene left there.
        assert_eq!(
            capture_rects_for(IntroStyle::Curtain),
            [FIELD_CAPTURE_ROWS, FIELD_CAPTURE_COLS]
        );
    }

    #[test]
    fn styles_with_unestablished_sampling_stay_conservative() {
        for s in [
            IntroStyle::ScatterParticles,
            IntroStyle::SpinUpParticles,
            IntroStyle::Swirl,
        ] {
            assert_eq!(
                capture_rects_for(s),
                [FIELD_CAPTURE_ROWS, FIELD_CAPTURE_COLS],
                "{s:?} sampling is not established; keep both rects"
            );
        }
    }
}

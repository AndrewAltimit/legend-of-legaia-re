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
fn the_four_unported_styles_tick_their_working_set_and_draw_nothing() {
    // Not a wire and not claimed as one: their retail packet builders are not
    // ported. What they must still do is advance, because the fade and the
    // handoff both ride the same clock - so a battle opened on any of them
    // still fades and still hands off on the retail frame.
    for style in [
        IntroStyle::ScatterParticles,
        IntroStyle::SpinUpParticles,
        IntroStyle::TileShatter,
        IntroStyle::Swirl,
    ] {
        let total = 100;
        let mut it = intro(style, total);
        let early = it.tick(0, 1);
        assert!(!early.style_drawn, "{style:?} must not claim a draw");
        assert!(early.prims.is_empty(), "{style:?} emitted geometry");
        // The fade still arrives on schedule.
        let lead = match style {
            IntroStyle::TileShatter => 0x1C,
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

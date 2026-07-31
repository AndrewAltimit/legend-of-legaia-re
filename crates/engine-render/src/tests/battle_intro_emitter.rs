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
    rng: legaia_engine_vm::battle_intro_particles::IntroRng,
}

impl Env {
    fn new() -> Self {
        Self {
            rng: legaia_engine_vm::battle_intro_particles::IntroRng::new(0x1234_5678),
        }
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
        self.rng.draw()
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
fn every_style_skips_its_first_frame_and_keeps_its_fade_schedule() {
    // Frame one projects through the field camera's stale view matrix in
    // retail, so no style draws it; and whatever a style draws, the fade and
    // the handoff ride the same clock.
    for style in [
        IntroStyle::ScatterParticles,
        IntroStyle::SpinUpParticles,
        IntroStyle::Swirl,
    ] {
        let total = 100;
        let mut it = intro(style, total);
        let early = it.tick(0, 1);
        assert!(!early.style_drawn, "{style:?} must not draw frame one");
        assert!(
            early.prims.is_empty(),
            "{style:?} emitted frame-one geometry"
        );
        // The fade still arrives on schedule, over the style's own draws.
        let lead = match style {
            IntroStyle::Swirl => 0x20,
            _ => 0x18,
        };
        let late = it.tick(total as i16 - lead + 1, 1);
        assert!(
            late.fade.is_some(),
            "{style:?} lost its fade because the clock did not reach it"
        );
        let ScreenPrim::Flat(f) = late.prims.last().copied().unwrap() else {
            panic!("{style:?}: the fade quad must be last");
        };
        assert!(f.semi_transparent, "{style:?}: fade quad is blended");
    }
}

// ---------------------------------------------------------------------------
// The particle fields' emitters (FUN_801CFDA0 / FUN_801D0370)
// ---------------------------------------------------------------------------

/// Prims of one frame split into (wash, textured particle quads, flat ring
/// quads, fade).
fn split_particle_frame(
    f: &crate::battle_intro::IntroFrame,
) -> (usize, Vec<crate::screen_overlay::ScreenQuad>, usize) {
    let mut washes = 0;
    let mut quads = Vec::new();
    let mut flats = 0;
    for p in &f.prims {
        match p {
            ScreenPrim::Textured(q) => quads.push(*q),
            ScreenPrim::Flat(q) if q.ot_index == u32::MAX => washes += 1,
            ScreenPrim::Flat(q) if q.semi_transparent && f.fade.is_none() => flats += 1,
            ScreenPrim::Flat(_) => flats += 1,
        }
    }
    (washes, quads, flats)
}

#[test]
fn the_scatter_field_reconstructs_the_screen_as_8px_patches_at_rest() {
    use legaia_engine_vm::battle_intro_styles::{PARTICLE_TICK_COUNT, PARTICLE_TPAGE_BIAS};
    let mut it = intro(IntroStyle::ScatterParticles, 400);
    it.tick(0, 1);
    let f = it.tick(1, 1);
    assert!(f.style_drawn);
    let (washes, quads, _) = split_particle_frame(&f);
    // FUN_801CFDA0 washes near-black behind the confetti from frame two on.
    assert_eq!(washes, 1, "the 0x101010 wash");
    // All 0x488 visited records are on screen at the seeded rest pose.
    assert_eq!(quads.len(), PARTICLE_TICK_COUNT);
    // Particle 0 is the top-left 8x8 patch of the captured frame: page 0x135
    // (the capture's own column 320), texel (0, 4), projected to its source
    // cell. At z 0x1000 under H 0x80 the projection is a 1/32 scale, so the
    // 0x100-unit quad is 8 px.
    let q0 = quads[0];
    assert_eq!(q0.tpage, PARTICLE_TPAGE_BIAS as u16);
    assert_eq!(q0.uv, [(0, 4), (8, 4), (0, 12), (8, 12)]);
    let w = q0.xy[1].0 - q0.xy[0].0;
    assert!((w - 8).abs() <= 1, "an 8-px patch, got {w}");
    // The whole sheet tiles the display.
    let max_x = quads.iter().map(|q| q.xy[3].0).max().unwrap();
    let max_y = quads.iter().map(|q| q.xy[3].1).max().unwrap();
    assert!((i32::from(max_x) - 320).abs() <= 8, "right edge at {max_x}");
    assert!(max_y >= 220, "bottom edge at {max_y}");
    // Retail links every scatter quad at OT word 100 (byte 400).
    assert!(
        quads
            .iter()
            .all(|q| q.ot_index == crate::battle_intro::PARTICLE_OT)
    );
    // At clock 1 the diagonal delay ramp (`(col + row) * 0x40`) has released
    // the cells within `1 * 0x6E`: (0,0) at delay 0 and (0,1)/(1,0) at 0x40.
    // A moved particle turns semi-transparent (the `|= 2` on the packet code
    // byte); cell (0,2) at delay 0x80 has not moved yet.
    assert!(quads[0].semi_transparent, "cell (0,0) has moved");
    assert!(quads[1].semi_transparent, "cell (0,1) has moved");
    assert!(!quads[2].semi_transparent, "cell (0,2)'s delay is 0x80");
}

#[test]
fn the_spinup_field_draws_confetti_and_the_expanding_ring() {
    use legaia_engine_vm::battle_intro_styles::PARTICLE_TICK_COUNT;
    let mut it = intro(IntroStyle::SpinUpParticles, 400);
    it.tick(0, 1);
    let f = it.tick(1, 1);
    assert!(f.style_drawn);
    let (washes, quads, ring) = split_particle_frame(&f);
    assert_eq!(washes, 0, "FUN_801D0370 never washes");
    assert_eq!(quads.len(), PARTICLE_TICK_COUNT);
    // The FUN_801D1CFC tail: a 96-segment ring, phase 0xA0 at clock 1.
    assert_eq!(ring, crate::battle_intro::SPINUP_RING_SEGMENTS);
    // Style B's rest pose also reconstructs the frame: the `>> 3` position
    // pre-divide against the doubled seed constants lands the same 8-px
    // cells (x = -0x500 + col * 0x40 at view z 0x400 is a 1/8 scale).
    let q0 = quads[0];
    let w = q0.xy[1].0 - q0.xy[0].0;
    assert!((w - 8).abs() <= 1, "an 8-px patch, got {w}");
}

#[test]
fn a_moved_spinup_particle_links_one_ot_word_nearer() {
    use crate::battle_intro::{PARTICLE_OT, PARTICLE_OT_MOVED};
    let mut it = intro(IntroStyle::SpinUpParticles, 400);
    it.tick(0, 1);
    // Run the clock far enough that the radial delays start expiring but not
    // so far that everything has flown off screen.
    it.tick(1, 1);
    let f = it.tick(6, 1);
    let (_, quads, _) = split_particle_frame(&f);
    let moved: Vec<_> = quads.iter().filter(|q| q.semi_transparent).collect();
    let still: Vec<_> = quads.iter().filter(|q| !q.semi_transparent).collect();
    assert!(!moved.is_empty(), "no particle moved by clock 6");
    assert!(!still.is_empty(), "every particle moved by clock 6");
    assert!(moved.iter().all(|q| q.ot_index == PARTICLE_OT_MOVED));
    assert!(still.iter().all(|q| q.ot_index == PARTICLE_OT));
}

#[test]
fn the_spinup_ring_expands_fades_and_expires() {
    use crate::battle_intro::emit_spinup_ring;
    // Phase 0: not started. Phase past 0x1000: expired (clock 26 on).
    let mut none = Vec::new();
    assert!(!emit_spinup_ring(0, &mut none));
    assert!(!emit_spinup_ring(0x1001, &mut none));
    assert!(none.is_empty());

    let radius = |prims: &Vec<ScreenPrim>| -> i32 {
        prims
            .iter()
            .filter_map(|p| match p {
                ScreenPrim::Flat(q) => Some(i32::from(q.xy[0].0)),
                _ => None,
            })
            .max()
            .unwrap()
    };
    let mut early = Vec::new();
    assert!(emit_spinup_ring(0xA0, &mut early));
    let mut later = Vec::new();
    assert!(emit_spinup_ring(0x500, &mut later));
    assert!(
        radius(&later) > radius(&early),
        "the ring must expand with the phase"
    );
    // The depth-cue fade toward the staged black ambient: the colour level
    // falls as the phase grows.
    let level = |prims: &Vec<ScreenPrim>| match prims[0] {
        ScreenPrim::Flat(q) => q.color[0],
        _ => panic!(),
    };
    assert!(level(&later) < level(&early));
}

// ---------------------------------------------------------------------------
// The swirl's emitter (FUN_801D1888 / FUN_801D1A20)
// ---------------------------------------------------------------------------

#[test]
fn the_swirl_draws_both_halves_of_every_drawing_band() {
    use legaia_engine_vm::battle_intro_swirl::{
        BANDS_DRAWN, COLUMNS, TPAGE_MIRRORED, TPAGE_PRIMARY,
    };
    let mut it = intro(IntroStyle::Swirl, 400);
    it.tick(0, 1);
    let f = it.tick(1, 1);
    assert!(f.style_drawn);
    let quads: Vec<_> = f
        .prims
        .iter()
        .filter_map(|p| match p {
            ScreenPrim::Textured(q) => Some(*q),
            _ => None,
        })
        .collect();
    // 12 bands x 2 halves x 32 column pairs x 2 quads (ring + wall), all
    // accepted: the dispatch is double-sided (flag bit 27), so the mirrored
    // half's reversed winding does not cull, and every band sits past the
    // near cutoff at its seeded depth.
    assert_eq!(quads.len(), BANDS_DRAWN * 2 * (COLUMNS - 1) * 2);
    // The two halves sample the two capture pages - the right and left
    // 320-column halves of the captured frame.
    let primary = quads.iter().filter(|q| q.tpage == TPAGE_PRIMARY as u16);
    let mirrored = quads.iter().filter(|q| q.tpage == TPAGE_MIRRORED as u16);
    assert_eq!(primary.count(), quads.len() / 2);
    assert_eq!(mirrored.count(), quads.len() / 2);
    // Ring quads carry the neutral colour word, wall quads the darker one,
    // and the whole packet is opaque (code 0x2C, no |2 anywhere).
    for q in &quads {
        assert!(!q.semi_transparent);
        assert!(
            q.color == crate::battle_intro::SWIRL_RING_RGB
                || q.color == crate::battle_intro::SWIRL_WALL_RGB
        );
    }
}

#[test]
fn swirl_bands_fly_along_z_and_wind_down_bands_stop_drawing() {
    // The band scalar is a view depth, not a rotation angle: bands with
    // negative rates fly toward the camera and drop below the 0x80 draw
    // threshold, after which their quads stop appearing.
    let mut it = intro(IntroStyle::Swirl, 400);
    it.tick(0, 1);
    let first = it.tick(1, 1);
    // Band 0's rate is -0x6E00 >> 8 per frame ~ -110/frame from 0xA00: it
    // crosses 0x80 within ~21 frames.
    let mut clock = 2i16;
    let mut later = it.tick(clock, 1);
    while clock < 40 {
        clock += 1;
        later = it.tick(clock, 1);
    }
    let count = |f: &crate::battle_intro::IntroFrame| {
        f.prims
            .iter()
            .filter(|p| matches!(p, ScreenPrim::Textured(_)))
            .count()
    };
    assert!(
        count(&later) < count(&first),
        "wound-down bands must stop drawing: {} vs {}",
        count(&later),
        count(&first)
    );
}

#[test]
fn the_swirl_washes_once_the_late_phase_arrives() {
    use legaia_engine_vm::battle_intro_swirl::LATE_PHASE_FRAME;
    let mut it = intro(IntroStyle::Swirl, 400);
    it.tick(0, 1);
    // The wash reads the previous frame's clock, so it lags the crossing.
    let at = it.tick(LATE_PHASE_FRAME as i16, 1);
    let washes = |f: &crate::battle_intro::IntroFrame| {
        f.prims
            .iter()
            .filter(|p| matches!(p, ScreenPrim::Flat(q) if q.ot_index == u32::MAX))
            .count()
    };
    assert_eq!(washes(&at), 0, "the wash lags the late-phase crossing");
    // `DAT_801D2470` takes this frame's clock *after* the wash test reads the
    // previous one, so the wash needs the stored clock to have passed the
    // bound - two frames after the crossing.
    let next = it.tick(LATE_PHASE_FRAME as i16 + 1, 1);
    assert_eq!(washes(&next), 0, "the stored clock is exactly the bound");
    let after = it.tick(LATE_PHASE_FRAME as i16 + 2, 1);
    assert_eq!(washes(&after), 1, "the 0x101010 wash");
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

/// Screen bounding box of a frame's primitives - the measurement that told a
/// moving transition from a still one.
fn prim_bbox(f: &crate::battle_intro::IntroFrame) -> (i16, i16, i16, i16) {
    let (mut lo_x, mut lo_y, mut hi_x, mut hi_y) = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
    for p in &f.prims {
        let ScreenPrim::Textured(q) = p else { continue };
        for &(x, y) in &q.xy {
            lo_x = lo_x.min(x);
            lo_y = lo_y.min(y);
            hi_x = hi_x.max(x);
            hi_y = hi_y.max(y);
        }
    }
    (lo_x, lo_y, hi_x, hi_y)
}

/// **The tile shatter has to shatter.** A per-frame audit of the five styles
/// found this one's prim bounding box pinned at the seeded sheet's rect
/// (`[0,-4]..[320,252]`) *to the pixel* for the whole transition, while the
/// spin-up field's grew as its confetti flew - so the style was a whitening
/// fade over a re-tiled still frame, not a shatter.
///
/// Two independent causes, both fixed and both pinned here:
///
/// 1. the seeder's PRNG stand-in was a degenerate LCG that returned one
///    constant from its sixth draw on, so all 256 records got the *same*
///    spawn delay ([`IntroRng`](legaia_engine_vm::battle_intro_particles::IntroRng));
/// 2. the transition ran for 32 frames against a spawn window that needs 84
///    ([`INTRO_DURATION_FRAMES`](legaia_engine_vm::battle_intro_styles::INTRO_DURATION_FRAMES)
///    is retail's `0x84`).
///
/// Either one alone parks the grid, so the test drives a retail-length
/// transition and asserts the box **grows past the sheet on every side**.
#[test]
fn the_tile_sheet_breaks_apart_over_a_retail_length_transition() {
    use legaia_engine_vm::battle_intro_styles::{INTRO_DURATION_FRAMES, intro_duration_frames};

    let total = intro_duration_frames(IntroStyle::TileShatter);
    assert_eq!(total, INTRO_DURATION_FRAMES);

    let mut it = intro(IntroStyle::TileShatter, total);
    it.tick(0, 1);
    let seeded = prim_bbox(&it.tick(1, 1));
    assert_eq!(
        seeded,
        (0, -4, 320, 252),
        "the seeded sheet is the retail rect"
    );

    let mut widest = seeded;
    let mut moved_frame = None;
    for clock in 2..=total as i16 {
        let bb = prim_bbox(&it.tick(clock, 1));
        if bb.0 == i16::MAX {
            continue; // every record retired
        }
        if moved_frame.is_none() && bb != seeded {
            moved_frame = Some(clock);
        }
        widest = (
            widest.0.min(bb.0),
            widest.1.min(bb.1),
            widest.2.max(bb.2),
            widest.3.max(bb.3),
        );
    }

    let moved = moved_frame.expect("the sheet never moved - the shatter is static again");
    assert!(
        moved <= 40,
        "the first record only starts at frame {moved}; some tiles must go early"
    );
    assert!(widest.0 < seeded.0, "nothing left the sheet on the left");
    assert!(widest.1 < seeded.1, "nothing left the sheet on the top");
    assert!(widest.2 > seeded.2, "nothing left the sheet on the right");
    assert!(widest.3 > seeded.3, "nothing left the sheet on the bottom");
}

/// The same style over the **old** 32-frame window still moves, which is what
/// separates the two causes: with a working PRNG the early records start at
/// once, so a short transition degrades the shatter rather than freezing it.
#[test]
fn a_short_transition_still_moves_the_early_records() {
    let mut it = intro(IntroStyle::TileShatter, 32);
    it.tick(0, 1);
    let seeded = prim_bbox(&it.tick(1, 1));
    let mut changed = false;
    for clock in 2..=32i16 {
        changed |= prim_bbox(&it.tick(clock, 1)) != seeded;
    }
    assert!(changed, "not even the zero-delay records moved");
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
    fn the_particle_and_swirl_pages_live_wholly_in_the_columns_rect() {
        // Particle pages 0x135..=0x139 and swirl pages 0x115/0x117 all carry
        // the y=256 texpage bit, so their sampling sits inside the columns
        // rect and the rows blit would buy them nothing.
        for s in [
            IntroStyle::ScatterParticles,
            IntroStyle::SpinUpParticles,
            IntroStyle::Swirl,
        ] {
            assert_eq!(capture_rects_for(s), [FIELD_CAPTURE_COLS]);
        }
        // The page decode behind that claim: 15bpp texpage x/y from the TSB.
        for tpage in [0x135u16, 0x139, 0x115, 0x117] {
            let x = u32::from(tpage & 0xF) * 64;
            let y = u32::from((tpage >> 4) & 1) * 256;
            assert_eq!(y, 256, "page {tpage:#x} carries the y=256 bit");
            assert!((320..640).contains(&x), "page {tpage:#x} at x {x}");
        }
    }
}

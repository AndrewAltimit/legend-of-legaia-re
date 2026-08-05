//! The field-to-battle transition's per-frame emitter: the working-set owner
//! that stands between the wired transition clock and the ordering table.
//!
//! # What was missing, and what this is
//!
//! Everything either side of this module was already built. On the simulation
//! side `legaia_engine_vm::battle_intro_transition::tick_transition` is live -
//! `legaia_engine_core::World::tick_encounter` runs it every frame the
//! encounter session sits in its `Transition` phase - and all five style
//! kernels are ported. On the render side [`crate::screen_prim`] is the
//! ordering table, the native renderer's `SceneWithScreenPrims` target
//! composites it over a drawn scene, and [`crate::vram_capture`] lands a
//! drawn frame back in the software VRAM the strips sample.
//!
//! What nothing owned was the thing in between: a per-frame, per-style object
//! that holds the working set across frames, advances it off the transition's
//! own clock, and turns the result into primitives. Retail keeps that state in
//! the transition entity's `+0x48` block and emits straight into the OT
//! cursor. [`BattleIntro`] is that owner.
//!
//! # All five styles emit
//!
//! `FUN_801CF5BC`'s first tail switch dispatches five styles
//! ([`IntroStyle`]), and each reaches a primitive here through its own retail
//! packet builder:
//!
//! | Style | Retail packet builder | Port |
//! |---|---|---|
//! | [`IntroStyle::ScatterParticles`] | `FUN_801CFDA0` | [`emit_particle_field`] |
//! | [`IntroStyle::SpinUpParticles`] | `FUN_801D0370` + tail `FUN_801D1CFC` | [`emit_particle_field`] + [`emit_spinup_ring`] |
//! | [`IntroStyle::TileShatter`] | `FUN_801D0E54` via `FUN_80043390` | [`emit_tile`] |
//! | [`IntroStyle::Curtain`] | `FUN_801CF1B0` | [`intro_quad_to_screen`] |
//! | [`IntroStyle::Swirl`] | `FUN_801D1A20` via `FUN_80043390` / `FUN_80029888` | [`emit_swirl_band`] |
//!
//! The curtain's builder produces **screen-space** corners with texture page,
//! CLUT, UVs and a top/bottom colour pair, so there is no projection step to
//! invent; its descriptor table is disc data that parses
//! ([`IntroQuadTable`]), and the texture pages its two passes name decode to
//! exactly the rects [`crate::vram_capture`] captures into.
//!
//! The other four all end in the same GTE projection this module reproduces
//! ([`project_intro_corner`] + the FT4 handler's accept chain in
//! [`push_ft4_quad`]). The tile shatter and the swirl both assemble a
//! synthetic Legaia-TMD object in the `_DAT_8007B85C + 0x5DC00` scratch block
//! and hand it to the generic per-prim dispatcher; the two particle styles
//! write `POLY_FT4` packets straight into the ordering-table cursor and
//! project through `FUN_8005BAC8` (RotTransPers4; its return is `SZ3 >> 2`,
//! which is the depth [`legaia_engine_vm::battle_intro_styles::particle_quad_accepted`]
//! bounds).
//!
//! Two retail nuances are *not* carried, both stated rather than hidden. The
//! dispatcher runs a moving tile's opaque faces - and the whole late-phase
//! swirl - through a depth-cue bank (fade toward a per-caller far colour),
//! which the screen overlay has no channel for: receding tiles keep their
//! face grey, and the late swirl keeps its packet colours instead of hazing
//! toward the mid-grey far colour `FUN_80029888` stages. And the spin-up
//! ring's mesh comes from `FUN_80028158`, a 1395-instruction multi-shape
//! generator of which only the case-0 annulus parameters are modelled - see
//! [`emit_spinup_ring`].
//!
//! [`IntroFrame::style_drawn`] still reports per frame whether the style's own
//! geometry reached the list, as against the frame carrying only the fade
//! quad; every style's first frame is a deliberate no-draw (see the
//! stale-view-matrix note on [`BattleIntro::tick`]'s tile arm).
//!
//! # The curtain is a two-pass render-to-texture, and only one pass is on screen
//!
//! The curtain's row pass samples texture pages `0x105` / `0x108` and its
//! column pass `0x115` / `0x118`. Those decode to 15-bpp pages at VRAM
//! `(320, 0)` / `(512, 0)` and `(320, 256)` / `(512, 256)`. It is tempting to
//! read that as "two identical copies of the capture, one per pass", and this
//! module used to. `FUN_801D11D0`'s own draw-environment packets say otherwise.
//!
//! Between the two passes retail pushes `SetDrawArea` / `SetDrawOffset` pairs
//! into the ordering table, and their OT buckets order them against the strips:
//!
//! | OT bucket | what retail links there |
//! |---|---|
//! | `0x1F4` (500) | `SetDrawOffset(0, 0)` + `SetDrawArea(320, 0, 320, 240)` |
//! | `0x1EA` (490) | `FUN_801D1D9C(0x1EA, 2, 0x808080)` - the mid-pass emitter |
//! | `0x1C2` (450) | the **column** strips ([`styles::CURTAIN_COL_OT_DEPTH`]) |
//! | `0x190` (400) | `SetDrawArea(0, y, 320, h)` + `SetDrawOffset(0, y)` - the back buffer |
//! | `0x12C` (300) | the **row** strips ([`styles::CURTAIN_ROW_OT_DEPTH`]) |
//!
//! A higher OT index draws first, so the column pass runs with the draw area
//! pointed at VRAM `(320, 0)` - which is [`FIELD_CAPTURE_ROWS`], the very rect
//! the row pass then samples. The column strips are not on screen at all: they
//! render an intermediate, and the row pass slices *that* into the display.
//! `styles::CURTAIN_COL_DRAW_BIAS` (`0x1E0`) is what makes it fit - a column
//! that passes the visibility test (which re-centres on `0xA0`) lands at
//! `x` in `320..640`, exactly the installed draw area, with the offset at
//! `(0, 0)` so primitive coordinates are absolute VRAM.
//!
//! So the capture goes into [`FIELD_CAPTURE_COLS`] **only**, and
//! [`BattleIntro::refresh_captured_page`] re-composes the intermediate every
//! frame. The port rasterises the column pass on the CPU
//! ([`compose_curtain_intermediate`]) because a screen-space quad list has no
//! render-to-VRAM target; the arithmetic is the same quad
//! `legaia_engine_vm::battle_intro_transition::build_intro_quad` produced.
//!
//! The mid-pass emitter is established now. `FUN_801D1D9C` is dumped from the
//! `overlay_field_battle_intro` image itself
//! (`ghidra/scripts/funcs/overlay_field_battle_intro_801d1d9c.txt` - the
//! earlier note that it existed only at a VA aliasing another overlay is
//! retired), and it is `FUN_80024EE4`'s shape pointed at the intermediate: a
//! five-word `0x2B` (untextured, semi-transparent) quad over
//! `x 0x140..0x140+W, y -4..H` - the display halfwords `_DAT_1F80038C` /
//! `_DAT_1F80038E` biased one screen right - preceded by a
//! `SetDrawMode((abr << 5) | 0xE)` packet at the same OT layer. With the
//! curtain's arguments `(0x1EA, 2, 0x808080)` that is a **subtractive decay of
//! `0x80` per channel per frame over the whole intermediate**, between the
//! draw-area install at `0x1F4` and the column strips at `0x1C2` - so a column
//! the warp has culled leaves a ghost that reaches black in two frames rather
//! than vanishing. [`BattleIntro`] carries it: the intermediate persists
//! across frames and decays by [`CURTAIN_MIDPASS_DECAY_5`] instead of being
//! cleared.
//!
//! The **display** side of the same no-clear law is carried for the curtain
//! too. `FUN_801D11D0` re-arms the screen wash `FUN_8004695C(0x80808)` at the
//! top of **every** frame (`0x801D1228..0x801D1230`, unconditional), and
//! nothing clears the display buffer during the transition - so the gap a
//! departing row strip leaves shows the previous frames' pixels decaying by 8
//! per 8-bit channel per frame (~31 frames to black), not flat black.
//! [`BattleIntro`] models that display buffer on the CPU
//! ([`CURTAIN_TRAIL_RECT`] holds it), seeds it from the same field capture
//! retail's init lands in both display buffers, decays it by
//! [`CURTAIN_DISPLAY_DECAY_5`] each frame, and emits it as textured backdrop
//! quads behind the live row strips. Two disclosed approximations: the wash
//! drain (`FUN_80046978`) scales its constant by the scratchpad brightness
//! byte, taken here at full brightness; and retail's display is
//! double-buffered, so its per-buffer decay may interleave at half this rate -
//! settling that needs a retail frame capture of a curtain formation
//! (hypothesis, graded inference).

use crate::gte::{
    GteMat3, GteVec3, ScreenXY, avsz4_with_scale, gte_divide, gte_persp_term, nclip, psx_cos,
    psx_sin,
};
use crate::screen_prim::{FlatQuad, ScreenPrim, ScreenQuad, display_rect_flat_quad, fade_prim};
use crate::vram_capture::{
    CaptureOpts, FIELD_CAPTURE_COLS, FIELD_CAPTURE_ROWS, PSX_SCREEN_HEIGHT, PSX_SCREEN_WIDTH,
    VramRect,
};
use legaia_engine_vm::battle_intro_styles::{
    self as styles, IntroFade, IntroStyle, PARTICLE_TICK_A, PARTICLE_TICK_B, ParticleTickStyle,
};
use legaia_engine_vm::battle_intro_swirl::{self as swirl, SwirlBandDraw, SwirlMesh};
use legaia_engine_vm::battle_intro_tiles::{self as tiles, TileGrid};
use legaia_engine_vm::battle_intro_transition::{INTRO_QUAD_DESC_STRIDE, IntroQuad, IntroQuadDesc};
use legaia_tim::Vram;

/// The 4bpp "edge shade" texture page the tile shatter stretches over each
/// box side face: `tpage 0x0027` = VRAM `(448, 0)`, 64x64 texels, so 16
/// halfwords wide and 64 rows tall.
///
/// It matters here because it lies **inside** [`FIELD_CAPTURE_ROWS`]
/// (`320..640` x `0..240`). Capturing the field frame into that rect
/// overwrites the page, so a style that needs the shade texture must not have
/// the rows rect written under it - see [`capture_rects_for`].
pub const TILE_SHADE_PAGE: VramRect = VramRect::new(448, 0, 16, 64);

/// Which capture rects a style's own primitives actually sample.
///
/// The capture is not free of consequences: it writes over whatever else
/// lives in the destination rect, and the two rects are not equally safe.
///
/// | Style | Samples | Rects written |
/// |---|---|---|
/// | [`IntroStyle::Curtain`] | column pass `0x115`/`0x118` at `(320,256)`/`(512,256)`; the row pass' `0x105`/`0x108` at `(320,0)`/`(512,0)` is the column pass' **output** | columns only |
/// | [`IntroStyle::TileShatter`] | `0x135`/`0x137` at `(320,256)`/`(448,256)`, plus the 4bpp [`TILE_SHADE_PAGE`] at `(448,0)` | columns only |
/// | the particle styles | `0x135..=0x139` at `(320,256)..(576,256)` (u < 64, so the five pages tile the 320 capture columns) | columns only |
/// | [`IntroStyle::Swirl`] | `0x115`/`0x117` at `(320,256)`/`(448,256)` | columns only |
///
/// **No style writes the rows rect**, and for two different reasons. For the
/// curtain the rows rect is the intermediate its column pass renders into (see
/// the module docs) - blitting the capture there would be overwritten by the
/// first column strip anyway, and until it was it would show the transition an
/// un-warped frame. For the tile shatter it is load-bearing the other way: its
/// own pages are wholly inside the column rect and the 4bpp
/// [`TILE_SHADE_PAGE`] it also needs is inside the *row* rect, so writing the
/// rows would destroy an input it depends on while gaining it nothing. The
/// particle and swirl pages are likewise wholly inside the column rect (every
/// page they name carries the `y = 256` bit).
pub fn capture_rects_for(_style: IntroStyle) -> &'static [VramRect] {
    const COLS_ONLY: [VramRect; 1] = [FIELD_CAPTURE_COLS];
    &COLS_ONLY
}

// ---------------------------------------------------------------------------
// The tile shatter's projection (the dispatcher work around `FUN_801D0E54`)
// ---------------------------------------------------------------------------

/// GTE screen-centre offsets during the transition, in whole pixels.
/// `OFY = 114` is **not** `240 / 2` - pinned by the GTE control file of nine
/// save states (`crates/mednafen/tests/gte_projection_real.rs`).
pub const INTRO_OFX: i32 = 160;
/// See [`INTRO_OFX`].
pub const INTRO_OFY: i32 = 114;

/// GTE focal length the tile tick loads (`li a0,0x80` at `0x801D0D30` into
/// `FUN_8003D254`).
pub const INTRO_H: u16 = 0x80;

/// The FT4 handler's near cutoff: a primitive whose `AVSZ4` OT depth lands
/// below this is dropped (`sub v1,s2,t4; bltz` at `0x80043AF0`). The value
/// is the scratch halfword `0x1F80037E`, read as `0x10` on every frame of a
/// live shatter capture.
pub const INTRO_NEAR_OTZ: i32 = 0x10;

/// `ZSF4` during the transition: the dispatcher writes
/// `0x400 >> (_DAT_1F8003A4 & 0x1F)` and the shift is `0` in the live
/// capture, so `AVSZ4` yields `(sz0+sz1+sz2+sz3) >> 2` - the plain average.
pub const INTRO_ZSF4: i32 = 0x400;

/// Overlay VA of the tile seeder's corner-offset table (`0x801CE8BC`, four
/// words) inside PROT 0979. `parse_tile_corner_table` reads it; the values
/// decode to `[0, 1, 17, 18]` - one step right / one row down in the
/// `17 x 17` vertex grid.
pub const TILE_CORNER_TABLE_VA: u32 = 0x801C_E8BC;

/// Read the tile corner-offset table out of a PROT 0979 image relocated to
/// its load base. `None` when the image is too short.
pub fn parse_tile_corner_table(as_loaded: &[u8], base_va: u32) -> Option<[i32; 4]> {
    let off = TILE_CORNER_TABLE_VA.checked_sub(base_va)? as usize;
    let bytes = as_loaded.get(off..off + 16)?;
    Some(std::array::from_fn(|i| {
        i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

/// The SCUS Euler-triple → GTE rotation kernel `FUN_80026988`, exactly as
/// the disassembly composes it: `Rx(x) * Ry(y) * Rz(z)` with every product
/// truncated to q3.12 **per term** (each `mult` is followed by its own
/// `sra 12`; the mixed second-row/third-row terms are two-step products, so
/// they truncate twice). Angles are 12-bit (`& 0xFFF`), sin/cos from the
/// same table [`psx_sin`] reproduces.
///
/// PORT: FUN_80026988
pub fn euler_rot_psx(angles: (i16, i16, i16)) -> GteMat3 {
    let ang = |a: i16| (a as u16) & 0xFFF;
    let (cx, sx) = (psx_cos(ang(angles.0)), psx_sin(ang(angles.0)));
    let (cy, sy) = (psx_cos(ang(angles.1)), psx_sin(ang(angles.1)));
    let (cz, sz) = (psx_cos(ang(angles.2)), psx_sin(ang(angles.2)));
    let q = |v: i32| v >> 12;
    // The two shared two-step products (`iVar3` / `iVar2` in the decomp).
    let a = q(cz * -sy);
    let b = q(sz * -sy);
    let e = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    GteMat3 {
        m: [
            [e(q(cz * cy)), e(q(-(sz * cy))), e(sy)],
            [
                e(q(sz * cx) - q(a * sx)),
                e(q(cz * cx) + q(b * sx)),
                e(q(-(cy * sx))),
            ],
            [
                e(q(sz * sx) + q(a * cx)),
                e(q(cz * sx) - q(b * cx)),
                e(q(cy * cx)),
            ],
        ],
    }
}

/// One projected corner: the saturated SXY plus the SZ depth bucket.
#[derive(Debug, Clone, Copy)]
struct ProjCorner {
    xy: ScreenXY,
    sz: i32,
}

/// `RTPS` for one intro-packet corner: rotate by the packet's own matrix,
/// translate by the record position (the per-record MVMVA result - the
/// transition's view matrix is identity rotation with zero translation from
/// the second frame on, pinned live), perspective-divide through the UNR
/// reciprocal at [`INTRO_H`], and offset by the transition's screen centre.
/// Shared by the tile shatter, the two particle fields, the swirl and the
/// spin-up ring - all five run on the same transition GTE file.
fn project_intro_corner(rot: &GteMat3, tr: GteVec3, c: (i16, i16, i16)) -> ProjCorner {
    let v = rot.mul_vec(GteVec3::new(i32::from(c.0), i32::from(c.1), i32::from(c.2)));
    let (x, y, z) = (v.x + tr.x, v.y + tr.y, v.z + tr.z);
    // SZ3 saturates to 0..0xFFFF; the divide then saturates at the
    // behind-camera bound exactly as hardware does.
    let sz = z.clamp(0, 0xFFFF);
    let (recip, _overflow) = gte_divide(INTRO_H, sz as u16);
    let ir = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX));
    let sx = INTRO_OFX + gte_persp_term(ir(x), recip) as i32;
    let sy = INTRO_OFY + gte_persp_term(ir(y), recip) as i32;
    ProjCorner {
        xy: ScreenXY::new(sx, sy).saturate_sxy(),
        sz,
    }
}

/// Project one tile's ten-face packet and append the survivors to `prims`.
///
/// The accept chain is the FT4 handler's (`FUN_800439E4`, the slot-17 leaf
/// `FUN_80043390` dispatches this packet to): `NCLIP` over corners 0-2, a
/// second `NCLIP` over 1-3 after the fourth `RTPS`, **accept unless
/// `nclip1 <= 0 && nclip2 >= 0`**. A planar quad's two strip triangles wind
/// oppositely, so the straight orientation (`n1 > 0`) passes and the reversed
/// one (`n1 < 0, n2 > 0`) rejects - the faces are single-sided, which is why
/// the packet's back face reverses its corner order relative to the front:
/// each culls exactly when it faces away. Then `AVSZ4` for the OT depth
/// and the [`INTRO_NEAR_OTZ`] cutoff. Faces are appended in packet order;
/// [`crate::screen_prim::order_primitives`]'s tie-break (later-submitted
/// draws first) then reproduces `AddPrim`'s prepend, which is what lands the
/// shade set on top of its opaque siblings within a bucket.
pub fn emit_tile(
    rec: &legaia_engine_vm::battle_intro_tiles::TileRecord,
    prims: &mut Vec<ScreenPrim>,
) {
    use legaia_engine_vm::battle_intro_tiles::tile_face_quads;
    let Some(quads) = tile_face_quads(rec) else {
        return;
    };
    let rot = euler_rot_psx(rec.angles);
    let tr = GteVec3::new(
        i32::from(rec.pos.0),
        i32::from(rec.pos.1),
        i32::from(rec.pos.2),
    );
    for q in &quads {
        let p: [ProjCorner; 4] =
            std::array::from_fn(|i| project_intro_corner(&rot, tr, q.corners[i]));
        let grey = u32::from(q.grey);
        push_ft4_quad(
            &p,
            q.uv,
            q.clut,
            q.tpage,
            grey << 16 | grey << 8 | grey,
            q.semi_transparent,
            CullMode::SingleSided,
            prims,
        );
    }
}

/// Backface handling of one dispatched packet - the dispatcher's NCLIP mask.
///
/// `FUN_80043390` decodes bit 27 of its second argument into the mask the
/// kind handlers AND over each NCLIP result (`0x80043520..0x80043540`):
/// clear = `0xFFFFFFFF` (the plain single-sided accept), set = `0x7FFFFFFF`
/// (the sign bit is stripped, so `n <= 0 && n >= 0` can only reject a
/// degenerate quad - effectively double-sided). The tile shatter passes `0`
/// there and culls; the swirl passes `0x18808080` and does not, which is what
/// lets its x-mirrored half - whose winding is reversed - draw at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CullMode {
    SingleSided,
    DoubleSided,
}

/// The FT4 handler's accept chain (`FUN_800439E4` and its fog-bank sibling
/// share it): `NCLIP` over corners 0-2, a second `NCLIP` over 1-3 after the
/// fourth `RTPS`, reject on `n1 <= 0 && n2 >= 0` (through the [`CullMode`]
/// mask), then `AVSZ4` for the OT depth and the [`INTRO_NEAR_OTZ`] cutoff.
/// Returns whether the quad was pushed.
#[allow(clippy::too_many_arguments)]
fn push_ft4_quad(
    p: &[ProjCorner; 4],
    uv: [(u8, u8); 4],
    clut: u16,
    tpage: u16,
    color: u32,
    semi_transparent: bool,
    cull: CullMode,
    prims: &mut Vec<ScreenPrim>,
) -> bool {
    if cull == CullMode::SingleSided {
        let n1 = nclip(p[0].xy, p[1].xy, p[2].xy);
        let n2 = nclip(p[1].xy, p[2].xy, p[3].xy);
        if n1 <= 0 && n2 >= 0 {
            return false;
        }
    }
    let otz = avsz4_with_scale(p[0].sz, p[1].sz, p[2].sz, p[3].sz, INTRO_ZSF4);
    if otz < INTRO_NEAR_OTZ {
        return false;
    }
    prims.push(ScreenPrim::Textured(ScreenQuad {
        xy: std::array::from_fn(|i| (p[i].xy.x as i16, p[i].xy.y as i16)),
        uv,
        clut,
        tpage,
        color,
        gouraud: None,
        semi_transparent,
        ot_index: otz as u32,
    }));
    true
}

// ---------------------------------------------------------------------------
// The particle fields (FUN_801CFDA0 / FUN_801D0370)
// ---------------------------------------------------------------------------

/// OT bucket a particle quad links at, in the raw-OTZ scale the dispatcher
/// buckets use (a dispatcher word index is `otz >> 2`, and both particle
/// ticks link at OT byte offset `400` - word `100` - so the equivalent OTZ is
/// `400`). Nearer than the spin-up ring's projected depth (`~0x200` at view z
/// `0x800`), farther than the fade quad's `1..=2`.
pub const PARTICLE_OT: u32 = 400;
/// `FUN_801D0370` links a particle that moved this frame **one OT word
/// nearer** (`s6 = -1`, `OT + 400 + s6 * 4`).
pub const PARTICLE_OT_MOVED: u32 = 396;

/// The full-screen wash `FUN_801CFDA0` arms on every frame after the first,
/// and `FUN_801D1888` arms once the swirl clock has passed its late-phase
/// frame (both `func_0x8004695C(0x101010)` - near-black).
pub const PARTICLE_WASH_RGB: u32 = 0x0010_1010;

/// A `FUN_8004695C` wash as a primitive.
///
/// `FUN_8004695C` only *arms* it (`gp+0x9D4 = 1`, `gp+0x9D0 = rgb`, and it
/// clears `_DAT_8007B6CC` on the way past). The drain is `FUN_80046978`,
/// which scales each channel by the scratchpad brightness byte at
/// `0x1F800393`, clamps to `0xFF`, clears the armed flag - so it is a
/// **one-shot per arm**, which is why `FUN_801CFDA0` re-arms it every frame -
/// and hands it to `FUN_80024EE4(otlen - 1, 2, rgb)`. Both arguments matter:
///
/// * `a0 = otlen - 1` is the **farthest** OT bucket, so this draws before
///   everything else in the frame,
/// * `a1 = 2` is ABR mode `B - F`, so it **subtracts** rather than fills.
///
/// Retail is therefore darkening whatever the framebuffer already holds, not
/// painting an opaque background. The port models the packet faithfully; what
/// this *primitive* cannot reproduce is the accumulation the effect rides on,
/// because each port frame is composed from scratch rather than from the
/// previous frame's pixels. Over [`BACKDROP_RGB`] the subtraction is a
/// near-no-op, which is the honest outcome rather than a fabricated one.
///
/// The **curtain** no longer relies on this: its per-frame `0x80808` wash is
/// carried as the CPU display model's decay
/// ([`CURTAIN_DISPLAY_DECAY_5`], see the module docs), which is why its tick
/// emits no wash primitive at all. The two `0x101010` washes this function
/// still draws - the scatter field's and the swirl's late phase - keep the
/// from-scratch caveat: their styles redraw nearly the whole display every
/// frame, so the residual is the absence of a short motion trail behind the
/// flying patches, not a wrongly-lit frame.
pub fn wash_prim(gp0_rgb_word: u32) -> ScreenPrim {
    display_rect_flat_quad(
        [
            (gp0_rgb_word & 0xFF) as u8,
            ((gp0_rgb_word >> 8) & 0xFF) as u8,
            ((gp0_rgb_word >> 16) & 0xFF) as u8,
            0xFF,
        ],
        true,
        WASH_ABR_SUBTRACT,
        u32::MAX,
    )
}

/// The ABR mode `FUN_80046978` passes for the armed wash: `2`, i.e. `B - F`.
pub const WASH_ABR_SUBTRACT: u8 = 2;

/// The colour the transition's own frame starts from.
///
/// The transition **owns the whole frame**: its init routine writes game mode
/// `9` into `_DAT_8007B83C` (`0x801CF180`/`0x801CF188`, the "efect" mode) and
/// the field's mode-3 renderer never runs again for the rest of the window.
/// The field's last frame is captured once, at init, into the texture page at
/// VRAM `(320, 256)` - and from there on every visible pixel is a transition
/// primitive sampling that page.
///
/// That is what makes the base colour black rather than "the field". Each
/// particle packet is semi-transparent with the page's own ABR `1` (`B + F`,
/// additive), so a record still at its rest pose reproduces its captured
/// patch **exactly** only when what is under it is black; the un-moved grid
/// then reconstructs the frame, and every patch that flies away leaves the
/// base colour behind it.
pub const BACKDROP_RGB: u32 = 0x0000_0000;

/// The frame the transition composes onto: an opaque display-rect quad at the
/// farthest OT bucket, in [`BACKDROP_RGB`].
///
/// Retail has no such primitive, and does not need one - it has a game mode
/// with no field renderer in it. The port composes the transition's screen
/// primitives *over a live scene* (the native renderer's
/// `SceneWithScreenPrims` target),
/// which is a port artifact, so the field kept rendering underneath. Two
/// things came out of that, both visible in a screenshot sweep: every patch
/// still at its rest pose was drawn additively over an identical live copy of
/// itself and read at double brightness, and once the last particle expired
/// the transition emitted nothing at all - leaving the remaining ~54 frames
/// of the window showing a clean, untouched, still-animating field.
///
/// This quad is the port's stand-in for "the field is not in the ordering
/// table". It is emitted on every frame of the window, including the frames a
/// style draws nothing on, which is also what keeps the host's target choice
/// on the compositing arm for the whole transition.
pub fn backdrop_prim() -> ScreenPrim {
    display_rect_flat_quad(
        [
            (BACKDROP_RGB & 0xFF) as u8,
            ((BACKDROP_RGB >> 8) & 0xFF) as u8,
            ((BACKDROP_RGB >> 16) & 0xFF) as u8,
            0xFF,
        ],
        false,
        0,
        u32::MAX,
    )
}

/// A GP0 colour word (`R` in the low byte) as [`ScreenQuad::color`]'s
/// `0x00RRGGBB`.
fn gp0_rgb(word: u32) -> u32 {
    (word & 0xFF) << 16 | (word & 0xFF00) | ((word >> 16) & 0xFF)
}

/// One frame of a particle-field style, integration and packet emission
/// together. `FUN_801CFDA0` / `FUN_801D0370`.
///
/// Retail's per-particle packet is a 10-word `POLY_FT4` (tag `0x09000000`,
/// colour code `0x2C`, `|= 2` - semi-transparent, ABR 1 additive off the
/// page's TSB - once the particle's delay has expired) sampling an **8 x 8
/// texel patch of the captured field frame**: texture page
/// `(rec[+0x28] >> 6) + 0x135` with `u = rec[+0x28] & 0x3F`,
/// `v = rec[+0x2A]`. Pages `0x135..=0x139` are 15-bpp pages at
/// `(320 + 64k, 256)`, and u stays below `0x40`, so the five pages tile
/// exactly the 320 captured columns - the seeded grid *is* the screen cut
/// into 8 x 8 patches (40 columns x 29 visited rows = 320 x 232 of the 240
/// rows, which is why the ticks stop at `0x488` records). At the seeded rest
/// pose the projection maps a cell straight back onto its own patch
/// (`z = 0x1000` under `H = 0x80` is a 1/32 scale for style A; style B's
/// `>> 3` pre-divide against `z = 0x2000 >> 3` lands the same 8-px cell), so
/// the un-moved field reconstructs the captured frame and particles peel off
/// it as their delays expire.
///
/// The packet is authored from the record's **frame-entry** state: the colour
/// word, UVs and both pose matrices are latched before the integration block
/// runs (`0x801CFEA4..0x801CFF3C` all precede the moved arm), so a decaying or
/// accelerating particle draws this frame's starting pose and colour. The
/// pose chain is the per-record `SetRotMatrix(view)` →
/// `FUN_8003D344(rec+0x10)` → `FUN_80026988(rec+0x08)` sequence: **`+0x10` is
/// the position triple and `+0x08` the Euler triple** (`FUN_80026988` is the
/// Euler-to-matrix kernel and it is handed `+0x08`), the reverse of the
/// labels the seeder-era notes used - the same position/rotation inversion
/// the tile record went through. The kernel field names
/// (`IntroParticle::rot` / `IntroParticle::trans`) still carry the old
/// labels; the arithmetic is unaffected because integration pairs are
/// unchanged.
///
/// Projection is `FUN_8005BAC8` (RotTransPers4): corners `(0,0)`, `(s,0)`,
/// `(0,s)`, `(s,s)` at the style's quad size, depth = `SZ3 >> 2`, then the
/// screen-window accept
/// ([`legaia_engine_vm::battle_intro_styles::particle_quad_accepted`]) and a
/// fixed OT bucket ([`PARTICLE_OT`]). `emit` is the first-frame gate the
/// caller derives from the transition clock - see the tile arm's
/// stale-view-matrix note.
///
/// PORT: FUN_801CFDA0
/// PORT: FUN_801D0370
/// REF: FUN_8005BAC8 (RotTransPers4; returns `SZ3 >> 2`)
pub fn emit_particle_field(
    grid: &mut [legaia_engine_vm::battle_intro_particles::IntroParticle],
    style: &ParticleTickStyle,
    elapsed: &mut i16,
    frame_step: u8,
    emit: bool,
    prims: &mut Vec<ScreenPrim>,
) -> bool {
    let scaled = i32::from(*elapsed) * style.delay_scale;
    let mut drawn = false;
    for p in grid.iter_mut().take(styles::PARTICLE_TICK_COUNT) {
        // Latch the packet inputs before the integration mutates the record.
        let color = gp0_rgb(p.tint);
        let u = (p.texel_page as u16 & 0x3F) as u8;
        let v = p.texel_v as u8;
        let tpage = (p.texel_page >> 6).wrapping_add(styles::PARTICLE_TPAGE_BIAS) as u16;
        let pos = p.rot; // +0x10: the position triple (see the doc above)
        let ang = p.trans; // +0x08: the Euler triple
        let step = styles::step_particle(p, style, frame_step, scaled);
        let styles::ParticleStep::Live { moved, .. } = step else {
            continue;
        };
        if !emit {
            continue;
        }
        let s = style.rot_prescale_shift;
        let tr = GteVec3::new(
            i32::from(pos.0) >> s,
            i32::from(pos.1) >> s,
            i32::from(pos.2) >> s,
        );
        let rot = euler_rot_psx(ang);
        let q = style.quad_size;
        let corners = [(0, 0), (q, 0), (0, q), (q, q)];
        let pc: [ProjCorner; 4] = std::array::from_fn(|i| {
            project_intro_corner(&rot, tr, (corners[i].0, corners[i].1, 0))
        });
        // FUN_8005BAC8 returns the fourth corner's SZ quartered.
        let depth = pc[3].sz >> 2;
        if !styles::particle_quad_accepted(style, depth, (pc[0].xy.x as i16, pc[0].xy.y as i16)) {
            continue;
        }
        let ot = if moved && style.moved_links_nearer {
            PARTICLE_OT_MOVED
        } else {
            PARTICLE_OT
        };
        prims.push(ScreenPrim::Textured(ScreenQuad {
            xy: std::array::from_fn(|i| (pc[i].xy.x as i16, pc[i].xy.y as i16)),
            uv: [
                (u, v),
                (u.wrapping_add(8), v),
                (u, v.wrapping_add(8)),
                (u.wrapping_add(8), v.wrapping_add(8)),
            ],
            clut: 0,
            tpage,
            color,
            gouraud: None,
            semi_transparent: moved,
            ot_index: ot,
        }));
        drawn = true;
    }
    *elapsed = (*elapsed as u16).wrapping_add(u16::from(frame_step)) as i16;
    drawn
}

// ---------------------------------------------------------------------------
// The spin-up style's expanding ring (FUN_801D1CFC)
// ---------------------------------------------------------------------------

/// Ring phase per clock frame: `FUN_801D0370`'s tail calls
/// `FUN_801D1CFC(elapsed * 0xA0)` with the pre-increment clock.
pub const SPINUP_RING_PHASE_STEP: i32 = 0xA0;
/// The ring draws while its phase is `1..=0x1000`
/// (`param_1 - 1 <u 0x1000`), i.e. for clock frames 1 through 25.
pub const SPINUP_RING_PHASE_MAX: i32 = 0x1000;
/// Segments around the ring (`FUN_80028158(dest, 0, 0x60, params)`).
pub const SPINUP_RING_SEGMENTS: usize = 0x60;
/// The mesh params' x/y scale pair (`params +0x20/+0x22 = 0xE00`).
pub const SPINUP_RING_SCALE: i32 = 0xE00;
/// The outer rim's z depth (`params +0x1E = 0xC8`); the inner rim sits at 0.
pub const SPINUP_RING_DEPTH: i16 = 0xC8;
/// The view translation the tail stages before the call
/// (`0x1F800348..0x350 = (0, 0, 0x800)`).
pub const SPINUP_RING_VIEW_Z: i32 = 0x800;
/// The ring's flat colour (`params +0x08 = 0x303030`).
pub const SPINUP_RING_RGB: u32 = 0x0030_3030;

/// The expanding shockwave ring `FUN_801D0370` draws behind its confetti:
/// `FUN_801D1CFC(phase)`, `phase = clock * 0xA0`.
///
/// The tail builds a procedural annulus via `FUN_80028158(scratch, 0, 0x60,
/// params)` - the shared SCUS multi-shape mesh generator - and dispatches it
/// through `FUN_80043390` with flag word `0x89000000` (bit 27: double-sided)
/// and `a2 = phase` (non-zero: the depth-cue alpha bank, ambient staged from
/// the flag word's zero RGB). The generator's case-0 arm with these params
/// makes a 96-column cone band: inner rim at radius `phase` and z `0`, outer
/// rim at radius `phase + 2` and z [`SPINUP_RING_DEPTH`], both scaled by
/// `0xE00 / 0x1000`, one quad per column pair, colour `0x303030`.
///
/// Two halves of this are modelled rather than instruction-ported, and both
/// are recorded here so the boundary is inspectable: the exact vertex walk of
/// `FUN_80028158` (1395 instructions; only its case-0 parameters and lane
/// assignment are taken - x from the `_DAT_8007B7F8` lane, y from
/// `_DAT_8007B81C`, per-column angle step `0x1000 / 96` with a half-step
/// offset and the generator's `-0x400` quarter-turn bias), and the fog bank's
/// blend (drawn here as the half-blend bank's `B/2 + F/2` with the colour
/// faded toward the staged black ambient as the phase grows - the ring
/// self-extinguishes as it expands, matching `a2` rising to `0x1000`).
///
/// PORT: FUN_801D1CFC
/// REF: FUN_80028158 (case-0 annulus parameters only - see above)
pub fn emit_spinup_ring(phase: i32, prims: &mut Vec<ScreenPrim>) -> bool {
    if !(1..=SPINUP_RING_PHASE_MAX).contains(&phase) {
        return false;
    }
    let (r_in, r_out) = (phase, phase + 2);
    let seg_step = 0x1000 / SPINUP_RING_SEGMENTS as i32; // 0x2A
    let half_step = seg_step >> 1; // 0x15
    let tr = GteVec3::new(0, 0, SPINUP_RING_VIEW_Z);
    let vert = |k: usize, r: i32, z: i16| -> ProjCorner {
        let a = ((k as i32 % SPINUP_RING_SEGMENTS as i32) * seg_step - half_step - 0x400) & 0xFFF;
        let x = (((psx_cos(a as u16) * SPINUP_RING_SCALE) >> 12) * r) >> 12;
        let y = (((psx_sin(a as u16) * SPINUP_RING_SCALE) >> 12) * r) >> 12;
        project_intro_corner(&GteMat3::IDENTITY, tr, (x as i16, y as i16, z))
    };
    // The depth-cue fade toward the staged black ambient: `a2` is the phase,
    // full at 0x1000.
    let level = (((SPINUP_RING_RGB & 0xFF) as i32 * (0x1000 - phase)) >> 12) as u8;
    let mut drawn = false;
    for k in 0..SPINUP_RING_SEGMENTS {
        let pc = [
            vert(k, r_in, 0),
            vert(k, r_out, SPINUP_RING_DEPTH),
            vert(k + 1, r_in, 0),
            vert(k + 1, r_out, SPINUP_RING_DEPTH),
        ];
        let otz = avsz4_with_scale(pc[0].sz, pc[1].sz, pc[2].sz, pc[3].sz, INTRO_ZSF4);
        if otz < INTRO_NEAR_OTZ {
            continue;
        }
        prims.push(ScreenPrim::Flat(FlatQuad {
            xy: std::array::from_fn(|i| (pc[i].xy.x as i16, pc[i].xy.y as i16)),
            color: [level, level, level, 0xFF],
            semi_transparent: true,
            abr_mode: 0,
            ot_index: otz as u32,
        }));
        drawn = true;
    }
    drawn
}

// ---------------------------------------------------------------------------
// The swirl's band submit (FUN_801D1A20)
// ---------------------------------------------------------------------------

/// Colour word of a swirl **ring** quad (`0x2C808080`: opaque FT4, neutral).
pub const SWIRL_RING_RGB: u32 = 0x0080_8080;
/// Colour word of a swirl **wall** quad (`0x2C404040`: the darker quad that
/// joins a column's far-z copy to the near ring).
pub const SWIRL_WALL_RGB: u32 = 0x0040_4040;

/// One band half of the swirl, projected and appended. `FUN_801D1A20`.
///
/// The submit is the tile shatter's mechanism again: a synthetic Legaia-TMD
/// object in the `_DAT_8007B85C + 0x5DC00` scratch block - group header
/// `count = 0x40`, `flags = 0x22` (dispatch kind 17, the flat-textured-quad
/// handler), `ilen = 6`, `mode = 0x2C` - handed to `FUN_80043390`. The **64**
/// primitives come from a 32-iteration loop writing two quads per column
/// pair (the historical "32 triangles per half" reading is falsified by the
/// packet: every primitive is a `POLY_FT4`):
///
/// * the **ring** quad `(p, p+1, p+3, p+4)` - inner/outer rim of column `c`
///   to inner/outer rim of column `c+1` - colour `0x2C808080`, and
/// * the **wall** quad `(p+2, p, p+5, p+3)` - the far-z copies joined to the
///   near inner rim - colour `0x2C404040`,
///
/// each with the texel pairs of its own vertices and the half's texture page
/// (`0x117` primary / `0x115` mirrored - the right and left 320-column halves
/// of the capture). The flag word `0x1880_8080` sets bit 27, so the dispatch
/// is **double-sided** ([`CullMode`]) - which is what lets the x-mirrored
/// half, whose winding is reversed, draw at all - and `a2 = 0` keeps it on
/// the opaque bank, so the `0x808080` in the flag word never multiplies
/// anything (`desc + 0xC` is zero and the tint block is skipped).
///
/// `band_z` is the band record's `+0x08` scalar **latched before the tick's
/// integration**: the per-band pose is identity rotation with translation
/// `(0, 0, band_z)` (`FUN_801D1888` at `0x801D1904..0x801D195C`), so the
/// scalar is the band's view depth, not a rotation angle - alternating rate
/// signs make alternating bands fly toward and away from the camera.
///
/// Past the late-phase frame the submit goes through `FUN_80029888` instead,
/// which stages a mid-grey far colour and builds an extra Euler rotation from
/// its fourth argument (`(clock - 0x3C) * 4`, `<< 4` into the angle lanes) -
/// the roll that gives the style its name. The port carries the rotation
/// ([`SwirlBandDraw::late_arg`]); the far-colour haze has no screen-overlay
/// channel and is left un-carried, like the tile shatter's depth-cue note.
/// (The rotation-axis detail is graded decompiled-C: the Euler vector build
/// in `FUN_80029888` is read off the C rendering, not the disassembly.)
///
/// PORT: FUN_801D1A20
/// REF: FUN_80029888 (the late-phase dispatcher; far colour + roll staging)
pub fn emit_swirl_band(
    mesh: &SwirlMesh,
    band_z: i32,
    draw: &SwirlBandDraw,
    prims: &mut Vec<ScreenPrim>,
) -> bool {
    let tr = GteVec3::new(0, 0, band_z);
    let rot = match draw.late_arg {
        // `local_44 = param_4 << 4` feeds the Euler build's x and z lanes.
        Some(n) => {
            let a = (n << 4) as i16;
            euler_rot_psx((a, 0, a))
        }
        None => GteMat3::IDENTITY,
    };
    let mut drawn = false;
    for seg in 0..swirl::COLUMNS - 1 {
        let p = draw.first_vertex + seg * swirl::VERTS_PER_COLUMN;
        for (idx, rgb) in [
            ([p, p + 1, p + 3, p + 4], SWIRL_RING_RGB),
            ([p + 2, p, p + 5, p + 3], SWIRL_WALL_RGB),
        ] {
            let pc: [ProjCorner; 4] = std::array::from_fn(|i| {
                let v = mesh.vertices[idx[i]];
                project_intro_corner(&rot, tr, (v.x, v.y, v.z))
            });
            let uv: [(u8, u8); 4] = std::array::from_fn(|i| {
                let (u, v) = mesh.texels[idx[i]];
                (u as u8, v as u8)
            });
            drawn |= push_ft4_quad(
                &pc,
                uv,
                0,
                draw.tpage as u16,
                rgb,
                false,
                CullMode::DoubleSided,
                prims,
            );
        }
    }
    drawn
}

/// The swirl mesh builder's trig tables, in the phase the packet's own texel
/// math pins.
///
/// The build samples `_DAT_8007B81C` for x and `_DAT_8007B7F8` for y over
/// entries `0..=2048` (half the 4096-entry space). The primary half's u is
/// `(x >> 4) + 0x20` into the **right**-half page `0x117` and the mirrored
/// half's `-0x61 - (x >> 4)` into the left-half page `0x115`; both stay
/// inside their 320-column capture halves only when x is non-negative over
/// the sampled range - so the x table is **sine-phased** (0 at entry 0) and
/// the y table cosine-phased, spanning the full ±0x760 the `v` bias needs.
/// [`BattleIntro::new`] therefore builds the mesh with this pair - the exact
/// PSX table via [`psx_sin`] - rather than with the host-supplied
/// [`swirl::SwirlTrig`], whose phase convention (shared with the particle
/// seeders' heading tables) is the transpose.
struct SwirlTables;

impl swirl::SwirlTrig for SwirlTables {
    fn table_x(&mut self, entry: i32) -> i16 {
        psx_sin((entry & 0xFFF) as u16) as i16
    }
    fn table_y(&mut self, entry: i32) -> i16 {
        psx_cos((entry & 0xFFF) as u16) as i16
    }
}

// ---------------------------------------------------------------------------
// The curtain's no-clear accumulation (FUN_801D1D9C + the per-frame wash)
// ---------------------------------------------------------------------------

/// OT layer `FUN_801D11D0` hands the mid-pass emitter (`0x801D14A4`:
/// `FUN_801D1D9C(0x1EA, 2, 0x808080)`) - between the draw-area install at
/// `0x1F4` and the column strips at `0x1C2`, so the decay lands on the
/// intermediate **before** this frame's columns draw over it.
pub const CURTAIN_MIDPASS_OT: u32 = 0x1EA;
/// The emitter's second argument: ABR mode `2`, `B - F` - the quad subtracts.
pub const CURTAIN_MIDPASS_ABR: u8 = 2;
/// The emitter's third argument: `0x80` per channel, the amount subtracted
/// from the whole intermediate each frame.
pub const CURTAIN_MIDPASS_RGB: u32 = 0x0080_8080;
/// [`CURTAIN_MIDPASS_RGB`] as a 5-bit-channel step (`0x80 >> 3`): what one
/// frame of the mid-pass quad takes off a 15-bpp intermediate pixel.
pub const CURTAIN_MIDPASS_DECAY_5: u16 = 0x10;

/// The per-frame display wash the curtain arms
/// (`legaia_engine_vm::battle_intro_styles::CURTAIN_WASH_RGB` = `0x80808`,
/// re-armed unconditionally at `0x801D1228..0x801D1230` on **every** frame),
/// as a 5-bit-channel step: 8 per 8-bit channel is one 5-bit level, so an
/// undrawn display pixel takes ~31 frames to reach black.
pub const CURTAIN_DISPLAY_DECAY_5: u16 = 1;

/// Where the port keeps its CPU model of the transition's display buffer in
/// the cloned VRAM page, so the trail can be drawn as textured quads. Retail
/// needs no such rect - on the console the display buffer *is* VRAM and the
/// no-clear accumulation is free. The rect is otherwise unused during a
/// transition: whatever scene data the clone holds there is invisible for the
/// whole window (the backdrop covers the live scene), and the clone is
/// dropped when the transition ends.
pub const CURTAIN_TRAIL_RECT: VramRect = VramRect::new(640, 256, 320, 240);

/// OT bucket the trail's backdrop quads link at: farther than every on-screen
/// strip (rows at `0x12C`), nearer than the opaque backdrop - the trail *is*
/// the frame the strips draw over.
pub const CURTAIN_TRAIL_OT: u32 = 0x300;

/// Subtract `step` from each 5-bit channel of every 15-bpp pixel, saturating
/// at zero and preserving the mask bit - one application of an ABR-2
/// (`B - F`) full-rect quad with `step` in every channel. A whole-word zero
/// (the overlay shader's hole) stays a hole.
fn decay_15bpp(buf: &mut [u16], step: u16) {
    for p in buf.iter_mut() {
        let v = *p;
        if v == 0 {
            continue;
        }
        *p = (v & 0x1F).saturating_sub(step)
            | ((v >> 5) & 0x1F).saturating_sub(step) << 5
            | ((v >> 10) & 0x1F).saturating_sub(step) << 10
            | (v & 0x8000);
    }
}

/// Overlay VA of the transition sprite descriptor table, inside PROT 0979
/// `field_battle_intro`.
pub const INTRO_QUAD_TABLE_VA: u32 = 0x801D_1EC4;

/// Records of [`INTRO_QUAD_TABLE_VA`] the curtain actually indexes.
///
/// The style patches indices `2` and `3` and draws them; `0` and `1` are the
/// full-screen halves (`0xC0`- and `0x80`-wide, `0xF0` tall) the mid-pass
/// rectangles use. Past `3` the bytes stop being descriptor-shaped, which is
/// what bounds the table - it carries no count.
pub const INTRO_QUAD_TABLE_LEN: usize = 4;

/// The parsed descriptor table.
///
/// Held by value because [`legaia_engine_vm::battle_intro_styles::tick_curtain`]
/// **patches records in place** before every quad - that is retail's own
/// mechanism, not a port shortcut - so the table is per-transition mutable
/// state rather than a shared read-only asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntroQuadTable(pub Vec<IntroQuadDesc>);

impl IntroQuadTable {
    /// Parse the table out of a PROT 0979 image already relocated to its load
    /// base (`legaia_asset::static_overlay::as_loaded`).
    ///
    /// Returns `None` when the image is too short to hold the table, which is
    /// the only failure that can be detected structurally - the records carry
    /// no magic.
    pub fn parse_overlay(as_loaded: &[u8], base_va: u32) -> Option<Self> {
        let off = INTRO_QUAD_TABLE_VA.checked_sub(base_va)? as usize;
        let mut out = Vec::with_capacity(INTRO_QUAD_TABLE_LEN);
        for i in 0..INTRO_QUAD_TABLE_LEN {
            let at = off.checked_add(i * INTRO_QUAD_DESC_STRIDE)?;
            out.push(IntroQuadDesc::parse(as_loaded.get(at..)?)?);
        }
        Some(Self(out))
    }

    /// A neutral stand-in with the shape the curtain needs, for hosts that
    /// have no disc image to hand. Unity scale, white on both edges - so the
    /// strips carry the captured frame unmodulated.
    ///
    /// The four extents are **not** interchangeable, and a uniform `1 x 1`
    /// stand-in silently broke the column pass: `tick_curtain` patches `w` and
    /// `v0` on the row record but only `u0` and `tpage` on the column one, so
    /// the column strip's height comes from the table. The values here are the
    /// ones `battle_intro_table_real.rs` asserts of the disc records: records
    /// `0` / `1` the two full-screen halves, `2` a full-height column, `3` a
    /// single scanline.
    pub fn neutral() -> Self {
        let desc = |w: u8, h: u8| IntroQuadDesc {
            size_q12: 0x1000,
            w,
            h,
            top: [0xFF; 3],
            bottom: [0xFF; 3],
            ..Default::default()
        };
        Self(vec![
            desc(styles::CURTAIN_LEFT_W, styles::CURTAIN_ROWS as u8),
            desc(styles::CURTAIN_RIGHT_W, styles::CURTAIN_ROWS as u8),
            desc(1, styles::CURTAIN_ROWS as u8),
            desc(1, 1),
        ])
    }
}

/// The per-style working set, one variant per [`IntroStyle`].
#[derive(Debug, Clone)]
enum WorkingSet {
    Particles {
        grid: Vec<legaia_engine_vm::battle_intro_particles::IntroParticle>,
        style: &'static styles::ParticleTickStyle,
    },
    Tiles(Box<TileGrid>),
    Curtain(IntroQuadTable),
    Swirl {
        mesh: Box<SwirlMesh>,
        prev_clock: i32,
    },
}

/// What one emitted frame produced.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IntroFrame {
    /// The primitives, already in the OT buckets retail links them at. Empty
    /// for a style whose packet builder is not ported.
    pub prims: Vec<ScreenPrim>,
    /// Whether the style's own geometry reached [`Self::prims`] this frame, as
    /// against the frame carrying only the fade quad (or nothing).
    pub style_drawn: bool,
    /// The fade the second tail switch resolved, if its ramp has started.
    pub fade: Option<IntroFade>,
}

/// The field-to-battle transition's per-frame emitter.
///
/// One is armed when the encounter session enters its `Transition` phase and
/// dropped when it leaves. It owns the style's working set and its own copy of
/// the clock; the clock is **synchronised** from the live transition entity
/// each frame rather than free-running, so the emitter can never drift from
/// the state machine that decides when the battle actually opens.
#[derive(Clone)]
pub struct BattleIntro {
    style: IntroStyle,
    sub_style: i32,
    set: WorkingSet,
    clock: i16,
    total_duration: i32,
    /// The VRAM page with the captured field frame blitted into it, `None`
    /// until [`BattleIntro::land_capture_rgba`] runs. Owned here rather than
    /// written back into the host's page because the capture is transient: it
    /// exists for the transition and the field base must survive it unedited.
    captured: Option<Vram>,
    /// A capture landed since [`BattleIntro::refresh_captured_page`] last ran,
    /// so the host's uploaded copy is stale.
    upload_pending: bool,
    /// Read the capture's RGBA rows bottom-up (WebGL `readPixels` order).
    capture_flip_y: bool,
    /// The field-character texture pack (PROT 0874 §2), whose **entry 0** is
    /// the tile shatter's shade page: a 256x256 4bpp TIM at `(448, 0)` with
    /// its 16-CLUT block landing as a 256x1 strip on row 473. Retail keeps it
    /// resident for the whole field session; the engine's field VRAM
    /// deliberately does not (its extra pages would clobber rects the town
    /// meshes sample), so the capture re-lands it in the **cloned** page the
    /// transition draws against. `None` = no disc access; the shade faces
    /// then sample whatever the base page holds.
    shade_pack: Option<legaia_asset::field_char_textures::FieldCharTextures>,
    /// The captured frame lifted out of [`FIELD_CAPTURE_COLS`] as a plain
    /// `320 x 240` halfword image, so the curtain's column pass can sample it
    /// while the same page is being written. Empty for every other style.
    capture_src: Vec<u16>,
    /// This frame's **column**-pass quads, held between [`BattleIntro::tick`]
    /// (which builds them) and [`BattleIntro::refresh_captured_page`] (which
    /// rasterises them into the intermediate). Only the curtain fills it - see
    /// the module docs on why these never reach the screen.
    pending_columns: Vec<IntroQuad>,
    /// This frame's **row**-pass quads, kept alongside their on-screen
    /// emission so [`BattleIntro::refresh_captured_page`] can also land them in
    /// the CPU display model the trail decays. Only the curtain fills it.
    pending_rows: Vec<IntroQuad>,
    /// The curtain's intermediate ([`FIELD_CAPTURE_ROWS`] content) as a
    /// persistent buffer: retail never clears it - `FUN_801D1D9C` decays it by
    /// [`CURTAIN_MIDPASS_DECAY_5`] each frame before the column strips draw.
    /// Empty for every other style.
    intermediate: Vec<u16>,
    /// The CPU model of the transition's display buffer: seeded from the field
    /// capture (retail's init lands the drawn frame in both display buffers),
    /// decayed by [`CURTAIN_DISPLAY_DECAY_5`] each frame (the re-armed
    /// `0x80808` wash), and overdrawn with each frame's row strips. Uploaded
    /// into [`CURTAIN_TRAIL_RECT`] *before* the rows land, so the on-screen
    /// trail quads show exactly what retail's undrawn gaps show. Empty for
    /// every other style.
    display_accum: Vec<u16>,
}

impl BattleIntro {
    /// Arm the emitter for `style`.
    ///
    /// `sub_style` is `DAT_801D2464` - it selects the tile seeder's arm and
    /// the scatter style's fade depth. `table` is only consulted by
    /// [`IntroStyle::Curtain`]. `env` supplies the trig / sqrt / PRNG the
    /// seeders call; the two particle styles and the tiles need it, the
    /// curtain does not. `_trig` is kept as the documented seam for a host
    /// carrying the real `_DAT_8007B7F8` / `_DAT_8007B81C` tables, but the
    /// swirl mesh is currently built from [`SwirlTables`] - the packet's
    /// texel math pins the phase, and the host convention is its transpose.
    pub fn new(
        style: IntroStyle,
        sub_style: i32,
        total_duration: i32,
        table: IntroQuadTable,
        env: &mut dyn legaia_engine_vm::battle_intro_particles::ParticleEnv,
        _trig: &mut dyn swirl::SwirlTrig,
        tile_corners: [i32; 4],
    ) -> Self {
        use legaia_engine_vm::battle_intro_particles as particles;
        let set = match style {
            IntroStyle::ScatterParticles | IntroStyle::SpinUpParticles => {
                let (seed, tick) = if style == IntroStyle::ScatterParticles {
                    (&particles::STYLE_CFBB4, &PARTICLE_TICK_A)
                } else {
                    (&particles::STYLE_D0164, &PARTICLE_TICK_B)
                };
                let grid = match particles::seed_particle_grid(seed, true, env) {
                    particles::SeedOutcome::Seeded(g) => g,
                    particles::SeedOutcome::OutOfMemory => Vec::new(),
                };
                WorkingSet::Particles { grid, style: tick }
            }
            IntroStyle::TileShatter => {
                let sub = match sub_style {
                    0 => tiles::TileSubStyle::NegSpinRandomDelay,
                    1 => tiles::TileSubStyle::PosSpinRandomDelay,
                    2 => tiles::TileSubStyle::RadialDelayWithTumble,
                    _ => tiles::TileSubStyle::None,
                };
                match tiles::seed_tile_grid(sub, true, tile_corners, env) {
                    tiles::TileSeedOutcome::Seeded(g) => WorkingSet::Tiles(g),
                    tiles::TileSeedOutcome::OutOfMemory => WorkingSet::Tiles(Box::new(TileGrid {
                        vertices: Vec::new(),
                        tiles: Vec::new(),
                    })),
                }
            }
            IntroStyle::Curtain => WorkingSet::Curtain(table),
            // The mesh is built with the module's own [`SwirlTables`] rather
            // than the host's `trig`: the packet's texel math pins the x
            // table as sine-phased (see `SwirlTables`), while the host's
            // convention - shared with the particle seeders - is the
            // transpose. The parameter stays because the trait is the
            // documented seam and a future host may carry the real tables.
            IntroStyle::Swirl => match swirl::build_swirl_mesh(true, &mut SwirlTables) {
                swirl::SwirlBuildOutcome::Built(mesh) => WorkingSet::Swirl {
                    mesh,
                    prev_clock: 0,
                },
                swirl::SwirlBuildOutcome::OutOfMemory => WorkingSet::Swirl {
                    mesh: Box::new(SwirlMesh {
                        bands: [Default::default(); swirl::BANDS],
                        vertices: Vec::new(),
                        texels: Vec::new(),
                    }),
                    prev_clock: 0,
                },
            },
        };
        Self {
            style,
            sub_style,
            set,
            clock: 0,
            total_duration,
            captured: None,
            upload_pending: false,
            capture_flip_y: false,
            shade_pack: None,
            capture_src: Vec::new(),
            pending_columns: Vec::new(),
            pending_rows: Vec::new(),
            intermediate: Vec::new(),
            display_accum: Vec::new(),
        }
    }

    /// Attach the field-character texture pack whose entry 0 is the shade
    /// page (see the `shade_pack` field). Only the tile shatter consults it.
    pub fn with_shade_pack(
        mut self,
        pack: Option<legaia_asset::field_char_textures::FieldCharTextures>,
    ) -> Self {
        self.shade_pack = pack;
        self
    }

    /// The style this emitter is running.
    pub fn style(&self) -> IntroStyle {
        self.style
    }

    /// Whether the field frame has already been landed in VRAM. The capture is
    /// a one-shot: retail stashes the frame once as the transition arms and
    /// every subsequent frame samples that copy, which is exactly why the
    /// strips keep showing the field after the field has stopped being drawn.
    pub fn needs_capture(&self) -> bool {
        self.captured.is_none()
    }

    /// The captured page, once [`BattleIntro::land_capture_rgba`] has run.
    pub fn captured_vram(&self) -> Option<&Vram> {
        self.captured.as_ref()
    }

    /// This frame's curtain **column**-pass quads - the half that renders the
    /// intermediate rather than the display. Empty for every other style, and
    /// between the tick and the next one for a style that emitted none.
    pub fn pending_column_quads(&self) -> &[IntroQuad] {
        &self.pending_columns
    }

    /// Land the one-shot field-frame capture: blit the drawn frame (an RGBA8
    /// readback, any resolution - it is point-sampled down) into the rects
    /// [`capture_rects_for`] names - [`FIELD_CAPTURE_COLS`] for every style,
    /// since no style samples the rows rect (see the module docs). The mask
    /// bit is set so a black field pixel samples as opaque black rather than
    /// as a transparent hole.
    ///
    /// `base` is the scene's own VRAM, which is **cloned** rather than edited:
    /// the capture is transient and the host's pristine page has to survive
    /// the transition. A second call is a no-op - retail stashes the frame
    /// once as the transition arms.
    ///
    /// `flip_y` reads the source bottom-up - WebGL's `readPixels` returns
    /// rows in that order, the native `capture_rgba` readback is top-down.
    ///
    /// How a host obtains the RGBA frame is the one genuinely renderer-bound
    /// step of this module and stays with each host: the native window's
    /// `capture_rgba` re-render, the play page's `gl.readPixels`.
    pub fn land_capture_rgba(&mut self, rgba: &[u8], width: u32, height: u32, base: &Vram) {
        if self.captured.is_some() {
            return;
        }
        let flipped;
        let rgba = if self.capture_flip_y && height > 1 {
            let stride = width as usize * 4;
            flipped = rgba
                .chunks_exact(stride)
                .rev()
                .flatten()
                .copied()
                .collect::<Vec<u8>>();
            &flipped[..]
        } else {
            rgba
        };
        let style = self.style;
        let shade = if style == IntroStyle::TileShatter {
            self.shade_pack.as_ref()
        } else {
            None
        };
        let page = self.captured.insert(base.clone());
        let opts = CaptureOpts { set_mask_bit: true };
        for rect in capture_rects_for(style) {
            crate::vram_capture::blit_rgba_into_vram(rgba, width, height, page, *rect, opts);
        }
        // The shade page + its row-473 CLUT strip (see `shade_pack`). Entry
        // 0 only: the pack's character-atlas entries are already in the
        // base page via the host's field upload, and its other shared pages
        // land on rects the scene meshes sample.
        if let Some(pack) = shade {
            let mut entry0 = pack.clone();
            entry0.textures.retain(|t| t.index == 0);
            entry0.upload_to_vram(page, false);
        }
        if style == IntroStyle::Curtain {
            self.capture_src = lift_capture_src(page);
        }
        self.upload_pending = true;
    }

    /// Read the capture bottom-up when it lands (see
    /// [`BattleIntro::land_capture_rgba`]).
    pub fn with_flipped_capture(mut self) -> Self {
        self.capture_flip_y = true;
        self
    }

    /// Bring the transition's private VRAM page up to date for this frame, and
    /// say whether the host has to re-upload it.
    ///
    /// The per-frame half of what was one job with the capture: for the
    /// curtain, the column pass is rasterised into [`FIELD_CAPTURE_ROWS`],
    /// which is what its row pass samples - retail's own two-pass structure,
    /// not a port convenience (see the module docs) - and the display-trail
    /// model advances. Call it once per emitter tick, **after**
    /// [`BattleIntro::land_capture_rgba`] on the frame the capture arrives.
    ///
    /// Returns `Some(page)` on any frame whose contents changed - the capture
    /// frame for every style, and every frame for the curtain. `None` means
    /// the last uploaded page is still correct.
    pub fn refresh_captured_page(&mut self) -> Option<&Vram> {
        let landed = std::mem::take(&mut self.upload_pending);
        if self.style != IntroStyle::Curtain {
            return if landed { self.captured.as_ref() } else { None };
        }
        self.captured.as_ref()?;
        self.compose_curtain_intermediate();
        self.update_curtain_display_trail();
        self.captured.as_ref()
    }

    /// Install `page` as though the one-shot capture had already run, so the
    /// curtain's two-pass composition is drivable without a GPU. A test seam -
    /// `pub` (not `cfg(test)`) so `engine-render`'s emitter tests, which live
    /// with the GPU harness in that crate, can reach it across the boundary.
    #[doc(hidden)]
    pub fn seed_capture_for_test(&mut self, page: Vram) {
        self.capture_src = lift_capture_src(&page);
        self.captured = Some(page);
    }

    /// Run the curtain's column pass into the intermediate. Test seam for one
    /// renderer-free half of [`BattleIntro::refresh_captured_page`].
    #[doc(hidden)]
    pub fn compose_intermediate_for_test(&mut self) {
        self.compose_curtain_intermediate();
    }

    /// Run the curtain's display-model step. Test seam for the other
    /// renderer-free half of [`BattleIntro::refresh_captured_page`].
    #[doc(hidden)]
    pub fn update_display_trail_for_test(&mut self) {
        self.update_curtain_display_trail();
    }

    /// The CPU display model, for inspection.
    #[doc(hidden)]
    pub fn display_accum_for_test(&self) -> &[u16] {
        &self.display_accum
    }

    /// This frame's row-pass quads, for inspection.
    #[doc(hidden)]
    pub fn pending_row_quads_for_test(&self) -> &[IntroQuad] {
        &self.pending_rows
    }

    /// Rasterise this frame's column-pass quads into [`FIELD_CAPTURE_ROWS`],
    /// the intermediate the curtain's row pass samples.
    ///
    /// The port has no render-to-VRAM target for screen-space quads, so the
    /// pass runs on the CPU. Each quad is one texel wide and `h` pixels tall
    /// with its `v` running the full source column, which makes the rasteriser
    /// a per-column vertical resample rather than a general triangle setup -
    /// exactly the shape `build_intro_quad` produced.
    ///
    /// The intermediate is **not cleared between frames**. Retail's mid-pass
    /// emitter `FUN_801D1D9C(0x1EA, 2, 0x808080)` pushes an ABR-2 quad over
    /// the whole rect between the draw-area install and the column strips, so
    /// the previous frame's content decays by [`CURTAIN_MIDPASS_DECAY_5`] per
    /// channel and this frame's columns draw over the ghost - a culled column
    /// fades out over two frames instead of vanishing (see the module docs).
    ///
    /// PORT: FUN_801D1D9C
    fn compose_curtain_intermediate(&mut self) {
        let rect = FIELD_CAPTURE_ROWS;
        let (rw, rh) = (rect.w as usize, rect.h as usize);
        if self.capture_src.len() < rw * rh {
            return;
        }
        if self.intermediate.len() != rw * rh {
            // First frame: the rect starts opaque black (nothing has drawn the
            // intermediate yet; the identity-warp columns cover it entirely
            // this same frame).
            self.intermediate = vec![0x8000u16; rw * rh];
        } else {
            decay_15bpp(&mut self.intermediate, CURTAIN_MIDPASS_DECAY_5);
        }
        for q in &self.pending_columns {
            blit_intro_quad(
                q,
                &self.capture_src,
                FIELD_CAPTURE_COLS,
                rect,
                &mut self.intermediate,
            );
        }
        if let Some(page) = self.captured.as_mut() {
            page.write_block(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                bytemuck::cast_slice(&self.intermediate),
            );
        }
    }

    /// Advance the CPU model of the transition's display buffer and land it in
    /// [`CURTAIN_TRAIL_RECT`].
    ///
    /// Order matters and is retail's: the wash decays what the display already
    /// holds, the row strips then draw over it. So the model is decayed and
    /// **uploaded first** - that snapshot is what the trail quads show under
    /// this frame's live GPU-drawn strips - and this frame's rows are
    /// rasterised into it afterwards, for the next frame's trail.
    ///
    /// Seeded from the field capture: retail's transition init lands the drawn
    /// field frame in *both* display buffers (`DrawSync` + `MoveImage`), so
    /// the first frame's background is the field itself, already one wash step
    /// down (the wash is re-armed on every frame including the first).
    fn update_curtain_display_trail(&mut self) {
        let (w, h) = (PSX_SCREEN_WIDTH as usize, PSX_SCREEN_HEIGHT as usize);
        if self.capture_src.len() < w * h {
            return;
        }
        if self.display_accum.len() != w * h {
            self.display_accum = self.capture_src.clone();
        }
        decay_15bpp(&mut self.display_accum, CURTAIN_DISPLAY_DECAY_5);
        if let Some(page) = self.captured.as_mut() {
            let r = CURTAIN_TRAIL_RECT;
            page.write_block(
                r.x,
                r.y,
                r.w,
                r.h,
                bytemuck::cast_slice(&self.display_accum),
            );
        }
        let screen = VramRect::new(0, 0, PSX_SCREEN_WIDTH, PSX_SCREEN_HEIGHT);
        let rows = std::mem::take(&mut self.pending_rows);
        for q in &rows {
            blit_intro_quad(
                q,
                &self.intermediate,
                FIELD_CAPTURE_ROWS,
                screen,
                &mut self.display_accum,
            );
        }
        self.pending_rows = rows;
    }

    /// The five textured backdrop quads that put [`CURTAIN_TRAIL_RECT`] on
    /// screen behind the live row strips - the port's stand-in for the display
    /// buffer retail draws over without clearing. Five because a 15-bpp
    /// texture page is 64 texels wide, so the 320 columns tile five pages
    /// (the same scheme the particle capture pages use).
    fn curtain_trail_prims(&self, prims: &mut Vec<ScreenPrim>) {
        if self.display_accum.is_empty() {
            return;
        }
        let r = CURTAIN_TRAIL_RECT;
        let base_page = (r.x / 64) | ((r.y / 256) << 4) | (2 << 7);
        for k in 0..(r.w / 64) {
            let x0 = (k * 64) as i16;
            prims.push(ScreenPrim::Textured(ScreenQuad {
                xy: [
                    (x0, 0),
                    (x0 + 64, 0),
                    (x0, r.h as i16),
                    (x0 + 64, r.h as i16),
                ],
                uv: [(0, 0), (64, 0), (0, r.h as u8), (64, r.h as u8)],
                clut: 0,
                tpage: base_page + k,
                color: 0x0080_8080,
                gouraud: None,
                semi_transparent: false,
                ot_index: CURTAIN_TRAIL_OT,
            }));
        }
    }

    /// Advance one frame and emit.
    ///
    /// `elapsed` is the live transition entity's `+0x1A`; the emitter adopts it
    /// rather than counting for itself, so a host that stalls or repeats a
    /// simulation tick cannot desynchronise the visuals from the handoff.
    /// `frame_step` is retail's per-frame display-frame delta (`1` at the
    /// steady NTSC cadence).
    ///
    /// `prims[0]` is always [`backdrop_prim`] - the frame the style composes
    /// onto - including on frames the style itself draws nothing. Its OT
    /// bucket is the farthest one, so its position in submission order is
    /// immaterial to the draw order and it is emitted first only to keep the
    /// fade quad the list's last element.
    pub fn tick(&mut self, elapsed: i16, frame_step: u8) -> IntroFrame {
        self.clock = elapsed;
        let mut out = IntroFrame::default();
        out.prims.push(backdrop_prim());
        // The curtain's display trail: the previous frames' pixels, decayed by
        // the per-frame wash, drawn under this frame's live strips - retail
        // gets it by never clearing the display buffer. Empty (no quads) until
        // the first capture landing has seeded the model, which is also
        // the frame the capture itself lands.
        if self.style == IntroStyle::Curtain {
            self.curtain_trail_prims(&mut out.prims);
        }

        match &mut self.set {
            WorkingSet::Particles { grid, style } => {
                // Same first-frame gate as the tiles below: retail's first
                // frame projects through the field camera's stale view
                // matrix; from frame two the view is identity rotation +
                // zero translation (pinned live), which is what
                // `project_intro_corner` hard-codes.
                let emit = self.clock != 0;
                // `FUN_801CFDA0` (and only it - `stamp_field_16` is its
                // marker) washes the screen near-black on every frame after
                // the first; the confetti reconstructs the frame over it
                // until the delays expire.
                if emit && style.stamp_field_16 {
                    out.prims.push(wash_prim(PARTICLE_WASH_RGB));
                }
                // The spin-up tail's ring phase uses the pre-increment clock.
                let ring_phase = i32::from(self.clock) * SPINUP_RING_PHASE_STEP;
                let drawn = emit_particle_field(
                    grid,
                    style,
                    &mut self.clock,
                    frame_step,
                    emit,
                    &mut out.prims,
                );
                // `FUN_801D0370`'s tail (`spin_up` is its marker): the
                // expanding ring behind the confetti.
                let ring = emit && style.spin_up && emit_spinup_ring(ring_phase, &mut out.prims);
                out.style_drawn = drawn || ring;
            }
            WorkingSet::Tiles(grid) => {
                // Retail's first shatter frame projects through the field
                // camera's stale view matrix (the transition setup rewrites
                // the scratch matrix only after it), which puts every tile
                // behind the near plane - so frame one draws no tiles, and
                // `not_first_frame` (`_DAT_8007B6CC`) is the same signal.
                // From frame two the view is identity rotation + zero
                // translation (pinned live), which is what `emit_tile`
                // hard-codes.
                let emit = self.clock != 0;
                let before = out.prims.len();
                tiles::tick_tile_grid_emit(grid, &mut self.clock, frame_step, |rec| {
                    if emit {
                        emit_tile(rec, &mut out.prims);
                    }
                });
                out.style_drawn = out.prims.len() > before;
            }
            WorkingSet::Curtain(table) => {
                let tick = styles::tick_curtain(&mut table.0, &mut self.clock, frame_step);
                // The two passes go to two different places. Retail installs a
                // draw area at VRAM (320, 0) for the column pass and restores
                // the back buffer before the row pass, so only the rows are on
                // screen; the columns render the intermediate the rows sample.
                // See the module docs for the OT-bucket evidence.
                self.pending_columns.clear();
                self.pending_rows.clear();
                out.prims.reserve(tick.quads.len());
                for q in &tick.quads {
                    if q.desc_index == styles::CURTAIN_COL_DESC {
                        self.pending_columns.push(q.quad);
                    } else {
                        self.pending_rows.push(q.quad);
                        out.prims
                            .push(ScreenPrim::Textured(intro_quad_to_screen(&q.quad)));
                    }
                }
                out.style_drawn = !tick.quads.is_empty();
            }
            WorkingSet::Swirl { mesh, prev_clock } => {
                // Latch each band's view depth before the tick integrates it:
                // retail stages the per-band translation and *then* advances
                // the scalar, so the draw uses the frame-entry value.
                let emit = self.clock != 0;
                let zs: [i32; swirl::BANDS] = std::array::from_fn(|i| mesh.bands[i].angle);
                let tick = swirl::tick_swirl(mesh, &mut self.clock, frame_step, prev_clock);
                if tick.late_wash {
                    out.prims.push(wash_prim(swirl::LATE_WASH_RGB));
                }
                if emit {
                    for d in &tick.draws {
                        let band = d.first_vertex / swirl::VERTS_PER_BAND;
                        out.style_drawn |= emit_swirl_band(mesh, zs[band], d, &mut out.prims);
                    }
                }
            }
        }

        out.fade = styles::intro_fade(
            self.style,
            self.sub_style,
            i32::from(elapsed),
            self.total_duration,
        );
        if let Some(f) = out.fade {
            out.prims.push(fade_quad(&f));
        }
        out
    }
}

/// Lift [`FIELD_CAPTURE_COLS`] out of `page` as a flat halfword image.
///
/// The curtain's column pass samples that rect while writing
/// [`FIELD_CAPTURE_ROWS`] of the same page, which a borrow cannot express - and
/// the source never changes after the one-shot capture, so a copy is also the
/// cheaper answer.
fn lift_capture_src(page: &Vram) -> Vec<u16> {
    let src = FIELD_CAPTURE_COLS;
    (0..src.h as usize)
        .flat_map(|y| (0..src.w as usize).map(move |x| (src.x as usize + x, src.y as usize + y)))
        .map(|(x, y)| page.pixel(x, y))
        .collect()
}

/// PSX texture modulation on one 5-bit channel: `texel * colour / 128`,
/// saturated. `0x80` is the neutral colour, which is why the curtain's `0x7F`
/// (`0xFF` shaded by the pass' `0x80` intensity) reads as "carry the capture".
///
/// Rounded rather than truncated, and that is a deliberate divergence. The
/// hardware truncates, but the port's own row pass runs the same modulation in
/// the fragment shader in floating point and rounds only at the UNORM8 write -
/// so a truncating column pass would drop a whole 5-bit level per channel
/// before the row pass ever saw it, and the two halves of one effect would
/// disagree about a neutral modulation. Rounding makes `0x7F` the no-op it is
/// meant to be on both sides.
fn modulate5(c5: u16, colour: u8) -> u16 {
    ((u32::from(c5) * u32::from(colour) + 64) >> 7).min(31) as u16
}

/// Rasterise one axis-aligned curtain quad from one buffer into another.
///
/// `src` is `src_rect` as a flat halfword image and `out` is `dst` as one;
/// the quad's coordinates are in `dst`'s space (absolute VRAM for the column
/// pass, whose draw offset is zero; screen space for the row pass) and its
/// texture page must resolve inside `src_rect`. Both passes' quads are
/// `POLY_GT4`s with axis-aligned corners - a column is one texel wide with
/// `v` spanning the whole source column, a row one pixel tall with `u`
/// spanning its half of the source scanline - so the mapping is a per-line
/// resample rather than a general triangle setup.
fn blit_intro_quad(q: &IntroQuad, src: &[u16], src_rect: VramRect, dst: VramRect, out: &mut [u16]) {
    let (tx, ty, depth) = (
        ((q.tpage & 0x0F) as i32) * 64,
        if q.tpage & 0x10 != 0 { 256 } else { 0 },
        (q.tpage >> 7) & 0x3,
    );
    // Only a 15-bpp page inside the source rect can be resolved out of `src`.
    if depth != 2 || ty != i32::from(src_rect.y) {
        return;
    }
    let (sw, sh) = (src_rect.w as i32, src_rect.h as i32);
    let (x0, y0) = (i32::from(q.verts[0].x), i32::from(q.verts[0].y));
    let (x1, y1) = (i32::from(q.verts[3].x), i32::from(q.verts[3].y));
    let (w, h) = (x1 - x0, y1 - y0);
    if w <= 0 || h <= 0 {
        return;
    }
    let (u0, v0) = (i32::from(q.verts[0].u), i32::from(q.verts[0].v));
    let (u1, v1) = (i32::from(q.verts[3].u), i32::from(q.verts[3].v));
    let (rx, ry) = (i32::from(dst.x), i32::from(dst.y));
    let (rw, rh) = (i32::from(dst.w), i32::from(dst.h));
    for dy in y0.max(ry)..y1.min(ry + rh) {
        let v = v0 + (dy - y0) * (v1 - v0) / h;
        // The gradient is per-vertex; the two edges are equal on the disc
        // records, so this is a no-op there and correct if they ever differ.
        let t = (dy - y0) as u32 * 256 / h as u32;
        let shade: [u8; 3] = std::array::from_fn(|i| {
            let (a, b) = (u32::from(q.verts[0].rgb[i]), u32::from(q.verts[3].rgb[i]));
            ((a * (256 - t) + b * t) >> 8) as u8
        });
        for dx in x0.max(rx)..x1.min(rx + rw) {
            let u = u0 + (dx - x0) * (u1 - u0) / w;
            let (sx, sy) = (tx + u - i32::from(src_rect.x), v);
            if !(0..sw).contains(&sx) || !(0..sh).contains(&sy) {
                continue;
            }
            // The overlay shader's transparency rule, applied to the same
            // texels: a whole-word zero is a hole. The capture sets the mask
            // bit, so a black *captured* pixel is `0x8000` and still draws.
            let texel = src[(sy * sw + sx) as usize];
            if texel == 0 {
                continue;
            }
            let r = modulate5(texel & 0x1F, shade[0]);
            let g = modulate5((texel >> 5) & 0x1F, shade[1]);
            let b = modulate5((texel >> 10) & 0x1F, shade[2]);
            out[((dy - ry) * rw + (dx - rx)) as usize] = r | (g << 5) | (b << 10) | 0x8000;
        }
    }
}

/// Convert one built transition quad into a screen-space primitive.
///
/// The mapping is direct because `FUN_801CF1B0` already emits a screen-space
/// `POLY_GT4`: its corners are pixels, its `tpage` / `clut` are the GP0 words,
/// and its per-vertex colours are the gradient. The vertex order is shared
/// (`v0` top-left, `v1` top-right, `v2` bottom-left, `v3` bottom-right), so no
/// re-ordering happens here either.
///
/// The `abr` bit is carried in the primitive's code byte
/// (`(abr << 1) | POLY_GT4`), and the ABR *mode* in TSB bits 5..=6 of the same
/// tpage word the quad already carries - which is why `semi_transparent` reads
/// the code byte while [`ScreenQuad::abr_mode`] reads the tpage.
pub fn intro_quad_to_screen(q: &IntroQuad) -> ScreenQuad {
    let rgb = |c: [u8; 3]| u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]);
    ScreenQuad {
        xy: std::array::from_fn(|i| (q.verts[i].x, q.verts[i].y)),
        uv: std::array::from_fn(|i| (q.verts[i].u, q.verts[i].v)),
        clut: q.clut,
        tpage: q.tpage,
        color: rgb(q.verts[0].rgb),
        gouraud: Some(std::array::from_fn(|i| rgb(q.verts[i].rgb))),
        semi_transparent: q.code & 0x02 != 0,
        ot_index: q.ot_depth,
    }
}

/// The full-screen quad `FUN_80024EE4` pushes for a resolved fade.
///
/// Both halves are retail's now. The **ramp** is
/// [`legaia_engine_vm::battle_intro_styles::intro_fade`]; the **quad** is the
/// emitter's own six-word packet, corners taken from the scratchpad
/// display-rect words (`_DAT_1F800378` / `_DAT_1F80037A`), which is the whole
/// PSX display - so the port draws the whole display rect. Command byte
/// `0x2B` makes it semi-transparent, and its ABR mode is the emitter's second
/// argument, folded into the `SetDrawMode` packet that precedes it.
///
/// That second argument used to be read here as an OT depth, which put every
/// style's fade on ABR `0` (`0.5B + 0.5F`). It halved both tails: the
/// additive styles ([`IntroStyle::SpinUpParticles`],
/// [`IntroStyle::Swirl`]) topped out at a washed grey instead of a full
/// white-out, and the subtractive ones never reached black. The OT layer is
/// `a0` - [`legaia_engine_vm::battle_intro_styles::INTRO_FADE_LAYER`], the
/// same `2` for all five styles - so that is what the bucket carries.
/// The quad itself is [`legaia_engine_ui::screen_prim::fade_prim`], which is
/// also what the browser play page emits: the fade is the one part of this
/// transition both hosts draw, so the two must not each build the packet.
pub fn fade_quad(f: &IntroFade) -> ScreenPrim {
    fade_prim(f.rgb, f.abr, u32::from(f.layer))
}

/// Compose the object record's three authored angles into a model rotation,
/// in retail's order.
///
/// `FUN_80026988` builds `Rx * Ry * Rz` from the halfwords `FUN_8003A55C`
/// copies out of the placement record (`+0x08`/`+0x0A`/`+0x0C`). glam's
/// per-axis constructors carry the same handedness as retail's table-driven
/// sin/cos in that frame, so no sign flips are needed - which is not
/// self-evident and is pinned by `glam_euler_matches_retail_composition`.
///
/// Angles are 12-bit (`4096` = one revolution).
///
/// REF: FUN_80026988, FUN_8003A55C, FUN_8001ADA4
pub fn placement_rotation(rot_x: u16, rot_y: u16, rot_z: u16) -> glam::Mat4 {
    let a = |v: u16| f32::from(v & 0x0FFF) * (std::f32::consts::TAU / 4096.0);
    glam::Mat4::from_rotation_x(a(rot_x))
        * glam::Mat4::from_rotation_y(a(rot_y))
        * glam::Mat4::from_rotation_z(a(rot_z))
}

#[cfg(test)]
mod placement_rotation_tests {
    use super::*;
    use glam::{Mat4, Vec3};

    fn gte_to_mat4(m: &GteMat3) -> Mat4 {
        let f = |v: i16| f32::from(v) / 4096.0;
        Mat4::from_cols(
            [f(m.m[0][0]), f(m.m[1][0]), f(m.m[2][0]), 0.0].into(),
            [f(m.m[0][1]), f(m.m[1][1]), f(m.m[2][1]), 0.0].into(),
            [f(m.m[0][2]), f(m.m[1][2]), f(m.m[2][2]), 0.0].into(),
            [0.0, 0.0, 0.0, 1.0].into(),
        )
    }

    /// [`placement_rotation`] reproduces [`euler_rot_psx`] - retail's own
    /// matrix builder - for tilted placements, not just pure yaw.
    ///
    /// Worth pinning because three plausible alternatives (reversed order,
    /// negated angles, both) all agree with retail on the pure-yaw and
    /// single-axis cases that dominate the disc, and only diverge once two
    /// axes are non-zero at the same time. A port that guessed wrong would
    /// look right on almost every placement and wrong on the handful that
    /// actually carry a tilt.
    #[test]
    fn glam_euler_matches_retail_composition() {
        // jagaroom record 82 (yaw + roll), then single axes, then a triple.
        for (x, y, z) in [
            (0u16, 0x0EA0u16, 0x0120u16),
            (0x0400, 0, 0),
            (0, 0, 0x0800),
            (0x0200, 0x0300, 0x0100),
            (0x0FBF, 0x0100, 0x0200),
        ] {
            let retail = gte_to_mat4(&euler_rot_psx((x as i16, y as i16, z as i16)));
            let ours = placement_rotation(x, y, z);
            for v in [
                Vec3::new(100.0, 200.0, 300.0),
                Vec3::new(-676.0, 0.0, 0.0),
                Vec3::new(0.0, -676.0, 0.0),
            ] {
                let (a, b) = (retail.transform_vector3(v), ours.transform_vector3(v));
                // Retail truncates to q3.12 per term, so a few tenths of a
                // world unit over a 700-unit lever arm is the expected floor.
                assert!(
                    (a - b).length() < 1.0,
                    "x={x:#x} y={y:#x} z={z:#x} v={v:?}: retail {a:?} vs ours {b:?}"
                );
            }
        }
    }
}

#[cfg(test)]
mod capture_landing_tests {
    use super::*;
    use legaia_engine_vm::battle_intro_particles::IntroEnv;

    fn bare_intro(style: IntroStyle) -> BattleIntro {
        let mut env = IntroEnv::new(1);
        let mut trig = IntroEnv::new(1);
        BattleIntro::new(
            style,
            0,
            132,
            IntroQuadTable::neutral(),
            &mut env,
            &mut trig,
            [0, 1, 0x11, 0x12],
        )
    }

    /// Two source rows, captured 1:1 into a 2-row rect: the top-down path
    /// lands them in order, the `with_flipped_capture` path (WebGL
    /// `readPixels` hands rows bottom-up) reverses them - and nothing else.
    #[test]
    fn a_flipped_capture_reverses_rows_and_only_rows() {
        // Row 0 red, row 1 blue, two pixels wide.
        let mut rgba = Vec::new();
        for c in [[0xF8u8, 0, 0], [0xF8, 0, 0], [0, 0, 0xF8], [0, 0, 0xF8]] {
            rgba.extend_from_slice(&[c[0], c[1], c[2], 0xFF]);
        }
        let red = 0x8000 | 0x1F;
        let blue = 0x8000 | (0x1F << 10);
        let (x, y) = (FIELD_CAPTURE_COLS.x as usize, FIELD_CAPTURE_COLS.y as usize);

        let mut plain = bare_intro(IntroStyle::ScatterParticles);
        plain.land_capture_rgba(&rgba, 2, 2, &Vram::new());
        let page = plain.captured_vram().expect("capture landed");
        // The 2x2 source point-samples across the 320x240 rect; the top half
        // reads row 0, the bottom half row 1.
        assert_eq!(page.pixel(x, y), red, "top-down: first row first");
        assert_eq!(page.pixel(x, y + 239), blue, "top-down: last row last");

        let mut flipped = bare_intro(IntroStyle::ScatterParticles).with_flipped_capture();
        flipped.land_capture_rgba(&rgba, 2, 2, &Vram::new());
        let page = flipped.captured_vram().expect("capture landed");
        assert_eq!(page.pixel(x, y), blue, "flipped: bottom row first");
        assert_eq!(page.pixel(x, y + 239), red, "flipped: top row last");
    }

    /// The capture is a one-shot and `refresh_captured_page` reports the
    /// landing exactly once for a non-curtain style - the page's re-upload
    /// contract.
    #[test]
    fn refresh_reports_the_landing_once_then_goes_quiet() {
        let mut it = bare_intro(IntroStyle::ScatterParticles);
        assert!(it.needs_capture());
        assert!(it.refresh_captured_page().is_none(), "nothing landed yet");
        let rgba = vec![0xFFu8; 2 * 2 * 4];
        it.land_capture_rgba(&rgba, 2, 2, &Vram::new());
        assert!(!it.needs_capture());
        assert!(
            it.refresh_captured_page().is_some(),
            "landing frame uploads"
        );
        assert!(
            it.refresh_captured_page().is_none(),
            "steady state does not"
        );
        // A second landing is ignored - retail stashes the frame once.
        let black = vec![0u8; 2 * 2 * 4];
        it.land_capture_rgba(&black, 2, 2, &Vram::new());
        assert!(it.refresh_captured_page().is_none(), "re-land is a no-op");
    }
}

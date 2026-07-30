//! The field-to-battle transition's per-frame emitter: the working-set owner
//! that stands between the wired transition clock and the ordering table.
//!
//! # What was missing, and what this is
//!
//! Everything either side of this module was already built. On the simulation
//! side `legaia_engine_vm::battle_intro_transition::tick_transition` is live -
//! `legaia_engine_core::World::tick_encounter` runs it every frame the
//! encounter session sits in its `Transition` phase - and all five style
//! kernels are ported. On the render side [`crate::screen_overlay`] is the
//! ordering table, [`crate::RenderTarget::SceneWithScreenPrims`] composites it
//! over a drawn scene, and [`crate::vram_capture`] lands a drawn frame back in
//! the software VRAM the strips sample.
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
//! # The capture is a two-rect affair, and both rects are used
//!
//! The curtain's row pass samples texture pages `0x105` / `0x108` and its
//! column pass `0x115` / `0x118`. Those decode to 15-bpp pages at VRAM
//! `(320, 0)` / `(512, 0)` and `(320, 256)` / `(512, 256)`, and each pass
//! covers columns `320..=639` across its pair - so the row pass reads the
//! capture at [`FIELD_CAPTURE_ROWS`] and the column pass an identical copy at
//! [`FIELD_CAPTURE_COLS`]. [`BattleIntro::capture_field_frame`] writes both.

use crate::billboard::{psx_cos, psx_sin};
use crate::gte::{GteMat3, GteVec3, ScreenXY, avsz4_with_scale, gte_divide, gte_persp_term, nclip};
use crate::screen_overlay::{FlatQuad, ScreenPrim, ScreenQuad};
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
/// | [`IntroStyle::Curtain`] | row pass `0x105`/`0x108` at `(320,0)`/`(512,0)`; column pass `0x115`/`0x118` at `(320,256)`/`(512,256)` | both |
/// | [`IntroStyle::TileShatter`] | `0x135`/`0x137` at `(320,256)`/`(448,256)`, plus the 4bpp [`TILE_SHADE_PAGE`] at `(448,0)` | **columns only** |
/// | the particle styles | `0x135..=0x139` at `(320,256)..(576,256)` (u < 64, so the five pages tile the 320 capture columns) | **columns only** |
/// | [`IntroStyle::Swirl`] | `0x115`/`0x117` at `(320,256)`/`(448,256)` | **columns only** |
///
/// Only the curtain's row pass samples the rows rect. The tile shatter is the
/// style where columns-only is load-bearing rather than economical: its own
/// pages are wholly inside the column rect, and the shade page it also needs
/// is inside the row rect - so writing the rows would destroy an input it
/// depends on while gaining it nothing. The particle and swirl pages are
/// likewise wholly inside the column rect (every page they name carries the
/// `y = 256` bit), so the rows blit would buy them nothing either.
pub fn capture_rects_for(style: IntroStyle) -> &'static [VramRect] {
    const BOTH: [VramRect; 2] = [FIELD_CAPTURE_ROWS, FIELD_CAPTURE_COLS];
    const COLS_ONLY: [VramRect; 1] = [FIELD_CAPTURE_COLS];
    match style {
        IntroStyle::Curtain => &BOTH,
        _ => &COLS_ONLY,
    }
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
/// [`crate::screen_overlay::order_primitives`]'s tie-break (later-submitted
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

/// A `FUN_8004695C` wash as a primitive: the call arms a full-screen colour
/// fill the frame composer draws behind everything, so the port models it as
/// an opaque display-rect quad at the farthest possible OT bucket.
pub fn wash_prim(gp0_rgb_word: u32) -> ScreenPrim {
    let (w, h) = (PSX_SCREEN_WIDTH as i16, PSX_SCREEN_HEIGHT as i16);
    ScreenPrim::Flat(FlatQuad {
        xy: [(0, 0), (w, 0), (0, h), (w, h)],
        color: [
            (gp0_rgb_word & 0xFF) as u8,
            ((gp0_rgb_word >> 8) & 0xFF) as u8,
            ((gp0_rgb_word >> 16) & 0xFF) as u8,
            0xFF,
        ],
        semi_transparent: false,
        abr_mode: 0,
        ot_index: u32::MAX,
    })
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
    pub fn neutral() -> Self {
        Self(vec![
            IntroQuadDesc {
                size_q12: 0x1000,
                w: 1,
                h: 1,
                top: [0xFF; 3],
                bottom: [0xFF; 3],
                ..Default::default()
            };
            INTRO_QUAD_TABLE_LEN
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
    /// until [`BattleIntro::capture_field_frame`] runs. Owned here rather than
    /// written back into the host's page because the capture is transient: it
    /// exists for the transition and the field base must survive it unedited.
    captured: Option<Vram>,
    /// The field-character texture pack (PROT 0874 §2), whose **entry 0** is
    /// the tile shatter's shade page: a 256x256 4bpp TIM at `(448, 0)` with
    /// its 16-CLUT block landing as a 256x1 strip on row 473. Retail keeps it
    /// resident for the whole field session; the engine's field VRAM
    /// deliberately does not (its extra pages would clobber rects the town
    /// meshes sample), so the capture re-lands it in the **cloned** page the
    /// transition draws against. `None` = no disc access; the shade faces
    /// then sample whatever the base page holds.
    shade_pack: Option<legaia_asset::field_char_textures::FieldCharTextures>,
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
            shade_pack: None,
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

    /// The captured page, once [`BattleIntro::capture_field_frame`] has run.
    pub fn captured_vram(&self) -> Option<&Vram> {
        self.captured.as_ref()
    }

    /// Land a drawn frame in the two VRAM rects the transition styles sample.
    ///
    /// Both rects get the same image: the row pass reads
    /// [`FIELD_CAPTURE_ROWS`] and the column pass [`FIELD_CAPTURE_COLS`], and
    /// the tile and swirl styles' own pages sit inside the second rect. The
    /// mask bit is set so a black field pixel samples as opaque black rather
    /// than as a transparent hole - a full-screen transition wants the whole
    /// frame, including its dark areas.
    ///
    /// `base` is the scene's own VRAM, which is **cloned** rather than edited:
    /// the capture is transient and the host's pristine page has to survive
    /// the transition. The returned page is the one to upload and draw the
    /// strips against.
    ///
    /// Cost: one offscreen frame plus a full readback and a resample, once per
    /// transition.
    pub fn capture_field_frame(
        &mut self,
        renderer: &crate::Renderer,
        target: crate::RenderTarget<'_>,
        base: &Vram,
    ) -> anyhow::Result<&Vram> {
        let img = renderer.capture_rgba(target)?;
        let style = self.style;
        let shade = if style == IntroStyle::TileShatter {
            self.shade_pack.as_ref()
        } else {
            None
        };
        let page = self.captured.insert(base.clone());
        let opts = CaptureOpts { set_mask_bit: true };
        // Only the rects this style samples - see `capture_rects_for`. The
        // rows rect covers the tile shatter's own shade page, so writing it
        // unconditionally destroys an input the style needs.
        for rect in capture_rects_for(style) {
            crate::vram_capture::blit_rgba_into_vram(
                &img.rgba, img.width, img.height, page, *rect, opts,
            );
        }
        // The shade page + its row-473 CLUT strip (see `shade_pack`). Entry 0
        // only: the pack's character-atlas entries are already in the base
        // page via the host's field upload, and its other shared pages land
        // on rects the scene meshes sample.
        if let Some(pack) = shade {
            let mut entry0 = pack.clone();
            entry0.textures.retain(|t| t.index == 0);
            entry0.upload_to_vram(page, false);
        }
        Ok(page)
    }

    /// Advance one frame and emit.
    ///
    /// `elapsed` is the live transition entity's `+0x1A`; the emitter adopts it
    /// rather than counting for itself, so a host that stalls or repeats a
    /// simulation tick cannot desynchronise the visuals from the handoff.
    /// `frame_step` is retail's per-frame display-frame delta (`1` at the
    /// steady NTSC cadence).
    pub fn tick(&mut self, elapsed: i16, frame_step: u8) -> IntroFrame {
        self.clock = elapsed;
        let mut out = IntroFrame::default();

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
                out.prims.reserve(tick.quads.len());
                for q in &tick.quads {
                    out.prims
                        .push(ScreenPrim::Textured(intro_quad_to_screen(&q.quad)));
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
/// The **ramp** is retail's, ported in
/// [`legaia_engine_vm::battle_intro_styles::intro_fade`]. The **quad** is the
/// port's: retail's emitter writes a six-word GP0 packet whose corners come
/// from the scratchpad display-rect words, which is the whole PSX display, so
/// the port draws the whole display rect. It is emitted semi-transparent
/// because a fade that replaced the frame would hide the transition it is
/// fading, not blend into it.
pub fn fade_quad(f: &IntroFade) -> ScreenPrim {
    let (w, h) = (PSX_SCREEN_WIDTH as i16, PSX_SCREEN_HEIGHT as i16);
    ScreenPrim::Flat(FlatQuad {
        xy: [(0, 0), (w, 0), (0, h), (w, h)],
        color: [
            ((f.rgb >> 16) & 0xFF) as u8,
            ((f.rgb >> 8) & 0xFF) as u8,
            (f.rgb & 0xFF) as u8,
            0xFF,
        ],
        semi_transparent: true,
        abr_mode: 0,
        ot_index: u32::from(f.depth),
    })
}

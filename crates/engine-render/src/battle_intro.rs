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
//! # Style coverage is not uniform, and the difference is structural
//!
//! `FUN_801CF5BC`'s first tail switch dispatches five styles
//! ([`IntroStyle`]). They do **not** all reach a primitive here, and the split
//! is not effort - it is which retail packet builder is ported:
//!
//! | Style | Working set ticks | Emits primitives |
//! |---|---|---|
//! | [`IntroStyle::Curtain`] | yes | **yes** - complete |
//! | [`IntroStyle::TileShatter`] | yes | **yes** - complete |
//! | [`IntroStyle::ScatterParticles`] | yes | no - see below |
//! | [`IntroStyle::SpinUpParticles`] | yes | no |
//! | [`IntroStyle::Swirl`] | yes | no |
//!
//! The curtain is complete because it is the one style whose packet builder is
//! itself ported: `FUN_801CF1B0`
//! ([`legaia_engine_vm::battle_intro_transition::build_intro_quad`]) produces
//! **screen-space** corners with texture page, CLUT, UVs and a top/bottom
//! colour pair, so there is no projection step to invent. Its descriptor table
//! is disc data that parses ([`IntroQuadTable`]), and the texture pages its two
//! passes name decode to exactly the rects [`crate::vram_capture`] captures
//! into.
//!
//! The tile shatter - the style the **ordinary random encounter** takes - is
//! complete because every input its emitter needs is now pinned: the packet
//! is [`legaia_engine_vm::battle_intro_tiles::tile_face_quads`], the corner
//! table decodes off PROT 0979 ([`parse_tile_corner_table`]), the projection
//! chain is the FT4 handler's ([`emit_tile`]'s doc has the accept chain), and
//! the 4bpp shade page its side faces sample turned out to be **disc data
//! already parsed by the engine** - `legaia_asset::field_char_textures` entry
//! 0 (PROT 0874 §2), which [`BattleIntro::capture_field_frame`] lands in the
//! transition's cloned page. One retail nuance is *not* carried: the
//! dispatcher runs a moving tile's opaque faces through the depth-cue alpha
//! bank (fade toward a zeroed far colour), which the screen overlay has no
//! channel for - receding tiles keep their face grey instead of also dimming
//! with distance.
//!
//! The other three end in a GTE/GPU packet emitter that is documented but not
//! ported (`docs/subsystems/cutscene.md` § "Per-style emitters"): the particle
//! styles project sprite quads through the sprite projector, and the swirl
//! submits 32 primitives per band half from a 198-vertex fan. The trig tables
//! `_DAT_8007B7F8` / `_DAT_8007B81C` are no longer a blocker (the tile draw
//! reproduces them via [`crate::billboard::psx_sin`]); the swirl's fan is
//! triangles, which [`ScreenPrim`] has no variant for at all.
//!
//! Ticking their working sets anyway is deliberate and is not an inert
//! allocation: the fade ramp and the transition's own completion arm both read
//! the same clock, so a battle opened on any of the three still fades and
//! still hands off on the retail frame. What it does not do is draw the style,
//! and [`IntroFrame::style_drawn`] reports that per frame rather than leaving
//! a caller to infer it from an empty list.
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
    self as styles, IntroFade, IntroStyle, PARTICLE_TICK_A, PARTICLE_TICK_B,
};
use legaia_engine_vm::battle_intro_swirl::{self as swirl, SwirlMesh};
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
/// | the rest | not established | both |
///
/// The tile row is the one that has to differ. Its own pages are wholly
/// inside the column rect, and the shade page it also needs is inside the row
/// rect - so writing the rows would destroy an input it depends on while
/// gaining it nothing. The styles whose sampling is not established keep both
/// rects: that is the conservative choice, and it costs nothing today because
/// none of them reaches a primitive.
pub fn capture_rects_for(style: IntroStyle) -> &'static [VramRect] {
    const BOTH: [VramRect; 2] = [FIELD_CAPTURE_ROWS, FIELD_CAPTURE_COLS];
    const COLS_ONLY: [VramRect; 1] = [FIELD_CAPTURE_COLS];
    match style {
        IntroStyle::TileShatter => &COLS_ONLY,
        _ => &BOTH,
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

/// `RTPS` for one tile corner: rotate by the tile's own matrix, translate by
/// the record position (the per-tile MVMVA result - the transition's view
/// matrix is identity rotation with zero translation from the second frame
/// on, pinned live), perspective-divide through the UNR reciprocal at
/// [`INTRO_H`], and offset by the transition's screen centre.
fn project_tile_corner(rot: &GteMat3, tr: GteVec3, c: (i16, i16, i16)) -> ProjCorner {
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
            std::array::from_fn(|i| project_tile_corner(&rot, tr, q.corners[i]));
        let n1 = nclip(p[0].xy, p[1].xy, p[2].xy);
        let n2 = nclip(p[1].xy, p[2].xy, p[3].xy);
        if n1 <= 0 && n2 >= 0 {
            continue;
        }
        let otz = avsz4_with_scale(p[0].sz, p[1].sz, p[2].sz, p[3].sz, INTRO_ZSF4);
        if otz < INTRO_NEAR_OTZ {
            continue;
        }
        let grey = u32::from(q.grey);
        prims.push(ScreenPrim::Textured(ScreenQuad {
            xy: std::array::from_fn(|i| (p[i].xy.x as i16, p[i].xy.y as i16)),
            uv: q.uv,
            clut: q.clut,
            tpage: q.tpage,
            color: grey << 16 | grey << 8 | grey,
            gouraud: None,
            semi_transparent: q.semi_transparent,
            ot_index: otz as u32,
        }));
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
    /// curtain does not.
    pub fn new(
        style: IntroStyle,
        sub_style: i32,
        total_duration: i32,
        table: IntroQuadTable,
        env: &mut dyn legaia_engine_vm::battle_intro_particles::ParticleEnv,
        trig: &mut dyn swirl::SwirlTrig,
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
            IntroStyle::Swirl => match swirl::build_swirl_mesh(true, trig) {
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
                styles::tick_particle_field(grid, style, &mut self.clock, frame_step);
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
                swirl::tick_swirl(mesh, &mut self.clock, frame_step, prev_clock);
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

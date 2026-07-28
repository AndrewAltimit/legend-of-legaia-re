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
//! | [`IntroStyle::ScatterParticles`] | yes | no - see below |
//! | [`IntroStyle::SpinUpParticles`] | yes | no |
//! | [`IntroStyle::TileShatter`] | yes | no |
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
//! The other four end in a GTE/GPU packet emitter that is documented but not
//! ported (`docs/subsystems/cutscene.md` § "Per-style emitters"): the particle
//! styles project sprite quads through the sprite projector, the tiles project
//! eight-corner boxes, and the swirl submits 32 primitives per band half from a
//! 198-vertex fan. Two further inputs are missing for three of them - the
//! `0x801CE8BC` corner table (tiles) and the trig tables `_DAT_8007B7F8` /
//! `_DAT_8007B81C` (tiles, swirl, both particle styles) - and the swirl's fan
//! is triangles, which [`ScreenPrim`] has no variant for at all.
//!
//! Ticking their working sets anyway is deliberate and is not an inert
//! allocation: the fade ramp and the transition's own completion arm both read
//! the same clock, so a battle opened on any of the four still fades and still
//! hands off on the retail frame. What it does not do is draw the style, and
//! [`IntroFrame::style_drawn`] reports that per frame rather than leaving a
//! caller to infer it from an empty list.
//!
//! # The capture is a two-rect affair, and both rects are used
//!
//! The curtain's row pass samples texture pages `0x105` / `0x108` and its
//! column pass `0x115` / `0x118`. Those decode to 15-bpp pages at VRAM
//! `(320, 0)` / `(512, 0)` and `(320, 256)` / `(512, 256)`, and each pass
//! covers columns `320..=639` across its pair - so the row pass reads the
//! capture at [`FIELD_CAPTURE_ROWS`] and the column pass an identical copy at
//! [`FIELD_CAPTURE_COLS`]. [`BattleIntro::capture_field_frame`] writes both.

use crate::screen_overlay::{FlatQuad, ScreenPrim, ScreenQuad};
use crate::vram_capture::{
    CaptureOpts, FIELD_CAPTURE_COLS, FIELD_CAPTURE_ROWS, PSX_SCREEN_HEIGHT, PSX_SCREEN_WIDTH,
};
use legaia_engine_vm::battle_intro_styles::{
    self as styles, IntroFade, IntroStyle, PARTICLE_TICK_A, PARTICLE_TICK_B,
};
use legaia_engine_vm::battle_intro_swirl::{self as swirl, SwirlMesh};
use legaia_engine_vm::battle_intro_tiles::{self as tiles, TileGrid};
use legaia_engine_vm::battle_intro_transition::{INTRO_QUAD_DESC_STRIDE, IntroQuad, IntroQuadDesc};
use legaia_tim::Vram;

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
        }
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
        let page = self.captured.insert(base.clone());
        let opts = CaptureOpts { set_mask_bit: true };
        for rect in [FIELD_CAPTURE_ROWS, FIELD_CAPTURE_COLS] {
            crate::vram_capture::blit_rgba_into_vram(
                &img.rgba, img.width, img.height, page, rect, opts,
            );
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
                tiles::tick_tile_grid(grid, &mut self.clock, frame_step);
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

//! The [`BattleIntro`] session: construction, arming, capture landing and accessors.

use super::*;
use crate::battle_intro_styles as styles;
use crate::battle_intro_styles::{IntroFade, IntroStyle, PARTICLE_TICK_A, PARTICLE_TICK_B};
use crate::battle_intro_swirl as swirl;
use crate::battle_intro_swirl::SwirlMesh;
use crate::battle_intro_tiles as tiles;
use crate::battle_intro_tiles::TileGrid;
use crate::battle_intro_transition::IntroQuad;
use crate::screen_prim::ScreenPrim;
use crate::vram_capture::{CaptureOpts, FIELD_CAPTURE_CLEARS};
use legaia_tim::Vram;

/// The per-style working set, one variant per [`IntroStyle`].
#[derive(Debug, Clone)]
pub(super) enum WorkingSet {
    Particles {
        grid: Vec<crate::battle_intro_particles::IntroParticle>,
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
    pub(super) style: IntroStyle,
    pub(super) sub_style: i32,
    pub(super) set: WorkingSet,
    pub(super) clock: i16,
    pub(super) total_duration: i32,
    /// The VRAM page with the captured field frame blitted into it, `None`
    /// until [`BattleIntro::land_capture_rgba`] runs. Owned here rather than
    /// written back into the host's page because the capture is transient: it
    /// exists for the transition and the field base must survive it unedited.
    pub(super) captured: Option<Vram>,
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
    shade_pack: Option<legaia_actor_formats::field_char_textures::FieldCharTextures>,
    /// The captured page lifted out of
    /// [`crate::vram_capture::FIELD_CAPTURE_PAGE`] as a plain `320 x 256`
    /// halfword image (the frame sits four rows down in it), so the curtain's column pass can sample it
    /// while the same page is being written. Empty for every other style.
    pub(super) capture_src: Vec<u16>,
    /// This frame's **column**-pass quads, held between [`BattleIntro::tick`]
    /// (which builds them) and [`BattleIntro::refresh_captured_page`] (which
    /// rasterises them into the intermediate). Only the curtain fills it - see
    /// the module docs on why these never reach the screen.
    pub(super) pending_columns: Vec<IntroQuad>,
    /// This frame's **row**-pass quads, kept alongside their on-screen
    /// emission so [`BattleIntro::refresh_captured_page`] can also land them in
    /// the CPU display model the trail decays. Only the curtain fills it.
    pub(super) pending_rows: Vec<IntroQuad>,
    /// The curtain's intermediate ([`FIELD_CAPTURE_ROWS`] content) as a
    /// persistent buffer: retail never clears it - `FUN_801D1D9C` decays it by
    /// [`CURTAIN_MIDPASS_DECAY_5`] each frame before the column strips draw.
    /// Empty for every other style.
    pub(super) intermediate: Vec<u16>,
    /// The CPU model of the transition's display buffer: seeded from the field
    /// capture (retail's init lands the drawn frame in both display buffers),
    /// decayed by [`CURTAIN_DISPLAY_DECAY_5`] each frame (the re-armed
    /// `0x80808` wash), and overdrawn with each frame's row strips. Uploaded
    /// into [`CURTAIN_TRAIL_RECT`] *before* the rows land, so the on-screen
    /// trail quads show exactly what retail's undrawn gaps show. Empty for
    /// every other style.
    pub(super) display_accum: Vec<u16>,
    /// The clock [`BattleIntro::advance_to`] last stepped to and the frame it
    /// emitted there.
    pub(super) last_frame: Option<(i16, IntroFrame)>,
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
        env: &mut dyn crate::battle_intro_particles::ParticleEnv,
        _trig: &mut dyn swirl::SwirlTrig,
        tile_corners: [i32; 4],
    ) -> Self {
        use crate::battle_intro_particles as particles;
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
            last_frame: None,
        }
    }

    /// Attach the field-character texture pack whose entry 0 is the shade
    /// page (see the `shade_pack` field). Only the tile shatter consults it.
    pub fn with_shade_pack(
        mut self,
        pack: Option<legaia_actor_formats::field_char_textures::FieldCharTextures>,
    ) -> Self {
        self.shade_pack = pack;
        self
    }

    /// Arm the emitter for the battle about to open - the one arming both
    /// play hosts run. The style is not a host choice: `select_intro_style`
    /// ports the intro overlay's own init block over `inputs` (the host
    /// resolves them through `SceneHost::battle_intro_style_inputs`).
    ///
    /// `overlay_0979` is the raw PROT 0979 entry: relocated to its load base
    /// ([`intro_overlay_as_loaded`]) it carries the curtain's descriptor
    /// table and the tile seeder's corner table, each with its disc-free
    /// fallback ([`IntroQuadTable::neutral`], [`DEFAULT_TILE_CORNERS`]).
    /// `field_char_textures` is the raw field-character texture pack, whose
    /// entry 0 is the shade page the shatter's side faces sample. The
    /// particle env and the (unused) trig seam both seed from `rng_seed`, the
    /// world's RNG word.
    pub fn arm_for_battle(
        inputs: &crate::battle_intro_styles::IntroStyleInputs,
        total_duration: i32,
        overlay_0979: Option<&[u8]>,
        field_char_textures: Option<&[u8]>,
        rng_seed: u32,
    ) -> Self {
        use crate::battle_intro_particles::IntroEnv;
        let choice = crate::battle_intro_styles::select_intro_style(inputs);
        let overlay = overlay_0979.and_then(intro_overlay_as_loaded);
        let table = overlay
            .as_ref()
            .and_then(|(img, base)| IntroQuadTable::parse_overlay(img, *base))
            .unwrap_or_else(IntroQuadTable::neutral);
        let corners = overlay
            .as_ref()
            .and_then(|(img, base)| parse_tile_corner_table(img, *base))
            .unwrap_or(DEFAULT_TILE_CORNERS);
        let shade = field_char_textures
            .and_then(|b| legaia_actor_formats::field_char_textures::parse(b).ok());
        let mut env = IntroEnv::new(rng_seed);
        let mut trig = IntroEnv::new(rng_seed);
        Self::new(
            choice.style,
            choice.sub_style,
            total_duration,
            table,
            &mut env,
            &mut trig,
            corners,
        )
        .with_shade_pack(shade)
    }

    /// `DAT_801D2464`, the sub-style the emitter was armed with.
    pub fn sub_style(&self) -> i32 {
        self.sub_style
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
    /// [`capture_rects_for`] names - [`crate::vram_capture::FIELD_CAPTURE_LAND`]
    /// inside the column page for every style, since no style samples the
    /// rows rect (see the module docs) - after clearing the page around it as
    /// retail's init does ([`FIELD_CAPTURE_CLEARS`]). The mask
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
        // Retail's init clears the page around its capture to `0x0000`
        // (transparent): the rows above the frame, the rows below it and the
        // strip past the right edge (`FIELD_CAPTURE_CLEARS`). A texel
        // coordinate that reaches past the frame then reads as a hole, not as
        // whatever the scene left in VRAM there.
        for r in FIELD_CAPTURE_CLEARS {
            let zeros = vec![0u8; usize::from(r.w) * usize::from(r.h) * 2];
            page.write_block(r.x, r.y, r.w, r.h, &zeros);
        }
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
}

#[cfg(test)]
mod capture_landing_tests {
    use super::*;
    use crate::battle_intro_particles::IntroEnv;

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
        let land = crate::vram_capture::FIELD_CAPTURE_LAND;
        let (x, y) = (land.x as usize, land.y as usize);

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

    /// The page around the frame is retail's: drawn row `d` at
    /// `v = d + 4`, and the rows above and below it, plus the strip past the
    /// right edge, cleared to transparent `0x0000` whatever the scene's VRAM
    /// held there - the layout a live retail shatter state's VRAM shows
    /// (`v 0..=3` and `v >= 228` zero, `v 4..=227` all mask-set).
    #[test]
    fn the_capture_lands_four_rows_down_and_clears_around_it() {
        use crate::vram_capture::{FIELD_CAPTURE_PAGE, FIELD_CAPTURE_V_OFFSET};
        // A white 320x240 frame over a base page full of non-zero junk.
        let rgba = vec![0xFFu8; 320 * 240 * 4];
        let mut base = Vram::new();
        let junk = vec![0x55u8; 1024 * 512 * 2];
        base.write_block(0, 0, 1024, 512, &junk);
        let mut intro = bare_intro(IntroStyle::TileShatter);
        intro.land_capture_rgba(&rgba, 320, 240, &base);
        let page = intro.captured_vram().expect("capture landed");
        let (px, py) = (FIELD_CAPTURE_PAGE.x as usize, FIELD_CAPTURE_PAGE.y as usize);
        let off = usize::from(FIELD_CAPTURE_V_OFFSET);
        for x in [0usize, 160, 319] {
            for v in 0..off {
                assert_eq!(page.pixel(px + x, py + v), 0, "v {v} above the frame");
            }
            assert_eq!(page.pixel(px + x, py + off), 0xFFFF, "drawn row 0 at v 4");
            assert_eq!(
                page.pixel(px + x, py + off + 239),
                0xFFFF,
                "drawn row 239 at v 243"
            );
            for v in off + 240..256 {
                assert_eq!(page.pixel(px + x, py + v), 0, "v {v} below the frame");
            }
        }
        for y in [256usize, 400, 511] {
            for x in 640..656 {
                assert_eq!(page.pixel(x, y), 0, "the x = 640 strip");
            }
        }
        // Outside the cleared rects the base page survives.
        assert_eq!(page.pixel(656, 300), 0x5555);
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

#[cfg(test)]
mod arm_for_battle_tests {
    use super::*;
    use crate::battle_intro_styles::{IntroStyleInputs, select_intro_style};

    /// Disc-free, the arming lands on the selector's style with the
    /// documented fallbacks, and the same seed arms the same emitter.
    #[test]
    fn arming_without_the_overlay_takes_the_selector_style_and_fallbacks() {
        let inputs = IntroStyleInputs::default();
        let a = BattleIntro::arm_for_battle(&inputs, 60, None, None, 0x1234);
        let choice = select_intro_style(&inputs);
        assert_eq!(a.style(), choice.style);
        assert_eq!(a.sub_style(), choice.sub_style);
        let b = BattleIntro::arm_for_battle(&inputs, 60, Some(&[]), Some(&[]), 0x1234);
        assert_eq!(b.style(), a.style());
    }

    /// The emitter's picture is a function of the transition clock, not of
    /// how often a host draws: one host stepping every clock unit and one
    /// drawing on an uneven cadence (several world ticks per redraw, a late
    /// first draw, a redraw with no tick between) emit the same frame at the
    /// same clock. Stepping once per call is what put the native window's
    /// shatter two ticks behind the browser page's.
    #[test]
    fn the_frame_depends_on_the_clock_not_the_draw_cadence() {
        for formation_slot0 in [0u8, 0x10, 0x40, 0xA6] {
            let inputs = IntroStyleInputs {
                formation_slot0,
                ..Default::default()
            };
            let mut every = BattleIntro::arm_for_battle(&inputs, 60, None, None, 0x1234);
            let mut uneven = every.clone();
            let mut at = std::collections::BTreeMap::new();
            for e in 0..40i16 {
                at.insert(e, every.advance_to(e));
            }
            for e in [2i16, 2, 3, 7, 7, 8, 11, 20, 39] {
                assert_eq!(
                    uneven.advance_to(e),
                    at[&e],
                    "style {:?}, clock {e}",
                    uneven.style()
                );
            }
        }
    }
}

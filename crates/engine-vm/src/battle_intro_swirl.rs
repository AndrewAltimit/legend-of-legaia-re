//! The field-to-battle transition's **swirl** style: sixteen concentric
//! ring-bands of the captured field screen, counter-rotating against each
//! other.
//!
//! | Retail | Here | Job |
//! |---|---|---|
//! | `FUN_801D1564` | [`build_swirl_mesh`] | allocate + build the band table, the vertex fan and its texels |
//! | `FUN_801D1888` | [`tick_swirl`] | one frame: rotate every band, decide which still draw |
//! | `FUN_801D1A20` | [`swirl_band_draw`] | one band-half: which vertices and which of two submit paths |
//!
//! ## The three allocation sizes fix the mesh
//!
//! `FUN_801D1564` asks `FUN_80017888` for `0x100`, `0x6300` and `0x18C0`
//! bytes, and the three agree on one shape:
//!
//! * `0x100` = [`BANDS`] records of [`BAND_STRIDE`] bytes - the band table.
//! * `0x6300` = `16 * 198` vertices of 8 bytes - [`BANDS`] bands of
//!   [`VERTS_PER_BAND`], where `198 == 2 * 99` and `99 == 33 * 3` is one
//!   half-fan of [`COLUMNS`] samples at three vertices apiece.
//! * `0x18C0` = the same `16 * 198` vertices at **two** bytes each - one `(u, v)`
//!   texel pair per vertex.
//!
//! ## Why it is a half-turn plus a mirror
//!
//! Each column samples the two trig tables `_DAT_8007B7F8` / `_DAT_8007B81C`
//! at **stride `0x80`** (`sll v1,t7,0x7`), not at the `* 2` stride the particle
//! seeders use. Over a 4096-entry 12-bit-angle table that is one entry every
//! 64 units, so [`COLUMNS`] samples span `32 * 64 == 2048` units - exactly half
//! a turn, both ends inclusive. The other half is not sampled: it is written as
//! the [`SwirlHalf::Mirrored`] copy with x negated, which is why the vertex
//! count per band is `2 * 99` rather than `65 * 3`.
//!
//! ## Two radii per band, then a rectangular clamp
//!
//! Band `b` carries an inner radius `4 + b * 0x10` and an outer radius
//! `0x14 + b * 0x10`. Each trig read is multiplied by its radius and shifted
//! right by 8, then **clamped** to [`CLAMP_X`] on x and [`CLAMP_Y`] on y. The
//! outer bands overrun both bounds, so the fan stops being a circle and
//! becomes the screen rectangle - which is what lets the same mesh carry the
//! whole captured frame.
//!
//! ## One texel pair is deliberately off its vertex
//!
//! A column writes three vertices - `(x0, y0, 0)`, `(x1, y1, 0)` and
//! `(x0, y0, 0x1000)` - but only two distinct texel pairs: the third vertex
//! gets the **second** pair (`801d17c4` / `801d17d8` store `u1` / `v1` into
//! both slots 1 and 2). Vertex 2 therefore samples the outer radius' texel at
//! the inner radius' position. That is retail's, reproduced verbatim.
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_field_battle_intro_801d1564.txt`,
//! `..._801d1888.txt` and `..._801d1a20.txt` - disassembly, not the C.

/// Bands in the swirl (`slti v0,t5,0x10`).
pub const BANDS: usize = 0x10;
/// Byte stride of one band record.
pub const BAND_STRIDE: usize = 0x10;
/// Angle samples per half-fan (`slti v0,t7,0x21`).
pub const COLUMNS: usize = 0x21;
/// Vertices one column contributes.
pub const VERTS_PER_COLUMN: usize = 3;
/// Vertices in one half-fan: `33 * 3`.
pub const VERTS_PER_HALF: usize = COLUMNS * VERTS_PER_COLUMN;
/// Vertices in one whole band - both halves (`addiu s2,s2,0xc6`).
pub const VERTS_PER_BAND: usize = VERTS_PER_HALF * 2;

/// Bytes requested for the band table.
pub const BAND_BLOCK_BYTES: usize = BANDS * BAND_STRIDE;
/// Bytes requested for the vertex fan (8 bytes per vertex).
pub const VERTEX_BLOCK_BYTES: usize = BANDS * VERTS_PER_BAND * 8;
/// Bytes requested for the texel array (2 bytes per vertex).
pub const TEXEL_BLOCK_BYTES: usize = BANDS * VERTS_PER_BAND * 2;

const _: () = assert!(BAND_BLOCK_BYTES == 0x100);
const _: () = assert!(VERTEX_BLOCK_BYTES == 0x6300);
const _: () = assert!(TEXEL_BLOCK_BYTES == 0x18C0);

/// Entry stride into the two trig tables, in table entries
/// (`sll v1,t7,0x7` over a halfword table).
pub const TRIG_SAMPLE_STRIDE: i32 = 0x40;

/// Inner radius of band 0 (`li s0,0x4`), stepping by [`RADIUS_STEP`].
pub const INNER_RADIUS_BASE: i32 = 4;
/// Outer radius of band 0 (`li s1,0x14`), stepping by [`RADIUS_STEP`].
pub const OUTER_RADIUS_BASE: i32 = 0x14;
/// Radius step per band (`addiu s0,s0,0x10` / `addiu s1,s1,0x10`).
pub const RADIUS_STEP: i32 = 0x10;

/// Symmetric x clamp applied after the radius multiply.
pub const CLAMP_X: i32 = 0xA00;
/// Symmetric y clamp applied after the radius multiply.
pub const CLAMP_Y: i32 = 0x760;

/// z of a column's first two vertices.
pub const COLUMN_NEAR_Z: i16 = 0;
/// z of a column's third vertex (`li s6,0x1000`).
pub const COLUMN_FAR_Z: i16 = 0x1000;

/// u bias on the primary half (`addiu v0,t0,0x20`).
pub const U_BIAS: i32 = 0x20;
/// v bias on both halves (`addiu a3,a3,0x76`).
pub const V_BIAS: i32 = 0x76;
/// The mirrored half's u is `MIRROR_U_BIAS - (x >> 4)` (`li s5,-0x61`).
pub const MIRROR_U_BIAS: i32 = -0x61;

/// A band still draws while its animated scalar is above this
/// (`slti v0,v0,0x81` at `801d1974`).
pub const BAND_DRAW_THRESHOLD: i32 = 0x80;
/// Only the first this-many bands are ever drawn (`slti v0,s0,0xc`).
pub const BANDS_DRAWN: usize = 0xC;

/// Frame at which both the tick's screen-wash call and the band submit swap
/// to their second form (`slti v0,a3,0x5a` in `FUN_801D1A20`,
/// `slti v0,v0,0x5b` in `FUN_801D1888` - the two bounds differ by one, and
/// they are read one frame apart).
pub const LATE_PHASE_FRAME: i32 = 0x5A;

/// The packed RGB the tick washes the screen with once the clock has passed
/// [`LATE_PHASE_FRAME`] (`func_0x8004695C(0x101010)`).
pub const LATE_WASH_RGB: u32 = 0x0010_1010;

/// The tint the early submit path passes (`lui 0x1880; ori 0x8080`).
pub const EARLY_SUBMIT_TINT: u32 = 0x1880_8080;
/// The tint the late submit path passes (`lui 0x8180; ori 0x8080`).
pub const LATE_SUBMIT_TINT: u32 = 0x8180_8080;
/// Frame offset the late submit's fourth argument is measured from
/// (`addiu a3,a3,-0x3c`, then `<< 2`).
pub const LATE_SUBMIT_EPOCH: i32 = 0x3C;

/// Texture-page word for the primary half (`li v0,0x117`).
pub const TPAGE_PRIMARY: i32 = 0x117;
/// Texture-page word for the mirrored half (`li v0,0x115`).
pub const TPAGE_MIRRORED: i32 = 0x115;

/// One `0x10`-byte band record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SwirlBand {
    /// `+0x00` - `band << 4`.
    pub phase: i32,
    /// `+0x04` - `(band + 1) * 0x10`.
    pub width: i32,
    /// `+0x08` - the animated scalar. Seeded to [`CLAMP_X`], fed to the frame's
    /// rotation vector as its z component, and integrated by `+0x0C`.
    pub angle: i32,
    /// `+0x0C` - the per-frame rate. Alternating bands get opposite signs,
    /// which is what makes the rings counter-rotate:
    /// `((band & 1 == 0) ? -(6 - band) : (6 - band)) * 0x1400 + 0xA00`.
    pub rate: i32,
}

impl SwirlBand {
    /// The seeded record for band `index`, `FUN_801D1564`'s first loop.
    pub fn seed(index: usize) -> Self {
        let b = index as i32;
        // The negate is guarded by `bne (b & 1), 0` - so it lands on the
        // *even* bands, not on the odd ones.
        let signed = if b % 2 == 0 { -(6 - b) } else { 6 - b };
        Self {
            phase: b << 4,
            width: (b + 1) * 0x10,
            angle: CLAMP_X,
            rate: signed * 0x1400 + 0xA00,
        }
    }
}

/// One 8-byte fan vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SwirlVertex {
    /// `+0x00`.
    pub x: i16,
    /// `+0x02`.
    pub y: i16,
    /// `+0x04` - [`COLUMN_NEAR_Z`] or [`COLUMN_FAR_Z`].
    pub z: i16,
}

/// The whole style-4 working set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwirlMesh {
    /// `DAT_801D247C` - the band table.
    pub bands: [SwirlBand; BANDS],
    /// `DAT_801D2474` - `16 * 198` vertices, band-major, primary half then
    /// mirrored half.
    pub vertices: Vec<SwirlVertex>,
    /// `DAT_801D2478` - one `(u, v)` byte pair per vertex, same order.
    pub texels: Vec<(i8, i8)>,
}

/// What [`build_swirl_mesh`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwirlBuildOutcome {
    /// One of the three allocations came back null; the caller adds `10` to
    /// `_DAT_8007B828`. Retail attempts them in order and stops at the first
    /// failure, so a partial working set is never handed back.
    OutOfMemory,
    /// The mesh.
    Built(Box<SwirlMesh>),
}

/// Which half of a band a vertex range covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwirlHalf {
    /// `param_1 == 0` - the sampled half. Vertices `band * 198 ..`, texture
    /// page [`TPAGE_PRIMARY`].
    Primary,
    /// `param_1 != 0` - the x-negated copy. Vertices `band * 198 + 99 ..`,
    /// texture page [`TPAGE_MIRRORED`].
    Mirrored,
}

/// The trig tables the mesh builder samples - the same pair
/// [`crate::battle_intro_particles::ParticleEnv`] exposes, addressed by a raw
/// table index rather than by a heading.
pub trait SwirlTrig {
    /// `_DAT_8007B81C[entry]` - the table the **x** component comes from.
    fn table_x(&mut self, entry: i32) -> i16;
    /// `_DAT_8007B7F8[entry]` - the table the **y** component comes from.
    fn table_y(&mut self, entry: i32) -> i16;
}

fn clamp(v: i32, bound: i32) -> i32 {
    v.clamp(-bound, bound)
}

/// Build the swirl mesh. `FUN_801D1564`.
///
/// `allocated` is the answer to all three `FUN_80017888` calls; retail stops at
/// the first null and leaves the entity's `+0x48` chain incomplete.
///
/// PORT: FUN_801D1564
///
/// NOT WIRED: nothing owns a [`SwirlMesh`]. `legaia_engine_core::World` tracks
/// the transition as its phase counter only (`World::battle_intro`), and
/// `legaia-engine-render` has no pass that draws a textured fan over a captured
/// framebuffer - the engine has no captured framebuffer to texture with, since
/// it presents through a swapchain rather than through a re-readable VRAM.
/// Wiring needs a screen-capture render target first.
pub fn build_swirl_mesh(allocated: bool, trig: &mut dyn SwirlTrig) -> SwirlBuildOutcome {
    if !allocated {
        return SwirlBuildOutcome::OutOfMemory;
    }

    let mut bands = [SwirlBand::default(); BANDS];
    for (i, b) in bands.iter_mut().enumerate() {
        *b = SwirlBand::seed(i);
    }

    let mut vertices = vec![SwirlVertex::default(); BANDS * VERTS_PER_BAND];
    let mut texels = vec![(0i8, 0i8); BANDS * VERTS_PER_BAND];

    for band in 0..BANDS {
        let inner = INNER_RADIUS_BASE + band as i32 * RADIUS_STEP;
        let outer = OUTER_RADIUS_BASE + band as i32 * RADIUS_STEP;
        let base = band * VERTS_PER_BAND;
        for col in 0..COLUMNS {
            let entry = col as i32 * TRIG_SAMPLE_STRIDE;
            let tx = i32::from(trig.table_x(entry));
            let ty = i32::from(trig.table_y(entry));

            // Inner radius on both axes, then outer radius on both axes.
            let x0 = clamp((tx * inner) >> 8, CLAMP_X);
            let y0 = clamp((ty * inner) >> 8, CLAMP_Y);
            let x1 = clamp((tx * outer) >> 8, CLAMP_X);
            let y1 = clamp((ty * outer) >> 8, CLAMP_Y);

            let u0 = ((x0 >> 4) + U_BIAS) as i8;
            let v0 = ((y0 >> 4) + V_BIAS) as i8;
            let u1 = ((x1 >> 4) + U_BIAS) as i8;
            let v1 = ((y1 >> 4) + V_BIAS) as i8;

            let p = base + col * VERTS_PER_COLUMN;
            vertices[p] = SwirlVertex {
                x: x0 as i16,
                y: y0 as i16,
                z: COLUMN_NEAR_Z,
            };
            vertices[p + 1] = SwirlVertex {
                x: x1 as i16,
                y: y1 as i16,
                z: COLUMN_NEAR_Z,
            };
            vertices[p + 2] = SwirlVertex {
                x: x0 as i16,
                y: y0 as i16,
                z: COLUMN_FAR_Z,
            };
            texels[p] = (u0, v0);
            texels[p + 1] = (u1, v1);
            // Retail writes the *outer* pair here as well; see the module docs.
            texels[p + 2] = (u1, v1);

            let m = base + VERTS_PER_HALF + col * VERTS_PER_COLUMN;
            vertices[m] = SwirlVertex {
                x: (-x0) as i16,
                y: y0 as i16,
                z: COLUMN_NEAR_Z,
            };
            vertices[m + 1] = SwirlVertex {
                x: (-x1) as i16,
                y: y1 as i16,
                z: COLUMN_NEAR_Z,
            };
            vertices[m + 2] = SwirlVertex {
                x: (-x0) as i16,
                y: y0 as i16,
                z: COLUMN_FAR_Z,
            };
            let mu0 = (MIRROR_U_BIAS - (x0 >> 4)) as i8;
            let mu1 = (MIRROR_U_BIAS - (x1 >> 4)) as i8;
            texels[m] = (mu0, v0);
            texels[m + 1] = (mu1, v1);
            texels[m + 2] = (mu1, v1);
        }
    }

    SwirlBuildOutcome::Built(Box::new(SwirlMesh {
        bands,
        vertices,
        texels,
    }))
}

/// Where one band-half's geometry lives, and which of the two submit paths it
/// takes. `FUN_801D1A20`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwirlBandDraw {
    /// First vertex / texel index of the half.
    pub first_vertex: usize,
    /// Texture page word for the half.
    pub tpage: i32,
    /// Tint word passed to the submit.
    pub tint: u32,
    /// `Some(n)` when the late submit path runs, carrying its fourth argument
    /// `(clock - `[`LATE_SUBMIT_EPOCH`]`) * 4`; `None` on the early path.
    ///
    /// The late path also zeroes seven scratchpad words before submitting
    /// (`0x1F800354..0x1F80035A` and `0x1F8003BC..0x1F8003C4`), which is the
    /// shared GTE light/colour-matrix block - so the late draw runs unlit while
    /// the early draw keeps whatever the scene left there.
    pub late_arg: Option<i32>,
}

/// Resolve one band-half's draw. `FUN_801D1A20(half, band)`.
///
/// `clock` is `DAT_801D2470`, which [`tick_swirl`] has already set to the
/// entity's `+0x1A` for this frame - so the two functions never disagree about
/// which frame they are on.
///
/// PORT: FUN_801D1A20
///
/// NOT WIRED: called only by [`tick_swirl`], which is itself inert - see the
/// tag there. The packet body it builds (32 primitives from the half's texels
/// into `_DAT_8007B85C + 0x5DC00`) is renderer work and stays out of the
/// kernel; what is ported is the addressing and the path choice.
pub fn swirl_band_draw(half: SwirlHalf, band: usize, clock: i32) -> SwirlBandDraw {
    let (offset, tpage) = match half {
        SwirlHalf::Primary => (0, TPAGE_PRIMARY),
        SwirlHalf::Mirrored => (VERTS_PER_HALF, TPAGE_MIRRORED),
    };
    let late = clock >= LATE_PHASE_FRAME;
    SwirlBandDraw {
        first_vertex: band * VERTS_PER_BAND + offset,
        tpage,
        tint: if late {
            LATE_SUBMIT_TINT
        } else {
            EARLY_SUBMIT_TINT
        },
        late_arg: late.then(|| (clock - LATE_SUBMIT_EPOCH) * 4),
    }
}

/// What one [`tick_swirl`] frame decided.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwirlTick {
    /// `_DAT_8007B6CC` - `elapsed != 0` on entry.
    pub not_first_frame: bool,
    /// The tick washed the screen with [`LATE_WASH_RGB`] because the *previous*
    /// frame's clock had already passed [`LATE_PHASE_FRAME`]. The read happens
    /// before `DAT_801D2470` is refreshed, so this lags the band draws by one
    /// frame.
    pub late_wash: bool,
    /// The band-half draws this frame issued, in retail order (primary then
    /// mirrored, band by band).
    pub draws: Vec<SwirlBandDraw>,
}

/// One frame of the swirl. `FUN_801D1888`.
///
/// Every band is rotated; only bands `0..`[`BANDS_DRAWN`] whose angle is still
/// above [`BAND_DRAW_THRESHOLD`] are drawn, and each drawn band issues **two**
/// calls - the primary half then the mirrored half.
///
/// The integration is `angle += (rate * frame_step) >> 8` with retail's
/// toward-zero pre-bias on the negative arm (`addiu v1,v1,0xff`), so bands
/// whose rate is negative wind down and drop below the threshold, while
/// positive-rate bands keep drawing for the whole transition.
///
/// PORT: FUN_801D1888
///
/// NOT WIRED: same missing host as [`build_swirl_mesh`] - nothing owns a
/// [`SwirlMesh`] and there is no captured-framebuffer texture to draw it with.
pub fn tick_swirl(
    mesh: &mut SwirlMesh,
    elapsed: &mut i16,
    frame_step: u8,
    prev_clock: &mut i32,
) -> SwirlTick {
    let mut out = SwirlTick {
        not_first_frame: *elapsed != 0,
        // `slti v0,v0,0x5b` - one higher than the band submit's own bound.
        late_wash: *prev_clock > LATE_PHASE_FRAME,
        draws: Vec::new(),
    };
    // `DAT_801D2470` is refreshed before any band is drawn, so
    // `swirl_band_draw` reads this frame's clock while the wash above read the
    // previous frame's.
    *prev_clock = i32::from(*elapsed);
    let clock = *prev_clock;

    for band in 0..BANDS {
        let rec = &mut mesh.bands[band];
        if rec.angle > BAND_DRAW_THRESHOLD && band < BANDS_DRAWN {
            out.draws
                .push(swirl_band_draw(SwirlHalf::Primary, band, clock));
            out.draws
                .push(swirl_band_draw(SwirlHalf::Mirrored, band, clock));
        }
        let d = rec.rate.wrapping_mul(i32::from(frame_step));
        let d = if d < 0 { d + 0xFF } else { d } >> 8;
        rec.angle = rec.angle.wrapping_add(d);
    }

    *elapsed = (*elapsed as u16).wrapping_add(u16::from(frame_step)) as i16;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in whose two tables are distinguishable and full-scale, so the
    /// clamps and the radius scaling are both observable.
    struct Tables;
    impl SwirlTrig for Tables {
        fn table_x(&mut self, entry: i32) -> i16 {
            // A ramp from +0x1000 down to -0x1000 across the half turn.
            (0x1000 - entry) as i16
        }
        fn table_y(&mut self, _entry: i32) -> i16 {
            0x1000
        }
    }

    fn built_swirl_mesh() -> SwirlMesh {
        let SwirlBuildOutcome::Built(m) = build_swirl_mesh(true, &mut Tables) else {
            panic!("expected a mesh");
        };
        *m
    }

    #[test]
    fn the_three_allocations_agree_on_one_shape() {
        assert_eq!(BAND_BLOCK_BYTES, 0x100);
        assert_eq!(VERTEX_BLOCK_BYTES, 0x6300);
        assert_eq!(TEXEL_BLOCK_BYTES, 0x18C0);
        assert_eq!(VERTS_PER_BAND, 198);
        assert_eq!(VERTS_PER_HALF, 99);
    }

    #[test]
    fn the_columns_span_exactly_half_a_turn() {
        assert_eq!((COLUMNS as i32 - 1) * TRIG_SAMPLE_STRIDE, 2048);
    }

    #[test]
    fn allocation_failure_builds_nothing() {
        assert_eq!(
            build_swirl_mesh(false, &mut Tables),
            SwirlBuildOutcome::OutOfMemory
        );
    }

    #[test]
    fn alternating_bands_counter_rotate() {
        let even = SwirlBand::seed(0);
        let odd = SwirlBand::seed(1);
        assert_eq!(even.rate, -(6) * 0x1400 + 0xA00);
        assert_eq!(odd.rate, 5 * 0x1400 + 0xA00);
        assert!(even.rate < 0 && odd.rate > 0);
        assert_eq!(even.angle, CLAMP_X);
        assert_eq!((even.phase, even.width), (0, 0x10));
        assert_eq!((odd.phase, odd.width), (0x10, 0x20));
    }

    #[test]
    fn the_outer_bands_clamp_to_the_screen_rectangle() {
        let m = built_swirl_mesh();
        for v in &m.vertices {
            assert!(i32::from(v.x).abs() <= CLAMP_X);
            assert!(i32::from(v.y).abs() <= CLAMP_Y);
        }
        // Band 15's outer radius overruns both bounds, so its first column's
        // second vertex sits exactly on the corner.
        let p = 15 * VERTS_PER_BAND + 1;
        assert_eq!(m.vertices[p].x as i32, CLAMP_X);
        assert_eq!(m.vertices[p].y as i32, CLAMP_Y);
    }

    #[test]
    fn the_mirrored_half_negates_x_and_keeps_y() {
        let m = built_swirl_mesh();
        for col in 0..COLUMNS {
            let p = 3 * VERTS_PER_BAND + col * VERTS_PER_COLUMN;
            let q = p + VERTS_PER_HALF;
            for k in 0..VERTS_PER_COLUMN {
                assert_eq!(m.vertices[q + k].x, -m.vertices[p + k].x);
                assert_eq!(m.vertices[q + k].y, m.vertices[p + k].y);
                assert_eq!(m.vertices[q + k].z, m.vertices[p + k].z);
            }
        }
    }

    #[test]
    fn the_third_vertex_of_a_column_carries_the_second_texel() {
        let m = built_swirl_mesh();
        let p = 2 * VERTS_PER_BAND + 5 * VERTS_PER_COLUMN;
        assert_eq!(m.texels[p + 2], m.texels[p + 1]);
        assert_ne!(
            m.texels[p],
            m.texels[p + 1],
            "the two radii differ, so the pairs do too"
        );
        // ...while the third vertex's *position* is the first vertex's.
        assert_eq!(m.vertices[p + 2].x, m.vertices[p].x);
        assert_eq!(m.vertices[p + 2].z, COLUMN_FAR_Z);
    }

    #[test]
    fn band_halves_address_disjoint_vertex_runs() {
        let a = swirl_band_draw(SwirlHalf::Primary, 3, 0);
        let b = swirl_band_draw(SwirlHalf::Mirrored, 3, 0);
        assert_eq!(a.first_vertex, 3 * VERTS_PER_BAND);
        assert_eq!(b.first_vertex, 3 * VERTS_PER_BAND + VERTS_PER_HALF);
        assert_eq!((a.tpage, b.tpage), (TPAGE_PRIMARY, TPAGE_MIRRORED));
    }

    #[test]
    fn the_submit_path_swaps_at_the_late_phase_frame() {
        let early = swirl_band_draw(SwirlHalf::Primary, 0, LATE_PHASE_FRAME - 1);
        assert_eq!(early.tint, EARLY_SUBMIT_TINT);
        assert_eq!(early.late_arg, None);

        let late = swirl_band_draw(SwirlHalf::Primary, 0, LATE_PHASE_FRAME);
        assert_eq!(late.tint, LATE_SUBMIT_TINT);
        assert_eq!(
            late.late_arg,
            Some((LATE_PHASE_FRAME - LATE_SUBMIT_EPOCH) * 4)
        );
    }

    #[test]
    fn only_the_first_twelve_bands_ever_draw() {
        let mut m = built_swirl_mesh();
        let mut elapsed = 1i16;
        let mut clock = 0i32;
        let tick = tick_swirl(&mut m, &mut elapsed, 1, &mut clock);
        assert_eq!(tick.draws.len(), BANDS_DRAWN * 2);
        assert!(tick.not_first_frame);
        assert_eq!(elapsed, 2);
        assert_eq!(clock, 1, "DAT_801D2470 took this frame's value");
    }

    #[test]
    fn a_wound_down_band_stops_drawing_but_keeps_integrating() {
        let mut m = built_swirl_mesh();
        m.bands[0].angle = BAND_DRAW_THRESHOLD;
        let before = m.bands[0].angle;
        let mut elapsed = 1i16;
        let mut clock = 0i32;
        let tick = tick_swirl(&mut m, &mut elapsed, 1, &mut clock);
        assert_eq!(
            tick.draws.len(),
            (BANDS_DRAWN - 1) * 2,
            "band 0 sits exactly on the threshold, and the test is `>`"
        );
        assert_ne!(m.bands[0].angle, before);
    }

    #[test]
    fn the_screen_wash_lags_the_band_draws_by_one_frame() {
        let mut m = built_swirl_mesh();
        let mut elapsed = LATE_PHASE_FRAME as i16;
        let mut clock = LATE_PHASE_FRAME;
        // The wash reads the *previous* clock, which is one short of its bound,
        // so it stays clear for the frame on which the clock crosses and for
        // the frame that observes the crossing having happened.
        assert!(!tick_swirl(&mut m, &mut elapsed, 1, &mut clock).late_wash);
        assert!(!tick_swirl(&mut m, &mut elapsed, 1, &mut clock).late_wash);
        assert_eq!(clock, LATE_PHASE_FRAME + 1);
        assert!(tick_swirl(&mut m, &mut elapsed, 1, &mut clock).late_wash);
    }
}

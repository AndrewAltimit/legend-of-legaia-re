//! The field-to-battle transition overlay's two particle-grid seeders.
//!
//! Both `PORT` tags live on [`seed_particle_grid`], which is the one body they
//! share; the disclosure below applies to both.
//!
//! NOT WIRED: the transition state machine itself is now driven from
//! `legaia_engine_core::World::tick_encounter`, but these two are the style-0
//! and style-1 *render* buffers - 1280 sprite records apiece, consumed only by
//! the per-style GTE/GPU packet emitters in PROT 0979, which are
//! documented-not-ported at the clean-room boundary. Wiring them needs a
//! battle-intro particle renderer on the engine side plus the sine / cosine
//! height tables `_DAT_8007B7F8` / `_DAT_8007B81C` the [`ParticleEnv`] trait
//! abstracts; seeding a grid nothing draws would be an inert call with a cost.
//! Neither has a *dumped* retail caller either - see "Callers" below.
//!
//! Both routines do the same job with different constants: allocate one
//! `0xDC00`-byte block, then fill it as a **32 x 40 grid of 1280 particle
//! records, `0x2C` bytes apart** (`32 * 40 * 0x2C == 0xDC00`, which is what
//! fixes the grid shape independently of the loop bounds). Each record gets a
//! rotation derived from its cell, an outward angular velocity taken from the
//! sine / cosine tables at the cell's heading, a translation velocity, and a
//! per-cell texel. What each word *means* is fixed by the two per-frame ticks
//! that read it back - see [`IntroParticle`], whose field names are theirs.
//!
//! The two differ only in constants and in two rules - the y scale, the x
//! origin and step, the velocity scale, the fall rate, the translation
//! velocity, and how the delay field `+0x1E` and the v coordinate `+0x2A` are
//! produced - and [`IntroParticleStyle`] carries exactly those. The
//! `FUN_801D0164` delay is the only one that is not a pure function of the
//! cell: it mixes the cell's distance from the origin with an RNG draw.
//!
//! ## Callers
//!
//! No dump in the corpus contains a `jal` to either address: the five style
//! emitters (`FUN_801CFDA0` / `FUN_801D0370` / `FUN_801D0D24` / `FUN_801D11D0`
//! / `FUN_801D1888`, see `docs/subsystems/cutscene.md`) name neither, and all
//! five printed bodies have now been read end to end. Do not read that as
//! "unused": three of the five styles have a *separate* init routine that the
//! emitters equally never call (`FUN_801D081C` and `FUN_801D1564`, ported in
//! [`crate::battle_intro_tiles`] and [`crate::battle_intro_swirl`]), so the
//! whole family is installed by something upstream of the emitters.
//!
//! What *is* established is the consumer: the two `0x2C`-stride ticks
//! `FUN_801CFDA0` and `FUN_801D0370` walk the entity's `+0x48` block at exactly
//! this stride, and no other routine in the corpus does. Which of the two
//! seeders pairs with which tick is still open.
//!
//! ## Allocation failure
//!
//! Both start by writing `0xFFFFFF` to the entity's `+0x74` and the allocator
//! result to `+0x48`. A null result is not an error path with a message: they
//! bump `_DAT_8007B828` by ten and return, leaving `+0x48` null. The port
//! surfaces that as [`SeedOutcome::OutOfMemory`].
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_field_battle_intro_801cfbb4.txt`
//! and `overlay_field_battle_intro_801d0164.txt`.

/// Grid rows (`slti ..., 0x20`).
pub const PARTICLE_ROWS: usize = 0x20;
/// Grid columns (`slti ..., 0x28`).
pub const PARTICLE_COLS: usize = 0x28;
/// Total particles in one grid.
pub const PARTICLE_COUNT: usize = PARTICLE_ROWS * PARTICLE_COLS;
/// Byte stride of one particle record.
pub const PARTICLE_STRIDE: usize = 0x2C;
/// Byte size of the block both seeders request from the allocator.
pub const PARTICLE_BLOCK_BYTES: usize = 0xDC00;

const _: () = assert!(PARTICLE_COUNT * PARTICLE_STRIDE == PARTICLE_BLOCK_BYTES);

/// The tint word both seeders write to every particle's `+0x04`.
pub const PARTICLE_TINT: u32 = 0x0080_8080;

/// The value both seeders write to the entity's `+0x74` before allocating.
pub const PARTICLE_ENTITY_MASK: u32 = 0x00FF_FFFF;

/// The amount an allocation failure adds to `_DAT_8007B828`.
pub const ALLOC_FAILURE_PENALTY: i32 = 10;

/// The world-space z of grid row 0. Rows step by [`ROW_Z_STEP`].
pub const ROW_Z_ORIGIN: i32 = -0x390;
/// The world-space x of grid column 0 for the heading lookup. Columns step by
/// [`COL_X_STEP`].
pub const COL_X_ORIGIN: i32 = -0x500;
/// Column step of the heading-lookup x. Shared by both styles; only the
/// *stored* x uses the per-style step.
pub const COL_X_STEP: i32 = 0x40;
/// Row step of the heading-lookup z. Shared by both styles.
pub const ROW_Z_STEP: i32 = 0x40;

/// The constants and rules that separate the two seeders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntroParticleStyle {
    /// Left shift applied to the row's z when it is stored at `+0x12`:
    /// `3` for `FUN_801D0164`, `2` for `FUN_801CFBB4`.
    pub row_y_shift: u32,
    /// World x of column 0 as stored at `+0x10`.
    pub col_x_origin: i32,
    /// Column step of the stored x.
    pub col_x_step: i32,
    /// The word written at `+0x14` - the rotation vector's z component.
    pub z_scale: i16,
    /// How the sine / cosine table reads are scaled down before they are
    /// stored at `+0x18` / `+0x1A`. The two routines differ in **rounding**,
    /// not just magnitude - see [`VelocityScale`].
    pub velocity_scale: VelocityScale,
    /// The constant fall rate at `+0x1C`.
    pub fall_rate: i16,
    /// The pair written at `+0x20` / `+0x22`. Both ticks integrate the
    /// translation by it, so it is a velocity - see
    /// [`IntroParticle::trans_vel`].
    pub size: (i16, i16),
    /// The word written at `+0x24` - the third component of the same velocity.
    pub flags: i16,
    /// How `+0x1E` is produced.
    pub phase: PhaseRule,
    /// How `+0x2A` (the v coordinate) is produced.
    pub v_rule: VRule,
}

/// How a seeder scales a trig-table read down into a velocity component.
///
/// This is the one place where copying "divide by N" from the decompiled C
/// would put a wrong value in the engine: `FUN_801D0164`'s
/// `sll v1,v1,0x10; sra v1,v1,0x16` is a sign-extend followed by an
/// **arithmetic shift**, which floors, while `FUN_801CFBB4`'s `0x66666667`
/// magic multiply is a signed divide that truncates toward zero. They disagree
/// for every negative input that is not an exact multiple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VelocityScale {
    /// `FUN_801D0164`: arithmetic right shift by `bits` (floor).
    Shift {
        /// Shift amount; `6` in retail, i.e. a divide by 64.
        bits: u32,
    },
    /// `FUN_801CFBB4`: signed divide by `divisor`, truncating toward zero.
    /// `0x50` (80) in retail.
    TruncatingDivide {
        /// The divisor.
        divisor: i32,
    },
}

impl VelocityScale {
    /// Apply the scale to one trig-table read.
    pub fn apply(self, v: i16) -> i16 {
        match self {
            VelocityScale::Shift { bits } => (v as i32 >> bits) as i16,
            VelocityScale::TruncatingDivide { divisor } => (v as i32 / divisor) as i16,
        }
    }
}

/// The two `+0x1E` rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseRule {
    /// `FUN_801CFBB4`: `(col + row) * 0x40`, a pure diagonal ramp.
    DiagonalRamp,
    /// `FUN_801D0164`: `sqrt(x*x + z*z) / 16 + rand() % 2000` - the cell's
    /// distance from the origin plus a jitter draw. `sqrt` is `FUN_8005AF0C`,
    /// `rand` is the SCUS `FUN_80056798`.
    RadialPlusJitter,
}

/// The two `+0x2A` rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VRule {
    /// `FUN_801D0164`: `row * 8 + 4`.
    RowTimesEightPlusFour,
    /// `FUN_801CFBB4`: a running counter seeded at `4` and stepped by `8` per
    /// **row** - arithmetically the same sequence, kept distinct because the
    /// two routines compute it differently.
    RunningRowCounter,
}

/// `FUN_801CFBB4`'s constants.
pub const STYLE_CFBB4: IntroParticleStyle = IntroParticleStyle {
    row_y_shift: 2,
    col_x_origin: -0x1400,
    col_x_step: 0x100,
    z_scale: 0x1000,
    velocity_scale: VelocityScale::TruncatingDivide { divisor: 0x50 },
    fall_rate: -0x50,
    size: (0x40, 0x40),
    flags: 0x20,
    phase: PhaseRule::DiagonalRamp,
    v_rule: VRule::RunningRowCounter,
};

/// `FUN_801D0164`'s constants.
pub const STYLE_D0164: IntroParticleStyle = IntroParticleStyle {
    row_y_shift: 3,
    col_x_origin: -0x2800,
    col_x_step: 0x200,
    z_scale: 0x2000,
    velocity_scale: VelocityScale::Shift { bits: 6 },
    fall_rate: -8,
    size: (0x20, 0x40),
    flags: 0,
    phase: PhaseRule::RadialPlusJitter,
    v_rule: VRule::RowTimesEightPlusFour,
};

/// One `0x2C`-byte particle record.
///
/// The field names are the **consumers'**, not the seeders'. Both per-frame
/// ticks (`FUN_801CFDA0` / `FUN_801D0370`, ported in
/// [`crate::battle_intro_styles`]) read this record, and what they do with each
/// word is what fixes its meaning:
///
/// * `+0x10..+0x14` is the vector handed to `RotMatrix`, integrated by
///   `+0x18..+0x1C` - so it is a **rotation**, not a position, and the word the
///   seeder puts a `0x1000` / `0x2000` in is its z angle.
/// * `+0x20..+0x24` is what `+0x08..+0x0C` - the vector handed to
///   `SetTransMatrix` - integrates by. It is a **translation velocity**, which
///   is why the earlier reading of `+0x20` / `+0x22` as a sprite size and
///   `+0x24` as a flag word does not survive contact with either tick.
/// * `+0x1E` is compared against the entity clock before anything moves, so it
///   is a spawn **delay**, not a phase.
///
/// `+0x08..+0x0E` is the one region no seeder writes: the ticks integrate it
/// from whatever the allocator left there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntroParticle {
    /// `+0x04` - packed colour. The ticks skip the whole particle when bits
    /// `31..24` are non-zero.
    pub tint: u32,
    /// `+0x08` / `+0x0A` / `+0x0C` - translation. Not seeded; integrated by
    /// [`IntroParticle::trans_vel`].
    pub trans: (i16, i16, i16),
    /// `+0x10` / `+0x12` / `+0x14` - rotation vector. Seeded from the cell's
    /// world position and the style's z constant.
    pub rot: (i16, i16, i16),
    /// `+0x16` - written `1` by `FUN_801CFDA0` on every frame a particle
    /// moves, and never read in either tick's dump.
    pub field_16: i16,
    /// `+0x18` / `+0x1A` / `+0x1C` - angular velocity.
    pub spin: (i16, i16, i16),
    /// `+0x1E` - spawn delay, held against the scaled entity clock.
    pub delay: i16,
    /// `+0x20` / `+0x22` / `+0x24` - translation velocity.
    pub trans_vel: (i16, i16, i16),
    /// `+0x28` - packed `(texture page << 6) | u`.
    pub texel_page: i16,
    /// `+0x2A` - v. Stored as a halfword and consumed as its low byte.
    pub texel_v: i16,
}

/// What a seeder did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedOutcome {
    /// The block came back null. The caller is expected to add
    /// [`ALLOC_FAILURE_PENALTY`] to `_DAT_8007B828`; no particle is written.
    OutOfMemory,
    /// The grid, in retail write order (row-major, column inner).
    Seeded(Vec<IntroParticle>),
}

/// The trig tables and RNG the seeders reach into. Retail reads
/// `_DAT_8007B7F8` and `_DAT_8007B81C` (halfword tables indexed by the
/// heading `FUN_80019B28` returns) and calls `FUN_80056798` for the jitter.
pub trait ParticleEnv {
    /// `FUN_80019B28(0, 0, x, z)` - the 12-bit heading from the origin to
    /// `(x, z)`.
    fn heading(&mut self, x: i32, z: i32) -> i32;
    /// `_DAT_8007B7F8[heading]` as a signed halfword.
    fn sin(&mut self, heading: i32) -> i16;
    /// `_DAT_8007B81C[heading]` as a signed halfword.
    fn cos(&mut self, heading: i32) -> i16;
    /// `FUN_8005AF0C(v)` - integer square root.
    fn sqrt(&mut self, v: i32) -> i32;
    /// `FUN_80056798()` - the SCUS PRNG draw.
    fn rand(&mut self) -> i32;
}

/// Seed one transition particle grid. `FUN_801CFBB4` / `FUN_801D0164`.
///
/// `allocated` is the allocator's answer for the `0xDC00` request
/// (`FUN_80017888(0, 0xDC00)`); `false` takes the out-of-memory arm.
///
/// The heading lookup always uses the *unscaled* cell coordinates
/// `(COL_X_ORIGIN + col * 0x40, ROW_Z_ORIGIN + row * 0x40)`, not the stored
/// position - the stored x is the same cell run through the style's own
/// origin and step. That divergence is retail's, not a simplification.
///
/// PORT: FUN_801CFBB4
/// PORT: FUN_801D0164
/// REF: FUN_80019B28 (heading), FUN_8005AF0C (sqrt), FUN_80056798 (rand)
pub fn seed_particle_grid(
    style: &IntroParticleStyle,
    allocated: bool,
    env: &mut dyn ParticleEnv,
) -> SeedOutcome {
    if !allocated {
        return SeedOutcome::OutOfMemory;
    }

    let mut grid = Vec::with_capacity(PARTICLE_COUNT);
    let mut running_v: i16 = 4;
    for row in 0..PARTICLE_ROWS {
        let cell_z = ROW_Z_ORIGIN + row as i32 * ROW_Z_STEP;
        for col in 0..PARTICLE_COLS {
            let cell_x = COL_X_ORIGIN + col as i32 * COL_X_STEP;
            let heading = env.heading(cell_x, cell_z);
            let phase = match style.phase {
                PhaseRule::DiagonalRamp => ((col + row) as i32 * 0x40) as i16,
                PhaseRule::RadialPlusJitter => {
                    let d = env.sqrt(cell_x * cell_x + cell_z * cell_z);
                    let jitter = env.rand();
                    ((d >> 4) + jitter % 2000) as i16
                }
            };
            let v = match style.v_rule {
                VRule::RowTimesEightPlusFour => (row as i32 * 8 + 4) as i16,
                VRule::RunningRowCounter => running_v,
            };
            grid.push(IntroParticle {
                tint: PARTICLE_TINT,
                trans: (0, 0, 0),
                rot: (
                    (style.col_x_origin + col as i32 * style.col_x_step) as i16,
                    (cell_z << style.row_y_shift) as i16,
                    style.z_scale,
                ),
                field_16: 0,
                spin: (
                    style.velocity_scale.apply(env.sin(heading)),
                    style.velocity_scale.apply(env.cos(heading)),
                    style.fall_rate,
                ),
                delay: phase,
                trans_vel: (style.size.0, style.size.1, style.flags),
                texel_page: (col as i32 * 8) as i16,
                texel_v: v,
            });
        }
        running_v = running_v.wrapping_add(8);
    }
    SeedOutcome::Seeded(grid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic stand-in: quarter-turn trig and a counting PRNG, so the
    /// tests assert the seeder's arithmetic rather than the tables'.
    struct TestEnv {
        rand_seq: i32,
    }

    impl ParticleEnv for TestEnv {
        fn heading(&mut self, x: i32, z: i32) -> i32 {
            (x + z) & 0xFFF
        }
        fn sin(&mut self, heading: i32) -> i16 {
            (heading as i16).wrapping_mul(2)
        }
        fn cos(&mut self, heading: i32) -> i16 {
            -(heading as i16)
        }
        fn sqrt(&mut self, v: i32) -> i32 {
            (v as f64).sqrt() as i32
        }
        fn rand(&mut self) -> i32 {
            self.rand_seq += 1;
            self.rand_seq * 37
        }
    }

    fn env() -> TestEnv {
        TestEnv { rand_seq: 0 }
    }

    #[test]
    fn block_size_matches_the_grid() {
        assert_eq!(PARTICLE_COUNT, 1280);
        assert_eq!(PARTICLE_COUNT * PARTICLE_STRIDE, PARTICLE_BLOCK_BYTES);
    }

    #[test]
    fn allocation_failure_seeds_nothing() {
        assert_eq!(
            seed_particle_grid(&STYLE_D0164, false, &mut env()),
            SeedOutcome::OutOfMemory
        );
    }

    #[test]
    fn grid_is_row_major_and_full() {
        let SeedOutcome::Seeded(g) = seed_particle_grid(&STYLE_CFBB4, true, &mut env()) else {
            panic!("expected a grid");
        };
        assert_eq!(g.len(), PARTICLE_COUNT);
        // Column inner: the first 40 entries share row 0's y.
        let y0 = g[0].rot.1;
        assert!(g[..PARTICLE_COLS].iter().all(|p| p.rot.1 == y0));
        assert_ne!(g[PARTICLE_COLS].rot.1, y0);
    }

    #[test]
    fn cfbb4_positions_and_phase() {
        let SeedOutcome::Seeded(g) = seed_particle_grid(&STYLE_CFBB4, true, &mut env()) else {
            panic!()
        };
        // Cell (0,0): x = -0x1400, y = -0x390 << 2.
        assert_eq!(
            (g[0].rot.0, g[0].rot.1),
            (-0x1400, ((-0x390i32) << 2) as i16)
        );
        assert_eq!(g[0].delay, 0, "(col + row) * 0x40 at the origin cell");
        assert_eq!((g[0].texel_page, g[0].texel_v), (0, 4));
        assert_eq!(g[0].trans_vel, (0x40, 0x40, 0x20));
        assert_eq!(g[0].spin.2, -0x50);
        assert_eq!(g[0].rot.2, 0x1000);
        // Cell (0,1): x steps by 0x100, u by 8.
        assert_eq!(g[1].rot.0, -0x1400 + 0x100);
        assert_eq!(g[1].texel_page, 8);
        assert_eq!(g[1].delay, 0x40);
        // Row 1 raises v by 8.
        assert_eq!(g[PARTICLE_COLS].texel_v, 12);
    }

    #[test]
    fn d0164_positions_and_v_rule() {
        let SeedOutcome::Seeded(g) = seed_particle_grid(&STYLE_D0164, true, &mut env()) else {
            panic!()
        };
        assert_eq!(
            (g[0].rot.0, g[0].rot.1),
            (-0x2800, ((-0x390i32) << 3) as i16)
        );
        assert_eq!((g[0].texel_page, g[0].texel_v), (0, 4));
        assert_eq!(g[PARTICLE_COLS].texel_v, 12, "row * 8 + 4");
        assert_eq!(g[0].trans_vel, (0x20, 0x40, 0));
        assert_eq!(g[0].spin.2, -8);
        assert_eq!(g[0].rot.2, 0x2000);
    }

    #[test]
    fn the_two_v_rules_agree_numerically() {
        let SeedOutcome::Seeded(a) = seed_particle_grid(&STYLE_CFBB4, true, &mut env()) else {
            panic!()
        };
        let SeedOutcome::Seeded(b) = seed_particle_grid(&STYLE_D0164, true, &mut env()) else {
            panic!()
        };
        // Different code, same sequence - which is why the doc keeps them
        // separate but the test pins the equivalence.
        assert!(a.iter().zip(b.iter()).all(|(x, y)| x.texel_v == y.texel_v));
    }

    #[test]
    fn radial_phase_mixes_distance_and_jitter() {
        let mut e = env();
        let SeedOutcome::Seeded(g) = seed_particle_grid(&STYLE_D0164, true, &mut e) else {
            panic!()
        };
        // First cell: sqrt(0x500^2 + 0x390^2) >> 4, plus the first draw % 2000.
        let d = ((0x500i32 * 0x500 + 0x390 * 0x390) as f64).sqrt() as i32;
        // The first draw is 37, and 37 % 2000 is 37.
        assert_eq!(g[0].delay, ((d >> 4) + 37) as i16);
    }

    #[test]
    fn the_two_velocity_scales_round_differently() {
        let shift = VelocityScale::Shift { bits: 6 };
        let divide = VelocityScale::TruncatingDivide { divisor: 0x50 };
        // Positive inputs agree in shape; negatives are where they part.
        assert_eq!(shift.apply(-0x41), -2, "arithmetic shift floors");
        assert_eq!(shift.apply(-1), -1, "and keeps flooring near zero");
        assert_eq!(divide.apply(-0x4F), 0, "the magic divide truncates");
        assert_eq!(divide.apply(-0x51), -1);
    }
}

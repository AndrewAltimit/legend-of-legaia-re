//! The field-to-battle intro, driven end to end from the *live* transition
//! clock through all four ported style working sets.
//!
//! ## What this test is for
//!
//! `battle_intro_transition::tick_transition` is wired -
//! `legaia_engine_core::World::tick_battle_intro` runs it once per frame for
//! as long as the encounter session sits in its `Transition` phase. The five
//! style kernels underneath it are not, and each carries a `NOT WIRED:` note
//! naming what has to exist first.
//!
//! Those notes assert something checkable: that the missing pieces are the
//! *working-set owner* and the *per-frame draw emitter*, and **not** the
//! kernels, which already compose from the live clock with no gap in the
//! arithmetic. This test pins exactly that claim. It seeds every style's
//! working set, runs each style against the same `elapsed` counter
//! `tick_transition` advances, and asserts each one produces live per-frame
//! output.
//!
//! It is deliberately **not** a wire, and it must not be read as one: nothing
//! here is reachable from a host root, and the assertions are about the
//! kernels composing, not about anything being drawn. What it buys is a
//! regression guard - if a future edit breaks the composition, the corrected
//! disclosures in those modules become false and this fails.
//!
//! Disc-free by construction: the trig tables and PRNG are synthetic (see
//! [`IntroEnv`]), and the curtain's descriptor table is a synthetic four-record
//! stand-in for the one that lives in PROT 0979. No Sony bytes.

use legaia_engine_vm::battle_intro_particles::{self as particles, ParticleEnv, SeedOutcome};
use legaia_engine_vm::battle_intro_styles::{
    self as styles, PARTICLE_TICK_A, PARTICLE_TICK_B, PARTICLE_TICK_COUNT,
};
use legaia_engine_vm::battle_intro_swirl::{self as swirl, SwirlBuildOutcome, SwirlTrig};
use legaia_engine_vm::battle_intro_tiles::{self as tiles, TileSeedOutcome, TileSubStyle};
use legaia_engine_vm::battle_intro_transition::{
    IntroQuadDesc, TransitionEntity, TransitionGlobals, TransitionResponses, tick_transition,
};

/// Frame step the transition advances its clock by. Retail's per-frame step is
/// the display-frame delta; `1` is the steady-state NTSC value.
const FRAME_STEP: u8 = 1;

/// How many frames the intro is driven for. Long enough to pass the swirl's
/// `LATE_PHASE_FRAME` (0x5A) so the late-phase arms are exercised too.
const FRAMES: i32 = 0x78;

/// Deterministic stand-in for the game's two trig tables, integer square root
/// and PRNG.
///
/// The tables retail reads (`_DAT_8007B7F8` / `_DAT_8007B81C`) are disc data,
/// so this computes q12 sine/cosine instead. That changes the *values* a
/// seeded particle gets, which is fine - this test asserts the kernels compose
/// and stay live, not that they reproduce a captured frame.
struct IntroEnv {
    rng: legaia_engine_vm::battle_intro_particles::IntroRng,
}

impl IntroEnv {
    fn new() -> Self {
        Self {
            rng: legaia_engine_vm::battle_intro_particles::IntroRng::new(0x1234_5678),
        }
    }

    /// q12 sine of a 12-bit (0..0x1000) heading.
    fn sin_q12(units: i32) -> i16 {
        let radians = (units.rem_euclid(0x1000) as f64) * std::f64::consts::TAU / 4096.0;
        (radians.sin() * 4096.0) as i16
    }
}

impl ParticleEnv for IntroEnv {
    fn heading(&mut self, x: i32, z: i32) -> i32 {
        let a = (z as f64).atan2(x as f64);
        ((a * 4096.0 / std::f64::consts::TAU) as i32).rem_euclid(0x1000)
    }

    fn sin(&mut self, heading: i32) -> i16 {
        Self::sin_q12(heading)
    }

    fn cos(&mut self, heading: i32) -> i16 {
        Self::sin_q12(heading + 0x400)
    }

    fn sqrt(&mut self, v: i32) -> i32 {
        if v <= 0 { 0 } else { (v as f64).sqrt() as i32 }
    }

    fn rand(&mut self) -> i32 {
        // PsyQ-shaped 15-bit draw: the value range is what the seeders' `%`
        // arms care about, not the exact sequence. Shared with the shell's own
        // stand-in so a degenerate stream cannot hide here.
        self.rng.draw()
    }
}

impl SwirlTrig for IntroEnv {
    fn table_x(&mut self, entry: i32) -> i16 {
        IntroEnv::sin_q12(entry + 0x400)
    }

    fn table_y(&mut self, entry: i32) -> i16 {
        IntroEnv::sin_q12(entry)
    }
}

/// Run the live transition state machine the way `World::tick_battle_intro`
/// does, and return the per-frame `elapsed` clock the style kernels ride.
///
/// The globals mirror the engine's own construction: `assembly_state = 0x80`
/// (the engine assembles battle meshes synchronously, so phase 1 passes on its
/// first tick) and an idle load response (the formation is already resolved).
fn transition_clock() -> (Vec<i16>, TransitionEntity) {
    let mut entity = TransitionEntity::default();
    let globals = TransitionGlobals {
        battle_id: 1,
        total_duration: FRAMES,
        assembly_state: 0x80,
        ..Default::default()
    };
    let mut clock = Vec::new();
    for frame in 0..FRAMES {
        entity.elapsed = frame as i16;
        clock.push(entity.elapsed);
        let _ = tick_transition(&mut entity, &globals, &TransitionResponses::default());
    }
    (clock, entity)
}

#[test]
fn the_live_transition_sm_runs_to_completion_and_yields_a_clock() {
    let (clock, entity) = transition_clock();

    assert_eq!(clock.len(), FRAMES as usize);
    // Phase 7 is terminal: its arm raises `ready` bit 1 and never sets
    // `advance`, so the machine rests there rather than walking off the end of
    // the `sltiu v0, v1, 0x8` bound. Resting at 7 is the success state.
    assert_eq!(
        entity.phase, 7,
        "phase machine did not reach the terminal phase"
    );
    // `ready == 3` is the documented completion state: bit 0 from the
    // post-switch spin test (`total_duration - 0x1E < elapsed`), bit 1 from
    // phase 7. Both must be set or the handoff never reports ready.
    assert_eq!(
        entity.ready, 3,
        "transition finished without both ready bits - the handoff would hang"
    );
}

#[test]
fn both_particle_fields_seed_and_stay_live_across_the_transition() {
    for (label, seed_style, tick_style) in [
        ("FUN_801CFDA0", &particles::STYLE_CFBB4, &PARTICLE_TICK_A),
        ("FUN_801D0370", &particles::STYLE_D0164, &PARTICLE_TICK_B),
    ] {
        let mut env = IntroEnv::new();
        let SeedOutcome::Seeded(mut grid) =
            particles::seed_particle_grid(seed_style, true, &mut env)
        else {
            panic!("{label}: seeder took the out-of-memory arm with `allocated = true`");
        };
        assert_eq!(
            grid.len(),
            particles::PARTICLE_COUNT,
            "{label}: seeder wrote the wrong grid size"
        );
        // The ticks visit 0x488 of the 0x500 seeded records - the last 120 are
        // seeded and never drawn. That is a property of the style, not a slip.
        const {
            assert!(PARTICLE_TICK_COUNT < particles::PARTICLE_COUNT);
        }

        let (clock, _) = transition_clock();
        let mut moved_total = 0usize;
        let mut elapsed = clock[0];
        for _ in &clock {
            let tick = styles::tick_particle_field(&mut grid, tick_style, &mut elapsed, FRAME_STEP);
            assert!(
                tick.masked + tick.moved <= PARTICLE_TICK_COUNT,
                "{label}: tick visited more records than the style's bound"
            );
            moved_total += tick.moved;
        }
        assert!(
            moved_total > 0,
            "{label}: no particle ever passed its spawn delay across {FRAMES} frames - \
             the field would be visually dead"
        );
        assert_eq!(
            elapsed, FRAMES as i16,
            "{label}: the tick did not advance the clock by one step per frame"
        );
    }
}

#[test]
fn the_tile_grid_seeds_and_keeps_drawing_across_the_transition() {
    let mut env = IntroEnv::new();
    // The `0x801CE8BC` corner table is overlay data the engine does not load;
    // the obvious ordering is used here only to give the seeder a shape, and
    // the module deliberately does not assert it.
    let TileSeedOutcome::Seeded(mut grid) = tiles::seed_tile_grid(
        TileSubStyle::RadialDelayWithTumble,
        true,
        [0, 1, 0x11, 0x12],
        &mut env,
    ) else {
        panic!("tile seeder took the out-of-memory arm with `allocated = true`");
    };
    assert_eq!(grid.vertices.len(), tiles::GRID_DIM * tiles::GRID_DIM);
    assert_eq!(grid.tiles.len(), tiles::TILE_DIM * tiles::TILE_DIM);

    let (clock, _) = transition_clock();
    let mut elapsed = clock[0];
    let mut drawn_total = 0usize;
    let mut moved_total = 0usize;
    for _ in &clock {
        let tick = tiles::tick_tile_grid(&mut grid, &mut elapsed, FRAME_STEP);
        drawn_total += tick.drawn;
        moved_total += tick.moved;
    }
    assert!(
        drawn_total > 0 && moved_total > 0,
        "tile grid never drew ({drawn_total}) or never moved ({moved_total}) across {FRAMES} frames"
    );
}

#[test]
fn the_swirl_mesh_builds_and_issues_paired_band_draws() {
    let mut env = IntroEnv::new();
    let SwirlBuildOutcome::Built(mut mesh) = swirl::build_swirl_mesh(true, &mut env) else {
        panic!("swirl builder took the out-of-memory arm with `allocated = true`");
    };
    assert_eq!(mesh.vertices.len(), swirl::BANDS * swirl::VERTS_PER_BAND);
    assert_eq!(mesh.texels.len(), swirl::BANDS * swirl::VERTS_PER_BAND);

    let (clock, _) = transition_clock();
    let mut elapsed = clock[0];
    let mut prev_clock = 0i32;
    let mut draws_total = 0usize;
    let mut saw_late_wash = false;
    for _ in &clock {
        let tick = swirl::tick_swirl(&mut mesh, &mut elapsed, FRAME_STEP, &mut prev_clock);
        // Every drawn band issues exactly two halves - primary then mirrored.
        assert_eq!(
            tick.draws.len() % 2,
            0,
            "a band issued an unpaired half-draw"
        );
        draws_total += tick.draws.len();
        saw_late_wash |= tick.late_wash;
    }
    assert!(
        draws_total > 0,
        "no swirl band ever cleared the draw threshold across {FRAMES} frames"
    );
    assert!(
        saw_late_wash,
        "the late-phase wash never fired, so {FRAMES} frames did not reach LATE_PHASE_FRAME"
    );
}

#[test]
fn the_curtain_emits_row_and_column_strips_through_the_shared_quad_builder() {
    // Stand-in for the descriptor table in PROT 0979. Only indices
    // CURTAIN_ROW_DESC (3) and CURTAIN_COL_DESC (2) are read, but the table is
    // sized to the two the style patches plus the lower entries it indexes past.
    let mut table = vec![
        IntroQuadDesc {
            size_q12: 0x1000,
            w: 0x40,
            h: 0x40,
            top: [0x80, 0x80, 0x80],
            bottom: [0x80, 0x80, 0x80],
            ..Default::default()
        };
        4
    ];

    let (clock, _) = transition_clock();
    let mut elapsed = clock[0];
    let mut quads_total = 0usize;
    let mut culled_total = 0usize;
    for _ in &clock {
        let tick = styles::tick_curtain(&mut table, &mut elapsed, FRAME_STEP);
        // `_DAT_8007B6CC` is written twice and the second store wins, so this
        // style - alone among the five - always reports "first frame".
        assert!(
            !tick.not_first_frame,
            "the curtain reported a non-first frame; the dead first store came back"
        );
        quads_total += tick.quads.len();
        culled_total += tick.culled_columns;
    }
    assert!(
        quads_total > 0,
        "the curtain built no quads, so build_intro_quad never ran"
    );
    // As the clock runs the columns warp off-screen, so the visibility test
    // must reject some of them - otherwise the stretch is not happening.
    assert!(
        culled_total > 0,
        "no column was ever culled across {FRAMES} frames - the warp is not advancing"
    );
}

#[test]
fn every_style_rides_the_same_clock_the_wired_transition_advances() {
    // The point of the whole file: one clock, four working sets, no divergence
    // in how the styles consume it. Each tick advances `elapsed` by exactly
    // `frame_step`, which is what lets a single host own one counter.
    let mut env = IntroEnv::new();
    let SeedOutcome::Seeded(mut grid) =
        particles::seed_particle_grid(&particles::STYLE_CFBB4, true, &mut env)
    else {
        panic!("particle seeder failed");
    };
    let TileSeedOutcome::Seeded(mut tile_grid) = tiles::seed_tile_grid(
        TileSubStyle::NegSpinRandomDelay,
        true,
        [0, 1, 0x11, 0x12],
        &mut env,
    ) else {
        panic!("tile seeder failed");
    };
    let SwirlBuildOutcome::Built(mut mesh) = swirl::build_swirl_mesh(true, &mut env) else {
        panic!("swirl builder failed");
    };

    let mut particle_clock = 0i16;
    let mut tile_clock = 0i16;
    let mut swirl_clock = 0i16;
    let mut prev = 0i32;

    for frame in 0..FRAMES {
        styles::tick_particle_field(&mut grid, &PARTICLE_TICK_A, &mut particle_clock, FRAME_STEP);
        tiles::tick_tile_grid(&mut tile_grid, &mut tile_clock, FRAME_STEP);
        swirl::tick_swirl(&mut mesh, &mut swirl_clock, FRAME_STEP, &mut prev);

        let expected = (frame + 1) as i16;
        assert_eq!(particle_clock, expected, "particle clock diverged");
        assert_eq!(tile_clock, expected, "tile clock diverged");
        assert_eq!(swirl_clock, expected, "swirl clock diverged");
    }
}

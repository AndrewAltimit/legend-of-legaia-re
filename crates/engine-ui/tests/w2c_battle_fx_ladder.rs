//! Move-FX streak **schedule ladder**: drive the production dispatcher
//! [`legaia_engine_ui::streak_pass::streak_quads_scheduled`] through the
//! retail counter walk and assert each regime at the emitted packets.
//!
//! The dispatcher is the routine both hosts call once per battle frame (the
//! native window's `move_fx_streak_quads`, the browser play page's
//! `play_battle` streak arm), and its selector is the streak counter
//! `ctx[+0x6C6]`, walked down 4 per frame by the phase driver
//! (`MoveFxStreak::tick_counter` engine-side). The decoded schedule
//! (`FUN_801E09F8` phase 1, `0x801E0C64..0x801E0CE8`):
//!
//! * **party** acting actor: single-billboard afterimage while
//!   `counter >= 0x281`, a dead band through `0x280..0x201` where nothing
//!   draws at all, then the chained ribbon (`FUN_801E1D98`,
//!   `build_streak_ribbon`) below `0x201`;
//! * **monster** acting actor: the ribbon at every counter value.
//!
//! Walking the whole counter range through the dispatcher is what makes this
//! a ladder rather than a re-run of the per-kernel unit tests: the regime
//! *boundaries* are the dispatcher's own comparisons, and the ribbon arm is
//! reached the way a host reaches it - by the counter falling, not by a
//! direct call.
//!
//! Disc-free: the inputs are the terminator's context words, which are plain
//! numbers, and the projection is the engine camera. Runs everywhere.

use glam::{Mat4, Vec3};
use legaia_engine_ui::afterimage::{CLUT_BASE, MODULATION_COLOR, TEXPAGE};
use legaia_engine_ui::streak_pass::{
    AFTERIMAGE_COUNTER_MIN, RIBBON_COUNTER_MAX, StreakSource, streak_quads_scheduled,
};

/// The battle camera's shape: perspective * look-at. The launch point sits
/// deep enough that the ribbon's constant `0x100` half-width projects under
/// the `RIBBON_MAX_TOP_EDGE_SPAN` suppression test - the same condition a
/// mid-arena monster meets in a real fight.
fn cam() -> Mat4 {
    Mat4::perspective_rh(1.0, 4.0 / 3.0, 1.0, 10_000.0)
        * Mat4::look_at_rh(
            Vec3::new(0.0, 0.0, 1000.0),
            Vec3::ZERO,
            Vec3::new(0.0, 1.0, 0.0),
        )
}

/// A staged move: the terminator wrote a launch point and the move-power
/// record staged trail column 3 (`0x7703`).
fn source(counter: u16) -> StreakSource {
    // `ctx[+0x6C6] - 0x200` is the afterimage half-width; the ribbon ignores
    // it. `from_block` is the production lift from the engine world.
    StreakSource::from_block(
        Some((0, 0, -2500)),
        (counter as i32 - 0x200) as i16,
        Some(0x7703),
    )
    .expect("an armed block lifts")
}

#[test]
fn the_counter_walk_hands_off_from_afterimage_to_ribbon_at_the_decoded_bounds() {
    let mvp = cam();
    let mut afterimage_frames = 0usize;
    let mut dead_frames = 0usize;
    let mut ribbon_frames = 0usize;

    // The phase driver's own walk: 4 per frame, from a fresh arm to zero.
    let mut counter: u16 = 0x300;
    let mut frame = 0u32;
    while counter > 0 {
        let quads = streak_quads_scheduled(&source(counter), &mvp, frame, counter, true);
        if counter >= AFTERIMAGE_COUNTER_MIN {
            assert_eq!(
                quads.len(),
                1,
                "party afterimage regime at counter {counter:#x} must emit \
                 exactly one packet"
            );
            afterimage_frames += 1;
        } else if counter >= RIBBON_COUNTER_MAX {
            assert!(
                quads.is_empty(),
                "the dead band at counter {counter:#x} must draw nothing"
            );
            dead_frames += 1;
        } else {
            assert!(
                quads.len() > 1,
                "the ribbon regime at counter {counter:#x} must emit a chain, \
                 got {} packet(s)",
                quads.len()
            );
            ribbon_frames += 1;
            // Retail packet law, unchanged across every segment: the trail
            // CLUT row, the constant texpage, semi-transparent modulated.
            for q in &quads {
                assert_eq!(q.clut, CLUT_BASE + 3, "trail id 3 picks CLUT row +3");
                assert_eq!(q.tpage, TEXPAGE);
                assert_eq!(q.color, MODULATION_COLOR);
                assert!(q.semi_transparent);
                // Ribbon UV law: top row 0x00, bottom row 0x3f, one 0x20-wide
                // band sampled left-to-right (not the afterimage's mirror).
                let band = q.uv[0].0;
                assert_eq!(band & 0x1f, 0, "band {band:#04x} is a sub-column base");
                assert_eq!(q.uv[1].0, band | 0x1f);
                assert_eq!((q.uv[0].1, q.uv[1].1), (0x00, 0x00));
                assert_eq!((q.uv[2].1, q.uv[3].1), (0x3f, 0x3f));
            }
            // The chain climbs: each further segment's top edge sits above
            // (screen Y smaller than) the previous one's.
            for pair in quads.windows(2) {
                assert!(
                    pair[1].xy[0].1 < pair[0].xy[0].1,
                    "ribbon segments must climb"
                );
            }
        }
        counter = counter.saturating_sub(4);
        frame += 1;
    }
    eprintln!(
        "[w2c-streak] afterimage_frames={afterimage_frames} dead_frames={dead_frames} \
         ribbon_frames={ribbon_frames}"
    );
    // All three regimes must actually have run, or the walk said nothing.
    assert!(afterimage_frames > 0 && dead_frames > 0 && ribbon_frames > 0);
}

#[test]
fn a_monster_draws_the_ribbon_at_any_counter() {
    let mvp = cam();
    // A counter deep in the party's afterimage regime still draws the ribbon
    // for a monster acting actor (`0x801E0C7C` takes the ribbon branch before
    // the counter compares).
    let quads = streak_quads_scheduled(&source(0x300), &mvp, 0, 0x300, false);
    assert!(
        quads.len() > 1,
        "monster ribbon suppressed at counter 0x300 ({} packets)",
        quads.len()
    );
    // Determinism per frame - the wobble is the frame-seeded BIOS rand, so a
    // replay of the same frame emits the same packets.
    assert_eq!(
        quads,
        streak_quads_scheduled(&source(0x300), &mvp, 0, 0x300, false)
    );
    // And the shimmer moves between frames, or the trail is a static quad.
    let later = streak_quads_scheduled(&source(0x300), &mvp, 9, 0x300, false);
    assert_ne!(quads, later, "the ribbon wobble never moved");
}

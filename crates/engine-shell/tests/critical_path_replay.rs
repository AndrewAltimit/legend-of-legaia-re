//! Critical-path replay: the chapter-1 spine walked by **pad**, and scored.
//!
//! Every other progression oracle in this crate is *disc-denominated* - it
//! asks whether the disc's bytes decode, whether a leg's records resolve,
//! whether a transition fires. This one is **game-denominated**: it asks how
//! far a player pressing buttons can actually get from a cold boot, and
//! reports that as a number that goes up.
//!
//! ## Why this is not the spine oracle
//!
//! [`chapter1_spine_oracle`](chapter1_spine_oracle.rs) proves the same legs,
//! but it moves the player with [`World::seat_player_at_tile`] - its own
//! `walk_onto_tile` helper is a *teleport pair* (seat one tile off the
//! trigger, then seat onto it) that synthesises the tile crossing the walk-on
//! dispatch keys off. `scripts/replays/chapter1_spine.toml` says so plainly:
//! the `[[event]]` pad rows "document the traversal" while the disc-gated diff
//! "drives the transitions by seating the player on the trigger tiles".
//!
//! That is the right design for asking *does the scene graph connect*. It is
//! structurally blind to the class of defect that teleporting steps over:
//! locomotion speed and heading, the collision probe, the walkability grid,
//! the camera-relative pad remap. A port whose lead actor glides, or whose
//! remap is a quadrant off, or whose collision seals a doorway, passes the
//! spine oracle unchanged.
//!
//! Here movement is produced **only** by [`World::set_pad`] +
//! [`SceneHost::tick`]. Nothing is seated. If the player cannot walk out of
//! Rim Elm, this test says so and names the tile they stopped on.
//!
//! ## The ladder
//!
//! Milestones are ordered and cumulative - the run stops at the first one it
//! cannot reach, and the score is the count it cleared:
//!
//! | # | milestone | what it proves |
//! |---|---|---|
//! | 1 | `town01` loads into free-roam Field, entry script running | the scene boots to a controllable player and its system script executes |
//! | 2 | pad-walk to the south gate -> `map01` | field locomotion + collision + walk-on trigger |
//! | 3 | pad-walk `map01` across the continent | overworld remap + collision + the encounter round trip |
//! | 4 | pad-walk on to the Ravine, via `suimon` | multi-scene overworld route + portal engage |
//! | 5 | pad-walk the Ravine interior out of a *different* door | dungeon locomotion + the scene's own exit bands |
//!
//! Rung 5 exists because rungs 2-4 all end the instant a door fires, so none
//! of them walks a dungeon *interior*. That is its own surface: the exits are
//! `.MAP` walk-on bands rather than overworld entity portals and the corridors
//! are the narrowest geometry the leading-edge probe meets. Under a four-rung
//! ladder "the Ravine loads" and "the Ravine can be walked" scored the same.
//!
//! What it does **not** cover is the dungeon's own encounter table, and that
//! is a finding rather than a design choice - `keikoku` decodes to a single
//! rate-0 whole-map region and rolls nothing however the loop is armed. The
//! `LEGAIA_CPR_FIGHT=1` run prints the region set at the rung's start.
//!
//! It is scored on **which door the player came out of**, not on the bare
//! transition. `keikoku` carries four scene-change records and all four return
//! to `map01`, each at its own tile, so the arrival coordinate names the door
//! exactly ([`ExitRecord`]) - and the one thing a step back through the
//! entrance can never produce is a different one.
//!
//! Rung 3 is the first leg of rung 4's route, scored on its own, because that
//! leg is sixty-odd tiles long: under a single portal check, "walked nowhere"
//! and "crossed the continent and was turned back at the last ridge" are the
//! same number. Rung 3's threshold is [`OVERWORLD_CROSSING_TILES`] **and** at
//! least one random encounter fought and survived - the capability
//! [`drain_battle`] documents, and the one a regression would otherwise
//! re-break silently.
//!
//! Rung 4 is **three** legs, not one, because `map01` is not one walk
//! component: the arrival is on the northern half, every `keikoku` mouth is on
//! the southern half, and the crossing between them is the `suimon` scene.
//! See [`SUIMON_SOUTH_EXIT`] and [`WATER_GATE_FLAG`].
//!
//! ## Navigation
//!
//! Waypoints come from a BFS over the walkability grid, with each edge
//! validated by [`World::field_dir_blocked`] (retail `FUN_801cfe4c`'s
//! static-wall arm) *and* [`World::field_actor_dir_blocked`] (its prop / NPC
//! sibling) at the source node - so the planner and the engine consult the
//! same obstacles. The follower converts the desired world step into a pad
//! mask by inverting whatever remap the world is currently walking under,
//! which is what a player does when they look at the screen and press the
//! direction that moves them the way they want.
//!
//! There are **two** such remaps and they are not interchangeable: the field
//! walk goes through `decode_field_direction` (world axes, quadrant rotation),
//! the overworld walk through `world_map_camera_relative_bits` (camera-relative).
//! Inverting the field one on the overworld sends the player off at an angle,
//! and map01's collision leaves the sea open, so nothing stops them - the leg
//! walks off the map rather than stalling against a wall. See
//! [`pad_for_step`].
//!
//! On the overworld the route has two more constraints a field route does not:
//! other scenes' portals are **hazards** to route around, because stepping on
//! one enters it ([`portal_hazards`]), and a scene with several doors needs
//! the door the walk can actually reach ([`portal_tile`]).
//!
//! A hazard is a tile it is unsafe to **enter**, never a tile it is unsafe to
//! *occupy*, and the difference is not cosmetic. Retail's dispatcher fires on
//! a tile *change*, so a step that stays inside one tile fires nothing - and a
//! dungeon arrival is seated on the door it came in through, i.e. inside a
//! hazard. A planner that tests only the destination tile therefore refuses
//! its own first step and reports the scene as sealed. See [`plan_path`].
//!
//! A leg that stops making progress is reported as `Stalled` with the tile it
//! died on, not as a bare assertion failure - a stall is the finding.
//!
//! ## A leg is not always walking
//!
//! Two modes take the frame away mid-leg and they are not the same problem.
//! A dialogue or cutscene timeline owns *input* while the world stays put
//! ([`drain_scripted`], which pages through it the way a player does - a
//! neutral pad is enough for a sequence that runs on its own clock and is not
//! enough for one that waits on a confirm). A random encounter replaces the
//! *world*: in
//! [`SceneMode::Battle`] the player actor's `move_state` is the battle arena
//! transform, so [`player_world`] stops meaning an overworld position at all.
//! [`drain_battle`] takes the frame off the walk and hands it to
//! [`FightPolicy`], which *plays* the encounter through the battle command UI
//! rather than sitting it out; reading both doc comments first will save
//! re-deriving why an unguarded run reports a coordinate off the corner of a
//! map the player is standing in the middle of, and why a neutral pad is a
//! fighting model rather than the absence of one.
//!
//! ## Ratchet
//!
//! `scripts/replays/critical_path_baseline.toml` carries the highest score
//! reached so far. The test asserts `score >= baseline` (no regression) and
//! prints the line to paste when the score goes up. It never auto-writes the
//! baseline: raising it is a reviewed edit, the same contract
//! `scripts/ci/disc-coverage.py` uses.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or
//! `extracted/` missing.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use legaia_engine_field::world_map_entity_config::WorldMapEntityConfig;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use legaia_engine_system::input::PadButton;

mod environment;
mod exits;
mod fights;
mod ladder;
mod planner;
mod route;
mod rung4_diagnostics;
mod walker;

use environment::*;
use exits::*;
use fights::*;
use ladder::*;
use planner::*;
use route::*;
use rung4_diagnostics::*;
use walker::*;

#[test]
fn critical_path_score_does_not_regress() {
    let Some(mut host) = open_host() else {
        return;
    };

    let rungs = run_ladder(&mut host);
    let score = rungs.iter().take_while(|r| r.cleared).count();
    let baseline = read_baseline();

    eprintln!("\n=== critical-path replay (pad-driven, one session) ===");
    for r in &rungs {
        eprintln!(
            "  [{}] {:<40} {}",
            if r.cleared { "ok" } else { "--" },
            r.label,
            r.detail
        );
    }
    eprintln!(
        "  score {score} / {} rungs attempted (baseline {baseline})",
        rungs.len()
    );
    if score > baseline {
        eprintln!(
            "  baseline can be raised: set `reached = {score}` in {}",
            baseline_path().display()
        );
    }
    eprintln!();

    assert!(
        score >= baseline,
        "critical-path score regressed: {score} < baseline {baseline}. \
         The first unclear rung above names the tile the run died on."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pad inversion must round-trip through the same quadrant rotation
    /// `decode_field_direction` applies, for every quadrant and every
    /// cardinal world step. Disc-free: this is arithmetic, and it is the one
    /// piece of the navigator that can be wrong in a way that still looks
    /// like movement (the player walks confidently in the wrong direction).
    #[test]
    fn pad_inversion_round_trips_through_every_quadrant() {
        // Mirror of World::decode_field_direction's screen -> world rotation.
        fn forward(azimuth: u16, pad: u16) -> (i16, i16) {
            let mut sx = 0i16;
            let mut sy = 0i16;
            if pad & PadButton::Up.mask() != 0 {
                sy += 1;
            }
            if pad & PadButton::Down.mask() != 0 {
                sy -= 1;
            }
            if pad & PadButton::Right.mask() != 0 {
                sx += 1;
            }
            if pad & PadButton::Left.mask() != 0 {
                sx -= 1;
            }
            let quadrant = ((azimuth as u32).wrapping_add(512) / 1024) & 3;
            let (wx, wz) = match quadrant {
                0 => (sx, sy),
                1 => (sy, -sx),
                2 => (-sx, -sy),
                _ => (-sy, sx),
            };
            (wx.clamp(-1, 1), wz.clamp(-1, 1))
        }

        // One azimuth inside each quadrant, including the wrap boundary.
        for azimuth in [0u16, 700, 1024, 1800, 2048, 2900, 3072, 4000] {
            for (dwx, dwz) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (-1, 1),
                (1, -1),
                (-1, -1),
            ] {
                let pad = pad_for_world_step(azimuth, dwx, dwz);
                assert_eq!(
                    forward(azimuth, pad),
                    (dwx, dwz),
                    "azimuth {azimuth} step ({dwx},{dwz}) did not round-trip (pad {pad:#06x})"
                );
            }
        }
    }

    /// The overworld inversion is checked against the engine's **own** forward
    /// remap, not a re-derivation of it - the two walks use different spaces
    /// and the field inversion above is silently wrong here.
    ///
    /// The contract is *not* an exact round trip, and asserting one would be a
    /// bug in the test: a 4-way d-pad through
    /// `world_map_camera_relative_bits` reaches only 8 world directions, and
    /// which 8 depends on the azimuth, so at a rotated framing no press yields
    /// a pure cardinal. What the follower needs - and what is asserted - is
    /// that the press always moves the player *toward* the request: the
    /// resulting world direction has a strictly positive dot product with it,
    /// i.e. it is within 90°. Disc-free.
    #[test]
    fn world_map_pad_inversion_always_moves_toward_the_request() {
        use legaia_engine_core::world::world_map_camera_relative_bits;

        for azimuth in [0i32, 700, 1024, 1800, 2048, 2900, 3072, 4000] {
            for (dwx, dwz) in [(1i16, 0i16), (-1, 0), (0, 1), (0, -1)] {
                let pad = world_map_pad_for_world_step(azimuth, dwx, dwz);
                assert_ne!(pad, 0, "azimuth {azimuth} step ({dwx},{dwz}): no press");
                let sx = i32::from(pad & PadButton::Right.mask() != 0)
                    - i32::from(pad & PadButton::Left.mask() != 0);
                let sy = i32::from(pad & PadButton::Up.mask() != 0)
                    - i32::from(pad & PadButton::Down.mask() != 0);
                let bits = world_map_camera_relative_bits(azimuth, sx, sy);
                let gz = i32::from(bits & 0x1000 != 0) - i32::from(bits & 0x4000 != 0);
                let gx = i32::from(bits & 0x2000 != 0) - i32::from(bits & 0x8000 != 0);
                let dot = gx * i32::from(dwx) + gz * i32::from(dwz);
                assert!(
                    dot > 0,
                    "azimuth {azimuth} step ({dwx},{dwz}) walked the wrong way \
                     (pad {pad:#06x} -> screen ({sx},{sy}) -> world ({gx},{gz}))"
                );
            }
        }
    }

    /// Rung 5's door-identity clause. The four real `keikoku` return tiles
    /// are used as the fixture, because the shape that matters is "two doors
    /// of the same scene differ only in their arrival tile" - the case a
    /// scene-name comparison silently passes. Disc-free.
    #[test]
    fn a_door_is_only_different_when_both_ends_are_known() {
        // keikoku partition-2 records 0..3, in order.
        const RETURNS: [(i16, i16); 4] = [(52, 94), (64, 67), (77, 68), (82, 83)];
        let entrance = RETURNS[1]; // arrived through record 1
        for (i, &r) in RETURNS.iter().enumerate() {
            assert_eq!(
                left_by_another_door(Some(r), Some(entrance)),
                i != 1,
                "landing on record {i}'s return tile {r:?} after entering by record 1"
            );
        }
        // No transition at all is not a traverse, however far the walk went.
        assert!(!left_by_another_door(None, Some(entrance)));
        // Nor is a transition whose entrance record could not be identified:
        // an unknown door is not a different door.
        assert!(!left_by_another_door(Some(RETURNS[0]), None));
        assert!(!left_by_another_door(None, None));
    }

    /// A missing baseline file reads as zero rather than panicking, so a
    /// fresh clone runs the ladder instead of failing on a file it has not
    /// been given.
    #[test]
    fn absent_baseline_reads_as_zero() {
        // `read_baseline` resolves a fixed repo path; this asserts the parse
        // contract on the text form directly.
        let parse = |text: &str| -> usize {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.starts_with('#'))
                .find_map(|l| {
                    l.strip_prefix("reached")?
                        .trim()
                        .strip_prefix('=')?
                        .trim()
                        .parse()
                        .ok()
                })
                .unwrap_or(0)
        };
        assert_eq!(parse(""), 0);
        assert_eq!(
            parse("# reached = 9\n"),
            0,
            "a commented row is not a value"
        );
        assert_eq!(parse("reached = 2\n"), 2);
        assert_eq!(parse("# note\nreached = 3\n"), 3);
    }
}

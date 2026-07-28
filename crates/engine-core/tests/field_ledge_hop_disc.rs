//! Disc-gated: a **real authored ledge in a real scene** starts a hop through
//! the ordinary pad path, and the arc carries the player over it.
//!
//! The synthetic coverage in `field_ledge_hop_wired.rs` pins the clip's shape
//! against a hand-built grid. This file answers the different question - does
//! any shipped scene actually contain geometry the walk controller classifies
//! as a ledge - by searching the scene's own `.MAP` collision + elevation data
//! for a candidate and then walking the player into it with `World::tick`, the
//! same per-frame path `play-window` and the browser play page use.
//!
//! The search predicate is exactly `FUN_801d1878`'s: both forward probe points
//! (`+64` and `+96` units along the committed step delta) clear of walls, and
//! the floor sampled `+32` units ahead crossing the `+0x61` / `-0x60`
//! thresholds against the actor's own height.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing - CI runs
//! without disc data.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};
use legaia_engine_core::world::SceneMode;

/// Retail's movement-disabled / hop-lock bit on the player actor's `+0x10`.
const MOVE_LOCK: u32 = 0x0008_0000;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// One `+Z`-facing ledge candidate: the world position to stand at, and the
/// floor rise the probe would classify there.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    x: i32,
    z: i32,
    rise: i32,
}

/// Sweep the live scene grid for positions where a `+Z` step faces a ledge.
///
/// Restricted to `+Z` so the driver can hold a single d-pad direction: at
/// camera azimuth `0` (what a field entry installs) Up maps to `+Z`, which
/// `field_locomotion_disc.rs` pins independently.
fn plus_z_candidates(host: &SceneHost) -> Vec<Candidate> {
    let mut out = Vec::new();
    if host.world.field_collision_grid.len() < 0x4000 {
        return out;
    }
    // The step-delta probe scale: `s0 = dz << 2` with `dz = 8`.
    const S: i32 = 32;
    for zi in 1..255i32 {
        let z = zi * 64 + 32;
        for xi in 0..256i32 {
            let x = xi * 64 + 32;
            if host.world.field_tile_is_wall(x as i16, z as i16) {
                continue;
            }
            // Both forward points must be clear - the actor is stepping up,
            // not walking into a wall.
            if host.world.field_tile_is_wall(x as i16, (z + 2 * S) as i16)
                || host.world.field_tile_is_wall(x as i16, (z + 3 * S) as i16)
            {
                continue;
            }
            let here = host.world.sample_field_floor_height(x, z);
            let ahead = host.world.sample_field_floor_height(x, z + S);
            let rise = ahead - here;
            // Retail's dead band: `slti 0x61` / `slti -0x60` in FUN_801d1878.
            if !(-0x60..0x61).contains(&rise) {
                out.push(Candidate { x, z, rise });
            }
        }
    }
    out
}

/// Walk the player into `cand` and return the completed hop, or `None` when
/// the approach never classifies one (the sampled point is not necessarily on
/// the walk's own step parity).
fn drive_into(host: &mut SceneHost, cand: Candidate) -> Option<HopRun> {
    // Start a short way back so the trigger fires off a genuinely committed
    // walk step rather than off the placement.
    host.world.actors[0].move_state.world_x = cand.x as i16;
    host.world.actors[0].move_state.world_z = (cand.z - 64) as i16;
    host.world.actors[0].move_state.flags &= !MOVE_LOCK;
    host.world.field_ledge_hop = None;

    let mut started = None;
    for _ in 0..24 {
        host.world.set_pad(PadButton::Up.mask());
        let _ = host.world.tick();
        if let Some(h) = host.world.field_ledge_hop {
            started = Some(h);
            break;
        }
    }
    let hop = started?;
    let take_off = {
        let ms = &host.world.actors[0].move_state;
        (ms.world_x, ms.world_y, ms.world_z)
    };
    assert!(
        host.world.actors[0].move_state.flags & MOVE_LOCK != 0,
        "the hop setup must lock the player"
    );

    // Release the pad: the clip is committed. Track the trajectory to the
    // landing and the release.
    let mut peak_y = take_off.1;
    let mut landed = None;
    let mut released = None;
    for frame in 1..64 {
        host.world.set_pad(0);
        let _ = host.world.tick();
        let Some(h) = host.world.field_ledge_hop else {
            break;
        };
        let ms = &host.world.actors[0].move_state;
        peak_y = peak_y.min(ms.world_y);
        if h.landed && landed.is_none() {
            landed = Some((frame, (ms.world_x, ms.world_y, ms.world_z)));
        }
        if h.finished {
            released = Some(frame);
        }
    }
    Some(HopRun {
        cand,
        kind: hop.kind,
        take_off,
        target: (hop.target_x, hop.target_y, hop.target_z),
        peak_y,
        landed,
        released,
    })
}

struct HopRun {
    cand: Candidate,
    kind: u16,
    take_off: (i16, i16, i16),
    target: (i16, i16, i16),
    peak_y: i16,
    landed: Option<(usize, (i16, i16, i16))>,
    released: Option<usize>,
}

#[test]
fn a_real_scene_ledge_starts_and_completes_a_hop() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));

    // The play-window default: the walk snaps the player's Y to the sampled
    // floor, which is what puts the actor at the height the hop classifier
    // measures against.
    let mut run = None;
    let mut scanned = Vec::new();
    for scene in ["town01", "uru", "tunnelc"] {
        host.enter_field_scene(scene, 0)
            .unwrap_or_else(|e| panic!("enter_field_scene('{scene}') failed: {e:#}"));
        assert!(matches!(host.world.mode, SceneMode::Field));
        host.world.follow_terrain_height = true;
        // Let the scene prescript run: it is what paints the story-conditional
        // wall deltas on top of the base grid.
        for _ in 0..600 {
            host.world.set_pad(0);
            let _ = host.world.tick();
        }
        let cands = plus_z_candidates(&host);
        eprintln!("[{scene}] +Z ledge candidates: {}", cands.len());
        scanned.push((scene, cands.len()));
        for cand in cands.iter().take(32) {
            if let Some(r) = drive_into(&mut host, *cand) {
                eprintln!(
                    "[{scene}] hop at ({}, {}) rise {} -> kind {:#04x}",
                    cand.x, cand.z, cand.rise, r.kind
                );
                run = Some((scene, r));
                break;
            }
        }
        if run.is_some() {
            break;
        }
    }

    // If this ever fires it is a finding, not a flake: it would mean no
    // shipped scene carries geometry the classifier calls a ledge, and the
    // whole controller would be dead weight rather than a wiring gap.
    let (scene, run) = run.unwrap_or_else(|| {
        panic!("no reachable authored ledge found; candidate counts: {scanned:?}")
    });

    // The candidate really was a ledge, and the class matches its direction.
    // World Y grows downward: a numerically larger floor ahead is lower, and
    // retail's drop apex is 0x10 / its step-up apex 0x18.
    let expect_kind = if run.cand.rise >= 0x61 { 0x10 } else { 0x18 };
    assert_eq!(run.kind, expect_kind, "[{scene}] hop class vs floor rise");

    // The landing point is retail's: 96 units along the step delta.
    assert_eq!(
        run.target.2,
        run.take_off.2 + 96,
        "[{scene}] landing is three step-deltas ahead"
    );
    assert_eq!(run.target.0, run.take_off.0, "[{scene}] no lateral drift");

    // The arc runs its full 16 frames and puts the player exactly on the
    // landing triple.
    let (landed_frame, landed_pos) = run.landed.expect("[{scene}] the arc must land");
    assert_eq!(landed_frame, 0x10, "[{scene}] 0x1000 / 0x100 = 16 frames");
    assert_eq!(landed_pos, run.target, "[{scene}] landed on the target");

    // It is an arc, not a lerp: the flight clears the higher of the two
    // endpoints (Y down, so "higher" is the smaller value).
    assert!(
        run.peak_y < run.take_off.1.min(run.target.1),
        "[{scene}] peak {} did not clear endpoints {} / {}",
        run.peak_y,
        run.take_off.1,
        run.target.1
    );

    // And the movement lock is released six frames after the landing.
    assert_eq!(
        run.released,
        Some(0x16),
        "[{scene}] landing + 6 frames of recovery"
    );
    assert_eq!(
        host.world.actors[0].move_state.flags & MOVE_LOCK,
        0,
        "[{scene}] the player walks again afterwards"
    );
    eprintln!(
        "[{scene}] hop verified: {:?} -> {:?}, peak {} ",
        run.take_off, run.target, run.peak_y
    );
}

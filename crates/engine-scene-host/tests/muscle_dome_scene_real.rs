//! Disc-gated: the Muscle Dome's 3D arena surface
//! ([`legaia_engine_minigame_scenes::muscle_dome_scene`]) over the real disc.
//!
//! Seats the first rung of the course ladder through the same `read_prot`
//! closure the hosts pass, and asserts structure only: the fighter, the
//! monster and the arena shell all decode, both bodies project inside the
//! frame under the surface's camera on opposite sides of the screen, the
//! idle clips move the bodies between frames, and the generation holds
//! still while the seated pair does. No Sony bytes are asserted. Skips +
//! passes when `LEGAIA_DISC_BIN` is absent.

use legaia_engine_battle_menus::muscle_dome_session::{MuscleCard, MuscleDomeSession};
use legaia_engine_minigame_scenes::muscle_dome_scene::MuscleDomeSurface;
use legaia_engine_scene_host::scene_host::SceneHost;

fn project(vp: &[f32; 16], p: [f32; 3]) -> Option<[f32; 2]> {
    let c = |r: usize| vp[r] * p[0] + vp[4 + r] * p[1] + vp[8 + r] * p[2] + vp[12 + r];
    let w = c(3);
    (w > 0.0).then(|| [c(0) / w, c(1) / w])
}

fn centroid(ps: &[[f32; 3]]) -> [f32; 3] {
    let n = ps.len().max(1) as f32;
    let mut s = [0.0f32; 3];
    for p in ps {
        for k in 0..3 {
            s[k] += p[k];
        }
    }
    [s[0] / n, s[1] / n, s[2] / n]
}

#[test]
fn the_dome_surface_poses_the_real_fighters_in_the_arena() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let read = |i: usize| host.index.entry_bytes(i as u32).ok();
    let card = |command_id: u8| MuscleCard {
        command_id,
        cost: 0x1E,
    };
    let hand = [card(0xC), card(0xD), card(0xE), card(0xF)];
    let session = MuscleDomeSession::new(hand, hand, [120, 120], [500, 400], 1);

    let mut surface = MuscleDomeSurface::default();
    let scene = surface
        .frame(read, Some(&session), None, 0)
        .expect("the first rung's dome decodes");
    let [fb, mb, sb] = scene.bases;
    assert!(fb == 0 && mb > 0 && sb > mb, "both bodies carry geometry");
    assert!(
        scene.positions.len() > sb,
        "the arena shell + ground follow the bodies"
    );
    assert!(scene.positions.iter().flatten().all(|v| v.is_finite()));
    assert!(!scene.textured_indices.is_empty());
    assert_eq!(scene.flat_rgba.len(), scene.positions.len() * 4);
    let vp = scene.camera.vp_raw(4.0 / 3.0);
    let f = project(&vp, centroid(&scene.positions[fb..mb])).expect("fighter in front");
    let m = project(&vp, centroid(&scene.positions[mb..sb])).expect("monster in front");
    for p in [f, m] {
        assert!(p[0].abs() < 1.0 && p[1].abs() < 1.0, "on screen: {p:?}");
    }
    assert!(
        (f[0] - m[0]).abs() > 0.1,
        "the pair stands apart on screen: {f:?} vs {m:?}"
    );
    let first = scene.positions[fb..sb].to_vec();
    let generation = surface.generation();
    assert!(surface.vram().is_some());

    let mut moved = false;
    for _ in 0..60 {
        let s = surface
            .frame(read, Some(&session), None, 0)
            .expect("still live");
        moved |= s.positions[fb..sb] != first[..];
    }
    assert!(moved, "the idle clips animate the bodies");
    assert_eq!(surface.generation(), generation, "same seat, same buffers");

    assert!(surface.frame(read, None, None, 0).is_none());
    assert!(surface.scene().is_none(), "no session drops the scene");
    eprintln!("[ran] dome surface: {} verts", first.len());
}

/// The minigames page re-seats the surface on the next rung's opponent
/// without a session-less frame in between (`muscle_start_vs` replaces the
/// leg in one call), so the surface must rebuild its posed buffers on a new
/// `(monster, character)` key: round 2 posed into round 1's index / UV /
/// CBA buffers is the "opponent mesh corrupts on the second round" defect.
/// The re-seated scene must equal a fresh surface's scene for the same pair.
#[test]
fn a_new_opponent_rebuilds_the_posed_buffers() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let read = |i: usize| host.index.entry_bytes(i as u32).ok();
    let ladder = read(legaia_engine_minigame_scenes::muscle_dome_scene::ARENA_OVERLAY_PROT_INDEX)
        .and_then(|raw| legaia_engine_minigames::muscle_dome::parse_course_ladder(&raw))
        .expect("the course ladder parses");
    let rounds = &ladder[0].rounds;
    assert!(rounds.len() >= 2, "course 0 has a second round");
    let (a, b) = (rounds[0].monster_id as u16, rounds[1].monster_id as u16);
    assert_ne!(a, b, "rounds 1 and 2 seat different opponents");

    let card = |command_id: u8| MuscleCard {
        command_id,
        cost: 0x1E,
    };
    let hand = [card(0xC), card(0xD), card(0xE), card(0xF)];
    let leg = || MuscleDomeSession::new(hand, hand, [120, 120], [500, 400], 1);

    // Round 1, played for a few frames.
    let s1 = leg();
    let mut surface = MuscleDomeSurface::default();
    for _ in 0..8 {
        surface
            .frame_seated(read, &s1, a, 0)
            .expect("round 1 decodes");
    }
    let generation = surface.generation();

    // Round 2 against the next rung, straight after (no None frame).
    let s2 = leg();
    let reseated = surface
        .frame_seated(read, &s2, b, 0)
        .expect("round 2 decodes")
        .clone();
    assert_ne!(
        surface.generation(),
        generation,
        "a new seat bumps the generation"
    );

    let mut fresh = MuscleDomeSurface::default();
    let want = fresh
        .frame_seated(read, &s2, b, 0)
        .expect("round 2 decodes fresh")
        .clone();
    assert_eq!(
        reseated.bases, want.bases,
        "body offsets follow the new opponent"
    );
    assert_eq!(reseated.positions.len(), want.positions.len());
    assert_eq!(
        reseated.indices, want.indices,
        "triangles are the new opponent's"
    );
    assert_eq!(reseated.uvs, want.uvs);
    assert_eq!(reseated.cba_tsb, want.cba_tsb);
    assert_eq!(reseated.flat_rgba, want.flat_rgba);
    assert_eq!(reseated.textured_indices, want.textured_indices);
    assert_eq!(reseated.untextured_indices, want.untextured_indices);
    assert_eq!(
        reseated.positions, want.positions,
        "same pose on the same tick"
    );
    eprintln!(
        "[ran] dome re-seat {a:#x} -> {b:#x}: {} verts",
        want.positions.len()
    );
}

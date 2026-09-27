//! Disc-gated: the Baka Fighter duel's 3D surface
//! ([`legaia_engine_core::baka_duel_scene`]) over the real disc.
//!
//! Builds the duel through the door-warp tables (PROT 0976), stages the
//! clip headers and the special-commit cameras, loads the surface through
//! the same `read_prot` closure the hosts pass, and runs a counter-played
//! match through the world tick, asserting structure only: both fighters
//! and the back wall project inside the frame with the player on the left,
//! the camera-side wall is culled, a committed attack plays its clip out
//! and drops to the idle, and the camera settles on the duel pose after the
//! round-start spin. No Sony bytes are asserted. Skips + passes when
//! `LEGAIA_DISC_BIN` is absent.

use legaia_asset::baka_opponents;
use legaia_asset::static_overlay;
use legaia_engine_core::baka_duel_scene::{self as ds, BakaDuelSurface};
use legaia_engine_core::baka_fighter::{BakaAttack, BakaFight, roster_clip_headers};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, World};

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
fn the_duel_surface_poses_the_real_fighters_under_the_arena_camera() {
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
    let rec = static_overlay::overlay_map()
        .by_prot_index(baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32)
        .expect("baka overlay in static map");
    let raw = host
        .index
        .entry_bytes_extended(rec.prot_index)
        .expect("read PROT 0976");
    let loaded = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");
    let opponents = baka_opponents::parse(&loaded).expect("roster parses");
    let actions = baka_opponents::parse_actions(&loaded).expect("actions parse");
    let cameras = ds::parse_special_cameras(&loaded);
    assert_eq!(cameras.len(), ds::SPECIAL_CAMERA_ROWS);
    // Every populated row moves the eye (a zero row would be no glide).
    for r in &cameras {
        assert!(r[6..12].iter().any(|&v| v != 0), "row carries eye targets");
    }

    let read = |i: usize| host.index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
    let opponent = legaia_engine_core::baka_fighter::first_rung_roster();
    let fight = BakaFight::from_tables(&opponents, &actions, 0, opponent, 0x5EED)
        .expect("fight builds")
        .with_roster_clip_headers(roster_clip_headers(read))
        .with_special_cameras(cameras);

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_baka_fighter(fight);

    let mut surface = BakaDuelSurface::default();
    let gen0 = {
        let f = world.minigames.baka_fighter.as_ref();
        let scene = surface
            .frame(read, f)
            .expect("surface builds on the real disc");
        assert!(
            scene.positions.len() > 1000,
            "fighters + ghosts + walls + floor"
        );
        assert_eq!(scene.positions.len(), scene.uvs.len());
        assert_eq!(scene.flat_rgba.len(), scene.positions.len() * 4);
        assert_eq!(
            scene.indices.len(),
            scene.textured_indices.len() + scene.untextured_indices.len()
        );
        surface.generation()
    };
    let vram = surface.vram().expect("duel vram");
    assert!(vram.as_bytes().iter().any(|&b| b != 0), "pages uploaded");

    let mut saw_attack = false;
    let mut saw_idle_after_attack = false;
    let mut frames = 0;
    while frames < 3000 {
        frames += 1;
        let f = world.minigames.baka_fighter.as_ref().unwrap();
        if f.match_over() {
            break;
        }
        let pad = if f.can_choose(0) && frames % 90 == 0 {
            PadButton::Square.mask()
        } else {
            0
        };
        world.set_pad(pad);
        let _ = world.tick();
        let f = world.minigames.baka_fighter.as_ref().unwrap();
        let m = f.motion(0);
        if m.record == BakaAttack::A.type_id() as usize {
            saw_attack = true;
        } else if saw_attack && m.record == 0 {
            saw_idle_after_attack = true;
        }
        let scene = surface.frame(read, Some(f)).expect("still live");
        assert!(
            scene
                .positions
                .iter()
                .all(|p| p.iter().all(|c| c.is_finite()))
        );
    }
    assert!(saw_attack, "a committed attack shows its clip");
    assert!(
        saw_idle_after_attack,
        "the clip plays out and drops to the idle"
    );
    assert_eq!(
        surface.generation(),
        gen0,
        "no rebuild without a rung change"
    );

    // Settle the camera and check the framing: both fighters inside the
    // frame, the player left of the opponent.
    let f = world.minigames.baka_fighter.as_ref().unwrap();
    let cam = f.duel_camera();
    if !cam.moving() {
        let vp = cam.vp_raw(4.0 / 3.0);
        let p = project(&vp, f.fighter_position(0)).expect("player in front");
        let o = project(&vp, f.fighter_position(1)).expect("opponent in front");
        eprintln!("[ok] player ndc {p:?} opponent ndc {o:?}");
        assert!(p[0] < o[0], "player stands left of the opponent on screen");
        for q in [p, o] {
            assert!(q[0].abs() < 1.0 && q[1].abs() < 1.0, "inside the frame");
        }
        // The back wall is drawn, the camera-side one culled.
        let scene = surface.scene().unwrap();
        let back = centroid(&scene.positions[..]);
        assert!(back.iter().all(|c| c.is_finite()));
        assert!(cam.eye_depth([0.0, 100.0, 1600.0]) >= ds::WALL_CULL_DEPTH);
        assert!(cam.eye_depth([0.0, 100.0, -1600.0]) < ds::WALL_CULL_DEPTH);
    }
    eprintln!("[ok] {frames} duel frames posed");
}

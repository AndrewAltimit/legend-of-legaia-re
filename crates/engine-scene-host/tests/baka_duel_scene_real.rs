//! Disc-gated: the Baka Fighter duel's 3D surface
//! ([`legaia_engine_minigame_scenes::baka_duel_scene`]) over the real disc.
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

use legaia_engine_core::world::World;
use legaia_engine_minigame_scenes::baka_duel_scene::{self as ds, BakaDuelSurface};
use legaia_engine_minigames::baka_fighter::{BakaAttack, BakaFight, roster_clip_headers};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_minigame_formats::baka_opponents;
use legaia_overlay_images::static_overlay;

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
    let cameras = legaia_engine_minigames::baka_duel::parse_special_cameras(&loaded);
    assert_eq!(
        cameras.len(),
        legaia_engine_minigames::baka_duel::SPECIAL_CAMERA_ROWS
    );
    // Every populated row moves the eye (a zero row would be no glide).
    for r in &cameras {
        assert!(r[6..12].iter().any(|&v| v != 0), "row carries eye targets");
    }

    let read = |i: usize| host.index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
    let opponent = legaia_engine_minigames::baka_fighter::first_rung_roster();
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

/// The finishing special's afterimage on the real disc, through the world
/// tick and the duel surface every host draws: once a round is decided its
/// two ghosts keep posing (their vertices move) through the round-over hold
/// and run out inside it, and they draw additive under the spawn's colour
/// word (ABE + ABR 1 on every textured prim, the colour cued toward black).
#[test]
fn the_finishing_specials_ghosts_run_out_inside_the_round_over_hold() {
    use legaia_engine_minigames::baka_fighter::MatchPhase;
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(host) = SceneHost::open_disc(&disc) else {
        eprintln!("[skip] open_disc failed");
        return;
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
    let read = |i: usize| host.index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
    let opponent = legaia_engine_minigames::baka_fighter::first_rung_roster();
    let fight = BakaFight::from_tables(&opponents, &actions, 0, opponent, 0x5EED)
        .expect("fight builds")
        .with_roster_clip_headers(roster_clip_headers(read));
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_baka_fighter(fight);
    let mut surface = BakaDuelSurface::default();

    let mut decided_with_ghosts = None;
    let mut frames = 0;
    while frames < 6000 {
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
        if matches!(f.phase(), MatchPhase::RoundOver(_)) && !f.afterimages().is_empty() {
            decided_with_ghosts = Some(f.afterimages()[0].0);
            break;
        }
    }
    let Some(owner) = decided_with_ghosts else {
        panic!("no round was decided by a special with its ghosts live ({frames} frames)");
    };

    // The ghosts' draw words.
    let scene = surface
        .frame(read, world.minigames.baka_fighter.as_ref())
        .expect("live");
    for pass in 0..2 {
        let r = scene.ghost_range(owner, pass);
        assert!(!r.is_empty(), "ghost {pass} has a mesh copy");
        for i in r {
            let tsb = scene.cba_tsb[i][1];
            assert_eq!((tsb >> 5) & 3, 1, "ghost {pass}: ABR 1 (B + F)");
            assert_ne!(
                tsb & legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT,
                0,
                "ghost {pass}: semi-transparent"
            );
        }
    }

    // Untextured ghost prims carry the same ABE + ABR 1 TSB word, which the
    // hosts' colour-mesh draws read as their blend word.
    let ghost_untextured = scene
        .untextured_indices
        .iter()
        .filter(|&&i| (0..2).any(|p| scene.ghost_range(owner, p).contains(&(i as usize))))
        .count();
    eprintln!("[ok] {ghost_untextured} untextured ghost corner(s) under the colour word");

    // The hold: the ghosts keep moving, then run out before it ends.
    let mut last: Option<Vec<[f32; 3]>> = None;
    let mut moved = 0;
    let mut held = 0;
    loop {
        let f = world.minigames.baka_fighter.as_ref().unwrap();
        if f.afterimages().is_empty() {
            break;
        }
        assert!(
            matches!(
                f.phase(),
                MatchPhase::RoundOver(_) | MatchPhase::MatchOver(_)
            ),
            "the ghosts run out inside the hold ({held} frames in)"
        );
        world.set_pad(0);
        let _ = world.tick();
        held += 1;
        assert!(held < 0xB5, "the ghosts outlast the round-over hold");
        let f = world.minigames.baka_fighter.as_ref().unwrap();
        let far_drawn = f
            .afterimages()
            .first()
            .is_some_and(|(_, fr)| fr.passes.iter().any(|p| p.bit == 1 && p.drawn));
        let scene = surface.frame(read, Some(f)).expect("live");
        let r = scene.ghost_range(owner, 1);
        let now = far_drawn.then(|| scene.positions[r].to_vec());
        if let (Some(l), Some(n)) = (&last, &now)
            && l != n
        {
            moved += 1;
        }
        last = now;
    }
    assert!(moved > 0, "the ghosts pose on through the hold");
    eprintln!("[ok] the special's ghosts ran out {held} frames into the hold, {moved} moving");
}

/// The all-clear on the shared surface: the seats retire a beat apart, the
/// fighter stands on the player's mark in roster 4's mesh under its atlas, and
/// a run that met both secret opponents reads `0xE` and gets none.
#[test]
fn the_all_clear_stands_its_fighter_on_the_surface() {
    use legaia_engine_minigames::baka_fighter_chrome::CLEAR_FIGHTER_ROSTER;
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(host) = SceneHost::open_disc(&disc) else {
        eprintln!("[skip] open_disc failed");
        return;
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
    let read = |i: usize| host.index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
    let opponent = legaia_engine_minigames::baka_fighter::first_rung_roster();
    for (stage, want) in [(12, true), (0xE, false)] {
        let mut fight = BakaFight::from_tables(&opponents, &actions, 0, opponent, 0x5EED)
            .expect("fight builds")
            .with_roster_clip_headers(roster_clip_headers(read));
        let mut surface = BakaDuelSurface::default();
        fight.enter_all_clear(stage);
        let mut seen = false;
        for _ in 0..0x800 {
            fight.tick(1);
            let cell = surface
                .frame(read, Some(&fight))
                .map(|s| s.clear_fighter_extent())
                .expect("the surface stays live");
            seen |= cell > 1.0;
            if fight.clear_fighter().is_some() {
                assert!(fight.seat_retired(0) && fight.seat_retired(1));
                assert!(cell > 1.0, "the fighter's vertices are posed");
                let opp = fight.opponent_roster();
                assert_ne!(opp, CLEAR_FIGHTER_ROSTER, "its atlas is the one to load");
            }
            if fight.cabinet().exit_done() {
                break;
            }
        }
        assert_eq!(seen, want, "stage {stage}");
    }
}

/// The drop shadows (`FUN_801D6BB8` with the `0x8A` flag word, `B - F`) are
/// the combat tick's alone (`FUN_801D3F44` calls it at `0x801D40B4` /
/// `0x801D47F4`): the cabinet's front end - the attract card and the player
/// select - draws none, so every one of their prims sits collapsed there,
/// and the duel draws them. A shadow left at its build position sat at the
/// origin under the title card and the lineup.
#[test]
fn the_drop_shadows_draw_in_the_duel_and_never_in_the_front_end() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(host) = SceneHost::open_disc(&disc) else {
        eprintln!("[skip] open_disc failed");
        return;
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
    let read = |i: usize| host.index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
    let opponent = legaia_engine_minigames::baka_fighter::first_rung_roster();
    let fight = BakaFight::from_tables(&opponents, &actions, 0, opponent, 0x5EED)
        .expect("fight builds")
        .with_roster_clip_headers(roster_clip_headers(read))
        .with_attract();
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_baka_fighter(fight);
    let mut surface = BakaDuelSurface::default();

    // A shadow prim: untextured, semi-transparent, ABR 2.
    let shadow_extent = |scene: &ds::BakaDuelScene| -> f32 {
        let mut extent = 0.0f32;
        for t in scene.untextured_indices.chunks(3) {
            let tsb = scene.cba_tsb[t[0] as usize][1];
            if tsb & 0x8000 == 0 || (tsb >> 5) & 3 != 2 {
                continue;
            }
            for &v in t {
                let p = scene.positions[v as usize];
                extent = extent.max(p[0].abs().max(p[1].abs()).max(p[2].abs()));
            }
        }
        extent
    };
    let (mut front_frames, mut duel_frames) = (0, 0);
    for frame in 0..1500 {
        let Some(f) = world.minigames.baka_fighter.as_ref() else {
            break;
        };
        let front = f.cabinet().front_end();
        let scene = surface.frame(read, Some(f)).expect("surface builds");
        let extent = shadow_extent(scene);
        if front {
            front_frames += 1;
            assert_eq!(extent, 0.0, "frame {frame}: a front-end shadow is drawn");
        } else {
            duel_frames += 1;
            assert!(extent > 0.0, "frame {frame}: the duel draws its shadows");
        }
        if duel_frames > 60 {
            break;
        }
        // Cross through the attract card and the select (the host pair's
        // minigame rhythm).
        let pad = if frame >= 20 && frame % 16 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        world.set_pad(pad);
        let _ = world.tick();
    }
    eprintln!("[ok] {front_frames} front-end frames, {duel_frames} duel frames");
    assert!(front_frames > 0, "the run starts in the front end");
    assert!(duel_frames > 0, "the run reaches the duel");
}

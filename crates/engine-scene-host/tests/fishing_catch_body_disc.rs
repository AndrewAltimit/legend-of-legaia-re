//! Disc-gated: a landed fish is on screen. Retail shows the catch twice
//! (`docs/subsystems/minigame-fishing.md`, "The landed fish"): the hooked
//! fish's body flung up through the close-up, then the result actor's body
//! hanging from a line in the middle of the plate. Both come out of the pond
//! surface every play host draws (`FishingSurface`), and the plate's line out
//! of `FishingScene::catch_line`.
//!
//! The plate's numbers are pinned to a retail film of a catch
//! (`autorun_fishing_catch_film.lua` from `minigame_fishing_retail_lure_cast`,
//! a species-5 fish at render scale `0x52B`): the line's tie lands at
//! `(159, 92)` once the fish has settled. Skips and passes without
//! `LEGAIA_DISC_BIN`.

use legaia_engine_fishing::fishing::{PondPhase, WINDUP_FRAMES};
use legaia_engine_fishing::fishing_actors::{CatchFish, CatchFishPlace, plate_fish_y};
use legaia_engine_minigame_scenes::fishing_catch::{CatchBodies, plate_line};
use legaia_engine_minigame_scenes::fishing_scene::{FISHING_VENUE_SCENE, FishingSurface};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_overlay_images::static_overlay;

fn enter(disc: &std::ffi::OsStr) -> SceneHost {
    let mut host = SceneHost::open_disc(disc).expect("open disc");
    host.load_scene("town01").expect("town01");
    let rec = static_overlay::overlay_map()
        .by_prot_index(legaia_minigame_formats::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32)
        .expect("fishing overlay in static map");
    let raw = host.index.entry_bytes_extended(rec.prot_index).unwrap();
    let loaded = static_overlay::as_loaded(&raw, rec).unwrap();
    assert!(host.enter_fishing_from_overlay(&loaded));
    assert_eq!(host.world.mode, SceneMode::Fishing);
    host
}

/// One host frame: the world tick, the venue's actor frame, the surface.
fn frame(host: &mut SceneHost, surface: &mut FishingSurface, mask: u16) {
    host.world.set_pad(mask);
    let _ = host.world.tick();
    legaia_engine_scene_host::fishing_venue::tick_fishing_venue_on_host(host);
    surface.frame(&host.index, &host.world.minigames, true, [0; 3]);
}

fn phase(host: &SceneHost) -> PondPhase {
    host.world.minigames.fishing.as_ref().unwrap().phase()
}

/// Every vertex of a span at one point: the body rasterises nothing.
fn collapsed(pos: &[[f32; 3]]) -> bool {
    pos.iter().all(|p| *p == pos[0])
}

#[test]
fn a_landed_fish_is_lifted_then_hangs_on_the_plate() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = enter(&disc);
    assert_eq!(
        host.world.minigames.fishing_catch_poses.len(),
        legaia_minigame_formats::fishing_species::SPECIES_COUNT,
        "the showcase poses decode off the overlay"
    );
    let mut surface = FishingSurface::default();
    frame(&mut host, &mut surface, 0);
    let spans = surface
        .scene()
        .expect("the pond decodes")
        .catch_spans
        .clone();
    assert!(
        spans.iter().all(Option::is_some),
        "all ten bodies resolve: {spans:?}"
    );

    // Cast at full power, work the lure, fight on the gauge.
    let cast = |host: &mut SceneHost, surface: &mut FishingSurface| {
        frame(host, surface, PadButton::Cross.mask());
        for _ in 0..WINDUP_FRAMES + 112 {
            frame(host, surface, 0);
        }
        frame(host, surface, PadButton::Cross.mask());
    };
    let mut landed = false;
    for f in 0..200_000usize {
        match phase(&host) {
            PondPhase::Idle => cast(&mut host, &mut surface),
            PondPhase::Hooked => {
                let t = host.world.minigames.fishing.as_ref().unwrap().tension();
                let m = if t < 0x800 {
                    PadButton::Cross.mask()
                } else {
                    0
                };
                frame(&mut host, &mut surface, m);
            }
            PondPhase::Landed => {
                landed = true;
                break;
            }
            _ => {
                let m = if f % 65 < 25 {
                    PadButton::Cross.mask()
                } else {
                    0
                };
                frame(&mut host, &mut surface, m);
            }
        }
        // No body shows before the catch.
        if phase(&host) == PondPhase::Landed {
            continue;
        }
        let scene = surface.scene().unwrap();
        assert!(scene.catch_fish.is_none());
        for (b, n) in scene.catch_spans.iter().flatten() {
            assert!(collapsed(&scene.positions[*b..b + n]));
        }
    }
    assert!(landed, "a fish lands within the budget");
    // The landing frame still draws the catch HUD (the lure tick lands the
    // fish after the driver's tail drew it).
    assert!(
        host.world
            .minigames
            .fishing
            .as_ref()
            .unwrap()
            .catch_hud()
            .visible
    );

    // The close-up: the hooked body at its lifted world point, climbing.
    let mut lift_ys = Vec::new();
    let mut plate = None;
    let mut flash_from = None;
    let (mut sprites, mut locked, mut lit) = (0, false, 0);
    for f in 0..2000 {
        frame(&mut host, &mut surface, 0);
        let s = host.world.minigames.fishing.as_ref().unwrap();
        assert!(!s.catch_hud().visible, "state 0x1E clears the catch HUD");
        // The white-out (`FUN_801D7964` twice at the lift's second stage)
        // draws as additive full-screen quads over the HUD sprites.
        let flash = host.world.minigames.fishing_venue.catch_flash_prims();
        if flash_from.is_none() && !flash.is_empty() {
            flash_from = Some(f);
        }
        // The close-up's own parts (`FUN_801D4948` sub-state 4): the
        // sprite-arm halo and flare from its first frame, the camera-locked
        // rainbow rings from its first stage, all drawn out of the pond VRAM.
        let venue = &host.world.minigames.fishing_venue;
        if let Some(scene) = venue.ripple_fx.as_ref() {
            sprites = sprites.max(scene.sprite_arm_draws().len());
            locked |= venue.ripple_draws().iter().any(|d| d.flags_52 & 0x400 != 0);
        }
        if f == 0x80 {
            let vram = surface.vram().expect("the pond VRAM");
            let words: Vec<u16> = (0..512)
                .flat_map(|y| (0..1024).map(move |x| (x, y)))
                .map(|(x, y)| vram.pixel(x, y))
                .collect();
            lit = venue
                .ripple_rgba(&words, 320, 240)
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] != 0 && (p[0] | p[1] | p[2]) != 0)
                .count();
        }
        let scene = surface.scene().unwrap();
        let Some(fish) = scene.catch_fish else {
            continue;
        };
        let (b, n) = scene.catch_spans[fish.species].unwrap();
        let body = &scene.positions[b..b + n];
        assert!(!collapsed(body), "the landed species' body is posed");
        for (s, span) in scene.catch_spans.iter().enumerate() {
            if s != fish.species {
                let (b, n) = span.unwrap();
                assert!(collapsed(&scene.positions[b..b + n]), "one body at a time");
            }
        }
        match fish.place {
            CatchFishPlace::World(p) => {
                lift_ys.push(p[1]);
                assert!(scene.catch_line.is_none(), "no plate line in the close-up");
            }
            CatchFishPlace::CameraLocked { y, .. } => {
                plate = Some((fish, y));
                if y == -i32::from(host.world.minigames.fishing_catch_poses[fish.species].hang) {
                    break;
                }
            }
        }
    }
    assert!(
        lift_ys.len() > 100,
        "the close-up lifts the body: {}",
        lift_ys.len()
    );
    assert!(
        lift_ys.iter().min().unwrap() < &-2000,
        "flung well up out of the water (Y-down): {:?}",
        lift_ys.iter().min()
    );
    assert_eq!(sprites, 8, "the close-up seats its eight sprite parts");
    assert!(locked, "the first stage bursts camera-locked rings");
    assert!(
        lit > 1000,
        "the parts draw out of the pond VRAM: {lit} pixels"
    );
    let flash_from = flash_from.expect("the close-up whites out");
    assert!(
        (0x86..=0x8A).contains(&flash_from),
        "the white-out starts at the lift's second stage: frame {flash_from}"
    );
    let (fish, y) = plate.expect("the plate shows the body");
    let scene = surface.scene().unwrap();
    let ((tx, ty), (bx, by)) = scene.catch_line.expect("the plate hangs it from a line");
    assert_eq!(ty, 0, "the line starts at the top of the frame");
    assert_eq!(tx, bx, "the line hangs straight down");
    assert!((150..=170).contains(&bx), "centred: {bx}");
    eprintln!(
        "[ran] species {} scale {:#x}: lifted {} frames to y {:?}, settled y {y}, tie ({bx}, {by})",
        fish.species,
        fish.scale,
        lift_ys.len(),
        lift_ys.iter().min()
    );
}

/// The retail film's species-5 fish at rest on the plate: the line ties at
/// `(159, 92)` (`DAT_801D9154`, read every vsync of state `0x22`).
#[test]
fn the_plate_line_ties_where_the_retail_film_reads() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let host = enter(&disc);
    let scene = legaia_engine_scene::scene::Scene::load(&host.index, FISHING_VENUE_SCENE).unwrap();
    let anm = legaia_engine_scene::npc_catalog::scene_anm_bundle(&scene);
    let bodies = CatchBodies::load(&scene, anm.as_ref());
    let poses = &host.world.minigames.fishing_catch_poses;
    let pose = poses[5];
    let fish = CatchFish {
        species: 5,
        scale: 0x52B,
        place: CatchFishPlace::CameraLocked {
            y: plate_fish_y(0x1000, pose.hang),
            rot: pose.rot,
        },
    };
    // The showcase camera of venue 1 held (state `0x22`); a camera-locked
    // body does not depend on it, which this also checks.
    let mut cam = legaia_engine_fishing::fishing_actors::VenueCamera::default();
    cam.showcase(1);
    let view = legaia_engine_minigame_scenes::fishing_venue::venue_camera_globals_view(&cam);
    let body = bodies.body(5).expect("species 5's body");
    let mut ties = Vec::new();
    for frame in 0..body.frames.len().max(1) {
        let (_, tie) = plate_line(body, &pose, &fish, frame, &view).expect("a plate line");
        ties.push(tie);
    }
    eprintln!("[ran] species-5 ties over the clip: {ties:?}");
    for &(x, y) in &ties {
        assert!((x - 159).abs() <= 2, "tie x {x} vs retail 159");
        assert!((y - 92).abs() <= 3, "tie y {y} vs retail 92");
    }
}

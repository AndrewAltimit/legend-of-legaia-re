//! The fishing result actor's draw `FUN_801D67BC` (PROT 0972) on its
//! camera-locked path, up to its model matrix: the base block `0x8007BF10`
//! turned by the actor's angles, the lock arm of `FUN_8001CF50` (`+0x52 &
//! 0x400`) and the render scale `+0x72`, read off the scratchpad matrix
//! `0x1F8002D4` where the part loop begins (`0x801D6940`). Held against
//! `camera_locked_linear`, which keeps it over the base scale. The part
//! loop itself is skipped: the actor's clip names a part count its model
//! does not have.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing_actors::camera_locked_linear;
use legaia_minigame_formats::fishing_species as fs;
use psx_retail_exec::Stop;
use std::cell::RefCell;
use std::rc::Rc;

const DRAW: u32 = 0x801D_67BC;
const PART_LOOP: u32 = 0x801D_6940;
const BASE_BLOCK: u32 = 0x8007_BF10;
const MATRIX: u32 = 0x1F80_02D4;
const ACTOR: u32 = 0x8014_0000;
const MODEL: u32 = 0x8014_1000;
const CLIP: u32 = 0x8014_2000;

#[test]
fn the_plate_fish_matrix_is_retails() {
    differential!(prot(972, DRAW));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    // The runtime pointers to the SCUS cosine / sine tables RotMatrix reads.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    // The base block as the venue leaves it for the plate: `6 * I`, no
    // translation.
    for k in 0..8u32 {
        cpu.mem.wr32(BASE_BLOCK + 4 * k, 0);
    }
    for k in [0u32, 4, 8] {
        cpu.mem.wr16(BASE_BLOCK + 2 * k, 0x6000);
    }
    let seen = Rc::new(RefCell::new(None::<[[i16; 3]; 3]>));
    {
        let seen = seen.clone();
        cpu.probe(PART_LOOP, move |_, m, _| {
            *seen.borrow_mut() = Some(std::array::from_fn(|r| {
                std::array::from_fn(|c| m.rd16(MATRIX + 2 * (3 * r + c) as u32) as i16)
            }));
        });
    }
    let mut rng = Rng(0x504C_4154);
    let mut saturated = 0usize;
    let mut worst = 0f64;
    for round in 0..4000 {
        // Axis-aligned poses (every disc pose is one) and arbitrary ones.
        let rot: [i16; 3] = if round % 2 == 0 {
            std::array::from_fn(|_| (rng.below(4) * 0x400) as i16)
        } else {
            std::array::from_fn(|_| rng.below(0x1000) as i16)
        };
        let scale = match round % 3 {
            0 => 0x1000,
            1 => 0x800 + rng.below(0x1000) as u16,
            _ => 1 + rng.below(0x3000) as u16,
        };
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0);
        }
        cpu.mem.wr16(ACTOR + 0x24, rot[0] as u16);
        cpu.mem.wr16(ACTOR + 0x26, rot[1] as u16);
        cpu.mem.wr16(ACTOR + 0x28, rot[2] as u16);
        cpu.mem.wr16(ACTOR + 0x52, 0x400);
        // A depth-biased actor (`+0x10 & 0x80800000`) draws 0x14 OT slots
        // nearer (`0x801D6818..0x801D6820`) and restores the bias after
        // its part loop: the matrix is the same either way.
        if round % 4 == 3 {
            cpu.mem
                .wr32(ACTOR + 0x10, [0x0080_0000, 0x8000_0000][(round / 4) % 2]);
        }
        cpu.mem.wr16(ACTOR + 0x72, scale);
        cpu.mem.wr32(ACTOR + 0x44, MODEL);
        cpu.mem.wr32(ACTOR + 0x4C, CLIP);
        cpu.mem.wr32(MODEL, 1);
        cpu.mem.wr8(CLIP, 2);
        *seen.borrow_mut() = None;
        assert_eq!(cpu.call(DRAW, &[ACTOR]), Stop::Returned, "round {round}");
        let got = seen.borrow().expect("the draw reaches its part loop");
        let want = camera_locked_linear(rot, scale);
        let ctx = format!("round {round} rot {rot:?} scale {scale:#x}");
        for r in 0..3 {
            for c in 0..3 {
                let port = f64::from(want[r][c]) * 6.0 * 4096.0;
                let retail = f64::from(got[r][c]);
                // An axis-aligned pose is exact. Off the axes the port works
                // in reals where retail reads the SCUS sine table and
                // truncates each of the chain's GTE products: up to sixteen units
                // on the `6 * N` stage, times the lock arm's `6` and the
                // scale.
                let tol = if round % 2 == 0 {
                    1.0
                } else {
                    2.0 + 96.0 * f64::from(scale) / 4096.0
                };
                // Past `0x1000` the halfword wraps, so a near miss can land
                // on either side of it.
                let off = (port - retail).rem_euclid(65536.0);
                let off = off.min(65536.0 - off);
                worst = worst.max(off);
                assert!(
                    off <= tol,
                    "[{r}][{c}] retail {retail} port {port:.1}: {ctx} {got:?}"
                );
                saturated += usize::from(scale == 0x1000 && got[r][c].unsigned_abs() == 0x7FFF);
            }
        }
    }
    // A zero render scale skips the draw whole (`beqz` on `+0x72` at
    // `0x801D67F8`); the port's matrix for it is zero, which collapses the
    // body onto its origin.
    for round in 0..64 {
        let rot: [i16; 3] = std::array::from_fn(|_| rng.below(0x1000) as i16);
        cpu.mem.wr16(ACTOR + 0x24, rot[0] as u16);
        cpu.mem.wr16(ACTOR + 0x26, rot[1] as u16);
        cpu.mem.wr16(ACTOR + 0x28, rot[2] as u16);
        cpu.mem.wr16(ACTOR + 0x72, 0);
        *seen.borrow_mut() = None;
        assert_eq!(cpu.call(DRAW, &[ACTOR]), Stop::Returned, "zero {round}");
        assert!(seen.borrow().is_none(), "a zero scale draws nothing");
        assert_eq!(camera_locked_linear(rot, 0), [[0.0; 3]; 3]);
    }
    eprintln!("worst {worst}, saturated {saturated}");
    assert!(saturated > 1000, "saturated elements {saturated}");
}

/// The result actor's draw on the retail catch-plate state
/// (`minigame_fishing_retail_catch_plate`: the landed species-5 fish at
/// render scale `0x52B` hanging on the plate's line): the real species model
/// at `+0x44` and clip at `+0x4C`, as the result actor's tick seats them, run
/// whole through the part loop (`0x801D6970..0x801D6B34`) - the per-part pose
/// `FUN_8001BE80`, the tie part's angles, the tie vertex's projection into
/// `DAT_801D9154` and the draw `FUN_80043390` - plain and depth-biased.
/// Against `plate_line` over the same body, pose and showcase camera: the
/// port poses in reals, so the tie lands within a pixel or two of retail's
/// integer chain, on the clip frame nearest it.
#[test]
fn the_plate_line_ties_where_retails_draw_projects_it() {
    use legaia_engine_fishing::fishing_actors::{
        CatchFish, CatchFishPlace, VenueCamera, plate_fish_y,
    };
    use legaia_engine_minigame_scenes::fishing_catch::{CatchBodies, plate_line};
    const TIE: u32 = 0x801D_9154;
    let Some(state) = crate::common::library_state(
        "1bb2252da0fb4b8314efb9ea48a90a395f73b1c66c090842a63b82e40eec278c",
    ) else {
        return;
    };
    // Declared past the skip: without the library state nothing runs.
    differential!(prot(972, DRAW));
    assert!(
        crate::common::resident_is_retail(&state, DRAW, Some((972, fs::FISHING_OVERLAY_BASE_VA))),
        "the state's fishing overlay is the disc's"
    );
    // The result actor: render scale `0x52B`, the spawn's `0x420` in `+0x52`.
    let actors: Vec<u32> = (0..0x20_0000usize - 0x100)
        .step_by(4)
        .filter(|&o| {
            let h = |k: usize| u16::from_le_bytes([state.ram[o + k], state.ram[o + k + 1]]);
            h(0x72) == 0x52B && h(0x52) & 0x420 == 0x420 && state.ram[o + 0x47] == 0x80
        })
        .map(|o| 0x8000_0000 + o as u32)
        .collect();
    assert_eq!(actors.len(), 1, "{actors:x?}");
    let actor = actors[0];
    let held = state.u32_at(TIE);

    // The port's body, pose and camera for the same fish.
    let index = crate::walk_on::index();
    let scene = legaia_engine_scene::scene::Scene::load(
        &index,
        legaia_engine_minigame_scenes::fishing_scene::FISHING_VENUE_SCENE,
    )
    .expect("the venue");
    let anm = legaia_engine_scene::npc_catalog::scene_anm_bundle(&scene);
    let bodies = CatchBodies::load(&scene, anm.as_ref());
    let overlay = crate::common::disc()
        .expect("disc")
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    let poses = fs::parse_catch_poses(&overlay).expect("the showcase poses");
    let pose = poses[5];
    let fish = CatchFish {
        species: 5,
        scale: 0x52B,
        place: CatchFishPlace::CameraLocked {
            y: plate_fish_y(0x1000, pose.hang),
            rot: pose.rot,
        },
    };
    let mut cam = VenueCamera::default();
    cam.showcase(1);
    let view = legaia_engine_minigame_scenes::fishing_venue::venue_camera_globals_view(&cam);
    let body = bodies.body(5).expect("species 5's body");
    let ties: Vec<(i16, i16)> = (0..body.frames.len().max(1))
        .map(|f| {
            plate_line(body, &pose, &fish, f, &view)
                .expect("a plate line")
                .1
        })
        .collect();

    for bias in [0u32, 0x0080_0000, 0x8000_0000] {
        let mut cpu = state.cpu();
        cpu.step_budget = 5_000_000;
        let flags = cpu.mem.rd32(actor + 0x10);
        cpu.mem.wr32(actor + 0x10, (flags & !0x8080_0000) | bias);
        cpu.mem.wr32(TIE, 0);
        assert_eq!(cpu.call(DRAW, &[actor]), Stop::Returned, "bias {bias:#x}");
        let tie = cpu.mem.rd32(TIE);
        assert_eq!(tie, held, "bias {bias:#x}: the tie the state holds");
        // The depth bias is restored after the part loop.
        assert_eq!(
            cpu.mem.rd32(actor + 0x10) & 0x8080_0000,
            bias,
            "bias {bias:#x}"
        );
        let (x, y) = (tie as u16 as i16, (tie >> 16) as u16 as i16);
        let near = ties
            .iter()
            .map(|&(px, py)| (px - x).abs().max((py - y).abs()))
            .min()
            .unwrap();
        assert!(near <= 2, "retail tie ({x}, {y}) port ties {ties:?}");
    }
}

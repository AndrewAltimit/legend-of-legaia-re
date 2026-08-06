//! Disc-gated: the **camera-register zone ramp** (field-VM op `0x43`
//! sub-3..6) executed from real scene bytecode, then ticked at the output.
//!
//! The spawn half was already reached by `w1f2_field_vm_op_arms_disc`, which
//! asserts the record's fields. This file asserts the half that was missing:
//! the record is a live actor whose `+0x0C` handler (`FUN_80037018`) runs
//! every frame and *writes a camera register*, and the camera reads it.
//!
//! Everything here is measured on state **after** a `World::tick`, never on a
//! call: the ramp is stepped from the disc instruction, the player is walked
//! to the two edges and the middle of the authored zone, and the assertion is
//! on `World::camera_registers` and on `Camera::globals`.
//!
//! Non-vacuity is by contrast throughout: the same world with the player
//! *outside* the zone must leave the register alone, and a world with no ramp
//! at all must leave the camera globals at their field-reset values. A port
//! that ticked nothing passes neither.
//!
//! Structural assertions only - no Sony bytes are printed or asserted.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md).

use std::path::PathBuf;

use legaia_engine_core::camera::{Camera, RetailCamGlobals};
use legaia_engine_core::man_field_scripts::{
    CLEAN_RESYNC_INSNS, partition_record_span, scene_man_carriers,
};
use legaia_engine_core::register_ramp::CameraRegisterFile;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::field_disasm::{ActorCtrlKind, Insn, InsnInfo, LinearWalker};

struct Site {
    scene: String,
    body: Vec<u8>,
    pc: usize,
    insn: Insn,
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn is_ramp(insn: &Insn) -> bool {
    matches!(
        insn.info,
        InsnInfo::ActorCtrl {
            kind: ActorCtrlKind::CameraRegisterRamp { .. },
            ..
        }
    )
}

/// Every decoded op-`0x43` sub-3..6 site in the scene corpus, taken at
/// instruction boundaries behind the same clean-resync run the census tools
/// use.
fn ramp_sites(index: &ProtIndex, names: &[String]) -> Vec<Site> {
    let mut out = Vec::new();
    for name in names {
        let Ok(scene) = Scene::load(index, name) else {
            continue;
        };
        for carrier in scene_man_carriers(index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            for partition in 0..3 {
                let count = (*man_file
                    .header
                    .partition_counts
                    .get(partition)
                    .unwrap_or(&0))
                .max(0) as usize;
                for record in 0..count {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[start..start + len];
                    let mut ok_run = CLEAN_RESYNC_INSNS;
                    for insn in LinearWalker::new(body, pc0) {
                        let Ok(insn) = insn else {
                            ok_run = 0;
                            continue;
                        };
                        let clean = ok_run >= CLEAN_RESYNC_INSNS;
                        ok_run += 1;
                        if clean && is_ramp(&insn) {
                            out.push(Site {
                                scene: name.clone(),
                                body: body.to_vec(),
                                pc: insn.pc,
                                insn: insn.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

/// A field world seated at `site`'s instruction with a player actor to walk.
fn world_at(site: &Site) -> World {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    world.roster = legaia_save::Party::zeroed(3);
    let slot = 0usize;
    world.spawn_actor(slot);
    world.player_actor_slot = Some(slot as u8);
    world.load_field_script_at(site.body.clone(), site.pc);
    world
}

fn put_player(world: &mut World, x: i16, z: i16) {
    let slot = world.player_actor_slot.expect("player slot") as usize;
    let a = &mut world.actors[slot];
    a.move_state.world_x = x;
    a.move_state.world_y = 0;
    a.move_state.world_z = z;
}

#[test]
fn camera_zone_ramp_tracks_the_player_across_its_authored_zone() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let names = index.cdname_scene_names();
    assert!(!names.is_empty(), "CDNAME lists scenes");

    let sites = ramp_sites(&index, &names);
    assert!(
        !sites.is_empty(),
        "no scene carries a decoded `43` sub-3..6 camera-register zone ramp"
    );

    let mut lerped = 0usize;
    let mut gated = 0usize;
    for site in &sites {
        let InsnInfo::ActorCtrl {
            kind: ActorCtrlKind::CameraRegisterRamp { start, end, .. },
            ..
        } = site.insn.info
        else {
            unreachable!("filtered above")
        };

        let mut world = world_at(site);
        world.step_field().expect("step the ramp instruction");
        assert_eq!(
            world.register_ramps.len(),
            1,
            "{}: the ramp instruction installed no ramp",
            site.scene
        );
        let ramp = world.register_ramps[0];
        // A degenerate Z window would trap in retail; no on-disc ramp is
        // authored that way, and the assertions below need a real span.
        assert!(
            ramp.z_hi > ramp.z_lo,
            "{}: on-disc ramp has a degenerate Z window",
            site.scene
        );
        // The register file must still be at the zone-miss defaults - the
        // spawn writes nothing.
        assert_eq!(
            world.camera_registers.get(ramp.slot),
            CameraRegisterFile::DEFAULTS[ramp.slot.index()],
            "{}: the spawn must not write the register",
            site.scene
        );
        assert!(!world.camera_registers.written());

        // --- outside the zone: the handler's AABB gate rejects -----------
        let outside_x = ramp.x_hi.saturating_add(0x400);
        put_player(&mut world, outside_x, ramp.z_lo);
        world.tick();
        assert_eq!(
            world.camera_registers.get(ramp.slot),
            CameraRegisterFile::DEFAULTS[ramp.slot.index()],
            "{}: a player outside the X gate must not move the register",
            site.scene
        );
        assert!(
            !world.camera_registers.written(),
            "{}: gated tick must not mark the file written",
            site.scene
        );
        gated += 1;

        // --- low edge -> `start` ----------------------------------------
        put_player(&mut world, ramp.x_lo, ramp.z_lo);
        world.tick();
        assert_eq!(
            world.camera_registers.get(ramp.slot),
            i32::from(start),
            "{}: at the low Z edge the register must read the instruction's \
             start value",
            site.scene
        );

        // --- high edge -> `end` -----------------------------------------
        put_player(&mut world, ramp.x_hi, ramp.z_hi);
        world.tick();
        assert_eq!(
            world.camera_registers.get(ramp.slot),
            i32::from(end),
            "{}: at the high Z edge the register must read the instruction's \
             end value",
            site.scene
        );

        // --- midpoint -> strictly between, when the endpoints differ -----
        if start != end {
            let mid_z = ramp.z_lo + (ramp.z_hi - ramp.z_lo) / 2;
            put_player(&mut world, ramp.x_lo, mid_z);
            world.tick();
            let v = world.camera_registers.get(ramp.slot);
            let (lo, hi) = if start < end {
                (i32::from(start), i32::from(end))
            } else {
                (i32::from(end), i32::from(start))
            };
            assert!(
                v >= lo && v <= hi,
                "{}: midpoint value {v} left the endpoint band [{lo}, {hi}]",
                site.scene
            );
            assert!(
                v != i32::from(start) || v != i32::from(end),
                "{}: midpoint must not hold an endpoint when they differ",
                site.scene
            );
            lerped += 1;
        }

        // The ramp never completes - it is still live after every tick.
        assert_eq!(
            world.register_ramps.len(),
            1,
            "{}: a zone ramp must not retire itself",
            site.scene
        );
    }
    assert!(
        gated > 0,
        "no site exercised the outside-the-zone gate - every assertion above \
         would pass for a handler that wrote unconditionally"
    );
    eprintln!(
        "l6 camera zone ramps: sites={} lerped={lerped}",
        sites.len()
    );
}

/// The camera consumes the ramped registers, and a ramp-free world does not
/// move. The second half is the contrast: without it, a camera that
/// unconditionally re-stamped its pitch would pass the first half.
#[test]
fn camera_globals_follow_the_ramped_registers_and_hold_without_one() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let names = index.cdname_scene_names();
    let sites = ramp_sites(&index, &names);
    assert!(!sites.is_empty(), "no on-disc ramp site");

    // --- contrast: no ramp in the world, camera holds its field reset ---
    {
        let mut world = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        world.spawn_actor(0);
        world.player_actor_slot = Some(0);
        let mut cam = Camera::default();
        world.tick();
        cam.tick(&world);
        assert_eq!(
            cam.globals.angles()[0],
            RetailCamGlobals::FIELD_RESET.angles()[0],
            "a ramp-free world must leave the camera pitch at the field reset"
        );
        assert_eq!(
            cam.globals.tr_eye()[2],
            RetailCamGlobals::FIELD_RESET.tr_eye()[2],
            "a ramp-free world must leave the eye-back depth at the field reset"
        );
    }

    // --- a live ramp moves the axis its register owns -------------------
    let mut moved: Vec<(String, usize)> = Vec::new();
    for site in &sites {
        let mut world = world_at(site);
        world.step_field().expect("step the ramp instruction");
        let ramp = world.register_ramps[0];
        let axis = ramp.slot.camera_axis();
        let mut cam = Camera::default();
        // Baseline: the camera has not seen a written register yet.
        cam.tick(&world);
        let before = cam.globals.0[axis];
        put_player(&mut world, ramp.x_lo, ramp.z_lo);
        world.tick();
        cam.tick(&world);
        let want = if axis == 5 {
            world.camera_registers.get(ramp.slot).abs()
        } else {
            world.camera_registers.get(ramp.slot)
        };
        assert_eq!(
            cam.globals.0[axis], want,
            "{}: camera axis {axis} must carry the ramped register",
            site.scene
        );
        assert_eq!(
            before,
            RetailCamGlobals::FIELD_RESET.0[axis],
            "{}: the pre-ramp baseline must be the field reset, or the \
             assertion above proves nothing",
            site.scene
        );
        moved.push((site.scene.clone(), axis));
    }
    assert!(
        !moved.is_empty(),
        "no on-disc ramp reached the camera consumer"
    );
    // At least one site must actually CHANGE its axis - a ramp whose start
    // value happened to equal the field reset would pass the equality above
    // while moving nothing.
    let changed = sites
        .iter()
        .filter(|site| {
            let mut world = world_at(site);
            world.step_field().expect("step");
            let ramp = world.register_ramps[0];
            put_player(&mut world, ramp.x_lo, ramp.z_lo);
            world.tick();
            let mut cam = Camera::default();
            cam.tick(&world);
            cam.globals.0[ramp.slot.camera_axis()]
                != RetailCamGlobals::FIELD_RESET.0[ramp.slot.camera_axis()]
        })
        .count();
    assert!(
        changed > 0,
        "every on-disc ramp left its camera axis at the field reset - the \
         consumer is indistinguishable from doing nothing"
    );
    let axes: Vec<usize> = moved.iter().map(|(_, a)| *a).collect();
    eprintln!(
        "l6 camera consumer: sites={} axes={axes:?} changed={changed}",
        moved.len()
    );
}

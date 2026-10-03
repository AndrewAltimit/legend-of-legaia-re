//! Disc-gated: the follow camera frames the floor ladder `concnow`'s entry
//! script installs, not the one its MAN header shipped.
//!
//! Field-VM `4C 9E` installs all sixteen rungs of the floor-height ladder.
//! Its arm (`0x801E24F8..0x801E2538` in `FUN_801DE840`) stores each word
//! twice in one loop: negated into the live rungs at scratchpad
//! `0x1F80035C`, and raw into the MAN-header ladder at `*(_DAT_8007B898) + 2`,
//! the copy the camera composer `FUN_801DAB90` swaps in around its floor
//! sample. The port wrote only the live rungs, so in `concnow` the player
//! walked a floor about a thousand units above the one the camera composed
//! against, and the eye sank under the raised ground.
//!
//! The oscillators (`4C 90`) still move the live rungs alone, so the two
//! samples may differ by a bob's amplitude - never by a whole tier.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::camera::Camera;
use legaia_engine_core::frame_step::{camera_after_world_tick, camera_before_world_tick};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn concnow_camera_composes_against_the_installed_ladder() {
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
    host.enter_field_scene("concnow", 0)
        .expect("enter_field_scene('concnow')");
    host.world.locomotion.follow_terrain_height = true;
    let shipped = host.world.terrain.floor_height_lut_static;
    let mut cam = Camera::default();
    cam.reset_globals_for_scene_entry();

    let mut worst = 0i32;
    let mut frames = 0usize;
    for (pad, n) in [
        (0u16, 200),
        (PadButton::Up.mask(), 150),
        (PadButton::Left.mask(), 150),
        (PadButton::Down.mask(), 300),
        (PadButton::Right.mask(), 300),
    ] {
        for _ in 0..n {
            camera_before_world_tick(&mut cam, &mut host.world, None);
            host.world.set_pad(pad);
            let _ = host.world.tick();
            camera_after_world_tick(&mut cam, &mut host.world, false);
            let slot = host.world.player_actor_slot.unwrap_or(0) as usize;
            let ms = &host.world.actors[slot].move_state;
            let (x, z) = (i32::from(ms.world_x), i32::from(ms.world_z));
            let live = host.world.sample_field_floor_height(x, z);
            let composed = host.world.sample_field_floor_height_static(x, z);
            worst = worst.max((live - composed).abs());
            frames += 1;
        }
    }
    eprintln!("[ran] concnow: {frames} frames, worst live-vs-composed floor gap {worst}");
    assert_ne!(
        host.world.terrain.floor_height_lut_static, shipped,
        "concnow's entry script installs a ladder of its own (4C 9E)"
    );
    // A bob is tens of units; the stale-ladder defect was ~1000.
    assert!(
        worst <= 0x40,
        "the camera's floor sample must follow the installed ladder: gap {worst}"
    );
}

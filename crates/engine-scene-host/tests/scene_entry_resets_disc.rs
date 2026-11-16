//! Per-scene camera state a scene change must not carry.
//!
//! Ratayu (`retockin`) is the disc's one field scene whose entry script
//! spawns a camera-register zone ramp (`P1[0]`: `43 03`, the GTE `H` register
//! `0x8007B618` over tiles `14..19` x `90..95`). The MAN loader's second
//! retire sweep marks every `FUN_80037018` node (`0x8003B414..0x8003B43C`), so
//! the ramp ends at the door, and the next scene's zone query rewrites the
//! four registers it fed. The same scene authors op `4C 49`, the camera
//! vertical offset in the scene control block's `+0x4A`, which the block's
//! re-allocation zeroes (`FUN_8003A024`, `0x8003A0D4`); the field initialiser
//! re-seeds the offset accumulator `_DAT_8007BCAC` to `0x3C` (`FUN_801D6704`,
//! `0x801D67B8`).
//!
//! `town01` authors neither op, so whatever it holds after the entry is
//! what the scene change left.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or
//! `extracted/` missing.

use std::path::PathBuf;

use legaia_engine_field::camera_ease::CAMERA_OFFSET_EASE_SEED;
use legaia_engine_field::register_ramp::CameraRegisterFile;
use legaia_engine_scene::scene::DefaultMapIdResolver;
use legaia_engine_scene_host::scene_host::SceneHost;

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())?;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    Some(host)
}

#[test]
fn ratayu_camera_ramps_and_offset_end_at_the_door() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene("retockin", 0)
        .expect("enter retockin");
    let mut armed = None;
    for t in 0..600 {
        let _ = host.tick();
        if !host.world.camera.register_ramps.is_empty() {
            armed = Some(t);
            break;
        }
    }
    assert!(
        armed.is_some(),
        "retockin's P1[0] spawns its projection ramp"
    );
    // Stand in for a ramp write and an authored offset the walk would make.
    host.world.camera.registers.set(
        legaia_engine_field::register_ramp::RampSlot::Dat8007B618,
        0xA0,
    );
    host.world.camera.scene_offset = 0x120;
    host.world.camera.offset_ease = 0x200;

    host.enter_field_scene("town01", 0).expect("enter town01");
    assert!(
        host.world.camera.register_ramps.is_empty(),
        "the MAN load retires every FUN_80037018 node"
    );
    assert_eq!(host.world.camera.registers, CameraRegisterFile::default());
    assert_eq!(host.world.camera.scene_offset, 0, "+0x4A is re-zeroed");
    assert_eq!(host.world.camera.offset_ease, CAMERA_OFFSET_EASE_SEED);
}

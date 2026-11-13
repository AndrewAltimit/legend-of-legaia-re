//! Disc-gated: the retail comparison's sprite-arm seat
//! (`World::install_sprite_arm_snapshot`) spawns a sheet for a record the
//! engine holds none of. `kor`'s steam emitter (stager record `0xEEC`) picks
//! each sheet's record at random, so a captured frame's sheets
//! (`kor_field_route_walk` draws `0x12F4` and `0x124E`) need not be live in
//! the engine when the seat runs.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_core::world::ambient::SpriteArmSeed;
use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

#[test]
fn kor_seat_spawns_a_sheet_of_a_record_with_none_live() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("open");
    host.enter_field_scene("kor", 0).expect("enter kor");
    for _ in 0..200 {
        host.tick().expect("tick");
        let _ = host.step_headless_frame_tail();
    }
    const RECORD: usize = 0x12F4;
    let sheet = |p: &legaia_engine_core::world::ambient::AmbientPart| {
        !p.finished && p.state.move_substate == 4 && p.state.field_9e & 0x4000 != 0
    };
    // Retire every live sheet of the record: the state the seat can meet.
    for p in host.world.ambient.fx.iter_mut() {
        if p.record_off == RECORD && sheet(p) {
            p.finished = true;
        }
    }
    host.world.install_sprite_arm_snapshot(&[SpriteArmSeed {
        record_off: RECORD as u32,
        pos: [1936, -226, 3906],
        rot: [0; 3],
        scale: 0x1000,
        colour: 0xC900_0000,
        level: 0x1FF,
        pose: Vec::new(),
        shape: None,
    }]);
    let seated: Vec<_> = host
        .world
        .ambient
        .fx
        .iter()
        .filter(|p| p.record_off == RECORD && sheet(p))
        .collect();
    assert_eq!(seated.len(), 1, "one sheet of the record, spawned");
    let st = &seated[0].state;
    assert_eq!([st.world_x, st.world_y, st.world_z], [1936, -226, 3906]);
    assert_eq!(st.field_78, 0x1FF);
    // The first slice ran the record's op 0x23: the quad block is set.
    assert_eq!(st.actor_u16(0xB4), 512, "the sheet's width");
    assert_eq!(st.actor_u16(0xB0), 0x0007, "the fog texture page (448, 0)");
    assert_eq!(host.world.active_effect_kind4_draws().len(), 1);
}

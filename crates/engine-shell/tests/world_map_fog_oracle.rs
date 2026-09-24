//! Disc-gated: the fog pool runs on the kingdom overworld, through the
//! overworld arm of the spawner, and draws through the walk camera.
//!
//! Retail's overworld is a game-mode-3 field-run scene, so the field render
//! pass's fog stage (`0x80026EA4`: game mode `3` and the gate word
//! `_DAT_8007B854`) runs there too. The `keikoku_chest_preload` state
//! (`map01`, player at `(8266, 8700)`) holds the gate raised, `0x48` records
//! live (the raised cap every kingdom MAN seats) and every record at a height
//! in `-0x28 - 0x7F ..= -0x28` - the spawner's overworld arm
//! (`_DAT_1F800394 & 1`), which lifts each particle a further `0x28`.
//!
//! This seats the engine on the same scene and position and runs the world
//! and the render step through the resolved world-map frame each tick; it
//! asserts that `map01`'s entry script (`P1[0]`) raises the gate and sets the
//! overworld's visible-tile window, that the pool fills toward the raised cap
//! with records ahead of the player as well as behind, that every live
//! record carries the overworld lift, and that the quads carry retail's
//! packet shape.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use legaia_engine_core::camera_view::{FieldCameraFrame, resolve_field_camera};
use legaia_engine_core::fog_particles::{FOG_CAP_RAISED, FOG_CLUT, FOG_OVERWORLD_LIFT, FOG_TPAGE};
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gated() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })
}

#[test]
fn the_overworld_runs_the_fog_pool_through_its_own_arm() {
    let Some(extracted) = gated() else { return };
    let scene = "map01".to_string();
    let cfg = BootConfig {
        scene: scene.clone(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("boot session");
    let mode = session
        .enter_world_map_live(&scene, &FieldLiveOpts::default())
        .expect("enter map01");
    assert_eq!(mode, SceneMode::WorldMap);
    assert_eq!(
        session.host.world.fog.cap, FOG_CAP_RAISED,
        "every kingdom MAN seats the raised cap (MAN[1] bit 0)"
    );
    assert!(
        !session.host.world.fog.regions.is_empty(),
        "map01 carries a MAN section-4 fog-region table"
    );
    assert!(session.host.world.debug_seat_player(8266, 8700));
    session.camera.zone.arm_arrival();

    let mut max_live = 0u16;
    let mut max_quads = 0usize;
    let mut walk_frames = 0usize;
    let mut raised_at = None;
    let mut ahead = 0usize;
    let mut behind = 0usize;
    for f in 0..1500 {
        session.host.world.set_pad(0);
        let _ = session.tick();
        let frame = resolve_field_camera(&session.host.world, &session.camera, None, [0.0, 0.0]);
        if matches!(frame, FieldCameraFrame::WorldMapWalk { .. }) {
            walk_frames += 1;
        }
        let Some(view) = frame.field_view() else {
            panic!("no field-frame view for the overworld at tick {f}: {frame:?}");
        };
        let quads = session.host.world.fog_render_step(&view).to_vec();
        if session.host.world.fog.gate {
            raised_at.get_or_insert(f);
        }
        max_live = max_live.max(session.host.world.fog.live);
        max_quads = max_quads.max(quads.len());
        for q in &quads {
            assert_eq!(q.tpage, FOG_TPAGE);
            assert_eq!(q.clut, FOG_CLUT);
        }
        let pz = session.host.world.fog_player_world_pos()[2];
        for r in session.host.world.fog.records.iter().filter(|r| r.alive) {
            if f == 1499 {
                if (r.z >> 4) > pz {
                    ahead += 1;
                } else {
                    behind += 1;
                }
            }
            assert!(
                (-0x7F - FOG_OVERWORLD_LIFT..=-FOG_OVERWORLD_LIFT).contains(&r.y),
                "record height {} is not the overworld arm's (tick {f})",
                r.y
            );
        }
    }
    eprintln!(
        "[ok] map01 fog: gate raised by the entry script at tick {raised_at:?}; \
         window {:?}; peak live {max_live} (cap {FOG_CAP_RAISED}, retail keikoku state: 72 live); \
         peak quads/frame {max_quads}; walk frames {walk_frames}/1500; \
         last frame {ahead} ahead of the player / {behind} behind",
        session.host.world.fog.view_window
    );
    assert!(
        raised_at.is_some(),
        "map01's entry script (P1[0] `4C 30`) raises the gate on the overworld"
    );
    assert_eq!(
        session.host.world.fog.view_window,
        Some([-18, -12, 18, 32]),
        "map01's entry script (`46 24 EE F4 12 20`) sets retail's overworld window"
    );
    assert_eq!(
        walk_frames, 1500,
        "the walk camera owns every overworld frame"
    );
    assert!(
        max_live > legaia_engine_core::fog_particles::FOG_CAP_DEFAULT,
        "the pool never passed the field cap - the raised overworld cap is not in effect"
    );
    assert!(max_quads > 0, "the overworld pool drew nothing");
    // Retail's pool at this seat reaches 1075 units behind and 1337 ahead
    // of the player; the widened window is what puts records ahead.
    assert!(
        ahead > 0,
        "no fog record ahead of the player ({behind} behind)"
    );
}

//! Disc-gated field-state oracle: the engine's intra-town walk-touch warp
//! (`0xA3 0xF8 xb zb` player MOVE_TO) lands the player at the SAME world position
//! a real PCSX-Redux capture recorded.
//!
//! Ground truth is the `s4_rimelm_door_transition` anchor: the grid-BFS door-nav
//! walked Vahn out of his house and a walk-touch warp jumped him to world
//! `(3264, 3520)` = tile `(25, 27)` (see `docs/tooling/playthrough-coverage.md`).
//! This test (1) loads that anchor's main RAM through the [`legaia_pcsxr`] bridge
//! and confirms retail parks the player at `(3264, 3520)` in `town01` field mode,
//! then (2) finds the town01 MAN's `0xA3 0xF8` op whose target decodes to that
//! position and drives it through the real field VM, asserting the engine emits a
//! `MoveTo` to exactly `(3264, 3520)`. This is the missing player-position /
//! warp-end-state oracle (the existing field oracles cover the collision grid, not
//! the warp landing). Skips when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};

/// The captured S4 warp end-state: world `(3264, 3520)` = tile `(25, 27)`.
const S4_LAND: (u16, u16) = (3264, 3520);
const S4_FINGERPRINT: &str = "a89f131f74811b56ef12146fcae0f49867f2a3307941a39c292bbd15831c890e";

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn library_save(fp: &str) -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let p = PathBuf::from(c)
            .join("pcsx-redux")
            .join(format!("{fp}.sstate"));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Field-VM grid byte -> world coordinate: `(b & 0x7F) * 0x80 + 0x40`, plus a
/// further `0x40` when the half-tile bit is set (retail `case 0x23`).
fn grid_to_world(b: u8) -> u16 {
    let base = u16::from(b & 0x7F) * 0x80 + 0x40;
    if b & 0x80 != 0 { base + 0x40 } else { base }
}

/// Drive the bytecode starting at `op_pc` (a `0xA3 0xF8 xb zb` warp) through the
/// real field VM and return the first `MoveTo` event's world coords.
fn warp_world_coords(decoded: &[u8], op_pc: usize) -> Option<(u16, u16)> {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_field_script(decoded[op_pc..].to_vec());
    let _ = world.tick();
    world.drain_field_events().into_iter().find_map(|ev| {
        if let FieldEvent::MoveTo {
            world_x, world_z, ..
        } = ev
        {
            Some((world_x, world_z))
        } else {
            None
        }
    })
}

#[test]
fn engine_walk_touch_warp_matches_s4_capture() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };

    // (1) Retail ground truth via the PCSX-Redux bridge: the S4 anchor parks the
    //     player at (3264,3520) in town01 field mode.
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: single-threaded test setup before any save load.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }
    let retail_land = match library_save(S4_FINGERPRINT) {
        Some(path) => {
            let st = legaia_pcsxr::SaveState::from_path(&path).expect("load s4 .sstate");
            assert_eq!(st.scene_name(), "town01", "S4 anchor scene");
            assert_eq!(st.game_mode(), 0x03, "S4 anchor field mode");
            let (x, z) = st.player_pos().expect("S4 player pos");
            eprintln!("[retail] S4 anchor: town01 mode 0x03, player at ({x}, {z})");
            assert_eq!((x as u16, z as u16), S4_LAND, "S4 captured warp end-state");
            Some(S4_LAND)
        }
        None => {
            eprintln!("[warn] S4 library save absent; asserting engine vs the documented constant");
            None
        }
    };

    // (2) Find the town01 MAN's `A3 F8` op whose target decodes to (3264,3520),
    //     and drive it through the field VM.
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("prot index"));
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man = scene
        .field_man_payload(&index)
        .expect("man payload")
        .expect("town01 MAN");

    let mut op_pc = None;
    let mut targets = Vec::new();
    let mut i = 0;
    while i + 3 < man.len() {
        if man[i] == 0xA3 && man[i + 1] == 0xF8 {
            let target = (grid_to_world(man[i + 2]), grid_to_world(man[i + 3]));
            targets.push((i, target));
            if target == S4_LAND {
                op_pc = Some(i);
            }
        }
        i += 1;
    }
    eprintln!(
        "[disc] town01 has {} A3 F8 player-warps; targeting {:?} at pc {:?}",
        targets.len(),
        S4_LAND,
        op_pc
    );
    let op_pc = op_pc.unwrap_or_else(|| {
        panic!(
            "no town01 0xA3 0xF8 warp targets the S4 end-state {S4_LAND:?}; targets: {targets:?}"
        )
    });

    let engine_land = warp_world_coords(&man, op_pc).expect("field VM emits a MoveTo for the warp");
    eprintln!("[engine] walk-touch warp at pc 0x{op_pc:X} -> {engine_land:?}");
    assert_eq!(
        engine_land, S4_LAND,
        "engine warp lands at the decoded target"
    );

    if let Some(retail_land) = retail_land {
        assert_eq!(
            engine_land, retail_land,
            "engine warp end-state matches the retail S4 capture"
        );
    }
}

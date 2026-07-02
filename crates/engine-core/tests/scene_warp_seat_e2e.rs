//! Disc-gated oracle for the **door-warp arrival seat**: a named scene
//! transition (field-VM op `0x3F`) carries the destination entry tile in its
//! trailing bytes, and `SceneHost::tick` must (1) load the destination scene
//! and report `SceneTickEvent::SceneEntered`, and (2) seat the player at that
//! tile's centre (`world = tile*128 + 0x40`, the same tile->world mapping the
//! MAN placement spawns use) instead of the cold-boot spawn
//! (`FIELD_COLD_SPAWN_XZ`), so the player stands at the door it arrived
//! through.
//!
//! A baseline pass first asserts a plain `enter_field_scene` DOES land on the
//! cold spawn, so the warp assertion can't pass vacuously (if the cold spawn
//! ever moved onto the entry tile the test would say so). Skips without
//! `extracted/` + `LEGAIA_DISC_BIN`.

use legaia_engine_core::scene::{SceneHost, SceneTickEvent};
use legaia_engine_core::world::FIELD_COLD_SPAWN_XZ;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    let d = PathBuf::from("extracted");
    if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
        Some(d)
    } else {
        let alt = PathBuf::from("../../extracted");
        if alt.join("PROT.DAT").exists() && alt.join("CDNAME.TXT").exists() {
            Some(alt)
        } else {
            None
        }
    }
}

#[test]
fn named_warp_seats_player_at_entry_tile() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town01", 0).expect("enter town01");

    // --- baseline: a cold field entry lands on the cold-boot spawn ---
    let pslot = host.world.player_actor_slot.expect("player installed") as usize;
    let ms = &host.world.actors[pslot].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        (FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ),
        "cold entry must land on the cold-boot spawn (non-vacuous baseline)"
    );

    // --- stage a named warp with an entry tile off the cold spawn ---
    // The same staging the field-VM 0x3F host hook writes; a field
    // destination first, then a world-map destination below (both paths
    // seat the arrival).
    const DEST: &str = "keikoku";
    const ENTRY_TILE: (u8, u8) = (10, 20);
    host.world.pending_named_scene_transition =
        Some((DEST.to_string(), ENTRY_TILE.0, ENTRY_TILE.1));
    let event = host.tick().expect("transition tick");
    match event {
        SceneTickEvent::SceneEntered { ref name } => assert_eq!(name, DEST),
        other => panic!("expected SceneEntered, got {other:?}"),
    }

    // --- the player stands on the entry tile's centre, not the cold spawn ---
    let pslot = host.world.player_actor_slot.expect("player re-installed") as usize;
    let ms = &host.world.actors[pslot].move_state;
    let expect = (
        i16::from(ENTRY_TILE.0) * 128 + 0x40,
        i16::from(ENTRY_TILE.1) * 128 + 0x40,
    );
    assert_eq!(
        (ms.world_x, ms.world_z),
        expect,
        "warp arrival must seat the player at the op-0x3F entry tile centre"
    );

    // --- a WORLD-MAP destination seats too (the town-exit-onto-overworld
    // case): Rim Elm's retail exit carries map01 entry tile (0x60, 0x19), so
    // the player must arrive on the Drake continent beside the town rather
    // than at the map origin (open, unwalkable ocean). ---
    const WM_ENTRY: (u8, u8) = (0x60, 0x19);
    host.world.pending_named_scene_transition = Some(("map01".to_string(), WM_ENTRY.0, WM_ENTRY.1));
    let event = host.tick().expect("world-map transition tick");
    match event {
        SceneTickEvent::SceneEntered { ref name } => assert_eq!(name, "map01"),
        other => panic!("expected SceneEntered(map01), got {other:?}"),
    }
    let pslot = host.world.player_actor_slot.expect("player kept") as usize;
    let ms = &host.world.actors[pslot].move_state;
    let expect = (
        i16::from(WM_ENTRY.0) * 128 + 0x40,
        i16::from(WM_ENTRY.1) * 128 + 0x40,
    );
    assert_eq!(
        (ms.world_x, ms.world_z),
        expect,
        "overworld warp arrival must seat the player at the destination entry tile"
    );
}

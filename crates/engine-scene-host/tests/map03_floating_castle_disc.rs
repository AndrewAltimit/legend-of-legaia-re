//! Disc-gated: an overworld placement whose flag word carries `0x20000000`
//! draws at `-(+0x8E)`, not on the floor.
//!
//! `map03`'s P1[1] is Zora's Floating Castle: its prologue raises the bit
//! (`31 1D`) and sets `+0x8E = 900` (`4C 42 84 03`), and its motion stream
//! drifts it across Karisto. Retail's actor driver `FUN_8003BC08` runs the
//! same height arm on the overworld as in a field (`0x8003BC50..0x8003BC64`:
//! `+0x16 = -(+0x8E)`); a retail capture on `map03` (PCSX-Redux library state
//! `35e64051...`) holds the castle actor (pool model 49) at `(8256, -900,
//! 6304)`. The port's world-map tick skipped the arm, so the castle snapped
//! to the ground under it. `map02`'s Karisto cart (P1[3..5], `+0x8E = 228`)
//! rides the same law.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// Enter `scene` the way the play hosts' scene picker does and run `frames`
/// host ticks.
fn overworld(root: &std::path::Path, scene: &str, frames: usize) -> SceneHost {
    let mut host = SceneHost::open_extracted(root).expect("open SceneHost");
    host.world.stage_picker_entry(scene, false);
    host.world.npcs.animate = true;
    host.enter_world_map_scene(scene).expect("enter overworld");
    for _ in 0..frames {
        host.tick().expect("tick");
    }
    host
}

#[test]
fn the_floating_castle_hovers_over_karisto_or_skip() {
    let Some(root) = extracted_root() else { return };
    let host = overworld(&root, "map03", 240);
    let castle = 1u8;
    let spawn = host.world.npcs.positions[&castle];
    let (flags, height) = host.world.field_channel_flags_and_8e(castle);
    assert_ne!(flags & 0x2000_0000, 0, "P1[1] raises the held-height bit");
    assert_eq!(height, 900, "P1[1] holds +0x8E at 900");
    let pose = host
        .world
        .field_npc_draw_pose(castle, spawn)
        .expect("the castle is drawn");
    eprintln!("[ran] castle draw pose {:?}", pose.pos);
    assert_eq!(pose.pos[1], -900.0, "retail holds the castle 900 units up");
}

#[test]
fn every_held_height_overworld_placement_leaves_the_floor_or_skip() {
    let Some(root) = extracted_root() else { return };
    let mut checked = 0;
    for scene in ["map01", "map02", "map03"] {
        let host = overworld(&root, scene, 240);
        let mut slots: Vec<_> = host
            .world
            .npcs
            .positions
            .iter()
            .map(|(&s, &p)| (s, p))
            .collect();
        slots.sort();
        for (slot, p) in slots {
            let (flags, height) = host.world.field_channel_flags_and_8e(slot);
            if flags & 0x2000_0000 == 0 {
                continue;
            }
            let Some(pose) = host.world.field_npc_draw_pose(slot, p) else {
                continue;
            };
            eprintln!("[ran] {scene} P1[{slot}] at {:?}, +0x8E {height}", pose.pos);
            assert_eq!(
                pose.pos[1],
                -f32::from(height),
                "{scene} P1[{slot}] draws at -(+0x8E)"
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "at least the castle is drawn");
}

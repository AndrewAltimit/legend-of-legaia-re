//! Disc-gated: field-NPC motion derives from real MAN placement scripts and
//! the motion VM walks the NPCs in the engine.
//!
//! Retail drives field-NPC walking through each placement's own field-VM
//! script: the per-actor record's `0x4C 0x51` NPC move-to-tile ops feed the
//! glide path whose per-frame stepper is the motion VM (`FUN_8003774C`,
//! started through the `FUN_800358c0`-shape target write). The engine mirrors
//! this with [`World::field_npc_routes`] (decoded by
//! `man_field_scripts::placement_motion_route` from the same pre-text script
//! region the interaction-prologue runner executes) driven through
//! [`legaia_engine_vm::motion_vm::step`] by `World::tick_field_npc_motions`,
//! writing live positions back into `field_npc_positions` so the ±40-unit
//! moving-actor collision box and the interact probe follow the walking NPC.
//!
//! Assertions are structural (slots, world coordinates, movement deltas) - no
//! Sony bytes. Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`
//! (CLAUDE.md convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::man_field_scripts::NPC_ROUTE_LOCALITY;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn town01_npc_motion_routes_walk_npcs_through_the_motion_vm() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man_bytes = scene
        .field_man_payload(&index)
        .expect("read MAN")
        .expect("town01 has a MAN payload");
    let man_file = parse_man(&man_bytes).expect("parse MAN");

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_carriers_from_man(&man_file, &man_bytes);

    // Route derivation is non-vacuous: several Rim Elm villagers carry local
    // `0x4C 0x51` walk legs in their placement scripts.
    assert!(
        world.field_npc_routes.len() >= 3,
        "town01 derives walk routes for several NPCs (got {})",
        world.field_npc_routes.len()
    );
    // Every derived waypoint is a local, on-map world position (the locality
    // gate that drops story-relocation branches held).
    let anchors = world.field_npc_positions.clone();
    for (slot, route) in &world.field_npc_routes {
        let &(ax, az) = anchors
            .get(slot)
            .expect("routed slots are installed NPC placements");
        for &(wx, wz) in route {
            assert!(
                (wx as i32 - ax as i32).abs() <= NPC_ROUTE_LOCALITY
                    && (wz as i32 - az as i32).abs() <= NPC_ROUTE_LOCALITY,
                "slot {slot}: waypoint ({wx},{wz}) within locality of anchor ({ax},{az})"
            );
        }
    }

    // Baseline: channel stepping is always-on (retail `FUN_80039B7C` steps
    // every context every frame), so even with the patrol flag off the
    // placement scripts run live in a raw-World install: one-shot spawn
    // prologue legs land (the SceneHost path consumes those in the entry
    // pre-run instead) and the authored CONTINUOUS walkers keep pacing
    // their local loops - exactly what retail towns do by default. The
    // flag-off contract is locality: nothing runs away from its scripted
    // seat.
    for _ in 0..600 {
        let _ = world.tick();
    }
    let settled = world.field_npc_positions.clone();
    for _ in 0..60 {
        let _ = world.tick();
    }
    for (slot, &(x, z)) in &world.field_npc_positions {
        let &(sx, sz) = settled.get(slot).expect("no slots appear mid-run");
        assert!(
            (x as i32 - sx as i32).abs() <= NPC_ROUTE_LOCALITY
                && (z as i32 - sz as i32).abs() <= NPC_ROUTE_LOCALITY,
            "flag off: slot {slot} roams locally (({x},{z}) vs settled ({sx},{sz}))"
        );
    }

    // Flag on: the motion VM walks at least one routed NPC off its settled
    // seat.
    world.animate_field_npcs = true;
    for _ in 0..120 {
        let _ = world.tick();
    }
    let moved: Vec<u8> = world
        .field_npc_routes
        .keys()
        .filter(|slot| world.field_npc_positions.get(slot) != settled.get(slot))
        .copied()
        .collect();
    assert!(
        !moved.is_empty(),
        "at least one routed town01 NPC walked off its anchor"
    );
    eprintln!(
        "[town01] {} routed NPCs, {} moved off-anchor after 120 ticks: {:?}",
        world.field_npc_routes.len(),
        moved.len(),
        moved
    );

    // Collision consistency: a moved NPC blocks at its LIVE position with the
    // ±40-unit moving-actor box, exactly like the anchored case - the probes
    // read `field_npc_positions`, which the motion tick keeps live.
    world.solid_field_npcs = true;
    let slot = moved[0];
    let &(lx, lz) = world.field_npc_positions.get(&slot).unwrap();
    assert!(
        world.field_actor_dir_blocked(lx - 102, lz, 3),
        "the moving NPC's collision box follows its live position"
    );
}

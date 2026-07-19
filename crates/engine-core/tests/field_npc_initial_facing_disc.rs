//! Disc-gated: never-walked field NPCs get their retail **initial facing**
//! from the MAN spawn prologue, replacing the unrotated default.
//!
//! Retail applies these at scene load: the placement installer `FUN_8003A1E4`
//! pre-runs each record's `0x24`/`0x25`-marked prologue through the field VM
//! (`FUN_801DE840`), and the prologue's `0x4C 0x51` (nibble-5 sub-1) /
//! `0x38` (simple-path) ops write the actor's `+0x26` heading from the
//! 8-direction LUT at SCUS `0x80073F04` (entry `i` = `i * 0x200` retail
//! units; retail `0` = Z-, pinned from `FUN_801d01b0`'s pad->facing writes).
//! The engine derives the same LUT index statically
//! (`man_field_scripts::placement_initial_facing`) and seeds
//! `World::field_npc_headings` (engine convention `0` = Z+, retail + 0x800)
//! via `World::seed_field_npc_facings`.
//!
//! Assertions are structural (facing-carrying prologues exist, every derived
//! heading lands on the 8-direction ladder, and one authored NPC pair faces
//! each other along X) - no Sony bytes. Skip-passes without
//! `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::man_field_scripts::{
    facing_index_to_engine_heading, placement_initial_facing,
};
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
fn town01_npc_initial_facings_derive_from_spawn_prologues() {
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

    // Non-vacuous: several Rim Elm villagers carry a facing-writing spawn
    // prologue, and every derived index is a real direction slot (0..=7 -
    // the SCUS LUT's direction half), so the heading lands on the 0x200
    // ladder rather than a synthetic value.
    let placements = man_file.actor_placements(&man_bytes);
    let mut derived = 0usize;
    for p in &placements {
        let Some(idx) = placement_initial_facing(&man_file, &man_bytes, p) else {
            continue;
        };
        derived += 1;
        assert!(
            idx <= 7,
            "placement {} facing index {idx} outside the authored 0..=7 range",
            p.index
        );
        let heading = facing_index_to_engine_heading(idx).expect("direction slot");
        assert_eq!(
            heading & 0x1FF,
            0,
            "placement {} heading {heading:#X} off the 0x200 ladder",
            p.index
        );
    }
    assert!(
        derived >= 4,
        "town01 authors facing prologues on several placements (got {derived})"
    );

    // Semantic pin: the two villagers standing side by side at tiles
    // (29,22) / (30,22) face EACH OTHER along X - the west one faces X+
    // (LUT 6, engine 0x400), the east one X- (LUT 2, engine 0xC00). A
    // convention error (axis mirror or missing half-turn) breaks this.
    let facing_at = |tile: (u8, u8)| -> Option<u8> {
        placements
            .iter()
            .find(|p| (p.tile_x, p.tile_z) == tile)
            .and_then(|p| placement_initial_facing(&man_file, &man_bytes, p))
    };
    let west = facing_at((29, 22)).expect("west villager derives a facing");
    let east = facing_at((30, 22)).expect("east villager derives a facing");
    assert_eq!(west, 6, "west villager faces X+ (toward the east one)");
    assert_eq!(east, 2, "east villager faces X- (toward the west one)");
    assert_eq!(facing_index_to_engine_heading(west), Some(0x400));
    assert_eq!(facing_index_to_engine_heading(east), Some(0xC00));

    // World seeding: after the placement-derived install + facing seed, the
    // heading map carries the derived values for never-walked NPCs (keyed by
    // placement index), so the field renderer stands them at their retail
    // heading.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_carriers_from_man(&man_file, &man_bytes);
    world.seed_field_npc_facings(&man_file, &man_bytes);
    assert!(
        world.field_npc_headings.len() >= derived.min(4),
        "seeded headings missing (got {})",
        world.field_npc_headings.len()
    );
    for p in &placements {
        let Some(idx) = placement_initial_facing(&man_file, &man_bytes, p) else {
            continue;
        };
        let expect = facing_index_to_engine_heading(idx).expect("direction slot");
        assert_eq!(
            world.field_npc_headings.get(&(p.index as u8)),
            Some(&expect),
            "placement {} seeded heading mismatch",
            p.index
        );
    }
}

/// Disc-gated: the **dynamic** facing law - how a placement's heading changes
/// once it starts walking a route the disc actually authors.
///
/// Retail's `0x47` walk kernel (`FUN_8003774C`, tail `0x80037D4C`) quantises
/// each frame's step to the eight-entry compass LUT at `0x80073F04` and writes
/// `+0x26` outright, so a walking NPC's facing is always one of eight values
/// and never an interpolated angle. It also closes the **dominant axis first**
/// (`0x80037C4C`), which is what keeps a walker on a cardinal heading for most
/// of a leg. This drives the ported VM over each town01 placement's own
/// decoded waypoints, at that placement's own decoded glide speed.
#[test]
fn town01_walk_legs_only_ever_face_the_eight_compass_points() {
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
    let placements = man_file.actor_placements(&man_bytes);

    // Every compass heading the walk law may emit, and nothing else.
    let compass: Vec<u16> = (0..8u8)
        .map(|i| legaia_engine_vm::motion_vm::heading_lut_engine(i).expect("direction slot"))
        .collect();

    let mut legs = 0usize;
    let mut cardinal_frames = 0usize;
    for p in &placements {
        let route =
            legaia_engine_core::man_field_scripts::placement_motion_route(&man_file, &man_bytes, p);
        if route.is_empty() {
            continue;
        }
        // The placement's own decoded walk-kernel step, so the pacing is the
        // disc's rather than a stand-in.
        let speed =
            legaia_engine_core::man_field_scripts::placement_wander_step(&man_file, &man_bytes, p)
                .map(|s| s.speed)
                .unwrap_or(4);
        let mut state = legaia_engine_vm::motion_vm::MotionState {
            world_x: p.world_x,
            world_z: p.world_z,
            speed: speed.max(1),
            ..Default::default()
        };
        for &(tx, tz) in &route {
            legs += 1;
            // Retail resets the progress cursor per leg (the `+0x54` zero in
            // the Done epilogue); the engine's `start_field_npc_motion` does
            // the same, so the harness must too.
            state.pc = 0;
            state.op_accum = 0;
            let target = legaia_engine_vm::motion_vm::MotionTarget {
                x: tx,
                z: tz,
                ..Default::default()
            };
            let program = [legaia_engine_vm::motion_vm::MotionOp::MoveTowardTarget as u8];
            for frame in 0..4096 {
                let moved_from = (state.world_x, state.world_z);
                let r = legaia_engine_vm::motion_vm::step(&mut state, target, &program);
                if (state.world_x, state.world_z) != moved_from {
                    assert!(
                        compass.contains(&state.yaw),
                        "placement {} leg to ({tx}, {tz}) frame {frame}: heading {:#05X} \
                         is not one of the eight compass points",
                        p.index,
                        state.yaw,
                    );
                    // A cardinal heading means exactly one axis advanced -
                    // the dominant-axis phase.
                    if state.yaw.is_multiple_of(0x400) {
                        cardinal_frames += 1;
                    }
                }
                if r == legaia_engine_vm::motion_vm::StepResult::Done {
                    break;
                }
            }
            assert_eq!(
                (state.world_x, state.world_z),
                (tx, tz),
                "placement {} leg to ({tx}, {tz}) never arrived",
                p.index
            );
        }
    }

    assert!(legs >= 4, "town01 authors several walk legs (got {legs})");
    assert!(
        cardinal_frames > 0,
        "no leg spent a frame on a cardinal heading - the dominant-axis \
         approach is not running"
    );
}

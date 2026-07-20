//! Battle-camera per-action framing (`FUN_801F0348`) end-to-end.
//!
//! Retail resolves the camera's height / distance `ctx+0x6D0` every time an
//! actor is seeded, from the monster record's `+0x1F` size class:
//! `clamp(size << 7, 0x0C00, 0x1400)`. The chain under test is
//! `record +0x1F` -> `MonsterRecord::size_class` -> `MonsterDef::size_class` ->
//! the `BattleActionHost::monster_size_class` hook -> the port's
//! `camera_height_for_frame` at action seed -> `World::battle_camera_frame_height`.
//!
//! The first test is disc-free (synthetic catalog); the second is disc-gated
//! and pins the chain to real records.

use legaia_engine_core::battle_events::BattleEvent;
use legaia_engine_core::monster_catalog::MonsterDef;
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::battle_action::ActionState;
use legaia_engine_vm::battle_formulas::{CAMERA_HEIGHT_MAX, CAMERA_HEIGHT_MIN};

/// Seat 3 party + 5 monster slots, put `size_class` on the monster the
/// attacker will frame on, and run the battle SM until it seeds an action.
fn frame_height_for(size_class: u8) -> (i16, bool) {
    let mut world = World {
        mode: SceneMode::Battle,
        party_count: 3,
        ..World::default()
    };
    for i in 0..8 {
        let actor = world.spawn_actor(i);
        actor.battle.liveness = 1;
    }
    // Monster slot 3 carries monster id 1; the catalog gives it the size class.
    world.actors[3].battle_monster_id = Some(1);
    let mut def = MonsterDef::new(1, "Test Bulk", 500, 40);
    def.size_class = size_class;
    world.monster_catalog.insert(def);

    // Party slot 0 attacks monster slot 3 - retail's target-side arm.
    world.actors[0].battle.active_target = 3;
    world.battle_ctx.queued_action = 3; // Attack
    world.battle_ctx.action_state = ActionState::Begin.as_byte();

    let mut saw_event = false;
    for _ in 0..500 {
        let _ = world.tick();
        for ev in world.drain_battle_events() {
            if matches!(ev, BattleEvent::CameraFrameHeight { .. }) {
                saw_event = true;
            }
        }
    }
    (world.battle_camera_frame_height, saw_event)
}

/// A bulky monster pulls the camera back; the framing must reach the world
/// state, not just the formula. Fails outright (`0x0C00`, no event) if the
/// `camera_height_for_frame` call is not wired into the action-seed dispatch
/// or `MonsterDef` carries no size class.
#[test]
fn action_seed_frames_the_camera_on_the_target_size_class() {
    // 0x28 << 7 == 0x1400: the ceiling.
    let (height, saw_event) = frame_height_for(0x28);
    assert!(saw_event, "no CameraFrameHeight event reached the world");
    assert_eq!(height, CAMERA_HEIGHT_MAX);

    // 0x20 << 7 == 0x1000: mid-band, so this is not just a saturating clamp.
    let (height, _) = frame_height_for(0x20);
    assert_eq!(height, 0x1000);

    // Below the floor's pre-image the clamp holds at the retail default.
    let (height, _) = frame_height_for(0x0E);
    assert_eq!(height, CAMERA_HEIGHT_MIN);
}

/// The world seeds `ctx+0x6D0` at the retail floor, so a battle whose catalog
/// carries no size classes frames exactly as before this was wired.
#[test]
fn default_world_seeds_the_retail_floor() {
    assert_eq!(
        World::default().battle_camera_frame_height,
        CAMERA_HEIGHT_MIN
    );
}

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = std::path::PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

/// Pin `+0x1F` to real records. The byte tracks model bulk, not any stat: the
/// bee / bat family sits at the low end while the two largest bosses saturate
/// the camera clamp. Lapis is the control - 64800 HP but an ordinary body, so
/// a byte that tracked HP could not produce this row.
#[test]
fn disc_records_carry_the_size_class_the_camera_frames_on() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // (monster id, name, size class, framed height)
    let expected: &[(u16, &str, u8, i16)] = &[
        (62, "Killer Bee", 14, CAMERA_HEIGHT_MIN), // small flier, clamps to floor
        (73, "Caruban", 46, CAMERA_HEIGHT_MAX),    // giant, saturates
        (182, "Koru", 48, CAMERA_HEIGHT_MAX),      // largest in the roster
        (167, "Lapis", 20, CAMERA_HEIGHT_MIN),     // 64800 HP, ordinary body
        (98, "Gola Gola", 37, 0x1280),             // mid-band, no clamp
        (25, "Theeder", 14, CAMERA_HEIGHT_MIN),
    ];
    for &(id, name, size, height) in expected {
        let rec = legaia_asset::monster_archive::record(&entry, id)
            .expect("decode")
            .unwrap_or_else(|| panic!("monster id {id} ({name}) missing"));
        assert_eq!(rec.name, name, "id {id} name");
        assert_eq!(rec.size_class, size, "id {id} ({name}) record +0x1F");

        let def = legaia_engine_core::monster_catalog::monster_def_from_record(&rec);
        assert_eq!(def.size_class, size, "id {id} ({name}) catalog size_class");
        assert_eq!(
            legaia_engine_vm::battle_formulas::camera_height_from_size_class(def.size_class),
            height,
            "id {id} ({name}) framed height",
        );
    }

    // Whole-roster shape: the byte is populated on every record and stays in a
    // narrow band. A zero or a wild outlier would mean the offset drifted.
    let slots = legaia_asset::monster_archive::slot_count(&entry);
    let mut seen = 0usize;
    for id in 1..=slots as u16 {
        let Ok(Some(rec)) = legaia_asset::monster_archive::record(&entry, id) else {
            continue;
        };
        seen += 1;
        assert!(
            (10..=60).contains(&rec.size_class),
            "id {id} ({}) size_class {} outside the roster band",
            rec.name,
            rec.size_class,
        );
    }
    assert!(
        seen > 150,
        "only {seen} records decoded; archive truncated?"
    );
}

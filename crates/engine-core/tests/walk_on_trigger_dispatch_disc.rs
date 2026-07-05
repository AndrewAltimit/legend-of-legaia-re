//! Disc-gated: the per-frame **walk-on tile-trigger dispatch** - the retail
//! field loop's tile compare (`FUN_801D1EC4` -> `FUN_801D5630` ->
//! `FUN_8003BDE0`) that makes scene exits and walk-on story beats work
//! during free-roam.
//!
//! Covers the full New-Game journey that used to soft-lock: the prologue
//! hand-off into `town01`, naming, then free-roam where
//!
//! 1. walking onto the post-naming story-beat trigger band spawns its
//!    partition-2 record (its C2 gate reads the flag the opening timeline
//!    set through the field VM's system-flag bank - one store, not two);
//! 2. walking into a house door (a gate-0 tile trigger binding a
//!    partition-0 ＩＮ/ＯＵＴ record) teleports the player through it;
//! 3. walking onto the south-gate exit tiles spawns the partition-2 record
//!    whose `0x3F` leaves Rim Elm for the `map01` overworld, seating the
//!    player at the op's entry tile.
//!
//! Structural assertions only (tiles, coords, mode) - no Sony bytes.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md
//! convention).

use legaia_engine_core::name_entry::NameEntryInput;
use legaia_engine_core::scene::{SceneHost, SceneTickEvent};
use legaia_engine_core::world::World;
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

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

/// Teleport the player actor to a tile centre (the walk-on dispatch keys off
/// position, so a direct seat exercises the tile-crossing compare).
fn seat_at_tile(world: &mut World, tile_x: i16, tile_z: i16) {
    let slot = world.player_actor_slot.expect("player installed") as usize;
    world.actors[slot].move_state.world_x = tile_x * 128 + 0x40;
    world.actors[slot].move_state.world_z = tile_z * 128 + 0x40;
}

/// Run the `town01` opening to free-roam: prologue hand-off entry, tick to
/// the name-entry park, commit a name, tick the timeline to completion.
fn run_opening_to_freeroam(host: &mut SceneHost) {
    host.world.entering_town01_opening = true;
    host.enter_field_scene(legaia_asset::new_game::OPENING_SCENE, 0)
        .expect("enter town01");
    let mut ticks = 0u32;
    while !host.world.name_entry_active() && ticks < 4000 {
        host.world.tick();
        ticks += 1;
    }
    assert!(host.world.name_entry_active(), "name entry opens");
    // Type one glyph, go to End, confirm Yes.
    host.world.name_entry.as_mut().unwrap().cursor = 0;
    host.world.step_name_entry(NameEntryInput {
        confirm: true,
        ..Default::default()
    });
    host.world.name_entry.as_mut().unwrap().cursor =
        legaia_engine_core::name_entry::CHAR_CELLS + 16;
    host.world.step_name_entry(NameEntryInput {
        confirm: true,
        ..Default::default()
    });
    host.world.step_name_entry(NameEntryInput {
        confirm: true,
        ..Default::default()
    });
    let mut more = 0u32;
    while host.world.cutscene_timeline.is_some() && more < 4000 {
        host.world.tick();
        more += 1;
    }
    assert!(
        host.world.cutscene_timeline.is_none(),
        "opening timeline completes"
    );
}

/// The gate-flag store is unified: the opening timeline's flag writes (the
/// field VM's 0x50/0x60/0x70 system-flag bank, retail `DAT_80085758`) are
/// the same bits the partition-2 record dispatcher gates on.
#[test]
fn opening_flags_reach_the_p2_gate_check() {
    let Some(mut host) = open_host() else {
        return;
    };
    run_opening_to_freeroam(&mut host);
    // town01's opening record (P2[3]) lists flag 0x225 (= 549) in its C1
    // one-shot gate and sets it during execution.
    assert!(
        host.world.system_flag_test(549),
        "the opening timeline set its one-shot flag in the system bank"
    );
    assert!(
        host.world.p2_gate_flag_set(549),
        "the P2 gate check reads the same store the VM wrote"
    );
}

/// After naming, walking onto the story-beat trigger band spawns its
/// partition-2 record: the C2 gate (flag 549) passes, the beat plays, and
/// its own C1 one-shot flag (550) is set by execution - the progression
/// that used to dead-end.
#[test]
fn post_naming_story_beat_spawns_on_walk_on() {
    let Some(mut host) = open_host() else {
        return;
    };
    run_opening_to_freeroam(&mut host);
    assert!(!host.world.p2_gate_flag_set(550), "beat not yet played");
    // The (18..=22, 24..=26) trigger band south of the opening's rest
    // position references the post-naming record (P2[4], C1=[550] C2=[549]).
    seat_at_tile(&mut host.world, 20, 26);
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    assert!(
        host.world.cutscene_timeline_active(),
        "walking onto the trigger band spawns the post-naming beat"
    );
    // The beat is the Mei conversation: its inline `0x1F` dialog boxes park
    // the timeline until the player confirms, so mash confirm on alternate
    // ticks (edge-triggered input) while it plays out. The timeline completes
    // at the record's resident loop-back (choreography wrapped), well under
    // the anti-hang frame cap.
    let mut n = 0u32;
    while host.world.cutscene_timeline_active() && n < 20000 {
        let pad = if n.is_multiple_of(2) {
            legaia_engine_core::input::PadButton::Cross.mask()
        } else {
            0
        };
        host.world.set_pad(pad);
        host.tick().expect("tick");
        n += 1;
    }
    host.world.set_pad(0);
    assert!(
        n < 3000,
        "the beat completes naturally (ticked {n}, cap would be ~1200 frames of lock)"
    );
    assert!(
        host.world.p2_gate_flag_set(550),
        "the beat set its one-shot flag by execution (no re-fire)"
    );
    // Standing on the band again does not re-spawn: the C1 gate blocks.
    seat_at_tile(&mut host.world, 20, 20);
    for _ in 0..2 {
        host.tick().expect("tick");
    }
    seat_at_tile(&mut host.world, 20, 26);
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    assert!(
        !host.world.cutscene_timeline_active(),
        "the one-shot C1 gate blocks a re-fire"
    );
}

/// Walking onto Rim Elm's south-gate exit tiles leaves for the overworld:
/// the gate-1 trigger spawns P2[0], whose `0x3F` names `map01` and seats
/// the arrival at the op's entry tile (0x60, 0x19).
#[test]
fn town01_exit_tiles_leave_for_the_overworld() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene("town01", 0).expect("enter town01");
    // Settle at the cold spawn (no trigger there).
    for _ in 0..5 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected early transition to {name}")
        }
    }
    seat_at_tile(&mut host.world, 25, 46);
    let mut entered = None;
    for _ in 0..120 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    assert_eq!(
        entered.as_deref(),
        Some("map01"),
        "the south-gate trigger's 0x3F record leaves for the overworld"
    );
    let slot = host.world.player_actor_slot.unwrap() as usize;
    let ms = &host.world.actors[slot].move_state;
    assert_eq!(
        ((ms.world_x - 0x40) >> 7, (ms.world_z - 0x40) >> 7),
        (0x60, 0x19),
        "arrival seats the player at the op-0x3F entry tile"
    );
    assert_eq!(
        host.world.mode,
        legaia_engine_core::world::SceneMode::WorldMap,
        "map01 routes through the world-map entry"
    );
}

/// House doors: a gate-0 tile trigger binds its partition-0 record as a
/// touch object; walking into the door teleports the player to the paired
/// endpoint (the record's cross-context `0xA3 0xF8` player MOVE_TO).
#[test]
fn house_door_binds_teleport_on_contact() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene("town01", 0).expect("enter town01");
    // The (17,29) doorway bind targets the pinned interior landing
    // (12480, 6976) = tile (97, 54) - the captured Mei's-house-variant warp.
    let bind = host
        .world
        .field_walk_touch
        .iter()
        .find(|(s, _)| **s >= World::TRIGGER_WALK_TOUCH_SLOT_BASE)
        .map(|(_, &(pos, event))| (pos, event));
    assert!(bind.is_some(), "town01 installs gate-0 door binds");
    let doorway = (17i16 * 128 + 0x40, 29i16 * 128 + 0x40);
    let target = host
        .world
        .field_walk_touch
        .values()
        .find_map(|&((wx, wz), event)| {
            if (wx, wz) != doorway {
                return None;
            }
            match event {
                legaia_engine_core::man_field_scripts::WalkTouchEvent::PlayerMoveTo {
                    world_x,
                    world_z,
                } => Some((world_x, world_z)),
                _ => None,
            }
        });
    assert_eq!(
        target,
        Some((12480, 6976)),
        "the (17,29) doorway bind decodes the pinned interior landing"
    );
    // Pad-walk into the doorway; the touch dispatch snaps the player inside.
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    let slot = host.world.player_actor_slot.unwrap() as usize;
    host.world.actors[slot].move_state.world_x = doorway.0;
    host.world.actors[slot].move_state.world_z = doorway.1 - 200;
    host.world
        .set_pad(legaia_engine_core::input::PadButton::Up.mask());
    let mut landed = false;
    for _ in 0..60 {
        host.tick().expect("tick");
        let ms = &host.world.actors[slot].move_state;
        if (ms.world_x, ms.world_z) == (12480, 6976) {
            landed = true;
            break;
        }
    }
    host.world.set_pad(0);
    assert!(landed, "walking into the door lands at the interior tile");
}

/// Ambient walk-on records - fog-config / flag-reset parks with empty gates
/// (town01 P2[21]/P2[22] at the north-path tiles, P2[16] at the well row) -
/// complete within a tick: the record does its writes and parks in a
/// `Nop`+`JmpRel`-to-self spin that retail leaves spinning as a parallel
/// context. They must never lock the player (the pre-fix symptom: ~20 s of
/// frozen controls + camera grab on every crossing, read in play-testing as
/// "the opening scene fired again in odd places near doors").
#[test]
fn ambient_records_never_lock_the_player() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene("town01", 0).expect("enter town01");
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    for (tx, tz) in [(14i16, 8i16), (97, 11), (14, 11)] {
        seat_at_tile(&mut host.world, tx, tz);
        for _ in 0..3 {
            host.tick().expect("tick");
        }
        assert!(
            !host.world.cutscene_timeline_active(),
            "ambient record at ({tx},{tz}) must complete without locking the player"
        );
        // Step off so the next crossing re-arms the tile compare.
        seat_at_tile(&mut host.world, 20, 20);
        host.tick().expect("tick");
    }
}

/// Gate/progression flags survive a full save/load round trip: `save_full`
/// mirrors the system bank into the story-flag window at `+0x158` (the
/// retail RAM overlap) and `load_full` seeds it back. Disc-free.
#[test]
fn gate_flags_survive_save_roundtrip() {
    let mut world = World::new();
    world.system_flag_set(549);
    world.system_flag_set(550);
    let sf = world.save_full();
    let mut reloaded = World::new();
    reloaded.load_full(sf);
    assert!(reloaded.p2_gate_flag_set(549));
    assert!(reloaded.p2_gate_flag_set(550));
    assert!(!reloaded.p2_gate_flag_set(551));
}

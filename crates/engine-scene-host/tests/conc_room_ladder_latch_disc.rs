//! Disc-gated: walking along the west wall of `conc`'s southern hall must
//! not shake the world.
//!
//! `conc`'s scene-entry system script (`P1[0]`) is a room selector. Each pass
//! of its loop tests the region-type mask `_DAT_8007B8F4` one room bit at a
//! time (op `0x42` mode 0, bits 2..8) and, for the first room whose bit is set
//! and whose own system flag (`0x19D..`) is clear, clears every room flag,
//! raises its own, retires the floor oscillators (`4C 9F`), installs the
//! room's floor ladder (`4C 9E`) and view window (`0x46`) - room 2 adds two
//! oscillators on rungs 1 and 2 (`4C 90`).
//!
//! The `.MAP` region table gives the hall (type 2, tiles `x 45..56`, `z
//! 99..123`) a one-column overlap with a type-3 region on each side, and the
//! overlap columns are open floor. A mask carrying both bits makes the loop
//! install room 2 on one pass and room 3 on the next, for as long as the
//! player stands there: rungs 1..3 jump by 256 / 512 / 1280 units every
//! pass, and with the free-roam camera's whole-map draw every tier of the
//! town jumps with them.
//!
//! Retail never reaches that state because the mask is a latch: the field
//! tick rewrites it per frame only behind scratchpad bit 22 (`FUN_801D1344`,
//! `0x801D17DC..0x801D1830`), and otherwise only the re-centres, the zone
//! queries and `[4C 3D]` do. The port re-ran the region scan on every tile
//! crossing. Two retail library states (`town01` at tile `(96, 18)`) hold a
//! mask that is not their tile's, which a per-tile refresh cannot produce.
//!
//! REF: FUN_801D1344, FUN_800180EC, FUN_801DBA20
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_field::camera::Camera;
use legaia_engine_field::field_regions::{RegionTable, region_type_mask};
use legaia_engine_field::frame_step::{camera_after_world_tick, camera_before_world_tick};
use legaia_engine_scene::scene::DefaultMapIdResolver;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

/// The hall's region bit (type 2) and its west neighbour's (type 3).
const HALL: u32 = 1 << 2;
const WEST_ROOM: u32 = 1 << 3;
/// A seat inside the hall, and the overlap column west of it.
const SEAT_TILE: (i32, i32) = (50, 115);
const OVERLAP_COLUMN: i32 = 45;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn tile(v: i16) -> i32 {
    (i32::from(v) - 0x40) >> 7
}

#[test]
fn conc_hall_walk_keeps_one_room_ladder() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    host.enter_field_scene("conc", 0)
        .expect("enter_field_scene('conc')");
    // The play hosts run the live loop, which is where the per-tile refresh
    // lived.
    host.world.toggles.live_gameplay_loop = true;
    host.world.locomotion.follow_terrain_height = true;

    // Non-vacuity: the overlap column really carries both room bits.
    let block = host.world.terrain.map_region_block.clone();
    let table = RegionTable::parse(&block).expect("conc region table");
    let overlap = region_type_mask(Some(&table), OVERLAP_COLUMN, SEAT_TILE.1);
    assert_eq!(
        overlap & (HALL | WEST_ROOM),
        HALL | WEST_ROOM,
        "conc's hall overlaps its west neighbour on column {OVERLAP_COLUMN}"
    );

    let mut cam = Camera::default();
    cam.reset_globals_for_scene_entry();
    let step = |host: &mut SceneHost, cam: &mut Camera, pad: u16| {
        camera_before_world_tick(cam, &mut host.world, None);
        host.world.set_pad(pad);
        let _ = host.world.tick();
        camera_after_world_tick(cam, &mut host.world, false);
    };
    for _ in 0..60 {
        step(&mut host, &mut cam, 0);
    }
    let (sx, sz) = (
        (SEAT_TILE.0 * 128 + 0x40) as i16,
        (SEAT_TILE.1 * 128 + 0x40) as i16,
    );
    assert!(host.debug_seat_standing(sx, sz), "seat inside the hall");
    for _ in 0..40 {
        step(&mut host, &mut cam, 0);
    }
    assert_eq!(
        host.world.flags.extra_flags & (HALL | WEST_ROOM),
        HALL,
        "the seat latches the hall"
    );
    let hall_ladder = host.world.terrain.floor_height_lut;

    // Walk west onto the overlap column and stand there.
    let slot = host.world.player_actor_slot.expect("player") as usize;
    let mut reached = false;
    let mut jumps = 0usize;
    let mut prev = host.world.terrain.floor_height_lut;
    for (pad, frames) in [(PadButton::Left.mask(), 75), (0, 60)] {
        for _ in 0..frames {
            step(&mut host, &mut cam, pad);
            let lut = host.world.terrain.floor_height_lut;
            // An oscillator moves a rung a unit or two a frame; a room
            // install moves it by whole tiers.
            if lut
                .iter()
                .zip(&prev)
                .any(|(a, b)| (i32::from(*a) - i32::from(*b)).abs() > 64)
            {
                jumps += 1;
            }
            prev = lut;
            reached |= tile(host.world.actors[slot].move_state.world_x) == OVERLAP_COLUMN;
        }
    }
    eprintln!(
        "[ran] conc hall: reached the overlap column {reached}, ladder jumps {jumps}, mask {:#x}",
        host.world.flags.extra_flags
    );
    assert!(reached, "the walk reaches the overlap column");
    assert_eq!(
        jumps, 0,
        "standing on the overlap must not re-install room ladders every pass"
    );
    assert_eq!(
        host.world.flags.extra_flags & (HALL | WEST_ROOM),
        HALL,
        "a walk keeps the latched room"
    );
    assert_eq!(
        host.world.terrain.floor_height_lut[3], hall_ladder[3],
        "the hall's ladder stays installed"
    );
}

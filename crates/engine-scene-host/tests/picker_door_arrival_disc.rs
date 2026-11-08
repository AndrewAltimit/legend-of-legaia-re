//! Disc-gated sweep of the scene picker's default seat
//! (`SceneHost::arm_picker_entry_seat`): every scene the play page's picker
//! lists (the `town` / `field` / `world_map` categories of the one scene-name
//! table) is entered as a picker visit, and
//!
//! - a scene some door or warp leads into seats the party on one of those
//!   arrivals (or the arrival's bounded wall rescue): standable, clear of the
//!   collision walls, and still in the picked scene after the first ticks (a
//!   seat on an exit trigger would warp straight out);
//! - a scene whose doors are all rejected, or that no door leads into, seats
//!   on one of its own scripts' player seats or keeps the cold-entry
//!   resolver's seat;
//! - a scene with a `PICKER_SEAT_OVERRIDES` row (a reviewed seat, for the
//!   scenes whose ranked arrival or cold seat is unplayable) seats there, on
//!   a standable tile that is not walled in, and stays in the scene.
//!
//! Every seat, whatever chose it, is checked the same way: off the wall bits,
//! not walled in, and off every gate-1 walk-on trigger tile and kind-0
//! teleport pad (the first tick's stale-tile compare fires either) - except
//! the scenes in [`ON_TRIGGER_BY_DESIGN`], which seat on their arrival beat's
//! trigger as the door does.
//!
//! The scenes that fall back are printed, not pinned: the list is the disc's
//! property, and the sweep's job is that no seat is out of bounds.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset - CI runs without disc data.

use std::path::PathBuf;

use legaia_engine_scene::scene::is_world_map_scene;
use legaia_engine_scene_host::scene_host::{PICKER_SEAT_OVERRIDES, PickerSeat, SceneHost};

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

/// Ticks run after entry: enough for a walk-on trigger under the seat to
/// fire and its record to reach a scene change.
const SETTLE_TICKS: usize = 90;

/// Scenes whose picker seat stands on a walk-on trigger on purpose: the
/// pass-through `taiku2`, whose one arrival's beat is the whole visit
/// (`SceneHost::picker_pass_through`), and `rugi`, whose override seats on
/// the door so its entrance beat fires (`PICKER_SEAT_OVERRIDES`).
const ON_TRIGGER_BY_DESIGN: &[&str] = &["taiku2", "rugi"];

/// The failures a seat shows whatever chose it: a wall bit, walled in on all
/// four sides, or a gate-1 trigger / kind-0 pad tile under it; and, for a seat
/// the port derived rather than one an op names (`authored` false), a pocket
/// boxed in by trigger rows. A door's own arrival tile can stand between two
/// trigger rows - `izumi`'s, `teien`'s and `tunnelc`'s do - and is still
/// where retail seats the party, so a seat on the arrival's tile is exempt.
fn seat_faults(h: &SceneHost, scene: &str, seat: (i16, i16), authored: bool) -> Vec<String> {
    let mut out = Vec::new();
    if h.world.field_tile_is_wall(seat.0, seat.1) {
        out.push(format!("{scene}: seat {seat:?} is a wall"));
    }
    if (0..4).all(|d| h.world.field_dir_blocked(seat.0, seat.1, d)) {
        out.push(format!("{scene}: seat {seat:?} is walled in"));
    }
    if ON_TRIGGER_BY_DESIGN.contains(&scene) {
        return out;
    }
    let tile = ((seat.0 as u16 >> 7) as u8, (seat.1 as u16 >> 7) as u8);
    let Some(sc) = h.scene.as_ref() else {
        return out;
    };
    if let Ok((p, f)) = sc.field_tile_triggers(&h.index)
        && let Some(t) = p
            .iter()
            .chain(f.iter())
            .find(|t| t.gate == 1 && (t.tile_x, t.tile_z) == tile)
    {
        out.push(format!(
            "{scene}: seat {seat:?} stands on the gate-1 trigger of P2[{}] at {tile:?}",
            t.record
        ));
    }
    if let Ok((p, f)) = sc.field_intra_scene_teleports(&h.index)
        && p.iter()
            .chain(f.iter())
            .any(|t| (t.tile_x, t.tile_z) == tile)
    {
        out.push(format!(
            "{scene}: seat {seat:?} stands on a teleport pad at {tile:?}"
        ));
    }
    if authored {
        return out;
    }
    // Not boxed into a pocket by trigger rows: the open sub-cells reachable
    // from the seat without crossing a wall bit, a gate-1 trigger or a pad
    // make a room, not a stub of path behind an exit (`garmel`'s old seat,
    // two tiles of walled path north of its exit-to-`map01` row).
    let mut closed: Vec<(u8, u8)> = Vec::new();
    if let Ok((p, f)) = sc.field_tile_triggers(&h.index) {
        closed.extend(
            p.iter()
                .chain(f.iter())
                .filter(|t| t.gate == 1)
                .map(|t| (t.tile_x, t.tile_z)),
        );
    }
    if let Ok((p, f)) = sc.field_intra_scene_teleports(&h.index) {
        closed.extend(p.iter().chain(f.iter()).map(|t| (t.tile_x, t.tile_z)));
    }
    let open = |sx: i32, sz: i32| {
        (0..256).contains(&sx)
            && (0..256).contains(&sz)
            && !closed.contains(&((sx >> 1) as u8, (sz >> 1) as u8))
            && !h
                .world
                .field_tile_is_wall((sx * 64 + 32) as i16, (sz * 64 + 32) as i16)
    };
    let start = (i32::from(seat.0) >> 6, i32::from(seat.1) >> 6);
    let mut seen = std::collections::HashSet::from([start]);
    let mut queue = std::collections::VecDeque::from([start]);
    while let Some((x, z)) = queue.pop_front() {
        if seen.len() >= POCKET_SUBCELLS {
            break;
        }
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = (x + dx, z + dz);
            if open(n.0, n.1) && seen.insert(n) {
                queue.push_back(n);
            }
        }
    }
    if seen.len() < POCKET_SUBCELLS {
        out.push(format!(
            "{scene}: seat {seat:?} is boxed into {} sub-cells by walls and trigger rows",
            seen.len()
        ));
    }
    out
}

/// The smallest open area, in 64-unit sub-cells, a seat may stand in once
/// the trigger rows around it count as closed: the room size the picker's
/// own rule demands (`PICKER_ARRIVAL_MIN_REGION_SUBCELLS`).
const POCKET_SUBCELLS: usize = legaia_engine_core::world::PICKER_ARRIVAL_MIN_REGION_SUBCELLS;

#[test]
fn picker_entry_seats_on_a_standable_door_arrival() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let db = legaia_gamedata::Database::load();
    let picker: Vec<String> = db
        .scene_names()
        .iter()
        .filter(|s| matches!(s.category.as_str(), "town" | "field" | "world_map"))
        .map(|s| s.id.clone())
        .collect();
    assert!(
        !picker.is_empty(),
        "the scene-name table lists picker scenes"
    );

    let mut host = SceneHost::open_disc(&disc).expect("open disc host");
    let t0 = std::time::Instant::now();
    let catalog = host.arrival_catalog();
    println!("arrival catalog built in {:?}", t0.elapsed());
    let mut seated = 0usize;
    let mut fallbacks: Vec<String> = Vec::new();
    let mut overridden: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for scene in &picker {
        // A fresh host per scene: a scene's entry script can leave story
        // state the next entry would read.
        let mut h = SceneHost::open_disc(&disc).expect("open disc host");
        h.set_arrival_catalog(catalog.clone());
        assert!(
            h.arm_picker_entry_seat(scene),
            "[{scene}] picker arm refused"
        );
        let entered = if is_world_map_scene(scene) {
            h.enter_world_map_scene(scene)
        } else {
            h.enter_field_scene(scene, 0)
        };
        if let Err(e) = entered {
            eprintln!("[{scene}] does not enter ({e:#}) - skipped");
            continue;
        }
        let seat = h.picker_seat().cloned();
        match seat {
            Some(PickerSeat::Arrival {
                arrival,
                seat,
                rejected,
            }) => {
                let ms = &h.world.actors[0].move_state;
                if (ms.world_x, ms.world_z) != seat {
                    failures.push(format!(
                        "{scene}: player at ({},{}) not on the arrival seat {seat:?}",
                        ms.world_x, ms.world_z
                    ));
                }
                // On the door's own tile (exactly, or nudged within it off a
                // wall bit) the seat is the one retail's arrival makes.
                let tile = |(x, z): (i16, i16)| (x >> 7, z >> 7);
                let authored = tile(seat) == tile(arrival.world_xz());
                failures.extend(seat_faults(&h, scene, seat, authored));
                for _ in 0..SETTLE_TICKS {
                    if h.tick().is_err() {
                        break;
                    }
                }
                let now = h.scene.as_ref().map(|s| s.name.clone()).unwrap_or_default();
                if now != *scene {
                    failures.push(format!(
                        "{scene}: left for '{now}' within {SETTLE_TICKS} ticks of the seat {seat:?}"
                    ));
                }
                println!(
                    "[{scene}] seat {seat:?} <- {} @{:#x} ({:?}, tile {:#04x},{:#04x}, {rejected} rejected)",
                    arrival.from_scene,
                    arrival.op_pc,
                    arrival.class,
                    arrival.entry_x,
                    arrival.entry_z
                );
                seated += 1;
            }
            Some(PickerSeat::Override { rule, seat }) => {
                let ms = &h.world.actors[0].move_state;
                if (ms.world_x, ms.world_z) != seat {
                    failures.push(format!(
                        "{scene}: player at ({},{}) not on the override seat {seat:?}",
                        ms.world_x, ms.world_z
                    ));
                }
                // Standing on its floor from the first frame, not gliding
                // onto it.
                let floor = h
                    .world
                    .sample_field_floor_height(i32::from(seat.0), i32::from(seat.1));
                if i32::from(ms.world_y) != floor {
                    failures.push(format!(
                        "{scene}: override seat y {} is not its floor {floor}",
                        ms.world_y
                    ));
                }
                failures.extend(seat_faults(&h, scene, seat, false));
                for _ in 0..SETTLE_TICKS {
                    if h.tick().is_err() {
                        break;
                    }
                }
                let now = h.scene.as_ref().map(|s| s.name.clone()).unwrap_or_default();
                if now != *scene {
                    failures.push(format!(
                        "{scene}: left for '{now}' within {SETTLE_TICKS} ticks of the override seat {seat:?}"
                    ));
                }
                println!("[{scene}] OVERRIDE seat {seat:?} <- {rule:?}");
                overridden.push(scene.clone());
            }
            Some(PickerSeat::ColdFallback { arrivals, seat }) => {
                // World-map scenes keep their own overworld seat rules.
                if !is_world_map_scene(scene) {
                    failures.extend(seat_faults(&h, scene, seat, false));
                    for _ in 0..SETTLE_TICKS {
                        if h.tick().is_err() {
                            break;
                        }
                    }
                    let now = h.scene.as_ref().map(|s| s.name.clone()).unwrap_or_default();
                    if now != *scene {
                        failures.push(format!(
                            "{scene}: left for '{now}' within {SETTLE_TICKS} ticks of the cold seat {seat:?}"
                        ));
                    }
                }
                println!("[{scene}] FALLBACK cold seat {seat:?} ({arrivals} arrival(s) rejected)");
                fallbacks.push(format!("{scene} ({arrivals})"));
            }
            None => failures.push(format!("{scene}: picker entry recorded no seat")),
        }
    }
    println!(
        "picker seats: {seated} on a door arrival, {} cold fallback(s): {}",
        fallbacks.len(),
        fallbacks.join(", ")
    );
    assert!(seated > 0, "no picker scene seated on a door arrival");
    // Every reviewed row took: an override whose arrival the disc no longer
    // carries would fall back to the ranked rule silently.
    for (scene, _) in PICKER_SEAT_OVERRIDES {
        if picker.iter().any(|s| s == scene) && !overridden.iter().any(|s| s == scene) {
            failures.push(format!(
                "{scene}: its PICKER_SEAT_OVERRIDES row did not seat it"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "picker seat failures:\n{}",
        failures.join("\n")
    );
}

/// The two reported scenes, pinned. `garmel`'s one entrance is `map01`'s
/// object-bound mouth (`P0[6]`, a gate-0 bind), landing on tile `(0x14, 0x09)`,
/// the gate-1 trigger of the entrance beat `P2[10]`; the seat steps one tile
/// into the hall, past the exit-to-`map01` row at `z = 0x08`, never onto the
/// walled stub of path north of it. `town0e`, entered by retail only from
/// FMV 8's hand-off (no entry tile), seats where its epilogue `P2[5]` stands
/// the hero (`CC F8 51 20 DA`), and its entry script ends on the scene's home
/// track `2070` (it starts `2016`, then `2070`).
#[test]
fn garmel_and_town0e_seat_inside_their_scenes() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut h = SceneHost::open_disc(&disc).expect("open disc host");
    h.world.stage_picker_entry("garmel", false);
    assert!(h.arm_picker_entry_seat("garmel"));
    h.enter_field_scene("garmel", 0).expect("enter garmel");
    let Some(PickerSeat::Arrival { arrival, seat, .. }) = h.picker_seat().cloned() else {
        panic!("garmel seats on its map01 entrance: {:?}", h.picker_seat());
    };
    assert_eq!(arrival.from_scene, "map01");
    assert_eq!((arrival.entry_x, arrival.entry_z), (0x14, 0x09));
    assert_eq!(
        seat,
        (0x14 * 128 + 0x40, 0x0A * 128 + 0x40),
        "one tile into the hall"
    );

    let mut h = SceneHost::open_disc(&disc).expect("open disc host");
    h.world.stage_picker_entry("town0e", false);
    assert!(h.arm_picker_entry_seat("town0e"));
    h.enter_field_scene("town0e", 0).expect("enter town0e");
    let Some(PickerSeat::Arrival { arrival, seat, .. }) = h.picker_seat().cloned() else {
        panic!(
            "town0e seats on its own epilogue seat: {:?}",
            h.picker_seat()
        );
    };
    assert_eq!(arrival.from_scene, "town0e");
    assert_eq!((arrival.entry_x, arrival.entry_z), (0x20, 0xDA));
    assert_eq!(seat, arrival.world_xz());
    let _ = h.tick();
    assert_eq!(h.world.audio.current_bgm, Some(2070));
}

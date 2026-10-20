//! Disc-gated sweep of the scene picker's default seat
//! (`SceneHost::arm_picker_entry_seat`): every scene the play page's picker
//! lists (the `town` / `field` / `world_map` categories of the one scene-name
//! table) is entered as a picker visit, and
//!
//! - a scene some door or warp leads into seats the party on one of those
//!   arrivals (or the arrival's bounded wall rescue): standable, clear of the
//!   collision walls, and still in the picked scene after the first ticks (a
//!   seat on an exit trigger would warp straight out);
//! - a scene nothing leads into keeps the cold-entry resolver's seat.
//!
//! The scenes that fall back are printed, not pinned: the list is the disc's
//! property, and the sweep's job is that no arrival seat is out of bounds.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset - CI runs without disc data.

use std::path::PathBuf;

use legaia_engine_scene::scene::is_world_map_scene;
use legaia_engine_scene_host::scene_host::{PickerSeat, SceneHost};

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

/// Ticks run after entry: enough for a walk-on trigger under the seat to
/// fire and its record to reach a scene change.
const SETTLE_TICKS: usize = 90;

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
                if h.world.field_tile_is_wall(seat.0, seat.1) {
                    failures.push(format!("{scene}: arrival seat {seat:?} is a wall"));
                }
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
            Some(PickerSeat::ColdFallback { arrivals, seat }) => {
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
    assert!(
        failures.is_empty(),
        "picker seat failures:\n{}",
        failures.join("\n")
    );
}

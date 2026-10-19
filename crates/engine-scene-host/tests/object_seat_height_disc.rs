//! Disc-gated: a placed object's scripted height is **absolute**, and its draw
//! stands at that height - not that far above its `.MAP` seat.
//!
//! `FUN_8003A55C` seats a bound object's actor at `ladder[nibble] + y_off`
//! (`+0x16`, `0x8003A62C..0x8003A64C`). A record that raises `+0x10 &
//! 0x20000000` (`31 1D`) and writes `+0x8E` with `4C 42` puts the actor at
//! `Y = -(+0x8E)` outright (the immediate arm's mirror, and every tick after
//! in `FUN_8003BC08`'s height arm). The case-5 draw reads the actor, so the
//! mesh stands at that Y.
//!
//! `suimon` (the Water Gate) carries five such records: partition-0 records
//! 5..9 are the sheet under the sluice and the four channel segments
//! upstream of it (`4C 42` to `260`, `326`, `330`, `334`, `338`). The port
//! once read the actor's Y as a displacement from the seat, which put the
//! water `260..338` units above the ditch it runs in.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_scene::scene::Scene;
use legaia_engine_scene_host::scene_host::SceneHost;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let candidates = std::env::var_os("LEGAIA_EXTRACTED_DIR")
        .map(PathBuf::from)
        .into_iter()
        .chain(["extracted", "../extracted", "../../extracted"].map(PathBuf::from));
    for d in candidates {
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// `(flat record, op 4C 42 target)` for suimon's water records.
const SUIMON_WATER: [(usize, i32); 5] = [(5, 260), (6, 326), (7, 330), (8, 334), (9, 338)];

#[test]
fn suimon_water_draws_at_its_scripted_height_or_skip() {
    let Some(root) = extracted_root() else { return };
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    host.enter_field_scene("suimon", 0).expect("enter suimon");
    let index = host.index.clone();
    let scene = Scene::load(&index, "suimon").expect("load suimon");
    let lut = scene
        .field_floor_height_lut(&index)
        .expect("lut")
        .expect("suimon carries a floor ladder");
    let placements = scene
        .field_object_placements(&index)
        .expect("placements")
        .expect("suimon has a field map");
    let binds = scene
        .field_object_binds(&index)
        .expect("binds")
        .expect("suimon binds its objects");

    // A few ticks, so the per-tick height law has run as well as the
    // prologue's immediate mirror.
    for _ in 0..8 {
        host.tick().expect("tick");
    }
    let moves = host.world.object_draw_displacements();
    for (record, target) in SUIMON_WATER {
        let draws: Vec<_> = placements
            .iter()
            .filter(|p| {
                binds
                    .get(&(p.anchor_col, p.anchor_row))
                    .is_some_and(|b| b.record as usize == record)
            })
            .collect();
        assert!(!draws.is_empty(), "P0[{record}] binds a placed draw");
        let seat = host
            .world
            .object_bind_height(record)
            .unwrap_or_else(|| panic!("P0[{record}] has a seat height"));
        assert_eq!(
            i32::from(seat),
            draws[0].world_y(&lut),
            "P0[{record}]'s seat is its first placement's ladder[nibble] + y_off"
        );
        let dy = moves.get(&record).map_or(0, |d| d[1]);
        for p in draws {
            let drawn = p.world_y(&lut) + dy;
            assert_eq!(
                drawn, -target,
                "P0[{record}] at ({}, {}) draws at -(+0x8E), not {} above its seat",
                p.col, p.row, target
            );
        }
    }
    // The channel stays inside its ditch: below the bank tier (rung 12)
    // and above the bed (rung 4) - in the Y-down frame, between the two.
    let bank = -i32::from(lut[12]);
    let bed = -i32::from(lut[4]);
    for (record, target) in SUIMON_WATER {
        assert!(
            (bank..bed).contains(&-target),
            "P0[{record}] at {} lies between bank {bank} and bed {bed}",
            -target
        );
    }
}

/// The boulder pieces in `chitei2` (P0[28..30]) take the same law: parked
/// at `-700` absolute at spawn, whatever their seat.
#[test]
fn chitei2_boulder_parks_at_an_absolute_height_or_skip() {
    let Some(root) = extracted_root() else { return };
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    host.enter_field_scene("chitei2", 0).expect("enter chitei2");
    let moves = host.world.object_draw_displacements();
    for rec in [28usize, 29, 30] {
        let seat = i32::from(host.world.object_bind_height(rec).expect("seat"));
        let dy = moves.get(&rec).map_or(0, |d| d[1]);
        assert_eq!(
            seat + dy,
            -700,
            "P0[{rec}] parks 700 up: seat {seat}, dy {dy}"
        );
    }
}

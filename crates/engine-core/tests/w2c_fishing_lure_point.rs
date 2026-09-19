//! The cast lure: the point the bite tick probes, and what the two probes do
//! with it.
//!
//! Retail's `FUN_801CF3BC` case `0x14` spawns the lure a fixed radius ahead of
//! the angler and `FUN_801D26CC` then reads two independent things off the
//! tile under it every frame - the `+0x4000` walk grid's high nibble (a drift
//! push on the lure's `x` accumulator) and the `+0x8000` cell word's water bit
//! (a region-kind walk whose class adds to the strike credit). This test
//! drives both through the session a host ticks, over a synthetic venue map,
//! so the wiring is exercised where the hosts reach it rather than only at the
//! leaf.

use legaia_asset::fishing_species::FishingSpecies;
use legaia_engine_core::field_regions::RegionTable;
use legaia_engine_core::fishing::{
    CAST_POWER_MAX, FishingRecord, PondInput, PondPhase, PondSession, PondVenue,
};
use legaia_engine_core::fishing_actors::{
    CELL_WATER_BIT, LURE_CAST_RADIUS, LureActor, VENUE_ANCHOR, WATER_TILE_CLASSES,
    WATER_TILE_DEFAULT, walk_grid_overhead,
};
use legaia_engine_core::minigame_floor::{CELL_GRID_OFF, CELL_GRID_PITCH, GRID_EXTENT};

/// A `.MAP` buffer long enough for the object-descriptor band, the `+0x4000`
/// walk grid, the `+0x8000` cell grid and the `+0x10000` region block.
fn map_buffer() -> Vec<u8> {
    vec![0u8; 0x12000]
}

fn set_cell(buf: &mut [u8], gx: i32, gz: i32, v: u16) {
    let off = CELL_GRID_OFF + gz as usize * CELL_GRID_PITCH + gx as usize * 2;
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

/// One region record covering the whole tile plane with kind `kind`, laid out
/// the way `RegionTable::parse` reads the `+0x10000` block: an `s16` body
/// offset at `+0xE` and an `s16` count at `+0x10`.
fn set_region(buf: &mut [u8], kind: u8) {
    let block = 0x10000usize;
    let body = 0x20i16;
    buf[block + 0xE..block + 0x10].copy_from_slice(&body.to_le_bytes());
    buf[block + 0x10..block + 0x12].copy_from_slice(&1i16.to_le_bytes());
    let rec = block + body as usize;
    buf[rec] = 0;
    buf[rec + 1] = 0;
    buf[rec + 2] = 0x7F;
    buf[rec + 3] = 0x7F;
    buf[rec + 4] = kind;
}

fn species(index: usize) -> FishingSpecies {
    FishingSpecies {
        index,
        name_ptr_va: 0,
        pull_factor: 100,
        dart_factor: 20,
        sink_factor: 40,
        depth_gate: 0x100,
        score_value: 500,
        roll_cutoff_a: 0x400,
        roll_cutoff_b: 0x800,
        roll_cutoff_c: 0xc00,
        strike_gate: 0,
    }
}

fn pond(venue: Option<PondVenue>) -> PondSession {
    let table: Vec<FishingSpecies> = (0..10).map(species).collect();
    let spawn = vec![[0u32; 8]; 3];
    let mut p = PondSession::new(
        table,
        spawn,
        Vec::new(),
        0,
        1,
        2,
        100,
        FishingRecord::default(),
        0,
        0x1234_5678,
    );
    if let Some(v) = venue {
        p.attach_venue(v);
    }
    p
}

fn cast(p: &mut PondSession) {
    let press = PondInput {
        cast_edge: true,
        ..Default::default()
    };
    let idle = PondInput::default();
    p.tick(press, 1, 0x80);
    for _ in 0..64 {
        if p.phase() == PondPhase::Power {
            break;
        }
        p.tick(idle, 1, 0x80);
    }
    assert_eq!(p.phase(), PondPhase::Power);
    for _ in 0..64 {
        if p.cast_power() >= CAST_POWER_MAX {
            break;
        }
        p.tick(idle, 1, 0x80);
    }
    p.tick(press, 1, 0x80);
    for _ in 0..64 {
        if p.phase() == PondPhase::Waiting {
            break;
        }
        p.tick(idle, 1, 0x80);
    }
    assert_eq!(p.phase(), PondPhase::Waiting, "the lure never settled");
}

#[test]
fn the_cast_puts_the_lure_one_radius_from_the_anchor() {
    let (ax, az) = VENUE_ANCHOR;
    // Facing 0 is `+Z` in the quadrature pair's phase, so the whole offset
    // lands on one axis and the distance is exact rather than rounded twice.
    let lure = LureActor::cast(ax, az, 0, 1).expect("the materialised tables cover every angle");
    let dx = (lure.x() as i32 - ax as i32).abs();
    let dz = (lure.z as i32 - az as i32).abs();
    let d2 = dx * dx + dz * dz;
    let r2 = LURE_CAST_RADIUS * LURE_CAST_RADIUS;
    assert!(
        (d2 - r2).abs() <= 2 * LURE_CAST_RADIUS,
        "cast landed {d2} away, wanted about {r2}"
    );
    assert!(
        dx != 0 || dz != 0,
        "the cast must move the lure off the anchor"
    );
}

#[test]
fn a_water_tile_class_adds_its_credit_and_weight() {
    let mut map = map_buffer();
    // The class the region kind selects: `WATER_TILE_CLASSES[0]` is bit
    // `1 << 2`, so a region record of kind 2 raises it.
    let (bit, bonus, weight) = WATER_TILE_CLASSES[0];
    assert_eq!(bit, 0x04, "the first class is the `1 << 2` one");
    set_region(&mut map, 2);
    for gz in 0..GRID_EXTENT {
        for gx in 0..GRID_EXTENT {
            set_cell(&mut map, gx, gz, CELL_WATER_BIT);
        }
    }
    let (ax, az) = VENUE_ANCHOR;
    let mut lure = LureActor::cast(ax, az, 0, 1).unwrap();
    let block = map[0x10000..].to_vec();
    let table = RegionTable::parse(&block).expect("the synthetic block parses");
    let probe = lure.probe(&map, Some(&table), 0, 1);
    assert!(probe.water, "every cell carries the water bit");
    assert_eq!(probe.countdown_bonus, bonus);
    assert_eq!(probe.weight, weight);
}

#[test]
fn a_dry_tile_leaves_the_credit_and_weight_at_their_defaults() {
    let map = map_buffer();
    let (ax, az) = VENUE_ANCHOR;
    let mut lure = LureActor::cast(ax, az, 0, 1).unwrap();
    let probe = lure.probe(&map, None, 0, 1);
    assert!(!probe.water);
    assert_eq!(probe.countdown_bonus, 0);
    assert_eq!(probe.weight, WATER_TILE_DEFAULT.1);
    assert_eq!(probe.drift, 0, "an empty walk grid pushes nothing");
}

#[test]
fn the_walk_grid_drift_takes_its_sign_from_the_cast_counter() {
    let mut map = map_buffer();
    let (ax, az) = VENUE_ANCHOR;
    let seed = LureActor::cast(ax, az, 0, 1).unwrap();
    // Fill the whole `+0x4000` grid's high nibble so the probe hits wherever
    // the cast put the lure.
    for b in map[0x4000..0x8000].iter_mut() {
        *b = 0xF0;
    }
    assert!(
        walk_grid_overhead(&map[0x4000..], seed.x() as i32, seed.z as i32),
        "the filled grid must report an overhead bit"
    );
    let mut even = seed;
    let mut odd = seed;
    let a = even.probe(&map, None, 0, 1);
    let b = odd.probe(&map, None, 1, 1);
    assert!(a.drift < 0 && b.drift > 0, "{a:?} / {b:?}");
    assert_eq!(a.drift, -b.drift, "one push, two signs");
    assert_eq!(
        even.x() as i32 - seed.x() as i32,
        a.drift,
        "the push lands on the accumulator"
    );
}

#[test]
fn a_session_with_no_venue_still_casts_and_reports_no_lure() {
    let mut p = pond(None);
    cast(&mut p);
    assert!(p.lure_actor().is_none());
    assert_eq!(p.lure_probe().countdown_bonus, 0);
}

#[test]
fn a_session_with_a_venue_carries_a_probed_lure_while_the_line_is_out() {
    let mut map = map_buffer();
    set_region(&mut map, 2);
    for gz in 0..GRID_EXTENT {
        for gx in 0..GRID_EXTENT {
            set_cell(&mut map, gx, gz, CELL_WATER_BIT);
        }
    }
    let (anchor_x, anchor_z) = VENUE_ANCHOR;
    let region_block = Some(map[0x10000..].to_vec());
    let mut p = pond(Some(PondVenue {
        map,
        region_block,
        anchor_x,
        anchor_z,
        facing: 0,
    }));
    cast(&mut p);
    assert!(
        p.lure_actor().is_some(),
        "a cast with a venue places a lure"
    );
    // One waiting frame with the reel held, so the strike roll (and so the
    // probe that feeds it) runs.
    p.tick(
        PondInput {
            reel_mask: 0x40,
            ..Default::default()
        },
        1,
        0x80,
    );
    let probe = p.lure_probe();
    assert!(probe.water, "the whole synthetic venue is water");
    assert_eq!(probe.countdown_bonus, WATER_TILE_CLASSES[0].1);
}

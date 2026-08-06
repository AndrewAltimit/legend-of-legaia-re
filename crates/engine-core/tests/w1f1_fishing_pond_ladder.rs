//! Pad-driven ladder for the **fishing session kernels** - a full cast, a
//! reel-cadence match, a strike, a landed catch and a snapped line.
//!
//! `crates/engine-core/tests/fishing_minigame_real.rs` drives the simpler
//! [`FishingSession`](legaia_engine_core::fishing::FishingSession) fight, which
//! never touches the venue-faithful pre-hook half. The kernels this file
//! reaches are the ones only [`PondSession`] composes:
//!
//! | address | kernel |
//! |---|---|
//! | `FUN_801d3db4` / `FUN_801d746c` | the 16-slot reel-cadence ring + its reset |
//! | `FUN_801d26cc` | the band roll, the band-4 gate, the spawn lookup, the strike roll and the bite-interval ladder |
//! | `FUN_801d5298` | the persistent point credit + best-catch update |
//!
//! **Disc-free by construction.** Species, spawn page and gesture templates are
//! all `pub` decoded types, so this ladder supplies synthetic ones rather than
//! reading the user's overlay. That is deliberate: a disc-gated ladder
//! contributes nothing to a coverage export taken without `LEGAIA_DISC_BIN`,
//! and none of these kernels' *behaviour* depends on which numbers the disc
//! carries - only on the shape of the tables, which is what is asserted here.
//! The real values stay pinned by the disc-gated
//! `crates/web-viewer/tests/minigames_fishing_pond.rs`.
//!
//! Assertions are written from the retail kernels' intent, not from the port's
//! current output: the cast counter increments **when the lure lands** (the
//! event that advances retail's SM to state `0x19`), the reel-cadence ring
//! resets on a match, a strike can only happen while a reel button is held,
//! the far band cannot strike at all however many input edges are fed, and the
//! landed award is credited to the capped persistent total with the best-catch
//! pair updated only on an improvement.

use legaia_asset::fishing_species::{CadenceStep, CadenceTemplate, FishingSpecies, SPAWN_BANDS};
use legaia_engine_core::fishing::{
    BandCheck, CAST_POWER_MAX, FISH_POINTS_CAP, FishingRecord, LAND_RECORD, PondEvent, PondInput,
    PondPhase, PondSession, RECORD_STRIKE_BASE, ReelCadence, band_roll, band4_gate, spawn_species,
};
use legaia_engine_core::levelup::BiosRand;

/// Reel-A (Cross) held bit of the retail `_DAT_8007b850` word.
const REEL_A: u32 = 0x40;

/// A species record with the fight factors dialled for the outcome the caller
/// wants. `pull_factor` is what decides whether a straight reel-in lands the
/// fish or pins the tension gauge at its ceiling first.
fn species(index: usize, score_value: i32, pull_factor: i32) -> FishingSpecies {
    FishingSpecies {
        index,
        name_ptr_va: 0,
        score_value,
        pull_factor,
        dart_factor: 60,
        sink_factor: 4,
        depth_gate: 4096,
        roll_cutoff_a: 200,
        roll_cutoff_b: 512,
        roll_cutoff_c: 90,
        strike_gate: 100,
    }
}

/// A venue page whose every `(lure, band)` cell names `id`.
fn spawn_page(id: u32) -> Vec<[u32; SPAWN_BANDS]> {
    vec![[id; SPAWN_BANDS]; 8]
}

/// The gesture the ladder plays: reel A for `HOLD` frame-steps, then idle for
/// `HOLD`. Template **0**, so a match drives the band to `0` - which is the
/// one band the band-4 gate can upgrade.
const HOLD: i32 = 6;

fn templates() -> Vec<CadenceTemplate> {
    vec![CadenceTemplate {
        history_window: HOLD * 2,
        steps: vec![
            CadenceStep {
                duration: HOLD,
                button: 1,
            },
            CadenceStep {
                duration: HOLD,
                button: 0,
            },
        ],
    }]
}

/// Open a pond over the synthetic tables. `lure` / `rod` / `casts` are the
/// venue-0 band-4 preconditions (Normal lure, third rod, an even lifetime cast
/// counter past 50), so the gate is *reached* every strike rather than
/// short-circuited before its roll.
fn pond(fish: FishingSpecies, seed: u32) -> PondSession {
    let id = fish.index as u32;
    let mut table: Vec<FishingSpecies> = (0..10).map(|i| species(i, 1_000, 250)).collect();
    table[fish.index] = fish;
    PondSession::new(
        table,
        spawn_page(id),
        templates(),
        0,
        1,
        2,
        100,
        FishingRecord::default(),
        0,
        seed,
    )
}

/// Cast: press, wind up, press again at whatever power the oscillator is on,
/// and let the lure fly. Leaves the session in `Waiting`.
fn cast(p: &mut PondSession) {
    let press = PondInput {
        cast_edge: true,
        ..Default::default()
    };
    let idle = PondInput::default();
    // Idle -> WindUp.
    p.tick(press, 1, 0x80);
    // WindUp -> Power (the oscillator opens after the wind-up frames).
    for _ in 0..64 {
        if p.phase() == PondPhase::Power {
            break;
        }
        p.tick(idle, 1, 0x80);
    }
    assert_eq!(p.phase(), PondPhase::Power, "the power meter never opened");
    // Let the meter climb to full before locking, so the cast is deep enough
    // for the near bite interval.
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

/// Play the reel gesture until something hooks (or the line reels all the way
/// back in and the session returns to the shore). Returns the events raised.
fn work_the_lure(p: &mut PondSession, frames: usize) -> Vec<PondEvent> {
    let mut events = Vec::new();
    let mut f = 0usize;
    while f < frames && p.phase() == PondPhase::Waiting {
        // Reel A held for HOLD frames, released for HOLD - the template's own
        // gesture, and the only input that reaches the strike roll (an
        // unheld frame returns before it).
        let held = (f as i32 / HOLD) % 2 == 0;
        p.tick(
            PondInput {
                reel_mask: if held { REEL_A } else { 0 },
                cast_edge: false,
                // One fresh edge on each hold/release transition - the pad
                // nudge the retail check counts, at the realistic magnitude.
                edge_bonus: i32::from(f as i32 % HOLD == 0),
            },
            1,
            0x80,
        );
        events.extend(p.take_events());
        f += 1;
    }
    events
}

/// Reel a hooked fish in, straight-lining reel A until the fight resolves.
fn fight(p: &mut PondSession, frames: usize) -> Vec<PondEvent> {
    let mut events = Vec::new();
    for _ in 0..frames {
        if p.phase() != PondPhase::Hooked {
            break;
        }
        p.tick(
            PondInput {
                reel_mask: REEL_A,
                cast_edge: false,
                edge_bonus: 0,
            },
            1,
            0x80,
        );
        events.extend(p.take_events());
    }
    events
}

/// Cast and work the lure until a fish hooks, across as many casts as it
/// takes. Returns every event the whole run raised.
fn hook_a_fish(p: &mut PondSession, max_casts: usize) -> Vec<PondEvent> {
    let mut all = Vec::new();
    for _ in 0..max_casts {
        cast(p);
        all.extend(work_the_lure(p, 4000));
        if p.phase() == PondPhase::Hooked {
            return all;
        }
        // The line fully reeled in: the session is back at the shore.
        assert_eq!(
            p.phase(),
            PondPhase::Idle,
            "the waiting loop ended in neither a hook nor a return to the shore"
        );
    }
    panic!("no strike in {max_casts} casts");
}

#[test]
fn a_cast_reels_a_cadence_a_strike_and_a_landed_catch() {
    // A weak fish: reel A brings the line in faster than the pull loads the
    // tension gauge, so a straight reel-in lands it.
    let mut p = pond(species(3, 40_000, 90), 0x1234_5678);
    let casts_before = p.casts;

    cast(&mut p);
    // Retail increments the lifetime cast counter when the lure *lands* - the
    // same event that advances its SM to state 0x19 - not when the power is
    // locked.
    assert_eq!(
        p.casts,
        casts_before + 1,
        "the cast counter increments on the lure landing"
    );
    assert!(
        p.line_record() > RECORD_STRIKE_BASE,
        "a full-power cast seeds a line record above the strike base"
    );

    let mut events = work_the_lure(&mut p, 4000);
    let mut casts = 1;
    while p.phase() != PondPhase::Hooked && casts < 64 {
        assert_eq!(p.phase(), PondPhase::Idle);
        cast(&mut p);
        casts += 1;
        events.extend(work_the_lure(&mut p, 4000));
    }
    assert_eq!(p.phase(), PondPhase::Hooked, "no strike in {casts} casts");

    // The gesture is the template's own, so the recogniser must have matched
    // at least once on the way - each match raises the "Good!" splash.
    assert!(
        events.iter().any(|e| matches!(e, PondEvent::Splash)),
        "the reel gesture never matched a cadence template"
    );
    let hooked = events
        .iter()
        .find_map(|e| match e {
            PondEvent::Hooked(id) => Some(*id),
            _ => None,
        })
        .expect("a hook event");
    assert_eq!(
        hooked, 3,
        "the spawn page names one species, so every band must resolve to it"
    );

    let fight_events = fight(&mut p, 8000);
    assert_eq!(p.phase(), PondPhase::Landed, "the weak fish should land");
    let award = fight_events
        .iter()
        .find_map(|e| match e {
            PondEvent::Landed(pts) => Some(*pts),
            _ => None,
        })
        .expect("a landed event");
    assert!(award > 0, "a landed catch is worth points");

    // The line-record land gate: the fight ends the frame the record drops
    // below 0x136.
    assert!(
        p.line_record() < LAND_RECORD,
        "the fight landed without the record reaching the reel-in gate"
    );

    // FUN_801d5298: the award lands in the persistent total and, being the
    // first catch, becomes the best.
    assert_eq!(p.record.points, award, "award credited to the point total");
    assert_eq!(p.record.best_points, award, "first catch is the best catch");
    assert_eq!(p.record.best_fish, 3, "best catch names its species");
    assert_eq!(p.last_award(), award);
}

#[test]
fn a_fish_that_outpulls_the_reel_snaps_the_line() {
    // Same reel-in, a fish whose pull loads the gauge far faster than the
    // record falls: the tension ceiling is reached first.
    let mut p = pond(species(7, 40_000, 20_000), 0xF00D_1234);
    hook_a_fish(&mut p, 64);
    let events = fight(&mut p, 8000);
    assert_eq!(p.phase(), PondPhase::Snapped, "the strong fish should snap");
    assert!(events.iter().any(|e| matches!(e, PondEvent::Snapped)));
    assert_eq!(
        p.record.points, 0,
        "a snapped line credits nothing to the persistent total"
    );
}

#[test]
fn a_strike_needs_a_reel_button_held() {
    // `FUN_801d26cc` returns before the strike roll when neither reel bit is
    // set, so an unheld line cannot ever hook - even given an unrealistically
    // generous edge bonus and a full band-hold countdown.
    let mut b = BandCheck {
        band: 0,
        countdown: 0x40,
        splash: false,
    };
    let mut rng = BiosRand::new(0x2222);
    for _ in 0..4000 {
        assert!(
            !b.tick(&mut rng, None, 1500, 1200, 999, false, 1),
            "a strike landed with no reel button held"
        );
    }
    // The same state with the reel held does strike, so the check above is
    // not vacuous.
    let mut b = BandCheck {
        band: 0,
        countdown: 0x40,
        splash: false,
    };
    let mut rng = BiosRand::new(0x2222);
    let struck = (0..4000).any(|_| b.tick(&mut rng, None, 1500, 1200, 999, true, 1));
    assert!(struck, "a held reel never struck in 4000 frames");
}

#[test]
fn the_far_band_cannot_strike_however_many_edges_are_fed() {
    // The far band replaces the credit base with -100 and raises the modulus
    // to 2000, so the water-class bonus and the pad nudges - at most 0x1E + 3
    // between them - cannot bring the roll back above zero. A readout under
    // 100 additionally zeroes the credit outright.
    //
    // The band's boundary is the interval ladder's single live threshold
    // (`BITE_LADDER_PIVOT` = 200), NOT the 300 the line-record base uses:
    // `readout = record - 300` is a different quantity from the distance the
    // ladder discriminates on, and reading the two as one puts the far band
    // 100 units too wide.
    let mut b = BandCheck::default();
    let mut rng = BiosRand::new(0x9999);
    for readout in [0, 50, 99, 120, 150, 199] {
        for _ in 0..2000 {
            assert!(
                !b.tick(&mut rng, None, 1000, readout, 33, true, 1),
                "a strike landed at readout {readout}, inside the far band"
            );
        }
    }

    // Exactly at the pivot the ladder's *untouched initial* modulus (0x200)
    // survives - the write-order artifact the port keeps rather than folding
    // into either neighbour - and the credit override does not apply, so a
    // strike is reachable. This is what makes the sweep above a boundary
    // check rather than a vacuous one.
    let mut b = BandCheck::default();
    let mut rng = BiosRand::new(0x9999);
    let struck = (0..2000).any(|_| b.tick(&mut rng, None, 1000, 200, 33, true, 1));
    assert!(struck, "the pivot readout must still be able to strike");
}

#[test]
fn the_cadence_ring_resets_on_a_match_so_one_gesture_cannot_match_twice() {
    let mut c = ReelCadence::new(templates());
    // Play the template's gesture: HOLD frames of reel A, then HOLD idle.
    let mut matches = 0;
    for f in 0..(HOLD * 2) {
        let button = if f < HOLD { 1 } else { 0 };
        if c.feed(button, 1).is_some() {
            matches += 1;
        }
    }
    assert_eq!(
        matches, 1,
        "the gesture matched {matches} times, expected 1"
    );
    // The reset (`FUN_801d746c`) cleared the ring, so the very next frame -
    // which would otherwise still see a full matching window behind it -
    // cannot match again.
    assert!(
        c.feed(0, 1).is_none(),
        "the ring was not reset on the match"
    );
}

#[test]
fn the_band_roll_partitions_the_whole_0xfff_draw_space() {
    // `r = rand & 0xfff`: <= 0xc00 band 3, <= 0xe70 band 2, <= 0xf38 band 1,
    // else band 0. No draw maps to band 4 - that band exists only through the
    // gate.
    let mut counts = [0usize; 5];
    for r in 0..=0xfff {
        let b = band_roll(r) as usize;
        assert!(b < 4, "draw {r:#x} produced band {b}");
        counts[b] += 1;
    }
    assert_eq!(counts.iter().sum::<usize>(), 0x1000);
    assert_eq!(counts[4], 0, "no roll outcome may reach the rare band");
    // Band 3 is the overwhelming majority; band 0 is the thin tail.
    assert!(counts[3] > counts[2] + counts[1] + counts[0]);
    assert!(counts[0] > 0 && counts[0] < counts[3]);
    // The draw is masked, so anything above 0xfff repeats the same partition.
    for r in 0..=0xfff {
        assert_eq!(band_roll(r), band_roll(r + 0x1000));
    }
}

#[test]
fn the_band_4_gate_is_venue_hardwired_and_only_upgrades_band_0() {
    let mut rng = BiosRand::new(1);
    // Every precondition is an AND: wrong band, wrong rod or an odd cast
    // counter short-circuits before the roll is even taken.
    for band in 1..4 {
        assert!(!band4_gate(0, 1, 2, band, 100, &mut rng));
    }
    for rod in 0..2 {
        assert!(!band4_gate(0, 1, rod, 0, 100, &mut rng));
    }
    assert!(!band4_gate(0, 1, 2, 0, 101, &mut rng), "odd cast counter");
    // Buma additionally needs the Normal lure and more than 50 lifetime casts.
    assert!(!band4_gate(0, 2, 2, 0, 100, &mut rng), "wrong lure at Buma");
    assert!(
        !band4_gate(0, 1, 2, 0, 20, &mut rng),
        "under the cast floor"
    );
    // Vidna wants the Heavy lure and has no cast floor.
    assert!(
        !band4_gate(1, 1, 2, 0, 100, &mut rng),
        "wrong lure at Vidna"
    );

    // With every precondition met the gate is a roll, so over many draws it
    // fires sometimes and not always - the 1-in-16 / 1-in-4 masks.
    let mut rng = BiosRand::new(0xABCD);
    let buma = (0..4000)
        .filter(|_| band4_gate(0, 1, 2, 0, 100, &mut rng))
        .count();
    assert!(buma > 0 && buma < 4000, "Buma gate fired {buma}/4000");
    let mut rng = BiosRand::new(0xABCD);
    let vidna = (0..4000)
        .filter(|_| band4_gate(1, 2, 2, 0, 100, &mut rng))
        .count();
    assert!(vidna > 0 && vidna < 4000, "Vidna gate fired {vidna}/4000");
    assert!(
        vidna > buma,
        "the 1-in-4 Vidna mask must fire more often than the 1-in-16 Buma one \
         ({vidna} vs {buma})"
    );
}

#[test]
fn the_spawn_lookup_is_row_major_over_lure_and_rejects_a_species_past_the_table() {
    let mut page: Vec<[u32; SPAWN_BANDS]> = vec![[0; SPAWN_BANDS]; 3];
    for (lure, row) in page.iter_mut().enumerate() {
        for (band, cell) in row.iter_mut().enumerate() {
            *cell = (lure * 8 + band) as u32;
        }
    }
    // `spawn_table[lure * 8 + band]` - the id is the cell, and the row index
    // is the equipped lure.
    assert_eq!(spawn_species(&page, 0, 3), Some(3));
    assert_eq!(spawn_species(&page, 1, 1), Some(9));
    // Ids past the 10-record species table are rejected rather than indexed.
    assert_eq!(spawn_species(&page, 1, 2), None, "id 10 is past the table");
    assert_eq!(spawn_species(&page, 2, 0), None, "id 16 is past the table");
    // Out-of-range rows / bands are `None`, not a wrap.
    assert_eq!(spawn_species(&page, 3, 0), None);
    assert_eq!(spawn_species(&page, 0, 9), None);
}

#[test]
fn the_persistent_point_total_caps_and_the_best_catch_only_improves() {
    let mut r = FishingRecord::default();
    r.credit(3, 100);
    assert_eq!((r.points, r.best_points, r.best_fish), (100, 100, 3));
    // A worse catch adds points but does not take the best slot.
    r.credit(1, 40);
    assert_eq!((r.points, r.best_points, r.best_fish), (140, 100, 3));
    // A better one takes both.
    r.credit(7, 250);
    assert_eq!((r.points, r.best_points, r.best_fish), (390, 250, 7));
    // The total clamps at the retail cap; the best-catch pair is not capped
    // by the same literal.
    r.credit(0, FISH_POINTS_CAP);
    assert_eq!(r.points, FISH_POINTS_CAP);
    assert_eq!(r.best_points, FISH_POINTS_CAP);
    // A negative award cannot drain the bank.
    let before = r.points;
    r.credit(2, -5000);
    assert_eq!(r.points, before);
}

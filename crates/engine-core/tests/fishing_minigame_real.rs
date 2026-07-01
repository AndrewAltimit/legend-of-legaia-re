//! Disc-gated: drive the **real** parsed fishing species table (PROT 0972)
//! through the engine fishing rules engine ([`legaia_engine_core::fishing`]).
//!
//! The species parser itself is pinned by `legaia-asset`'s `fishing_species_real`;
//! this closes the engine end - the play-window load path (`SceneHost::open_disc`
//! -> `entry_bytes_extended(972)` -> `static_overlay::as_loaded` -> `parse`)
//! resolves a real table, and a `FishingSession` casts, hooks a real species, and
//! reels it to a scored resolution. No Sony bytes are asserted, only structural
//! facts. Skips + passes when `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` are absent.

use legaia_asset::static_overlay;
use legaia_engine_core::fishing::{FightOutcome, FishingPhase, FishingRecord, FishingSession};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, World};

#[test]
fn playwindow_load_path_fishes_a_real_species() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let rec = static_overlay::overlay_map()
        .by_prot_index(legaia_asset::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32)
        .expect("fishing overlay in static map");
    let raw = host
        .index
        .entry_bytes_extended(rec.prot_index)
        .expect("read PROT 0972 (extended)");
    let loaded = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");
    let species = legaia_asset::fishing_species::parse(&loaded).expect("real species table parses");
    assert_eq!(species.len(), legaia_asset::fishing_species::SPECIES_COUNT);

    // Drive the session through the World exactly like play-window's L key.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_fishing(FishingSession::new(species, 4, FishingRecord::default()));
    assert_eq!(world.mode, SceneMode::Fishing);

    // Cast a few frames, lock, then reel until the fight resolves.
    for _ in 0..4 {
        world.set_pad(0);
        let _ = world.tick();
    }
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.fishing.as_ref().unwrap().phase(),
        FishingPhase::Fighting
    );
    let hooked = world
        .fishing
        .as_ref()
        .unwrap()
        .fight()
        .unwrap()
        .species()
        .index;
    assert!(hooked < legaia_asset::fishing_species::SPECIES_COUNT);

    for _ in 0..5000 {
        if world.fishing.as_ref().unwrap().phase() != FishingPhase::Fighting {
            break;
        }
        world.set_pad(PadButton::Cross.mask()); // hold reel A
        let _ = world.tick();
    }
    let session = world.exit_fishing().expect("session installed");
    assert_eq!(session.phase(), FishingPhase::Done);
    match session.last_outcome() {
        Some(FightOutcome::Landed { points }) => {
            assert!(points > 0, "a landed real-species catch scores");
            assert_eq!(session.record().points, points);
            eprintln!("[fishing] landed species {hooked} for {points} points");
        }
        Some(FightOutcome::Snapped) => {
            // A snap is a legitimate resolution (a strong fish on the dev rod);
            // the point is that the real table drove a terminal fight.
            eprintln!("[fishing] species {hooked} snapped the dev-rod line");
        }
        other => panic!("expected a resolved fight, got {other:?}"),
    }
}

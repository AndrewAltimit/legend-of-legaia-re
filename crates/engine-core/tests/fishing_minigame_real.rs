//! Disc-gated: drive the **real** fishing overlay tables (PROT 0972) through
//! the one engine fishing session ([`legaia_engine_core::fishing::PondSession`]).
//!
//! The species parser itself is pinned by `legaia-asset`'s `fishing_species_real`;
//! this closes the engine end - the load path every host takes
//! (`SceneHost::open_disc` -> `entry_bytes_extended(972)` ->
//! `static_overlay::as_loaded` -> `SceneHost::enter_fishing_from_overlay`)
//! decodes the species, spawn and cadence tables, and the world's pad path
//! casts, hooks a species off the real spawn page, and reels it to a
//! resolution. No Sony bytes are asserted, only structural facts. Skips +
//! passes when `LEGAIA_DISC_BIN` is absent.

use legaia_asset::static_overlay;
use legaia_engine_core::fishing::{FLIGHT_FRAMES, PondEvent, PondPhase, WINDUP_FRAMES};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, World};

fn frame(world: &mut World, mask: u16) {
    world.set_pad(mask);
    let _ = world.tick();
}

fn phase(world: &World) -> PondPhase {
    world.minigames.fishing.as_ref().unwrap().phase()
}

fn cast(world: &mut World) {
    frame(world, 0);
    frame(world, PadButton::Circle.mask());
    for _ in 0..WINDUP_FRAMES + 28 {
        frame(world, 0);
    }
    frame(world, PadButton::Circle.mask());
    for _ in 0..FLIGHT_FRAMES {
        frame(world, 0);
    }
    assert_eq!(phase(world), PondPhase::Waiting);
}

#[test]
fn the_warp_load_path_fishes_a_real_species() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = match SceneHost::open_disc(&disc) {
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

    host.world.mode = SceneMode::Field;
    assert!(
        host.enter_fishing_from_overlay(&loaded),
        "species + spawn + cadence tables all decode"
    );
    let world = &mut host.world;
    assert_eq!(world.mode, SceneMode::Fishing);
    assert_eq!(
        world.minigames.fishing.as_ref().unwrap().species.len(),
        legaia_asset::fishing_species::SPECIES_COUNT
    );

    // Hold reel A through the pre-hook loop, recasting whenever the empty
    // line is reeled all the way in, until a real species strikes.
    cast(world);
    let mut hooked = None;
    for _ in 0..60_000 {
        frame(world, PadButton::Cross.mask());
        if let Some(PondEvent::Hooked(id)) = world
            .minigames
            .fishing_events
            .iter()
            .find(|e| matches!(e, PondEvent::Hooked(_)))
        {
            hooked = Some(*id);
            break;
        }
        if phase(world) == PondPhase::Idle {
            cast(world);
        }
    }
    let hooked = hooked.expect("a real species strikes within the budget");
    assert!(hooked < legaia_asset::fishing_species::SPECIES_COUNT);

    for _ in 0..60_000 {
        if phase(world) != PondPhase::Hooked {
            break;
        }
        let t = world.minigames.fishing.as_ref().unwrap().tension();
        frame(
            world,
            if t < 0x800 {
                PadButton::Cross.mask()
            } else {
                0
            },
        );
    }
    let session = world.exit_fishing().expect("session installed");
    match session.phase() {
        PondPhase::Landed => {
            let points = session.last_award();
            assert!(points > 0, "a landed real-species catch scores");
            assert_eq!(world.minigames.fishing_points, session.record.points);
            eprintln!("[ok] landed species {hooked} for {points} points");
        }
        // A snap is a legitimate resolution; the point is that the real
        // tables drove a terminal fight.
        PondPhase::Snapped => eprintln!("[ok] species {hooked} snapped the line"),
        other => panic!("expected a resolved fight, got {other:?}"),
    }
}

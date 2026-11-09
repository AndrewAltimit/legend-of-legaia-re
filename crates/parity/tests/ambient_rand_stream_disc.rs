//! The field NPC `rand()` stream against retail. Two pins:
//!
//! - `field_wander_draws_track_retail_over_the_free_walk`: state
//!   `s3_rimelm_freeroam` (`town01`, retail SCUS), seeded with the capture's
//!   BIOS seed and its wanderers' live stream state, then run with no input.
//!   The engine's `rng_state` must equal the retail film's seed word tick for
//!   tick, and each draw must be the actor retail's draw was.
//! - `spawn_draws_match_the_capture`: the placement spawner's per-move draw
//!   count on two scene entries.
//!
//! The retail side is a PCSX-Redux breakpoint capture of the seed word
//! `0x80009010` and the draw sites: `RETAIL_DRAWS` holds the LCG state each
//! vsync's draws land on and the `+0x50` flat index of the actors that made
//! them; these are arithmetic of the capture's seed, no disc bytes. `draws`
//! on a row is the actors' flat indices in draw order.
//!
//! Skips (passes) unless `LEGAIA_DISC_BIN` is set and the save library and
//! extracted disc are found (`LEGAIA_SAVES_LIBRARY` / `LEGAIA_EXTRACTED_DIR`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use legaia_parity::retail_compare::{
    enumerate_corpus, read_retail, resolve_dirs, run_engine_then, seat_ambient_streams,
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `s3_rimelm_freeroam` (one library file under two scenario labels).
const STATE: &str = "e9e526595d0479ce1af9619418922ca337019ba0d1078169bc65235c22d75fc8";
/// The seed word `0x80009010` in that state.
const CAPTURE_SEED: u32 = 0x1A11_93DF;
/// Engine ticks to run. The capture's first engage raises the player's
/// `0x80000` at vsync 169; no input is held here, so the window is the
/// wanderers alone.
const TICKS: u64 = 168;
/// The engine row a retail film row lands on: retail's film row `v` holds
/// the seed after vsync `v`'s game step, the engine's tick `v + 1` is the
/// same step (the state saves between the two).
const ROW_TO_TICK: u64 = 1;

/// `(film vsync, seed word, the draws' actors in draw order)`.
const RETAIL_DRAWS: &[(u64, u32, &[u16])] = &[
    (5, 0x7204182C, &[50]),
    (27, 0xA923E2F5, &[50]),
    (39, 0xB1EC788A, &[79]),
    (45, 0x13258EFB, &[49]),
    (59, 0xE9B08B18, &[52]),
    (67, 0xF71CB971, &[49]),
    (69, 0x16219356, &[49]),
    (89, 0x71B41FD7, &[48]),
    (95, 0x82AF40C4, &[50]),
    (97, 0xE5F27BAD, &[52]),
    (99, 0x5EBB8EE2, &[49]),
    (101, 0x6030E273, &[49]),
    (111, 0x31C1A530, &[73]),
    (117, 0x22E825A9, &[50]),
    (119, 0xB2AD32CF, &[53, 50]),
    (121, 0xB453E45C, &[75]),
    (141, 0x7BDC183A, &[79, 50]),
    (143, 0x42F42CEB, &[50]),
    (155, 0x5068EA48, &[49]),
];

#[test]
fn field_wander_draws_track_retail_over_the_free_walk() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let (_, library, extracted) = resolve_dirs();
    let (Some(library), Some(extracted)) = (library, extracted) else {
        eprintln!("[skip] save library or extracted disc not found");
        return;
    };
    let manifest =
        psx_ram_image::ScenarioManifest::from_path(repo().join("scripts/scenarios.toml"))
            .expect("scenario manifest");
    let scus = std::fs::read(extracted.join("SCUS_942.54")).expect("SCUS");
    let entries = enumerate_corpus(&manifest, &library);
    let Some(entry) = entries.iter().find(|e| e.fingerprint == STATE) else {
        eprintln!("[skip] {STATE}: no library backup");
        return;
    };
    let retail = read_retail(entry, &scus).expect("read retail state");
    assert_eq!(retail.rand_seed, Some(CAPTURE_SEED), "capture seed");
    // Expected draws by engine tick.
    let expected: BTreeMap<u64, (u32, &[u16])> = RETAIL_DRAWS
        .iter()
        .map(|&(row, seed, flats)| (row + ROW_TO_TICK, (seed, flats)))
        .collect();
    run_engine_then(&extracted, &retail, &mut |s| {
        s.host.world.npcs.animate = true;
        seat_ambient_streams(s, &retail);
        let w = &s.host.world;
        // Slot to flat: the seat wrote the retail flat of each ambient channel.
        let slot_to_flat: BTreeMap<u8, u16> = w
            .npcs
            .ambient_order
            .iter()
            .copied()
            .zip(retail.ambient_streams.iter().map(|r| r.seat.flat))
            .collect();
        let draws = |w: &legaia_engine_core::world::World| -> BTreeMap<u8, u32> {
            w.npcs
                .ambient
                .iter()
                .map(|(k, c)| (*k, c.vm.rand_draws))
                .collect()
        };
        let mut seed = w.rng_state;
        assert_eq!(seed, CAPTURE_SEED, "the seat stands on the capture's seed");
        let mut counts = draws(w);
        for t in 1..=TICKS {
            s.host.world.input.set_pad(0);
            s.tick()?;
            s.field_vram_tick();
            let w = &s.host.world;
            let now = draws(w);
            let mut who: Vec<u16> = w
                .npcs
                .ambient_order
                .iter()
                .filter_map(|slot| {
                    let n = now.get(slot)? - counts.get(slot)?;
                    (n > 0).then_some(slot_to_flat[slot])
                })
                .collect();
            counts = now;
            let want = expected.get(&t);
            if w.rng_state != seed {
                seed = w.rng_state;
                let want = want.unwrap_or_else(|| {
                    panic!("engine drew at tick {t} ({seed:08x}) where retail's film drew nothing")
                });
                assert_eq!(seed, want.0, "seed word at tick {t}");
                who.sort_unstable();
                let mut want_flats: Vec<u16> = want.1.to_vec();
                want_flats.sort_unstable();
                assert_eq!(who, want_flats, "drawing actors at tick {t}");
            } else {
                assert!(
                    want.is_none(),
                    "retail's film drew at tick {t} where the engine drew nothing"
                );
            }
        }
        Ok(())
    })
    .expect("engine run");
}

/// The spawner's per-move draw on a scene entry: `FUN_8003A1E4` draws once
/// for each `1..N1` placement with a non-zero anim byte. Retail's counts are
/// from a breakpoint capture of `0x8003A410` over the two door hops.
#[test]
fn spawn_draws_match_the_capture() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let (_, _, extracted) = resolve_dirs();
    let Some(extracted) = extracted else {
        eprintln!("[skip] extracted disc not found");
        return;
    };
    let index = legaia_engine_scene::scene::ProtIndex::open_extracted(&extracted).expect("index");
    for (name, expected) in [("map01", 6usize), ("cave01", 7usize)] {
        let scene = legaia_engine_scene::scene::Scene::load(&index, name).expect("scene");
        let man = scene
            .field_man_payload(&index)
            .expect("MAN readable")
            .expect("MAN");
        let file = legaia_scene_formats::man_section::parse(&man).expect("parse");
        let draws = file
            .actor_placements(&man)
            .iter()
            .filter(|p| p.anim_id > 0)
            .count();
        assert_eq!(draws, expected, "{name}: placements with a move");
    }
}

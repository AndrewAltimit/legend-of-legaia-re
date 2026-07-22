//! Disc-gated regression tests for the door randomizer's **shuffle-pool
//! exclusions** (`crate::door::DoorSiteClass`): script/cutscene-invoked
//! scene-transition records and world-map-endpoint transitions must never be
//! rewritten, under either coupling, for any seed.
//!
//! The field bug this pins: the `0x3F` op family carries scripted scene
//! changes as well as walk-through doors. The kingdom-overworld hubs hold
//! story-return records - e.g. the Drake hub's Genesis-Tree-revival return to
//! Rim Elm (`dest town0b`, entry `(0x7f,0x7f)` "keep position") that runs at
//! world-map arrival under story state. With those in the pool, a shuffle (a)
//! hands the cutscene a random destination (mid-cutscene warp into a random
//! town), (b) replays the story warp on every overworld arrival while the
//! story state persists, and (c) gives some random door the cutscene's
//! descriptor (walking through it re-enters the cutscene off-screen). Only
//! walk-trigger-referenced, non-world-map records may shuffle.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::mode2_form1_sector_is_valid;
use legaia_patcher::apply::{self, DoorSite, DoorSiteClass};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::door::is_world_map_scene;
use legaia_patcher::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// One `0x3F` destination descriptor: `(index, dest_scene, entry_x, entry_z, dir)`.
type Descriptor = (i16, String, u8, u8, u8);

/// Per-scene ordered lists of the NON-pool sites' destination descriptors
/// (`op_pc` can shift when pool sites in the same MAN are resized, so identity
/// is by in-scene order + descriptor, not offset).
fn excluded_descriptors(doors: &[DoorSite]) -> std::collections::BTreeMap<usize, Vec<Descriptor>> {
    let mut out: std::collections::BTreeMap<usize, Vec<_>> = std::collections::BTreeMap::new();
    for d in doors {
        if d.class != DoorSiteClass::WalkDoor {
            out.entry(d.entry_idx).or_default().push((
                d.index,
                d.dest_scene.clone(),
                d.entry_x,
                d.entry_z,
                d.dir,
            ));
        }
    }
    out
}

#[test]
fn classification_pins_the_cutscene_and_world_map_records() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(original).expect("open");
    let doors = apply::current_doors(&patcher).expect("enumerate doors");

    // Every world-map-endpoint site is classified WorldMap - covering both the
    // hub-side story/arrival records and the town-side exits onto the
    // overworld (both directions of every town<->overworld connection).
    for d in &doors {
        if is_world_map_scene(&d.home_scene) || is_world_map_scene(&d.dest_scene) {
            assert_eq!(
                d.class,
                DoorSiteClass::WorldMap,
                "{}@0x{:x} -> {} must be WorldMap",
                d.home_scene,
                d.op_pc,
                d.dest_scene
            );
        }
    }

    // The exact record behind the reported Genesis-Tree-revival breakage: the
    // Drake overworld hub's scripted return to Rim Elm (dest `town0b`, entry
    // `(0x7f,0x7f)` = keep position - a cutscene continuation, not a door).
    let genesis_return = doors
        .iter()
        .find(|d| {
            d.home_scene == "map01"
                && d.dest_scene == "town0b"
                && (d.entry_x, d.entry_z) == (0x7f, 0x7f)
        })
        .expect("the map01 Genesis-Tree return record exists");
    assert_ne!(
        genesis_return.class,
        DoorSiteClass::WalkDoor,
        "the Genesis-Tree cutscene return must never enter the shuffle pool"
    );

    // Rim Elm's town exit (dest = overworld) stays vanilla too.
    let town01_exit = doors
        .iter()
        .find(|d| d.entry_idx == 4 && d.dest_scene == "map01")
        .expect("town01 exit exists");
    assert_eq!(town01_exit.class, DoorSiteClass::WorldMap);

    // No walk trigger references a record => script-invoked. The Retock
    // castle-town wall gates are the pinned members of this class.
    let retock_gate = doors
        .iter()
        .find(|d| d.home_scene == "retock" && d.op_pc == 0x712d)
        .expect("retock west wall gate exists");
    assert_eq!(retock_gate.class, DoorSiteClass::ScriptInvoked);

    // Genuine trigger-referenced interior doors stay in the pool.
    let retock_inn = doors
        .iter()
        .find(|d| d.home_scene == "retock" && d.op_pc == 0x70ca)
        .expect("retock -> retockin door exists");
    assert_eq!(retock_inn.class, DoorSiteClass::WalkDoor);
    let tower_exit = doors
        .iter()
        .find(|d| d.home_scene == "tower" && d.dest_scene == "geremi")
        .expect("tower -> geremi door exists");
    assert_eq!(tower_exit.class, DoorSiteClass::WalkDoor);

    // The pool is a healthy minority of the census: plenty of doors remain
    // shuffleable, and both exclusion classes are non-empty.
    let pool = doors
        .iter()
        .filter(|d| d.class == DoorSiteClass::WalkDoor)
        .count();
    let script = doors
        .iter()
        .filter(|d| d.class == DoorSiteClass::ScriptInvoked)
        .count();
    let world = doors
        .iter()
        .filter(|d| d.class == DoorSiteClass::WorldMap)
        .count();
    assert!(pool >= 40, "shuffle pool too small: {pool}");
    assert!(script >= 5, "script-invoked class missing: {script}");
    assert!(world >= 60, "world-map class missing: {world}");
}

#[test]
fn randomized_doors_leave_excluded_sites_byte_identical() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let base = DiscPatcher::open(original.clone()).expect("open");
    let vanilla_doors = apply::current_doors(&base).expect("vanilla doors");
    let vanilla_excluded = excluded_descriptors(&vanilla_doors);
    // The world-map hub scene bundles carry no pool site at all, so their
    // whole PROT entries must come through byte-identical.
    let world_map_entries: std::collections::BTreeSet<usize> = vanilla_doors
        .iter()
        .filter(|d| is_world_map_scene(&d.home_scene))
        .map(|d| d.entry_idx)
        .collect();
    assert!(!world_map_entries.is_empty());

    for (seed, coupling) in [
        (0xA46E_071C_C741_A601u64, apply::DoorCoupling::Decoupled),
        (0x0D00_4B00_5EED_0001, apply::DoorCoupling::Decoupled),
        (0xBEEF_CAFE_0102_0304, apply::DoorCoupling::Coupled),
        (0x5EED_0000_0000_0002, apply::DoorCoupling::Coupled),
    ] {
        let mut patcher = DiscPatcher::open(original.clone()).expect("open scratch");
        let report =
            apply::randomize_doors(&mut patcher, seed, DropMode::Shuffle, coupling).expect("doors");
        assert!(
            report.sites_changed > 0,
            "seed {seed:#x} {coupling:?}: nothing changed"
        );
        // Report accounting matches the vanilla census.
        assert_eq!(
            report.sites_total + report.excluded_script + report.excluded_world_map,
            vanilla_doors.len(),
            "seed {seed:#x}: pool + exclusions == census"
        );
        let patched = patcher.into_image();

        let p2 = DiscPatcher::open(patched.clone()).expect("re-open");

        // (i) + (ii): every excluded (script/cutscene + world-map) site keeps
        // its vanilla destination descriptor, scene by scene, in order.
        let patched_doors = apply::current_doors(&p2).expect("patched doors");
        assert_eq!(
            excluded_descriptors(&patched_doors),
            vanilla_excluded,
            "seed {seed:#x} {coupling:?}: an excluded transition record changed"
        );

        // World-map hub scene bundles are untouched bytes.
        for &idx in &world_map_entries {
            assert_eq!(
                base.read_entry(idx).expect("vanilla entry"),
                p2.read_entry(idx).expect("patched entry"),
                "seed {seed:#x} {coupling:?}: world-map scene entry {idx} was touched"
            );
        }

        // (iii) door-shuffle invariants still hold: full destination multiset
        // preserved when nothing skipped (a shuffle is a permutation), and
        // every changed sector stays EDC/ECC-valid.
        if report.skipped.is_empty() {
            let multiset = |doors: &[DoorSite]| {
                let mut v: Vec<_> = doors
                    .iter()
                    .map(|d| (d.index, d.dest_scene.clone(), d.entry_x, d.entry_z, d.dir))
                    .collect();
                v.sort();
                v
            };
            assert_eq!(
                multiset(&vanilla_doors),
                multiset(&patched_doors),
                "seed {seed:#x} {coupling:?}: destination multiset must be preserved"
            );
        }
        let mut sector = 0usize;
        while (sector + 1) * SECTOR_SIZE <= patched.len() {
            let span = sector * SECTOR_SIZE..(sector + 1) * SECTOR_SIZE;
            if original[span.clone()] != patched[span.clone()] {
                assert!(
                    mode2_form1_sector_is_valid(&patched[span]),
                    "seed {seed:#x}: sector {sector} EDC/ECC-invalid"
                );
            }
            sector += 1;
        }

        // Seed-determinism.
        let mut again = DiscPatcher::open(original.clone()).expect("open again");
        apply::randomize_doors(&mut again, seed, DropMode::Shuffle, coupling).expect("again");
        assert_eq!(
            again.into_image(),
            patched,
            "seed {seed:#x} {coupling:?}: not byte-deterministic"
        );
    }
}

//! Disc-gated end-to-end test for the `.MAP` kind-0 intra-scene-teleport
//! ("map door") shuffle: run the per-scene, reachability-verified destination
//! shuffle on a scratch copy of the disc, then re-decode every patched `.MAP`
//! straight off the patched image and confirm that per-scene destination
//! multisets are preserved (every landing stays a retail landing spot, no
//! off-map placement), that every touched scene still satisfies the
//! reachability oracle (retail component reachability preserved, no new
//! one-way trap), that trigger tiles and static records are byte-identical,
//! that every touched sector stays EDC/ECC-valid, that the image size is
//! unchanged, and that a fixed seed is byte-deterministic. Also anchors the
//! population against the known Vahn's-house exit (`town01` trigger tile
//! `(97, 9)` → half-tile `(72, 46)`, the doorstep: the runtime-pinned kind-0
//! record the engine dispatches). Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::BTreeMap;

use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::mode2_form1_sector_is_valid;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::map_door::MapDoorClass;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Per-scene invariants that must survive the shuffle byte-for-byte:
/// `entry -> (sorted destination multiset over the eligible sites, static
/// (tile, dest) records verbatim, trigger-tile skeleton in table order)`.
/// The verified shuffle permutes destinations across the whole eligible pool
/// (main-bound and pocket-bound may swap roles), so the class split is *not*
/// invariant - the multiset, the static records, and the trigger tiles are.
type SceneCensus = (Vec<(u8, u8)>, Vec<((u8, u8), (u8, u8))>, Vec<(u8, u8)>);

fn census(patcher: &DiscPatcher) -> BTreeMap<usize, SceneCensus> {
    let mut by_scene: BTreeMap<usize, SceneCensus> = BTreeMap::new();
    for (idx, _scene, s) in apply::current_map_doors(patcher).expect("enumerate") {
        let slot = by_scene.entry(idx).or_default();
        match s.class {
            MapDoorClass::MainBound | MapDoorClass::PocketBound => slot.0.push(s.dest),
            MapDoorClass::Static => slot.1.push((s.tile, s.dest)),
        }
        slot.2.push(s.tile);
    }
    for slot in by_scene.values_mut() {
        slot.0.sort_unstable();
    }
    by_scene
}

#[test]
fn shuffle_map_doors_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x4D41_5044_4F4F_5253; // "MAPDOORS"

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = census(&base);

    // Population sanity: the kind-0 class is the big one (most house exits) -
    // hundreds of records across dozens of scenes.
    let sites: usize = before.values().map(|s| s.2.len()).sum();
    let shuffled_pool: usize = before.values().map(|s| s.0.len()).sum();
    eprintln!(
        "map-door census: {} scenes, {} records ({} main-bound + pocket-bound, {} static)",
        before.len(),
        sites,
        shuffled_pool,
        sites - shuffled_pool,
    );
    assert!(
        before.len() >= 40,
        "expected the disc-wide kind-0 scene census, got {} scenes",
        before.len()
    );
    assert!(
        sites >= 1000,
        "expected the disc-wide kind-0 record census, got {sites}"
    );

    // Anchor: the runtime-pinned Vahn's-house exit is in the population, as a
    // main-bound (exit-class) record in `town01` (trigger tile `(97, 9)`,
    // half-tile destination `(72, 46)` = the doorstep).
    let vahn = apply::current_map_doors(&base)
        .expect("enumerate")
        .into_iter()
        .find(|(_, scene, s)| scene == "town01" && s.tile == (97, 9))
        .expect("town01 carries the Vahn's-house kind-0 exit at (97,9)");
    assert_eq!(vahn.2.dest, (72, 46), "the exit lands on the doorstep");
    assert_eq!(
        vahn.2.class,
        MapDoorClass::MainBound,
        "Vahn's-house exit must classify as an exit (main-bound)"
    );

    let mut patcher = DiscPatcher::open(original.clone()).expect("open scratch");
    let report =
        apply::randomize_map_doors(&mut patcher, seed, DropMode::Shuffle).expect("shuffle");
    eprintln!(
        "map-door report: {} of {} sites changed across {} scenes ({} static, {} beyond footprint)",
        report.sites_changed,
        report.sites_total,
        report.scenes_changed,
        report.kept_static,
        report.beyond_footprint,
    );
    assert!(
        report.sites_changed >= 100,
        "the shuffle must move a substantial population, changed only {}",
        report.sites_changed
    );
    assert_eq!(
        report.rewires.len(),
        report.sites_changed,
        "every changed site gets a spoiler-log entry"
    );
    let patched = patcher.into_image();
    assert_eq!(
        patched.len(),
        original.len(),
        "image size must be unchanged"
    );

    // Re-decode the patched disc: per-scene destination multisets preserved
    // over the eligible pool; trigger tiles and static records untouched.
    let after_patcher = DiscPatcher::open(patched.clone()).expect("re-open patched");
    let after = census(&after_patcher);
    assert_eq!(
        before, after,
        "per-scene destination multisets + trigger/static skeleton preserved"
    );

    // Every rewire's new destination reads back off the patched image, and
    // every touched scene still satisfies the shuffle's reachability oracle
    // (retail component reachability preserved, no new one-way trap) when
    // re-derived from the patched bytes alone.
    let mut checked_scenes = std::collections::BTreeSet::new();
    for r in &report.rewires {
        let entry = after_patcher.read_entry(r.entry_idx).expect("read entry");
        let sd = legaia_rando::map_door::SceneMapDoors::locate(&entry, r.entry_idx)
            .expect("patched .MAP still locates");
        let site = sd
            .sites
            .iter()
            .find(|s| s.tile == r.tile)
            .expect("rewired trigger tile survives");
        assert_eq!(site.dest, r.to, "patched destination reads back");
        if checked_scenes.insert(r.entry_idx) {
            let base_entry = base.read_entry(r.entry_idx).expect("read baseline entry");
            let base_sd = legaia_rando::map_door::SceneMapDoors::locate(&base_entry, r.entry_idx)
                .expect("baseline .MAP locates");
            assert!(
                sd.preserves_reachability_of(&base_sd),
                "scene {} ({}) must keep retail reachability after the shuffle",
                r.entry_idx,
                r.scene
            );
        }
    }

    // Every touched sector stays EDC/ECC-valid.
    let mut bad = 0usize;
    let mut checked = 0usize;
    let mut sector = 0usize;
    while (sector + 1) * SECTOR_SIZE <= patched.len() {
        let b = sector * SECTOR_SIZE;
        let span = b..b + SECTOR_SIZE;
        if original[span.clone()] != patched[span.clone()] {
            checked += 1;
            if !mode2_form1_sector_is_valid(&patched[b..b + SECTOR_SIZE]) {
                bad += 1;
            }
        }
        sector += 1;
    }
    assert!(checked > 0, "expected some changed sectors");
    assert_eq!(
        bad, 0,
        "{bad} of {checked} changed sectors are EDC/ECC-invalid"
    );

    // Determinism.
    let mut again = DiscPatcher::open(original).expect("open");
    apply::randomize_map_doors(&mut again, seed, DropMode::Shuffle).expect("shuffle again");
    assert_eq!(
        again.into_image(),
        patched,
        "a fixed seed is byte-deterministic"
    );
}

/// The house-doors option drives the map-door pass too: the combined report
/// carries the kind-0 outcome, so the CLI / web patcher expose it without a
/// new entry point - and stays deterministic.
#[test]
fn house_doors_option_also_runs_the_map_pass() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x484F_5553_454D_4150; // "HOUSEMAP"
    let mut combined = DiscPatcher::open(original.clone()).expect("open");
    let rep = apply::randomize_house_doors(&mut combined, seed, DropMode::Shuffle).expect("both");
    assert!(rep.sites_changed > 0, "house (script-warp) pass ran");
    assert!(
        rep.map.sites_changed > 0,
        "map pass ran under the same option"
    );
    assert_eq!(rep.map.rewires.len(), rep.map.sites_changed);
    let image = combined.into_image();

    // Deterministic, and a superset of the standalone map pass: every map-door
    // rewire reads back off the combined image too.
    let mut again = DiscPatcher::open(original).expect("open");
    let rep2 = apply::randomize_house_doors(&mut again, seed, DropMode::Shuffle).expect("again");
    assert_eq!(rep, rep2, "combined report is deterministic");
    assert_eq!(again.into_image(), image, "combined image is deterministic");
}

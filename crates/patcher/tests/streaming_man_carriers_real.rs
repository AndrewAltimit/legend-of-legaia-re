//! Disc-gated: scenes whose MAN is a **streaming chunk** rather than a
//! `scene_asset_table` bundle descriptor are reached by the MAN-based
//! randomizer passes - encounters and chests.
//!
//! The v12-family dungeons carry their MAN - or a story-state variant of it -
//! as a raw type-3 chunk of a `DATA_FIELD` streaming entry. `Mt. Rikuroa` has
//! **no** bundle MAN anywhere in its CDNAME block, so a sweep built only on
//! [`SceneEncounters::locate`] never sees its formations and the dungeon keeps
//! vanilla enemies at every pool width. These tests pin both halves: the
//! structural claim about where the MAN lives, and the behavioural one that a
//! kingdom shuffle actually rewrites it, plus the same pair for chest loot.
//!
//! Every test skips and passes without `LEGAIA_DISC_BIN`.

use legaia_patcher::apply::{self, EncounterScope};
use legaia_patcher::chest::SceneChests;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::drops::DropMode;
use legaia_patcher::encounter::SceneEncounters;
use legaia_prot::cdname::{self, RAW_TOC_INDEX_OFFSET};

const SEED: u64 = 0x5EED_1C0D_E5A1_7F03;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Extraction-index range of a CDNAME scene block.
fn block_range(patcher: &DiscPatcher, name: &str) -> Option<(usize, usize)> {
    let map = patcher.cdname()?;
    let (raw_start, raw_end) = cdname::block_range_for_name(&map, name)?;
    Some((
        raw_start.checked_sub(RAW_TOC_INDEX_OFFSET)? as usize,
        raw_end.checked_sub(RAW_TOC_INDEX_OFFSET)? as usize,
    ))
}

/// Every random-encounter monster id in a scene block, over **both** MAN
/// carriers, in entry order.
fn block_random_ids(patcher: &DiscPatcher, name: &str) -> Vec<u8> {
    let Some((lo, hi)) = block_range(patcher, name) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for idx in lo..hi {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        if let Some(s) = SceneEncounters::locate(&entry, idx) {
            out.extend(s.random_slot_ids());
        }
        for s in SceneEncounters::locate_streaming_mans(&entry, idx) {
            out.extend(s.random_slot_ids());
        }
    }
    out
}

/// The structural finding, stated as an assertion so it cannot rot: Mt. Rikuroa
/// has no bundle MAN, and its encounters live in a streaming chunk.
#[test]
fn rikuroa_carries_its_encounters_in_a_streaming_chunk_not_a_bundle() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(original).expect("open");
    let (lo, hi) = block_range(&patcher, "rikuroa").expect("rikuroa in CDNAME");

    let mut bundle_carriers = 0usize;
    let mut streaming_carriers = 0usize;
    let mut streaming_random_ids = 0usize;
    for idx in lo..hi {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        if SceneEncounters::locate(&entry, idx).is_some() {
            bundle_carriers += 1;
        }
        for s in SceneEncounters::locate_streaming_mans(&entry, idx) {
            streaming_carriers += 1;
            streaming_random_ids += s.random_slot_ids().len();
        }
    }

    assert_eq!(
        bundle_carriers, 0,
        "rikuroa [{lo}..{hi}) unexpectedly has a scene_asset_table MAN - if this \
         fires, the bundle-only sweep was not what skipped it"
    );
    assert!(
        streaming_carriers > 0,
        "rikuroa [{lo}..{hi}) has no streaming-chunk MAN either, so the scene \
         has no encounter carrier at all"
    );
    assert!(
        streaming_random_ids > 0,
        "rikuroa's streaming MAN carries no random-encounter slots, so there \
         would be nothing for the randomizer to rewrite"
    );
}

/// The user-visible symptom: a `balanced`-preset run (kingdom-scope shuffle)
/// must change Mt. Rikuroa's enemies.
#[test]
fn kingdom_shuffle_rewrites_rikuroa_encounters() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = block_random_ids(&base, "rikuroa");
    assert!(
        !before.is_empty(),
        "no baseline random ids for rikuroa - the test would pass vacuously"
    );
    // Control: a bundle-MAN scene that always worked, so a failure here means
    // the run itself did nothing rather than that the streaming path is broken.
    // `map01` and not `keikoku` - the Ravine authors one rate-0 region group and
    // has no random formations at all, so it would be a control that can never
    // move. See `docs/formats/encounter.md` on flag-gated region groups.
    let control_before = block_random_ids(&base, "map01");

    let mut patcher = DiscPatcher::open(original).expect("open");
    apply::randomize_encounters_scoped(
        &mut patcher,
        SEED,
        DropMode::Shuffle,
        EncounterScope::Kingdom,
        &[],
    )
    .expect("kingdom shuffle");

    let after = block_random_ids(&patcher, "rikuroa");
    let control_after = block_random_ids(&patcher, "map01");

    assert_eq!(
        before.len(),
        after.len(),
        "the rewrite must not change how many random slots rikuroa has"
    );
    assert_ne!(
        before, after,
        "rikuroa's random encounters are unchanged by a kingdom shuffle"
    );
    assert_ne!(
        control_before, control_after,
        "the control scene did not change either - this run randomized nothing"
    );
}

/// A streaming-chunk rewrite is same-size by construction, and must leave the
/// chunk parseable. Guards the write-back rather than the choice of ids.
#[test]
fn streaming_man_rewrite_is_same_size_and_still_parses() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let base = DiscPatcher::open(original).expect("open");

    // Sizes of every streaming MAN payload before the patch.
    let sizes = |p: &DiscPatcher| -> Vec<(usize, usize, usize)> {
        (0..p.entry_count())
            .filter_map(|i| p.read_entry(i).ok().map(|e| (i, e)))
            .flat_map(|(i, e)| {
                SceneEncounters::locate_streaming_mans(&e, i)
                    .into_iter()
                    .map(move |s| (i, s.man_offset, s.decoded.len()))
            })
            .collect()
    };
    let before = sizes(&base);
    assert!(
        !before.is_empty(),
        "no streaming MAN carriers found at all - the sweep is vacuous"
    );

    apply::randomize_encounters_scoped(
        &mut patcher,
        SEED,
        DropMode::Shuffle,
        EncounterScope::Kingdom,
        &[],
    )
    .expect("kingdom shuffle");

    // Every carrier still parses, at the same offset, at the same length.
    assert_eq!(
        before,
        sizes(&patcher),
        "a streaming MAN changed offset or length, or stopped parsing"
    );
}

/// Chest sites live in the same two carriers, and Mt. Rikuroa's are all in the
/// streaming one.
#[test]
fn rikuroa_chest_sites_are_only_reachable_through_the_streaming_carrier() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(original).expect("open");
    let (lo, hi) = block_range(&patcher, "rikuroa").expect("rikuroa in CDNAME");

    let (mut bundle_sites, mut streaming_sites) = (0usize, 0usize);
    for idx in lo..hi {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        if let Some(sc) = SceneChests::locate(&entry, idx) {
            bundle_sites += sc.sites.len();
        }
        for sc in SceneChests::locate_streaming_mans(&entry, idx) {
            streaming_sites += sc.sites.len();
        }
    }
    assert_eq!(
        bundle_sites, 0,
        "rikuroa has bundle chest sites - the bundle-only sweep was not what missed them"
    );
    assert!(
        streaming_sites > 0,
        "rikuroa has no chest sites in either carrier, so there is nothing to shuffle"
    );
}

/// A chest shuffle must reach Mt. Rikuroa's loot, and must not resize the MAN.
#[test]
fn chest_shuffle_rewrites_rikuroa_loot_without_resizing() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let chest_items = |p: &DiscPatcher, name: &str| -> Vec<u8> {
        let Some((lo, hi)) = block_range(p, name) else {
            return Vec::new();
        };
        let mut v = Vec::new();
        for idx in lo..hi {
            let Ok(entry) = p.read_entry(idx) else {
                continue;
            };
            if let Some(sc) = SceneChests::locate(&entry, idx) {
                v.extend(sc.current_items());
            }
            for sc in SceneChests::locate_streaming_mans(&entry, idx) {
                v.extend(sc.current_items());
            }
        }
        v
    };
    let sizes = |p: &DiscPatcher| -> Vec<(usize, usize, usize)> {
        (0..p.entry_count())
            .filter_map(|i| p.read_entry(i).ok().map(|e| (i, e)))
            .flat_map(|(i, e)| {
                SceneChests::locate_streaming_mans(&e, i)
                    .into_iter()
                    .map(move |s| (i, s.man_offset, s.decoded.len()))
            })
            .collect()
    };

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = chest_items(&base, "rikuroa");
    let before_sizes = sizes(&base);
    assert!(
        !before.is_empty(),
        "no baseline chest items for rikuroa - the test would pass vacuously"
    );

    let mut patcher = DiscPatcher::open(original).expect("open");
    // Shuffle redistributes the chests' own items, so the pool is unused; no
    // item is pinned static.
    let report = apply::randomize_chests(
        &mut patcher,
        &[],
        SEED,
        DropMode::Shuffle,
        &std::collections::BTreeSet::new(),
    )
    .expect("chest shuffle");
    assert!(
        report.items_changed > 0,
        "the chest shuffle rewrote nothing"
    );

    let after = chest_items(&patcher, "rikuroa");
    assert_eq!(
        before.len(),
        after.len(),
        "the rewrite changed how many chest sites rikuroa has"
    );
    assert_ne!(
        before, after,
        "rikuroa's chest loot is unchanged by a shuffle"
    );
    assert_eq!(
        before_sizes,
        sizes(&patcher),
        "a streaming MAN changed offset or length under the chest rewrite"
    );
}

//! Disc-gated end-to-end tests for the **scoped** encounter randomizer
//! (`--encounter-scope kingdom|world`). These exercise the two new behaviours
//! the user asked for, on a scratch copy of the real disc:
//!
//! - **`world`** ("across regions"): late-game monsters can appear at the start
//!   - a Karisto/Sebucus monster shows up in a Drake scene.
//! - **`kingdom`** ("within a region"): monsters are reshuffled across a whole
//!   kingdom (Drake / Sebucus / Karisto) but never cross a kingdom boundary.
//!
//! Each check re-decodes the patched scene MANs straight off the patched image,
//! so it validates the full decompress → mutate → recompress → write-back chain,
//! plus the kingdom partition derived from the disc's own `CDNAME.TXT`. All
//! tests skip + pass without `LEGAIA_DISC_BIN`.

use std::collections::BTreeMap;

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use legaia_prot::cdname::{self, RAW_TOC_INDEX_OFFSET};
use legaia_rando::apply::{self, EncounterScope};
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::encounter::SceneEncounters;
use legaia_rando::kingdom::{Kingdom, KingdomMap};

const SEED: u64 = 0x0BAD_F00D_DEAD_BEEF;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Every PROT entry index that carries a decodable encounter section.
fn scene_indices(patcher: &DiscPatcher) -> Vec<usize> {
    (0..patcher.entry_count())
        .filter(|&i| {
            patcher
                .read_entry(i)
                .ok()
                .and_then(|e| SceneEncounters::locate(&e, i))
                .is_some()
        })
        .collect()
}

/// The monster ids currently in a scene's **random** formations (boss/scripted
/// rows excluded), read live off `patcher`.
fn scene_random_ids(patcher: &DiscPatcher, idx: usize) -> Vec<u8> {
    patcher
        .read_entry(idx)
        .ok()
        .and_then(|e| SceneEncounters::locate(&e, idx))
        .map(|s| s.random_slot_ids())
        .unwrap_or_default()
}

/// Build the per-kingdom original random-encounter pools (the union of every
/// member scene's random ids) from an unpatched disc.
fn kingdom_pools(patcher: &DiscPatcher, km: &KingdomMap) -> BTreeMap<&'static str, Vec<u8>> {
    let mut pools: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
    for idx in scene_indices(patcher) {
        let k = km.kingdom_for_extraction_index(idx).as_str();
        let pool = pools.entry(k).or_default();
        for id in scene_random_ids(patcher, idx) {
            if !pool.contains(&id) {
                pool.push(id);
            }
        }
    }
    for p in pools.values_mut() {
        p.sort_unstable();
    }
    pools
}

#[test]
fn kingdom_partition_matches_cdname_anchors() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(original).expect("open");
    let map = patcher.cdname().expect("disc carries CDNAME.TXT");
    let km = KingdomMap::from_cdname(&map).expect("map01/map02 anchors present");

    // Resolve a scene NAME to its kingdom via the disc's own CDNAME indices.
    let kingdom_of_name = |name: &str| -> Option<Kingdom> {
        let (raw_start, _) = cdname::block_range_for_name(&map, name)?;
        let ext = raw_start.checked_sub(RAW_TOC_INDEX_OFFSET)? as usize;
        Some(km.kingdom_for_extraction_index(ext))
    };

    // Spot-check the boundary on names a human recognises: the opening town is
    // Drake, the first block after map01 (garmel) is Sebucus, the first after
    // map02 (tower) is Karisto, and the post-map03 dungeons (doman) are Karisto.
    assert_eq!(kingdom_of_name("town01"), Some(Kingdom::Drake));
    assert_eq!(kingdom_of_name("map01"), Some(Kingdom::Drake));
    assert_eq!(kingdom_of_name("garmel"), Some(Kingdom::Sebucus));
    assert_eq!(kingdom_of_name("map02"), Some(Kingdom::Sebucus));
    assert_eq!(kingdom_of_name("tower"), Some(Kingdom::Karisto));
    assert_eq!(kingdom_of_name("map03"), Some(Kingdom::Karisto));
    assert_eq!(kingdom_of_name("doman"), Some(Kingdom::Karisto));

    // All three kingdoms must actually host random encounters (non-vacuous).
    let base = DiscPatcher::open(patcher.into_image()).expect("reopen");
    let pools = kingdom_pools(&base, &km);
    for k in Kingdom::all() {
        let n = pools.get(k.as_str()).map(Vec::len).unwrap_or(0);
        assert!(
            n > 0,
            "kingdom {} has no random-encounter monsters",
            k.as_str()
        );
    }
}

#[test]
fn world_random_mixes_monsters_across_kingdoms() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let base = DiscPatcher::open(original.clone()).expect("open");
    let map = base.cdname().expect("CDNAME");
    let km = KingdomMap::from_cdname(&map).expect("anchors");
    let pools = kingdom_pools(&base, &km);
    let drake_orig: Vec<u8> = pools.get("drake").cloned().unwrap_or_default();
    let world_pool: Vec<u8> = {
        let mut v: Vec<u8> = pools.values().flatten().copied().collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    // Monsters that do NOT belong to Drake originally - the "late-game" set.
    let foreign: Vec<u8> = world_pool
        .iter()
        .copied()
        .filter(|id| !drake_orig.contains(id))
        .collect();
    assert!(!foreign.is_empty(), "expected non-Drake monsters to exist");

    let mut patcher = DiscPatcher::open(original).expect("open");
    let report = apply::randomize_encounters_scoped(
        &mut patcher,
        SEED,
        DropMode::Random,
        EncounterScope::World,
        &[],
    )
    .expect("world random");
    assert!(report.scenes_changed > 0, "world random rewrote nothing");

    // Every reassigned id is somewhere in the world pool (it stays a real,
    // loadable monster), and at least one Drake scene now hosts a foreign
    // (non-Drake) monster - the user's "late-game monster at the start".
    let mut saw_foreign_in_drake = false;
    for idx in scene_indices(&base) {
        if report.skipped.contains(&idx) {
            continue;
        }
        let after = scene_random_ids(&patcher, idx);
        for &id in &after {
            assert!(
                world_pool.contains(&id),
                "scene {idx} id {id} outside world pool"
            );
        }
        if km.kingdom_for_extraction_index(idx) == Kingdom::Drake
            && after.iter().any(|id| foreign.contains(id))
        {
            saw_foreign_in_drake = true;
        }
    }
    assert!(
        saw_foreign_in_drake,
        "world scope must let a non-Drake monster appear in a Drake scene"
    );
    eprintln!(
        "world random: {} scenes rewritten, {} ids changed",
        report.scenes_changed, report.ids_changed
    );
}

#[test]
fn kingdom_random_confines_monsters_to_their_kingdom() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let base = DiscPatcher::open(original.clone()).expect("open");
    let map = base.cdname().expect("CDNAME");
    let km = KingdomMap::from_cdname(&map).expect("anchors");
    let pools = kingdom_pools(&base, &km);

    let mut patcher = DiscPatcher::open(original).expect("open");
    let report = apply::randomize_encounters_scoped(
        &mut patcher,
        SEED,
        DropMode::Random,
        EncounterScope::Kingdom,
        &[],
    )
    .expect("kingdom random");
    assert!(report.scenes_changed > 0, "kingdom random rewrote nothing");

    // Confinement: every reassigned id belongs to that scene's kingdom pool, so
    // no monster ever crosses a kingdom boundary. Non-vacuous: at least one
    // scene gains an id it did not originally host but its kingdom does (proving
    // the pool really is kingdom-wide, not scene-local).
    let mut widened = false;
    for idx in scene_indices(&base) {
        if report.skipped.contains(&idx) {
            continue;
        }
        let kpool = pools
            .get(km.kingdom_for_extraction_index(idx).as_str())
            .expect("kingdom pool");
        let orig_scene = scene_random_ids(&base, idx);
        let after = scene_random_ids(&patcher, idx);
        for &id in &after {
            assert!(
                kpool.contains(&id),
                "scene {idx} id {id} leaked outside its kingdom pool"
            );
        }
        if after.iter().any(|id| !orig_scene.contains(id)) {
            widened = true;
        }
    }
    assert!(
        widened,
        "kingdom scope should draw from a pool wider than a single scene"
    );
    eprintln!(
        "kingdom random: {} scenes rewritten, {} ids changed",
        report.scenes_changed, report.ids_changed
    );
}

#[test]
fn world_shuffle_preserves_global_multiset_and_bosses() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let base = DiscPatcher::open(original.clone()).expect("open");
    let indices = scene_indices(&base);

    // Global multiset of random-encounter ids, before.
    let before: Vec<u8> = {
        let mut v: Vec<u8> = indices
            .iter()
            .flat_map(|&i| scene_random_ids(&base, i))
            .collect();
        v.sort_unstable();
        v
    };

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::randomize_encounters_scoped(
        &mut patcher,
        SEED,
        DropMode::Shuffle,
        EncounterScope::World,
        &[],
    )
    .expect("world shuffle");
    assert!(report.scenes_changed > 0, "world shuffle rewrote nothing");

    // The global multiset is conserved (same monsters, redistributed), and at
    // least one scene actually moved monsters in from elsewhere.
    let after: Vec<u8> = {
        let mut v: Vec<u8> = indices
            .iter()
            .flat_map(|&i| scene_random_ids(&patcher, i))
            .collect();
        v.sort_unstable();
        v
    };
    assert_eq!(
        before, after,
        "world shuffle must conserve the global id multiset"
    );

    // Scripted/boss formations are byte-identical (never shuffled).
    let mut scripted_checked = 0usize;
    for &idx in &indices {
        let o = base
            .read_entry(idx)
            .ok()
            .and_then(|e| SceneEncounters::locate(&e, idx))
            .unwrap();
        let a = patcher
            .read_entry(idx)
            .ok()
            .and_then(|e| SceneEncounters::locate(&e, idx))
            .unwrap();
        for i in 0..o.formation_count() {
            if o.is_random_formation(i) {
                continue;
            }
            assert_eq!(
                o.formation_ids(i),
                a.formation_ids(i),
                "scene {idx} scripted formation {i} changed"
            );
            scripted_checked += 1;
        }
    }
    assert!(
        scripted_checked > 0,
        "no scripted formations checked (vacuous)"
    );

    // One patched scene's first PROT sector stays EDC/ECC-valid.
    let changed = indices
        .iter()
        .copied()
        .find(|i| !report.skipped.contains(i))
        .unwrap();
    let img = patcher.image();
    let (prot_lba, psize) = find_file_in_image(img, "PROT.DAT").unwrap();
    // Recover the entry's start LBA from a fresh parse of the TOC - patches are
    // same-size, so the LBA is unchanged from the original.
    let psectors = (psize as usize).div_ceil(USER_DATA_SIZE);
    let mut payload = Vec::with_capacity(psectors * USER_DATA_SIZE);
    for i in 0..psectors {
        let b = (prot_lba as usize + i) * SECTOR_SIZE + USER_DATA_OFFSET;
        payload.extend_from_slice(&img[b..b + USER_DATA_SIZE]);
    }
    payload.truncate(psize as usize);
    let archive = legaia_prot::archive::Archive::from_bytes(payload).unwrap();
    let disc_sector = prot_lba as usize + archive.entries[changed].start_lba as usize;
    let sb = disc_sector * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
        "patched scene {changed} first sector must be EDC/ECC-valid"
    );

    // Determinism: same seed -> byte-identical patched image.
    let mut p2 = DiscPatcher::open(original).expect("open");
    apply::randomize_encounters_scoped(
        &mut p2,
        SEED,
        DropMode::Shuffle,
        EncounterScope::World,
        &[],
    )
    .expect("world shuffle 2");
    assert!(
        p2.image() == patcher.image(),
        "world shuffle must be deterministic"
    );
    eprintln!("world shuffle: {} scenes rewritten", report.scenes_changed);
}

#[test]
fn kingdom_shuffle_preserves_per_kingdom_multiset() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let base = DiscPatcher::open(original.clone()).expect("open");
    let map = base.cdname().expect("CDNAME");
    let km = KingdomMap::from_cdname(&map).expect("anchors");
    let indices = scene_indices(&base);

    // Per-kingdom multiset of random ids, before.
    let multiset = |patcher: &DiscPatcher| -> BTreeMap<&'static str, Vec<u8>> {
        let mut m: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
        for &idx in &indices {
            let k = km.kingdom_for_extraction_index(idx).as_str();
            m.entry(k)
                .or_default()
                .extend(scene_random_ids(patcher, idx));
        }
        for v in m.values_mut() {
            v.sort_unstable();
        }
        m
    };
    let before = multiset(&base);

    let mut patcher = DiscPatcher::open(original).expect("open");
    let report = apply::randomize_encounters_scoped(
        &mut patcher,
        SEED,
        DropMode::Shuffle,
        EncounterScope::Kingdom,
        &[],
    )
    .expect("kingdom shuffle");
    assert!(report.scenes_changed > 0, "kingdom shuffle rewrote nothing");

    let after = multiset(&patcher);
    // Each kingdom keeps its own monster multiset (nothing crosses a boundary),
    // and the global multiset is therefore also conserved.
    for k in Kingdom::all() {
        assert_eq!(
            before.get(k.as_str()),
            after.get(k.as_str()),
            "kingdom {} multiset changed under a within-kingdom shuffle",
            k.as_str()
        );
    }
    eprintln!(
        "kingdom shuffle: {} scenes rewritten",
        report.scenes_changed
    );
}

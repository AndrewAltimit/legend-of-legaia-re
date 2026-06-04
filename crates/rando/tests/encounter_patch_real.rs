//! Disc-gated end-to-end test for the random-encounter randomizer: shuffle the
//! formation monster ids of every scene on a scratch copy of the disc, then
//! re-decode each patched scene MAN straight off the patched image and confirm
//! the edit is faithful — counts preserved, id multiset preserved (shuffle),
//! every id still within the scene's own pool, sectors EDC/ECC-valid, and a
//! fixed seed byte-deterministic. Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::encounter::SceneEncounters;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Per-scene snapshot: sorted id multiset + per-formation counts + pool.
fn snapshot(patcher: &DiscPatcher, idx: usize) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let entry = patcher.read_entry(idx).ok()?;
    // Presence guard (a scene with no decodable encounter section is skipped).
    SceneEncounters::locate(&entry, idx)?;
    // Re-derive counts + ids without exposing internals: parse again here.
    let table = legaia_asset::scene_asset_table::detect(&entry)?;
    let man = table.used().iter().find(|d| d.type_byte == 0x03).copied()?;
    let (decoded, _) =
        legaia_lzs::decompress_tracked(&entry[man.data_offset as usize..], man.size as usize)
            .ok()?;
    let mf = legaia_asset::man_section::parse(&decoded).ok()?;
    let body = mf.encounter_section_body(&decoded)?;
    let sec = legaia_asset::man_section::parse_encounter_section(body).ok()?;
    let base = mf.encounter_section().body_offset() + sec.formation_range.0;
    let mut ids = Vec::new();
    let mut counts = Vec::new();
    for i in 0..sec.formation_count as usize {
        let rec = base + i * sec.formation_stride as usize;
        let c = (decoded[rec + 3] as usize).min(4);
        counts.push(c as u8);
        ids.extend_from_slice(&decoded[rec + 4..rec + 4 + c]);
    }
    ids.sort_unstable();
    // The "pool" the membership check uses is the full set of ids the scene
    // loads (every formation, random + scripted) — a shuffled id must be one of
    // them. (`SceneEncounters::monster_pool` is now the narrower random-only
    // pool, so it isn't the right set for an all-formation membership check.)
    let mut pool = ids.clone();
    pool.dedup();
    Some((ids, counts, pool))
}

#[test]
fn shuffle_encounters_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x0BAD_F00D_DEAD_BEEF;

    // Snapshot the original scenes.
    let base = DiscPatcher::open(original.clone()).expect("open");
    let scene_indices: Vec<usize> = (0..base.entry_count())
        .filter(|&i| {
            base.read_entry(i)
                .ok()
                .and_then(|e| SceneEncounters::locate(&e, i))
                .is_some()
        })
        .collect();
    assert!(
        scene_indices.len() > 40,
        "expected many scene bundles, found {}",
        scene_indices.len()
    );
    let before: Vec<_> = scene_indices
        .iter()
        .map(|&i| (i, snapshot(&base, i)))
        .collect();

    // Apply the shuffle to a scratch copy.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report =
        apply::randomize_encounters(&mut patcher, seed, DropMode::Shuffle, &[]).expect("randomize");
    assert!(
        report.scenes_changed > 0,
        "should rewrite at least one scene"
    );

    // Re-decode every changed scene off the patched image.
    let mut verified = 0;
    for (idx, snap) in &before {
        if report.skipped.contains(idx) {
            continue;
        }
        let Some((before_ids, before_counts, before_pool)) = snap else {
            continue;
        };
        let Some((after_ids, after_counts, _after_pool)) = snapshot(&patcher, *idx) else {
            panic!("scene {idx} no longer decodes after patch");
        };
        // Counts unchanged; shuffle preserves the exact id multiset.
        assert_eq!(after_counts, *before_counts, "scene {idx} counts changed");
        assert_eq!(
            after_ids, *before_ids,
            "scene {idx} shuffle must preserve the id multiset"
        );
        // Every id is still one the scene loads (drawn from its own pool).
        for &id in &after_ids {
            assert!(
                before_pool.contains(&id),
                "scene {idx} id {id} not in original scene pool"
            );
        }
        verified += 1;
    }
    assert!(verified > 20, "verified too few scenes ({verified})");

    // A patched scene's PROT.DAT sectors stay EDC/ECC-valid.
    let changed_idx = before
        .iter()
        .map(|(i, _)| *i)
        .find(|i| !report.skipped.contains(i))
        .unwrap();
    let img = patcher.image();
    let (prot_lba, prot_size) = find_file_in_image(img, "PROT.DAT").unwrap();
    let psize = prot_size as usize;
    let psectors = psize.div_ceil(USER_DATA_SIZE);
    let mut payload = Vec::with_capacity(psectors * USER_DATA_SIZE);
    for i in 0..psectors {
        let b = (prot_lba as usize + i) * SECTOR_SIZE + USER_DATA_OFFSET;
        payload.extend_from_slice(&img[b..b + USER_DATA_SIZE]);
    }
    payload.truncate(psize);
    let archive = legaia_prot::archive::Archive::from_bytes(payload).unwrap();
    let start_lba = archive.entries[changed_idx].start_lba;
    let disc_sector = prot_lba as u64 + start_lba as u64;
    let sb = disc_sector as usize * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
        "patched scene {changed_idx} first sector must be EDC/ECC-valid"
    );

    // Determinism: same seed -> byte-identical patched image.
    let mut patcher2 = DiscPatcher::open(original.clone()).expect("open");
    let report2 = apply::randomize_encounters(&mut patcher2, seed, DropMode::Shuffle, &[])
        .expect("randomize");
    assert_eq!(report2.skipped, report.skipped);
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "encounters shuffle seed {seed:#x}: {} scenes rewritten, {} ids changed, {} verified, {} skipped",
        report.scenes_changed,
        report.ids_changed,
        verified,
        report.skipped.len()
    );
}

/// Scripted / boss formations (the ones no region range reaches) must be left
/// **byte-identical** by an encounter shuffle — randomizing them would replace a
/// boss (Tetsu, Cort, Songi, …). This compares every non-random formation's ids
/// before vs after a whole-disc shuffle, on real data, and confirms the
/// population of scripted formations is non-trivial (so it isn't vacuous) and
/// includes the Rim Elm Tetsu fight (formation id `0x4F`).
#[test]
fn scripted_formations_survive_the_shuffle() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x0BAD_F00D_DEAD_BEEF;

    let base = DiscPatcher::open(original.clone()).expect("open");
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    apply::randomize_encounters(&mut patcher, seed, DropMode::Shuffle, &[]).expect("randomize");

    let mut scripted_checked = 0usize;
    let mut saw_tetsu = false;
    for idx in 0..base.entry_count() {
        let Some(orig) = base
            .read_entry(idx)
            .ok()
            .and_then(|e| SceneEncounters::locate(&e, idx))
        else {
            continue;
        };
        let Some(after) = patcher
            .read_entry(idx)
            .ok()
            .and_then(|e| SceneEncounters::locate(&e, idx))
        else {
            continue;
        };
        for i in 0..orig.formation_count() {
            if orig.is_random_formation(i) {
                continue;
            }
            let before_ids = orig.formation_ids(i);
            assert_eq!(
                after.formation_ids(i),
                before_ids,
                "scene {idx} scripted formation {i} must be untouched by the shuffle"
            );
            scripted_checked += 1;
            if before_ids.contains(&0x4F) {
                saw_tetsu = true;
            }
        }
    }
    assert!(
        scripted_checked > 10,
        "expected many scripted formations across the corpus, saw {scripted_checked}"
    );
    assert!(
        saw_tetsu,
        "the Rim Elm Tetsu fight (formation id 0x4F) should be among the protected scripted formations"
    );
    eprintln!("scripted-formation protection: {scripted_checked} scripted formations unchanged");
}

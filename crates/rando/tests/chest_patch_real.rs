//! Disc-gated end-to-end test for the chest randomizer: shuffle every chest's
//! item id on a scratch copy, then re-decode each patched scene MAN off the disc
//! and confirm the edit is faithful — the give-item site offsets are unchanged,
//! the global chest-item multiset is preserved (shuffle), sectors stay
//! EDC/ECC-valid, and a fixed seed is byte-deterministic. Skips without
//! `LEGAIA_DISC_BIN`.

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use legaia_rando::apply;
use legaia_rando::chest::SceneChests;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// (scene idx, site offsets, current items) for every scene with chest sites.
fn snapshot(patcher: &DiscPatcher) -> Vec<(usize, Vec<usize>, Vec<u8>)> {
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        if let Some(sc) = SceneChests::locate(&entry, idx) {
            out.push((idx, sc.sites.clone(), sc.current_items()));
        }
    }
    out
}

#[test]
fn shuffle_chests_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0xC0FFEE_u64;

    let before = snapshot(&DiscPatcher::open(original.clone()).unwrap());
    let total_sites: usize = before.iter().map(|(_, s, _)| s.len()).sum();
    assert!(total_sites > 0, "expected chest give-item sites");

    let mut patcher = DiscPatcher::open(original.clone()).unwrap();
    let report =
        apply::randomize_chests(&mut patcher, &[], seed, DropMode::Shuffle).expect("randomize");
    assert_eq!(report.sites_total, total_sites);
    assert!(report.items_changed > 0);

    let after = snapshot(&patcher);

    // Same scenes, same site offsets (only operand bytes changed; widths intact).
    assert_eq!(before.len(), after.len(), "scene set changed");
    for ((bi, bsites, _), (ai, asites, _)) in before.iter().zip(&after) {
        assert_eq!(bi, ai, "scene order changed");
        assert_eq!(bsites, asites, "chest site offsets changed in scene {bi}");
    }

    // Global multiset of chest items preserved (minus skipped scenes).
    let skipped: std::collections::HashSet<usize> = report.skipped.iter().copied().collect();
    let mut mb: Vec<u8> = before
        .iter()
        .filter(|(i, _, _)| !skipped.contains(i))
        .flat_map(|(_, _, items)| items.clone())
        .collect();
    let mut ma: Vec<u8> = after
        .iter()
        .filter(|(i, _, _)| !skipped.contains(i))
        .flat_map(|(_, _, items)| items.clone())
        .collect();
    mb.sort_unstable();
    ma.sort_unstable();
    assert_eq!(mb, ma, "shuffle must preserve the chest-item multiset");

    // A patched scene's first PROT.DAT sector stays EDC/ECC-valid.
    let changed = after
        .iter()
        .map(|(i, _, _)| *i)
        .find(|i| !skipped.contains(i))
        .unwrap();
    let img = patcher.image();
    let (prot_lba, prot_size) = find_file_in_image(img, "PROT.DAT").unwrap();
    let psectors = (prot_size as usize).div_ceil(USER_DATA_SIZE);
    let mut payload = Vec::with_capacity(psectors * USER_DATA_SIZE);
    for i in 0..psectors {
        let b = (prot_lba as usize + i) * SECTOR_SIZE + USER_DATA_OFFSET;
        payload.extend_from_slice(&img[b..b + USER_DATA_SIZE]);
    }
    payload.truncate(prot_size as usize);
    let archive = legaia_prot::archive::Archive::from_bytes(payload).unwrap();
    let lba = archive.entries[changed].start_lba;
    let sb = (prot_lba as u64 + lba as u64) as usize * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
        "patched chest scene {changed} sector must be EDC/ECC-valid"
    );

    // Determinism.
    let mut p2 = DiscPatcher::open(original.clone()).unwrap();
    let r2 = apply::randomize_chests(&mut p2, &[], seed, DropMode::Shuffle).unwrap();
    assert_eq!(r2.skipped, report.skipped);
    assert!(
        p2.image() == patcher.image(),
        "same seed -> identical image"
    );

    eprintln!(
        "chests shuffle seed {seed:#x}: {} sites, {} changed, {} scenes, {} skipped",
        report.sites_total,
        report.items_changed,
        report.scenes_changed,
        report.skipped.len()
    );
}

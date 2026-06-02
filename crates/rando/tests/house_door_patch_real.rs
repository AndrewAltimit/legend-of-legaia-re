//! Disc-gated end-to-end test for the intra-town (house / interior) door
//! shuffle: per-scene multiset-preserving shuffle of the `0x23 MOVE_TO` target
//! tiles on a scratch copy of the disc, then re-decode every patched scene MAN
//! straight off the patched image and confirm — the per-scene MOVE_TO target
//! multiset is preserved (so every target stays a tile the scene uses, no
//! off-map placement), every patched MAN re-parses through disc → ISO → PROT →
//! LZS, every touched sector stays EDC/ECC-valid, the image size is unchanged,
//! and a fixed seed is byte-deterministic. Skips + passes without
//! `LEGAIA_DISC_BIN`.

use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::mode2_form1_sector_is_valid;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Per-scene sorted MOVE_TO target multiset: `(entry, sorted [(tx,tz)])`.
fn per_scene_targets(patcher: &DiscPatcher) -> Vec<(usize, Vec<(u8, u8)>)> {
    use std::collections::BTreeMap;
    let mut by_scene: BTreeMap<usize, Vec<(u8, u8)>> = BTreeMap::new();
    for (idx, tx, tz) in apply::current_house_doors(patcher).expect("enumerate house doors") {
        by_scene.entry(idx).or_default().push((tx, tz));
    }
    by_scene
        .into_iter()
        .map(|(k, mut v)| {
            v.sort_unstable();
            (k, v)
        })
        .collect()
}

#[test]
fn shuffle_house_doors_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x484F_5553_4544_4F4F; // "HOUSEDOO"

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = per_scene_targets(&base);
    assert!(
        before.len() >= 20,
        "expected many scenes, got {}",
        before.len()
    );

    let mut patcher = DiscPatcher::open(original.clone()).expect("open scratch");
    let report =
        apply::randomize_house_doors(&mut patcher, seed, DropMode::Shuffle).expect("shuffle");
    assert!(
        report.sites_changed > 50,
        "changed {}",
        report.sites_changed
    );
    let patched = patcher.into_image();
    assert_eq!(
        patched.len(),
        original.len(),
        "image size must be unchanged"
    );

    // Re-decode the patched disc and check the per-scene target multiset is
    // preserved everywhere (a per-scene shuffle keeps every target a valid
    // scene tile). Skipped scenes also keep their multiset (unchanged).
    let after_patcher = DiscPatcher::open(patched.clone()).expect("re-open patched");
    let after = per_scene_targets(&after_patcher);
    assert_eq!(
        before, after,
        "per-scene MOVE_TO target multiset is preserved across the shuffle"
    );

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
    apply::randomize_house_doors(&mut again, seed, DropMode::Shuffle).expect("shuffle again");
    assert_eq!(
        again.into_image(),
        patched,
        "a fixed seed is byte-deterministic"
    );
}

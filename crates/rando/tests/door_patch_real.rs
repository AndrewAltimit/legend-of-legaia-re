//! Disc-gated end-to-end test for the one-way (decoupled) door randomizer:
//! shuffle scene-transition destinations on a scratch copy of the disc, then
//! re-decode every patched scene MAN straight off the patched image and confirm
//! the edit is faithful — the destination multiset is preserved (shuffle is a
//! permutation), every patched MAN re-parses + re-walks cleanly through
//! disc → ISO → PROT → LZS, every touched sector stays EDC/ECC-valid, the image
//! size is unchanged, and a fixed seed is byte-deterministic. Skips + passes
//! without `LEGAIA_DISC_BIN`.

use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::mode2_form1_sector_is_valid;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Sorted multiset of every door's full destination descriptor.
fn dest_multiset(patcher: &DiscPatcher) -> Vec<(i16, String, u8, u8, u8)> {
    let mut v: Vec<_> = apply::current_doors(patcher)
        .expect("enumerate doors")
        .into_iter()
        .map(|d| (d.index, d.dest_scene, d.entry_x, d.entry_z, d.dir))
        .collect();
    v.sort();
    v
}

#[test]
fn shuffle_doors_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Seed chosen to fit every scene's rebuild within budget (0 skips), so the
    // strong "exact multiset preserved" invariant is exercised; the test also
    // handles the skip case for robustness against disc/seed variation.
    let seed = 0xA46E_071C_C741_A601;

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = dest_multiset(&base);
    assert!(
        before.len() >= 120,
        "expected many doors, got {}",
        before.len()
    );

    // Patch a scratch copy.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open scratch");
    let report = apply::randomize_doors(&mut patcher, seed, DropMode::Shuffle).expect("shuffle");
    assert!(
        report.sites_changed > 50,
        "shuffle changed too few sites: {}",
        report.sites_changed
    );
    let patched = patcher.into_image();
    assert_eq!(
        patched.len(),
        original.len(),
        "image size must be unchanged"
    );

    // Re-decode the patched disc from scratch (full pipeline) and compare the
    // destinations.
    let patched_patcher = DiscPatcher::open(patched.clone()).expect("re-open patched");
    let after = dest_multiset(&patched_patcher);
    assert_eq!(before.len(), after.len(), "door count is preserved");
    if report.skipped.is_empty() {
        // A clean shuffle is a permutation, so the destination multiset is
        // preserved exactly (every scene stays reachable as some door's target).
        assert_eq!(before, after, "shuffle preserves the destination multiset");
    } else {
        // With skipped scenes the multiset isn't a clean permutation, but every
        // patched destination must still be an original destination name (the
        // shuffle only ever moves existing descriptors — no garbage introduced).
        let orig_names: std::collections::BTreeSet<&str> =
            before.iter().map(|d| d.1.as_str()).collect();
        for d in &after {
            assert!(
                orig_names.contains(d.1.as_str()),
                "patched door names a non-existent scene {:?}",
                d.1
            );
        }
    }

    // Every touched 2352-byte sector must stay EDC/ECC-valid.
    let mut bad = 0usize;
    let mut checked = 0usize;
    let mut sector = 0usize;
    while (sector + 1) * SECTOR_SIZE <= patched.len() {
        let base = sector * SECTOR_SIZE;
        let span = base..base + SECTOR_SIZE;
        if original[span.clone()] != patched[span.clone()] {
            checked += 1;
            if !mode2_form1_sector_is_valid(&patched[base..base + SECTOR_SIZE]) {
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

    // Determinism: same seed reproduces the same image byte-for-byte.
    let mut again = DiscPatcher::open(original.clone()).expect("open");
    apply::randomize_doors(&mut again, seed, DropMode::Shuffle).expect("shuffle again");
    assert_eq!(
        again.into_image(),
        patched,
        "a fixed seed is byte-deterministic"
    );
}
